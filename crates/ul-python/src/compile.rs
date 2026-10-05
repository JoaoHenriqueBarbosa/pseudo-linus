//! Compilador do AST (`parser::parse_module`) para o bytecode próprio da VM (fatia 10 de
//! `docs/python3-port.md`).
//!
//! O bytecode é de pilha e não copia os opcodes do CPython (`dis` e `.pyc` estão fora do escopo),
//! mas a ordem de avaliação é a mesma: operandos da esquerda para a direita, valor antes do alvo na
//! atribuição, comparações encadeadas avaliando cada operando uma vez só. Cada instrução guarda a
//! linha do nó que a gerou, que é o `lineno` do traceback.
//!
//! Esta fatia cobre o nível de módulo: expressões aritméticas, lógicas e de comparação, nomes
//! globais, atribuição (simples, múltipla, desempacotamento e subscrição), atribuição aumentada,
//! `if`, `while` e `for` (com `else`, `break` e `continue`), `pass`, chamadas com argumentos
//! posicionais e nomeados, e os displays de `list`, `tuple`, `set` e `dict`. O que ainda não é
//! suportado (funções, classes, `import`, `try`, atributos, fatias, f-strings...) vira
//! `CompileError` do tipo `NotImplementedError`, que as próximas fatias vão eliminando.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::ast::{
    Arguments, BoolOp, CmpOp, Constant, ExceptHandler, Expr, ExprKind as E, Mod, Operator, Stmt, StmtKind as S, UnaryOp,
};
use crate::object::Value;

/// Instrução da VM. Os operandos `u32` são índices em `consts`/`names` ou alvos de salto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    LoadConst(u32),
    LoadName(u32),
    StoreName(u32),
    /// Descarta o topo.
    Pop,
    /// Duplica o topo.
    Dup,
    /// Duplica os dois do topo (`[a, b]` vira `[a, b, a, b]`).
    Dup2,
    /// Troca os dois do topo.
    Rot2,
    /// Leva o topo para a terceira posição (`[a, b, c]` vira `[c, a, b]`).
    Rot3,
    /// Operação binária; `inplace` é a forma da atribuição aumentada (`list += x` estende no lugar).
    Binary { op: Operator, inplace: bool },
    Unary(UnaryOp),
    Compare(CmpOp),
    Jump(u32),
    PopJumpIfFalse(u32),
    PopJumpIfTrue(u32),
    /// Salta mantendo o topo se ele for falso; senão o descarta (`and`).
    JumpIfFalseOrPop(u32),
    /// Salta mantendo o topo se ele for verdadeiro; senão o descarta (`or`).
    JumpIfTrueOrPop(u32),
    /// Troca o iterável do topo pelo iterador dele.
    GetIter,
    /// Empilha o próximo item do iterador do topo; esgotado, descarta o iterador e salta.
    ForIter(u32),
    /// Chamada: função, `argc` posicionais e, se `kwnames` aponta uma tupla de nomes em `consts`,
    /// os valores nomeados correspondentes por último.
    Call { argc: u32, kwnames: Option<u32> },
    BuildList(u32),
    BuildTuple(u32),
    BuildSet(u32),
    /// `n` pares chave e valor.
    BuildDict(u32),
    /// `container[index]`.
    Subscript,
    /// `container[index] = value`, com a pilha `[value, container, index]`.
    StoreSubscript,
    /// Desempacota o topo em `n` valores, o primeiro no topo.
    UnpackSequence(u32),
    /// Abre um bloco protegido; uma exceção salta para o alvo com a pilha restaurada.
    SetupTry(u32),
    /// Fecha o bloco protegido mais interno.
    PopBlock,
    /// Move a exceção do topo (mantendo-a) para a pilha de exceções tratadas.
    PushExc,
    /// Descarta a exceção tratada mais recente.
    PopExc,
    /// `[exc, cls]` vira `[exc, bool]`: a exceção é instância da classe (ou de alguma da tupla).
    ExcMatch,
    /// Levanta o topo da pilha.
    Raise,
    /// `raise` sem argumento: levanta a exceção tratada mais recente.
    ReraiseCurrent,
    /// Relevanta a exceção do topo, descartando a tratada mais recente (fim de `except` sem casamento
    /// e de `finally`).
    Reraise,
    /// `del nome`, ignorando nome ausente.
    DeleteName(u32),
    /// `objeto.nome`.
    LoadAttr(u32),
    /// Variável local da função em execução (`UnboundLocalError` se ainda sem valor).
    LoadLocal(u32),
    StoreLocal(u32),
    /// Cria a função `functions[code]` com `ndefaults` valores padrão tirados da pilha.
    MakeFunction { code: u32, ndefaults: u32 },
    /// Devolve o topo ao chamador.
    Return,
    /// `import nome`: empilha o módulo.
    Import(u32),
    /// `from m import nome`: com o módulo no topo, empilha o atributo ou levanta `ImportError`.
    ImportName(u32),
}

