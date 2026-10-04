//! Ações da gramática do bc: geração do código de cada construção, as checagens de compilação e
//! a execução de cada item de entrada assim que ele termina, como o GNU bc 1.07.1.
//!
//! Mensagens de compilação medidas no oráculo:
//!
//! - erros (o item de entrada não roda): `syntax error`, `illegal character`, as checagens de
//!   expressão `void` (`void expression with +`, `Assignment of a void expression`, `void argument`,
//!   ...), `Break outside a for/while`, `Continue outside a for`, `Return outside of a function.`,
//!   parâmetros e `auto` repetidos;
//! - avisos de extensão do GNU, que viram `Error:` com `-s` e `(Warning)` com `-w`;
//! - a linha da mensagem é a do léxico no momento da redução, então depende de a regra ter lido ou
//!   não o próximo token (o fim de linha já lido conta a linha seguinte).

use std::rc::Rc;

use super::exec::{self, Code, Ins, Kind, Label, Param, Rel, Vm};
use super::grammar::{A, Bc};
use super::lalr::Handler;
use super::lexer::{LexDiag, Lexer, T};
use crate::util::io;

const EX_ASSGN: u8 = 1;
const EX_PAREN: u8 = 2;
const EX_VOID: u8 = 4;
const EX_EMPTY: u8 = 8;

#[derive(Clone, Copy, Debug)]
pub enum Named {
    Var(u32),
    Array(u32),
}

#[derive(Clone, Debug)]
pub struct Def {
    idx: u32,
    kind: Kind,
}

/// Valor semântico de cada símbolo.
#[derive(Clone, Debug, Default)]
pub enum V {
    #[default]
    None,
    Text(Rc<[u8]>),
    Expr(u8),
    Named(Named),
    Args(Vec<bool>),
    Defs(Vec<Def>),
    Void(bool),
}

impl V {
    fn text(&self) -> Rc<[u8]> {
        match self {
            V::Text(t) => t.clone(),
            _ => Rc::from(&b""[..]),
        }
    }

    fn flags(&self) -> u8 {
        match self {
            V::Expr(f) => *f,
            _ => 0,
        }
    }

    fn named(&self) -> Named {
        match self {
            V::Named(n) => *n,
            _ => Named::Var(exec::VAR_LAST),
        }
    }

    fn args(&self) -> Vec<bool> {
        match self {
            V::Args(a) => a.clone(),
            _ => Vec::new(),
        }
    }

    fn defs(&self) -> Vec<Def> {
        match self {
            V::Defs(d) => d.clone(),
            _ => Vec::new(),
        }
    }
}

const WARRANTY: &str = "
bc 1.07.1
Copyright 1991-1994, 1997, 1998, 2000, 2004, 2006, 2008, 2012-2017 Free Software Foundation, Inc.

    This program is free software; you can redistribute it and/or modify
    it under the terms of the GNU General Public License as published by
    the Free Software Foundation; either version 3 of the License , or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

    You should have received a copy of the GNU General Public License
    along with this program. If not, write to

       The Free Software Foundation, Inc.
       51 Franklin Street, Fifth Floor
       Boston, MA 02110-1335  USA

";

const LIMITS: &str = "BC_BASE_MAX     = 2147483647
BC_DIM_MAX      = 16777215
BC_SCALE_MAX    = 2147483647
BC_STRING_MAX   = 2147483647
MAX Exponent    = 9223372036854775807
Number of vars  = 32767
";

/// Função em construção.
struct FuncBuild {
    idx: u32,
    code: Code,
}

pub struct Compiler {
    pub lex: Lexer,
    pub vm: Vm,
    g: &'static Bc,
    main: Code,
    func: Option<FuncBuild>,
    had_error: bool,
    std_only: bool,
    warn_mode: bool,
    break_label: Option<Label>,
    continue_label: Option<Label>,
    saved_labels: Vec<(Option<Label>, Option<Label>)>,
    if_labels: Vec<Label>,
    loop_tops: Vec<Label>,
    for_labels: Vec<(Label, Label)>,
    logic_labels: Vec<Label>,
    in_function: bool,
    void_function: bool,
    errok: bool,
    /// Código de saída quando o processo tem de terminar (`quit`, `halt`, erro fatal).
    pub exit: Option<i32>,
}

