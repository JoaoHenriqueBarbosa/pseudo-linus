//! Funções embutidas que dependem da VM inteira: `setattr`, `delattr`, `slice`, `vars`, `dir`,
//! `globals`, `format`, `input`, `exit`, `quit`, `__import__`, `eval` e `exec`.

use std::rc::Rc;


use crate::native_util::{bind, want_str};
use crate::object::{Dict, Kw, NativeFnPtr, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

pub const TABLE: &[(&str, NativeFnPtr)] = &[
    ("setattr", b_setattr),
    ("delattr", b_delattr),
    ("slice", b_slice),
    ("vars", b_vars),
    ("dir", b_dir),
    ("globals", b_globals),
    // Fora de função `locals()` é o mesmo dicionário das globais; dentro, o compilador emite `Op::Locals`.
    ("locals", b_locals),
    ("format", b_format),
    ("input", b_input),
    ("exit", b_exit),
    ("quit", b_exit),
    ("help", b_help),
    ("__import__", b_import),
    ("eval", b_eval),
    ("exec", b_exec),
    ("compile", b_compile),
    ("aiter", b_aiter),
    ("anext", b_anext),
    ("breakpoint", b_breakpoint),
];

/// `aiter(obj)`: o `__aiter__` do objeto, que precisa devolver um iterador assíncrono.
fn b_aiter(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("aiter", &kw)?;
    if args.len() != 1 {
        return Err(type_error(format!("aiter() takes exactly one argument ({} given)", args.len())));
    }
    let obj = &args[0];
    let Ok(method) = vm.load_attr(obj, "__aiter__") else {
        return Err(type_error(format!("'{}' object is not an async iterable", obj.type_name())));
    };
    let it = vm.call(&method, Vec::new(), Vec::new())?;
    if vm.load_attr(&it, "__anext__").is_err() {
        return Err(type_error(format!("aiter() returned not an async iterator of type '{}'", it.type_name())));
    }
    Ok(it)
}

/// `anext(iterador[, padrão])`: o aguardável do `__anext__`; com padrão, um `anext_awaitable`
/// que troca o `StopAsyncIteration` pelo padrão.
fn b_anext(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("anext", &kw)?;
    if args.is_empty() || args.len() > 2 {
        return Err(type_error(format!("anext expected at least 1 argument, got {}", args.len())));
    }
    let it = &args[0];
    let Ok(method) = vm.load_attr(it, "__anext__") else {
        return Err(type_error(format!("'{}' object is not an async iterator", it.type_name())));
    };
    let awaitable = vm.call(&method, Vec::new(), Vec::new())?;
    let Some(default) = args.get(1) else { return Ok(awaitable) };
    let module = crate::modules::import_value(vm, "_anext")?;
    let cls = vm.load_attr(&module, "anext_awaitable")?;
    vm.call(&cls, vec![awaitable, default.clone()], Vec::new())
}

/// Emite o evento de auditoria `event` com `arg` pelo `sys.audit` de `sys`.
fn audit_event(vm: &mut Vm, sys: &Value, event: &str, arg: Value) -> PyResult<()> {
    let audit = vm.load_attr(sys, "audit")?;
    vm.call(&audit, vec![Value::str(event), arg], Vec::new())?;
    Ok(())
}

/// `breakpoint(*args, **kws)`: chama `sys.breakpointhook`.
fn b_breakpoint(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let sys = crate::modules::import_value(vm, "sys")?;
    let Ok(hook) = vm.load_attr(&sys, "breakpointhook") else {
        return Err(exc("RuntimeError", "lost sys.breakpointhook"));
    };
    audit_event(vm, &sys, "builtins.breakpoint", hook.clone())?;
    vm.call(&hook, args, kw)
}

fn attr_name(v: &Value) -> PyResult<String> {
    match v {
        Value::Str(s) => Ok(s.as_str().to_string()),
        other => Err(type_error(format!("attribute name must be string, not '{}'", other.type_name()))),
    }
}

fn b_setattr(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("setattr", args, kw, &["obj", "name", "value"], 3)?;
    let (obj, name, value) = (a[0].clone().unwrap_or(Value::None), a[1].clone().unwrap_or(Value::None), a[2].clone().unwrap_or(Value::None));
    vm.store_attr(&obj, &attr_name(&name)?, value)?;
    Ok(Value::None)
}

fn b_delattr(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("delattr", args, kw, &["obj", "name"], 2)?;
    let (obj, name) = (a[0].clone().unwrap_or(Value::None), a[1].clone().unwrap_or(Value::None));
    vm.delete_attr(&obj, &attr_name(&name)?)?;
    Ok(Value::None)
}

fn b_slice(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("slice", &kw)?;
    let (lo, hi, step) = match args.as_slice() {
        [stop] => (Value::None, stop.clone(), Value::None),
        [start, stop] => (start.clone(), stop.clone(), Value::None),
        [start, stop, step] => (start.clone(), stop.clone(), step.clone()),
        other => {
            return Err(type_error(format!(
                "slice expected at most 3 arguments, got {}",
                other.len()
            )))
        }
    };
    Ok(Value::Slice(Rc::new((lo, hi, step))))
}

fn b_vars(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("vars", &kw)?;
    match args.first() {
        None => b_globals(vm, Vec::new(), Vec::new()),
        Some(obj) => vm
            .load_attr(obj, "__dict__")
            .map_err(|_| type_error("vars() argument must have __dict__ attribute")),
    }
}

fn b_globals(vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(crate::globalsview::view_for(&vm.globals, None))
}

/// As variáveis do quadro de função que chamou a nativa; `None` quando o chamador é um módulo ou uma classe.
fn caller_function_locals(vm: &mut Vm) -> PyResult<Option<Value>> {
    // Chamada por referência (`f = locals; f()`) dentro de função: as variáveis do quadro do chamador.
    let caller = vm.frames.borrow().last().map(|(code, _, env)| (code.clone(), env.clone()));
    match caller {
        Some((code, env)) if !env.is_module && !env.is_class => Ok(Some(Value::dict(crate::frameobj::locals_dict(&code, &env)?))),
        _ => Ok(None),
    }
}

/// `locals()` no nível do módulo: as globais, como no CPython. É uma função à parte de `globals`
/// porque cada uma tem a sua docstring (registrada pelo endereço da função).
fn b_locals(vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(match caller_function_locals(vm)? {
        Some(locals) => locals,
        None => crate::globalsview::view_for(&vm.globals, None),
    })
}

/// Nomes que os protocolos (`collections.abc`, `numbers`, `io`) e os tipos embutidos costumam expor: o
/// interpretador não enumera os métodos de um tipo embutido, então `dir`/`__dict__` sondam esta lista.
const PROBE_NAMES: &[&str] = &[
    "__abs__", "__add__", "__aenter__", "__aexit__", "__aiter__", "__and__", "__anext__", "__await__",
    "__bool__", "__buffer__", "__bytes__", "__call__", "__class_getitem__", "__contains__", "__delitem__",
    "__enter__", "__eq__", "__exit__", "__float__", "__floordiv__", "__format__", "__fspath__", "__ge__",
    "__getitem__", "__gt__", "__hash__", "__iadd__", "__index__", "__init__", "__int__", "__invert__",
    "__iter__", "__le__", "__len__", "__lshift__", "__lt__", "__mod__", "__mul__", "__ne__", "__neg__",
    "__next__", "__or__", "__pos__", "__pow__", "__radd__", "__repr__", "__reversed__", "__rmul__",
    "__rshift__", "__setitem__", "__str__", "__sub__", "__truediv__", "__xor__", "__length_hint__",
    "append", "clear", "close", "copy", "count", "decode", "encode", "extend", "get", "index", "insert",
    "items", "join", "keys", "pop", "popitem", "remove", "reverse", "send", "setdefault", "sort", "split",
    "throw", "update", "values", "add", "discard", "difference", "intersection", "union", "isdisjoint",
    "issubset", "issuperset", "read", "readable", "readline", "readlines", "seek", "seekable", "tell",
    "truncate", "writable", "write", "writelines", "flush", "fileno", "isatty", "detach", "real", "imag",
    "numerator", "denominator", "conjugate", "bit_length", "bit_count", "to_bytes", "from_bytes", "startswith",
    "endswith", "strip", "replace", "format", "lower", "upper", "find", "fromkeys", "move_to_end",
];

/// Atributos de um tipo embutido que o interpretador resolve, na ordem de `PROBE_NAMES`.
pub(crate) fn probe_type_attrs(vm: &mut Vm, ty: &Value) -> Vec<(String, Value)> {
    PROBE_NAMES.iter().filter_map(|n| vm.load_attr(ty, n).ok().map(|v| ((*n).to_string(), v))).collect()
}

/// Os operadores de `object` que o tipo `name` redefine no próprio `__dict__` (o resto de
/// `OBJECT_ATTRS` é herdado). Os tipos não listados redefinem só `__new__` e `__repr__`.
fn own_object_slots(name: &str) -> &'static str {
    match name {
        "int" | "float" => "__new__ __repr__ __getattribute__ __lt__ __le__ __eq__ __ne__ __gt__ __ge__ __hash__ __format__ __sizeof__",
        "complex" => "__new__ __repr__ __getattribute__ __lt__ __le__ __eq__ __ne__ __gt__ __ge__ __hash__ __format__",
        "str" => "__new__ __repr__ __getattribute__ __lt__ __le__ __eq__ __ne__ __gt__ __ge__ __hash__ __str__ __format__ __sizeof__",
        "bytes" => "__new__ __repr__ __getattribute__ __lt__ __le__ __eq__ __ne__ __gt__ __ge__ __hash__ __str__ __sizeof__",
        "bytearray" => "__new__ __repr__ __getattribute__ __lt__ __le__ __eq__ __ne__ __gt__ __ge__ __hash__ __str__ __init__ __sizeof__",
        "list" | "dict" | "set" => "__new__ __repr__ __getattribute__ __lt__ __le__ __eq__ __ne__ __gt__ __ge__ __hash__ __init__ __sizeof__",
        "tuple" | "range" | "slice" | "memoryview" => "__new__ __repr__ __getattribute__ __lt__ __le__ __eq__ __ne__ __gt__ __ge__ __hash__",
        "frozenset" => "__new__ __repr__ __getattribute__ __lt__ __le__ __eq__ __ne__ __gt__ __ge__ __hash__ __sizeof__",
        "bool" => "__new__ __repr__ __and__ __rand__ __or__ __ror__ __xor__ __rxor__",
        "NoneType" | "ellipsis" | "NotImplementedType" => "__new__ __repr__",
        "property" | "classmethod" | "staticmethod" | "super" | "type" => "__new__ __init__ __getattribute__",
        "BaseException" => "__new__ __init__ __repr__ __str__ __getattribute__ __reduce__",
        "KeyError" | "SyntaxError" | "UnicodeEncodeError" | "UnicodeDecodeError" | "UnicodeTranslateError" => {
            "__new__ __init__ __str__"
        }
        "OSError" | "ImportError" => "__new__ __init__ __str__ __reduce__",
        n if crate::object::EXC_CLASSES.iter().any(|(e, _)| *e == n) => "__new__ __init__",
        _ => "__new__ __repr__",
    }
}