/// Código compilado de um módulo ou de uma função.
#[derive(Debug, Default)]
pub struct Code {
    pub ops: Vec<Op>,
    /// Linha de cada instrução, paralela a `ops`.
    pub lines: Vec<usize>,
    pub consts: Vec<Value>,
    pub names: Vec<String>,
    /// Nome da função (`<module>` no nível de módulo, vazio por `Default`).
    pub name: String,
    pub params: Vec<String>,
    pub is_function: bool,
    pub functions: Vec<Rc<Code>>,
}

/// Erro de compilação: `SyntaxError` dos que o CPython detecta no compilador (`'break' outside
/// loop`) ou `NotImplementedError` para construções que esta fatia ainda não executa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileError {
    pub kind: &'static str,
    pub msg: String,
    pub lineno: usize,
}

/// Compila um `Mod::Module`.
pub fn compile_module(module: &Mod) -> Result<Code, CompileError> {
    let Mod::Module { body, .. } = module else {
        return Err(CompileError { kind: "NotImplementedError", msg: "only modules can be compiled".into(), lineno: 1 });
    };
    let mut c = Compiler {
        code: Code { name: "<module>".into(), ..Code::default() },
        line: 1,
        loops: Vec::new(),
        name_index: HashMap::new(),
        tries: Vec::new(),
        locals: None,
    };
    c.block(body)?;
    Ok(c.code)
}

/// Nomes ligados no corpo de uma função (alvos de atribuição, `for`, `except as`, `def`) e os
/// declarados `global`; os primeiros, menos os segundos, são as variáveis locais.
fn collect_scope(body: &[Stmt], locals: &mut HashSet<String>, globals: &mut HashSet<String>) {
    fn target(e: &Expr, locals: &mut HashSet<String>) {
        match &e.kind {
            E::Name { id, .. } => {
                locals.insert(id.clone());
            }
            E::Tuple { elts, .. } | E::List { elts, .. } => elts.iter().for_each(|x| target(x, locals)),
            E::Starred { value, .. } => target(value, locals),
            _ => {}
        }
    }
    for s in body {
        match &s.kind {
            S::Assign { targets, .. } => targets.iter().for_each(|t| target(t, locals)),
            S::AugAssign { target: t, .. } => target(t, locals),
            S::For { target: t, body, orelse, .. } => {
                target(t, locals);
                collect_scope(body, locals, globals);
                collect_scope(orelse, locals, globals);
            }
            S::While { body, orelse, .. } | S::If { body, orelse, .. } => {
                collect_scope(body, locals, globals);
                collect_scope(orelse, locals, globals);
            }
            S::Try { body, handlers, orelse, finalbody } => {
                collect_scope(body, locals, globals);
                for h in handlers {
                    if let Some(n) = &h.name {
                        locals.insert(n.clone());
                    }
                    collect_scope(&h.body, locals, globals);
                }
                collect_scope(orelse, locals, globals);
                collect_scope(finalbody, locals, globals);
            }
            S::FunctionDef { name, .. } => {
                locals.insert(name.clone());
            }
            S::Global { names } => globals.extend(names.iter().cloned()),
            _ => {}
        }
    }
}