impl Compiler {
    pub fn new(lex: Lexer, vm: Vm, std_only: bool, warn_mode: bool) -> Compiler {
        Compiler {
            lex,
            vm,
            g: super::grammar::get(),
            main: Code::default(),
            func: None,
            had_error: false,
            std_only,
            warn_mode,
            break_label: None,
            continue_label: None,
            saved_labels: Vec::new(),
            if_labels: Vec::new(),
            loop_tops: Vec::new(),
            for_labels: Vec::new(),
            logic_labels: Vec::new(),
            in_function: false,
            void_function: false,
            errok: false,
            exit: None,
        }
    }

    fn code(&mut self) -> &mut Code {
        match &mut self.func {
            Some(f) => &mut f.code,
            None => &mut self.main,
        }
    }

    fn emit(&mut self, ins: Ins, size: u32) {
        self.code().emit(ins, size);
    }

    fn new_label(&mut self) -> Label {
        self.code().new_label()
    }

    fn define(&mut self, l: Label) {
        self.code().define(l);
    }

    // ---- mensagens ----

    fn message(&mut self, msg: &str) {
        self.vm.out.flush();
        io::eprint(format!("{}{msg}\n", self.lex.prefix()));
    }

    fn yyerror(&mut self, msg: &str) {
        self.message(msg);
        self.had_error = true;
    }

    /// Construção fora do POSIX: erro com `-s`, aviso com `-w`, nada sem eles.
    fn warn(&mut self, msg: &str) {
        if self.std_only {
            self.yyerror(&format!("Error: {msg}"));
        } else if self.warn_mode {
            self.message(&format!("(Warning) {msg}"));
        }
    }

    fn void_check(&mut self, flags: u8, msg: &str) {
        if flags & EX_VOID != 0 {
            self.yyerror(msg);
        }
    }

    fn check_name(&mut self, name: &[u8]) {
        if name.len() != 1 {
            self.warn(&format!("multiple letter name - {}", io::lossy(name)));
        }
    }

    fn lookup_var(&mut self, name: &[u8]) -> u32 {
        self.check_name(name);
        self.vm.var_index(name)
    }

    fn lookup_array(&mut self, name: &[u8]) -> u32 {
        self.check_name(name);
        self.vm.array_index(name)
    }

    fn lookup_func(&mut self, name: &[u8]) -> u32 {
        self.check_name(name);
        self.vm.func_index(name)
    }

    // ---- geração ----

    fn emit_load(&mut self, n: Named) {
        match n {
            Named::Var(i) => self.emit(Ins::Load(i), 1 + exec::name_bytes(i)),
            Named::Array(a) => self.emit(Ins::LoadArr(a), 1 + exec::name_bytes(a)),
        }
    }

    fn emit_store(&mut self, n: Named) {
        match n {
            Named::Var(i) => self.emit(Ins::Store(i), 1 + exec::name_bytes(i)),
            Named::Array(a) => self.emit(Ins::StoreArr(a), 1 + exec::name_bytes(a)),
        }
    }

    /// Fim de um item de entrada: roda o código principal se não houve erro e recomeça.
    fn run_code(&mut self) {
        let code = std::mem::take(&mut self.main);
        if !self.had_error
            && !code.is_empty()
            && let Some(c) = self.vm.run(Rc::new(code))
        {
            self.exit = Some(c);
        }
        self.had_error = false;
    }

