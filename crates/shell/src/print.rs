//! Texto de comandos reconstruído do AST, no formato do `print_cmd.c` do bash: `BASH_COMMAND`,
//! mensagens de job, `declare -f`, `type`.

use crate::ast::*;
use crate::builtins::{AssignArg, AssignedValue};

const INDENT: usize = 4;

fn redirect_text(r: &Redirect) -> String {
    let fd = match &r.fd {
        RedirFd::Default => String::new(),
        RedirFd::Num(n) => n.to_string(),
        RedirFd::Var(v) => format!("{{{v}}}"),
    };
    let target = match &r.target {
        RedirTarget::Word(w) => w.raw.to_string(),
        RedirTarget::ProcSub(p) => format!("{}({})", if p.write { '>' } else { '<' }, p.src),
        RedirTarget::HereDoc(h) => h.delimiter.clone(),
    };
    match r.op {
        RedirOp::Read => format!("{fd}< {target}"),
        RedirOp::Write => format!("{fd}> {target}"),
        RedirOp::Append => format!("{fd}>> {target}"),
        RedirOp::ReadWrite => format!("{fd}<> {target}"),
        RedirOp::Clobber => format!("{fd}>| {target}"),
        RedirOp::DupIn => format!("{fd}<&{target}"),
        RedirOp::DupOut => format!("{fd}>&{target}"),
        RedirOp::HereDoc => match &r.target {
            RedirTarget::HereDoc(h) => {
                let op = if h.strip_tabs { "<<-" } else { "<<" };
                let delim = if h.expand { h.delimiter.clone() } else { format!("'{}'", h.delimiter) };
                format!("{fd}{op} {delim}")
            }
            _ => format!("{fd}<< {target}"),
        },
        RedirOp::HereString => format!("{fd}<<< {target}"),
        RedirOp::OutErr { append } => format!("{}> {target}", if append { "&>" } else { "&" }).replace("&>>", "&>>"),
    }
}

fn heredoc_bodies(redirs: &[Redirect]) -> String {
    let mut out = String::new();
    for r in redirs {
        if let RedirTarget::HereDoc(h) = &r.target {
            out.push('\n');
            out.push_str(&String::from_utf8_lossy(&h.body));
            out.push_str(&h.delimiter);
        }
    }
    out
}

/// Comando simples numa linha (sem os corpos de here-doc).
pub fn simple_text(s: &Simple) -> String {
    let mut parts: Vec<String> = Vec::new();
    for a in &s.assigns {
        parts.push(a.raw.to_string());
    }
    for w in &s.words {
        parts.push(w.raw.to_string());
    }
    for r in &s.redirects {
        parts.push(redirect_text(r));
    }
    parts.join(" ")
}

pub fn command_text(c: &Command) -> String {
    let mut p = Printer::default();
    p.command(c, 0);
    p.out.trim_end().to_string()
}

pub fn and_or_text(ao: &AndOr) -> String {
    let mut p = Printer::default();
    p.and_or(ao, 0);
    p.out
}

/// Linha de xtrace de uma atribuição.
pub fn assign_trace(a: &AssignArg) -> Vec<u8> {
    let mut out = a.name.as_bytes().to_vec();
    if let Some(idx) = &a.index {
        out.push(b'[');
        out.extend_from_slice(idx);
        out.push(b']');
    }
    if a.append {
        out.push(b'+');
    }
    out.push(b'=');
    match &a.value {
        AssignedValue::Scalar(v) => out.extend(crate::quote::xtrace_word(v)),
        AssignedValue::Array(items) => {
            out.push(b'(');
            for (i, (k, _, v)) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                if let Some(k) = k {
                    out.push(b'[');
                    out.extend_from_slice(k);
                    out.extend_from_slice(b"]=");
                }
                out.extend(crate::quote::xtrace_word(v));
            }
            out.push(b')');
        }
    }
    out
}

/// `declare -f nome`: a função inteira.
pub fn function_text(f: &FunctionDef) -> String {
    let mut p = Printer::default();
    p.function(f, 0);
    p.out
}

#[derive(Default)]
struct Printer {
    out: String,
}

impl Printer {
    fn indent(&mut self, level: usize) {
        for _ in 0..level * INDENT {
            self.out.push(' ');
        }
    }

