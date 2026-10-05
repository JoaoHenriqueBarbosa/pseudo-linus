//! Máquina virtual de pilha que executa o bytecode de `compile` (fatia 10 de
//! `docs/python3-port.md`).
//!
//! A semântica dos operadores segue o CPython 3.13 sobre os tipos de `object`: `int` com divisão
//! inteira arredondando para baixo, `%` com o sinal do divisor, `**` com expoente negativo virando
//! `float`; `float` com o `float_divmod` do `Objects/floatobject.c`; concatenação e repetição de
//! sequências; comparações de ordem lexicográficas em `list`/`tuple`; e as mensagens de `TypeError`,
//! `ZeroDivisionError`, `IndexError`, `KeyError`, `NameError` e `ValueError` iguais às do CPython.
//!
//! Limitações conhecidas desta fatia:
//! - `int` é `i64` (ver `object::int`); resultado fora da faixa vira `OverflowError` com mensagem
//!   própria até a fatia 19 trazer o inteiro arbitrário. `int / int` converte os operandos para
//!   `double`, o que só difere do CPython (que arredonda corretamente) acima de 2**53.
//! - A saída do `print` vai para `Vm::stdout`, descarregado pelo chamador no fim, como o buffer de
//!   bloco do stdout do CPython quando ele não é um terminal; `file=` fica para a fatia 13.
//! - O traceback é a forma simples (sem a linha fonte nem os marcadores), refinada na fatia 11.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{CmpOp, Operator, UnaryOp};
use crate::compile::{Code, Op};
use crate::modules::{csv, json};
use crate::object::{
    exc_is_subclass, exc_str, int_add, int_mul, int_neg, int_sub, is, py_eq, repr, to_str, BoundMethod, Dict, ExcObj,
    FileKind, FuncObj, Native, ObjError, PyFile, PyStr, Range, Set, Value, EXC_CLASSES,
};

/// Exceção Python levantada durante a execução: o nome da classe e a mensagem (`str(exc)`). Quando
/// ela vem de um `raise` ou foi capturada, `value` guarda a instância com os `args` originais.
#[derive(Debug, Clone)]
pub struct PyException {
    pub kind: &'static str,
    pub msg: String,
    pub value: Option<Value>,
    /// Quadros que a exceção atravessou sem tratamento: linha e nome do código, do mais interno
    /// para o mais externo.
    pub tb: Vec<(usize, String)>,
}

impl PyException {
    /// A instância que `except ... as e` enxerga.
    fn to_value(&self) -> Value {
        if let Some(v) = &self.value {
            return v.clone();
        }
        let args = if self.msg.is_empty() { Vec::new() } else { vec![Value::str(self.msg.clone())] };
        Value::Exception(Rc::new(ExcObj { kind: self.kind, args }))
    }

    fn from_value(v: &Value) -> PyException {
        match v {
            Value::Exception(e) => {
                PyException { kind: e.kind, msg: exc_str(e), value: Some(v.clone()), tb: Vec::new() }
            }
            _ => type_error("exceptions must derive from BaseException"),
        }
    }
}

/// Exceção não tratada com a linha da instrução que a levantou.
#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub exc: PyException,
    pub lineno: usize,
}

type PyResult<T> = Result<T, PyException>;

fn exc(kind: &'static str, msg: impl Into<String>) -> PyException {
    PyException { kind, msg: msg.into(), value: None, tb: Vec::new() }
}

fn type_error(msg: impl Into<String>) -> PyException {
    exc("TypeError", msg)
}

impl From<ObjError> for PyException {
    fn from(e: ObjError) -> PyException {
        match e {
            ObjError::TypeError(msg) => type_error(msg),
            ObjError::IntOverflow => {
                exc("OverflowError", "integer result outside the 64-bit range (arbitrary int is pending)")
            }
        }
    }
}

/// Traceback do CPython para exceção de um `-c`, sem a linha fonte.
pub fn format_traceback(err: &RuntimeError) -> String {
    format_traceback_in(err, "<string>", None)
}

/// Traceback com o nome do arquivo; com `src` (execução de arquivo) cada quadro mostra a linha fonte
/// sem a indentação, como o CPython faz fora do `-c`.
pub fn format_traceback_in(err: &RuntimeError, file: &str, src: Option<&str>) -> String {
    let mut out = String::from("Traceback (most recent call last):\n");
    let frame = |out: &mut String, line: usize, name: &str| {
        out.push_str(&format!("  File \"{file}\", line {line}, in {name}\n"));
        if let Some(text) = src.and_then(|s| s.lines().nth(line.saturating_sub(1))) {
            let t = text.trim();
            if !t.is_empty() {
                out.push_str(&format!("    {t}\n"));
            }
        }
    };
    if err.exc.tb.is_empty() {
        frame(&mut out, err.lineno, "<module>");
    }
    for (line, name) in err.exc.tb.iter().rev() {
        frame(&mut out, *line, name);
    }
    if err.exc.msg.is_empty() {
        out.push_str(err.exc.kind);
        out.push('\n');
    } else {
        out.push_str(&format!("{}: {}\n", err.exc.kind, err.exc.msg));
    }
    out
}

/// Funções embutidas desta fatia.
const BUILTINS: &[&str] = &[
    "print", "len", "range", "str", "int", "repr", "open", "list", "tuple", "bool", "float", "abs", "min",
    "max", "sum", "sorted", "reversed", "enumerate", "zip", "any", "all", "ord", "chr",
];

/// Iterador de um laço `for`, que vive na pilha da VM e não é um `Value`.
enum PyIter {
    /// O iterador de `list` relê a lista a cada passo, como o `listiter_next` (mudanças no laço
    /// são vistas).
    List(Rc<std::cell::RefCell<Vec<Value>>>, usize),
    Tuple(Rc<[Value]>, usize),
    /// Posição em bytes dentro do texto.
    Str(Rc<PyStr>, usize),
    Range { next: i64, step: i64, remaining: i64 },
    /// Cópia dos itens (chaves de `dict`, elementos de `set`, bytes de `bytes`).
    Items(Vec<Value>, usize),
    /// Arquivo (uma linha por passo) ou leitor de `csv` (uma lista de campos por passo).
    Native(Rc<RefCell<Native>>),
}

impl PyIter {
    fn next(&mut self) -> PyResult<Option<Value>> {
        Ok(match self {
            PyIter::List(items, i) => {
                let Some(v) = items.borrow().get(*i).cloned() else { return Ok(None) };
                *i += 1;
                Some(v)
            }
            PyIter::Tuple(items, i) => {
                let Some(v) = items.get(*i).cloned() else { return Ok(None) };
                *i += 1;
                Some(v)
            }
            PyIter::Items(items, i) => {
                let Some(v) = items.get(*i).cloned() else { return Ok(None) };
                *i += 1;
                Some(v)
            }
            PyIter::Str(s, pos) => {
                let Some(c) = s.as_str()[*pos..].chars().next() else { return Ok(None) };
                *pos += c.len_utf8();
                Some(Value::str(c.to_string()))
            }
            PyIter::Range { next, step, remaining } => {
                if *remaining <= 0 {
                    return Ok(None);
                }
                let v = *next;
                *remaining -= 1;
                if *remaining > 0 {
                    *next += *step;
                }
                Some(Value::Int(v))
            }
            PyIter::Native(n) => native_next(n)?,
        })
    }
}

fn get_iter(v: &Value) -> PyResult<PyIter> {
    Ok(match v {
        Value::List(l) => PyIter::List(l.clone(), 0),
        Value::Tuple(t) => PyIter::Tuple(t.clone(), 0),
        Value::Str(s) => PyIter::Str(s.clone(), 0),
        Value::Range(r) => PyIter::Range { next: r.start, step: r.step, remaining: r.len() },
        Value::Dict(d) => PyIter::Items(d.borrow().keys().cloned().collect(), 0),
        Value::Set(s) => PyIter::Items(s.borrow().iter().cloned().collect(), 0),
        Value::Bytes(b) => PyIter::Items(b.iter().map(|&x| Value::Int(i64::from(x))).collect(), 0),
        Value::Native(n) if matches!(&*n.borrow(), Native::File(_) | Native::CsvReader { .. }) => {
            PyIter::Native(n.clone())
        }
        _ => return Err(type_error(format!("'{}' object is not iterable", v.type_name()))),
    })
}

/// Todos os itens de um iterável.
fn collect(v: &Value) -> PyResult<Vec<Value>> {
    let mut it = get_iter(v)?;
    let mut out = Vec::new();
    while let Some(x) = it.next()? {
        out.push(x);
    }
    Ok(out)
}

/// Elemento da pilha.
enum Slot {
    Val(Value),
    Iter(PyIter),
}

/// Estado do interpretador: variáveis globais e o buffer do stdout.
pub struct Vm {
    globals: HashMap<String, Value>,
    pub stdout: Vec<u8>,
    /// Exceções sendo tratadas (a mais recente por último), para `raise` sem argumento.
    handled: Vec<Value>,
    /// Profundidade de chamadas de função em andamento.
    depth: usize,
    /// `sys.argv`.
    argv: Vec<String>,
    /// `sys.stdin`, `sys.stdout` e `sys.stderr`, criados uma vez.
    std_files: [Rc<RefCell<Native>>; 3],
}

/// Limite de recursão (`sys.getrecursionlimit()` do CPython).
const MAX_DEPTH: usize = 1000;

/// Bloco protegido aberto por `SetupTry`.
struct Block {
    handler: usize,
    depth: usize,
    handled: usize,
}

fn internal(msg: &str) -> PyException {
    exc("SystemError", msg.to_string())
}

impl Default for Vm {
    fn default() -> Vm {
        Vm::new()
    }
}

impl Vm {
    pub fn new() -> Vm {
        Vm::with_argv(Vec::new())
    }

    pub fn with_argv(argv: Vec<String>) -> Vm {
        let file = |kind, name: &str| {
            Rc::new(RefCell::new(Native::File(PyFile {
                kind,
                lines: Vec::new(),
                pos: 0,
                loaded: !matches!(kind, FileKind::Stdin),
                closed: false,
                name: name.to_string(),
            })))
        };
        Vm {
            globals: HashMap::new(),
            stdout: Vec::new(),
            handled: Vec::new(),
            depth: 0,
            argv,
            std_files: [file(FileKind::Stdin, "<stdin>"), file(FileKind::Stdout, "<stdout>"), file(FileKind::Stderr, "<stderr>")],
        }
    }

    /// Executa o código de um módulo.
    pub fn run(&mut self, code: &Code) -> Result<(), RuntimeError> {
        let mut locals = HashMap::new();
        match self.exec(code, &mut locals) {
            Ok(_) => Ok(()),
            Err(e) => Err(RuntimeError { lineno: e.tb.last().map_or(0, |t| t.0), exc: e }),
        }
    }