    /// Recuperação de erro no nível de item: descarta o que estava sendo montado.
    fn reset_after_error(&mut self) {
        self.main = Code::default();
        self.func = None;
        self.in_function = false;
        self.void_function = false;
        self.break_label = None;
        self.continue_label = None;
        self.saved_labels.clear();
        self.if_labels.clear();
        self.loop_tops.clear();
        self.for_labels.clear();
        self.logic_labels.clear();
        self.had_error = false;
    }

    fn check_duplicates(&mut self, defs: &[Def], msg: &str) {
        for (i, d) in defs.iter().enumerate() {
            if defs[..i].iter().any(|e| e.idx == d.idx && e.kind.is_array() == d.kind.is_array()) {
                self.yyerror(msg);
                return;
            }
        }
    }

    fn def(&mut self, name: Rc<[u8]>, kind: Kind) -> Def {
        let idx = if kind.is_array() { self.lookup_array(&name) } else { self.lookup_var(&name) };
        Def { idx, kind }
    }

    fn act(&mut self, a: A, stack: &[V], len: usize) -> V {
        let n = stack.len();
        let v = &stack[n - len..];
        match a {
            A::None => V::None,
            A::Pass1 => v[0].clone(),
            A::RunCode => {
                self.run_code();
                V::None
            }
            A::ErrorLine => {
                self.reset_after_error();
                self.errok = true;
                V::None
            }
            A::Warranty => {
                self.vm.out.raw(WARRANTY.as_bytes());
                V::None
            }
            A::Limits => {
                self.vm.out.raw(LIMITS.as_bytes());
                V::None
            }
            A::ExprStmt => {
                let f = v[0].flags();
                if f & (EX_ASSGN | EX_VOID) != 0 {
                    self.emit(Ins::Pop, 1);
                } else {
                    self.emit(Ins::PrintPop, 1);
                }
                V::None
            }
            A::StringStmt => {
                let t = v[0].text();
                let size = 2 + t.len() as u32;
                self.emit(Ins::StrStmt(t), size);
                V::None
            }
            A::Break => {
                match self.break_label {
                    Some(l) => self.emit(Ins::Jump(l), 3),
                    None => self.yyerror("Break outside a for/while"),
                }
                V::None
            }
            A::Continue => {
                self.warn("Continue statement");
                match self.continue_label {
                    Some(l) => self.emit(Ins::Jump(l), 3),
                    None => self.yyerror("Continue outside a for"),
                }
                V::None
            }
            A::Quit => {
                self.exit = Some(0);
                V::None
            }
            A::Halt => {
                self.emit(Ins::Halt, 1);
                V::None
            }
            A::Return => {
                let f = v[1].flags();
                if !self.in_function {
                    self.yyerror("Return outside of a function.");
                } else if f & EX_EMPTY != 0 {
                    self.emit(Ins::Num(Rc::from(&b"0"[..])), 1);
                    self.emit(Ins::Ret, 1);
                } else {
                    if self.void_function {
                        self.yyerror("Return expression in a void function.");
                    }
                    self.emit(Ins::Ret, 1);
                }
                V::None
            }
            A::RetEmpty => V::Expr(EX_EMPTY),
            A::RetExpr => {
                let f = v[0].flags();
                if f & EX_VOID != 0 {
                    self.yyerror("return requires non-void expression");
                } else if f & EX_PAREN == 0 {
                    self.warn("return expression requires parenthesis");
                }
                V::Expr(f & !EX_EMPTY)
            }
            A::ForM1 => {
                self.saved_labels.push((self.break_label, self.continue_label));
                let b = self.new_label();
                self.break_label = Some(b);
                V::None
            }
            A::ForM2 => {
                let f = stack[n - 2].flags();
                if f & EX_VOID != 0 {
                    self.yyerror("first expression is void");
                }
                if f & EX_EMPTY != 0 {
                    self.warn("Missing expression in for statement");
                } else {
                    self.emit(Ins::Pop, 1);
                }
                let cond = self.new_label();
                self.define(cond);
                let body = self.new_label();
                self.for_labels.push((cond, body));
                V::None
            }
            A::ForM3 => {
                let f = stack[n - 2].flags();
                if f & EX_VOID != 0 {
                    self.yyerror("second expression is void");
                }
                if f & EX_EMPTY != 0 {
                    self.warn("Missing expression in for statement");
                    self.emit(Ins::Num(Rc::from(&b"1"[..])), 1);
                }
                let brk = self.break_label.unwrap_or(0);
                let body = self.for_labels.last().map(|l| l.1).unwrap_or(0);
                self.emit(Ins::JumpZero(brk), 3);
                self.emit(Ins::Jump(body), 3);
                let incr = self.new_label();
                self.define(incr);
                self.continue_label = Some(incr);
                V::None
            }
            A::ForM4 => {
                let f = stack[n - 2].flags();
                if f & EX_VOID != 0 {
                    self.yyerror("third expression is void");
                }
                if f & EX_EMPTY != 0 {
                    self.warn("Missing expression in for statement");
                } else {
                    self.emit(Ins::Pop, 1);
                }
                let (cond, body) = self.for_labels.last().copied().unwrap_or((0, 0));
                self.emit(Ins::Jump(cond), 3);
                self.define(body);
                V::None
            }
            A::ForEnd => {
                self.for_labels.pop();
                if let Some(c) = self.continue_label {
                    self.emit(Ins::Jump(c), 3);
                }
                if let Some(b) = self.break_label {
                    self.define(b);
                }
                let (b, c) = self.saved_labels.pop().unwrap_or((None, None));
                self.break_label = b;
                self.continue_label = c;
                V::None
            }
            A::IfM1 => {
                let f = stack[n - 2].flags();
                self.void_check(f, "void expression");
                let l = self.new_label();
                self.emit(Ins::JumpZero(l), 3);
                self.if_labels.push(l);
                V::None
            }
            A::IfNoElse => {
                if let Some(l) = self.if_labels.pop() {
                    self.define(l);
                }
                V::None
            }
            A::ElseM1 => {
                self.warn("else clause in if statement");
                let end = self.new_label();
                self.emit(Ins::Jump(end), 3);
                if let Some(l) = self.if_labels.pop() {
                    self.define(l);
                }
                self.if_labels.push(end);
                V::None
            }
            A::ElseEnd => {
                if let Some(l) = self.if_labels.pop() {
                    self.define(l);
                }
                V::None
            }
            A::WhileM1 => {
                self.saved_labels.push((self.break_label, self.continue_label));
                let b = self.new_label();
                self.break_label = Some(b);
                let top = self.new_label();
                self.define(top);
                self.loop_tops.push(top);
                V::None
            }
            A::WhileM2 => {
                let f = stack[n - 2].flags();
                self.void_check(f, "void expression");
                let b = self.break_label.unwrap_or(0);
                self.emit(Ins::JumpZero(b), 3);
                V::None
            }
            A::WhileEnd => {
                if let Some(top) = self.loop_tops.pop() {
                    self.emit(Ins::Jump(top), 3);
                }
                if let Some(b) = self.break_label {
                    self.define(b);
                }
                let (b, c) = self.saved_labels.pop().unwrap_or((None, None));
                self.break_label = b;
                self.continue_label = c;
                V::None
            }
            A::PrintM1 => {
                self.warn("print statement");
                V::None
            }
            A::PrintStr => {
                let t = v[0].text();
                let size = 2 + t.len() as u32;
                self.emit(Ins::PrintStr(t), size);
                V::None
            }
            A::PrintExpr => {
                self.void_check(v[0].flags(), "void expression in print");
                self.emit(Ins::PrintNoNl, 1);
                V::None
            }
            A::OptVoid => {
                self.warn("void functions");
                V::Void(true)
            }
            A::RequiredEolEmpty => {
                self.warn("End of line required");
                V::None
            }
            A::DefVar => V::Defs(vec![self.def(v[0].text(), Kind::Var)]),
            A::DefArray => V::Defs(vec![self.def(v[0].text(), Kind::Array)]),
            A::DefRef => {
                self.warn("Call by variable arrays");
                V::Defs(vec![self.def(v[1].text(), Kind::RefArray)])
            }
            A::DefListVar | A::DefListArray | A::DefListRef => {
                let mut list = v[0].defs();
                let d = match a {
                    A::DefListVar => self.def(v[2].text(), Kind::Var),
                    A::DefListArray => self.def(v[2].text(), Kind::Array),
                    _ => {
                        self.warn("Call by variable arrays");
                        self.def(v[3].text(), Kind::RefArray)
                    }
                };
                list.push(d);
                V::Defs(list)
            }
            A::AutoList => {
                let defs = v[1].defs();
                self.check_duplicates(&defs, "duplicate auto variable names");
                V::Defs(defs)
            }
            A::FuncM1 => {
                // define OptVoid NAME ( OptParameterList ) OptNewline { RequiredEol OptAutoDefineList .
                let autos = stack[n - 1].defs();
                let params = stack[n - 6].defs();
                let name = stack[n - 8].text();
                let void = matches!(stack[n - 9], V::Void(true));
                for _ in params.iter().filter(|p| p.kind == Kind::RefArray) {
                    self.warn("Variable array parameter");
                }
                self.check_duplicates(&params, "duplicate parameter names");
                if params.iter().any(|p| autos.iter().any(|a| a.idx == p.idx && a.kind.is_array() == p.kind.is_array())) {
                    self.yyerror("variable in both parameter and auto lists");
                }
                let idx = self.lookup_func(&name);
                let f = &mut self.vm.funcs[idx as usize];
                f.params = params.iter().map(|d| Param { idx: d.idx, kind: d.kind }).collect();
                f.autos = autos.iter().map(|d| Param { idx: d.idx, kind: d.kind }).collect();
                f.void = void;
                self.func = Some(FuncBuild { idx, code: Code::default() });
                self.in_function = true;
                self.void_function = void;
                V::None
            }
            A::FuncEnd => {
                self.emit(Ins::Num(Rc::from(&b"0"[..])), 1);
                self.emit(Ins::Ret, 1);
                if let Some(fb) = self.func.take() {
                    let f = &mut self.vm.funcs[fb.idx as usize];
                    if self.had_error {
                        f.defined = false;
                    } else {
                        f.defined = true;
                        f.native = None;
                        f.code = Rc::new(fb.code);
                    }
                }
                self.in_function = false;
                self.void_function = false;
                V::None
            }
            A::ArgsEmpty => V::Args(Vec::new()),
            A::ArgExpr => {
                self.void_check(v[0].flags(), "void argument");
                V::Args(vec![false])
            }
            A::ArgArray => {
                let idx = self.lookup_array(&v[0].text());
                self.emit(Ins::PushArray(idx), exec::push_array_bytes(idx));
                V::Args(vec![true])
            }
            A::ArgListExpr => {
                self.void_check(v[2].flags(), "void argument");
                let mut l = v[0].args();
                l.push(false);
                V::Args(l)
            }
            A::ArgListArray => {
                let idx = self.lookup_array(&v[2].text());
                self.emit(Ins::PushArray(idx), exec::push_array_bytes(idx));
                let mut l = v[0].args();
                l.push(true);
                V::Args(l)
            }
            A::OptExprEmpty => V::Expr(EX_EMPTY),
            A::AssignM1 => {
                // NamedExpression AssignOp . Expression
                let op = stack[n - 1].text();
                if op.as_ref() != b"=" {
                    match stack[n - 2].named() {
                        Named::Var(i) => self.emit(Ins::Load(i), 1 + exec::name_bytes(i)),
                        Named::Array(a) => self.emit(Ins::DupLoadArr(a), 2 + exec::name_bytes(a)),
                    }
                }
                V::None
            }
            A::Assign => {
                let op = v[1].text();
                if v[3].flags() & EX_VOID != 0 {
                    self.yyerror("Assignment of a void expression");
                }
                if op.as_ref() != b"=" {
                    self.emit(Ins::Bin(op[0]), 1);
                }
                self.emit_store(v[0].named());
                V::Expr(EX_ASSGN)
            }
            A::AndM1 => {
                self.warn("&& operator");
                let l = self.new_label();
                self.emit(Ins::AndCheck(l), 5);
                self.logic_labels.push(l);
                V::None
            }
            A::And => {
                if (v[0].flags() | v[3].flags()) & EX_VOID != 0 {
                    self.yyerror("void expression with &&");
                }
                self.emit(Ins::AndEnd, 6);
                if let Some(l) = self.logic_labels.pop() {
                    self.define(l);
                }
                V::Expr(0)
            }
            A::OrM1 => {
                self.warn("|| operator");
                let l = self.new_label();
                self.emit(Ins::OrCheck(l), 3);
                self.logic_labels.push(l);
                V::None
            }
            A::Or => {
                if (v[0].flags() | v[3].flags()) & EX_VOID != 0 {
                    self.yyerror("void expression with ||");
                }
                self.emit(Ins::OrEnd, 8);
                if let Some(l) = self.logic_labels.pop() {
                    self.define(l);
                }
                V::Expr(0)
            }
            A::Not => {
                self.warn("! operator");
                self.void_check(v[1].flags(), "void expression with !");
                self.emit(Ins::Not, 1);
                V::Expr(0)
            }
            A::Rel => {
                if (v[0].flags() | v[2].flags()) & EX_VOID != 0 {
                    self.yyerror("void expression with comparison");
                }
                let r = match v[1].text().as_ref() {
                    b"==" => Rel::Eq,
                    b"!=" => Rel::Ne,
                    b"<" => Rel::Lt,
                    b"<=" => Rel::Le,
                    b">" => Rel::Gt,
                    _ => Rel::Ge,
                };
                self.emit(Ins::Rel(r), 1);
                V::Expr(0)
            }
            A::Binary(op) => {
                if (v[0].flags() | v[2].flags()) & EX_VOID != 0 {
                    // O GNU passa o texto montado como formato do printf: com `%` sai vazio.
                    let msg = if op == b'%' { String::new() } else { format!("void expression with {}", op as char) };
                    self.yyerror(&msg);
                }
                self.emit(Ins::Bin(op), 1);
                V::Expr(0)
            }
            A::Neg => {
                self.void_check(v[1].flags(), "void expression with unary -");
                self.emit(Ins::Neg, 1);
                V::Expr(0)
            }
            A::LoadNamed => {
                self.emit_load(v[0].named());
                V::Expr(0)
            }
            A::Number => {
                let t = v[0].text();
                let size = exec::const_bytes(&t);
                self.emit(Ins::Num(t), size);
                V::Expr(0)
            }
            A::Paren => {
                let f = v[1].flags();
                self.void_check(f, "void expression in parenthesis");
                V::Expr((f & !EX_ASSGN) | EX_PAREN)
            }
            A::Call => {
                let fi = self.lookup_func(&v[0].text());
                let kinds = v[2].args();
                let size = 1 + exec::name_bytes(fi) + kinds.len() as u32 + 1;
                self.emit(Ins::Call(fi, Rc::from(kinds)), size);
                let void = self.vm.funcs[fi as usize].void;
                V::Expr(if void { EX_VOID } else { 0 })
            }
            A::PreIncr | A::PostIncr => {
                let (tok, named) = if a == A::PreIncr { (&v[0], v[1].named()) } else { (&v[1], v[0].named()) };
                let up = tok.text().first() == Some(&b'+');
                match (a, named) {
                    (A::PreIncr, Named::Var(i)) => self.emit(Ins::PreInc(i, up), 2 + 2 * exec::name_bytes(i)),
                    (_, Named::Var(i)) => self.emit(Ins::PostInc(i, up), 2 + 2 * exec::name_bytes(i)),
                    (A::PreIncr, Named::Array(x)) => self.emit(Ins::PreIncArr(x, up), 3 + 2 * exec::name_bytes(x)),
                    (_, Named::Array(x)) => self.emit(Ins::PostIncArr(x, up), 4 + 2 * exec::name_bytes(x)),
                }
                V::Expr(0)
            }
            A::Length | A::Sqrt | A::ScaleFn => {
                let (ins, msg) = match a {
                    A::Length => (Ins::Length, "void expression in length()"),
                    A::Sqrt => (Ins::Sqrt, "void expression in sqrt()"),
                    _ => (Ins::ScaleOf, "void expression in scale()"),
                };
                self.void_check(v[2].flags(), msg);
                self.emit(ins, 2);
                V::Expr(0)
            }
            A::Read => {
                self.warn("read function");
                self.emit(Ins::Read, 2);
                V::Expr(0)
            }
            A::Random => {
                self.warn("random function");
                self.emit(Ins::Random, 2);
                V::Expr(0)
            }
            A::NamedVar => V::Named(Named::Var(self.lookup_var(&v[0].text()))),
            A::NamedArray => {
                self.void_check(v[2].flags(), "void expression as subscript");
                V::Named(Named::Array(self.lookup_array(&v[0].text())))
            }
            A::NamedSpecial(i) => V::Named(Named::Var(u32::from(i))),
            A::NamedHistory => {
                self.warn("History variable");
                V::Named(Named::Var(exec::VAR_HISTORY))
            }
            A::NamedLast => {
                self.warn("Last variable");
                V::Named(Named::Var(exec::VAR_LAST))
            }
        }
    }
}

