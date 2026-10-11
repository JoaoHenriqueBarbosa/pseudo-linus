//! `gc.get_referents`: o que o `tp_traverse` de cada tipo do CPython 3.13 visita, na ordem em que visita (medido no
//! oráculo, cenas `gc_referents` e `gc_referents_greenlet`). A VM conta referências, não tem lista de objetos do
//! coletor, então o percurso é feito aqui, tipo a tipo; o que ainda não tem tipo aqui devolve `[]`.

use std::rc::Rc;

use crate::object::{is_builtin_type, ClassObj, Descriptor, Dict, FuncObj, InstanceObj, ModuleObj, NativeFn, Value};
use crate::vm::Vm;

/// Os objetos que o objeto `value` referencia, como o `gc.get_referents(value)`.
pub(crate) fn referents(vm: &mut Vm, value: &Value) -> Vec<Value> {
    match value {
        // `list_traverse` e `tuple_traverse` andam do último item ao primeiro.
        Value::List(l) => l.borrow().iter().rev().cloned().collect(),
        Value::Tuple(t) => t.iter().rev().cloned().collect(),
        Value::Dict(d) => dict_referents(&d.borrow()),
        Value::Set(s) => s.borrow().iter().cloned().collect(),
        Value::Slice(s) => vec![s.0.clone(), s.1.clone(), s.2.clone()],
        // `method_traverse`: a função e o `self`.
        Value::BoundFn(b) => vec![Value::Function(b.1.clone()), b.0.clone()],
        // O método de um tipo embutido (`[].append`): o `m_self`.
        Value::Bound(b) => vec![b.recv.clone()],
        // `meth_traverse`: o `m_self` e o `m_module`. Uma função de módulo (`len`, `os.getcwd`) tem o módulo dela como
        // `m_self` e o nome dele como `m_module`; um tipo de C (`int`, `list`) é um tipo estático e não visita nada.
        Value::Builtin(_) => native_owner(vm, None),
        Value::NativeFn(n) if is_builtin_type(n.name) => Vec::new(),
        Value::NativeFn(n) => native_owner(vm, Some(n)),
        // O `m_dict` do módulo.
        Value::Module(m) => {
            let mut d = Dict::default();
            for (k, v) in m.attrs.borrow().iter() {
                let _ = d.set(Value::str(k.clone()), v.clone());
            }
            vec![Value::dict(d)]
        }
        Value::Function(f) => function_referents(vm, f),
        Value::Class(c) => class_referents(c),
        Value::Instance(i) => instance_referents(vm, i),
        Value::Ext(e) => match e.descriptor() {
            Some(descriptor) => descriptor_referents(vm, &descriptor),
            None if value.type_name() == "cell" => vm.load_attr(value, "cell_contents").ok().into_iter().collect(),
            None => e.referents(),
        },
        _ => Vec::new(),
    }
}

/// O módulo dono de uma função nativa e o nome dele: o `m_self` e o `m_module`. Sem `native` (`Value::Builtin`) é o
/// `builtins`; com ele, o primeiro módulo carregado (o `builtins` antes) cujo atributo é essa função.
fn native_owner(vm: &mut Vm, native: Option<&Rc<NativeFn>>) -> Vec<Value> {
    let builtins = crate::modules::import(vm, "builtins");
    let owns = |m: &Rc<ModuleObj>| match native {
        None => true,
        // A mesma função, não só o mesmo objeto: `len` avaliado de dentro de um escopo pode ser um `NativeFn` novo.
        Some(n) => m.attrs.borrow().values().any(|v| {
            matches!(v, Value::NativeFn(other) if Rc::ptr_eq(other, n) || (other.name == n.name && other.f as usize == n.f as usize))
        }),
    };
    let owner = builtins.filter(|m| owns(m)).or_else(|| vm.modules.borrow().values().find(|m| owns(m)).cloned());
    owner.map_or_else(Vec::new, |m| {
        let name = match m.attrs.borrow().get("__name__") {
            Some(Value::Str(s)) => Value::Str(s.clone()),
            _ => Value::str(m.name),
        };
        vec![Value::Module(m), name]
    })
}