    /// Executa o código de um módulo ou de uma função até o `Return` (ou o fim do módulo).
    fn exec(&mut self, code: &Code, locals: &mut HashMap<String, Value>) -> PyResult<Value> {
        let mut stack: Vec<Slot> = Vec::new();
        let mut blocks: Vec<Block> = Vec::new();
        let mut pc = 0;
        while pc < code.ops.len() {
            let op = code.ops[pc];
            let result = match op {
                Op::SetupTry(h) => {
                    blocks.push(Block { handler: h as usize, depth: stack.len(), handled: self.handled.len() });
                    Ok(None)
                }
                Op::PopBlock => {
                    blocks.pop();
                    Ok(None)
                }
                Op::Return => match stack.pop() {
                    Some(Slot::Val(v)) => return Ok(v),
                    _ => Err(internal("bad value stack")),
                },
                _ => self.step(code, op, &mut stack, locals),
            };
            match result {
                Ok(Some(target)) => pc = target,
                Ok(None) => pc += 1,
                Err(mut e) => match blocks.pop() {
                    Some(b) => {
                        stack.truncate(b.depth);
                        self.handled.truncate(b.handled);
                        stack.push(Slot::Val(e.to_value()));
                        pc = b.handler;
                    }
                    None => {
                        e.tb.push((code.lines[pc], code.name.clone()));
                        return Err(e);
                    }
                },
            }
        }
        Ok(Value::None)
    }

    fn call_function(&mut self, f: &Rc<FuncObj>, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
        let code = f.code.clone();
        let name = code.name.as_str();
        let params = &code.params;
        let ndefaults = f.defaults.len();
        let required = params.len() - ndefaults;
        if args.len() > params.len() {
            let takes = if ndefaults == 0 {
                format!("{}", params.len())
            } else {
                format!("from {required} to {}", params.len())
            };
            let plural = if ndefaults == 0 && params.len() == 1 { "" } else { "s" };
            let given = if args.len() == 1 { "was" } else { "were" };
            return Err(type_error(format!(
                "{name}() takes {takes} positional argument{plural} but {} {given} given",
                args.len()
            )));
        }
        let mut locals: HashMap<String, Value> = HashMap::new();
        for (p, a) in params.iter().zip(args) {
            locals.insert(p.clone(), a);
        }
        for (k, v) in kwargs {
            if !params.contains(&k) {
                return Err(type_error(format!("{name}() got an unexpected keyword argument '{k}'")));
            }
            if locals.contains_key(&k) {
                return Err(type_error(format!("{name}() got multiple values for argument '{k}'")));
            }
            locals.insert(k, v);
        }
        let mut missing: Vec<&String> = Vec::new();
        for (i, p) in params.iter().enumerate() {
            if !locals.contains_key(p) {
                if i >= required {
                    locals.insert(p.clone(), f.defaults[i - required].clone());
                } else {
                    missing.push(p);
                }
            }
        }
        if !missing.is_empty() {
            let quoted: Vec<String> = missing.iter().map(|m| format!("'{m}'")).collect();
            let list = match quoted.as_slice() {
                [one] => one.clone(),
                [a, b] => format!("{a} and {b}"),
                many => format!("{}, and {}", many[..many.len() - 1].join(", "), many[many.len() - 1]),
            };
            let plural = if missing.len() == 1 { "" } else { "s" };
            return Err(type_error(format!("{name}() missing {} required positional argument{plural}: {list}", missing.len())));
        }
        if self.depth >= MAX_DEPTH {
            return Err(exc("RecursionError", "maximum recursion depth exceeded"));
        }
        self.depth += 1;
        let result = self.exec(&code, &mut locals);
        self.depth -= 1;
        result
    }

    /// Executa uma instrução; `Some(alvo)` quando ela salta.
    fn step(
        &mut self,
        code: &Code,
        op: Op,
        stack: &mut Vec<Slot>,
        locals: &mut HashMap<String, Value>,
    ) -> PyResult<Option<usize>> {
        fn pop(stack: &mut Vec<Slot>) -> PyResult<Value> {
            match stack.pop() {
                Some(Slot::Val(v)) => Ok(v),
                _ => Err(internal("bad value stack")),
            }
        }
        fn pop_n(stack: &mut Vec<Slot>, n: usize) -> PyResult<Vec<Value>> {
            let mut items = Vec::with_capacity(n);
            for _ in 0..n {
                items.push(pop(stack)?);
            }
            items.reverse();
            Ok(items)
        }
        fn top(stack: &[Slot]) -> PyResult<&Value> {
            match stack.last() {
                Some(Slot::Val(v)) => Ok(v),
                _ => Err(internal("bad value stack")),
            }
        }
        match op {
            Op::LoadConst(i) => stack.push(Slot::Val(code.consts[i as usize].clone())),
            Op::LoadName(i) => {
                let name = &code.names[i as usize];
                let v = match self.globals.get(name) {
                    Some(v) => v.clone(),
                    None => match BUILTINS.iter().find(|b| **b == name.as_str()) {
                        Some(b) => Value::Builtin(b),
                        None => match EXC_CLASSES.iter().find(|(n, _)| *n == name.as_str()) {
                            Some((n, _)) => Value::Builtin(n),
                            None => return Err(exc("NameError", format!("name '{name}' is not defined"))),
                        },
                    },
                };
                stack.push(Slot::Val(v));
            }
            Op::StoreName(i) => {
                let v = pop(stack)?;
                self.globals.insert(code.names[i as usize].clone(), v);
            }
            Op::Pop => {
                stack.pop();
            }
            Op::Dup => {
                let v = top(stack)?.clone();
                stack.push(Slot::Val(v));
            }
            Op::Dup2 => {
                let b = pop(stack)?;
                let a = pop(stack)?;
                for v in [a.clone(), b.clone(), a, b] {
                    stack.push(Slot::Val(v));
                }
            }
            Op::Rot2 => {
                let n = stack.len();
                if n < 2 {
                    return Err(internal("bad value stack"));
                }
                stack.swap(n - 1, n - 2);
            }
            Op::Rot3 => {
                let n = stack.len();
                if n < 3 {
                    return Err(internal("bad value stack"));
                }
                stack[n - 3..].rotate_right(1);
            }
            Op::Binary { op, inplace } => {
                let b = pop(stack)?;
                let a = pop(stack)?;
                stack.push(Slot::Val(binary(op, &a, &b, inplace)?));
            }
            Op::Unary(op) => {
                let a = pop(stack)?;
                stack.push(Slot::Val(unary(op, &a)?));
            }
            Op::Compare(op) => {
                let b = pop(stack)?;
                let a = pop(stack)?;
                stack.push(Slot::Val(Value::Bool(compare(op, &a, &b)?)));
            }
            Op::Jump(t) => return Ok(Some(t as usize)),
            Op::PopJumpIfFalse(t) => {
                if !pop(stack)?.is_true() {
                    return Ok(Some(t as usize));
                }
            }
            Op::PopJumpIfTrue(t) => {
                if pop(stack)?.is_true() {
                    return Ok(Some(t as usize));
                }
            }
            Op::JumpIfFalseOrPop(t) => {
                if !top(stack)?.is_true() {
                    return Ok(Some(t as usize));
                }
                stack.pop();
            }
            Op::JumpIfTrueOrPop(t) => {
                if top(stack)?.is_true() {
                    return Ok(Some(t as usize));
                }
                stack.pop();
            }
            Op::GetIter => {
                let v = pop(stack)?;
                stack.push(Slot::Iter(get_iter(&v)?));
            }
            Op::ForIter(t) => {
                let next = match stack.last_mut() {
                    Some(Slot::Iter(it)) => it.next()?,
                    _ => return Err(internal("FOR_ITER without iterator")),
                };
                match next {
                    Some(v) => stack.push(Slot::Val(v)),
                    None => {
                        stack.pop();
                        return Ok(Some(t as usize));
                    }
                }
            }
            Op::Call { argc, kwnames } => {
                let mut values = pop_n(stack, argc as usize)?;
                let func = pop(stack)?;
                let names: Vec<String> = match kwnames {
                    Some(i) => match &code.consts[i as usize] {
                        Value::Tuple(t) => t.iter().map(to_str).collect(),
                        _ => Vec::new(),
                    },
                    None => Vec::new(),
                };
                let kw_values = values.split_off(values.len() - names.len());
                let kwargs: Vec<(String, Value)> = names.into_iter().zip(kw_values).collect();
                let result = self.call(&func, values, kwargs)?;
                stack.push(Slot::Val(result));
            }
            Op::BuildList(n) => {
                let items = pop_n(stack, n as usize)?;
                stack.push(Slot::Val(Value::list(items)));
            }
            Op::BuildTuple(n) => {
                let items = pop_n(stack, n as usize)?;
                stack.push(Slot::Val(Value::tuple(items)));
            }
            Op::BuildSet(n) => {
                let mut set = Set::new();
                for item in pop_n(stack, n as usize)? {
                    set.add(item)?;
                }
                stack.push(Slot::Val(Value::set(set)));
            }
            Op::BuildDict(n) => {
                let items = pop_n(stack, 2 * n as usize)?;
                let mut d = Dict::new();
                for pair in items.chunks(2) {
                    d.set(pair[0].clone(), pair[1].clone())?;
                }
                stack.push(Slot::Val(Value::dict(d)));
            }
            Op::Subscript => {
                let index = pop(stack)?;
                let container = pop(stack)?;
                stack.push(Slot::Val(subscript(&container, &index)?));
            }
            Op::StoreSubscript => {
                let index = pop(stack)?;
                let container = pop(stack)?;
                let value = pop(stack)?;
                store_subscript(&container, &index, value)?;
            }
            Op::SetupTry(_) | Op::PopBlock | Op::Return => {}
            Op::LoadLocal(i) => {
                let name = &code.names[i as usize];
                match locals.get(name) {
                    Some(v) => stack.push(Slot::Val(v.clone())),
                    None => {
                        return Err(exc(
                            "UnboundLocalError",
                            format!("cannot access local variable '{name}' where it is not associated with a value"),
                        ))
                    }
                }
            }
            Op::StoreLocal(i) => {
                let v = pop(stack)?;
                locals.insert(code.names[i as usize].clone(), v);
            }
            Op::MakeFunction { code: idx, ndefaults } => {
                let defaults = pop_n(stack, ndefaults as usize)?;
                let f = FuncObj { code: code.functions[idx as usize].clone(), defaults };
                stack.push(Slot::Val(Value::Function(Rc::new(f))));
            }
            Op::PushExc => {
                let v = top(stack)?.clone();
                self.handled.push(v);
            }
            Op::PopExc => {
                self.handled.pop();
            }
            Op::ExcMatch => {
                let cls = pop(stack)?;
                let matched = {
                    let Value::Exception(e) = top(stack)? else { return Err(internal("ExcMatch without exception")) };
                    exc_matches(e.kind, &cls)?
                };
                stack.push(Slot::Val(Value::Bool(matched)));
            }
            Op::Raise => {
                let v = pop(stack)?;
                return Err(raise_value(v)?);
            }
            Op::ReraiseCurrent => {
                let Some(v) = self.handled.last().cloned() else {
                    return Err(exc("RuntimeError", "No active exception to reraise"));
                };
                return Err(PyException::from_value(&v));
            }
            Op::Reraise => {
                let v = pop(stack)?;
                self.handled.pop();
                return Err(PyException::from_value(&v));
            }
            Op::DeleteName(i) => {
                let name = &code.names[i as usize];
                if code.is_function {
                    locals.remove(name);
                } else {
                    self.globals.remove(name);
                }
            }
            Op::Import(i) => {
                let name = &code.names[i as usize];
                let m = match name.as_str() {
                    "sys" => "sys",
                    "csv" => "csv",
                    "json" => "json",
                    _ => return Err(exc("ModuleNotFoundError", format!("No module named '{name}'"))),
                };
                stack.push(Slot::Val(Value::Module(m)));
            }
            Op::ImportName(i) => {
                let obj = pop(stack)?;
                let name = &code.names[i as usize];
                match self.load_attr(&obj, name) {
                    Ok(v) => stack.push(Slot::Val(v)),
                    Err(_) => {
                        let module = match &obj {
                            Value::Module(m) => *m,
                            _ => "?",
                        };
                        return Err(exc(
                            "ImportError",
                            format!("cannot import name '{name}' from '{module}' (unknown location)"),
                        ));
                    }
                }
            }
            Op::LoadAttr(i) => {
                let obj = pop(stack)?;
                let name = &code.names[i as usize];
                let v = self.load_attr(&obj, name)?;
                stack.push(Slot::Val(v));
            }
            Op::UnpackSequence(n) => {
                let v = pop(stack)?;
                let n = n as usize;
                let items = collect(&v)?;
                let known_len = matches!(v, Value::List(_) | Value::Tuple(_));
                if items.len() < n {
                    return Err(exc(
                        "ValueError",
                        format!("not enough values to unpack (expected {n}, got {})", items.len()),
                    ));
                }
                if items.len() > n {
                    let msg = if known_len {
                        format!("too many values to unpack (expected {n}, got {})", items.len())
                    } else {
                        format!("too many values to unpack (expected {n})")
                    };
                    return Err(exc("ValueError", msg));
                }
                for item in items.into_iter().rev() {
                    stack.push(Slot::Val(item));
                }
            }
        }
        Ok(None)
    }