struct LoopCtx {
    continue_target: usize,
    breaks: Vec<usize>,
    /// Laço `for`: o `break` descarta o iterador da pilha antes de sair.
    is_for: bool,
    /// Quantos `try` estavam abertos quando o laço começou.
    try_depth: usize,
}

/// `try` aberto durante a compilação: o `finally`, se houver, é repetido onde o controle sai.
#[derive(Clone)]
struct TryCtx {
    finalbody: Vec<Stmt>,
}

struct Compiler {
    code: Code,
    line: usize,
    loops: Vec<LoopCtx>,
    name_index: HashMap<String, u32>,
    tries: Vec<TryCtx>,
    /// Variáveis locais da função sendo compilada (`None` no módulo).
    locals: Option<HashSet<String>>,
}

impl Compiler {
    fn unsupported(&self, what: &str) -> CompileError {
        CompileError {
            kind: "NotImplementedError",
            msg: format!("{what} is not supported yet by this interpreter"),
            lineno: self.line,
        }
    }

    fn emit(&mut self, op: Op) -> usize {
        self.code.ops.push(op);
        self.code.lines.push(self.line);
        self.code.ops.len() - 1
    }

    fn here(&self) -> usize {
        self.code.ops.len()
    }

    /// Aponta o salto em `at` para `target`.
    fn patch(&mut self, at: usize, target: usize) {
        let t = target as u32;
        self.code.ops[at] = match self.code.ops[at] {
            Op::Jump(_) => Op::Jump(t),
            Op::PopJumpIfFalse(_) => Op::PopJumpIfFalse(t),
            Op::PopJumpIfTrue(_) => Op::PopJumpIfTrue(t),
            Op::JumpIfFalseOrPop(_) => Op::JumpIfFalseOrPop(t),
            Op::JumpIfTrueOrPop(_) => Op::JumpIfTrueOrPop(t),
            Op::ForIter(_) => Op::ForIter(t),
            Op::SetupTry(_) => Op::SetupTry(t),
            other => other,
        };
    }

    fn constant(&mut self, value: Value) -> u32 {
        self.code.consts.push(value);
        (self.code.consts.len() - 1) as u32
    }

    fn name(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.name_index.get(name) {
            return i;
        }
        let i = self.code.names.len() as u32;
        self.code.names.push(name.to_string());
        self.name_index.insert(name.to_string(), i);
        i
    }

    fn block(&mut self, body: &[Stmt]) -> Result<(), CompileError> {
        body.iter().try_for_each(|s| self.stmt(s))
    }