/// Os tipos de sequência: `__add__`, `__mul__` e companhia ficam depois dos de mapeamento.
const SEQUENCE_TYPES: &[&str] = &["list", "tuple", "str", "bytes", "bytearray"];

/// A ordem em que o CPython põe os operadores no `__dict__` de um tipo estático (a das posições
/// dos `tp_*` e `nb_*` na estrutura do tipo); `__new__` abre e `__doc__` fecha.
const SLOT_ORDER: &[&str] = &[
    "__repr__", "__hash__", "__call__", "__str__", "__getattribute__", "__setattr__", "__delattr__", "__lt__",
    "__le__", "__eq__", "__ne__", "__gt__", "__ge__", "__iter__", "__next__", "__get__", "__set__", "__delete__",
    "__init__", "__await__", "__aiter__", "__anext__", "__add__", "__radd__", "__sub__", "__rsub__", "__mul__",
    "__rmul__", "__mod__", "__rmod__", "__divmod__", "__rdivmod__", "__pow__", "__rpow__", "__neg__", "__pos__",
    "__abs__", "__bool__", "__invert__", "__lshift__", "__rlshift__", "__rshift__", "__rrshift__", "__and__",
    "__rand__", "__xor__", "__rxor__", "__or__", "__ror__", "__int__", "__float__", "__iadd__", "__isub__",
    "__imul__", "__imod__", "__ipow__", "__ilshift__", "__irshift__", "__iand__", "__ixor__", "__ior__",
    "__floordiv__", "__rfloordiv__", "__truediv__", "__rtruediv__", "__ifloordiv__", "__itruediv__", "__index__",
    "__matmul__", "__rmatmul__", "__imatmul__", "__len__", "__getitem__", "__setitem__", "__delitem__",
];

/// O fim da tabela de slots: o que vem do `tp_as_sequence`.
const SEQUENCE_TAIL: &[&str] = &["__add__", "__mul__", "__rmul__", "__contains__", "__iadd__", "__imul__"];

fn own_key_rank(name: &str, index: usize, sequence: bool) -> usize {
    let tail = SEQUENCE_TAIL.iter().position(|t| *t == name);
    match name {
        "__new__" => 0,
        "__doc__" => usize::MAX,
        _ if tail.is_some() && (sequence || name == "__contains__") => 1 + SLOT_ORDER.len() + tail.unwrap_or(0),
        n => match SLOT_ORDER.iter().position(|s| *s == n) {
            Some(i) => 1 + i,
            None => 1 + SLOT_ORDER.len() + SEQUENCE_TAIL.len() + index,
        },
    }
}

/// As chaves do `__dict__` do tipo embutido `name`: só as do próprio tipo, na ordem do CPython.
pub(crate) fn own_type_keys(name: &str) -> Option<Vec<&'static str>> {
    // A tabela do oráculo é a ordem exata; sem linha nela (ou antes de ser gerada), a heurística abaixo.
    if let Some(keys) = type_vars(name) {
        return Some(keys);
    }
    if name == "object" {
        return Some(OBJECT_DICT_ORDER.to_vec());
    }
    let listed = type_dir(name)?;
    let parent = exception_or_bool_parent(name);
    let inherited = parent.and_then(type_dir).unwrap_or_default();
    let mut own: Vec<&'static str> = if name == "object" {
        listed
    } else {
        let overrides: Vec<&str> = own_object_slots(name).split(' ').collect();
        listed
            .into_iter()
            .filter(|n| {
                *n == "__doc__"
                    || overrides.contains(n)
                    || (!OBJECT_ATTRS.contains(n) && !inherited.contains(n))
            })
            .collect()
    };
    // Os métodos de `object` que o tipo não redefine saem; `__hash__` fica só nos tipos com ele.
    let sequence = SEQUENCE_TYPES.contains(&name);
    let mut ranked: Vec<(usize, &'static str)> =
        own.drain(..).enumerate().map(|(i, n)| (own_key_rank(n, i, sequence), n)).collect();
    ranked.sort_by_key(|(r, _)| *r);
    Some(ranked.into_iter().map(|(_, n)| n).collect())
}

