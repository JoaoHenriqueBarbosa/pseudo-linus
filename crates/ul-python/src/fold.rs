//! Os consumidores nativos que puxam um iterável item a item (`sum`, `set`, `frozenset`, `dict`, `min`,
//! `max`, `any`, `all` e `list.extend`) como máquina de estados em dados.
//!
//! O CPython os escreve como um laço em C sobre `PyIter_Next`: cada item entra na conta antes de o iterador
//! produzir o seguinte, `any`/`all` param no primeiro item que decide e `list.extend` deixa na lista o que
//! já tinha entrado quando o iterador levanta. Escrever o laço em Rust faria a nativa esperar o gerador
//! dentro de uma chamada, e uma troca de thread ou um `os.fork` no meio do gerador não teria como suspender.
//! Aqui a conta é o [`Fold`]: o laço de quadros (`Vm::deliver`) entrega um item por vez a [`Fold::feed`] e
//! guarda o `Fold` em `ResumeUse::Collect` entre um item e o seguinte, então o estado em andamento é dado e
//! a imagem do heap o leva. O caminho síncrono (iterável que não é gerador) usa o mesmo `Fold` por [`run`].

use crate::object::{Dict, Set, Value};
use crate::vm::{internal, py_binary, py_lt, PyIter, PyResult, Vm};

/// A fase da soma de `sum`, como no `builtin_sum_impl` do CPython 3.12 em diante: inteiros de `i64` até
/// estourar, `float` com a soma compensada de Neumaier, e o `+` genérico para o resto.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SumPhase {
    Int(i64),
    Float { f: f64, c: f64 },
    Generic,
}

/// A conta de um consumidor nativo, com o que ela já acumulou.
pub(crate) enum Fold {
    /// `sum(it, start)`: `acc` vale na fase genérica e como o `start` antes dela.
    Sum { acc: Value, phase: SumPhase },
    /// `set(it)` e `frozenset(it)`: o conjunto em construção (congelado só no fim).
    Set { target: Value, frozen: bool },
    /// `dict(it, **kwargs)`: o dicionário em construção, a posição do próximo item (para a mensagem de
    /// erro) e os nomeados, aplicados depois dos itens.
    Dict { target: Value, kwargs: Vec<(String, Value)>, index: usize },
    /// `min(it, key=, default=)` e `max(...)`: o melhor até agora, com a chave dele.
    MinMax { max: bool, key: Option<Value>, default: Option<Value>, best: Option<(Value, Value)> },
    Any,
    All,
    /// `lista.extend(it)`: cada item entra na lista na hora, então uma falha no meio deixa os parciais.
    Extend { target: Value },
    /// `sorted(it, key=, reverse=)`: o CPython esgota o iterável antes de chamar a chave, então os itens
    /// entram em `items` e só no fim cada chave é chamada, na ordem, formando `pairs` (chave, item).
    Sorted { key: Option<Value>, reverse: bool, items: Vec<Value>, pairs: Vec<(Value, Value)> },
}

/// O que um item (ou o fim do iterável) faz com a conta: segue, ela já tem o resultado (curto-circuito de
/// `any`/`all`), ou pede a chave de um item, que roda código Python e por isso o laço de quadros a chama e
/// devolve por [`Fold::keyed`].
pub(crate) enum Flow {
    More,
    Done(Value),
    Key { func: Value, item: Value },
}

/// O nome da função embutida que `func` é (`Builtin` ou a `NativeFn` da tabela de `builtins`).
pub(crate) fn builtin_name(func: &Value) -> Option<&str> {
    static NAMES: std::sync::OnceLock<std::collections::HashSet<&'static str>> = std::sync::OnceLock::new();
    match func {
        Value::Builtin(name) => Some(*name),
        Value::NativeFn(f) => {
            let names = NAMES.get_or_init(|| crate::builtins::TABLE.iter().map(|(name, _)| *name).collect());
            names.contains(f.name).then_some(f.name)
        }
        _ => None,
    }
}