    fn call(&mut self, func: &Value, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
        if let Value::Function(f) = func {
            return self.call_function(f, args, kwargs);
        }
        if let Value::Bound(b) = func {
            return self.call_method(&b.recv, b.name, args, kwargs);
        }
        let Value::Builtin(name) = func else {
            return Err(type_error(format!("'{}' object is not callable", func.type_name())));
        };
        let name = *name;
        if let Some((kind, _)) = EXC_CLASSES.iter().find(|(n, _)| *n == name) {
            if let Some((kw, _)) = kwargs.first() {
                return Err(type_error(format!("{name}() takes no keyword arguments ('{kw}' given)")));
            }
            return Ok(Value::Exception(Rc::new(ExcObj { kind, args })));
        }
        if !matches!(name, "print" | "open" | "csv.reader" | "csv.writer" | "json.dumps" | "sorted" | "enumerate")
            && let Some((kw, _)) = kwargs.first() {
                return Err(type_error(match name {
                    "range" | "len" | "repr" | "json.loads" => format!("{name}() takes no keyword arguments"),
                    _ => format!("'{kw}' is an invalid keyword argument for {name}()"),
                }));
            }
        match name {
            "print" => self.print(args, kwargs),
            "open" => self.open(args, kwargs),
            "csv.reader" | "csv.writer" => self.csv_open(name, args, kwargs),
            "json.dumps" => {
                let mut ensure_ascii = true;
                for (k, v) in &kwargs {
                    match k.as_str() {
                        "ensure_ascii" => ensure_ascii = v.is_true(),
                        _ => return Err(type_error(format!("dumps() got an unexpected keyword argument '{k}'"))),
                    }
                }
                let [v] = one_arg(name, args)?;
                match json::dumps(&v, ensure_ascii) {
                    Ok(s) => Ok(Value::str(s)),
                    Err(m) if m.starts_with("Object of type") => Err(type_error(m)),
                    Err(m) => Err(exc("ValueError", m)),
                }
            }
            "json.loads" => {
                let [v] = one_arg(name, args)?;
                let Value::Str(s) = &v else {
                    return Err(type_error(format!(
                        "the JSON object must be str, bytes or bytearray, not {}",
                        v.type_name()
                    )));
                };
                json::loads(s.as_str()).map_err(|e| exc("json.decoder.JSONDecodeError", e.msg))
            }
            "len" => {
                let [v] = one_arg(name, args)?;
                Ok(Value::Int(len(&v)?))
            }
            "repr" => {
                let [v] = one_arg(name, args)?;
                Ok(Value::str(repr(&v)))
            }
            "str" => match args.len() {
                0 => Ok(Value::str("")),
                1 => Ok(Value::str(to_str(&args[0]))),
                n => Err(type_error(format!("str() takes at most 1 argument ({n} given)"))),
            },
            "int" => match args.len() {
                0 => Ok(Value::Int(0)),
                1 => int_of(&args[0]),
                n => Err(type_error(format!("int() takes at most 2 arguments ({n} given)"))),
            },
            "range" => range_of(&args),
            "list" | "tuple" | "bool" | "float" | "abs" | "min" | "max" | "sum" | "sorted" | "reversed"
            | "enumerate" | "zip" | "any" | "all" | "ord" | "chr" => builtin_seq(name, args, kwargs),
            _ => Err(type_error(format!("'{}' object is not callable", func.type_name()))),
        }
    }

    /// `print(*args, sep=' ', end='\n', file=None, flush=False)`.
    fn print(&mut self, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
        let mut sep = " ".to_string();
        let mut end = "\n".to_string();
        let mut file: Option<Value> = None;
        for (name, value) in kwargs {
            match name.as_str() {
                "sep" | "end" => {
                    let text = match &value {
                        Value::None => None,
                        Value::Str(s) => Some(s.as_str().to_string()),
                        other => {
                            return Err(type_error(format!(
                                "{name} must be None or a string, not {}",
                                other.type_name()
                            )))
                        }
                    };
                    if let Some(text) = text {
                        if name == "sep" {
                            sep = text;
                        } else {
                            end = text;
                        }
                    }
                }
                "file" if matches!(value, Value::None) => {}
                "file" => file = Some(value),
                "flush" => {}
                _ => return Err(type_error(format!("'{name}' is an invalid keyword argument for print()"))),
            }
        }
        let parts: Vec<String> = args.iter().map(to_str).collect();
        let text = format!("{}{}", parts.join(&sep), end);
        match file {
            Some(f) => {
                self.write_to(&f, &text)?;
            }
            None => self.stdout.extend_from_slice(text.as_bytes()),
        }
        Ok(Value::None)
    }

    /// `arquivo.write(texto)`: devolve a quantidade de caracteres.
    fn write_to(&mut self, target: &Value, text: &str) -> PyResult<usize> {
        let Value::Native(n) = target else {
            return Err(exc("AttributeError", format!("'{}' object has no attribute 'write'", target.type_name())));
        };
        let kind = match &*n.borrow() {
            Native::File(f) => {
                if f.closed {
                    return Err(exc("ValueError", "I/O operation on closed file."));
                }
                f.kind
            }
            _ => return Err(exc("AttributeError", format!("'{}' object has no attribute 'write'", target.type_name()))),
        };
        match kind {
            FileKind::Stdout => self.stdout.extend_from_slice(text.as_bytes()),
            FileKind::Stderr => {
                // O stderr do CPython é sem buffer; o stdout pendente sai antes, para manter a ordem.
                let _ = sysabi::sys::write_all(sysabi::Fd::STDOUT, &self.stdout);
                self.stdout.clear();
                let _ = sysabi::sys::write_all(sysabi::Fd::STDERR, text.as_bytes());
            }
            _ => return Err(exc("UnsupportedOperation", "not writable")),
        }
        Ok(text.chars().count())
    }

    fn load_attr(&mut self, obj: &Value, name: &str) -> PyResult<Value> {
        let missing = || {
            exc("AttributeError", format!("'{}' object has no attribute '{name}'", obj.type_name()))
        };
        match obj {
            Value::Exception(e) if name == "args" => Ok(Value::tuple(e.args.clone())),
            Value::Module("sys") => match name {
                "argv" => Ok(Value::list(self.argv.iter().map(|a| Value::str(a.clone())).collect())),
                "stdin" => Ok(Value::Native(self.std_files[0].clone())),
                "stdout" => Ok(Value::Native(self.std_files[1].clone())),
                "stderr" => Ok(Value::Native(self.std_files[2].clone())),
                _ => Err(exc("AttributeError", format!("module 'sys' has no attribute '{name}'"))),
            },
            Value::Module("csv") => match name {
                "reader" => Ok(Value::Builtin("csv.reader")),
                "writer" => Ok(Value::Builtin("csv.writer")),
                "Error" => Ok(Value::Builtin("_csv.Error")),
                "QUOTE_MINIMAL" => Ok(Value::Int(i64::from(csv::QUOTE_MINIMAL))),
                "QUOTE_ALL" => Ok(Value::Int(i64::from(csv::QUOTE_ALL))),
                "QUOTE_NONNUMERIC" => Ok(Value::Int(i64::from(csv::QUOTE_NONNUMERIC))),
                "QUOTE_NONE" => Ok(Value::Int(i64::from(csv::QUOTE_NONE))),
                "QUOTE_STRINGS" => Ok(Value::Int(i64::from(csv::QUOTE_STRINGS))),
                "QUOTE_NOTNULL" => Ok(Value::Int(i64::from(csv::QUOTE_NOTNULL))),
                _ => Err(exc("AttributeError", format!("module 'csv' has no attribute '{name}'"))),
            },
            Value::Module("json") => match name {
                "dumps" => Ok(Value::Builtin("json.dumps")),
                "loads" => Ok(Value::Builtin("json.loads")),
                "JSONDecodeError" => Ok(Value::Builtin("json.decoder.JSONDecodeError")),
                _ => Err(exc("AttributeError", format!("module 'json' has no attribute '{name}'"))),
            },
            Value::Native(n) => {
                let methods: &[&'static str] = match &*n.borrow() {
                    Native::File(_) => &["write", "read", "readline", "readlines", "close", "flush"],
                    Native::CsvWriter { .. } => &["writerow", "writerows"],
                    Native::CsvReader { reader, .. } => {
                        if name == "line_num" {
                            return Ok(Value::Int(reader.line_num as i64));
                        }
                        &[]
                    }
                };
                match methods.iter().find(|m| **m == name) {
                    Some(m) => Ok(Value::Bound(Rc::new(BoundMethod { recv: obj.clone(), name: m }))),
                    None if name == "closed" => match &*n.borrow() {
                        Native::File(f) => Ok(Value::Bool(f.closed)),
                        _ => Err(missing()),
                    },
                    None if name == "name" => match &*n.borrow() {
                        Native::File(f) => Ok(Value::str(f.name.clone())),
                        _ => Err(missing()),
                    },
                    None => Err(missing()),
                }
            }
            _ => Err(missing()),
        }
    }