impl Handler for Compiler {
    type Value = V;

    fn lex(&mut self) -> (u16, V) {
        let mut diags = Vec::new();
        let tok = self.lex.next(&mut diags);
        if !self.lex.echo.is_empty() {
            let echo = std::mem::take(&mut self.lex.echo);
            self.vm.out.raw(&echo);
        }
        for (prefix, d) in diags {
            match d {
                LexDiag::Illegal(c) => {
                    self.vm.out.flush();
                    io::eprint(format!("{prefix}illegal character: {c}\n"));
                    self.had_error = true;
                }
                LexDiag::NonStandardBase => {
                    let msg = "Non-standard base in numeric constant";
                    if self.std_only {
                        self.vm.out.flush();
                        io::eprint(format!("{prefix}Error: {msg}\n"));
                        self.had_error = true;
                    } else if self.warn_mode {
                        self.vm.out.flush();
                        io::eprint(format!("{prefix}(Warning) {msg}\n"));
                    }
                }
                LexDiag::CommentEof => {
                    self.vm.out.flush();
                    io::eprint("EOF encountered in a comment.\n");
                }
                LexDiag::Unavailable(_) | LexDiag::ReadFailed => {}
            }
        }
        match tok {
            Ok(t) => {
                let text: Rc<[u8]> = Rc::from(t.text);
                (t.kind as u16, V::Text(text))
            }
            Err(LexDiag::Unavailable(name)) => {
                self.vm.out.flush();
                io::eprint(format!("File {} is unavailable.\n", io::lossy(&name)));
                self.exit = Some(1);
                (T::Eof as u16, V::None)
            }
            Err(_) => {
                self.vm.out.flush();
                io::eprint("read() in flex scanner failed\n");
                self.exit = Some(1);
                (T::Eof as u16, V::None)
            }
        }
    }

    fn reduce(&mut self, rule: u32, stack: &[V], len: usize) -> V {
        let a = self.g.actions[rule as usize];
        self.act(a, stack, len)
    }

    fn syntax_error(&mut self) {
        self.yyerror("syntax error");
    }

    fn take_errok(&mut self) -> bool {
        std::mem::take(&mut self.errok)
    }

    fn aborted(&self) -> bool {
        self.exit.is_some()
    }
}