/// `prop_traverse` (o `get`, o `set` e o `del` que existem) e `sm_traverse`/`cm_traverse` (o callable e o `__dict__`
/// que o `staticmethod` e o `classmethod` criam copiando `__module__`, `__name__`, `__qualname__`, `__doc__` e
/// `__wrapped__` do callable).
fn descriptor_referents(vm: &mut Vm, descriptor: &Descriptor) -> Vec<Value> {
    match descriptor {
        Descriptor::Property { get, set, del } => {
            [Some(get.clone()), set.clone(), del.clone()].into_iter().flatten().filter(|v| !matches!(v, Value::None)).collect()
        }
        Descriptor::Static(callable) | Descriptor::Class(callable) => {
            let mut wrapped = Dict::default();
            for name in ["__module__", "__name__", "__qualname__", "__doc__"] {
                if let Ok(v) = vm.load_attr(callable, name) {
                    let _ = wrapped.set(Value::str(name), v);
                }
            }
            let _ = wrapped.set(Value::str("__wrapped__"), callable.clone());
            vec![callable.clone(), Value::dict(wrapped)]
        }
    }
}

/// `dict_traverse`: só os valores quando todas as chaves são `str` (a tabela de chaves Unicode), e valor e chave de
/// cada entrada, nessa ordem, quando há outra chave.
fn dict_referents(d: &Dict) -> Vec<Value> {
    if d.keys().all(|k| matches!(k, Value::Str(_))) {
        return d.values().cloned().collect();
    }
    d.iter().flat_map(|(k, v)| [v.clone(), k.clone()]).collect()
}

/// `func_traverse`: o código, as globais, as `builtins`, o módulo, os defaults, os kwdefaults, a doc, o nome, o
/// `__dict__`, o closure, as anotações e o qualname, pulando o que é nulo (defaults, kwdefaults, closure, `__dict__` e
/// anotações ainda não criados). A doc é visitada mesmo sendo `None`.
fn function_referents(vm: &mut Vm, f: &Rc<FuncObj>) -> Vec<Value> {
    let function = Value::Function(f.clone());
    let mut out = Vec::new();
    for name in ["__code__", "__globals__", "__builtins__", "__module__"] {
        out.extend(vm.load_attr(&function, name).ok());
    }
    let present = |v: Option<Value>| v.filter(|v| !matches!(v, Value::None));
    out.extend(present(vm.load_attr(&function, "__defaults__").ok()));
    out.extend(present(vm.load_attr(&function, "__kwdefaults__").ok()));
    out.extend(vm.load_attr(&function, "__doc__").ok());
    out.extend(vm.load_attr(&function, "__name__").ok());
    if f.attrs.borrow().borrow().iter().next().is_some() {
        out.push(Value::Dict(f.attrs.borrow().clone()));
    }
    out.extend(present(vm.load_attr(&function, "__closure__").ok()));
    if let Some(annotations) = f.annotations.borrow().clone() {
        out.push(annotations);
    }
    out.extend(vm.load_attr(&function, "__qualname__").ok());
    out
}

/// `type_traverse`: o `tp_dict`, o `tp_mro`, o `tp_bases` e o `tp_base`.
fn class_referents(c: &Rc<ClassObj>) -> Vec<Value> {
    let mut d = Dict::default();
    for (k, v) in c.dict.borrow().iter() {
        let _ = d.set(Value::str(k.to_string()), v.clone());
    }
    let mut out = vec![
        Value::dict(d),
        Value::tuple(c.mro().into_iter().map(Value::Class).collect()),
        Value::tuple(c.bases.iter().cloned().map(Value::Class).collect()),
    ];
    out.push(c.bases.first().cloned().map_or(Value::Builtin("object"), Value::Class));
    out
}