    fn call_method(
        &mut self,
        recv: &Value,
        name: &'static str,
        args: Vec<Value>,
        kwargs: Vec<(String, Value)>,
    ) -> PyResult<Value> {
        if let Some((kw, _)) = kwargs.first() {
            return Err(type_error(format!("{name}() takes no keyword arguments ('{kw}' given)")));
        }
        let Value::Native(n) = recv else { return Err(internal("bound method without native receiver")) };
        match name {
            "write" => {
                let [v] = one_arg(name, args)?;
                let Value::Str(s) = &v else {
                    return Err(type_error(format!("write() argument must be str, not {}", v.type_name())));
                };
                Ok(Value::Int(self.write_to(recv, s.as_str())? as i64))
            }
            "flush" => Ok(Value::None),
            "close" => {
                if let Native::File(f) = &mut *n.borrow_mut() {
                    f.closed = true;
                }
                Ok(Value::None)
            }
            "read" => {
                let mut out = String::new();
                while let Some(l) = file_readline(n)? {
                    out.push_str(&l);
                }
                Ok(Value::str(out))
            }
            "readline" => Ok(Value::str(file_readline(n)?.unwrap_or_default())),
            "readlines" => {
                let mut out = Vec::new();
                while let Some(l) = file_readline(n)? {
                    out.push(Value::str(l));
                }
                Ok(Value::list(out))
            }
            "writerow" | "writerows" => {
                let [row] = one_arg(name, args)?;
                let (dialect, target) = match &*n.borrow() {
                    Native::CsvWriter { dialect, target } => (dialect.clone(), target.clone()),
                    _ => return Err(internal("writerow on non-writer")),
                };
                let rows = if name == "writerow" { vec![row] } else { collect(&row)? };
                let mut last = Value::None;
                for r in rows {
                    let fields = match &r {
                        Value::Str(_) | Value::Int(_) | Value::Float(_) | Value::Bool(_) | Value::None => {
                            return Err(exc("_csv.Error", format!("iterable expected, not {}", r.type_name())))
                        }
                        _ => collect(&r)?,
                    };
                    let line = csv::writerow(&dialect, &fields).map_err(|e| exc("_csv.Error", e.msg))?;
                    last = Value::Int(self.write_to(&target, &line)? as i64);
                }
                Ok(if name == "writerow" { last } else { Value::None })
            }
            _ => Err(internal("unknown method")),
        }
    }

    /// `open(path, mode='r', ..., newline=None, encoding=None)`: só leitura de texto UTF-8.
    fn open(&mut self, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
        let mut newline_keep = false;
        let mut mode = "r".to_string();
        for (k, v) in &kwargs {
            match k.as_str() {
                "newline" => newline_keep = matches!(v, Value::Str(s) if s.as_str().is_empty()),
                "encoding" | "errors" => {}
                "mode" => mode = to_str(v),
                _ => return Err(type_error(format!("open() got an unexpected keyword argument '{k}'"))),
            }
        }
        if let Some(m) = args.get(1) {
            mode = to_str(m);
        }
        let Some(Value::Str(path)) = args.first() else {
            return Err(type_error("expected str, bytes or os.PathLike object"));
        };
        if mode != "r" && mode != "rt" {
            return Err(exc("NotImplementedError", format!("open(mode={mode:?}) is not supported yet")));
        }
        let path = path.as_str().to_string();
        let bytes = match sysabi::sys::read_file(path.as_bytes()) {
            Ok(b) => b,
            Err(e) => {
                let kind = if e == sysabi::Errno::ENOENT { "FileNotFoundError" } else { "OSError" };
                let msg = format!("[Errno {}] {}: '{path}'", e.0, e.message());
                return Err(exc(kind, msg));
            }
        };
        let text = String::from_utf8(bytes).map_err(|e| {
            let at = e.utf8_error().valid_up_to();
            let b = e.as_bytes()[at];
            exc("UnicodeDecodeError", format!("'utf-8' codec can't decode byte 0x{b:02x} in position {at}: invalid start byte"))
        })?;
        Ok(Value::Native(Rc::new(RefCell::new(Native::File(PyFile {
            kind: FileKind::Read,
            lines: split_lines(&text, newline_keep),
            pos: 0,
            loaded: true,
            closed: false,
            name: path,
        })))))
    }

    /// `csv.reader(f, **dialeto)` e `csv.writer(f, **dialeto)`.
    fn csv_open(&mut self, name: &str, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
        let Some(target) = args.first().cloned() else {
            return Err(type_error("expected at least 1 argument, got 0"));
        };
        let mut d = csv::Dialect::default();
        let one_char = |k: &str, v: &Value| -> PyResult<Option<char>> {
            match v {
                Value::None => Ok(None),
                Value::Str(s) if s.as_str().chars().count() == 1 => Ok(s.as_str().chars().next()),
                Value::Str(_) => Err(type_error(format!("\"{k}\" must be a unicode character or None, not a string of length {}", match v { Value::Str(s) => s.as_str().chars().count(), _ => 0 }))),
                _ => Err(type_error(format!("\"{k}\" must be string or None, not {}", v.type_name()))),
            }
        };
        for (k, v) in &kwargs {
            match k.as_str() {
                "delimiter" => d.delimiter = one_char(k, v)?.ok_or_else(|| type_error("\"delimiter\" must be a 1-character string"))?,
                "quotechar" => d.quotechar = one_char(k, v)?,
                "escapechar" => d.escapechar = one_char(k, v)?,
                "doublequote" => d.doublequote = v.is_true(),
                "skipinitialspace" => d.skipinitialspace = v.is_true(),
                "strict" => d.strict = v.is_true(),
                "lineterminator" => d.lineterminator = to_str(v),
                "quoting" => match v {
                    Value::Int(i) => d.quoting = *i as i32,
                    _ => return Err(type_error("\"quoting\" must be an integer")),
                },
                _ => return Err(type_error(format!("'{k}' is an invalid keyword argument for {name}()"))),
            }
        }
        let native = if name == "csv.reader" {
            collect_check_iter(&target)?;
            Native::CsvReader { reader: csv::Reader::new(d), src: target }
        } else {
            Native::CsvWriter { dialect: d, target }
        };
        Ok(Value::Native(Rc::new(RefCell::new(native))))
    }
}

/// O alvo de `csv.reader` precisa ser iterável.
fn collect_check_iter(v: &Value) -> PyResult<()> {
    get_iter(v).map(|_| ())
}