    fn stmt(&mut self, stmt: &Stmt) -> Result<(), CompileError> {
        self.line = stmt.pos.lineno;
        match &stmt.kind {
            S::Expr { value } => {
                self.expr(value)?;
                self.emit(Op::Pop);
            }
            S::Assign { targets, value, .. } => {
                self.expr(value)?;
                for (i, target) in targets.iter().enumerate() {
                    if i + 1 < targets.len() {
                        self.emit(Op::Dup);
                    }
                    self.store(target)?;
                }
            }
            S::AugAssign { target, op, value } => self.aug_assign(target, *op, value)?,
            S::Pass => {}
            // No nível de módulo `global x` não muda nada.
            S::Global { .. } => {}
            S::If { test, body, orelse } => {
                self.expr(test)?;
                let to_else = self.emit(Op::PopJumpIfFalse(0));
                self.block(body)?;
                if orelse.is_empty() {
                    let end = self.here();
                    self.patch(to_else, end);
                } else {
                    let to_end = self.emit(Op::Jump(0));
                    let else_start = self.here();
                    self.patch(to_else, else_start);
                    self.block(orelse)?;
                    let end = self.here();
                    self.patch(to_end, end);
                }
            }
            S::While { test, body, orelse } => {
                let top = self.here();
                self.line = stmt.pos.lineno;
                self.expr(test)?;
                let to_else = self.emit(Op::PopJumpIfFalse(0));
                self.loops.push(LoopCtx { continue_target: top, breaks: Vec::new(), is_for: false, try_depth: self.tries.len() });
                self.block(body)?;
                self.line = stmt.pos.lineno;
                self.emit(Op::Jump(top as u32));
                let ctx = self.loops.pop().unwrap_or(LoopCtx { continue_target: top, breaks: Vec::new(), is_for: false, try_depth: 0 });
                let else_start = self.here();
                self.patch(to_else, else_start);
                self.block(orelse)?;
                let end = self.here();
                for b in ctx.breaks {
                    self.patch(b, end);
                }
            }
            S::For { target, iter, body, orelse, .. } => {
                self.expr(iter)?;
                self.line = stmt.pos.lineno;
                self.emit(Op::GetIter);
                let top = self.emit(Op::ForIter(0));
                self.store(target)?;
                self.loops.push(LoopCtx { continue_target: top, breaks: Vec::new(), is_for: true, try_depth: self.tries.len() });
                self.block(body)?;
                self.line = stmt.pos.lineno;
                self.emit(Op::Jump(top as u32));
                let ctx = self.loops.pop().unwrap_or(LoopCtx { continue_target: top, breaks: Vec::new(), is_for: true, try_depth: 0 });
                let else_start = self.here();
                self.patch(top, else_start);
                self.block(orelse)?;
                let end = self.here();
                for b in ctx.breaks {
                    self.patch(b, end);
                }
            }
            S::Break => {
                let Some(ctx) = self.loops.last() else {
                    return Err(CompileError { kind: "SyntaxError", msg: "'break' outside loop".into(), lineno: self.line });
                };
                let (is_for, depth) = (ctx.is_for, ctx.try_depth);
                self.leave_tries(depth)?;
                if is_for {
                    self.emit(Op::Pop);
                }
                let at = self.emit(Op::Jump(0));
                if let Some(ctx) = self.loops.last_mut() {
                    ctx.breaks.push(at);
                }
            }
            S::Continue => {
                let Some(ctx) = self.loops.last() else {
                    return Err(CompileError {
                        kind: "SyntaxError",
                        msg: "'continue' not properly in loop".into(),
                        lineno: self.line,
                    });
                };
                let (target, depth) = (ctx.continue_target as u32, ctx.try_depth);
                self.leave_tries(depth)?;
                self.emit(Op::Jump(target));
            }
            S::Try { body, handlers, orelse, finalbody } => self.try_stmt(body, handlers, orelse, finalbody)?,
            S::Raise { exc, cause } => {
                if cause.is_some() {
                    return Err(self.unsupported("raise ... from"));
                }
                match exc {
                    Some(e) => {
                        self.expr(e)?;
                        self.line = stmt.pos.lineno;
                        self.emit(Op::Raise);
                    }
                    None => {
                        self.emit(Op::ReraiseCurrent);
                    }
                }
            }
            S::Assert { test, msg } => {
                self.expr(test)?;
                let ok = self.emit(Op::PopJumpIfTrue(0));
                self.line = stmt.pos.lineno;
                self.emit_load("AssertionError");
                let argc = if let Some(m) = msg {
                    self.expr(m)?;
                    1
                } else {
                    0
                };
                self.line = stmt.pos.lineno;
                self.emit(Op::Call { argc, kwnames: None });
                self.emit(Op::Raise);
                let end = self.here();
                self.patch(ok, end);
            }
            S::FunctionDef { name, args, body, decorator_list, returns, .. } => {
                if !decorator_list.is_empty() || returns.is_some() {
                    return Err(self.unsupported("decorators and annotations"));
                }
                self.function_def(stmt, name, args, body)?;
            }
            S::Return { value } => {
                if !self.code.is_function {
                    return Err(CompileError {
                        kind: "SyntaxError",
                        msg: "'return' outside function".into(),
                        lineno: self.line,
                    });
                }
                match value {
                    Some(v) => self.expr(v)?,
                    None => {
                        let c = self.constant_none();
                        self.emit(Op::LoadConst(c));
                    }
                }
                self.line = stmt.pos.lineno;
                self.emit_return()?;
            }
            S::AsyncFunctionDef { .. } => return Err(self.unsupported("async def")),
            S::ClassDef { .. } => return Err(self.unsupported("class")),
            S::Import { names } => {
                for alias in names {
                    if alias.name.contains('.') && alias.asname.is_none() {
                        return Err(self.unsupported("dotted imports"));
                    }
                    let n = self.name(&alias.name);
                    self.emit(Op::Import(n));
                    let bound = alias.asname.clone().unwrap_or_else(|| alias.name.clone());
                    self.emit_store(&bound);
                }
            }
            S::ImportFrom { module, names, level } => {
                let Some(module) = module.as_ref().filter(|_| level.unwrap_or(0) == 0) else {
                    return Err(self.unsupported("relative import"));
                };
                for alias in names {
                    if alias.name == "*" {
                        return Err(self.unsupported("import *"));
                    }
                    let m = self.name(module);
                    self.emit(Op::Import(m));
                    let n = self.name(&alias.name);
                    self.emit(Op::ImportName(n));
                    let bound = alias.asname.clone().unwrap_or_else(|| alias.name.clone());
                    self.emit_store(&bound);
                }
            }
            S::TryStar { .. } => return Err(self.unsupported("except*")),
            S::Delete { .. } => return Err(self.unsupported("del")),
            S::With { .. } | S::AsyncWith { .. } => return Err(self.unsupported("with")),
            S::Match { .. } => return Err(self.unsupported("match")),
            S::AnnAssign { .. } | S::TypeAlias { .. } => return Err(self.unsupported("annotations")),
            S::Nonlocal { .. } => return Err(self.unsupported("nonlocal")),
            S::AsyncFor { .. } => return Err(self.unsupported("async for")),
        }
        Ok(())
    }