/// A conta que a chamada `func(*args, **kwargs)` faz sobre o iterável dela, e o iterável. `None` quando a
/// chamada não é de um desses consumidores, ou está malformada (aí o caminho comum dá o erro do CPython).
pub(crate) fn for_call(func: &Value, args: &[Value], kwargs: &[(String, Value)]) -> Option<(Value, Fold)> {
    if let Value::Bound(b) = func {
        return match (b.name, &b.recv, args, kwargs) {
            ("extend", Value::List(_), [source], []) => Some((source.clone(), Fold::Extend { target: b.recv.clone() })),
            _ => None,
        };
    }
    let name = builtin_name(func)?;
    match name {
        "sum" => {
            let (source, start) = match (args, kwargs) {
                ([source], []) => (source, None),
                ([source, start], []) => (source, Some(start)),
                ([source], [(key, start)]) if key.as_str() == "start" => (source, Some(start)),
                _ => return None,
            };
            let acc = start.cloned().unwrap_or(Value::Int(0));
            (!matches!(acc, Value::Str(_) | Value::Bytes(_) | Value::ByteArray(_))).then(|| (source.clone(), Fold::sum(acc)))
        }
        "min" | "max" => {
            let [source] = args else { return None };
            let mut key = None;
            let mut default = None;
            for (name, value) in kwargs {
                match name.as_str() {
                    "key" => key = (!matches!(value, Value::None)).then(|| value.clone()),
                    "default" => default = Some(value.clone()),
                    _ => return None,
                }
            }
            Some((source.clone(), Fold::min_max(name == "max", key, default)))
        }
        "any" | "all" => match (args, kwargs) {
            ([source], []) => Some((source.clone(), if name == "any" { Fold::Any } else { Fold::All })),
            _ => None,
        },
        "sorted" => {
            let [source] = args else { return None };
            let mut key = None;
            let mut reverse = false;
            for (name, value) in kwargs {
                match name.as_str() {
                    "key" => key = (!matches!(value, Value::None)).then(|| value.clone()),
                    "reverse" => reverse = value.is_true(),
                    _ => return None,
                }
            }
            Some((source.clone(), Fold::Sorted { key, reverse, items: Vec::new(), pairs: Vec::new() }))
        }
        "set" | "frozenset" => match (args, kwargs) {
            ([source], []) => Some((source.clone(), Fold::set(name == "frozenset"))),
            _ => None,
        },
        "dict" => {
            let [source] = args else { return None };
            Some((source.clone(), Fold::dict(kwargs.to_vec())))
        }
        _ => None,
    }
}

/// O total de `f` com a compensação `c` acumulada, como o CPython fecha a fase de ponto flutuante.
fn float_total(f: f64, c: f64) -> f64 {
    if c != 0.0 && c.is_finite() {
        f + c
    } else {
        f
    }
}

/// Um item de `sum`: leva a conta pelas fases (`SumPhase`); um item que a fase atual não soma é refeito na
/// seguinte, sem perdê-lo.
fn sum_feed(acc: &mut Value, phase: &mut SumPhase, item: &Value) -> PyResult<()> {
    loop {
        match *phase {
            SumPhase::Int(i) => match item {
                Value::Int(x) => match i.checked_add(*x) {
                    Some(r) => {
                        *phase = SumPhase::Int(r);
                        return Ok(());
                    }
                    None => {
                        *acc = Value::Int(i);
                        *phase = SumPhase::Generic;
                    }
                },
                Value::Float(_) => *phase = SumPhase::Float { f: i as f64, c: 0.0 },
                _ => {
                    *acc = Value::Int(i);
                    *phase = SumPhase::Generic;
                }
            },
            SumPhase::Float { f, c } => match item {
                Value::Float(x) => {
                    let x = *x;
                    let t = f + x;
                    let c = if f.abs() >= x.abs() { c + ((f - t) + x) } else { c + ((x - t) + f) };
                    *phase = SumPhase::Float { f: t, c };
                    return Ok(());
                }
                Value::Int(x) => {
                    *phase = SumPhase::Float { f: f + *x as f64, c };
                    return Ok(());
                }
                _ => {
                    *acc = Value::Float(float_total(f, c));
                    *phase = SumPhase::Generic;
                }
            },
            SumPhase::Generic => {
                *acc = py_binary("+", acc, item)?;
                return Ok(());
            }
        }
    }
}

