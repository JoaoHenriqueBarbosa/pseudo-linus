//! Replay: o que deixa uma nativa "pura até o fim" chamar código Python em quadro do laço, sem recursar.
//!
//! O problema: `'%s' % x`, `d[chave]`, `a == b` entre contêineres, `sorted(xs)`... chamam um método de usuário
//! (`__str__`, `__hash__`, `__eq__`, `__lt__`) de dentro de código Rust, que recursa um `run_loop`; ali o greenlet não
//! troca (a pilha de C que o CPython copia não existe aqui). Converter cada nativa numa máquina de continuações é
//! o caminho de `wip/notes/python-fork.md` (G4); este módulo é o caminho geral para as nativas que não mudam nada
//! antes da última chamada:
//!
//! 1. a nativa roda com um *memo* (os resultados das chamadas de usuário já feitas, em ordem);
//! 2. cada chamada de usuário que ela faz ([`intercept`], em `Vm::call_function`) devolve o próximo item do memo, ou,
//!    se o memo acabou, anota a chamada pedida e levanta a exceção de suspensão;
//! 3. com a suspensão anotada, o laço roda a chamada num quadro (onde o greenlet pode trocar), põe o resultado
//!    (ou a exceção) no memo e roda a nativa de novo, do começo.
//!
//! Cada chamada de usuário roda exatamente uma vez, na ordem que o CPython a faria; a nativa, determinística e sem
//! efeito antes da última chamada, refaz o mesmo caminho até o ponto seguinte. O que muda estado no meio (um
//! `print` com vários argumentos, `dict.update`) fica fora da lista de [`native_call`].

use std::cell::RefCell;
use std::rc::Rc;

use crate::compile::Op;
use crate::object::{FuncObj, Value};
use crate::vm::{exc, PyException, PyResult, Slot};

/// A chamada de usuário que a nativa pediu e o memo ainda não tinha.
pub(crate) struct Pending {
    pub(crate) func: Rc<FuncObj>,
    pub(crate) args: Vec<Value>,
    pub(crate) kwargs: Vec<(String, Value)>,
}

struct State {
    /// `rust_nest` em que a repetição roda: um `run_loop` aninhado (outro nível) não é interceptado.
    level: usize,
    memo: Vec<PyResult<Value>>,
    cursor: usize,
    need: Option<Pending>,
    /// Um iterador que já existia antes da passada (gerador, `list_iterator`, arquivo) avançou nela: repetir a nativa
    /// o avançaria de novo. Daí em diante a passada não se suspende mais, e as chamadas de usuário que faltam rodam
    /// como sempre (recursando), para que cada item seja lido uma única vez.
    advanced: bool,
}

thread_local! {
    static STATES: RefCell<Vec<State>> = const { RefCell::new(Vec::new()) };
}

/// Começa uma repetição no nível `level` com o `memo` já colhido.
pub(crate) fn begin(level: usize, memo: Vec<PyResult<Value>>) {
    STATES.with(|s| s.borrow_mut().push(State { level, memo, cursor: 0, need: None, advanced: false }));
}

/// Termina a repetição: o memo de volta e a chamada que faltou (`None`: a nativa terminou sozinha).
pub(crate) fn end() -> (Vec<PyResult<Value>>, Option<Pending>) {
    STATES.with(|s| {
        let st = s.borrow_mut().pop().expect("replay end without begin");
        (st.memo, st.need)
    })
}

/// Um iterador com estado próprio avançou (ver `State::advanced`). Sem repetição em curso, nada muda.
pub(crate) fn iterator_advanced() {
    STATES.with(|s| {
        if let Some(st) = s.borrow_mut().last_mut() {
            st.advanced = true;
        }
    });
}

/// O tipo da exceção que sobe de dentro da nativa até o laço quando falta uma chamada no memo: nunca chega ao programa.
pub(crate) const SUSPEND: &str = "_ReplaySuspend";

fn suspend() -> PyException {
    exc(SUSPEND, "")
}

/// `Vm::call_function` chamada de dentro de uma nativa repetida: o item do memo, ou a suspensão. `None` quando não há
/// repetição neste nível (a chamada roda como sempre).
pub(crate) fn intercept(level: usize, f: &Rc<FuncObj>, args: &[Value], kwargs: &[(String, Value)]) -> Option<PyResult<Value>> {
    STATES.with(|s| {
        let mut s = s.borrow_mut();
        let st = s.last_mut().filter(|st| st.level == level)?;
        if let Some(done) = st.memo.get(st.cursor) {
            let done = done.clone();
            st.cursor += 1;
            return Some(done);
        }
        if st.advanced {
            return None;
        }
        if st.need.is_none() {
            st.need = Some(Pending { func: f.clone(), args: args.to_vec(), kwargs: kwargs.to_vec() });
        }
        Some(Err(suspend()))
    })
}

/// Algum método de usuário pode ser chamado por causa de `v` (uma instância, ou um contêiner que guarda uma até
/// `depth` níveis abaixo).
pub(crate) fn may_call(v: &Value, depth: u32) -> bool {
    match v {
        Value::Instance(_) => true,
        _ if depth == 0 => false,
        Value::Tuple(t) => t.iter().any(|x| may_call(x, depth - 1)),
        Value::List(l) => l.borrow().iter().any(|x| may_call(x, depth - 1)),
        Value::Dict(d) => d.borrow().iter().any(|(k, x)| may_call(k, depth - 1) || may_call(x, depth - 1)),
        Value::Set(s) => s.borrow().iter().any(|x| may_call(x, depth - 1)),
        Value::Slice(s) => [&s.0, &s.1, &s.2].into_iter().any(|x| may_call(x, depth - 1)),
        Value::Ext(e) => e.to_items().is_some_and(|items| items.iter().any(|x| may_call(x, depth - 1))),
        _ => false,
    }
}

