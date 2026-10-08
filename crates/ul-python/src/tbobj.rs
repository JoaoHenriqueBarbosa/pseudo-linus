//! Objetos de traceback, quadro e código que `e.__traceback__` e o módulo `traceback` enxergam.
//! São só leitura: cada entrada guarda a linha e o nome do código registrados quando a exceção
//! atravessou o quadro, com o nome do arquivo de quem a capturou.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::object::{ExtImage, ExtObject, Value};
use crate::vm::{PyException, PyResult, Vm};

/// Uma entrada: linha e nome do código, do quadro mais externo para o mais interno.
pub type Entries = Rc<Vec<crate::vm::TbEntry>>;

pub struct TracebackObj {
    entries: Entries,
    idx: usize,
    filename: Rc<str>,
    /// `tb.tb_next = ...` (o `unittest` corta tracebacks): vale no lugar do próximo quadro.
    next: std::cell::RefCell<Option<Value>>,
    /// O `tb_frame`, criado na primeira leitura: o mesmo objeto a cada acesso (o `pdb` usa o quadro como chave).
    frame: std::cell::RefCell<Option<Value>>,
}

impl TracebackObj {
    /// O traceback que começa no quadro mais externo de `entries`; `None` se estiver vazio.
    pub fn make(entries: Vec<crate::vm::TbEntry>, filename: &str) -> Value {
        if entries.is_empty() {
            return Value::None;
        }
        Value::Ext(Rc::new(TracebackObj {
            entries: Rc::new(entries),
            idx: 0,
            filename: Rc::from(filename),
            next: Default::default(),
            frame: Default::default(),
        }))
    }
}

impl TracebackObj {
    /// Os quadros que este traceback cobre (do mais externo para o mais interno) e o arquivo padrão.
    pub fn frames(&self) -> (Vec<crate::vm::TbEntry>, Rc<str>) {
        let mut out = vec![self.entries[self.idx].clone()];
        match &*self.next.borrow() {
            // Cortado ou religado por `tb_next = ...`: segue o que foi atribuído.
            Some(Value::None) => {}
            Some(Value::Ext(n)) => {
                if let Some(t) = n.as_any().and_then(|a| a.downcast_ref::<TracebackObj>()) {
                    out.extend(t.frames().0);
                }
            }
            _ => out.extend(self.entries[self.idx + 1..].iter().cloned()),
        }
        (out, self.filename.clone())
    }
}

impl ExtObject for TracebackObj {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn type_name(&self) -> &'static str {
        "traceback"
    }

    fn repr(&self) -> String {
        crate::lazy::object_repr("traceback", self)
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        let (line, code, own, span, held) = &self.entries[self.idx];
        let file = if own.is_empty() { self.filename.clone() } else { own.clone() };
        Some(Ok(match name {
            "tb_lineno" => Value::Int(*line as i64),
            "tb_lasti" => Value::Int(held.as_ref().map_or(0, |(_, c)| synthetic_lasti(c, *line)) as i64),
            "_position" if span.lineno > 0 => Value::tuple(vec![
                Value::Int(span.lineno as i64),
                Value::Int(span.end_lineno as i64),
                Value::Int(span.col as i64),
                Value::Int(span.end_col as i64),
            ]),
            "_position" => Value::None,
            "tb_frame" => {
                if let Some(frame) = self.frame.borrow().clone() {
                    return Some(Ok(frame));
                }
                let frame = crate::frameobj::detached_frame(&crate::frameobj::FrameLink {
                    line: *line,
                    name: code.clone(),
                    file,
                    code: held.as_ref().map(|(_, c)| c.clone()),
                    env: held.as_ref().map(|(e, _)| e.clone()),
                    caller_line: 0,
                });
                *self.frame.borrow_mut() = Some(frame.clone());
                frame
            }
            "tb_next" if self.next.borrow().is_some() => self.next.borrow().clone().unwrap_or(Value::None),
            "tb_next" => {
                // O nó seguinte é criado uma vez e guardado: `tb.tb_next.tb_next = None` precisa valer depois.
                let next = if self.idx + 1 < self.entries.len() {
                    Value::Ext(Rc::new(TracebackObj {
                        entries: self.entries.clone(),
                        idx: self.idx + 1,
                        filename: self.filename.clone(),
                        next: Default::default(),
                        frame: Default::default(),
                    }))
                } else {
                    Value::None
                };
                *self.next.borrow_mut() = Some(next.clone());
                next
            }
            _ => return None,
        }))
    }

    fn setattr(&self, name: &str, value: Value) -> Option<Result<(), PyException>> {
        if name == "tb_next" {
            *self.next.borrow_mut() = Some(value);
            return Some(Ok(()));
        }
        None
    }
}