/// `T.__dict__` de um tipo embutido: um `mappingproxy` com o que é do próprio tipo.
pub(crate) fn type_own_dict(vm: &mut Vm, ty: &Value) -> PyResult<Value> {
    let name = match ty {
        Value::Builtin("type") => "type",
        _ => crate::builtins::class_name(ty).unwrap_or("object"),
    };
    let mut d = Dict::default();
    match own_type_keys(name) {
        Some(keys) => {
            for k in keys {
                // Os tipos sem hash guardam `None` em `__hash__`.
                let unhashable = k == "__hash__" && matches!(name, "list" | "dict" | "set" | "bytearray");
                let value = if unhashable {
                    Some(Value::None)
                } else if k == "__dict__" {
                    // O `__dict__` de `function`, `type`...: o `getset_descriptor` do tipo. Carregar o atributo
                    // reentraria neste mesmo `__dict__`, sem fim.
                    Some(crate::classes::builtin_getset_descriptor("__dict__", ty.clone()))
                } else if let Some(descriptor) = crate::typeattrs::descriptor_for_kind(name, k, ty) {
                    Some(descriptor)
                } else {
                    vm.load_attr(ty, k).ok()
                };
                if let Some(v) = value {
                    d.set(Value::str(k), v)?;
                }
            }
        }
        None => {
            for (k, v) in probe_type_attrs(vm, ty) {
                d.set(Value::str(k), v)?;
            }
        }
    }
    mapping_proxy(vm, Value::dict(d))
}

/// O `mappingproxy` somente leitura sobre `mapping`: o que `vars(T)` devolve num tipo e o `mapping` de uma
/// view de dicionário.
pub(crate) fn mapping_proxy(vm: &mut Vm, mapping: Value) -> PyResult<Value> {
    let class = crate::modules::import(vm, "_mappingproxy")
        .and_then(|m| m.attrs.borrow().get("mappingproxy").cloned())
        .ok_or_else(|| exc("NameError", "name 'mappingproxy' is not defined"))?;
    vm.call(&class, vec![mapping], Vec::new())
}

/// O nome do tipo de `builtins` que o shim em Python `cls` (`memoryview`, `complex`, os grupos de exceção)
/// emula: só uma classe do módulo `builtins` com linha na tabela do oráculo.
fn emulated_name(cls: &Rc<crate::object::ClassObj>) -> Option<&'static str> {
    (cls.module() == "builtins").then(|| crate::object::intern(&cls.name)).filter(|n| type_vars(n).is_some())
}

/// `vars(T)` de um shim em Python de um tipo de `builtins`: as chaves e os tipos de descritor do oráculo, no
/// lugar do dicionário da classe (que tem `__module__`, `__firstlineno__`, funções comuns e os auxiliares
/// privados do shim, nada do que o tipo em C mostra). Só o que o oráculo não guarda como descritor (a
/// docstring) vem do próprio shim.
pub(crate) fn emulated_type_dict(vm: &mut Vm, cls: &Rc<crate::object::ClassObj>) -> PyResult<Option<Value>> {
    let Some(tname) = emulated_name(cls) else { return Ok(None) };
    let ty = Value::Class(cls.clone());
    let mut d = Dict::default();
    for key in type_vars(tname).unwrap_or_default() {
        let value = match crate::typeattrs::descriptor_for_kind(tname, key, &ty) {
            Some(descriptor) => descriptor,
            None if key == "__doc__" => crate::modules::cpydocs::builtin_doc(tname).map_or(Value::None, Value::str),
            None => match cls.dict.borrow().get(key) {
                Some(own) => own.clone(),
                None => continue,
            },
        };
        d.set(Value::str(key), value)?;
    }
    mapping_proxy(vm, Value::dict(d)).map(Some)
}

/// `T.nome` num shim em Python de um tipo de `builtins` quando o oráculo guarda `nome` como descritor do
/// tipo (método, wrapper de slot ou campo): o descritor de tipo embutido, no lugar da função do shim; e o
/// `__text_signature__` da tabela do oráculo. Só a leitura do programa (`load_attr`) passa por aqui: as chamadas
/// internas do shim (`cls._parse`) e a construção da classe seguem pelo dicionário dela.
pub(crate) fn emulated_class_attr(cls: &Rc<crate::object::ClassObj>, name: &str) -> Option<Value> {
    let tname = emulated_name(cls)?;
    if name == "__text_signature__" {
        return Some(crate::typeattrs::type_text_signature(tname).map_or(Value::None, Value::str));
    }
    let key = own_type_keys(tname)?.into_iter().find(|k| *k == name)?;
    match type_var_kind(tname, key)? {
        "wrapper_descriptor" | "method_descriptor" | "member_descriptor" | "getset_descriptor" => {
            crate::typeattrs::descriptor_for_kind(tname, key, &Value::Class(cls.clone()))
        }
        _ => None,
    }
}

/// `dir(obj)`: nomes de atributo ordenados (instância, classe e módulo; o resto sai vazio).
fn b_dir(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("dir", &kw)?;
    // Objeto cuja classe define `__dir__` (o `Enum`, por exemplo): o resultado é o dele, ordenado.
    if let Some(Value::Instance(i)) = args.first() {
        if i.class().lookup("__dir__").is_some() {
            let f = vm.load_attr(&args[0], "__dir__")?;
            let listed = vm.call(&f, Vec::new(), Vec::new())?;
            return vm.call(&Value::Builtin("sorted"), vec![listed], Vec::new());
        }
    }
    // Módulo com `__dir__` no dict (PEP 562): o resultado é o dele, ordenado.
    if let Some(Value::Module(m)) = args.first() {
        let hook = vm.module_globals.borrow().get(m.name).and_then(|g| g.borrow().get("__dir__").cloned());
        if let Some(f) = hook {
            let listed = vm.call(&f, Vec::new(), Vec::new())?;
            return vm.call(&Value::Builtin("sorted"), vec![listed], Vec::new());
        }
    }
    Ok(plain_dir(vm, args.first()))
}

/// A lista de `dir()`: os nomes de `dir_names`, ordenados e sem repetição, sem consultar o `__dir__` da classe.
pub(crate) fn plain_dir(vm: &mut Vm, obj: Option<&Value>) -> Value {
    let mut names = dir_names(vm, obj);
    names.sort();
    names.dedup();
    Value::list(names.into_iter().map(Value::str).collect())
}

/// `object.__dir__(obj)` do CPython (`object___dir___impl` e `merge_class_dict`): as chaves de
/// `obj.__dict__`, depois as de `type(obj).__dict__` e, recursivamente, as de cada base em `__bases__`;
/// um nome fica na posição da primeira inserção. Sem ordenar. Valores fora desses casos (classes,
/// módulos, funções) seguem a lista ordenada.
pub(crate) fn merge_dir(vm: &mut Vm, obj: &Value) -> Value {
    let mut names: Vec<String> = Vec::new();
    match obj {
        Value::Instance(i) => {
            names.extend(instance_dict_keys(i));
            merge_class_dict(&i.class(), &mut names);
        }
        Value::Class(_) | Value::Module(_) | Value::Function(_) | Value::Builtin(_) | Value::NativeFn(_) => {
            return plain_dir(vm, Some(obj));
        }
        v => {
            let tname = match v {
                Value::Ext(e) => e.type_name(),
                v => v.type_name(),
            };
            merge_builtin_type(tname, &mut names);
        }
    }
    let mut seen = std::collections::HashSet::new();
    names.retain(|n| seen.insert(n.clone()));
    Value::list(names.into_iter().map(Value::str).collect())
}

/// As chaves do dicionário da instância (nenhuma quando a classe emula um tipo C ou tem `__slots__`
/// em toda a herança: o estado aparece pelos descritores da classe, nunca pelos nomes gravados).
fn instance_dict_keys(i: &Rc<crate::object::InstanceObj>) -> Vec<String> {
    let class = i.class();
    if class.emulates_c_type() || !class.slots_allow("__dict__") {
        return Vec::new();
    }
    let fields = class.slot_fields();
    i.dict.borrow().keys().filter(|k| !fields.contains(*k)).cloned().collect()
}