/// `subtype_traverse`: os campos de `__slots__` de cada tipo da cadeia (do mais derivado ao mais base), o `__dict__`
/// (o dict real se já foi criado, senão os valores inline) e o tipo, e depois o percurso da base de C (o valor
/// embutido de uma subclasse de `list`, o estado de um greenlet). Um tipo de C estático (o greenlet em si) não é
/// visitado como tipo e não tem campos de `__slots__`: os do shim são estado interno.
fn instance_referents(vm: &mut Vm, i: &Rc<InstanceObj>) -> Vec<Value> {
    let class = i.class();
    if let Some(out) = itertools_referents(i) {
        return out;
    }
    let heap = !class.emulates_c_type();
    let attrs = i.dict.borrow();
    let greenlet = attrs.get(GREENLET_STATE).cloned();
    // Um tipo de C estático (`complex`, `datetime`) guarda o valor nos campos do shim: no CPython são bytes do
    // objeto, sem referência a visitar.
    if !heap && greenlet.is_none() {
        return i.payload.borrow().clone().map_or_else(Vec::new, |payload| referents(vm, &payload));
    }
    let mut out = Vec::new();
    let mut slot_names: Vec<String> = Vec::new();
    for klass in class.mro() {
        if klass.emulates_c_type() {
            continue;
        }
        for name in klass.declared_slots().unwrap_or_default() {
            if name == "__dict__" || name == "__weakref__" {
                continue;
            }
            out.extend(attrs.get(name.as_str()).cloned());
            slot_names.push(name);
        }
    }
    // O greenlet tem o `__dict__` dele na base de C: entra depois do tipo, junto do resto do percurso da base.
    if greenlet.is_none() {
        let view = i.view.borrow().clone();
        match view {
            Some(dict) => out.push(Value::Dict(dict)),
            None => out.extend(
                attrs.iter().filter(|(k, _)| !slot_names.iter().any(|s| s == k.as_ref() as &str)).map(|(_, v)| v.clone()),
            ),
        }
    }
    if heap {
        out.push(Value::Class(class.clone()));
    }
    if let Some(payload) = i.payload.borrow().clone() {
        out.extend(referents(vm, &payload));
    }
    if let Some(state) = greenlet {
        let user_attrs: Vec<(String, Value)> =
            attrs.iter().filter(|(k, _)| k.as_ref() as &str != GREENLET_STATE).map(|(k, v)| (k.to_string(), v.clone())).collect();
        let view = i.view.borrow().clone();
        return greenlet_referents(out, &state, view, &user_attrs);
    }
    out
}

/// O campo do shim do greenlet que guarda o estado (o `_greenlet__st` de `_greenlet_impl.py`).
const GREENLET_STATE: &str = "_greenlet__st";

/// `green_is_gc` e `green_traverse` do greenlet 3.5.6: o greenlet ativo (iniciado e não morto, que não é o principal)
/// não é do coletor e não referencia nada; os outros visitam o `__dict__`, o pai, a principal da thread (depois de
/// iniciado), o `run` ainda não consumido e o contexto, nessa ordem.
fn greenlet_referents(mut out: Vec<Value>, state: &Value, view: Option<Rc<std::cell::RefCell<Dict>>>, user_attrs: &[(String, Value)]) -> Vec<Value> {
    let Value::Instance(st) = state else { return out };
    let field = |inst: &Rc<InstanceObj>, name: &str| inst.dict.borrow().get(name).cloned().filter(|v| !matches!(v, Value::None));
    let flag = |name: &str| matches!(st.dict.borrow().get(name), Some(Value::Bool(true)));
    let (main, started, dead) = (flag("main"), flag("started"), flag("dead"));
    if started && !dead && !main {
        return Vec::new();
    }
    match view {
        Some(dict) => out.push(Value::Dict(dict)),
        None if !user_attrs.is_empty() => {
            let mut d = Dict::default();
            for (k, v) in user_attrs {
                let _ = d.set(Value::str(k.clone()), v.clone());
            }
            out.push(Value::dict(d));
        }
        None => {}
    }
    out.extend(field(st, "parent"));
    if dead {
        out.extend(main_greenlet(st));
    }
    if !started {
        // `_NoRun` marca o `run_callable` `NULL`; o `run=None` explícito é um objeto como outro qualquer.
        let run = st.dict.borrow().get("run").cloned().filter(|v| !matches!(v, Value::Instance(n) if n.class().name == "_NoRun"));
        out.extend(run);
    }
    out.extend(field(st, "context"));
    out
}