/// Um objeto `code` solto (`função.__code__`).
pub fn code_object(name: &str, filename: &str) -> Value {
    Value::Ext(Rc::new(CodeObject { name: name.to_string(), filename: Rc::from(filename), code: None }))
}

/// `função.__code__` com os parâmetros e as marcas que `inspect` lê.
///
/// O CPython tem um objeto `code` por função compilada: `f.__code__ is f.__code__` e `frame.f_code is
/// f.__code__`. O mesmo `Code` devolve sempre o mesmo objeto.
pub fn function_code(code: &Rc<crate::compile::Code>, filename: &str) -> Value {
    thread_local! {
        static OBJECTS: RefCell<HashMap<usize, Value>> = RefCell::new(HashMap::new());
    }
    let key = Rc::as_ptr(code) as usize;
    OBJECTS.with(|objects| {
        objects
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                Value::Ext(Rc::new(CodeObject { name: code.name.clone(), filename: Rc::from(filename), code: Some(code.clone()) }))
            })
            .clone()
    })
}

/// Refaz um objeto `code` da imagem do heap (o inverso de `ExtObject::image`).
pub fn code_object_from_image(name: String, filename: Rc<str>, code: Option<Rc<crate::compile::Code>>) -> Value {
    Value::Ext(Rc::new(CodeObject { name, filename, code }))
}

/// `code.co_lnotab`: obsoleto desde o 3.12, avisa com `DeprecationWarning` (atribuído ao chamador) e devolve a
/// tabela de linhas no formato antigo.
fn code_lnotab(vm: &mut Vm, c: &crate::compile::Code) -> PyResult<Value> {
    let warnings = crate::modules::import_value(vm, "warnings")?;
    let warn = vm.load_attr(&warnings, "warn")?;
    let message = Value::str("co_lnotab is deprecated, use co_lines instead.");
    vm.call(&warn, vec![message, Value::Builtin("DeprecationWarning")], Vec::new())?;
    Ok(Value::bytes(crate::cpybc::of(c).lnotab()))
}

/// `co_flags` do CPython 3.13: OPTIMIZED e NEWLOCALS nas funções, VARARGS, VARKEYWORDS, NESTED (definido
/// dentro de uma função), GENERATOR, COROUTINE e ASYNC_GENERATOR. Módulo e corpo de classe não têm
/// OPTIMIZED nem NEWLOCALS, e o 3.13 não tem mais o NOFREE.
fn co_flags(c: &crate::compile::Code) -> i64 {
    const OPTIMIZED: i64 = 0x1;
    const NEWLOCALS: i64 = 0x2;
    const VARARGS: i64 = 0x4;
    const VARKEYWORDS: i64 = 0x8;
    const NESTED: i64 = 0x10;
    const GENERATOR: i64 = 0x20;
    const COROUTINE: i64 = 0x80;
    const ASYNC_GENERATOR: i64 = 0x200;
    let mut flags = c.future_flags;
    if c.is_function && !c.is_class {
        flags |= OPTIMIZED | NEWLOCALS;
    }
    if c.vararg.is_some() {
        flags |= VARARGS;
    }
    if c.kwarg.is_some() {
        flags |= VARKEYWORDS;
    }
    // `compute_code_flags`: `CO_NESTED` só nos blocos de função, nunca no corpo de classe.
    if !c.is_class && (c.qualname.contains("<locals>") || c.type_params_role & crate::pep695::NESTED != 0) {
        flags |= NESTED;
    }
    flags | match (c.is_async, c.is_generator) {
        (true, true) => ASYNC_GENERATOR,
        (true, false) => COROUTINE,
        (false, true) => GENERATOR,
        (false, false) => 0,
    }
}

struct CodeObject {
    name: String,
    filename: Rc<str>,
    /// O código compilado, quando o objeto vem de uma função (`f.__code__`).
    code: Option<Rc<crate::compile::Code>>,
}