/// `merge_class_dict`: o `__dict__` de `c` e, em profundidade, o de cada base; a raiz sem base de
/// usuário desce para o tipo embutido de que herda (ou `object`).
fn merge_class_dict(c: &Rc<crate::object::ClassObj>, out: &mut Vec<String>) {
    match c.emulates_c_type().then(|| own_type_keys(c.name.as_str())).flatten() {
        Some(keys) => out.extend(keys.iter().map(|k| (*k).to_string())),
        None => {
            // A ordem em que o CPython monta o `__dict__` de uma classe: o corpo, depois os nomes que a
            // criação da classe acrescenta.
            out.extend(["__module__", "__firstlineno__"].map(String::from));
            let skip = ["__module__", "__firstlineno__", "__static_attributes__", "__dict__", "__weakref__", "__qualname__"];
            out.extend(c.dict.borrow().keys().filter(|k| !skip.contains(&k.as_str())).cloned());
            out.push("__static_attributes__".to_string());
            match c.declared_slots() {
                None if c.bases.is_empty() => out.extend(["__dict__", "__weakref__"].map(String::from)),
                None => {}
                Some(slots) => out.extend(slots),
            }
            out.push("__doc__".to_string());
        }
    }
    for b in &c.bases {
        merge_class_dict(b, out);
    }
    if c.bases.is_empty() {
        merge_builtin_type(c.data_base.or(c.builtin_base).unwrap_or("object"), out);
    }
}

/// O tipo pai de um tipo embutido: `int` para `bool`, o da tabela de exceções, `object` para os demais.
fn type_parent(name: &str) -> Option<&'static str> {
    match name {
        "object" => None,
        _ => Some(exception_or_bool_parent(name).unwrap_or("object")),
    }
}

/// O `__dict__` do tipo embutido `name` e o de cada pai até `object`, na ordem do CPython (a da
/// tabela do oráculo; sem linha nela, o `dir()` ordenado do tipo).
fn merge_builtin_type(name: &str, out: &mut Vec<String>) {
    let mut t = Some(name);
    while let Some(n) = t {
        if let Some(keys) = own_type_keys(n).or_else(|| type_dir(n)) {
            out.extend(keys.iter().map(|k| (*k).to_string()));
        }
        t = type_parent(n);
    }
}

/// `list(object.__dict__)` no CPython 3.13 do Debian.
const OBJECT_DICT_ORDER: &[&str] = &[
    "__new__", "__repr__", "__hash__", "__str__", "__getattribute__", "__setattr__", "__delattr__", "__lt__", "__le__",
    "__eq__", "__ne__", "__gt__", "__ge__", "__init__", "__reduce_ex__", "__reduce__", "__getstate__",
    "__subclasshook__", "__init_subclass__", "__format__", "__sizeof__", "__dir__", "__class__", "__doc__",
];

/// `int` para `bool`, o pai da tabela de exceções para uma exceção embutida; senão nada.
fn exception_or_bool_parent(name: &str) -> Option<&'static str> {
    if name == "bool" {
        return Some("int");
    }
    crate::object::EXC_CLASSES.iter().find(|(e, _)| *e == name).map(|(_, p)| *p).filter(|p| !p.is_empty())
}

/// `list(vars(T))` de `builtin-type-vars.tsv` (gerado no oráculo): `tipo<TAB>chave chave ...`.
fn type_vars(name: &str) -> Option<Vec<&'static str>> {
    TYPE_VARS_TABLE.lines().find_map(|line| {
        let (t, keys) = line.split_once('\t')?;
        (t == name).then(|| keys.split(' ').collect())
    })
}

const TYPE_VARS_TABLE: &str = include_str!("../data/cpython-docs/builtin-type-vars.tsv");

/// O tipo do descritor que `vars(T)[chave]` tem no oráculo (`wrapper_descriptor`, `member_descriptor`,
/// `classmethod_descriptor`...), de `builtin-type-var-kinds.tsv`: `tipo<TAB>chave=tipo chave=tipo ...`.
/// Sem linha para o tipo (tabela ainda não gerada), `None`.
pub(crate) fn type_var_kind(name: &str, key: &str) -> Option<&'static str> {
    type_var_kinds().get(name)?.get(key).copied()
}

/// A tabela de `type_var_kind` tem linha para o tipo `name`.
pub(crate) fn has_var_kinds(name: &str) -> bool {
    type_var_kinds().contains_key(name)
}

type VarKindIndex = std::collections::HashMap<&'static str, std::collections::HashMap<&'static str, &'static str>>;

/// A tabela de `type_var_kind` indexada uma vez: `type_name()` de um método ligado a consulta a cada chamada.
fn type_var_kinds() -> &'static VarKindIndex {
    static INDEX: std::sync::OnceLock<VarKindIndex> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| {
        TYPE_VAR_KINDS_TABLE
            .lines()
            .filter_map(|line| {
                let (t, entries) = line.split_once('\t')?;
                Some((t, entries.split(' ').filter_map(|e| e.split_once('=')).collect()))
            })
            .collect()
    })
}

const TYPE_VAR_KINDS_TABLE: &str = include_str!("../data/cpython-docs/builtin-type-var-kinds.tsv");

/// Os atributos do tipo `object` no CPython 3.13.
const OBJECT_ATTRS: &[&str] = &[
    "__class__", "__delattr__", "__dir__", "__doc__", "__eq__", "__format__", "__ge__", "__getattribute__", "__getstate__",
    "__gt__", "__hash__", "__init__", "__init_subclass__", "__le__", "__lt__", "__ne__", "__new__", "__reduce__",
    "__reduce_ex__", "__repr__", "__setattr__", "__sizeof__", "__str__", "__subclasshook__",
];

/// Nomes de atributos de `obj` (ou das globais, sem argumento), na ordem em que o `dir()` os junta.
pub(crate) fn dir_names(vm: &mut Vm, obj: Option<&Value>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    match obj {
        None => names.extend(vm.globals.borrow().keys().map(|k| k.to_string())),
        // `dir(object)` e `dir(object())`: os slots do tipo `object` do CPython.
        Some(Value::Builtin("object")) => names.extend(OBJECT_ATTRS.iter().map(|s| (*s).to_string())),
        Some(Value::Ext(e)) if e.type_name() == "object" => names.extend(OBJECT_ATTRS.iter().map(|s| (*s).to_string())),
        Some(t @ (Value::Builtin(_) | Value::NativeFn(_))) if crate::builtins::class_name(t).is_some() => {
            match crate::builtins::class_name(t).and_then(type_dir) {
                Some(listed) => names.extend(listed.iter().map(|s| (*s).to_string())),
                None => names.extend(probe_type_attrs(vm, t).into_iter().map(|(n, _)| n)),
            }
        }
        Some(Value::Instance(i)) => {
            // Classe que emula um tipo embutido (`memoryview`, `complex`): no CPython a instância
            // não tem dicionário, e o estado interno do shim não aparece.
            names.extend(instance_dict_keys(i));
            class_dir_names(vm, &i.class(), &mut names);
        }
        Some(Value::Class(c)) => class_dir_names(vm, c, &mut names),
        Some(Value::Module(m)) => {
            // `__spec__` e `__loader__` são criados sob demanda; o `dir()` os lista como no CPython.
            let module = Value::Module(m.clone());
            let _ = vm.load_attr(&module, "__spec__");
            names.extend(m.attrs.borrow().keys().cloned());
            if let Some(g) = vm.module_globals.borrow().get(m.name) {
                names.extend(g.borrow().keys().map(|k| k.to_string()));
            }
        }
        Some(Value::Ext(e)) => match type_dir(e.type_name()) {
            Some(listed) => names.extend(listed.iter().map(|s| (*s).to_string())),
            None => names.extend(e.methods().iter().map(|s| (*s).to_string())),
        },
        // `NotImplemented` e `...`: valores únicos dos seus tipos, não funções.
        Some(Value::Builtin(n @ ("NotImplemented" | "Ellipsis"))) => {
            let t = if *n == "Ellipsis" { "ellipsis" } else { "NotImplementedType" };
            names.extend(type_dir(t).unwrap_or_default().iter().map(|s| (*s).to_string()));
        }
        Some(Value::Function(f)) => {
            names.extend(type_dir("function").unwrap_or_default().iter().map(|s| (*s).to_string()));
            names.extend(f.attrs.borrow().keys().map(|k| k.to_string()));
        }
        Some(v) => match type_dir(v.type_name()) {
            Some(listed) => names.extend(listed.iter().map(|s| (*s).to_string())),
            None => names.extend(crate::suggest::builtin_methods(v.type_name()).iter().map(|s| (*s).to_string())),
        },
    }
    names
}