    fn function(&mut self, f: &FunctionDef, level: usize) {
        self.out.push_str(&f.name);
        self.out.push_str(" () \n");
        self.indent(level);
        self.compound_body(&f.body, level);
        for r in &f.redirects {
            self.out.push(' ');
            self.out.push_str(&redirect_text(r));
        }
    }

    /// Corpo de função: grupo `{ }` em várias linhas.
    fn compound_body(&mut self, c: &Compound, level: usize) {
        match &c.kind {
            CompoundKind::Brace(list) => {
                self.out.push_str("{ \n");
                self.list_lines(list, level + 1);
                self.out.push('\n');
                self.indent(level);
                self.out.push('}');
            }
            _ => self.compound(c, level),
        }
    }

    /// Lista em linhas: cada item indentado, separados por `;\n` (ou `&\n`).
    fn list_lines(&mut self, list: &List, level: usize) {
        let n = list.items.len();
        for (i, item) in list.items.iter().enumerate() {
            self.indent(level);
            self.and_or(&item.and_or, level);
            if item.background {
                self.out.push_str(" &");
                if i + 1 < n {
                    self.out.push('\n');
                }
            } else if i + 1 < n {
                self.out.push_str(";\n");
            }
        }
    }

    /// Lista numa linha só (`a; b; c`).
    fn list_inline(&mut self, list: &List, level: usize) {
        let n = list.items.len();
        for (i, item) in list.items.iter().enumerate() {
            self.and_or(&item.and_or, level);
            if item.background {
                self.out.push_str(" &");
                if i + 1 < n {
                    self.out.push(' ');
                }
            } else if i + 1 < n {
                self.out.push_str("; ");
            }
        }
    }

    fn and_or(&mut self, ao: &AndOr, level: usize) {
        self.pipeline(&ao.first, level);
        for (c, p) in &ao.rest {
            self.out.push_str(match c {
                Connector::And => " && ",
                Connector::Or => " || ",
            });
            self.pipeline(p, level);
        }
    }

    fn pipeline(&mut self, p: &Pipeline, level: usize) {
        if let Some(t) = p.time {
            self.out.push_str(if t.posix { "time -p " } else { "time " });
        }
        if p.negated {
            self.out.push_str("! ");
        }
        for (i, c) in p.commands.iter().enumerate() {
            if i > 0 {
                self.out.push_str(" | ");
            }
            self.command(c, level);
        }
    }

    fn command(&mut self, c: &Command, level: usize) {
        match c {
            Command::Simple(s) => {
                self.out.push_str(&simple_text(s));
                self.out.push_str(&heredoc_bodies(&s.redirects));
            }
            Command::Compound(cc, redirs) => {
                self.compound(cc, level);
                for r in redirs.iter() {
                    self.out.push(' ');
                    self.out.push_str(&redirect_text(r));
                }
                self.out.push_str(&heredoc_bodies(redirs));
            }
            Command::FunctionDef(f) => self.function(f, level),
        }
    }

    fn words(ws: &[Word]) -> String {
        ws.iter().map(|w| w.raw.to_string()).collect::<Vec<_>>().join(" ")
    }