impl Fold {
    /// `sum` a partir de `start`.
    pub(crate) fn sum(start: Value) -> Fold {
        let phase = match &start {
            Value::Int(i) => SumPhase::Int(*i),
            Value::Float(f) => SumPhase::Float { f: *f, c: 0.0 },
            _ => SumPhase::Generic,
        };
        Fold::Sum { acc: start, phase }
    }

    pub(crate) fn min_max(max: bool, key: Option<Value>, default: Option<Value>) -> Fold {
        Fold::MinMax { max, key, default, best: None }
    }

    pub(crate) fn set(frozen: bool) -> Fold {
        Fold::Set { target: Value::set(Set::new()), frozen }
    }

    pub(crate) fn dict(kwargs: Vec<(String, Value)>) -> Fold {
        Fold::Dict { target: Value::dict(Dict::default()), kwargs, index: 0 }
    }

    /// A conta chama uma função Python por item (`key=`), que o laço de quadros executa em quadro.
    pub(crate) fn calls_python(&self) -> bool {
        match self {
            Fold::MinMax { key: Some(func), .. } | Fold::Sorted { key: Some(func), .. } => crate::lazy::runs_python(func),
            _ => false,
        }
    }

    /// Põe `item` na conta.
    pub(crate) fn feed(&mut self, vm: &mut Vm, item: Value) -> PyResult<Flow> {
        match self {
            Fold::Sum { acc, phase } => sum_feed(acc, phase, &item)?,
            Fold::Set { target, .. } => {
                if let Value::Set(set) = target {
                    set.borrow_mut().add(item)?;
                }
            }
            Fold::Dict { target, index, .. } => {
                if let Value::Dict(dict) = target {
                    crate::builtins::dict_item(&mut dict.borrow_mut(), *index, item)?;
                }
                *index += 1;
            }
            Fold::MinMax { key: Some(func), .. } if crate::lazy::runs_python(func) => {
                return Ok(Flow::Key { func: func.clone(), item });
            }
            Fold::MinMax { max, key, best, .. } => {
                let k = match key {
                    Some(f) => vm.call(f, vec![item.clone()], Vec::new())?,
                    None => item.clone(),
                };
                minmax_take(*max, best, k, item)?;
            }
            Fold::Any if item.is_true() => return Ok(Flow::Done(Value::Bool(true))),
            Fold::All if !item.is_true() => return Ok(Flow::Done(Value::Bool(false))),
            Fold::Any | Fold::All => {}
            Fold::Extend { target } => {
                if let Value::List(list) = target {
                    list.borrow_mut().push(item);
                }
            }
            Fold::Sorted { items, .. } => items.push(item),
        }
        Ok(Flow::More)
    }

    /// A chave `key` de `item`, pedida por [`Flow::Key`], chegou.
    pub(crate) fn keyed(&mut self, vm: &mut Vm, item: Value, key: Value) -> PyResult<Flow> {
        match self {
            Fold::MinMax { max, best, .. } => {
                minmax_take(*max, best, key, item)?;
                Ok(Flow::More)
            }
            Fold::Sorted { pairs, .. } => {
                pairs.push((key, item));
                self.sorted_step(vm)
            }
            _ => Err(internal("a key for a fold that has none")),
        }
    }