/// Divide o texto em linhas com o terminador. Com `keep` (`newline=''`) o terminador original
/// fica; sem ele (`newline=None`) `\r\n` e `\r` viram `\n`.
fn split_lines(text: &str, keep: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\n' => {
                cur.push('\n');
                out.push(std::mem::take(&mut cur));
            }
            '\r' => {
                let crlf = it.peek() == Some(&'\n');
                if crlf {
                    it.next();
                }
                if keep {
                    cur.push('\r');
                    if crlf {
                        cur.push('\n');
                    }
                } else {
                    cur.push('\n');
                }
                out.push(std::mem::take(&mut cur));
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Próxima linha de um arquivo de texto (o stdin carrega no primeiro uso).
fn file_readline(n: &Rc<RefCell<Native>>) -> PyResult<Option<String>> {
    let mut b = n.borrow_mut();
    let Native::File(f) = &mut *b else { return Err(type_error("not a file")) };
    if f.closed {
        return Err(exc("ValueError", "I/O operation on closed file."));
    }
    if !f.loaded {
        f.loaded = true;
        let bytes = sysabi::sys::read_to_end(sysabi::Fd::STDIN).unwrap_or_default();
        f.lines = split_lines(&String::from_utf8_lossy(&bytes), false);
    }
    if f.kind == FileKind::Stdout || f.kind == FileKind::Stderr {
        return Err(exc("UnsupportedOperation", "not readable"));
    }
    let l = f.lines.get(f.pos).cloned();
    if l.is_some() {
        f.pos += 1;
    }
    Ok(l)
}

/// Próximo item de um arquivo (linha) ou de um leitor de `csv` (lista de campos).
fn native_next(n: &Rc<RefCell<Native>>) -> PyResult<Option<Value>> {
    let is_reader = matches!(&*n.borrow(), Native::CsvReader { .. });
    if !is_reader {
        return Ok(file_readline(n)?.map(Value::str));
    }
    let src = match &*n.borrow() {
        Native::CsvReader { src, .. } => src.clone(),
        _ => return Ok(None),
    };
    let mut it = get_iter(&src)?;
    // O iterador da fonte é recriado a cada linha: arquivo e leitor guardam a posição neles mesmos,
    // as demais fontes (listas) são raras e leem pela posição própria do `PyIter::List`.
    let mut err: Option<PyException> = None;
    let mut next_line = || match it.next() {
        Ok(Some(Value::Str(s))) => Some(s.as_str().to_string()),
        Ok(_) => None,
        Err(e) => {
            err = Some(e);
            None
        }
    };
    let row = {
        let mut b = n.borrow_mut();
        let Native::CsvReader { reader, .. } = &mut *b else { return Ok(None) };
        reader.next_row(&mut next_line)
    };
    if let Some(e) = err {
        return Err(e);
    }
    match row.map_err(|e| exc("_csv.Error", e.msg))? {
        Some(fields) => Ok(Some(Value::list(fields.into_iter().map(Value::str).collect()))),
        None => Ok(None),
    }
}

/// `except cls`: `cls` é uma classe de exceção ou uma tupla delas.
fn exc_matches(kind: &str, cls: &Value) -> PyResult<bool> {
    match cls {
        Value::Builtin(name) if EXC_CLASSES.iter().any(|(n, _)| n == name) => Ok(exc_is_subclass(kind, name)),
        Value::Tuple(items) => {
            for item in items.iter() {
                if exc_matches(kind, item)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Err(type_error("catching classes that do not inherit from BaseException is not allowed")),
    }
}

/// Valor do `raise X`: uma classe vira instância sem argumentos.
fn raise_value(v: Value) -> PyResult<PyException> {
    match &v {
        Value::Exception(_) => Ok(PyException::from_value(&v)),
        Value::Builtin(name) => match EXC_CLASSES.iter().find(|(n, _)| n == name) {
            Some((n, _)) => Ok(PyException::from_value(&Value::Exception(Rc::new(ExcObj { kind: n, args: Vec::new() })))),
            None => Err(type_error("exceptions must derive from BaseException")),
        },
        _ => Err(type_error("exceptions must derive from BaseException")),
    }
}

fn list_of(items: Vec<Value>) -> Value {
    Value::List(Rc::new(RefCell::new(items)))
}

/// Ordena com `<`, estável, propagando o `TypeError` de tipos incomparáveis.
fn sort_values(items: &mut [Value]) -> PyResult<()> {
    let mut err = None;
    items.sort_by(|a, b| {
        if err.is_some() {
            return std::cmp::Ordering::Equal;
        }
        match order(CmpOp::Lt, a, b) {
            Ok(true) => std::cmp::Ordering::Less,
            Ok(false) => match order(CmpOp::Lt, b, a) {
                Ok(true) => std::cmp::Ordering::Greater,
                Ok(false) => std::cmp::Ordering::Equal,
                Err(e) => {
                    err = Some(e);
                    std::cmp::Ordering::Equal
                }
            },
            Err(e) => {
                err = Some(e);
                std::cmp::Ordering::Equal
            }
        }
    });
    err.map_or(Ok(()), Err)
}

/// Builtins sobre sequências e números (`list`, `sorted`, `min`, `sum`...).
fn builtin_seq(name: &'static str, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
    let at_most = |max: usize| -> PyResult<()> {
        if args.len() > max {
            return Err(type_error(format!("{name}() takes at most {max} argument{} ({} given)", if max == 1 { "" } else { "s" }, args.len())));
        }
        Ok(())
    };
    match name {
        "list" | "tuple" => {
            at_most(1)?;
            let items = match args.first() {
                Some(v) => collect(v)?,
                None => Vec::new(),
            };
            Ok(if name == "list" { list_of(items) } else { Value::Tuple(items.into()) })
        }
        "bool" => {
            at_most(1)?;
            Ok(Value::Bool(args.first().is_some_and(Value::is_true)))
        }
        "float" => {
            at_most(1)?;
            match args.first() {
                None => Ok(Value::Float(0.0)),
                Some(Value::Int(i)) => Ok(Value::Float(*i as f64)),
                Some(Value::Bool(b)) => Ok(Value::Float(f64::from(u8::from(*b)))),
                Some(Value::Float(x)) => Ok(Value::Float(*x)),
                Some(v @ Value::Str(s)) => {
                    let t = s.as_str().trim().replace('_', "");
                    let low = t.to_ascii_lowercase();
                    let parsed = match low.trim_start_matches(['+', '-']) {
                        "inf" | "infinity" => Some(if low.starts_with('-') { f64::NEG_INFINITY } else { f64::INFINITY }),
                        "nan" => Some(f64::NAN),
                        _ => t.parse::<f64>().ok().filter(|_| !low.contains("inf") && !low.contains("nan")),
                    };
                    parsed.map(Value::Float).ok_or_else(|| {
                        exc("ValueError", format!("could not convert string to float: {}", repr(v)))
                    })
                }
                Some(v) => Err(type_error(format!(
                    "float() argument must be a string or a real number, not '{}'",
                    v.type_name()
                ))),
            }
        }
        "abs" => {
            let [v] = one_arg(name, args)?;
            match v {
                Value::Int(i) => i.checked_abs().map(Value::Int).ok_or_else(|| ObjError::IntOverflow.into()),
                Value::Bool(b) => Ok(Value::Int(i64::from(b))),
                Value::Float(x) => Ok(Value::Float(x.abs())),
                other => Err(type_error(format!("bad operand type for abs(): '{}'", other.type_name()))),
            }
        }
        "min" | "max" => {
            let items = if args.len() == 1 { collect(&args[0])? } else { args.clone() };
            if args.is_empty() {
                return Err(type_error(format!("{name} expected at least 1 argument, got 0")));
            }
            let Some(mut best) = items.first().cloned() else {
                return Err(exc("ValueError", format!("{name}() iterable argument is empty")));
            };
            let op = if name == "min" { CmpOp::Lt } else { CmpOp::Gt };
            for x in &items[1..] {
                if order(op, x, &best)? {
                    best = x.clone();
                }
            }
            Ok(best)
        }
        "sum" => {
            let Some(first) = args.first() else {
                return Err(type_error("sum() takes at least 1 positional argument (0 given)"));
            };
            let mut acc = args.get(1).cloned().unwrap_or(Value::Int(0));
            for x in collect(first)? {
                acc = binary(Operator::Add, &acc, &x, false)?;
            }
            Ok(acc)
        }
        "sorted" => {
            let [v] = one_arg(name, args)?;
            let mut items = collect(&v)?;
            sort_values(&mut items)?;
            for (k, val) in &kwargs {
                match k.as_str() {
                    "reverse" => {
                        if val.is_true() {
                            items.reverse();
                        }
                    }
                    _ => return Err(type_error(format!("sort() got an unexpected keyword argument '{k}'"))),
                }
            }
            Ok(list_of(items))
        }
        "reversed" => {
            let [v] = one_arg(name, args)?;
            let mut items = collect(&v)?;
            items.reverse();
            Ok(list_of(items))
        }
        "enumerate" => {
            let mut start = 0i64;
            for (k, val) in &kwargs {
                match (k.as_str(), val) {
                    ("start", Value::Int(i)) => start = *i,
                    _ => return Err(type_error(format!("'{k}' is an invalid keyword argument for enumerate()"))),
                }
            }
            let [v] = one_arg(name, args)?;
            let out = collect(&v)?
                .into_iter()
                .enumerate()
                .map(|(i, x)| Value::Tuple(vec![Value::Int(start + i as i64), x].into()))
                .collect();
            Ok(list_of(out))
        }
        "zip" => {
            let cols: Vec<Vec<Value>> = args.iter().map(collect).collect::<PyResult<_>>()?;
            let n = cols.iter().map(Vec::len).min().unwrap_or(0);
            let out = (0..n).map(|i| Value::Tuple(cols.iter().map(|c| c[i].clone()).collect::<Vec<_>>().into())).collect();
            Ok(list_of(out))
        }
        "any" | "all" => {
            let [v] = one_arg(name, args)?;
            let items = collect(&v)?;
            Ok(Value::Bool(if name == "any" { items.iter().any(Value::is_true) } else { items.iter().all(Value::is_true) }))
        }
        "ord" => {
            let [v] = one_arg(name, args)?;
            match &v {
                Value::Str(s) if s.as_str().chars().count() == 1 => {
                    Ok(Value::Int(s.as_str().chars().next().map_or(0, |c| i64::from(u32::from(c)))))
                }
                Value::Str(s) => Err(type_error(format!(
                    "ord() expected a character, but string of length {} found",
                    s.as_str().chars().count()
                ))),
                other => Err(type_error(format!("ord() expected string of length 1, but {} found", other.type_name()))),
            }
        }
        _ => {
            let [v] = one_arg(name, args)?;
            match v {
                Value::Int(i) => u32::try_from(i)
                    .ok()
                    .and_then(char::from_u32)
                    .map(|c| Value::str(c.to_string()))
                    .ok_or_else(|| exc("ValueError", "chr() arg not in range(0x110000)")),
                other => Err(type_error(format!("'{}' object cannot be interpreted as an integer", other.type_name()))),
            }
        }
    }
}

fn one_arg(name: &str, args: Vec<Value>) -> PyResult<[Value; 1]> {
    let n = args.len();
    <[Value; 1]>::try_from(args)
        .map_err(|_| type_error(format!("{name}() takes exactly one argument ({n} given)")))
}

fn len(v: &Value) -> PyResult<i64> {
    Ok(match v {
        Value::Str(s) => s.len() as i64,
        Value::Bytes(b) => b.len() as i64,
        Value::List(l) => l.borrow().len() as i64,
        Value::Tuple(t) => t.len() as i64,
        Value::Dict(d) => d.borrow().len() as i64,
        Value::Set(s) => s.borrow().len() as i64,
        Value::Range(r) => r.len(),
        _ => return Err(type_error(format!("object of type '{}' has no len()", v.type_name()))),
    })
}

/// Inteiro de um índice (`__index__`): `int` e `bool`.
fn as_index(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Bool(b) => Some(i64::from(*b)),
        _ => None,
    }
}

fn range_of(args: &[Value]) -> PyResult<Value> {
    if args.is_empty() {
        return Err(type_error("range expected at least 1 argument, got 0"));
    }
    if args.len() > 3 {
        return Err(type_error(format!("range expected at most 3 arguments, got {}", args.len())));
    }
    let mut ints = Vec::with_capacity(3);
    for a in args {
        match as_index(a) {
            Some(i) => ints.push(i),
            None => {
                return Err(type_error(format!("'{}' object cannot be interpreted as an integer", a.type_name())))
            }
        }
    }
    let r = match ints[..] {
        [stop] => Range { start: 0, stop, step: 1 },
        [start, stop] => Range { start, stop, step: 1 },
        [start, stop, step] => {
            if step == 0 {
                return Err(exc("ValueError", "range() arg 3 must not be zero"));
            }
            Range { start, stop, step }
        }
        _ => return Err(internal("range arity")),
    };
    Ok(Value::Range(r))
}

/// `int(x)` com um argumento.
fn int_of(v: &Value) -> PyResult<Value> {
    match v {
        Value::Int(i) => Ok(Value::Int(*i)),
        Value::Bool(b) => Ok(Value::Int(i64::from(*b))),
        Value::Float(x) => {
            if x.is_nan() {
                return Err(exc("ValueError", "cannot convert float NaN to integer"));
            }
            if x.is_infinite() {
                return Err(exc("OverflowError", "cannot convert float infinity to integer"));
            }
            let t = x.trunc();
            if !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&t) {
                return Err(ObjError::IntOverflow.into());
            }
            Ok(Value::Int(t as i64))
        }
        Value::Str(s) => parse_int(s.as_str())
            .map(Value::Int)
            .ok_or_else(|| exc("ValueError", format!("invalid literal for int() with base 10: {}", repr(v)))),
        _ => Err(type_error(format!(
            "int() argument must be a string, a bytes-like object or a real number, not '{}'",
            v.type_name()
        ))),
    }
}

/// Literal decimal do `int(str)`: espaços em volta, sinal opcional, `_` só entre dígitos.
fn parse_int(text: &str) -> Option<i64> {
    let t = text.trim_matches(char::is_whitespace);
    let (negative, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') || digits.contains("__") {
        return None;
    }
    let mut value: i128 = 0;
    for c in digits.chars() {
        if c == '_' {
            continue;
        }
        let d = c.to_digit(10)?;
        value = value * 10 + i128::from(d);
        if value > i128::from(i64::MAX) + 1 {
            return None;
        }
    }
    let value = if negative { -value } else { value };
    i64::try_from(value).ok()
}

/// Índice normalizado de uma sequência de tamanho `len`, ou `None` se estiver fora.
fn normalize(i: i64, len: usize) -> Option<usize> {
    let len = len as i64;
    let i = if i < 0 { i + len } else { i };
    (0..len).contains(&i).then_some(i as usize)
}

fn subscript(container: &Value, index: &Value) -> PyResult<Value> {
    let seq_index = |what: &str| -> PyResult<i64> {
        as_index(index)
            .ok_or_else(|| type_error(format!("{what} indices must be integers or slices, not {}", index.type_name())))
    };
    match container {
        Value::List(l) => {
            let i = seq_index("list")?;
            let items = l.borrow();
            normalize(i, items.len())
                .map(|i| items[i].clone())
                .ok_or_else(|| exc("IndexError", "list index out of range"))
        }
        Value::Tuple(t) => {
            let i = seq_index("tuple")?;
            normalize(i, t.len()).map(|i| t[i].clone()).ok_or_else(|| exc("IndexError", "tuple index out of range"))
        }
        Value::Str(s) => {
            let i = as_index(index).ok_or_else(|| {
                type_error(format!("string indices must be integers, not '{}'", index.type_name()))
            })?;
            normalize(i, s.len())
                .and_then(|i| s.char_at(i))
                .map(|c| Value::str(c.to_string()))
                .ok_or_else(|| exc("IndexError", "string index out of range"))
        }
        Value::Bytes(b) => {
            let i = as_index(index).ok_or_else(|| {
                type_error(format!("byte indices must be integers or slices, not {}", index.type_name()))
            })?;
            normalize(i, b.len()).map(|i| Value::Int(i64::from(b[i]))).ok_or_else(|| exc("IndexError", "index out of range"))
        }
        Value::Range(r) => {
            let i = as_index(index).ok_or_else(|| {
                type_error(format!("range indices must be integers or slices, not {}", index.type_name()))
            })?;
            let len = r.len();
            let i = if i < 0 { i + len } else { i };
            if (0..len).contains(&i) {
                Ok(Value::Int(r.item(i)))
            } else {
                Err(exc("IndexError", "range object index out of range"))
            }
        }
        Value::Dict(d) => match d.borrow().get(index)? {
            Some(v) => Ok(v),
            None => Err(PyException {
                kind: "KeyError",
                msg: repr(index),
                value: Some(Value::Exception(Rc::new(ExcObj { kind: "KeyError", args: vec![index.clone()] }))),
                tb: Vec::new(),
            }),
        },
        _ => Err(type_error(format!("'{}' object is not subscriptable", container.type_name()))),
    }
}

fn store_subscript(container: &Value, index: &Value, value: Value) -> PyResult<()> {
    match container {
        Value::List(l) => {
            let i = as_index(index).ok_or_else(|| {
                type_error(format!("list indices must be integers or slices, not {}", index.type_name()))
            })?;
            let mut items = l.borrow_mut();
            let len = items.len();
            let i = normalize(i, len).ok_or_else(|| exc("IndexError", "list assignment index out of range"))?;
            items[i] = value;
            Ok(())
        }
        Value::Dict(d) => Ok(d.borrow_mut().set(index.clone(), value)?),
        _ => Err(type_error(format!("'{}' object does not support item assignment", container.type_name()))),
    }
}

/// Visão numérica de `bool`, `int` e `float`.
#[derive(Clone, Copy)]
enum Num {
    Int(i64),
    Float(f64),
}

fn num(v: &Value) -> Option<Num> {
    match v {
        Value::Bool(b) => Some(Num::Int(i64::from(*b))),
        Value::Int(i) => Some(Num::Int(*i)),
        Value::Float(x) => Some(Num::Float(*x)),
        _ => None,
    }
}

fn as_float(n: Num) -> f64 {
    match n {
        Num::Int(i) => i as f64,
        Num::Float(x) => x,
    }
}

fn op_symbol(op: Operator) -> &'static str {
    use Operator as O;
    match op {
        O::Add => "+",
        O::Sub => "-",
        O::Mult => "*",
        O::MatMult => "@",
        O::Div => "/",
        O::Mod => "%",
        O::Pow => "** or pow()",
        O::LShift => "<<",
        O::RShift => ">>",
        O::BitOr => "|",
        O::BitXor => "^",
        O::BitAnd => "&",
        O::FloorDiv => "//",
    }
}