/// A principal da thread de um greenlet: o fim da cadeia de pais.
fn main_greenlet(state: &Rc<InstanceObj>) -> Option<Value> {
    let mut parent = state.dict.borrow().get("parent").cloned().filter(|v| !matches!(v, Value::None))?;
    loop {
        let Value::Instance(glet) = &parent else { return Some(parent) };
        let next_state = glet.dict.borrow().get(GREENLET_STATE).cloned();
        let Some(Value::Instance(next)) = next_state else { return Some(parent) };
        if matches!(next.dict.borrow().get("main"), Some(Value::Bool(true))) {
            return Some(parent);
        }
        let up = next.dict.borrow().get("parent").cloned().filter(|v| !matches!(v, Value::None));
        match up {
            Some(p) => parent = p,
            None => return Some(parent),
        }
    }
}

/// O `tp_traverse` dos tipos do `itertools` (e do `_tee_dataobject`) do CPython 3.13: o tipo, depois os campos na
/// ordem em que cada `*_traverse` os visita, sem os que estão `NULL`. O estado vive nos atributos privados das
/// classes de `itertools.py`, que o programa não enxerga. `None` quando `i` não é de `itertools`.
fn itertools_referents(i: &Rc<InstanceObj>) -> Option<Vec<Value>> {
    let class = i.class();
    if !matches!(class.dict.borrow().get("__module__"), Some(Value::Str(m)) if m.as_str() == "itertools") {
        return None;
    }
    let attrs = i.dict.borrow();
    // `_Nothing` marca o campo `NULL`; `None` do programa é um objeto como outro qualquer.
    let set = |name: &str| attrs.get(name).cloned().filter(|v| !matches!(v, Value::Instance(n) if n.class().name == "_Nothing"));
    let live = |name: &str| set(name).filter(|v| !matches!(v, Value::None));
    let started = matches!(attrs.get("_started"), Some(Value::Bool(true)));
    let result = || Value::tuple(vec![Value::None]);
    let mut out = vec![Value::Class(class.clone())];
    let fields: Vec<Option<Value>> = match class.name.as_ref() as &str {
        // `count` em modo rápido (contador e passo `int` de passo 1) só guarda o passo.
        "count" => match (set("_n"), set("_step")) {
            (Some(Value::Int(_)), step @ Some(Value::Int(1))) => vec![step],
            (n, step) => vec![n, step],
        },
        "repeat" => vec![set("_object")],
        "cycle" => vec![live("_it"), set("_saved")],
        "accumulate" => vec![live("_func"), set("_it"), set("_total"), set("_initial")],
        "chain" => vec![live("_iterables"), live("_current")],
        "compress" => vec![set("_data"), set("_selectors")],
        "dropwhile" | "takewhile" | "filterfalse" | "starmap" => vec![set("_it"), set("_func")],
        "groupby" => vec![set("_it"), set("_keyfunc"), set("_tgtkey"), set("_currkey"), set("_currvalue")],
        "islice" | "batched" => vec![live("_it")],
        "pairwise" => vec![live("_it"), set("_old"), Some(result())],
        "product" => {
            let pools = match set("_pools") {
                Some(Value::List(l)) => Some(Value::tuple(l.borrow().clone())),
                other => other,
            };
            vec![pools, started.then(result)]
        }
        "permutations" | "combinations" | "combinations_with_replacement" => vec![set("_pool"), started.then(result)],
        "zip_longest" => {
            let iterators = match set("_iterators") {
                Some(Value::List(l)) => Some(Value::tuple(l.borrow().clone())),
                other => other,
            };
            vec![iterators, Some(result()), set("_fill")]
        }
        "_tee" => vec![set("_data")],
        "_tee_dataobject" => {
            let mut fields = vec![set("_it")];
            if let Some(Value::List(l)) = set("_values") {
                fields.extend(l.borrow().iter().cloned().map(Some));
            }
            fields.push(live("_nextlink"));
            fields
        }
        _ => return None,
    };
    out.extend(fields.into_iter().flatten());
    Some(out)
}