/// `code_richcompare`: nome, argumentos, marcas, primeira linha, bytecode, constantes, nomes, variáveis e as duas
/// tabelas. O `co_filename` não entra.
pub fn code_eq(a: &crate::compile::Code, b: &crate::compile::Code) -> bool {
    let (ea, eb) = (crate::cpybc::of(a), crate::cpybc::of(b));
    let (la, lb) = (crate::cpybc::layout(a), crate::cpybc::layout(b));
    a.name == b.name
        && a.qual() == b.qual()
        && (a.params.len(), a.posonly, a.kwonly.len(), co_flags(a), a.first_line.max(1))
            == (b.params.len(), b.posonly, b.kwonly.len(), co_flags(b), b.first_line.max(1))
        && ea.code == eb.code
        && ea.linetable == eb.linetable
        && ea.exceptiontable == eb.exceptiontable
        && ea.names == eb.names
        && ea.consts.len() == eb.consts.len()
        && ea.consts.iter().zip(&eb.consts).all(|(x, y)| crate::object::py_eq(x, y))
        && (la.varnames, la.cellvars, la.freevars) == (lb.varnames, lb.cellvars, lb.freevars)
        && a.functions.len() == b.functions.len()
        && a.functions.iter().zip(&b.functions).all(|(x, y)| code_eq(x, y))
}

/// `hash(code)`: o mesmo para os códigos que [`code_eq`] diz iguais.
pub fn code_hash(c: &crate::compile::Code) -> i64 {
    let e = crate::cpybc::of(c);
    crate::object::bytes_hash(&e.code) ^ crate::object::bytes_hash(c.name.as_bytes()).rotate_left(7) ^ c.first_line.max(1) as i64
}

impl CodeObject {
    /// `code.replace(**campos)`: um objeto novo com os campos trocados. Valem os que o interpretador guarda por conta
    /// própria (nome, nome qualificado, arquivo e primeira linha, que desloca as linhas da tabela); os outros campos
    /// que o CPython aceita (`co_code`, `co_consts`...) ficam como estão.
    fn replace(&self, c: &Rc<crate::compile::Code>, kw: crate::object::Kw) -> PyResult<Value> {
        const KEPT: &[&str] = &[
            "co_argcount", "co_posonlyargcount", "co_kwonlyargcount", "co_nlocals", "co_stacksize", "co_flags", "co_code",
            "co_consts", "co_names", "co_varnames", "co_freevars", "co_cellvars", "co_linetable", "co_exceptiontable",
        ];
        let mut code = (**c).clone();
        if code.qualname.is_empty() {
            code.qualname = code.name.clone();
        }
        let mut name = self.name.clone();
        let mut filename = self.filename.clone();
        for (key, value) in kw {
            match key.as_str() {
                "co_name" => name = crate::object::to_str(&value),
                "co_qualname" => code.qualname = crate::object::to_str(&value),
                "co_filename" => filename = Rc::from(crate::object::to_str(&value).as_str()),
                "co_firstlineno" => {
                    let line = crate::native_util::want_int(&value)?;
                    code.first_line = usize::try_from(line).unwrap_or(0);
                    if let Some(e) = &code.cpy {
                        code.cpy = Some(Rc::new(crate::cpybc::Emitted { first_line: line as i32, ..(**e).clone() }));
                    }
                }
                k if KEPT.contains(&k) => {}
                k => return Err(crate::vm::exc("TypeError", format!("code.replace() got an unexpected keyword argument '{k}'"))),
            }
        }
        code.name = name.clone();
        Ok(Value::Ext(Rc::new(CodeObject { name, filename, code: Some(Rc::new(code)) })))
    }
}

/// O deslocamento de instrução (`f_lasti`, `tb_lasti`) da primeira instrução da linha: no bytecode emitido de
/// verdade é o do `co_code`; no esqueleto (`Emitted::synthetic`) é o `RESUME` em 0 e dois bytes por
/// instrução interna depois dele.
pub fn synthetic_lasti(c: &crate::compile::Code, line: usize) -> usize {
    match &c.cpy {
        Some(e) => e.first_offset_of_line(line).unwrap_or(0),
        None => c.lines.iter().position(|l| *l == line).map_or(0, |i| 2 + 2 * i),
    }
}