fn unsupported(op: Operator, a: &Value, b: &Value, inplace: bool) -> PyException {
    let eq = if inplace { "=" } else { "" };
    let sym = op_symbol(op);
    // A forma aumentada de `**` é `**=`, sem o "or pow()".
    let sym = if inplace && op == Operator::Pow { "**".to_string() } else { sym.to_string() };
    type_error(format!(
        "unsupported operand type(s) for {sym}{eq}: '{}' and '{}'",
        a.type_name(),
        b.type_name()
    ))
}

fn binary(op: Operator, a: &Value, b: &Value, inplace: bool) -> PyResult<Value> {
    // `list += iterável` e `list *= n` mudam a própria lista.
    if inplace
        && let Value::List(l) = a {
            match op {
                Operator::Add => {
                    let items = collect(b)?;
                    l.borrow_mut().extend(items);
                    return Ok(a.clone());
                }
                Operator::Mult => {
                    if let Some(n) = as_index(b) {
                        let items = repeat(&l.borrow()[..], n)?;
                        *l.borrow_mut() = items;
                        return Ok(a.clone());
                    }
                }
                _ => {}
            }
        }
    if let (Some(x), Some(y)) = (num(a), num(b)) {
        // `bool & bool` (e `|`, `^`) continua `bool`.
        if let (Value::Bool(p), Value::Bool(q)) = (a, b) {
            match op {
                Operator::BitAnd => return Ok(Value::Bool(*p & *q)),
                Operator::BitOr => return Ok(Value::Bool(*p | *q)),
                Operator::BitXor => return Ok(Value::Bool(*p ^ *q)),
                _ => {}
            }
        }
        return match (x, y) {
            (Num::Int(x), Num::Int(y)) => int_binary(op, x, y).map_err(|e| match e {
                Some(e) => e,
                None => unsupported(op, a, b, inplace),
            }),
            _ => float_binary(op, as_float(x), as_float(y)).map_err(|e| match e {
                Some(e) => e,
                None => unsupported(op, a, b, inplace),
            }),
        };
    }
    match (op, a, b) {
        (Operator::Add, Value::Str(x), Value::Str(y)) => {
            let mut s = String::with_capacity(x.as_str().len() + y.as_str().len());
            s.push_str(x.as_str());
            s.push_str(y.as_str());
            Ok(Value::str(s))
        }
        (Operator::Add, Value::Str(_), _) => {
            Err(type_error(format!("can only concatenate str (not \"{}\") to str", b.type_name())))
        }
        (Operator::Add, Value::List(x), Value::List(y)) => {
            let mut items = x.borrow().clone();
            items.extend(y.borrow().iter().cloned());
            Ok(Value::list(items))
        }
        (Operator::Add, Value::List(_), _) => {
            Err(type_error(format!("can only concatenate list (not \"{}\") to list", b.type_name())))
        }
        (Operator::Add, Value::Tuple(x), Value::Tuple(y)) => {
            Ok(Value::tuple(x.iter().chain(y.iter()).cloned().collect()))
        }
        (Operator::Add, Value::Tuple(_), _) => {
            Err(type_error(format!("can only concatenate tuple (not \"{}\") to tuple", b.type_name())))
        }
        (Operator::Add, Value::Bytes(x), Value::Bytes(y)) => Ok(Value::bytes([&x[..], &y[..]].concat())),
        (Operator::Mult, seq, n) | (Operator::Mult, n, seq) if is_sequence(seq) && !is_sequence(n) => {
            match as_index(n) {
                Some(count) => repeat_value(seq, count),
                None => Err(type_error(format!("can't multiply sequence by non-int of type '{}'", n.type_name()))),
            }
        }
        (Operator::Mult, seq, other) if is_sequence(seq) => {
            Err(type_error(format!("can't multiply sequence by non-int of type '{}'", other.type_name())))
        }
        (Operator::Mod, Value::Str(_), _) => Err(exc("NotImplementedError", "str % formatting is not supported yet")),
        _ => Err(unsupported(op, a, b, inplace)),
    }
}

fn is_sequence(v: &Value) -> bool {
    matches!(v, Value::Str(_) | Value::List(_) | Value::Tuple(_) | Value::Bytes(_))
}

fn repeat<T: Clone>(items: &[T], n: i64) -> PyResult<Vec<T>> {
    let n = n.max(0) as usize;
    let total = items
        .len()
        .checked_mul(n)
        .filter(|t| *t <= isize::MAX as usize / 64)
        .ok_or_else(|| exc("MemoryError", ""))?;
    let mut out = Vec::with_capacity(total);
    for _ in 0..n {
        out.extend_from_slice(items);
    }
    Ok(out)
}

fn repeat_value(seq: &Value, n: i64) -> PyResult<Value> {
    Ok(match seq {
        Value::Str(s) => {
            let chars: Vec<char> = s.as_str().chars().collect();
            Value::str(repeat(&chars, n)?.into_iter().collect::<String>())
        }
        Value::List(l) => Value::list(repeat(&l.borrow()[..], n)?),
        Value::Tuple(t) => Value::tuple(repeat(&t[..], n)?),
        Value::Bytes(b) => Value::bytes(repeat(&b[..], n)?),
        _ => return Err(internal("repeat of non-sequence")),
    })
}

/// Aritmética de `int`; `Err(None)` é "tipo não suportado" (o chamador monta a mensagem).
fn int_binary(op: Operator, a: i64, b: i64) -> Result<Value, Option<PyException>> {
    use Operator as O;
    let overflow = |e: ObjError| Some(PyException::from(e));
    Ok(match op {
        O::Add => Value::Int(int_add(a, b).map_err(overflow)?),
        O::Sub => Value::Int(int_sub(a, b).map_err(overflow)?),
        O::Mult => Value::Int(int_mul(a, b).map_err(overflow)?),
        O::Div => {
            if b == 0 {
                return Err(Some(exc("ZeroDivisionError", "division by zero")));
            }
            Value::Float(a as f64 / b as f64)
        }
        O::FloorDiv => {
            if b == 0 {
                return Err(Some(exc("ZeroDivisionError", "integer division or modulo by zero")));
            }
            let q = a.checked_div(b).ok_or_else(|| overflow(ObjError::IntOverflow))?;
            let q = if a % b != 0 && ((a < 0) != (b < 0)) { q - 1 } else { q };
            Value::Int(q)
        }
        O::Mod => {
            if b == 0 {
                return Err(Some(exc("ZeroDivisionError", "integer modulo by zero")));
            }
            let r = a.checked_rem(b).unwrap_or(0);
            Value::Int(if r != 0 && ((r < 0) != (b < 0)) { r + b } else { r })
        }
        O::Pow => {
            if b < 0 {
                return float_binary(O::Pow, a as f64, b as f64);
            }
            let mut result: i64 = 1;
            let mut base = a;
            let mut exp = b;
            while exp > 0 {
                if exp & 1 == 1 {
                    result = int_mul(result, base).map_err(overflow)?;
                }
                exp >>= 1;
                if exp > 0 {
                    base = int_mul(base, base).map_err(overflow)?;
                }
            }
            Value::Int(result)
        }
        O::LShift => {
            if b < 0 {
                return Err(Some(exc("ValueError", "negative shift count")));
            }
            if a == 0 {
                return Ok(Value::Int(0));
            }
            if b >= 64 {
                return Err(overflow(ObjError::IntOverflow));
            }
            let r = i128::from(a) << b;
            Value::Int(i64::try_from(r).map_err(|_| overflow(ObjError::IntOverflow))?)
        }
        O::RShift => {
            if b < 0 {
                return Err(Some(exc("ValueError", "negative shift count")));
            }
            Value::Int(if b >= 64 { if a < 0 { -1 } else { 0 } } else { a >> b })
        }
        O::BitAnd => Value::Int(a & b),
        O::BitOr => Value::Int(a | b),
        O::BitXor => Value::Int(a ^ b),
        O::MatMult => return Err(None),
    })
}