    /// `try` completo: o `finally` embrulha o `try/except/else` por fora, e o corpo dele é repetido
    /// no caminho normal, no excepcional e em cada `break`/`continue` que atravessa o `try`.
    fn try_stmt(
        &mut self,
        body: &[Stmt],
        handlers: &[ExceptHandler],
        orelse: &[Stmt],
        finalbody: &[Stmt],
    ) -> Result<(), CompileError> {
        if finalbody.is_empty() {
            return self.try_except(body, handlers, orelse);
        }
        let setup = self.emit(Op::SetupTry(0));
        self.tries.push(TryCtx { finalbody: finalbody.to_vec() });
        if handlers.is_empty() {
            self.block(body)?;
            self.block(orelse)?;
        } else {
            self.try_except(body, handlers, orelse)?;
        }
        self.tries.pop();
        self.emit(Op::PopBlock);
        self.block(finalbody)?;
        let to_end = self.emit(Op::Jump(0));
        let handler = self.here();
        self.patch(setup, handler);
        self.emit(Op::PushExc);
        self.block(finalbody)?;
        self.emit(Op::Reraise);
        let end = self.here();
        self.patch(to_end, end);
        Ok(())
    }

    fn try_except(&mut self, body: &[Stmt], handlers: &[ExceptHandler], orelse: &[Stmt]) -> Result<(), CompileError> {
        let setup = self.emit(Op::SetupTry(0));
        self.tries.push(TryCtx { finalbody: Vec::new() });
        self.block(body)?;
        self.tries.pop();
        self.emit(Op::PopBlock);
        self.block(orelse)?;
        let mut ends = vec![self.emit(Op::Jump(0))];
        let handler = self.here();
        self.patch(setup, handler);
        self.emit(Op::PushExc);
        let mut pending: Option<usize> = None;
        for h in handlers {
            if let Some(at) = pending.take() {
                let here = self.here();
                self.patch(at, here);
            }
            self.line = h.pos.lineno;
            if let Some(t) = &h.r#type {
                self.expr(t)?;
                self.line = h.pos.lineno;
                self.emit(Op::ExcMatch);
                pending = Some(self.emit(Op::PopJumpIfFalse(0)));
            }
            match &h.name {
                Some(name) => {
                    self.emit_store(name);
                }
                None => {
                    self.emit(Op::Pop);
                }
            }
            self.block(&h.body)?;
            if let Some(name) = &h.name {
                let n = self.name(name);
                self.emit(Op::DeleteName(n));
            }
            self.emit(Op::PopExc);
            ends.push(self.emit(Op::Jump(0)));
        }
        if let Some(at) = pending.take() {
            let here = self.here();
            self.patch(at, here);
            self.emit(Op::Reraise);
        }
        let end = self.here();
        for e in ends {
            self.patch(e, end);
        }
        Ok(())
    }

    /// Sai dos `try` abertos acima de `depth` (por `break`/`continue`): fecha o bloco de cada um e
    /// repete o `finally` dele.
    fn leave_tries(&mut self, depth: usize) -> Result<(), CompileError> {
        let all = std::mem::take(&mut self.tries);
        let mut result = Ok(());
        for i in (depth..all.len()).rev() {
            self.emit(Op::PopBlock);
            if all[i].finalbody.is_empty() {
                continue;
            }
            self.tries = all[..i].to_vec();
            if let Err(e) = self.block(&all[i].finalbody) {
                result = Err(e);
                break;
            }
        }
        self.tries = all;
        result
    }

    fn emit_load(&mut self, id: &str) {
        let n = self.name(id);
        let local = self.locals.as_ref().is_some_and(|l| l.contains(id));
        self.emit(if local { Op::LoadLocal(n) } else { Op::LoadName(n) });
    }

    fn emit_store(&mut self, id: &str) {
        let n = self.name(id);
        let local = self.locals.as_ref().is_some_and(|l| l.contains(id));
        self.emit(if local { Op::StoreLocal(n) } else { Op::StoreName(n) });
    }

    /// `def nome(a, b=1): ...`: compila o corpo num `Code` próprio (nomes atribuídos viram locais,
    /// salvo os declarados `global`) e guarda o nome da função no escopo atual.
    fn function_def(&mut self, stmt: &Stmt, name: &str, args: &Arguments, body: &[Stmt]) -> Result<(), CompileError> {
        if !args.posonlyargs.is_empty()
            || args.vararg.is_some()
            || !args.kwonlyargs.is_empty()
            || args.kwarg.is_some()
        {
            return Err(self.unsupported("this kind of parameter"));
        }
        let params: Vec<String> = args.args.iter().map(|a| a.arg.clone()).collect();
        let mut locals: HashSet<String> = params.iter().cloned().collect();
        let mut globals: HashSet<String> = HashSet::new();
        collect_scope(body, &mut locals, &mut globals);
        for g in &globals {
            locals.remove(g);
        }
        let mut inner = Compiler {
            code: Code { name: name.to_string(), params: params.clone(), is_function: true, ..Code::default() },
            line: stmt.pos.lineno,
            loops: Vec::new(),
            name_index: HashMap::new(),
            tries: Vec::new(),
            locals: Some(locals),
        };
        inner.block(body)?;
        let c = inner.constant_none();
        inner.emit(Op::LoadConst(c));
        inner.emit(Op::Return);
        let code = Rc::new(inner.code);
        self.line = stmt.pos.lineno;
        for d in &args.defaults {
            self.expr(d)?;
        }
        self.line = stmt.pos.lineno;
        self.code.functions.push(code);
        let idx = (self.code.functions.len() - 1) as u32;
        self.emit(Op::MakeFunction { code: idx, ndefaults: args.defaults.len() as u32 });
        self.emit_store(name);
        Ok(())
    }

    fn constant_none(&mut self) -> u32 {
        self.constant(Value::None)
    }

    /// `return` com o valor já na pilha: fecha os `try` abertos (repetindo os `finally`).
    fn emit_return(&mut self) -> Result<(), CompileError> {
        self.leave_tries(0)?;
        self.emit(Op::Return);
        Ok(())
    }

    fn aug_assign(&mut self, target: &Expr, op: Operator, value: &Expr) -> Result<(), CompileError> {
        match &target.kind {
            E::Name { id, .. } => {
                self.emit_load(id);
                self.expr(value)?;
                self.emit(Op::Binary { op, inplace: true });
                self.emit_store(id);
            }
            E::Subscript { value: container, slice, .. } => {
                self.expr(container)?;
                self.expr(slice)?;
                self.emit(Op::Dup2);
                self.emit(Op::Subscript);
                self.expr(value)?;
                self.emit(Op::Binary { op, inplace: true });
                self.emit(Op::Rot3);
                self.emit(Op::StoreSubscript);
            }
            _ => return Err(self.unsupported("this augmented assignment target")),
        }
        Ok(())
    }

    /// Guarda o topo da pilha no alvo.
    fn store(&mut self, target: &Expr) -> Result<(), CompileError> {
        let saved = self.line;
        self.line = target.pos.lineno;
        match &target.kind {
            E::Name { id, .. } => {
                self.emit_store(id);
            }
            E::Subscript { value, slice, .. } => {
                self.expr(value)?;
                self.expr(slice)?;
                self.emit(Op::StoreSubscript);
            }
            E::Tuple { elts, .. } | E::List { elts, .. } => {
                if elts.iter().any(|e| matches!(e.kind, E::Starred { .. })) {
                    return Err(self.unsupported("starred assignment"));
                }
                self.emit(Op::UnpackSequence(elts.len() as u32));
                for elt in elts {
                    self.store(elt)?;
                }
            }
            _ => return Err(self.unsupported("this assignment target")),
        }
        self.line = saved;
        Ok(())
    }

    fn expr(&mut self, expr: &Expr) -> Result<(), CompileError> {
        let saved = self.line;
        self.line = expr.pos.lineno;
        self.expr_inner(expr)?;
        self.line = saved;
        Ok(())
    }

    fn exprs(&mut self, exprs: &[Expr]) -> Result<(), CompileError> {
        if exprs.iter().any(|e| matches!(e.kind, E::Starred { .. })) {
            return Err(self.unsupported("starred expressions"));
        }
        exprs.iter().try_for_each(|e| self.expr(e))
    }

    fn expr_inner(&mut self, expr: &Expr) -> Result<(), CompileError> {
        match &expr.kind {
            E::Constant { value, .. } => {
                let v = self.constant_value(value)?;
                let i = self.constant(v);
                self.emit(Op::LoadConst(i));
            }
            E::Name { id, .. } => {
                self.emit_load(id);
            }
            E::BinOp { left, op, right } => {
                self.expr(left)?;
                self.expr(right)?;
                self.emit(Op::Binary { op: *op, inplace: false });
            }
            E::UnaryOp { op, operand } => {
                self.expr(operand)?;
                self.emit(Op::Unary(*op));
            }
            E::BoolOp { op, values } => {
                let mut jumps = Vec::new();
                for (i, value) in values.iter().enumerate() {
                    self.expr(value)?;
                    if i + 1 < values.len() {
                        jumps.push(self.emit(match op {
                            BoolOp::And => Op::JumpIfFalseOrPop(0),
                            BoolOp::Or => Op::JumpIfTrueOrPop(0),
                        }));
                    }
                }
                let end = self.here();
                for j in jumps {
                    self.patch(j, end);
                }
            }
            E::IfExp { test, body, orelse } => {
                self.expr(test)?;
                let to_else = self.emit(Op::PopJumpIfFalse(0));
                self.expr(body)?;
                let to_end = self.emit(Op::Jump(0));
                let else_start = self.here();
                self.patch(to_else, else_start);
                self.expr(orelse)?;
                let end = self.here();
                self.patch(to_end, end);
            }
            E::Compare { left, ops, comparators } => self.compare(left, ops, comparators)?,
            E::Call { func, args, keywords } => {
                self.expr(func)?;
                self.exprs(args)?;
                let mut names = Vec::new();
                for kw in keywords {
                    let Some(name) = &kw.arg else { return Err(self.unsupported("**kwargs in calls")) };
                    names.push(Value::str(name.clone()));
                    self.expr(&kw.value)?;
                }
                let kwnames = if names.is_empty() { None } else { Some(self.constant(Value::tuple(names))) };
                self.line = expr.pos.lineno;
                self.emit(Op::Call { argc: (args.len() + keywords.len()) as u32, kwnames });
            }
            E::List { elts, .. } => {
                self.exprs(elts)?;
                self.emit(Op::BuildList(elts.len() as u32));
            }
            E::Tuple { elts, .. } => {
                self.exprs(elts)?;
                self.emit(Op::BuildTuple(elts.len() as u32));
            }
            E::Set { elts } => {
                self.exprs(elts)?;
                self.emit(Op::BuildSet(elts.len() as u32));
            }
            E::Dict { keys, values } => {
                for (key, value) in keys.iter().zip(values) {
                    let Some(key) = key else { return Err(self.unsupported("** in dict displays")) };
                    self.expr(key)?;
                    self.expr(value)?;
                }
                self.emit(Op::BuildDict(values.len() as u32));
            }
            E::Subscript { value, slice, .. } => {
                if matches!(slice.kind, E::Slice { .. }) {
                    return Err(self.unsupported("slicing"));
                }
                self.expr(value)?;
                self.expr(slice)?;
                self.line = expr.pos.lineno;
                self.emit(Op::Subscript);
            }
            E::Attribute { value, attr, .. } => {
                self.expr(value)?;
                let n = self.name(attr);
                self.line = expr.pos.lineno;
                self.emit(Op::LoadAttr(n));
            }
            E::Lambda { .. } => return Err(self.unsupported("lambda")),
            E::NamedExpr { .. } => return Err(self.unsupported("assignment expressions")),
            E::ListComp { .. } | E::SetComp { .. } | E::DictComp { .. } | E::GeneratorExp { .. } => {
                return Err(self.unsupported("comprehensions"))
            }
            E::JoinedStr { .. } | E::FormattedValue { .. } => return Err(self.unsupported("f-strings")),
            E::Await { .. } | E::Yield { .. } | E::YieldFrom { .. } => return Err(self.unsupported("generators")),
            E::Starred { .. } => return Err(self.unsupported("starred expressions")),
            E::Slice { .. } => return Err(self.unsupported("slicing")),
        }
        Ok(())
    }

    /// `a op1 b op2 c`: cada operando é avaliado uma vez, e a cadeia para no primeiro falso.
    fn compare(&mut self, left: &Expr, ops: &[CmpOp], comparators: &[Expr]) -> Result<(), CompileError> {
        self.expr(left)?;
        if ops.len() == 1 {
            self.expr(&comparators[0])?;
            self.emit(Op::Compare(ops[0]));
            return Ok(());
        }
        let mut cleanups = Vec::new();
        for (i, (op, right)) in ops.iter().zip(comparators).enumerate() {
            self.expr(right)?;
            if i + 1 < ops.len() {
                self.emit(Op::Dup);
                self.emit(Op::Rot3);
                self.emit(Op::Compare(*op));
                cleanups.push(self.emit(Op::JumpIfFalseOrPop(0)));
            } else {
                self.emit(Op::Compare(*op));
            }
        }
        let to_end = self.emit(Op::Jump(0));
        let cleanup = self.here();
        for c in cleanups {
            self.patch(c, cleanup);
        }
        self.emit(Op::Rot2);
        self.emit(Op::Pop);
        let end = self.here();
        self.patch(to_end, end);
        Ok(())
    }

    fn constant_value(&self, c: &Constant) -> Result<Value, CompileError> {
        Ok(match c {
            Constant::None => Value::None,
            Constant::Bool(b) => Value::Bool(*b),
            // Até o `int` arbitrário da fatia 19, literal fora de `i64` não compila.
            Constant::Int(digits) => match digits.parse::<i64>() {
                Ok(i) => Value::Int(i),
                Err(_) => return Err(self.unsupported("integer literals outside the 64-bit range")),
            },
            Constant::Float(x) => Value::Float(*x),
            Constant::Str(s) => Value::str(s.clone()),
            Constant::Bytes(b) => Value::bytes(b.clone()),
            Constant::Complex(..) => return Err(self.unsupported("complex numbers")),
            Constant::Ellipsis => return Err(self.unsupported("Ellipsis")),
        })
    }
}