/// Uma linha de `co_lines()`/`co_positions()` por item de `rows`.
fn code_rows<T>(rows: impl IntoIterator<Item = T>, row: impl FnMut(T) -> Value) -> Vec<Value> {
    rows.into_iter().map(row).collect()
}

/// `code.co_lines()`: tuplas `(início, fim, linha ou None)` sobre os deslocamentos de `co_code`.
pub fn co_lines(c: &crate::compile::Code) -> Vec<Value> {
    code_rows(crate::cpybc::of(c).lines(), |(start, end, line)| {
        Value::tuple(vec![Value::Int(start as i64), Value::Int(end as i64), line.map_or(Value::None, |l| Value::Int(i64::from(l)))])
    })
}

/// `code.co_positions()`: `(linha, linha_fim, coluna, coluna_fim)` de cada unidade de código, com `None` onde o
/// compilador não tem a informação.
pub fn co_positions(c: &crate::compile::Code) -> Vec<Value> {
    let opt = |n: i32| if n < 0 { Value::None } else { Value::Int(i64::from(n)) };
    code_rows(crate::cpybc::of(c).positions(), |p| Value::tuple(vec![opt(p.line), opt(p.end_line), opt(p.col), opt(p.end_col)]))
}

/// Um iterador sobre `items`, como o que `co_lines()` e `co_positions()` devolvem.
pub fn iter_of(vm: &mut Vm, items: Vec<Value>) -> PyResult<Value> {
    let (_, f) = crate::builtins::TABLE.iter().find(|(n, _)| *n == "iter").expect("função embutida iter");
    f(vm, vec![Value::list(items)], Vec::new())
}