/// Aritmética de `float` (`float_add`, `float_div`, `float_divmod`, `float_pow`).
fn float_binary(op: Operator, a: f64, b: f64) -> Result<Value, Option<PyException>> {
    use Operator as O;
    Ok(Value::Float(match op {
        O::Add => a + b,
        O::Sub => a - b,
        O::Mult => a * b,
        O::Div => {
            if b == 0.0 {
                return Err(Some(exc("ZeroDivisionError", "float division by zero")));
            }
            a / b
        }
        O::FloorDiv => {
            if b == 0.0 {
                return Err(Some(exc("ZeroDivisionError", "float floor division by zero")));
            }
            float_divmod(a, b).0
        }
        O::Mod => {
            if b == 0.0 {
                return Err(Some(exc("ZeroDivisionError", "float modulo by zero")));
            }
            float_divmod(a, b).1
        }
        O::Pow => {
            if a == 0.0 && b < 0.0 {
                return Err(Some(exc("ZeroDivisionError", "0.0 cannot be raised to a negative power")));
            }
            if a < 0.0 && b.is_finite() && b.fract() != 0.0 {
                return Err(Some(exc("NotImplementedError", "complex results are not supported yet")));
            }
            let r = a.powf(b);
            if r.is_infinite() && a.is_finite() && b.is_finite() {
                return Err(Some(exc("OverflowError", "(34, 'Numerical result out of range')")));
            }
            r
        }
        _ => return Err(None),
    }))
}

/// `float_divmod`: quociente arredondado para baixo e resto com o sinal do divisor.
fn float_divmod(vx: f64, wx: f64) -> (f64, f64) {
    let mut m = vx % wx;
    let mut div = (vx - m) / wx;
    if m != 0.0 {
        if (wx < 0.0) != (m < 0.0) {
            m += wx;
            div -= 1.0;
        }
    } else {
        m = 0.0_f64.copysign(wx);
    }
    let floordiv = if div != 0.0 {
        let mut f = div.floor();
        if div - f > 0.5 {
            f += 1.0;
        }
        f
    } else {
        0.0_f64.copysign(vx / wx)
    };
    (floordiv, m)
}

fn unary(op: UnaryOp, a: &Value) -> PyResult<Value> {
    let bad = |sym: &str| type_error(format!("bad operand type for unary {sym}: '{}'", a.type_name()));
    match op {
        UnaryOp::Not => Ok(Value::Bool(!a.is_true())),
        UnaryOp::USub => match num(a) {
            Some(Num::Int(i)) => Ok(Value::Int(int_neg(i)?)),
            Some(Num::Float(x)) => Ok(Value::Float(-x)),
            None => Err(bad("-")),
        },
        UnaryOp::UAdd => match num(a) {
            Some(Num::Int(i)) => Ok(Value::Int(i)),
            Some(Num::Float(x)) => Ok(Value::Float(x)),
            None => Err(bad("+")),
        },
        UnaryOp::Invert => match a {
            Value::Int(i) => Ok(Value::Int(!i)),
            Value::Bool(b) => Ok(Value::Int(!i64::from(*b))),
            _ => Err(bad("~")),
        },
    }
}

fn cmp_symbol(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Lt => "<",
        CmpOp::LtE => "<=",
        CmpOp::Gt => ">",
        CmpOp::GtE => ">=",
        CmpOp::Eq => "==",
        CmpOp::NotEq => "!=",
        CmpOp::Is => "is",
        CmpOp::IsNot => "is not",
        CmpOp::In => "in",
        CmpOp::NotIn => "not in",
    }
}

fn compare(op: CmpOp, a: &Value, b: &Value) -> PyResult<bool> {
    match op {
        CmpOp::Eq => Ok(py_eq(a, b)),
        CmpOp::NotEq => Ok(!py_eq(a, b)),
        CmpOp::Is => Ok(is(a, b)),
        CmpOp::IsNot => Ok(!is(a, b)),
        CmpOp::In => contains(b, a),
        CmpOp::NotIn => Ok(!contains(b, a)?),
        CmpOp::Lt | CmpOp::LtE | CmpOp::Gt | CmpOp::GtE => order(op, a, b),
    }
}

/// `item in container`.
fn contains(container: &Value, item: &Value) -> PyResult<bool> {
    let member = |items: &[Value]| items.iter().any(|x| is(x, item) || py_eq(x, item));
    match container {
        Value::List(l) => Ok(member(&l.borrow()[..])),
        Value::Tuple(t) => Ok(member(&t[..])),
        Value::Str(s) => match item {
            Value::Str(sub) => Ok(s.as_str().contains(sub.as_str())),
            _ => Err(type_error(format!(
                "'in <string>' requires string as left operand, not {}",
                item.type_name()
            ))),
        },
        Value::Dict(d) => Ok(d.borrow().contains(item)?),
        Value::Set(s) => Ok(s.borrow().contains(item)?),
        Value::Range(r) => match item {
            Value::Int(i) => Ok(r.contains_int(*i)),
            Value::Bool(b) => Ok(r.contains_int(i64::from(*b))),
            _ => Ok(member(&collect(container)?)),
        },
        Value::Bytes(b) => match as_index(item) {
            Some(i) if (0..256).contains(&i) => Ok(b.contains(&(i as u8))),
            Some(_) => Err(exc("ValueError", "byte must be in range(0, 256)")),
            None => match item {
                Value::Bytes(sub) => Ok(sub.is_empty() || b.windows(sub.len()).any(|w| w == &sub[..])),
                _ => Err(type_error(format!(
                    "a bytes-like object is required, not '{}'",
                    item.type_name()
                ))),
            },
        },
        _ => Err(type_error(format!("argument of type '{}' is not iterable", container.type_name()))),
    }
}

/// Resultado de `a op b` dado o `Ordering` entre eles.
fn apply(op: CmpOp, ord: std::cmp::Ordering) -> bool {
    use std::cmp::Ordering::*;
    match op {
        CmpOp::Lt => ord == Less,
        CmpOp::LtE => ord != Greater,
        CmpOp::Gt => ord == Greater,
        CmpOp::GtE => ord != Less,
        _ => false,
    }
}

/// `int` contra `float` sem perder precisão do inteiro.
fn int_float_cmp(i: i64, x: f64) -> Option<std::cmp::Ordering> {
    if x.is_nan() {
        return None;
    }
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    if x >= LIMIT {
        return Some(std::cmp::Ordering::Less);
    }
    if x < -LIMIT {
        return Some(std::cmp::Ordering::Greater);
    }
    let t = x.trunc();
    let ti = t as i64;
    Some(match i.cmp(&ti) {
        std::cmp::Ordering::Equal => 0.0_f64.partial_cmp(&(x - t)).unwrap_or(std::cmp::Ordering::Equal),
        other => other,
    })
}

/// Comparações de ordem (`<`, `<=`, `>`, `>=`).
fn order(op: CmpOp, a: &Value, b: &Value) -> PyResult<bool> {
    if let (Some(x), Some(y)) = (num(a), num(b)) {
        let ord = match (x, y) {
            (Num::Int(x), Num::Int(y)) => Some(x.cmp(&y)),
            (Num::Float(x), Num::Float(y)) => x.partial_cmp(&y),
            (Num::Int(i), Num::Float(x)) => int_float_cmp(i, x),
            (Num::Float(x), Num::Int(i)) => int_float_cmp(i, x).map(std::cmp::Ordering::reverse),
        };
        return Ok(ord.is_some_and(|o| apply(op, o)));
    }
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => Ok(apply(op, x.as_str().cmp(y.as_str()))),
        (Value::Bytes(x), Value::Bytes(y)) => Ok(apply(op, x[..].cmp(&y[..]))),
        (Value::List(x), Value::List(y)) => {
            let (x, y) = (x.borrow().clone(), y.borrow().clone());
            seq_order(op, &x, &y)
        }
        (Value::Tuple(x), Value::Tuple(y)) => seq_order(op, x, y),
        (Value::Set(_), Value::Set(_)) => Err(exc("NotImplementedError", "set ordering is not supported yet")),
        _ => Err(type_error(format!(
            "'{}' not supported between instances of '{}' and '{}'",
            cmp_symbol(op),
            a.type_name(),
            b.type_name()
        ))),
    }
}