/// Os nomes das funções embutidas que `native_call` repete.
const BUILTINS: &[&str] = &[
    "format", "sorted", "min", "max", "sum", "dict", "set", "frozenset", "list", "tuple", "str", "repr", "ascii", "hash", "getattr",
    "hasattr", "divmod", "hex", "bin", "oct", "range", "int", "float", "round", "abs", "len", "bool", "print",
];

/// Os métodos nativos que `native_call` repete: leem a chave (e, no fim, trocam uma entrada).
const METHODS: &[&str] = &[
    "format", "format_map", "get", "pop", "setdefault", "add", "discard", "remove", "__contains__", "__getitem__", "__eq__", "join",
    "union", "intersection", "difference", "symmetric_difference", "issubset", "issuperset", "isdisjoint", "copy", "count", "index",
    "__setitem__", "__delitem__",
];

/// As funções internas dos módulos embutidos que `native_call` repete (o laço de `_count_elements`).
const INTERNAL: &[&str] = &["_dict_get_known", "_dict_set_known"];

/// A chamada `func(*args)` é de uma nativa que se repete, e algum argumento pode chamar método de usuário.
pub(crate) fn native_call(func: &Value, args: &[Value], kwargs: &[(String, Value)]) -> bool {
    let listed = match func {
        Value::Bound(b) => METHODS.contains(&b.name),
        Value::NativeFn(f) if crate::builtins_ext::is_builtin(func, f.name) => BUILTINS.contains(&f.name),
        Value::NativeFn(f) if INTERNAL.contains(&f.name) => true,
        other => crate::fold::builtin_name(other).is_some_and(|n| BUILTINS.contains(&n)),
    };
    listed
        && (args.iter().any(|a| may_call(a, 2))
            || kwargs.iter().any(|(_, v)| may_call(v, 2))
            || matches!(func, Value::Bound(b) if may_call(&b.recv, 2)))
}

/// A instrução do laço se repete sobre os operandos: quantos ela consome (`None`: não é uma das que se repetem, ou
/// nenhum operando chama método de usuário).
pub(crate) fn step_operands(op: Op, stack: &[Slot]) -> Option<usize> {
    let top = |n: usize| match stack.len().checked_sub(n + 1).map(|i| &stack[i]) {
        Some(Slot::Val(v)) => Some(v),
        _ => None,
    };
    let any = |count: usize, depth: u32| (0..count).any(|i| top(i).is_some_and(|v| may_call(v, depth)));
    match op {
        Op::Compare(_) | Op::Binary { .. } => (stack.len() >= 2 && any(2, 2)).then_some(2),
        Op::Subscript => (stack.len() >= 2 && top(0).is_some_and(|v| may_call(v, 1))).then_some(2),
        Op::StoreSubscript => (stack.len() >= 3 && top(0).is_some_and(|v| may_call(v, 1))).then_some(3),
        Op::DeleteSubscript => (stack.len() >= 2 && top(0).is_some_and(|v| may_call(v, 1))).then_some(2),
        Op::FormatValue { has_spec, .. } => {
            let n = if has_spec { 2 } else { 1 };
            (stack.len() >= n && any(n, 1)).then_some(n)
        }
        Op::BuildSet(n) => (stack.len() >= n as usize && any(n as usize, 1)).then_some(n as usize),
        Op::BuildDict(n) => (stack.len() >= 2 * n as usize && any(2 * n as usize, 1)).then_some(2 * n as usize),
        _ => None,
    }
}

/// Já há uma repetição no nível `level`: a nativa que ela roda chama outra nativa, que segue sem repetição própria
/// (as chamadas de usuário dela vão para o memo da de fora).
pub(crate) fn active(level: usize) -> bool {
    STATES.with(|s| s.borrow().last().is_some_and(|st| st.level == level))
}

/// A repetição corrente já pediu uma chamada de usuário: a nativa não pode produzir efeito (escrever, mudar estado),
/// porque vai rodar de novo.
pub(crate) fn suspended(level: usize) -> bool {
    STATES.with(|s| s.borrow().last().is_some_and(|st| st.level == level && st.need.is_some()))
}

/// A exceção de suspensão, para a nativa que interrompe a si mesma antes de um efeito (ver [`suspended`]).
pub(crate) fn suspension() -> PyException {
    suspend()
}

/// O nome da nativa repetida, para o `CallLink::c_entry`: a embutida (`format`, `print`) ou o método (`str.format`).
pub(crate) fn callee_name(func: &Value) -> String {
    match func {
        Value::Bound(b) => b.name.to_string(),
        Value::NativeFn(f) => f.name.to_string(),
        other => crate::fold::builtin_name(other).unwrap_or("native").to_string(),
    }
}

/// O nome da instrução repetida, para o `CallLink::c_entry`.
pub(crate) fn op_name(op: &Op) -> &'static str {
    match op {
        Op::Compare(_) => "compare",
        Op::Binary { .. } => "binary",
        Op::Subscript => "subscript",
        Op::StoreSubscript => "store_subscript",
        Op::DeleteSubscript => "delete_subscript",
        Op::FormatValue { .. } => "format_value",
        Op::BuildSet(_) => "build_set",
        Op::BuildDict(_) => "build_dict",
        _ => "op",
    }
}