/// O `dir()` do tipo embutido `name` no CPython 3.13 do Debian (`builtin-type-dir.tsv`, gerado
/// no oráculo): `dict`, `list`, as exceções...
pub(crate) fn type_dir(name: &str) -> Option<Vec<&'static str>> {
    TYPE_DIR_TABLE.lines().find_map(|line| {
        let (t, names) = line.split_once('\t')?;
        (t == name).then(|| names.split(' ').collect())
    })
}

const TYPE_DIR_TABLE: &str = include_str!("../data/cpython-docs/builtin-type-dir.tsv");

/// O `dir()` do tipo embutido `tname` lista `name`? É o que decide se `(0).__index__` ou `[].__len__`
/// existem. Um tipo fora da tabela do oráculo (classe de usuário, extensão) não é filtrado.
pub(crate) fn type_has_name(tname: &str, name: &str) -> bool {
    type_dir_index().get(tname).is_none_or(|names| names.contains(name))
}

/// O `dir()` do tipo nativo `tname` lista `name`, e só quando a tabela do oráculo tem linha para o tipo:
/// sem linha, `false`. É o teste dos objetos de tipo nativo (iteradores, exceções, funções) que herdam
/// os mágicos de `object` sem os implementar.
pub(crate) fn type_listed(tname: &str, name: &str) -> bool {
    type_dir_index().get(tname).is_some_and(|names| names.contains(name))
}

type TypeDirIndex = std::collections::HashMap<&'static str, std::collections::HashSet<&'static str>>;

/// A tabela de `type_dir` indexada uma vez, para as buscas por nome.
fn type_dir_index() -> &'static TypeDirIndex {
    static INDEX: std::sync::OnceLock<TypeDirIndex> = std::sync::OnceLock::new();
    INDEX.get_or_init(|| {
        TYPE_DIR_TABLE
            .lines()
            .filter_map(|line| {
                let (t, names) = line.split_once('\t')?;
                Some((t, names.split(' ').collect()))
            })
            .collect()
    })
}

/// Os nomes que o `dir()` do CPython junta de uma classe: o dicionário de cada classe do MRO, o que
/// o `type` põe em toda classe (`__doc__`, `__module__` e, sem `__slots__`, `__dict__` e
/// `__weakref__`), os métodos do tipo embutido de base e os de `object`.
fn class_dir_names(vm: &mut Vm, cls: &Rc<crate::object::ClassObj>, names: &mut Vec<String>) {
    for c in cls.mro() {
        if c.emulates_c_type() {
            // O shim de um tipo embutido mostra a API do tipo real (tabela do oráculo), nunca os
            // auxiliares dele (`_check`) nem o que o `class` acrescenta.
            if let Some(listed) = type_dir(c.name.as_str()) {
                names.extend(listed.iter().map(|s| (*s).to_string()));
                continue;
            }
            let in_builtins = matches!(c.dict.borrow().get("__module__"), Some(Value::Str(m)) if m.as_str() == "builtins");
            names.extend(
                c.dict
                    .borrow()
                    .keys()
                    .filter(|k| !k.starts_with('_') || is_dunder(k))
                    .filter(|k| !matches!(k.as_str(), "__slots__" | "__firstlineno__" | "__static_attributes__"))
                    .filter(|k| !(in_builtins && k.as_str() == "__module__"))
                    .cloned(),
            );
            continue;
        }
        names.extend(c.dict.borrow().keys().cloned());
        names.extend(["__doc__", "__module__"].map(String::from));
        match c.declared_slots() {
            None => names.extend(["__dict__", "__weakref__"].map(String::from)),
            // Cada nome de `__slots__` vira um descritor na classe.
            Some(slots) => names.extend(slots),
        }
        if let Some(base) = c.data_base.or(c.builtin_base) {
            if let Some(listed) = type_dir(base) {
                names.extend(listed.iter().map(|s| (*s).to_string()));
            } else if let Some(t) = crate::builtins::get(base) {
                names.extend(probe_type_attrs(vm, &t).into_iter().map(|(n, _)| n));
            }
        }
    }
    names.extend(OBJECT_ATTRS.iter().map(|s| (*s).to_string()));
}

fn is_dunder(name: &str) -> bool {
    name.len() > 4 && name.starts_with("__") && name.ends_with("__")
}

fn b_format(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("format", args, kw, &["value", "format_spec"], 1)?;
    let value = a[0].clone().unwrap_or(Value::None);
    let spec = match &a[1] {
        Some(Value::Str(s)) => s.as_str().to_string(),
        Some(other) => {
            return Err(type_error(format!("format() argument 2 must be str, not {}", other.type_name())))
        }
        None => String::new(),
    };
    Ok(Value::str(vm.format_value(&value, &spec)?))
}

/// `input(prompt='')`: escreve o prompt no stdout e lê uma linha do stdin do pseudo-processo.
fn b_input(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("input", &kw)?;
    // Repetido depois de esperar o stdin (`SuspendRequest::Wait`): o prompt já saiu e o stdout já foi descarregado.
    if !crate::stdin::resumed() {
        if let Some(p) = args.first() {
            let text = crate::object::to_str(p);
            vm.write_stdout_text(&text)?;
        }
        // O CPython descarrega stdout (e stderr) em todo `input()`, com ou sem prompt.
        vm.flush_stdout()?;
    }
    let stdin = vm.std_files[0].clone();
    let Some(mut line) = crate::stdin::text_line(&stdin)? else {
        return Err(exc("EOFError", "EOF when reading a line"));
    };
    if line.ends_with('\n') {
        line.pop();
    }
    Ok(Value::str(line))
}

fn b_help(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    crate::native_util::no_kwargs("help", &kw)?;
    let m = crate::modules::import_checked(vm, "pydoc")?;
    let helper = vm.load_attr(&Value::Module(m), "help")?;
    vm.call(&helper, args, Vec::new())
}

fn b_exit(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Err(crate::native_util::system_exit(args))
}

fn b_import(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let entered = enter_import(vm, args, kw)?;
    vm.run_entered(entered)
}

/// `func` é a função embutida `name` desta tabela (`exec`, `eval`, `__import__`).
pub(crate) fn is_builtin(func: &Value, name: &str) -> bool {
    matches!(func, Value::NativeFn(f) if f.name == name && TABLE.iter().any(|(n, _)| *n == name))
}