impl ExtObject for CodeObject {
    fn type_name(&self) -> &'static str {
        "code"
    }

    fn image(&self) -> Option<ExtImage> {
        Some(ExtImage::CodeObject { name: self.name.clone(), filename: self.filename.clone(), code: self.code.clone() })
    }

    fn methods(&self) -> &'static [&'static str] {
        &["co_lines", "co_positions", "_varname_from_oparg", "replace", "__replace__"]
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    /// `code_richcompare`: o `co_filename` não entra na comparação.
    fn eq_value(&self, other: &Value) -> Option<bool> {
        let Value::Ext(o) = other else { return None };
        let o = o.as_any()?.downcast_ref::<CodeObject>()?;
        match (&self.code, &o.code) {
            (Some(a), Some(b)) => Some(self.name == o.name && code_eq(a, b)),
            _ => None,
        }
    }

    fn hash_value(&self) -> Option<i64> {
        self.code.as_ref().map(|c| code_hash(c))
    }

    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, kw: crate::object::Kw) -> PyResult<Value> {
        match (name, &self.code) {
            ("replace" | "__replace__", Some(c)) => self.replace(c, kw),
            ("co_lines", Some(c)) => iter_of(vm, co_lines(c)),
            ("co_positions", Some(c)) => iter_of(vm, co_positions(c)),
            // O código do script principal não é guardado: sem tabela de linhas.
            ("co_lines" | "co_positions", None) => iter_of(vm, Vec::new()),
            ("_varname_from_oparg", Some(c)) => {
                let i = crate::native_util::want_int(args.first().unwrap_or(&Value::None))?;
                let names = crate::cpybc::localsplus(c);
                match usize::try_from(i).ok().and_then(|i| names.get(i)) {
                    Some(n) => Ok(Value::str(&**n)),
                    None => Err(crate::vm::exc("IndexError", "tuple index out of range")),
                }
            }
            _ => Err(crate::object::no_attribute("code", name)),
        }
    }

    fn repr(&self) -> String {
        // O endereço sai do código (não do objeto), para ser o mesmo em todo acesso a `co_consts`.
        let (addr, line) = match &self.code {
            Some(c) => (Rc::as_ptr(c) as usize, format!(", line {}", c.first_line.max(1))),
            None => (self as *const Self as usize, String::new()),
        };
        format!(
            "<code object {} at {:#x}, file \"{}\"{}>",
            self.name,
            crate::object::py_addr(addr),
            self.filename,
            line
        )
    }

    fn getattr(&self, vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        if let ("co_lnotab", Some(c)) = (name, self.code.as_ref()) {
            return Some(code_lnotab(vm, c));
        }
        Some(Ok(match name {
            "co_name" => Value::str(self.name.clone()),
            "co_qualname" => Value::str(self.code.as_ref().map_or(self.name.as_str(), |c| c.qual())),
            "co_filename" => Value::str(self.filename.to_string()),
            "co_firstlineno" => Value::Int(self.code.as_ref().map_or(1, |c| c.first_line.max(1)) as i64),
            "co_flags" => Value::Int(self.code.as_ref().map_or(0, |c| co_flags(c))),
            _ => {
                let c = self.code.as_ref()?;
                let strs = |v: Vec<Rc<str>>| Value::tuple(v.into_iter().map(|s| Value::str(&*s)).collect());
                match name {
                    "co_argcount" => Value::Int(c.params.len() as i64),
                    "co_posonlyargcount" => Value::Int(c.posonly as i64),
                    "co_kwonlyargcount" => Value::Int(c.kwonly.len() as i64),
                    "co_varnames" => strs(crate::cpybc::layout(c).varnames),
                    "co_nlocals" => Value::Int(crate::cpybc::layout(c).varnames.len() as i64),
                    "co_cellvars" => strs(crate::cpybc::layout(c).cellvars),
                    "co_freevars" => strs(crate::cpybc::layout(c).freevars),
                    "co_flags_varargs" => Value::Bool(c.vararg.is_some()),
                    // Globais e atributos usados pelo código, sem os locais (que estão em `co_varnames`).
                    "co_names" if c.cpy.is_some() => {
                        strs(c.cpy.as_ref().map(|e| e.names.clone()).unwrap_or_default())
                    }
                    "co_names" => {
                        let locals: Vec<&Rc<str>> = c.varnames.iter().chain(&c.cellvars).chain(&c.freevars).collect();
                        let mut v: Vec<Rc<str>> = Vec::new();
                        for n in &c.names {
                            if !locals.contains(&n) && !v.contains(n) {
                                v.push(n.clone());
                            }
                        }
                        strs(v)
                    }
                    "co_flags_varkw" => Value::Bool(c.kwarg.is_some()),
                    // As constantes do código, com os corpos de funções e lambdas aninhados como `code`.
                    // As constantes emitidas; no lugar de cada marca de função aninhada entra o objeto `code` dela.
                    "co_consts" if c.cpy.is_some() => {
                        let mut functions = c.functions.iter();
                        let emitted = c.cpy.as_ref().map(|e| e.consts.clone()).unwrap_or_default();
                        Value::tuple(
                            emitted
                                .into_iter()
                                .map(|v| {
                                    if matches!(&v, Value::Builtin(crate::cpybc::CODE_CONST)) {
                                        if let Some(f) = functions.next() {
                                            return function_code(f, &self.filename);
                                        }
                                    }
                                    v
                                })
                                .collect(),
                        )
                    }
                    "co_code" | "_co_code_adaptive" => Value::bytes(crate::cpybc::of(c).code.clone()),
                    "co_linetable" => Value::bytes(crate::cpybc::of(c).linetable.clone()),
                    "co_exceptiontable" => Value::bytes(crate::cpybc::of(c).exceptiontable.clone()),
                    "co_stacksize" => Value::Int(crate::cpybc::of(c).stacksize as i64),
                    "co_consts" => {
                        let mut v: Vec<Value> = c.consts.clone();
                        for f in &c.functions {
                            v.push(Value::Ext(Rc::new(CodeObject {
                                name: f.name.clone(),
                                filename: self.filename.clone(),
                                code: Some(f.clone()),
                            })));
                        }
                        // Todo código do CPython guarda `None` (o `RETURN_CONST` implícito ou o espaço da docstring).
                        if !v.iter().any(|x| matches!(x, Value::None)) {
                            v.insert(0, Value::None);
                        }
                        Value::tuple(v)
                    }
                    "co_kinds" => {
                        let mut k = Vec::new();
                        match (c.is_async, c.is_generator) {
                            (true, true) => k.push(Rc::from("asyncgen")),
                            (true, false) => k.push(Rc::from("coroutine")),
                            (false, true) => k.push(Rc::from("generator")),
                            _ => {}
                        }
                        strs(k)
                    }
                    _ => return None,
                }
            }
        }))
    }
}