/// Ordem lexicográfica de `list_richcompare`/`tuplerichcompare`: o primeiro par diferente decide;
/// sem diferença, decide o tamanho.
fn seq_order(op: CmpOp, x: &[Value], y: &[Value]) -> PyResult<bool> {
    for (p, q) in x.iter().zip(y) {
        if !(is(p, q) || py_eq(p, q)) {
            return order(op, p, q);
        }
    }
    Ok(apply(op, x.len().cmp(&y.len())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compile::compile_module;
    use crate::parser::parse_module;

    /// Executa `src` e devolve o stdout e, se houver, o traceback.
    fn run(src: &str) -> (String, Option<String>) {
        // Pilha grande: o limite de recursão de 1000 chamadas não cabe nos 2 MiB de uma thread de teste.
        let src = src.to_string();
        std::thread::Builder::new()
            .stack_size(1 << 30)
            .spawn(move || {
                let module = parse_module(&src).expect("parse");
                let code = compile_module(&module).expect("compile");
                let mut vm = Vm::new();
                let err = vm.run(&code).err().map(|e| format_traceback(&e));
                (String::from_utf8(vm.stdout).expect("utf-8"), err)
            })
            .expect("thread")
            .join()
            .expect("join")
    }

    fn out(src: &str) -> String {
        let (stdout, err) = run(src);
        assert_eq!(err, None, "programa: {src}");
        stdout
    }

    fn error(src: &str) -> String {
        run(src).1.expect("esperava exceção")
    }

    #[test]
    fn print_basics() {
        assert_eq!(out("print('hi')\n"), "hi\n");
        assert_eq!(out("print(1, 2, sep='-')\n"), "1-2\n");
        assert_eq!(out("print('a', 'b', end='')\nprint()\n"), "a b\n");
        assert_eq!(out("print(None, True, 1.5, [1, 'x'], (1,), {'k': 2})\n"), "None True 1.5 [1, 'x'] (1,) {'k': 2}\n");
        assert_eq!(out("print(1, 2, sep=None, end=None)\n"), "1 2\n");
        assert_eq!(out("print(print, len, str, range(3), range(1, 9, 2))\n"),
            "<built-in function print> <built-in function len> <class 'str'> range(0, 3) range(1, 9, 2)\n");
    }

    #[test]
    fn arithmetic_follows_cpython() {
        assert_eq!(out("print(7 // 2, -7 // 2, 7 % -3, -7 % 3, 2 ** 10, 2 ** -1)\n"), "3 -4 -2 2 1024 0.5\n");
        assert_eq!(out("print(1 / 2, 6 / 3, 0.1 + 0.2, -7.5 // 2, -7.5 % 2, 5 % -2.0)\n"),
            "0.5 2.0 0.30000000000000004 -4.0 0.5 -1.0\n");
        assert_eq!(out("print(True + True, -True, ~5, 1 << 4, -9 >> 1, 6 & 3, 6 | 3, 6 ^ 3, True & False)\n"),
            "2 -1 -6 16 -5 2 7 5 False\n");
        assert_eq!(out("print('ab' * 3, 2 * [0], (1, 2) + (3,), 'a' + 'b', [1] * -1)\n"),
            "ababab [0, 0] (1, 2, 3) ab []\n");
    }

    #[test]
    fn comparisons_and_logic() {
        assert_eq!(out("print(1 < 2 < 3, 1 < 3 < 2, 1 == 1.0, 'a' < 'b', [1, 2] < [1, 3], (1,) < (1, 0))\n"),
            "True False True True True True\n");
        assert_eq!(out("print(2 in [1, 2], 'b' in 'abc', 3 not in range(3), 1 is None, None is None)\n"),
            "True True True False True\n");
        assert_eq!(out("print(0 or 'x', 1 and 2, not [], 0 and 1 / 0, 'y' if 0 else 'n')\n"), "x 2 True 0 n\n");
    }

    #[test]
    fn statements() {
        let src = "total = 0\nfor i in range(5):\n    if i == 3:\n        continue\n    total += i\nprint(total)\n";
        assert_eq!(out(src), "7\n");
        let src = "n = 0\nwhile True:\n    n = n + 1\n    if n > 4:\n        break\nelse:\n    print('nunca')\nprint(n)\n";
        assert_eq!(out(src), "5\n");
        let src = "for c in 'hé':\n    print(c)\nelse:\n    print('fim')\n";
        assert_eq!(out(src), "h\né\nfim\n");
        let src = "for x in [1, 2]:\n    for y in [3]:\n        break\n    print(x)\n";
        assert_eq!(out(src), "1\n2\n");
        let src = "a, b = 1, 2\na, b = b, a\nx = y = [a]\nx[0] += 10\nd = {}\nd['k'] = b\nprint(a, b, y, d, d['k'])\n";
        assert_eq!(out(src), "2 1 [12] {'k': 1} 1\n");
        let src = "l = [1]\nm = l\nl += [2]\nprint(m, len(m), len('héllo'), len({1: 2}))\n";
        assert_eq!(out(src), "[1, 2] 2 5 1\n");
    }

    #[test]
    fn conversions() {
        assert_eq!(out("print(str(1) + str(2.5), int(' -12 '), int(3.9), int('1_000'), repr('x'), repr(1.0))\n"),
            "12.5 -12 3 1000 'x' 1.0\n");
        assert_eq!(out("print(str(), int(), str([1, 'a']))\n"), " 0 [1, 'a']\n");
    }

    #[test]
    fn exceptions() {
        assert_eq!(
            error("x = 1\nprint(x / 0)\n"),
            "Traceback (most recent call last):\n  File \"<string>\", line 2, in <module>\n\
             ZeroDivisionError: division by zero\n"
        );
        assert!(error("print(y)\n").ends_with("NameError: name 'y' is not defined\n"));
        assert!(error("1 + 'a'\n").ends_with("TypeError: unsupported operand type(s) for +: 'int' and 'str'\n"));
        assert!(error("'a' + 1\n").ends_with("TypeError: can only concatenate str (not \"int\") to str\n"));
        assert!(error("[1][5]\n").ends_with("IndexError: list index out of range\n"));
        assert!(error("{}['k']\n").ends_with("KeyError: 'k'\n"));
        assert!(error("int('abc')\n").ends_with("ValueError: invalid literal for int() with base 10: 'abc'\n"));
        assert!(error("1 < 'a'\n").ends_with("TypeError: '<' not supported between instances of 'int' and 'str'\n"));
        assert!(error("len(5)\n").ends_with("TypeError: object of type 'int' has no len()\n"));
        assert!(error("for x in 5:\n    pass\n").ends_with("TypeError: 'int' object is not iterable\n"));
        assert!(error("a, b = [1, 2, 3]\n").ends_with("ValueError: too many values to unpack (expected 2, got 3)\n"));
        assert!(error("range(1, 2, 0)\n").ends_with("ValueError: range() arg 3 must not be zero\n"));
        assert!(error("print(1, sep=2)\n").ends_with("TypeError: sep must be None or a string, not int\n"));
        assert!(error("5 % 0\n").ends_with("ZeroDivisionError: integer modulo by zero\n"));
        assert!(error("1\n\n(1)(2)\n").contains("line 3,"));
    }

    #[test]
    fn functions() {
        let src = "def add(a, b=10):\n    return a + b\nprint(add(1), add(1, 2), add(b=5, a=1))\n";
        assert_eq!(out(src), "11 3 6\n");
        let src = "def fib(n):\n    if n < 2:\n        return n\n    return fib(n - 1) + fib(n - 2)\nprint(fib(15))\n";
        assert_eq!(out(src), "610\n");
        let src = "count = 0\ndef inc():\n    global count\n    count += 1\ninc()\ninc()\nprint(count)\n";
        assert_eq!(out(src), "2\n");
        let src = "def f():\n    x = 1\n    for i in range(3):\n        x += i\n    return x\nprint(f(), f)\n";
        assert!(out(src).starts_with("4 <function f at 0x"));
        let src = "def f():\n    try:\n        return 1\n    finally:\n        print('fin')\nprint(f())\n";
        assert_eq!(out(src), "fin\n1\n");
        let src = "def f(n):\n    for i in range(10):\n        try:\n            if i == n:\n                return i\n        finally:\n            print('f', i)\nprint(f(1))\n";
        assert_eq!(out(src), "f 0\nf 1\n1\n");
        let src = "def f():\n    pass\nprint(f())\n";
        assert_eq!(out(src), "None\n");
        let src = "def f(a, b):\n    return a / b\ntry:\n    f(1, 0)\nexcept ZeroDivisionError as e:\n    print('caught', e)\n";
        assert_eq!(out(src), "caught division by zero\n");
        assert_eq!(
            error("def f():\n    return 1 / 0\ndef g():\n    return f()\ng()\n"),
            "Traceback (most recent call last):\n  File \"<string>\", line 5, in <module>\n  File \"<string>\", line 4, in g\n  \
             File \"<string>\", line 2, in f\nZeroDivisionError: division by zero\n"
        );
        assert!(error("def f(a):\n    pass\nf()\n").ends_with("TypeError: f() missing 1 required positional argument: 'a'\n"));
        assert!(error("def f(a):\n    pass\nf(1, 2)\n").ends_with("TypeError: f() takes 1 positional argument but 2 were given\n"));
        assert!(error("def f(a, b=1):\n    pass\nf(1, 2, 3)\n")
            .ends_with("TypeError: f() takes from 1 to 2 positional arguments but 3 were given\n"));
        assert!(error("def f(a):\n    pass\nf(1, a=2)\n").ends_with("TypeError: f() got multiple values for argument 'a'\n"));
        assert!(error("def f(a):\n    pass\nf(z=2)\n").ends_with("TypeError: f() got an unexpected keyword argument 'z'\n"));
        assert!(error("def f():\n    print(x)\n    x = 1\nf()\n")
            .ends_with("UnboundLocalError: cannot access local variable 'x' where it is not associated with a value\n"));
        assert!(error("def f():\n    return f()\nf()\n").ends_with("RecursionError: maximum recursion depth exceeded\n"));
    }

    #[test]
    fn try_except_flow() {
        let src = "try:\n    1 / 0\nexcept ZeroDivisionError as e:\n    print('z', e, repr(e), e.args)\n";
        assert_eq!(out(src), "z division by zero ZeroDivisionError('division by zero') ('division by zero',)\n");
        let src = "try:\n    {}['k']\nexcept LookupError as e:\n    print(repr(e), str(e))\n";
        assert_eq!(out(src), "KeyError('k') 'k'\n");
        let src = "try:\n    raise ValueError('boom')\nexcept (TypeError, ValueError) as e:\n    print('got', e)\nelse:\n    print('no')\nfinally:\n    print('fin')\n";
        assert_eq!(out(src), "got boom\nfin\n");
        let src = "try:\n    pass\nexcept:\n    print('no')\nelse:\n    print('else')\nfinally:\n    print('fin')\n";
        assert_eq!(out(src), "else\nfin\n");
        let src = "try:\n    try:\n        raise KeyError(1)\n    except ValueError:\n        print('inner')\nexcept Exception as e:\n    print('outer', type_ok)\n";
        assert!(error(src).ends_with("NameError: name 'type_ok' is not defined\n"));
        let src = "for i in range(3):\n    try:\n        if i == 1:\n            continue\n        if i == 2:\n            break\n    finally:\n        print('f', i)\nprint('end')\n";
        assert_eq!(out(src), "f 0\nf 1\nf 2\nend\n");
        let src = "try:\n    try:\n        raise ValueError('a')\n    except ValueError:\n        raise\nexcept ValueError as e:\n    print('re', e)\n";
        assert_eq!(out(src), "re a\n");
        let src = "try:\n    assert 1 == 2, 'nope'\nexcept AssertionError as e:\n    print(e)\nassert 0\n";
        let (stdout, err) = run(src);
        assert_eq!(stdout, "nope\n");
        assert!(err.expect("exceção").ends_with("AssertionError\n"));
        assert!(error("raise ValueError\n").ends_with("ValueError\n"));
        assert!(error("raise 5\n").ends_with("TypeError: exceptions must derive from BaseException\n"));
        assert!(error("try:\n    raise ValueError('x')\nfinally:\n    print('f')\n").ends_with("ValueError: x\n"));
    }

    #[test]
    fn exception_edge_cases() {
        // 1. except sem casamento propaga com traceback
        let msg = error("try:\n    raise KeyError(1)\nexcept ValueError:\n    print('no')\n");
        assert!(msg.contains("Traceback"));
        assert!(msg.ends_with("KeyError: 1\n"));
        // 2. finally depois de break dentro de while
        let src = "i = 0\nwhile True:\n    try:\n        i += 1\n        break\n    finally:\n        print('f', i)\nprint('end')\n";
        assert_eq!(out(src), "f 1\nend\n");
        // 3. try aninhado com raise dentro de except
        let src = "try:\n    try:\n        raise ValueError('a')\n    except ValueError:\n        raise KeyError('b')\nexcept KeyError as k:\n    print('k', k)\n";
        assert_eq!(out(src), "k 'b'\n");
        // 4. except com tupla
        let src = "try:\n    {}['x']\nexcept (ValueError, KeyError) as e:\n    print('t', repr(e))\n";
        assert_eq!(out(src), "t KeyError('x')\n");
        // 5. args com vários argumentos
        let src = "try:\n    raise ValueError(1, 2)\nexcept ValueError as e:\n    print(e.args, str(e))\n";
        assert_eq!(out(src), "(1, 2) (1, 2)\n");
        // 6. str(KeyError('a'))
        assert_eq!(out("print(str(KeyError('a')))\n"), "'a'\n");
        // 7. raise ValueError() sem mensagem
        let src = "try:\n    raise ValueError()\nexcept ValueError as e:\n    print(repr(str(e)), e.args)\n";
        assert_eq!(out(src), "'' ()\n");
        assert!(error("raise ValueError()\n").ends_with("ValueError\n"));
        // 8. assert sem mensagem
        let src = "try:\n    assert False\nexcept AssertionError as e:\n    print(repr(e), e.args)\n";
        assert_eq!(out(src), "AssertionError() ()\n");
        // 9. NameError capturada por except NameError
        let src = "try:\n    undefined_name\nexcept NameError as e:\n    print(e)\n";
        assert_eq!(out(src), "name 'undefined_name' is not defined\n");
        // 10. ZeroDivisionError por except ArithmeticError
        let src = "try:\n    1 / 0\nexcept ArithmeticError as e:\n    print(repr(e))\n";
        assert_eq!(out(src), "ZeroDivisionError('division by zero')\n");
        // 11. print(ValueError('x'))
        assert_eq!(out("print(ValueError('x'))\n"), "x\n");
        // 12. repr(Exception())
        assert_eq!(out("print(repr(Exception()))\n"), "Exception()\n");
    }
}