    fn compound(&mut self, c: &Compound, level: usize) {
        match &c.kind {
            CompoundKind::Brace(list) => {
                self.out.push_str("{ \n");
                self.list_lines(list, level + 1);
                self.out.push('\n');
                self.indent(level);
                self.out.push('}');
            }
            CompoundKind::Subshell(list) => {
                self.out.push_str("( ");
                self.list_inline(list, level);
                self.out.push_str(" )");
            }
            CompoundKind::For { var, words, body } | CompoundKind::Select { var, words, body } => {
                let kw = if matches!(c.kind, CompoundKind::For { .. }) { "for" } else { "select" };
                self.out.push_str(kw);
                self.out.push(' ');
                self.out.push_str(var);
                if let Some(ws) = words {
                    self.out.push_str(" in ");
                    self.out.push_str(&Self::words(ws));
                }
                self.out.push_str(";\n");
                self.indent(level);
                self.out.push_str("do\n");
                self.list_lines(body, level + 1);
                self.out.push_str(";\n");
                self.indent(level);
                self.out.push_str("done");
            }
            CompoundKind::ArithFor { init, cond, step, body } => {
                let t = |a: &Option<std::sync::Arc<ArithExp>>| a.as_ref().map(|x| x.raw.to_string()).unwrap_or_default();
                self.out.push_str(&format!("for (({}; {}; {}))\n", t(init), t(cond), t(step)));
                self.indent(level);
                self.out.push_str("do\n");
                self.list_lines(body, level + 1);
                self.out.push_str(";\n");
                self.indent(level);
                self.out.push_str("done");
            }
            CompoundKind::Case { word, items } => {
                self.out.push_str(&format!("case {} in \n", word.raw));
                for it in items {
                    self.indent(level + 1);
                    self.out.push_str(&Self::words(&it.patterns).replace(' ', " | "));
                    self.out.push_str(")\n");
                    if !it.body.items.is_empty() {
                        self.list_lines(&it.body, level + 2);
                        self.out.push('\n');
                    }
                    self.indent(level + 1);
                    self.out.push_str(match it.term {
                        CaseTerm::Break => ";;\n",
                        CaseTerm::FallThrough => ";&\n",
                        CaseTerm::Continue => ";;&\n",
                    });
                }
                self.indent(level);
                self.out.push_str("esac");
            }
            CompoundKind::If { branches, else_body } => {
                for (i, (cond, body)) in branches.iter().enumerate() {
                    if i == 0 {
                        self.out.push_str("if ");
                    } else {
                        self.indent(level);
                        self.out.push_str("elif ");
                    }
                    self.list_inline(cond, level);
                    self.out.push_str("; then\n");
                    self.list_lines(body, level + 1);
                    self.out.push_str(";\n");
                }
                if let Some(e) = else_body {
                    self.indent(level);
                    self.out.push_str("else\n");
                    self.list_lines(e, level + 1);
                    self.out.push_str(";\n");
                }
                self.indent(level);
                self.out.push_str("fi");
            }
            CompoundKind::While { cond, body } | CompoundKind::Until { cond, body } => {
                let kw = if matches!(c.kind, CompoundKind::While { .. }) { "while" } else { "until" };
                self.out.push_str(kw);
                self.out.push(' ');
                self.list_inline(cond, level);
                self.out.push_str("; do\n");
                self.list_lines(body, level + 1);
                self.out.push_str(";\n");
                self.indent(level);
                self.out.push_str("done");
            }
            CompoundKind::Arith(a) => {
                self.out.push_str(&format!("(({}))", a.raw));
            }
            CompoundKind::Cond(e) => {
                self.out.push_str("[[ ");
                self.cond(e);
                self.out.push_str(" ]]");
            }
            CompoundKind::Coproc { name, body } => {
                self.out.push_str("coproc ");
                if name != "COPROC" {
                    self.out.push_str(name);
                    self.out.push(' ');
                }
                self.command(body, level);
            }
        }
    }

    fn cond(&mut self, e: &CondExpr) {
        match e {
            CondExpr::And(a, b) => {
                self.cond(a);
                self.out.push_str(" && ");
                self.cond(b);
            }
            CondExpr::Or(a, b) => {
                self.cond(a);
                self.out.push_str(" || ");
                self.cond(b);
            }
            CondExpr::Not(a) => {
                self.out.push_str("! ");
                self.cond(a);
            }
            CondExpr::Group(a) => {
                self.out.push_str("( ");
                self.cond(a);
                self.out.push_str(" )");
            }
            CondExpr::Unary(op, w) => {
                self.out.push_str(&format!("-{op} {}", w.raw));
            }
            CondExpr::Binary(op, l, r) => {
                let o = match op {
                    CondBinOp::Match => "==",
                    CondBinOp::NoMatch => "!=",
                    CondBinOp::Regex => "=~",
                    CondBinOp::Less => "<",
                    CondBinOp::Greater => ">",
                    CondBinOp::Eq => "-eq",
                    CondBinOp::Ne => "-ne",
                    CondBinOp::Lt => "-lt",
                    CondBinOp::Le => "-le",
                    CondBinOp::Gt => "-gt",
                    CondBinOp::Ge => "-ge",
                    CondBinOp::Newer => "-nt",
                    CondBinOp::Older => "-ot",
                    CondBinOp::SameFile => "-ef",
                };
                self.out.push_str(&format!("{} {o} {}", l.raw, r.raw));
            }
            CondExpr::Word(w) => self.out.push_str(&w.raw),
        }
    }
}