/// `__import__`: o módulo pronto, ou o quadro do corpo que falta rodar (o laço de instruções o executa).
pub(crate) fn enter_import(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<crate::vm::Entered> {
    let a = bind("__import__", args, kw, &["name", "globals", "locals", "fromlist", "level"], 1)?;
    let name = want_str("__import__", a[0].as_ref().unwrap_or(&Value::None))?.to_string();
    let level = match &a[4] {
        Some(v) => crate::native_util::want_int(v)?.max(0) as usize,
        None => 0,
    };
    let wants_leaf = a[3].as_ref().is_some_and(Value::is_true);
    let full = if level > 0 { crate::modules::resolve_relative(vm, &name, level)? } else { name };
    // Só código embutido importa os módulos de apoio; o `importlib.import_module` repassa o nome
    // que o programa pediu, então não conta como embutido.
    let trusted = vm.frames.borrow().last().is_some_and(|(c, _, _)| c.internal && c.name != "import_module");
    if !trusted && crate::modules::INTERNAL.contains(&full.as_str()) {
        return Err(crate::vm::exc("ModuleNotFoundError", format!("No module named '{full}'")));
    }
    // Importa a cadeia inteira (`a.b.c` carrega `a`, `a.b`, `a.b.c`); sem `fromlist` devolve a raiz. O
    // caminho é o da instrução `import`, com os finders do programa em `sys.meta_path` (o
    // `_distutils_hack` do setuptools troca o `distutils` por um deles via `importlib.import_module`).
    if wants_leaf || level > 0 {
        return crate::modules::begin_import(vm, &full, &full);
    }
    let top = full.split('.').next().unwrap_or("").to_string();
    crate::modules::begin_import(vm, &full, &top)
}

thread_local! {
    /// `m.__dict__` de cada módulo: o mesmo objeto a cada leitura, para `exec(src, m.__dict__)` poder
    /// rodar nas globais vivas do módulo.
    static MODULE_DICTS: std::cell::RefCell<Vec<(&'static str, Value)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// O dict de `name.__dict__`, atualizado com o conteúdo `fresh` e sempre o mesmo objeto.
pub(crate) fn module_dict_value(name: &'static str, fresh: Dict) -> Value {
    MODULE_DICTS.with(|m| {
        let mut m = m.borrow_mut();
        if let Some((_, Value::Dict(d))) = m.iter().find(|(n, _)| *n == name) {
            *d.borrow_mut() = fresh;
            return Value::Dict(d.clone());
        }
        let v = Value::dict(fresh);
        m.push((name, v.clone()));
        v
    })
}

fn module_of_dict(d: &Value) -> Option<&'static str> {
    MODULE_DICTS.with(|m| m.borrow().iter().find(|(_, v)| crate::object::is(v, d)).map(|(n, _)| *n))
}

/// Registra `dict` (a visão viva das globais do módulo) como o `__dict__` de `name`: `exec(codigo, mod.__dict__)`
/// passa a rodar direto nas globais do módulo.
pub(crate) fn module_dict_register(name: &'static str, dict: &Value) {
    MODULE_DICTS.with(|m| {
        let mut m = m.borrow_mut();
        match m.iter_mut().find(|(n, _)| *n == name) {
            Some(entry) => entry.1 = dict.clone(),
            None => m.push((name, dict.clone())),
        }
    });
}

/// Substitui o conteúdo de `target` pelas globais `map`: nomes existentes na ordem de antes, os
/// novos na ordem de inserção do mapa de globais.
fn write_back(target: &Value, map: &crate::object::VarMap, was: &[String]) -> PyResult<()> {
    let Value::Dict(d) = target else { return Ok(()) };
    let mut fresh = Dict::default();
    let old: Vec<(Value, Value)> = d.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, v) in old {
        match &k {
            Value::Str(name) if was.iter().any(|w| w == name.as_str()) => {
                if let Some(nv) = map.get(name.as_str()) {
                    fresh.set(k.clone(), nv.clone())?;
                }
            }
            _ => fresh.set(k, v)?,
        }
    }
    let added: Vec<&Rc<str>> = map.keys().filter(|k| !k.starts_with("__builtins__") && !fresh.contains(&Value::str(&***k)).unwrap_or(false)).collect();
    for k in added {
        fresh.set(Value::str(&**k), map[k].clone())?;
    }
    *d.borrow_mut() = fresh;
    Ok(())
}

/// Os pares `(chave, valor)` de um mapeamento (o `f_locals` de um quadro de função, por exemplo).
fn mapping_items(vm: &mut Vm, mapping: &Value) -> PyResult<Vec<(Value, Value)>> {
    let items = vm.load_attr(mapping, "items").map_err(|_| type_error("locals must be a mapping"))?;
    let listed = vm.call(&items, Vec::new(), Vec::new())?;
    let mut out = Vec::new();
    for pair in crate::vm::iterate(&listed)? {
        let two = crate::vm::iterate(&pair)?;
        let [key, value] = <[Value; 2]>::try_from(two).map_err(|_| type_error("locals must be a mapping"))?;
        out.push((key, value));
    }
    Ok(out)
}

/// O que `exec`/`eval` guardam enquanto o quadro do código roda (`Dunder::Exec`): as globais em que ele roda, os
/// espaços de nomes dados (`None`: roda nas globais atuais) e o mapeamento de `locals` que não é `dict`.
pub(crate) struct ExecRun {
    pub(crate) eval: bool,
    /// As globais do quadro (o `f_globals` dele).
    pub(crate) globals: Rc<std::cell::RefCell<crate::object::VarMap>>,
    pub(crate) ns: Option<Namespaces>,
    pub(crate) back: Option<MappingBack>,
}

/// Os dicts de `globals` e `locals` do `exec`, e o que é preciso para devolver o resultado a eles.
pub(crate) struct Namespaces {
    pub(crate) gdict: Value,
    /// Os nomes que o dict de globais tinha antes, e os que o `locals` separado tinha.
    pub(crate) was: Vec<String>,
    pub(crate) lwas: Vec<String>,
    /// O `locals` quando é um dict diferente do `globals`.
    pub(crate) separate: Option<Value>,
    /// As globais vivas do módulo antes de o `locals` entrar nelas: voltam depois do código.
    pub(crate) backup: Option<Rc<std::cell::RefCell<crate::object::VarMap>>>,
}

/// `locals` que não é um `dict`: o código roda sobre um instantâneo do mapeamento, e o que ele criou ou mudou volta
/// por `__setitem__` (no `f_locals` de um quadro, isso grava na variável).
pub(crate) struct MappingBack {
    pub(crate) mapping: Value,
    pub(crate) snapshot: Value,
    pub(crate) before: Vec<(Value, Value)>,
}

/// `exec`/`eval`: os argumentos ligados e o quadro do código pronto para o laço de instruções (ou o valor, se o
/// código nem chegou a rodar).
pub(crate) fn enter_exec(vm: &mut Vm, eval: bool, args: Vec<Value>, kw: Kw) -> PyResult<crate::vm::Entered> {
    let who = if eval { "eval" } else { "exec" };
    let a = bind(who, args, kw, &["source", "globals", "locals"], 1)?;
    let (src, filename) = source_text(vm, who, a[0].as_ref().unwrap_or(&Value::None))?;
    begin_ns(vm, &src, filename.as_deref(), a[1].clone(), a[2].clone(), eval, who)
}

/// `exec`/`eval` com `locals` que não é um `dict`: o código roda sobre um instantâneo do mapeamento.
fn begin_ns(
    vm: &mut Vm,
    src: &str,
    filename: Option<&str>,
    globals: Option<Value>,
    locals: Option<Value>,
    eval: bool,
    who: &str,
) -> PyResult<crate::vm::Entered> {
    let Some(mapping) = locals.clone().filter(|l| !matches!(l, Value::None | Value::Dict(_))) else {
        return begin_dict(vm, src, filename, globals, locals, eval, who, &mut None);
    };
    let before = mapping_items(vm, &mapping)?;
    let mut copy = Dict::default();
    for (k, v) in &before {
        copy.set(k.clone(), v.clone())?;
    }
    let snapshot = Value::dict(copy);
    let mut back = Some(MappingBack { mapping, snapshot: snapshot.clone(), before });
    match begin_dict(vm, src, filename, globals, Some(snapshot), eval, who, &mut back) {
        Err(e) => match back {
            Some(b) => finish_mapping(vm, b, Err(e)).map(crate::vm::Entered::Done),
            None => Err(e),
        },
        entered => entered,
    }
}

/// O fim de um `exec`/`eval` com `locals` que não é `dict`: o que o código criou ou mudou volta por `__setitem__`.
fn finish_mapping(vm: &mut Vm, back: MappingBack, result: PyResult<Value>) -> PyResult<Value> {
    let MappingBack { mapping, snapshot, before } = back;
    let Value::Dict(after) = &snapshot else { return result };
    let changed: Vec<(Value, Value)> = after
        .borrow()
        .iter()
        .filter(|(k, v)| !before.iter().any(|(bk, bv)| crate::object::to_str(bk) == crate::object::to_str(k) && crate::object::is(bv, v)))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if !changed.is_empty() {
        let store = vm.load_attr(&mapping, "__setitem__")?;
        for (k, v) in changed {
            vm.call(&store, vec![k, v], Vec::new())?;
        }
    }
    result
}

/// Abre o quadro de `src` (`exec`) ou da expressão (`eval`) nos espaços de nomes dados; sem eles, nas
/// globais atuais. `globals`/`locals` são dicts: o conteúdo entra numa tabela de globais, o código
/// roda nela, e o resultado volta para o dict (um dict de módulo roda direto nas globais do módulo) quando o
/// quadro fecha (`finish_exec`). O mapeamento de `back` só é tomado quando o quadro abre.
#[allow(clippy::too_many_arguments)]
fn begin_dict(
    vm: &mut Vm,
    src: &str,
    filename: Option<&str>,
    globals: Option<Value>,
    locals: Option<Value>,
    eval: bool,
    who: &str,
    back: &mut Option<MappingBack>,
) -> PyResult<crate::vm::Entered> {
    let mut text = if eval { format!("__eval_value__ = ({})", src.trim()) } else { src.to_string() };
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let module = crate::parser::parse_module(&text).map_err(|e| {
        if eval {
            crate::vm::eval_syntax_exc(e, filename.unwrap_or("<string>"), src)
        } else {
            crate::vm::syntax_exc(e, filename.unwrap_or("<string>"), src)
        }
    })?;
    let mut code = crate::compile::compile_module(&module).map_err(|e| exc("SyntaxError", e.msg))?;
    if let Some(f) = filename {
        code.set_filename(f);
    }
    let code = Rc::new(code);
    let globals = globals.filter(|g| !matches!(g, Value::None));
    let locals = locals.filter(|l| !matches!(l, Value::None));
    let Some(gdict) = globals else {
        let run = ExecRun { eval, globals: vm.globals.clone(), ns: None, back: back.take() };
        return open_exec(vm, &code, None, run);
    };
    let Value::Dict(g) = &gdict else {
        return Err(type_error(format!("{who}() globals must be a dict, not {}", gdict.type_name())));
    };
    // Como o CPython, globais sem `__builtins__` ganham o dict do módulo `builtins`.
    if !g.borrow().contains(&Value::str("__builtins__")).unwrap_or(true) {
        if let Some(b) = crate::modules::builtins_dict(vm) {
            g.borrow_mut().set(Value::str("__builtins__"), b)?;
        }
    }
    if let Some(l) = &locals {
        if !matches!(l, Value::Dict(_)) {
            return Err(type_error("locals must be a mapping"));
        }
    }
    let live = crate::globalsview::map_of_dict(&gdict)
        .or_else(|| module_of_dict(&gdict).and_then(|n| vm.module_globals.borrow().get(n).cloned()));
    let map: Rc<std::cell::RefCell<crate::object::VarMap>> = match &live {
        Some(m) => m.clone(),
        None => Rc::new(std::cell::RefCell::new(Default::default())),
    };
    let mut was: Vec<String> = Vec::new();
    let gitems: Vec<(Value, Value)> = g.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (k, v) in gitems {
        if let Value::Str(name) = &k {
            was.push(name.as_str().to_string());
            if live.is_none() {
                map.borrow_mut().insert(name.as_str().into(), v);
            }
        }
    }
    let separate = locals.as_ref().filter(|l| !crate::object::is(l, &gdict));
    // Com `locals` separado, o que o código cria não pode vazar para as globais vivas do módulo.
    let backup = if live.is_some() && separate.is_some() {
        Some(Rc::new(std::cell::RefCell::new(map.borrow().clone())))
    } else {
        None
    };
    let mut lwas: Vec<String> = Vec::new();
    if let Some(Value::Dict(l)) = separate {
        let litems: Vec<(Value, Value)> = l.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (k, v) in litems {
            if let Value::Str(name) = &k {
                lwas.push(name.as_str().to_string());
                map.borrow_mut().insert(name.as_str().into(), v);
            }
        }
    }
    let ns = Namespaces { gdict: gdict.clone(), was, lwas, separate: separate.cloned(), backup };
    let run = ExecRun { eval, globals: map.clone(), ns: Some(ns), back: back.take() };
    open_exec(vm, &code, Some(map), run)
}

/// Abre o quadro do código de `exec`/`eval`. Se o rastreador recusar a entrada, o `exec` conclui como se o código
/// tivesse falhado (as globais e o `locals` recebem o que já havia).
fn open_exec(
    vm: &mut Vm,
    code: &Rc<crate::compile::Code>,
    globals: Option<Rc<std::cell::RefCell<crate::object::VarMap>>>,
    run: ExecRun,
) -> PyResult<crate::vm::Entered> {
    match vm.enter_nested(code, globals, crate::vm::Dunder::Exec(Box::new(run))) {
        Ok(callee) => Ok(crate::vm::Entered::Frame(callee)),
        Err((e, crate::vm::Dunder::Exec(run))) => finish_exec(vm, *run, Err(e)).map(crate::vm::Entered::Done),
        Err((e, _)) => Err(e),
    }
}

/// O quadro de `exec`/`eval` acabou com `result`: o valor do `eval`, e o que o código criou ou mudou volta ao dict de
/// globais (e ao de `locals`, se for outro).
pub(crate) fn finish_exec(vm: &mut Vm, run: ExecRun, result: PyResult<()>) -> PyResult<Value> {
    let ExecRun { eval, globals, ns, back } = run;
    let settled = match ns {
        None => result.map(|()| {
            if eval { globals.borrow_mut().shift_remove("__eval_value__").unwrap_or(Value::None) } else { Value::None }
        }),
        Some(ns) => settle_namespaces(eval, &globals, ns, result),
    };
    match back {
        Some(b) => finish_mapping(vm, b, settled),
        None => settled,
    }
}

fn settle_namespaces(
    eval: bool,
    map: &Rc<std::cell::RefCell<crate::object::VarMap>>,
    ns: Namespaces,
    result: PyResult<()>,
) -> PyResult<Value> {
    let Namespaces { gdict, was, lwas, separate, backup } = ns;
    let Value::Dict(g) = &gdict else { return Err(crate::vm::internal("exec globals are not a dict")) };
    let value = if eval { map.borrow_mut().shift_remove("__eval_value__").unwrap_or(Value::None) } else { Value::None };
    let snapshot = map.borrow().clone();
    if let Some(original) = backup {
        let saved = original.borrow().clone();
        *map.borrow_mut() = saved;
    }
    match &separate {
        Some(l) => {
            // Só o que o código criou ou mudou vai para o `locals`; o resto é das globais.
            let initial: Vec<(String, Value)> = was
                .iter()
                .filter_map(|n| g.borrow().get(&Value::str(n.clone())).ok().flatten().map(|v| (n.clone(), v)))
                .collect();
            let mut mine = snapshot.clone();
            mine.retain(|k, v| {
                lwas.iter().any(|w| **w == **k)
                    || !initial.iter().any(|(n, old)| **n == **k && crate::object::is(old, v)) && &**k != "__eval_value__"
            });
            write_back(l, &mine, &lwas)?
        }
        None => write_back(&gdict, &snapshot, &was)?,
    }
    result?;
    Ok(value)
}

fn b_eval(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let entered = enter_exec(vm, true, args, kw)?;
    vm.run_entered(entered)
}

fn b_exec(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let entered = enter_exec(vm, false, args, kw)?;
    vm.run_entered(entered)?;
    Ok(Value::None)
}

/// O texto de um argumento de `exec`/`eval`: uma string ou o resultado de `compile` (que traz
/// também o nome de arquivo dos quadros).
fn source_text(vm: &mut Vm, who: &str, v: &Value) -> PyResult<(String, Option<String>)> {
    if let Value::Ext(e) = v {
        if e.type_name() == "code" {
            if let Some(Ok(Value::Str(s))) = e.getattr(vm, "_source") {
                let filename = match e.getattr(vm, "co_filename") {
                    Some(Ok(Value::Str(f))) => Some(f.as_str().to_string()),
                    _ => None,
                };
                return Ok((s.as_str().to_string(), filename));
            }
        }
    }
    Ok((want_str(who, v)?.to_string(), None))
}

/// Resultado de `compile()`: o fonte já validado, que `exec`/`eval` executam depois.
struct CodeSource {
    src: String,
    filename: String,
    /// O módulo compilado, para os atributos `co_*` que olham dentro do código.
    code: Rc<crate::compile::Code>,
}

/// Refaz o resultado de `compile()` da imagem do heap (o inverso de `ExtObject::image`).
pub(crate) fn code_source_from_image(src: String, filename: String, code: Rc<crate::compile::Code>) -> Value {
    Value::Ext(Rc::new(CodeSource { src, filename, code }))
}

impl crate::object::ExtObject for CodeSource {
    fn type_name(&self) -> &'static str {
        "code"
    }
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
    /// `code_richcompare`: o arquivo não entra na comparação.
    fn eq_value(&self, other: &Value) -> Option<bool> {
        let Value::Ext(o) = other else { return None };
        let o = o.as_any()?.downcast_ref::<CodeSource>()?;
        Some(crate::tbobj::code_eq(&self.code, &o.code))
    }
    fn hash_value(&self) -> Option<i64> {
        Some(crate::tbobj::code_hash(&self.code))
    }
    fn image(&self) -> Option<crate::object::ExtImage> {
        Some(crate::object::ExtImage::CodeSource { src: self.src.clone(), filename: self.filename.clone(), code: self.code.clone() })
    }
    fn repr(&self) -> String {
        format!(
            "<code object <module> at {:#x}, file \"{}\", line 1>",
            crate::object::py_addr(self as *const Self as usize),
            self.filename
        )
    }
    fn getattr(&self, vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        Some(Ok(match name {
            "_source" => Value::str(self.src.clone()),
            "co_filename" => Value::str(self.filename.clone()),
            "co_name" | "co_qualname" => Value::str("<module>".to_string()),
            "co_firstlineno" => Value::Int(1),
            "co_consts" | "co_names" | "co_code" | "_co_code_adaptive" | "co_linetable" | "co_exceptiontable" | "co_flags"
            | "co_stacksize" | "co_varnames" | "co_nlocals" | "co_argcount" | "co_posonlyargcount"
            | "co_kwonlyargcount" | "co_cellvars" | "co_freevars" => {
                let inner = crate::tbobj::function_code(&self.code, &self.filename);
                let Value::Ext(e) = &inner else { return None };
                let v = e.getattr(vm, name)?;
                // O `eval` compila `__eval_value__ = (expr)`: o nome auxiliar não é do código do usuário.
                return Some(v.map(|v| match (&v, name) {
                    (Value::Tuple(t), "co_names") => Value::tuple(
                        t.iter().filter(|n| !matches!(n, Value::Str(s) if s.as_str() == "__eval_value__")).cloned().collect(),
                    ),
                    _ => v,
                }));
            }
            _ => return None,
        }))
    }
    fn methods(&self) -> &'static [&'static str] {
        &["co_lines", "co_positions", "_varname_from_oparg"]
    }
    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        match name {
            "co_lines" | "co_positions" | "_varname_from_oparg" => {
                let inner = crate::tbobj::function_code(&self.code, &self.filename);
                let Value::Ext(e) = &inner else { return Err(type_error("'code' object is not available")) };
                e.call_method(vm, name, args, kw)
            }
            _ => Err(type_error(format!("'code' object has no method '{name}'"))),
        }
    }
}