    /// O próximo passo da ordenação com o iterável já esgotado: a chave do próximo item, se ela roda Python, ou
    /// a lista ordenada.
    fn sorted_step(&mut self, vm: &mut Vm) -> PyResult<Flow> {
        let Fold::Sorted { key, reverse, items, pairs } = self else { return Err(internal("not a sorted fold")) };
        if let Some(func) = key.as_ref().filter(|f| crate::lazy::runs_python(f)) {
            if let Some(item) = items.get(pairs.len()) {
                return Ok(Flow::Key { func: func.clone(), item: item.clone() });
            }
            let sorted = crate::builtins::sort_keyed(std::mem::take(pairs), *reverse)?;
            return Ok(Flow::Done(Value::list(sorted)));
        }
        let sorted = crate::builtins::sort_items(vm, std::mem::take(items), key.clone(), *reverse)?;
        Ok(Flow::Done(Value::list(sorted)))
    }

    /// O resultado quando o iterável acabou sem que a conta tivesse decidido antes (ou a chave do primeiro item
    /// da ordenação, se ela roda Python).
    pub(crate) fn finish(&mut self, vm: &mut Vm) -> PyResult<Flow> {
        if matches!(self, Fold::Sorted { .. }) {
            return self.sorted_step(vm);
        }
        let value = match self {
            Fold::Sum { acc, phase } => match *phase {
                SumPhase::Int(i) => Value::Int(i),
                SumPhase::Float { f, c } => Value::Float(float_total(f, c)),
                SumPhase::Generic => std::mem::replace(acc, Value::None),
            },
            Fold::Set { target, frozen } => match target {
                Value::Set(set) if *frozen => Value::frozenset(std::mem::replace(&mut *set.borrow_mut(), Set::new())),
                other => other.clone(),
            },
            Fold::Dict { target, kwargs, .. } => {
                if let Value::Dict(dict) = &*target {
                    for (name, value) in std::mem::take(kwargs) {
                        dict.borrow_mut().set(Value::str(name), value)?;
                    }
                }
                target.clone()
            }
            Fold::MinMax { max, default, best, .. } => match (best.take(), default.take()) {
                (Some((_, item)), _) => item,
                (None, Some(default)) => default,
                (None, None) => {
                    return Err(crate::native_util::value_error(format!(
                        "{}() iterable argument is empty",
                        if *max { "max" } else { "min" }
                    )));
                }
            },
            Fold::Any => Value::Bool(false),
            Fold::All => Value::Bool(true),
            Fold::Extend { .. } | Fold::Sorted { .. } => Value::None,
        };
        Ok(Flow::Done(value))
    }
}

/// O item `item` de chave `k` entra em `best` se for melhor (`>` no `max`, `<` no `min`; o empate fica com o
/// primeiro).
fn minmax_take(max: bool, best: &mut Option<(Value, Value)>, k: Value, item: Value) -> PyResult<()> {
    let better = match best {
        None => true,
        Some((best_key, _)) if max => crate::builtins::py_gt(&k, best_key)?,
        Some((best_key, _)) => py_lt(&k, best_key)?,
    };
    if better {
        *best = Some((k, item));
    }
    Ok(())
}

/// Resolve `flow` até a conta seguir (`None`) ou dar o resultado (`Some`), chamando as chaves na hora: o
/// caminho síncrono, sem laço de quadros.
fn settle(vm: &mut Vm, fold: &mut Fold, mut flow: Flow) -> PyResult<Option<Value>> {
    loop {
        match flow {
            Flow::More => return Ok(None),
            Flow::Done(result) => return Ok(Some(result)),
            Flow::Key { func, item } => {
                let key = vm.call(&func, vec![item.clone()], Vec::new())?;
                flow = fold.keyed(vm, item, key)?;
            }
        }
    }
}

/// Consome `it` inteiro com `fold`, sem suspender: o caminho dos iteráveis que não são geradores.
pub(crate) fn run(vm: &mut Vm, mut fold: Fold, mut it: PyIter) -> PyResult<Value> {
    while let Some(item) = it.next()? {
        let flow = fold.feed(vm, item)?;
        if let Some(result) = settle(vm, &mut fold, flow)? {
            return Ok(result);
        }
    }
    let flow = fold.finish(vm)?;
    settle(vm, &mut fold, flow)?.ok_or_else(|| internal("a fold finished without a result"))
}
