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

use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{CmpOp, Operator, UnaryOp};
use crate::compile::{Code, Op};
use crate::object::{
    int_add, int_mul, int_neg, int_sub, is, py_eq, repr, to_str, Dict, ObjError, PyStr, Range, Set, Value,
};

/// Exceção Python levantada durante a execução: o nome da classe e a mensagem (`str(exc)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PyException {
    pub kind: &'static str,
    pub msg: String,
}

/// Exceção não tratada com a linha da instrução que a levantou.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeError {
    pub exc: PyException,
    pub lineno: usize,
}

type PyResult<T> = Result<T, PyException>;

fn exc(kind: &'static str, msg: impl Into<String>) -> PyException {
    PyException { kind, msg: msg.into() }
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

/// Traceback do CPython para exceção no nível de módulo de um `-c`, sem a linha fonte.
pub fn format_traceback(err: &RuntimeError) -> String {
    let mut out = String::from("Traceback (most recent call last):\n");
    out.push_str(&format!("  File \"<string>\", line {}, in <module>\n", err.lineno));
    if err.exc.msg.is_empty() {
        out.push_str(err.exc.kind);
        out.push('\n');
    } else {
        out.push_str(&format!("{}: {}\n", err.exc.kind, err.exc.msg));
    }
    out
}

/// Funções embutidas desta fatia.
const BUILTINS: &[&str] = &["print", "len", "range", "str", "int", "repr"];

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
}

impl PyIter {
    fn next(&mut self) -> Option<Value> {
        match self {
            PyIter::List(items, i) => {
                let v = items.borrow().get(*i).cloned()?;
                *i += 1;
                Some(v)
            }
            PyIter::Tuple(items, i) => {
                let v = items.get(*i).cloned()?;
                *i += 1;
                Some(v)
            }
            PyIter::Items(items, i) => {
                let v = items.get(*i).cloned()?;
                *i += 1;
                Some(v)
            }
            PyIter::Str(s, pos) => {
                let c = s.as_str()[*pos..].chars().next()?;
                *pos += c.len_utf8();
                Some(Value::str(c.to_string()))
            }
            PyIter::Range { next, step, remaining } => {
                if *remaining <= 0 {
                    return None;
                }
                let v = *next;
                *remaining -= 1;
                if *remaining > 0 {
                    *next += *step;
                }
                Some(Value::Int(v))
            }
        }
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
        _ => return Err(type_error(format!("'{}' object is not iterable", v.type_name()))),
    })
}

/// Todos os itens de um iterável.
fn collect(v: &Value) -> PyResult<Vec<Value>> {
    let mut it = get_iter(v)?;
    let mut out = Vec::new();
    while let Some(x) = it.next() {
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
#[derive(Default)]
pub struct Vm {
    globals: HashMap<String, Value>,
    pub stdout: Vec<u8>,
}

fn internal(msg: &str) -> PyException {
    exc("SystemError", msg.to_string())
}

impl Vm {
    pub fn new() -> Vm {
        Vm::default()
    }

    /// Executa o código de um módulo.
    pub fn run(&mut self, code: &Code) -> Result<(), RuntimeError> {
        let mut stack: Vec<Slot> = Vec::new();
        let mut pc = 0;
        while pc < code.ops.len() {
            let op = code.ops[pc];
            match self.step(code, op, &mut stack) {
                Ok(Some(target)) => pc = target,
                Ok(None) => pc += 1,
                Err(exc) => return Err(RuntimeError { exc, lineno: code.lines[pc] }),
            }
        }
        Ok(())
    }

    /// Executa uma instrução; `Some(alvo)` quando ela salta.
    fn step(&mut self, code: &Code, op: Op, stack: &mut Vec<Slot>) -> PyResult<Option<usize>> {
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
                        Some(b) => Value::Builtin(*b),
                        None => return Err(exc("NameError", format!("name '{name}' is not defined"))),
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
                    Some(Slot::Iter(it)) => it.next(),
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
        let Value::Builtin(name) = func else {
            return Err(type_error(format!("'{}' object is not callable", func.type_name())));
        };
        let name = *name;
        if name != "print" {
            if let Some((kw, _)) = kwargs.first() {
                return Err(type_error(match name {
                    "range" | "len" | "repr" => format!("{name}() takes no keyword arguments"),
                    _ => format!("'{kw}' is an invalid keyword argument for {name}()"),
                }));
            }
        }
        match name {
            "print" => self.print(args, kwargs),
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
            _ => Err(type_error(format!("'{}' object is not callable", func.type_name()))),
        }
    }

    /// `print(*args, sep=' ', end='\n', file=None, flush=False)`.
    fn print(&mut self, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
        let mut sep = " ".to_string();
        let mut end = "\n".to_string();
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
                "file" => return Err(exc("NotImplementedError", "print(file=...) is not supported yet")),
                "flush" => {}
                _ => return Err(type_error(format!("'{name}' is an invalid keyword argument for print()"))),
            }
        }
        let parts: Vec<String> = args.iter().map(to_str).collect();
        self.stdout.extend_from_slice(parts.join(&sep).as_bytes());
        self.stdout.extend_from_slice(end.as_bytes());
        Ok(Value::None)
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
            None => Err(exc("KeyError", repr(index))),
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
    if inplace {
        if let Value::List(l) = a {
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
        let module = parse_module(src).expect("parse");
        let code = compile_module(&module).expect("compile");
        let mut vm = Vm::new();
        let err = vm.run(&code).err().map(|e| format_traceback(&e));
        (String::from_utf8(vm.stdout).expect("utf-8"), err)
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
}