fn b_compile(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("compile", args, kw, &["source", "filename", "mode", "flags", "dont_inherit", "optimize"], 3)?;
    let mut source = a[0].clone().unwrap_or(Value::None);
    let flags = match a[3].as_ref() {
        Some(Value::Int(n)) => *n,
        _ => 0,
    };
    // Uma árvore (`ast.AST`) vira texto com `ast.unparse` e segue o caminho de sempre.
    if let Value::Bytes(b) = &source {
        source = Value::str(String::from_utf8_lossy(b).into_owned());
    }
    if let Value::Instance(_) = &source {
        let ast = crate::modules::import_checked(vm, "ast")?;
        let unparse = vm.load_attr(&Value::Module(ast), "unparse")?;
        source = vm.call(&unparse, vec![source], Vec::new())?;
        if flags & 1024 != 0 {
            return Ok(a[0].clone().unwrap_or(Value::None));
        }
    }
    // `PyCF_ONLY_AST`: devolve a árvore em vez do código.
    if flags & 1024 != 0 {
        let m = crate::modules::import_checked(vm, "_ast")?;
        let parse = vm.load_attr(&Value::Module(m), "_parse")?;
        let rest = vec![source, a[1].clone().unwrap_or(Value::None), a[2].clone().unwrap_or(Value::None)];
        return vm.call(&parse, rest, Vec::new());
    }
    let src = want_str("compile", &source)?.to_string();
    let filename = a[1].as_ref().map(|f| crate::object::to_str(f)).unwrap_or_default();
    let mode = a[2].as_ref().map(|m| crate::object::to_str(m)).unwrap_or_default();
    if !matches!(mode.as_str(), "exec" | "eval" | "single") {
        return Err(exc("ValueError", "compile() mode must be 'exec', 'eval' or 'single'"));
    }
    let mut text = if mode == "eval" { format!("__eval_value__ = ({})", src.trim()) } else { src.clone() };
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let mut module = crate::parser::parse_module(&text).map_err(|e| {
        if mode == "eval" {
            crate::vm::eval_syntax_exc(e, &filename, &src)
        } else {
            crate::vm::syntax_exc(e, &filename, &src)
        }
    })?;
    // Só quando a expressão não começa com espaço: as posições do `eval` são as dela, não as do embrulho.
    let mut exact = false;
    if mode == "eval" && src.trim_start().len() == src.len() {
        if let Ok(expr) = crate::parser::parse_expression(src.trim()) {
            module = crate::compile::eval_module(expr);
            exact = true;
        }
    }
    let mut code = crate::compile::compile_module(&module).map_err(|e| exc("SyntaxError", e.msg))?;
    if mode == "eval" {
        // O módulo compilado é `__eval_value__ = expr`: o bytecode do `eval` sai da expressão.
        let crate::ast::Mod::Module { body, .. } = &module else { unreachable!("parse_module devolve um módulo") };
        let expr = match body.first() {
            Some(crate::ast::Stmt { kind: crate::ast::StmtKind::Assign { value, .. }, .. }) => Some(&**value),
            _ => None,
        };
        code.cpy = expr.filter(|_| exact).and_then(|e| crate::cpybc::expression(&code, e)).map(Rc::new);
    }
    if mode == "single" {
        crate::compile::reemit_interactive(&mut code, &module);
    }
    // Os quadros do código compilado levam o nome de arquivo dado (o `setup.py` do setuptools).
    code.set_filename(&filename);
    let code = Rc::new(code);
    // Modo `single`: uma expressão solta passa pelo `sys.displayhook` (é o que o doctest espera).
    let src = if mode == "single" && crate::parser::parse_module(&format!("__eval_value__ = ({})\n", src.trim())).is_ok() {
        format!("import sys as __single_sys__\n__single_sys__.displayhook({})\n", src.trim())
    } else {
        src
    };
    Ok(Value::Ext(Rc::new(CodeSource { src, filename, code })))
}
