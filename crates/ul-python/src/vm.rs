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

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{CmpOp, Operator, UnaryOp};
use crate::compile::{Code, Op};
use crate::modules::{csv, json};
use crate::object::{
    exc_is_subclass, exc_str, int_add, int_mul, int_neg, int_sub, is, py_eq, repr, to_str, BoundMethod, Dict, Env,
    ExcObj, FileKind, FuncObj, Native, ObjError, PyFile, PyStr, Range, Set, Value, EXC_CLASSES,
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
    /// O terceiro item é o arquivo do código (vazio: o script principal).
    pub tb: Vec<TbEntry>,
}

/// Uma entrada de traceback: linha, nome do código, arquivo (vazio: o script principal) e o intervalo de fonte
/// da instrução que falhou.
pub type TbEntry = (usize, String, Rc<str>, crate::compile::Span);

/// Desde o 3.12 (PEP 709) as compreensões de lista, conjunto e dicionário não têm quadro próprio:
/// o traceback mostra a linha de dentro com o nome da função que as contém. (`<genexpr>` mantém o seu.)
fn is_inlined_comp(name: &str) -> bool {
    matches!(name, "<listcomp>" | "<setcomp>" | "<dictcomp>")
}

/// `(__cause__, __context__, __suppress_context__)` de uma exceção.
pub(crate) fn exc_chain(v: &Value) -> (Option<Value>, Option<Value>, bool) {
    match v {
        Value::Exception(e) => {
            let c = e.chain.borrow();
            (c.cause.clone(), c.context.clone(), c.suppress)
        }
        Value::Instance(i) => {
            let d = i.dict.borrow();
            let get = |k: &str| d.get(k).filter(|x| !matches!(x, Value::None)).cloned();
            (get("__cause__"), get("__context__"), matches!(d.get("__suppress_context__"), Some(Value::Bool(true))))
        }
        _ => (None, None, false),
    }
}

/// `raise X from cause`: grava `__cause__` (`None` para `from None`) e suprime o contexto.
pub(crate) fn exc_set_cause(v: &Value, cause: Value) {
    match v {
        Value::Exception(e) => {
            let mut c = e.chain.borrow_mut();
            c.cause = if matches!(cause, Value::None) { None } else { Some(cause) };
            c.suppress = true;
        }
        Value::Instance(i) => {
            let mut d = i.dict.borrow_mut();
            d.insert("__cause__".to_string(), cause);
            d.insert("__suppress_context__".to_string(), Value::Bool(true));
        }
        _ => {}
    }
}

/// Contexto implícito: a exceção em tratamento quando outra é levantada (sem sobrescrever e sem ciclo).
pub(crate) fn exc_set_context(v: &Value, ctx: &Value) {
    if crate::object::is(v, ctx) {
        return;
    }
    match v {
        Value::Exception(e) => {
            let mut c = e.chain.borrow_mut();
            if c.context.is_none() {
                c.context = Some(ctx.clone());
            }
        }
        Value::Instance(i) => {
            let mut d = i.dict.borrow_mut();
            if !matches!(d.get("__context__"), Some(x) if !matches!(x, Value::None)) {
                d.insert("__context__".to_string(), ctx.clone());
            }
        }
        _ => {}
    }
}

impl PyException {
    /// A instância que `except ... as e` enxerga.
    pub(crate) fn to_value(&self) -> Value {
        if let Some(v) = &self.value {
            return v.clone();
        }
        if let Some(args) = os_error_args(self.kind, &self.msg) {
            return Value::Exception(Rc::new(ExcObj::new(self.kind, args)));
        }
        let args = if self.msg.is_empty() { Vec::new() } else { vec![Value::str(self.msg.clone())] };
        Value::Exception(Rc::new(ExcObj::new(self.kind, args)))
    }

    pub(crate) fn from_value(v: &Value) -> PyException {
        match v {
            Value::Exception(e) => {
                PyException { kind: e.kind, msg: exc_str(e), value: Some(v.clone()), tb: Vec::new() }
            }
            // Instância de exceção de usuário: o traceback mostra `__main__.Nome`.
            Value::Instance(i) if i.class.builtin_base.is_some() => PyException {
                kind: crate::object::intern(&format!("__main__.{}", i.class.name)),
                msg: instance_text(v, true).unwrap_or_default(),
                value: Some(v.clone()),
                tb: Vec::new(),
            },
            _ => type_error("exceptions must derive from BaseException"),
        }
    }

    /// A exceção de `v` voltando a subir (`raise` sem argumento, fim de `finally`/`with`): leva o traceback
    /// que já tinha, e o quadro onde o re-raise acontece não ganha uma entrada nova (como no CPython).
    pub(crate) fn reraised(v: &Value) -> PyException {
        let mut e = PyException::from_value(v);
        if e.seed_traceback() {
            e.tb.push(RERAISE_MARK());
        }
        e
    }

    /// Copia para `tb` os quadros do `__traceback__` do valor. `false` se ele ainda não tinha traceback.
    pub(crate) fn seed_traceback(&mut self) -> bool {
        let tb = match &self.value {
            Some(Value::Exception(x)) => x.traceback.borrow().clone(),
            Some(Value::Instance(i)) => i.dict.borrow().get("__traceback__").cloned(),
            _ => None,
        };
        if let Some(Value::Ext(t)) = tb {
            if let Some(obj) = t.as_any().and_then(|a| a.downcast_ref::<crate::tbobj::TracebackObj>()) {
                self.tb = obj.frames().0.into_iter().rev().collect();
                return !self.tb.is_empty();
            }
        }
        false
    }

    /// Tira a marca de re-raise, se houver. `true`: o quadro que está saindo não deve se acrescentar.
    pub(crate) fn take_reraise_mark(&mut self) -> bool {
        if self.tb.last().is_some_and(|t| t.0 == usize::MAX) {
            self.tb.pop();
            return true;
        }
        false
    }
}

/// Entrada sentinela no fim de `tb`: "este quadro já está no traceback" (ver `PyException::reraised`).
#[allow(non_snake_case)]
fn RERAISE_MARK() -> TbEntry {
    (usize::MAX, String::new(), Rc::from(""), crate::compile::Span::default())
}

impl PyException {
}

/// `OSError` montado pelos módulos nativos como `[Errno N] texto: 'caminho'`: decomposto em
/// `(errno, strerror[, filename])`, os args que o CPython dá.
fn os_error_args(kind: &str, msg: &str) -> Option<Vec<Value>> {
    if !exc_is_subclass(kind, "OSError") {
        return None;
    }
    let rest = msg.strip_prefix("[Errno ")?;
    let (num, rest) = rest.split_once("] ")?;
    let errno: i64 = num.parse().ok()?;
    let mut args = vec![Value::Int(errno)];
    match rest.rsplit_once(": '") {
        Some((text, file)) if file.ends_with('\'') => {
            args.push(Value::str(text.to_string()));
            args.push(Value::str(file[..file.len() - 1].to_string()));
        }
        _ => args.push(Value::str(rest.to_string())),
    }
    Some(args)
}

/// Exceção não tratada com a linha da instrução que a levantou.
#[derive(Debug, Clone)]
pub struct RuntimeError {
    pub exc: PyException,
    pub lineno: usize,
}

pub type PyResult<T> = Result<T, PyException>;

/// `SyntaxError` (ou `IndentationError`/`TabError`) de um erro do parser, com os args do CPython:
/// `(msg, (filename, lineno, offset, text, end_lineno, end_offset))`.
pub fn syntax_exc(e: crate::parser::ParseError, filename: &str, src: &str) -> PyException {
    let kind = match e.kind {
        crate::parser::ErrorKind::Syntax => "SyntaxError",
        crate::parser::ErrorKind::Indentation => "IndentationError",
        crate::parser::ErrorKind::Tab => "TabError",
    };
    let text = src.lines().nth(e.lineno.saturating_sub(1)).map_or(Value::None, |l| Value::str(format!("{l}\n")));
    let details = Value::tuple(vec![
        Value::str(filename.to_string()),
        Value::Int(e.lineno as i64),
        Value::Int(e.offset as i64),
        text,
        Value::Int(e.end_lineno as i64),
        Value::Int(e.end_offset as i64),
    ]);
    let value = Value::Exception(Rc::new(ExcObj::new(kind, vec![Value::str(e.msg.clone()), details])));
    PyException { kind, msg: e.msg, value: Some(value), tb: Vec::new() }
}

/// Exceção da classe embutida `kind` com a mensagem.
pub fn exc(kind: &'static str, msg: impl Into<String>) -> PyException {
    PyException { kind, msg: msg.into(), value: None, tb: Vec::new() }
}

/// Dá ao `AttributeError` de um `obj.nome` o `name` e o `obj` que o CPython guarda (e que as sugestões usam).
fn tag_attribute_error(mut e: PyException, obj: &Value, name: &str) -> PyException {
    if e.kind == "AttributeError" && e.value.is_none() {
        e.value = Some(crate::suggest::attribute_error(e.msg.clone(), obj, name));
    }
    e
}

impl Vm {
    /// Nome do arquivo do script principal como o CPython o mostra (`co_filename`, tracebacks, avisos):
    /// caminho absoluto normalizado, `<stdin>` ou `<string>` (`-c`).
    pub(crate) fn script_name(&self) -> String {
        match self.argv.first().map(String::as_str) {
            Some("-") => "<stdin>".to_string(),
            Some(a) if !a.is_empty() && a != "-c" => crate::absolute_path(a),
            _ => "<string>".to_string(),
        }
    }

    /// `NameError` com o `name` e a lista de nomes visíveis (locais, closures, globais, embutidos), para
    /// o "Did you mean" na hora de mostrar o erro.
    fn name_error_ctx(&self, mut e: PyException, env: &Rc<Env>) -> PyException {
        if e.kind != "NameError" || e.value.is_some() {
            return e;
        }
        let Some(name) = e.msg.strip_prefix("name '").and_then(|m| m.strip_suffix("' is not defined")) else {
            return e;
        };
        let name = name.to_string();
        let mut scope: Vec<Value> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut push = |k: &str| {
            if seen.insert(k.to_string()) {
                scope.push(Value::str(k.to_string()));
            }
        };
        let mut self_has = false;
        let mut cur = Some(env.clone());
        while let Some(en) = cur {
            for k in en.vars.borrow().keys() {
                push(k);
            }
            if let Some(obj) = en.vars.borrow().get("self").cloned() {
                self_has |= self.clone().getattr(&obj, &name).is_ok();
            }
            cur = en.parent.clone();
        }
        for k in self.globals.borrow().keys() {
            push(k);
        }
        for k in crate::modules::builtinsmod::names() {
            push(k);
        }
        let x = ExcObj::new("NameError", vec![Value::str(e.msg.clone())]);
        x.extra.borrow_mut().push(("name", Value::str(name)));
        x.extra.borrow_mut().push(("scope", Value::list(scope)));
        if self_has {
            x.extra.borrow_mut().push(("self_has", Value::Bool(true)));
        }
        e.value = Some(Value::Exception(Rc::new(x)));
        e
    }

    /// Calcula o "Did you mean" da exceção que vai ser mostrada e das que a encadeiam.
    pub(crate) fn prepare_error(&mut self, e: &PyException) {
        let mut stack: Vec<Value> = e.value.iter().cloned().collect();
        let mut seen: Vec<Value> = Vec::new();
        while let Some(v) = stack.pop() {
            if seen.iter().any(|s| crate::object::is(s, &v)) {
                continue;
            }
            self.exc_hint(&v);
            let (cause, context, _) = exc_chain(&v);
            stack.extend(cause);
            stack.extend(context);
            seen.push(v);
        }
    }

    /// Calcula (uma vez) o sufixo `. Did you mean: 'x'?` de uma exceção e o guarda em `extra`. Devolve o sufixo.
    pub(crate) fn exc_hint(&mut self, v: &Value) -> String {
        let Value::Exception(e) = v else { return String::new() };
        if let Some(Value::Str(s)) = e.extra_get("hint") {
            return s.as_str().to_string();
        }
        let hint = crate::suggest::hint_for(e, |obj| crate::builtins_ext::dir_names(self, Some(obj)));
        e.extra.borrow_mut().push(("hint", Value::str(hint.clone())));
        hint
    }
}

pub fn type_error(msg: impl Into<String>) -> PyException {
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
thread_local! {
    /// Erro de um `__repr__`/`__str__` de usuário, que o `repr()` interno (sem `Result`) não consegue devolver:
    /// a instrução em andamento o levanta assim que termina.
    static TEXT_ERROR: RefCell<Option<PyException>> = const { RefCell::new(None) };
    static TEXT_ERROR_SET: Cell<bool> = const { Cell::new(false) };
    /// Profundidade de chamadas de quem pediu o texto: só instruções desse quadro (ou de fora dele) levantam o erro.
    static TEXT_ERROR_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Guarda o primeiro erro de conversão para texto, para o laço de instruções levantá-lo.
pub(crate) fn note_text_error(e: PyException, depth: usize) {
    TEXT_ERROR.with(|t| {
        let mut t = t.borrow_mut();
        if t.is_none() {
            *t = Some(e);
            TEXT_ERROR_DEPTH.with(|d| d.set(depth));
        }
    });
    TEXT_ERROR_SET.with(|s| s.set(true));
}

fn take_text_error() -> Option<PyException> {
    TEXT_ERROR_SET.with(|s| s.set(false));
    TEXT_ERROR.with(|t| t.borrow_mut().take())
}

thread_local! {
    static SOURCES: RefCell<HashMap<String, Rc<str>>> = RefCell::new(HashMap::new());
}

/// Registra o texto de um módulo, para os tracebacks que passam por ele.
pub fn register_source(file: &str, text: &str) {
    SOURCES.with(|s| s.borrow_mut().insert(file.to_string(), Rc::from(text)));
}

pub(crate) fn source_line(file: &str, line: usize) -> Option<String> {
    SOURCES.with(|s| s.borrow().get(file).and_then(|t| t.lines().nth(line.saturating_sub(1)).map(str::to_string)))
}

const CAUSE_MESSAGE: &str = "\nThe above exception was the direct cause of the following exception:\n\n";
const CONTEXT_MESSAGE: &str = "\nDuring handling of the above exception, another exception occurred:\n\n";

fn push_frame(out: &mut String, file: &str, src: Option<&str>, line: usize, name: &str, own: &str, span: crate::compile::Span) {
    let shown = if own.is_empty() { file } else { own };
    out.push_str(&format!("  File \"{shown}\", line {line}, in {name}\n"));
    let fetch = |n: usize| -> Option<String> {
        if own.is_empty() {
            src.and_then(|s| s.lines().nth(n.saturating_sub(1))).map(str::to_string)
        } else {
            source_line(own, n)
        }
    };
    let Some(first) = fetch(line) else { return };
    // O intervalo só vale se for da mesma linha que o quadro registrou (instruções sintéticas ficam sem carets).
    let known = span.lineno as usize == line && span.end_lineno >= span.lineno && span.end_col > 0;
    let (lines, span) = if known {
        let lines = (span.lineno..=span.end_lineno)
            .map(|n| if n as usize == line { first.clone() } else { fetch(n as usize).unwrap_or_default() })
            .collect();
        (lines, Some(span))
    } else {
        (vec![first], None)
    };
    out.push_str(&crate::carets::frame_body(&lines, span));
}

/// Quadros do mais antigo ao mais novo; a partir da quarta repetição idêntica seguida, o CPython
/// troca os quadros por `[Previous line repeated N more times]`.
fn push_frames(out: &mut String, file: &str, src: Option<&str>, frames: &[(usize, &str, &str, crate::compile::Span)]) {
    let mut last: Option<(usize, &str, &str)> = None;
    let mut count = 0usize;
    let flush = |out: &mut String, count: usize| {
        if count > 3 {
            let extra = count - 3;
            out.push_str(&format!("  [Previous line repeated {extra} more time{}]\n", if extra > 1 { "s" } else { "" }));
        }
    };
    for f in frames {
        // `warnings.warn` é código C no CPython: não aparece nos tracebacks.
        if f.2.ends_with("/warnings.py") && matches!(f.1, "warn" | "warn_explicit") {
            continue;
        }
        if last != Some((f.0, f.1, f.2)) {
            flush(out, count);
            last = Some((f.0, f.1, f.2));
            count = 0;
        }
        count += 1;
        if count <= 3 {
            push_frame(out, file, src, f.0, f.1, f.2, f.3);
        }
    }
    flush(out, count);
}

/// As seções das exceções que antecedem `v` (causa ou contexto), do mais antigo para o mais novo,
/// cada uma com o aviso que o CPython imprime entre elas.
fn chain_prefix(v: &Value, file: &str, src: Option<&str>, seen: &mut Vec<Value>) -> String {
    seen.push(v.clone());
    let (cause, context, suppress) = exc_chain(v);
    let link = match cause {
        Some(c) => Some((c, CAUSE_MESSAGE)),
        None if !suppress => context.map(|c| (c, CONTEXT_MESSAGE)),
        None => None,
    };
    let Some((c, message)) = link else { return String::new() };
    if seen.iter().any(|s| crate::object::is(s, &c)) {
        return String::new();
    }
    let mut out = chain_prefix(&c, file, src, seen);
    out.push_str(&exc_section(&c, file, src));
    out.push_str(message);
    out
}

/// Sufixo "Did you mean" já calculado (`Vm::prepare_error`); vazio se não há.
fn hint_of(v: &Value) -> String {
    match v {
        Value::Exception(e) => match e.extra_get("hint") {
            Some(Value::Str(s)) => s.as_str().to_string(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// Traceback e linha final de uma exceção já capturada (usa o `__traceback__` dela).
fn exc_section(v: &Value, file: &str, src: Option<&str>) -> String {
    let mut out = String::new();
    let tb = match v {
        Value::Exception(e) => e.traceback.borrow().clone(),
        Value::Instance(i) => i.dict.borrow().get("__traceback__").cloned(),
        _ => None,
    };
    let frames = match tb {
        Some(Value::Ext(x)) => x
            .as_any()
            .and_then(|a| a.downcast_ref::<crate::tbobj::TracebackObj>())
            .map(crate::tbobj::TracebackObj::frames),
        _ => None,
    };
    if let Some((frames, _)) = frames {
        out.push_str("Traceback (most recent call last):\n");
        let list: Vec<(usize, &str, &str, crate::compile::Span)> = frames.iter().map(|(l, n, o, s)| (*l, n.as_str(), &**o, *s)).collect();
        push_frames(&mut out, file, src, &list);
    }
    let pe = PyException::from_value(v);
    if pe.msg.is_empty() {
        out.push_str(pe.kind);
        out.push('\n');
    } else {
        out.push_str(&format!("{}: {}{}\n", pe.kind, pe.msg, hint_of(v)));
    }
    out
}

pub fn format_traceback_in(err: &RuntimeError, file: &str, src: Option<&str>) -> String {
    let mut out = match &err.exc.value {
        Some(v) => chain_prefix(v, file, src, &mut Vec::new()),
        None => String::new(),
    };
    out.push_str("Traceback (most recent call last):\n");
    if err.exc.tb.is_empty() {
        push_frame(&mut out, file, src, err.lineno, "<module>", "", crate::compile::Span::default());
    }
    let list: Vec<(usize, &str, &str, crate::compile::Span)> = err.exc.tb.iter().rev().map(|(l, n, o, s)| (*l, n.as_str(), &**o, *s)).collect();
    push_frames(&mut out, file, src, &list);
    if err.exc.msg.is_empty() {
        out.push_str(err.exc.kind);
        out.push('\n');
    } else {
        out.push_str(&format!(
            "{}: {}{}\n",
            err.exc.kind,
            err.exc.msg,
            err.exc.value.as_ref().map(hint_of).unwrap_or_default()
        ));
    }
    out
}

/// Funções embutidas desta fatia.
pub(crate) const BUILTINS: &[&str] = &[
    "print", "len", "range", "str", "int", "repr", "open", "list", "tuple", "bool", "float", "abs", "min",
    "max", "sum", "sorted", "reversed", "enumerate", "zip", "any", "all", "ord", "chr",
];

pub use crate::classes::{instance_eq, instance_hash, instance_text, instance_truth};

/// Como um quadro termina: devolvendo um valor ou suspendendo num `yield`.
pub(crate) enum Exit {
    Return(Value),
    Yield(Value),
}

/// Iterador de um laço `for`, que vive na pilha da VM e não é um `Value`.
pub(crate) enum PyIter {
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
    /// Objeto de módulo nativo iterável (`re.finditer`).
    Ext(Rc<dyn crate::object::ExtObject>),
    /// Instância de classe de usuário com `__next__`.
    Inst(Value),
}

impl PyIter {
    pub(crate) fn next(&mut self) -> PyResult<Option<Value>> {
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
            PyIter::Ext(e) => e.iter_next()?,
            PyIter::Inst(v) => {
                let mut vm = current().ok_or_else(|| internal("no vm"))?;
                match vm.call_dunder(v, "__next__", Vec::new()) {
                    Some(Ok(x)) => Some(x),
                    Some(Err(e)) if e.kind == "StopIteration" => None,
                    Some(Err(e)) => return Err(e),
                    None => return Err(type_error(format!("'{}' object is not an iterator", v.type_name()))),
                }
            }
        })
    }
}

pub(crate) fn get_iter(v: &Value) -> PyResult<PyIter> {
    Ok(match v {
        Value::List(l) => PyIter::List(l.clone(), 0),
        Value::Tuple(t) => PyIter::Tuple(t.clone(), 0),
        Value::Str(s) => PyIter::Str(s.clone(), 0),
        Value::Range(r) => PyIter::Range { next: r.start, step: r.step, remaining: r.len() },
        Value::Dict(d) => PyIter::Items(d.borrow().keys().cloned().collect(), 0),
        Value::Set(s) => PyIter::Items(s.borrow().iter().cloned().collect(), 0),
        Value::Bytes(b) => PyIter::Items(b.iter().map(|&x| Value::Int(i64::from(x))).collect(), 0),
        Value::ByteArray(b) => PyIter::Items(b.borrow().iter().map(|&x| Value::Int(i64::from(x))).collect(), 0),
        Value::Native(n) if matches!(&*n.borrow(), Native::File(_) | Native::CsvReader { .. }) => {
            PyIter::Native(n.clone())
        }
        Value::Ext(e) if e.to_items().is_some() => PyIter::Items(e.to_items().unwrap_or_default(), 0),
        Value::Ext(e) if e.is_iterable() => PyIter::Ext(e.clone()),
        Value::Class(c) => {
            let mut vm = current().ok_or_else(|| internal("no vm"))?;
            match vm.meta_dunder(c, "__iter__", Vec::new(), Vec::new()) {
                Some(r) => get_iter(&r?)?,
                None => return Err(type_error(format!("'type' object is not iterable"))),
            }
        }
        Value::Instance(_) => {
            let mut vm = current().ok_or_else(|| internal("no vm"))?;
            match vm.call_dunder(v, "__iter__", Vec::new()) {
                Some(r) => match r? {
                    it @ Value::Instance(_) => PyIter::Inst(it),
                    other => get_iter(&other)?,
                },
                None => {
                    // Protocolo antigo de sequência: `__getitem__(0)`, `__getitem__(1)`... até `IndexError`.
                    let mut items = Vec::new();
                    let mut i = 0i64;
                    loop {
                        match vm.call_dunder(v, "__getitem__", vec![Value::Int(i)]) {
                            None => return Err(type_error(format!("'{}' object is not iterable", v.type_name()))),
                            Some(Ok(item)) => items.push(item),
                            Some(Err(e)) if e.kind == "IndexError" || e.kind == "StopIteration" => break,
                            Some(Err(e)) => return Err(e),
                        }
                        i += 1;
                    }
                    PyIter::Items(items, 0)
                }
            }
        }
        _ => return Err(type_error(format!("'{}' object is not iterable", v.type_name()))),
    })
}

/// `a < b` com a semântica do Python (usado por `sorted`, `min`, `max`, `list.sort`).
pub fn py_lt(a: &Value, b: &Value) -> PyResult<bool> {
    compare(CmpOp::Lt, a, b)
}

/// `a <op> b` pelo símbolo de comparação (`==`, `!=`, `<`, `<=`, `>`, `>=`), para os `__eq__`... dos embutidos.
pub(crate) fn py_compare(sym: &str, a: &Value, b: &Value) -> PyResult<bool> {
    let op = match sym {
        "==" => CmpOp::Eq,
        "!=" => CmpOp::NotEq,
        "<" => CmpOp::Lt,
        "<=" => CmpOp::LtE,
        ">" => CmpOp::Gt,
        _ => CmpOp::GtE,
    };
    compare(op, a, b)
}

/// `item in container`.
pub(crate) fn py_contains(container: &Value, item: &Value) -> PyResult<bool> {
    contains(container, item)
}

/// `container[index]`.
pub(crate) fn py_subscript(container: &Value, index: &Value) -> PyResult<Value> {
    subscript(container, index)
}

/// Operador binário `a <op> b` (`op` pelo símbolo: `"+"`, `"-"`, `"*"`, `"/"`, `"//"`, `"%"`, `"**"`).
pub fn py_binary(sym: &str, a: &Value, b: &Value) -> PyResult<Value> {
    let op = match sym {
        "+" => Operator::Add,
        "-" => Operator::Sub,
        "*" => Operator::Mult,
        "/" => Operator::Div,
        "//" => Operator::FloorDiv,
        "%" => Operator::Mod,
        "**" => Operator::Pow,
        _ => return Err(type_error(format!("unsupported operator {sym}"))),
    };
    binary(op, a, b, false)
}

/// Todos os itens de um iterável (para funções nativas que consomem uma sequência inteira).
pub fn iterate(v: &Value) -> PyResult<Vec<Value>> {
    collect(v)
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
pub(crate) enum Slot {
    Val(Value),
    Iter(PyIter),
}

/// Estado do interpretador. Todos os campos são compartilhados (`Rc`), então clonar a `Vm` é barato
/// e as cópias enxergam o mesmo estado: é assim que um gerador se retoma sozinho e que as funções
/// livres (`binary`, `compare`, `repr`...) chamam de volta o Python (`__add__`, `__repr__`...).
#[derive(Clone)]
pub struct Vm {
    pub(crate) globals: Rc<RefCell<crate::object::VarMap>>,
    /// Buffer do stdout, descarregado pelo chamador no fim.
    pub stdout: Rc<RefCell<Vec<u8>>>,
    /// Exceções sendo tratadas (a mais recente por último), para `raise` sem argumento.
    handled: Rc<RefCell<Vec<Value>>>,
    /// Profundidade de chamadas de função em andamento.
    depth: Rc<std::cell::Cell<usize>>,
    /// Linha da instrução em execução (para `sys._getframe` e `warnings`).
    pub(crate) cur_line: Rc<std::cell::Cell<usize>>,
    /// Funções em andamento (a mais interna por último), cada uma com a linha do chamador.
    pub(crate) frames: Rc<RefCell<Vec<(Rc<Code>, usize)>>>,
    /// `sys.argv`.
    pub(crate) argv: Rc<Vec<String>>,
    /// `sys.stdin`, `sys.stdout` e `sys.stderr`, criados uma vez.
    pub(crate) std_files: [Rc<RefCell<Native>>; 3],
    /// Módulos já importados, por nome.
    pub(crate) modules: Rc<RefCell<HashMap<String, Rc<crate::object::ModuleObj>>>>,
    /// Globais vivas dos módulos carregados de arquivo (por nome): `mod.x` lê e grava aqui, então
    /// o módulo e quem o importou enxergam o mesmo estado.
    pub(crate) module_globals: Rc<RefCell<HashMap<&'static str, Rc<RefCell<crate::object::VarMap>>>>>,
}

thread_local! {
    /// A `Vm` da thread, para as funções livres que precisam chamar código Python.
    static CURRENT: RefCell<Option<Vm>> = const { RefCell::new(None) };
}

/// A `Vm` em execução nesta thread (clone barato), se existir.
/// Os bytes de um `memoryview` (`tobytes()`), para os módulos nativos que pedem um buffer.
pub(crate) fn memoryview_bytes(v: &Value) -> Option<Rc<[u8]>> {
    let mut vm = current()?;
    match vm.call_dunder(v, "__bytes__", Vec::new()) {
        Some(Ok(Value::Bytes(b))) => Some(b),
        _ => None,
    }
}

pub fn current() -> Option<Vm> {
    CURRENT.with(|c| c.borrow().clone())
}

/// Limite de recursão (`sys.getrecursionlimit()` do CPython).
const MAX_DEPTH: usize = 1000;

/// Ligada quando o programa registra um tratador de sinal: a VM passa a consultar os sinais capturados.
pub(crate) static SIGNALS_ARMED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A thread que registrou o primeiro tratador: só ela roda tratadores (no CPython, só a principal).
pub(crate) static SIGNAL_THREAD: std::sync::OnceLock<std::thread::ThreadId> = std::sync::OnceLock::new();

/// Prazo do `signal.alarm`, em nanossegundos do relógio monotônico (0: sem alarme).
pub(crate) static ALARM_AT_NS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// O relógio monotônico do pseudo-processo, em nanossegundos (`None` sem pseudo-processo).
pub(crate) fn monotonic_ns() -> Option<i64> {
    let t = sysabi::sys::try_current()?.clock_gettime(sysabi::Clock::Monotonic).ok()?;
    Some(t.sec * 1_000_000_000 + i64::from(t.nsec))
}

thread_local! {
    /// Valor ajustável por `sys.setrecursionlimit`.
    pub(crate) static RECURSION_LIMIT: std::cell::Cell<usize> = const { std::cell::Cell::new(MAX_DEPTH) };
}

/// Bloco protegido aberto por `SetupTry`.
pub(crate) struct Block {
    pub(crate) handler: usize,
    pub(crate) depth: usize,
    pub(crate) handled: usize,
}

pub(crate) fn internal(msg: &str) -> PyException {
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

    /// Roda as funções registradas em `atexit` (se o módulo foi importado), como o CPython na saída.
    pub fn run_exit_hooks(&mut self) {
        let hook = self
            .modules
            .borrow()
            .get("atexit")
            .and_then(|m| m.attrs.borrow().get("_run_exitfuncs").cloned());
        if let Some(f) = hook {
            let _ = self.call(&f, Vec::new(), Vec::new());
        }
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
                raw: Vec::new(),
                raw_eof: false,
            })))
        };
        let vm = Vm {
            globals: Rc::new(RefCell::new(crate::object::VarMap::from_iter([("__name__".to_string(), Value::str("__main__"))]))),
            stdout: Rc::new(RefCell::new(Vec::new())),
            handled: Rc::new(RefCell::new(Vec::new())),
            depth: Rc::new(std::cell::Cell::new(0)),
            cur_line: Rc::new(std::cell::Cell::new(0)),
            frames: Rc::new(RefCell::new(Vec::new())),
            argv: Rc::new(argv),
            modules: Rc::new(RefCell::new(HashMap::new())),
            module_globals: Rc::new(RefCell::new(HashMap::new())),
            std_files: [file(FileKind::Stdin, "<stdin>"), file(FileKind::Stdout, "<stdout>"), file(FileKind::Stderr, "<stderr>")],
        };
        CURRENT.with(|c| *c.borrow_mut() = Some(vm.clone()));
        // O script principal como módulo: o `__main__` enxerga as globais dele de qualquer módulo.
        vm.module_globals.borrow_mut().insert("__main__", vm.globals.clone());
        vm
    }

    /// `obj.nome` (atributo ou método preso), como o bytecode `LoadAttr`.
    pub fn getattr(&mut self, obj: &Value, name: &str) -> PyResult<Value> {
        self.load_attr(obj, name)
    }

    /// Chama qualquer valor chamável (função de usuário, builtin, método). É o que as funções
    /// nativas usam para devolver a chamada ao Python (`key=` de `sorted`, `map`, callbacks).
    pub fn call_value(&mut self, f: &Value, args: Vec<Value>, kw: crate::object::Kw) -> PyResult<Value> {
        self.call(f, args, kw)
    }

    /// Profundidade atual de chamadas de função (para quem precisa saber em que quadro está).
    pub(crate) fn depth_now(&self) -> usize {
        self.depth.get()
    }

    /// Executa o código de um módulo.
    pub fn run(&mut self, code: &Rc<Code>) -> Result<(), RuntimeError> {
        let env = Env::new(None, false, true);
        match self.exec(code, &env) {
            Ok(_) => Ok(()),
            Err(e) => Err(RuntimeError { lineno: e.tb.last().map_or(0, |t| t.0), exc: e }),
        }
    }

    /// Executa o código de um módulo, de uma função ou de um corpo de classe até o `Return`.
    pub(crate) fn exec(&mut self, code: &Rc<Code>, env: &Rc<Env>) -> PyResult<Value> {
        let mut stack: Vec<Slot> = Vec::new();
        let mut blocks: Vec<Block> = Vec::new();
        let mut pc = 0;
        match self.run_loop(code, env, &mut stack, &mut blocks, &mut pc, None)? {
            Exit::Return(v) => Ok(v),
            Exit::Yield(_) => Err(internal("yield outside generator")),
        }
    }

    /// O laço de instruções, com o estado do quadro (pilha, blocos protegidos, `pc`) vindo de fora:
    /// um gerador guarda esse estado entre um `yield` e o `next` seguinte.
    pub(crate) fn run_loop(
        &mut self,
        code: &Rc<Code>,
        env: &Rc<Env>,
        stack: &mut Vec<Slot>,
        blocks: &mut Vec<Block>,
        pc: &mut usize,
        inject: Option<PyException>,
    ) -> PyResult<Exit> {
        let mut pending = inject;
        let mut signal_tick: u32 = 0;
        // `throw` num gerador parado numa delegação (`await`/`yield from`) vai para o sub-iterador.
        if pending.is_some() {
            if let Some(Op::DelegateNext(l)) = code.ops.get(*pc).copied() {
                let e = pending.take().unwrap_or_else(|| internal("no exception"));
                match self.delegate_throw(stack, e) {
                    Ok(Step::Yield(v)) => return Ok(Exit::Yield(v)),
                    Ok(Step::Done(v)) => {
                        stack.pop();
                        stack.push(Slot::Val(v));
                        if let Op::Delegate(end) = code.ops[l as usize] {
                            *pc = end as usize;
                        }
                    }
                    Err(e2) => pending = Some(e2),
                }
            }
        }
        while *pc < code.ops.len() {
            let op = code.ops[*pc];
            self.cur_line.set(code.lines[*pc]);
            // Sinais capturados chegam entre instruções, como no CPython (só depois de um `signal.signal`).
            if SIGNALS_ARMED.load(std::sync::atomic::Ordering::Relaxed) && pending.is_none() {
                signal_tick = signal_tick.wrapping_add(1);
                if signal_tick & 0x1fff == 0 {
                    if let Err(e) = self.deliver_signals() {
                        pending = Some(e);
                    }
                }
            }
            let result = if let Some(e) = pending.take() {
                Err(e)
            } else {
                match op {
                    Op::SetupTry(h) => {
                        blocks.push(Block { handler: h as usize, depth: stack.len(), handled: self.handled.borrow().len() });
                        Ok(None)
                    }
                    Op::PopBlock => {
                        blocks.pop();
                        Ok(None)
                    }
                    Op::Return => match stack.pop() {
                        Some(Slot::Val(v)) => return Ok(Exit::Return(v)),
                        _ => Err(internal("bad value stack")),
                    },
                    Op::Yield => match stack.pop() {
                        Some(Slot::Val(v)) => {
                            *pc += 1;
                            return Ok(Exit::Yield(v));
                        }
                        _ => Err(internal("bad value stack")),
                    },
                    Op::AsyncGenYield => match stack.pop() {
                        Some(Slot::Val(v)) => {
                            *pc += 1;
                            return Ok(Exit::Yield(crate::generator::wrap_async_value(v)));
                        }
                        _ => Err(internal("bad value stack")),
                    },
                    // Instruções quentes resolvidas aqui: `step` tem um quadro enorme (um `match` com
                    // centenas de braços) e chamá-lo a cada instrução custa mais que o trabalho.
                    Op::LoadConst(i) => {
                        stack.push(Slot::Val(code.consts[i as usize].clone()));
                        Ok(None)
                    }
                    Op::Jump(t) => Ok(Some(t as usize)),
                    Op::Pop => {
                        stack.pop();
                        Ok(None)
                    }
                    Op::StoreName(i) => match stack.pop() {
                        Some(Slot::Val(v)) => {
                            let name = &code.names[i as usize];
                            let mut g = self.globals.borrow_mut();
                            match g.get_mut(name) {
                                Some(slot) => *slot = v,
                                None => {
                                    g.insert(name.clone(), v);
                                }
                            }
                            Ok(None)
                        }
                        _ => Err(internal("bad value stack")),
                    },
                    Op::ForIter(t) => match stack.last_mut() {
                        Some(Slot::Iter(it)) => match it.next() {
                            Ok(Some(v)) => {
                                stack.push(Slot::Val(v));
                                Ok(None)
                            }
                            Ok(None) => {
                                stack.pop();
                                Ok(Some(t as usize))
                            }
                            Err(e) => Err(e),
                        },
                        _ => Err(internal("FOR_ITER without iterator")),
                    },
                    Op::PopJumpIfFalse(t) => match stack.pop() {
                        Some(Slot::Val(v)) => Ok(if v.is_true() { None } else { Some(t as usize) }),
                        _ => Err(internal("bad value stack")),
                    },
                    Op::LoadName(i) if env.parent.is_none() => {
                        match self.global_or_builtin(&code.names[i as usize]) {
                            Ok(v) => {
                                stack.push(Slot::Val(v));
                                Ok(None)
                            }
                            Err(e) => Err(self.name_error_ctx(e, env)),
                        }
                    }
                    Op::Binary { op: bop, inplace } if num_pair(stack) => {
                        let (Some(Slot::Val(b)), Some(Slot::Val(a))) = (stack.pop(), stack.pop()) else {
                            return Err(internal("bad value stack"));
                        };
                        match binary(bop, &a, &b, inplace) {
                            Ok(v) => {
                                stack.push(Slot::Val(v));
                                Ok(None)
                            }
                            Err(e) => Err(e),
                        }
                    }
                    Op::Compare(cop) if num_pair(stack) => {
                        let (Some(Slot::Val(b)), Some(Slot::Val(a))) = (stack.pop(), stack.pop()) else {
                            return Err(internal("bad value stack"));
                        };
                        match compare(cop, &a, &b) {
                            Ok(v) => {
                                stack.push(Slot::Val(Value::Bool(v)));
                                Ok(None)
                            }
                            Err(e) => Err(e),
                        }
                    }
                    Op::LoadLocal(i) if !env.is_class => {
                        let found = env.vars.borrow().get(&code.names[i as usize]).cloned();
                        match found {
                            Some(v) => {
                                stack.push(Slot::Val(v));
                                Ok(None)
                            }
                            None => self.step(code, op, stack, env),
                        }
                    }
                    Op::Call { argc, kwnames: None } if stack.len() > argc as usize => {
                        let at = stack.len() - argc as usize - 1;
                        let mut drained = stack.drain(at..);
                        let func = match drained.next() {
                            Some(Slot::Val(v)) => v,
                            _ => return Err(internal("bad value stack")),
                        };
                        let mut values = Vec::with_capacity(argc as usize);
                        for s in drained {
                            match s {
                                Slot::Val(v) => values.push(v),
                                _ => return Err(internal("bad value stack")),
                            }
                        }
                        match self.call(&func, values, Vec::new()) {
                            Ok(v) => {
                                stack.push(Slot::Val(v));
                                Ok(None)
                            }
                            Err(e) => Err(e),
                        }
                    }
                    _ => self.step(code, op, stack, env),
                }
            };
            let result = match result {
                Ok(_) if TEXT_ERROR_SET.with(Cell::get) && self.depth.get() <= TEXT_ERROR_DEPTH.with(Cell::get) => {
                    take_text_error().map_or(Ok(None), Err)
                }
                other => other,
            };
            match result {
                Ok(Some(target)) => *pc = target,
                Ok(None) => *pc += 1,
                Err(mut e) => match blocks.pop() {
                    Some(b) => {
                        stack.truncate(b.depth);
                        self.handled.borrow_mut().truncate(b.handled);
                        let value = e.to_value();
                        // `__traceback__`: o quadro que captura primeiro, depois os internos.
                        let reraised = e.take_reraise_mark();
                        let mut entries: Vec<TbEntry> = e.tb.iter().rev().cloned().collect();
                        if !reraised {
                            match entries.first_mut() {
                                Some(first) if is_inlined_comp(&first.1) => first.1 = code.name.clone(),
                                _ => entries.insert(0, (code.lines[*pc], code.name.clone(), Rc::from(code.filename.as_str()), code.spans[*pc])),
                            }
                        }
                        let filename = self.script_name();
                        let tb = crate::tbobj::TracebackObj::make(entries, &filename);
                        match &value {
                            Value::Exception(x) => *x.traceback.borrow_mut() = Some(tb),
                            Value::Instance(i) => {
                                i.dict.borrow_mut().insert("__traceback__".to_string(), tb);
                            }
                            _ => {}
                        }
                        stack.push(Slot::Val(value));
                        *pc = b.handler;
                    }
                    None => {
                        if !e.take_reraise_mark() {
                            match e.tb.last_mut() {
                                Some(last) if is_inlined_comp(&last.1) => last.1 = code.name.clone(),
                                _ => e.tb.push((code.lines[*pc], code.name.clone(), Rc::from(code.filename.as_str()), code.spans[*pc])),
                            }
                        }
                        return Err(e);
                    }
                },
            }
        }
        Ok(Exit::Return(Value::None))
    }

    /// `__context__` implícito: a exceção em tratamento quando `e` é levantada.
    pub(crate) fn link_context(&self, e: &mut PyException) {
        let Some(ctx) = self.handled.borrow().last().cloned() else { return };
        let value = e.to_value();
        e.value = Some(value.clone());
        exc_set_context(&value, &ctx);
    }

    /// Roda os tratadores de `signal.signal` dos sinais capturados que chegaram. Só faz algo depois que o
    /// programa registrou um tratador; um tratador que levanta (`KeyboardInterrupt`...) interrompe quem chamou.
    pub(crate) fn deliver_signals(&mut self) -> PyResult<()> {
        if !SIGNALS_ARMED.load(std::sync::atomic::Ordering::Relaxed) || sysabi::sys::try_current().is_none() {
            return Ok(());
        }
        if SIGNAL_THREAD.get().is_some_and(|t| *t != std::thread::current().id()) {
            return Ok(());
        }
        // Alarme vencido: o SIGALRM chega ao processo como qualquer outro sinal (padrão: termina; capturado: tratador).
        let at = ALARM_AT_NS.load(std::sync::atomic::Ordering::Relaxed);
        if at != 0 && monotonic_ns().is_some_and(|now| now >= at) {
            ALARM_AT_NS.store(0, std::sync::atomic::Ordering::Relaxed);
            let process = sysabi::sys::current();
            let _ = process.kill(sysabi::KillTarget::Pid(process.getpid()), sysabi::Signal(14));
        }
        let caught = sysabi::sys::current().take_caught_signals();
        if caught.is_empty() {
            return Ok(());
        }
        let Some(module) = crate::modules::import(self, "signal") else { return Ok(()) };
        let dispatch = module.attrs.borrow().get("_dispatch").cloned();
        if let Some(f) = dispatch {
            let list = Value::list(caught.into_iter().map(|s| Value::Int(i64::from(s.0))).collect());
            self.call(&f, vec![list], Vec::new())?;
        }
        Ok(())
    }

    pub(crate) fn call_function(&mut self, f: &Rc<FuncObj>, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
        // Função de outro módulo: roda numa `Vm` que enxerga as globais dela.
        if !Rc::ptr_eq(&self.globals, &f.globals) {
            let mut other = self.clone();
            other.globals = f.globals.clone();
            return other.call_function(f, args, kwargs);
        }
        let env = self.bind_params(f, args, kwargs)?;
        let code = f.code.clone();
        if code.is_generator || code.is_async {
            return Ok(crate::generator::new_generator(self.clone(), code, env));
        }
        if self.depth.get() + 1 >= RECURSION_LIMIT.with(|c| c.get()) {
            return Err(exc("RecursionError", "maximum recursion depth exceeded"));
        }
        self.depth.set(self.depth.get() + 1);
        let caller_line = self.cur_line.get();
        self.frames.borrow_mut().push((code.clone(), caller_line));
        // `return` dentro de um `except` sai sem fechar o tratador: a pilha volta ao tamanho de antes.
        let handled_len = self.handled.borrow().len();
        let mut result = self.exec(&code, &env);
        // Função embutida no CPython (escrita em Python aqui): o traceback não mostra o interior dela.
        if let Err(e) = &mut result {
            if f.attrs.borrow().contains_key("__no_bind__") {
                while e.tb.last().is_some_and(|t| t.2.starts_with("/usr/lib/python3.13/")) {
                    e.tb.pop();
                }
            }
        }
        self.handled.borrow_mut().truncate(handled_len);
        self.frames.borrow_mut().pop();
        self.cur_line.set(caller_line);
        self.depth.set(self.depth.get() - 1);
        result
    }

    /// Liga os argumentos aos parâmetros como o CPython (`_PyEval_MakeFrameVector`) e devolve o
    /// escopo da chamada, com as mesmas mensagens de `TypeError`.
    fn bind_params(
        &mut self,
        f: &Rc<FuncObj>,
        args: Vec<Value>,
        kwargs: Vec<(String, Value)>,
    ) -> PyResult<Rc<Env>> {
        let code = &f.code;
        // Caminho rápido: só posicionais, na quantidade exata e sem `*args`/`**kw`/só-nomeados.
        if kwargs.is_empty()
            && args.len() == code.params.len()
            && code.vararg.is_none()
            && code.kwarg.is_none()
            && code.kwonly.is_empty()
        {
            let env = Env::new(f.closure.clone(), false, false);
            {
                let mut vars = env.vars.borrow_mut();
                vars.reserve(args.len() + 4);
                for (p, v) in code.params.iter().zip(args) {
                    vars.insert(p.clone(), v);
                }
            }
            return Ok(env);
        }
        let name = code.qual();
        let params = &code.params;
        let n = params.len();
        let ndefaults = f.defaults.len();
        let required = n - ndefaults;
        let join = |missing: &[String]| -> String {
            let quoted: Vec<String> = missing.iter().map(|m| format!("'{m}'")).collect();
            match quoted.as_slice() {
                [one] => one.clone(),
                [a, b] => format!("{a} and {b}"),
                many => format!("{}, and {}", many[..many.len() - 1].join(", "), many[many.len() - 1]),
            }
        };
        if args.len() > n && code.vararg.is_none() {
            let takes = if ndefaults == 0 { format!("{n}") } else { format!("from {required} to {n}") };
            let plural = if ndefaults == 0 && n == 1 { "" } else { "s" };
            let given = if args.len() == 1 { "was" } else { "were" };
            return Err(type_error(format!(
                "{name}() takes {takes} positional argument{plural} but {} {given} given",
                args.len()
            )));
        }
        let mut slots: Vec<Option<Value>> = vec![None; n];
        let mut extra: Vec<Value> = Vec::new();
        for (i, a) in args.into_iter().enumerate() {
            if i < n {
                slots[i] = Some(a);
            } else {
                extra.push(a);
            }
        }
        let mut kwonly_vals: HashMap<String, Value> = HashMap::new();
        let mut extra_kw: Vec<(String, Value)> = Vec::new();
        let mut posonly_given: Vec<String> = Vec::new();
        for (k, v) in kwargs {
            match params.iter().position(|p| *p == k) {
                Some(i) if i < code.posonly => {
                    if code.kwarg.is_some() {
                        extra_kw.push((k, v));
                    } else {
                        posonly_given.push(k);
                    }
                }
                Some(i) => {
                    if slots[i].is_some() {
                        return Err(type_error(format!("{name}() got multiple values for argument '{k}'")));
                    }
                    slots[i] = Some(v);
                }
                None if code.kwonly.contains(&k) => {
                    if kwonly_vals.contains_key(&k) {
                        return Err(type_error(format!("{name}() got multiple values for argument '{k}'")));
                    }
                    kwonly_vals.insert(k, v);
                }
                None if code.kwarg.is_some() => extra_kw.push((k, v)),
                None => return Err(type_error(format!("{name}() got an unexpected keyword argument '{k}'"))),
            }
        }
        if !posonly_given.is_empty() {
            let list = posonly_given.iter().map(|m| format!("'{m}'")).collect::<Vec<_>>().join(", ");
            return Err(type_error(format!(
                "{name}() got some positional-only arguments passed as keyword arguments: {list}"
            )));
        }
        let mut missing: Vec<String> = Vec::new();
        for i in 0..n {
            if slots[i].is_none() {
                if i >= required {
                    slots[i] = Some(f.defaults[i - required].clone());
                } else {
                    missing.push(params[i].clone());
                }
            }
        }
        if !missing.is_empty() {
            let plural = if missing.len() == 1 { "" } else { "s" };
            return Err(type_error(format!(
                "{name}() missing {} required positional argument{plural}: {}",
                missing.len(),
                join(&missing)
            )));
        }
        let mut missing_kw: Vec<String> = Vec::new();
        for k in &code.kwonly {
            if !kwonly_vals.contains_key(k) {
                match f.kwdefaults.iter().find(|(n, _)| n == k) {
                    Some((_, v)) => {
                        kwonly_vals.insert(k.clone(), v.clone());
                    }
                    None => missing_kw.push(k.clone()),
                }
            }
        }
        if !missing_kw.is_empty() {
            let plural = if missing_kw.len() == 1 { "" } else { "s" };
            return Err(type_error(format!(
                "{name}() missing {} required keyword-only argument{plural}: {}",
                missing_kw.len(),
                join(&missing_kw)
            )));
        }
        let env = Env::new(f.closure.clone(), false, false);
        {
            let mut vars = env.vars.borrow_mut();
            for (p, v) in params.iter().zip(slots) {
                if let Some(v) = v {
                    vars.insert(p.clone(), v);
                }
            }
            if let Some(va) = &code.vararg {
                vars.insert(va.clone(), Value::tuple(extra));
            }
            for (k, v) in kwonly_vals {
                vars.insert(k, v);
            }
            if let Some(kw) = &code.kwarg {
                let mut d = Dict::new();
                for (k, v) in extra_kw {
                    d.set(Value::str(k), v)?;
                }
                vars.insert(kw.clone(), Value::dict(d));
            }
        }
        Ok(env)
    }

    /// Executa uma instrução; `Some(alvo)` quando ela salta.
    fn step(
        &mut self,
        code: &Rc<Code>,
        op: Op,
        stack: &mut Vec<Slot>,
        locals: &Rc<Env>,
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
                // Escopos de função externos (closures), depois globais e embutidos.
                let mut found = None;
                let mut cur = locals.parent.clone();
                while let Some(env) = cur {
                    if let Some(v) = env.vars.borrow().get(name) {
                        found = Some(v.clone());
                        break;
                    }
                    cur = env.parent.clone();
                }
                let v = match found {
                    Some(v) => v,
                    None => self.global_or_builtin(name).map_err(|e| self.name_error_ctx(e, locals))?,
                };
                stack.push(Slot::Val(v));
            }
            Op::LoadGlobal(i) => {
                let name = &code.names[i as usize];
                let v = self.global_or_builtin(name).map_err(|e| self.name_error_ctx(e, locals))?;
                stack.push(Slot::Val(v));
            }
            Op::StoreName(i) => {
                let v = pop(stack)?;
                let name = &code.names[i as usize];
                let mut g = self.globals.borrow_mut();
                match g.get_mut(name) {
                    Some(slot) => *slot = v,
                    None => {
                        g.insert(name.clone(), v);
                    }
                }
            }
            Op::StoreNonlocal(i) => {
                let v = pop(stack)?;
                let name = &code.names[i as usize];
                let mut cur = locals.parent.clone();
                loop {
                    let Some(env) = cur else {
                        return Err(exc("SyntaxError", format!("no binding for nonlocal '{name}' found")));
                    };
                    if env.vars.borrow().contains_key(name) {
                        env.vars.borrow_mut().insert(name.clone(), v);
                        break;
                    }
                    cur = env.parent.clone();
                }
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
                // `"..." % args` com instâncias nos argumentos: `__str__`, `__int__`, `__float__`...
                let has_instance = match (op, &a) {
                    (Operator::Mod, Value::Str(_)) => match &b {
                        Value::Instance(_) => true,
                        Value::Tuple(t) => t.iter().any(|x| matches!(x, Value::Instance(_))),
                        Value::Dict(d) => d.borrow().values().any(|x| matches!(x, Value::Instance(_))),
                        _ => false,
                    },
                    _ => false,
                };
                if let (true, Value::Str(fmt)) = (has_instance, &a) {
                    let text = crate::format::percent_format_with(fmt.as_str(), &b, &mut |conv, v| match conv {
                        's' => self.str_of(v).map(Value::str),
                        'r' | 'a' => self.repr_of(v).map(Value::str),
                        'e' | 'E' | 'f' | 'F' | 'g' | 'G' => self.call_value(&crate::builtins::get("float").unwrap_or(Value::Builtin("float")), vec![v.clone()], Vec::new()),
                        _ => self.call_value(&crate::builtins::get("int").unwrap_or(Value::Builtin("int")), vec![v.clone()], Vec::new()),
                    })?;
                    stack.push(Slot::Val(Value::str(text)));
                } else {
                    stack.push(Slot::Val(binary(op, &a, &b, inplace)?));
                }
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
            Op::Locals => {
                let vars = locals.vars.borrow();
                let mut d = Dict::new();
                for p in code.params.iter().chain(code.vararg.iter()).chain(code.kwonly.iter()).chain(code.kwarg.iter()) {
                    if let Some(v) = vars.get(p) {
                        d.set(Value::str(p.clone()), v.clone())?;
                    }
                }
                let mut rest: Vec<(&String, &Value)> = vars
                    .iter()
                    .filter(|(k, _)| {
                        !code.params.contains(k)
                            && code.vararg.as_ref() != Some(*k)
                            && !code.kwonly.contains(k)
                            && code.kwarg.as_ref() != Some(*k)
                    })
                    .collect();
                rest.sort_by(|a, b| a.0.cmp(b.0));
                for (k, v) in rest {
                    d.set(Value::str(k.clone()), v.clone())?;
                }
                stack.push(Slot::Val(Value::dict(d)));
            }
            Op::LoadLocal(i) => {
                let name = &code.names[i as usize];
                let found = locals.vars.borrow().get(name).cloned();
                match found {
                    Some(v) => stack.push(Slot::Val(v)),
                    // Corpo de classe: o nome ainda não ligado cai para os escopos de fora.
                    None if locals.is_class => {
                        let mut found = None;
                        let mut cur = locals.parent.clone();
                        while let Some(env) = cur {
                            if let Some(v) = env.vars.borrow().get(name) {
                                found = Some(v.clone());
                                break;
                            }
                            cur = env.parent.clone();
                        }
                        let v = match found {
                            Some(v) => v,
                            None => self.global_or_builtin(name)?,
                        };
                        stack.push(Slot::Val(v));
                    }
                    None => {
                        return Err(exc(
                            "UnboundLocalError",
                            format!("cannot access local variable '{name}' where it is not associated with a value"),
                        ))
                    }
                }
            }
            Op::Annotate(i) => {
                let ann = pop(stack)?;
                let name = code.names[i as usize].clone();
                let existing = if locals.is_module {
                    self.globals.borrow().get("__annotations__").cloned()
                } else {
                    locals.vars.borrow().get("__annotations__").cloned()
                };
                let dict = match existing {
                    Some(Value::Dict(d)) => d,
                    _ => {
                        let d = Value::dict(crate::object::Dict::new());
                        if locals.is_module {
                            self.globals.borrow_mut().insert("__annotations__".to_string(), d.clone());
                        } else {
                            locals.set("__annotations__", d.clone());
                        }
                        match d {
                            Value::Dict(d) => d,
                            _ => return Err(internal("annotations dict")),
                        }
                    }
                };
                dict.borrow_mut().set(Value::str(name), ann)?;
            }
            Op::StoreLocal(i) => {
                let v = pop(stack)?;
                locals.set(&code.names[i as usize], v);
            }
            Op::MakeFunction { code: idx, ndefaults, kwdefaults } => {
                let kw_names: Vec<String> = match kwdefaults {
                    Some(c) => match &code.consts[c as usize] {
                        Value::Tuple(t) => t.iter().map(to_str).collect(),
                        _ => Vec::new(),
                    },
                    None => Vec::new(),
                };
                let kw_values = pop_n(stack, kw_names.len())?;
                let defaults = pop_n(stack, ndefaults as usize)?;
                let f = FuncObj {
                    code: code.functions[idx as usize].clone(),
                    defaults,
                    kwdefaults: kw_names.into_iter().zip(kw_values).collect(),
                    closure: locals.capture(),
                    globals: self.globals.clone(),
                    attrs: RefCell::new(std::collections::BTreeMap::new()),
                };
                stack.push(Slot::Val(Value::Function(Rc::new(f))));
            }
            Op::SetAnnotations(c) => {
                let names: Vec<String> = match &code.consts[c as usize] {
                    Value::Tuple(t) => t.iter().map(to_str).collect(),
                    _ => Vec::new(),
                };
                let values = pop_n(stack, names.len())?;
                let mut d = crate::object::Dict::new();
                for (k, v) in names.into_iter().zip(values) {
                    d.set(Value::str(k), v)?;
                }
                if let Some(Slot::Val(Value::Function(f))) = stack.last() {
                    f.attrs.borrow_mut().insert("__annotations__".to_string(), Value::dict(d));
                }
            }
            Op::PushExc => {
                let v = top(stack)?.clone();
                self.handled.borrow_mut().push(v);
            }
            Op::PopExc => {
                self.handled.borrow_mut().pop();
            }
            Op::ExcMatch => {
                let cls = pop(stack)?;
                let exc_value = top(stack)?.clone();
                let matched = self.exc_matches_value(&exc_value, &cls)?;
                stack.push(Slot::Val(Value::Bool(matched)));
            }
            Op::Raise => {
                let v = pop(stack)?;
                let mut e = self.raise_any(v)?;
                self.link_context(&mut e);
                return Err(e);
            }
            Op::RaiseFrom => {
                let cause = pop(stack)?;
                let v = pop(stack)?;
                let mut e = self.raise_any(v)?;
                let cause = match cause {
                    Value::Class(_) => {
                        let pe = self.raise_any(cause)?;
                        pe.to_value()
                    }
                    other => other,
                };
                if !matches!(cause, Value::None | Value::Exception(_) | Value::Instance(_)) {
                    return Err(type_error("exception causes must derive from BaseException"));
                }
                let value = e.to_value();
                e.value = Some(value.clone());
                exc_set_cause(&value, cause);
                self.link_context(&mut e);
                return Err(e);
            }
            Op::ReraiseCurrent => {
                let last = self.handled.borrow().last().cloned();
                let Some(v) = last else {
                    return Err(exc("RuntimeError", "No active exception to reraise"));
                };
                return Err(PyException::reraised(&v));
            }
            Op::Reraise => {
                let v = pop(stack)?;
                self.handled.borrow_mut().pop();
                return Err(PyException::reraised(&v));
            }
            Op::DeleteName(i) => {
                let name = &code.names[i as usize];
                if locals.is_module {
                    self.globals.borrow_mut().remove(name);
                } else {
                    locals.vars.borrow_mut().remove(name);
                }
            }
            Op::DeleteLocal(i) => {
                let name = &code.names[i as usize];
                if locals.vars.borrow_mut().remove(name).is_none() {
                    return Err(exc(
                        "UnboundLocalError",
                        format!("cannot access local variable '{name}' where it is not associated with a value"),
                    ));
                }
            }
            Op::DeleteGlobal(i) => {
                let name = &code.names[i as usize];
                if self.globals.borrow_mut().remove(name).is_none() {
                    return Err(exc("NameError", format!("name '{name}' is not defined")));
                }
            }
            Op::CallEx { kwargs } => {
                let kw = if kwargs { Some(pop(stack)?) } else { None };
                let args = pop(stack)?;
                let func = pop(stack)?;
                let positional = collect(&args)?;
                let mut named: Vec<(String, Value)> = Vec::new();
                if let Some(Value::Dict(d)) = kw {
                    for (k, v) in d.borrow().iter() {
                        let Value::Str(s) = k else {
                            return Err(type_error("keywords must be strings"));
                        };
                        named.push((s.as_str().to_string(), v.clone()));
                    }
                }
                let result = self.call(&func, positional, named)?;
                stack.push(Slot::Val(result));
            }
            Op::ListAppend => {
                let item = pop(stack)?;
                match top(stack)? {
                    Value::List(l) => l.borrow_mut().push(item),
                    _ => return Err(internal("ListAppend without list")),
                }
            }
            Op::ListExtend => {
                let it = pop(stack)?;
                let items = collect(&it)?;
                match top(stack)? {
                    Value::List(l) => l.borrow_mut().extend(items),
                    _ => return Err(internal("ListExtend without list")),
                }
            }
            Op::ListToTuple => {
                let v = pop(stack)?;
                match v {
                    Value::List(l) => stack.push(Slot::Val(Value::tuple(l.borrow().clone()))),
                    _ => return Err(internal("ListToTuple without list")),
                }
            }
            Op::ListToSet => {
                let v = pop(stack)?;
                let mut set = Set::new();
                for item in collect(&v)? {
                    set.add(item)?;
                }
                stack.push(Slot::Val(Value::set(set)));
            }
            Op::DictSet => {
                let value = pop(stack)?;
                let key = pop(stack)?;
                match top(stack)? {
                    Value::Dict(d) => d.borrow_mut().set(key, value)?,
                    _ => return Err(internal("DictSet without dict")),
                }
            }
            Op::DictUpdate => {
                let m = pop(stack)?;
                let Some(pairs) = mapping_pairs(&m)? else {
                    return Err(type_error(format!("'{}' object is not a mapping", m.type_name())));
                };
                match top(stack)? {
                    Value::Dict(d) => {
                        for (k, v) in pairs {
                            d.borrow_mut().set(k, v)?;
                        }
                    }
                    _ => return Err(internal("DictUpdate without dict")),
                }
            }
            Op::ListAppendAt(d) | Op::SetAddAt(d) => {
                let item = pop(stack)?;
                let idx = stack.len().checked_sub(1 + d as usize).ok_or_else(|| internal("bad value stack"))?;
                match (&stack[idx], op) {
                    (Slot::Val(Value::List(l)), Op::ListAppendAt(_)) => l.borrow_mut().push(item),
                    (Slot::Val(Value::Set(s)), Op::SetAddAt(_)) => s.borrow_mut().add(item)?,
                    _ => return Err(internal("bad comprehension accumulator")),
                }
            }
            Op::MapAddAt(d) => {
                let value = pop(stack)?;
                let key = pop(stack)?;
                let idx = stack.len().checked_sub(1 + d as usize).ok_or_else(|| internal("bad value stack"))?;
                match &stack[idx] {
                    Slot::Val(Value::Dict(m)) => m.borrow_mut().set(key, value)?,
                    _ => return Err(internal("bad comprehension accumulator")),
                }
            }
            Op::BuildSlice => {
                let step = pop(stack)?;
                let hi = pop(stack)?;
                let lo = pop(stack)?;
                stack.push(Slot::Val(Value::Slice(Rc::new((lo, hi, step)))));
            }
            Op::BuildString(n) => {
                let parts = pop_n(stack, n as usize)?;
                let mut out = String::new();
                for p in &parts {
                    out.push_str(&to_str(p));
                }
                stack.push(Slot::Val(Value::str(out)));
            }
            Op::FormatValue { conv, has_spec } => {
                let spec = if has_spec { Some(pop(stack)?) } else { None };
                let v = pop(stack)?;
                let v = match conv {
                    1 => Value::str(self.str_of(&v)?),
                    2 => Value::str(self.repr_of(&v)?),
                    3 => Value::str(crate::format::ascii_repr(&v)),
                    _ => v,
                };
                let spec = match &spec {
                    Some(Value::Str(s)) => s.as_str().to_string(),
                    _ => String::new(),
                };
                stack.push(Slot::Val(Value::str(self.format_value(&v, &spec)?)));
            }
            Op::StoreAttr(i) => {
                let obj = pop(stack)?;
                let value = pop(stack)?;
                self.store_attr(&obj, &code.names[i as usize], value)?;
            }
            Op::DeleteAttr(i) => {
                let obj = pop(stack)?;
                self.delete_attr(&obj, &code.names[i as usize])?;
            }
            Op::DeleteSubscript => {
                let index = pop(stack)?;
                let container = pop(stack)?;
                self.delete_subscript(&container, &index)?;
            }
            Op::UnpackEx { before, after } => {
                let v = pop(stack)?;
                let items = collect(&v)?;
                let (b, a) = (before as usize, after as usize);
                if items.len() < b + a {
                    return Err(exc(
                        "ValueError",
                        format!("not enough values to unpack (expected at least {}, got {})", b + a, items.len()),
                    ));
                }
                let middle: Vec<Value> = items[b..items.len() - a].to_vec();
                // O primeiro alvo fica no topo: empilha de trás para frente.
                for x in items[items.len() - a..].iter().rev() {
                    stack.push(Slot::Val(x.clone()));
                }
                stack.push(Slot::Val(Value::list(middle)));
                for x in items[..b].iter().rev() {
                    stack.push(Slot::Val(x.clone()));
                }
            }
            Op::WithEnter => {
                let mgr = pop(stack)?;
                let (exit, entered) = self.with_enter(&mgr)?;
                stack.push(Slot::Val(exit));
                stack.push(Slot::Val(entered));
            }
            Op::WithExcept => {
                let exit = pop(stack)?;
                let exc_value = top(stack)?.clone();
                let ty = self.type_of(&exc_value);
                let tb = match &exc_value {
                    Value::Exception(x) => x.traceback.borrow().clone(),
                    Value::Instance(i) => i.dict.borrow().get("__traceback__").cloned(),
                    _ => None,
                }
                .unwrap_or(Value::None);
                let r = self.call(&exit, vec![ty, exc_value, tb], Vec::new())?;
                stack.push(Slot::Val(Value::Bool(r.is_true())));
            }
            Op::BuildClass { code: idx, nbases, kwnames } => {
                let kw_values = match kwnames {
                    Some(k) => match &code.consts[k as usize] {
                        Value::Tuple(names) => pop_n(stack, names.len())?
                            .into_iter()
                            .zip(names.iter())
                            .map(|(v, n)| (to_str(n), v))
                            .collect(),
                        _ => Vec::new(),
                    },
                    None => Vec::new(),
                };
                let bases = pop_n(stack, nbases as usize)?;
                let body = code.functions[idx as usize].clone();
                let cls = self.build_class(&body, bases, kw_values, locals)?;
                stack.push(Slot::Val(cls));
            }
            Op::Yield | Op::AsyncGenYield => return Err(internal("yield outside run loop")),
            Op::DelegateNext(t) => return Ok(Some(t as usize)),
            Op::GetAwaitable => {
                let v = pop(stack)?;
                let a = self.get_awaitable(&v)?;
                stack.push(Slot::Val(a));
            }
            Op::Delegate(end) => {
                let sent = pop(stack)?;
                let step = match stack.last_mut() {
                    Some(slot) => self.delegate_step(slot, sent)?,
                    None => return Err(internal("delegate without iterator")),
                };
                match step {
                    Step::Yield(v) => stack.push(Slot::Val(v)),
                    Step::Done(v) => {
                        stack.pop();
                        stack.push(Slot::Val(v));
                        return Ok(Some(end as usize));
                    }
                }
            }
            Op::GetAIter => {
                let v = pop(stack)?;
                let it = match &v {
                    Value::Ext(e) if e.type_name() == "async_generator" => v.clone(),
                    _ => match self.call_dunder(&v, "__aiter__", Vec::new()) {
                        Some(r) => r?,
                        None => {
                            return Err(type_error(format!(
                                "'async for' requires an object with __aiter__ method, got {}",
                                v.type_name()
                            )))
                        }
                    },
                };
                stack.push(Slot::Val(it));
            }
            Op::GetANext => {
                let it = top(stack)?.clone();
                let next = match &it {
                    Value::Ext(e) if e.type_name() == "async_generator" => {
                        e.clone().call_method(self, "__anext__", Vec::new(), Vec::new())?
                    }
                    _ => match self.call_dunder(&it, "__anext__", Vec::new()) {
                        Some(r) => r?,
                        None => {
                            return Err(type_error(format!(
                                "'async for' received an object from __aiter__ that does not implement __anext__: {}",
                                it.type_name()
                            )))
                        }
                    },
                };
                stack.push(Slot::Val(next));
            }
            Op::AsyncForExcept(target) => {
                let e = pop(stack)?;
                let is_stop = matches!(&e, Value::Exception(x) if x.kind == "StopAsyncIteration")
                    || matches!(&e, Value::Instance(i) if i.class.mro().iter().any(|c| c.name == "StopAsyncIteration"));
                if is_stop {
                    stack.pop();
                    return Ok(Some(target as usize));
                }
                return Err(PyException::from_value(&e));
            }
            Op::AsyncWithEnter => {
                let mgr = pop(stack)?;
                let missing = |what: &str| {
                    type_error(format!("'{}' object does not support the asynchronous context manager protocol{what}", mgr.type_name()))
                };
                let (Some(enter), Some(exit)) = (self.attr_of_type(&mgr, "__aenter__"), self.attr_of_type(&mgr, "__aexit__")) else {
                    return Err(missing(""));
                };
                let entered = self.call_value(&enter, Vec::new(), Vec::new())?;
                stack.push(Slot::Val(exit));
                stack.push(Slot::Val(entered));
            }
            Op::AsyncWithExceptCall => {
                let exit = pop(stack)?;
                let exc_value = top(stack)?.clone();
                let ty = self.type_of(&exc_value);
                let r = self.call(&exit, vec![ty, exc_value, Value::None], Vec::new())?;
                stack.push(Slot::Val(r));
            }
            Op::Import(i) => {
                let name = &code.names[i as usize];
                let m = crate::modules::import_checked(self, name)?;
                stack.push(Slot::Val(Value::Module(m)));
            }
            Op::ImportRel { name, level } => {
                let rel = &code.names[name as usize];
                let abs = crate::modules::resolve_relative(self, rel, level as usize)?;
                let m = crate::modules::import_checked(self, &abs)?;
                stack.push(Slot::Val(Value::Module(m)));
            }
            Op::ImportStar => {
                let Value::Module(m) = pop(stack)? else {
                    return Err(internal("import * from a non-module"));
                };
                let mut attrs = m.attrs.borrow().clone();
                if let Some(g) = self.module_globals.borrow().get(m.name) {
                    attrs.extend(g.borrow().iter().map(|(k, v)| (k.clone(), v.clone())));
                }
                let listed: Option<Vec<String>> = match attrs.get("__all__") {
                    Some(Value::List(l)) => Some(l.borrow().iter().map(|v| to_str(v)).collect()),
                    Some(Value::Tuple(t)) => Some(t.iter().map(|v| to_str(v)).collect()),
                    _ => None,
                };
                let mut globals = self.globals.borrow_mut();
                match listed {
                    Some(names) => {
                        for n in names {
                            let Some(v) = attrs.get(&n) else {
                                return Err(exc(
                                    "AttributeError",
                                    format!("module '{}' has no attribute '{n}'", m.name),
                                ));
                            };
                            globals.insert(n, v.clone());
                        }
                    }
                    None => {
                        for (n, v) in attrs {
                            if !n.starts_with('_') {
                                globals.insert(n, v);
                            }
                        }
                    }
                }
            }
            Op::ImportName(i) => {
                let obj = pop(stack)?;
                let name = &code.names[i as usize];
                match self.load_attr(&obj, name) {
                    Ok(v) => stack.push(Slot::Val(v)),
                    Err(_) => {
                        let module = match &obj {
                            Value::Module(m) => m.name,
                            _ => "?",
                        };
                        // `from pacote import submodulo`: importa o submódulo.
                        if let Value::Module(m) = &obj {
                            if m.attrs.borrow().contains_key("__path__")
                                || self.module_globals.borrow().get(m.name).is_some_and(|g| g.borrow().contains_key("__path__"))
                                || crate::modules::is_embedded_package(m.name)
                            {
                                let full = format!("{module}.{name}");
                                if let Ok(sub) = crate::modules::import_checked(self, &full) {
                                    stack.push(Slot::Val(Value::Module(sub)));
                                    return Ok(None);
                                }
                            }
                        }
                        let location = match self.load_attr(&obj, "__file__") {
                            Ok(Value::Str(p)) => p.as_str().to_string(),
                            _ => "unknown location".to_string(),
                        };
                        let msg = format!("cannot import name '{name}' from '{module}' ({location})");
                        let x = ExcObj::new("ImportError", vec![Value::str(msg.clone())]);
                        {
                            let mut extra = x.extra.borrow_mut();
                            extra.push(("name", Value::str(module.to_string())));
                            extra.push(("name_from", Value::str(name.clone())));
                            extra.push(("module", obj.clone()));
                        }
                        let mut e = exc("ImportError", msg);
                        e.value = Some(Value::Exception(Rc::new(x)));
                        return Err(e);
                    }
                }
            }
            Op::LoadAttr(i) => {
                let obj = pop(stack)?;
                let name = &code.names[i as usize];
                let v = self.load_attr(&obj, name).map_err(|e| tag_attribute_error(e, &obj, name))?;
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

    pub(crate) fn call(&mut self, func: &Value, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
        if let Value::Function(f) = func {
            return self.call_function(f, args, kwargs);
        }
        match func {
            Value::BoundFn(b) => {
                let mut full = Vec::with_capacity(args.len() + 1);
                full.push(b.0.clone());
                full.extend(args);
                return self.call_function(&b.1, full, kwargs);
            }
            Value::Class(c) => return self.instantiate(c, args, kwargs),
            Value::Instance(i) => {
                return match i.class.lookup("__call__") {
                    Some(Value::Function(f)) => {
                        let mut full = Vec::with_capacity(args.len() + 1);
                        full.push(func.clone());
                        full.extend(args);
                        self.call_function(&f, full, kwargs)
                    }
                    _ => Err(type_error(format!("'{}' object is not callable", func.type_name()))),
                };
            }
            Value::Ext(e) if e.methods().contains(&"__call__") => {
                let e = e.clone();
                return e.call_method(self, "__call__", args, kwargs);
            }
            Value::Builtin("method") => {
                return match args.as_slice() {
                    [recv, Value::Function(f)] => Ok(Value::BoundFn(Rc::new((recv.clone(), f.clone())))),
                    _ => Err(type_error("method expected 2 arguments, got a different shape")),
                };
            }
            Value::Builtin(name @ ("staticmethod" | "classmethod" | "property" | "super" | "type" | "object")) => {
                return self.call_class_builtin(name, args, kwargs);
            }
            _ => {}
        }
        if let Value::Bound(b) = func {
            if let Value::Ext(e) = &b.recv {
                let e = e.clone();
                return e.call_method(self, b.name, args, kwargs);
            }
            if !matches!(b.recv, Value::Native(_)) {
                let Some((_, f)) = crate::methods::lookup(&b.recv, b.name) else {
                    return Err(internal("bound method without table entry"));
                };
                let mut full = Vec::with_capacity(args.len() + 1);
                full.push(b.recv.clone());
                full.extend(args);
                return f(self, full, kwargs);
            }
            return self.call_method(&b.recv, b.name, args, kwargs);
        }
        if let Value::NativeFn(n) = func {
            return (n.f)(self, args, kwargs);
        }
        let Value::Builtin(name) = func else {
            return Err(type_error(format!("'{}' object is not callable", func.type_name())));
        };
        let name = *name;
        if let Some((kind, _)) = EXC_CLASSES.iter().find(|(n, _)| *n == name) {
            if let Some((kw, _)) = kwargs.first() {
                return Err(type_error(format!("{name}() takes no keyword arguments ('{kw}' given)")));
            }
            return Ok(Value::Exception(Rc::new(ExcObj::new(kind, args))));
        }
        if !matches!(name, "print" | "open" | "csv.reader" | "csv.writer" | "json.dumps" | "sorted" | "enumerate" | "module")
            && let Some((kw, _)) = kwargs.first() {
                return Err(type_error(match name {
                    "range" | "len" | "repr" | "json.loads" => format!("{name}() takes no keyword arguments"),
                    _ => format!("'{kw}' is an invalid keyword argument for {name}()"),
                }));
            }
        match name {
            "module" => crate::modules::new_module(self, args, kwargs),
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
            // `ValueError.__init__(self, ...)` chamado à mão por uma subclasse.
            "BaseException.__init__" => {
                if let Some(Value::Instance(inst)) = args.first() {
                    if inst.class.builtin_base.is_some() {
                        inst.dict.borrow_mut().insert("args".to_string(), Value::tuple(args[1..].to_vec()));
                    }
                }
                Ok(Value::None)
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
    /// `sys.stdout` quando o programa o trocou (por `redirect_stdout`, por exemplo); `None` enquanto
    /// ele ainda é o fluxo padrão.
    fn redirected_stdout(&self) -> Option<Value> {
        let sys = self.modules.borrow().get("sys").cloned()?;
        let current = sys.attrs.borrow().get("stdout").cloned()?;
        match &current {
            Value::Native(n) if Rc::ptr_eq(n, &self.std_files[1]) => None,
            _ => Some(current),
        }
    }

    /// A exceção que está sendo tratada agora (`sys.exc_info()`).
    pub(crate) fn handled_top(&self) -> Option<Value> {
        self.handled.borrow().last().cloned()
    }

    /// Acrescenta ao buffer do stdout com a política do CPython: num terminal, descarrega a cada
    /// quebra de linha; num pipe ou arquivo, em blocos de 8 KiB. O resto sai no `flush` ou no fim.
    pub(crate) fn push_stdout(&self, data: &[u8]) {
        self.stdout.borrow_mut().extend_from_slice(data);
        let Some(sys) = sysabi::sys::try_current() else { return };
        thread_local! {
            static STDOUT_TTY: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
        }
        let tty = STDOUT_TTY.with(|c| match c.get() {
            Some(t) => t,
            None => {
                let t = sys.isatty(sysabi::Fd::STDOUT);
                c.set(Some(t));
                t
            }
        });
        if tty {
            if data.contains(&b'\n') {
                self.flush_stdout();
            }
            return;
        }
        const BLOCK: usize = 8192;
        let mut buf = self.stdout.borrow_mut();
        if buf.len() >= BLOCK {
            let n = buf.len() - buf.len() % BLOCK;
            let _ = sysabi::sys::write_all(sysabi::Fd::STDOUT, &buf[..n]);
            buf.drain(..n);
        }
    }

    /// Descarrega o stdout pendente (`print(flush=True)`, `sys.stdout.flush()`). Sem pseudo-processo
    /// (os testes de unidade), o buffer fica como está para o chamador ler.
    pub(crate) fn flush_stdout(&self) {
        if sysabi::sys::try_current().is_none() {
            return;
        }
        let mut buf = self.stdout.borrow_mut();
        if !buf.is_empty() {
            let _ = sysabi::sys::write_all(sysabi::Fd::STDOUT, &buf);
            buf.clear();
        }
    }

    fn print(&mut self, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
        let mut sep = " ".to_string();
        let mut end = "\n".to_string();
        let mut file: Option<Value> = None;
        let mut flush = false;
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
                "flush" => flush = value.is_true(),
                _ => return Err(type_error(format!("print() got an unexpected keyword argument '{name}'"))),
            }
        }
        // `print` escreve argumento por argumento: um `__str__` que falha deixa na saída o que veio antes dele.
        let mut parts: Vec<String> = Vec::with_capacity(args.len());
        let mut failure = None;
        for a in &args {
            parts.push(to_str(a));
            if let Some(e) = take_text_error() {
                parts.pop();
                failure = Some(e);
                break;
            }
        }
        let text = if failure.is_some() {
            let mut t = parts.join(&sep);
            if !parts.is_empty() {
                t.push_str(&sep);
            }
            t
        } else {
            format!("{}{}", parts.join(&sep), end)
        };
        match file {
            Some(f) => {
                self.write_to(&f, &text)?;
                if flush {
                    let m = self.getattr(&f, "flush")?;
                    self.call_value(&m, Vec::new(), Vec::new())?;
                }
            }
            None => match self.redirected_stdout() {
                Some(f) => {
                    self.write_to(&f, &text)?;
                }
                None => {
                    self.push_stdout(text.as_bytes());
                    if flush {
                        self.flush_stdout();
                    }
                }
            },
        }
        if let Some(e) = failure {
            return Err(e);
        }
        Ok(Value::None)
    }

    /// `arquivo.write(texto)`: devolve a quantidade de caracteres.
    fn write_to(&mut self, target: &Value, text: &str) -> PyResult<usize> {
        let Value::Native(n) = target else {
            // Objeto de arquivo escrito em Python (`io.TextIOWrapper`, `StringIO`...): chama `write`.
            let write = self.load_attr(target, "write")?;
            self.call(&write, vec![Value::str(text)], Vec::new())?;
            return Ok(text.chars().count());
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
            FileKind::Stdout => self.push_stdout(text.as_bytes()),
            FileKind::Stderr => {
                // O stderr do CPython é sem buffer e independe do stdout: com stdout em pipe, o que está
                // pendente só sai no fim (ou a cada 8 KiB), depois do que o stderr já escreveu.
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
            Value::Instance(inst) => return self.instance_getattr(obj, inst, name),
            Value::Class(c) => return self.class_getattr(c, name),
            Value::Builtin(_) | Value::NativeFn(_) if name == "__dict__" && crate::builtins::class_name(obj).is_some() => {
                let mut d = crate::object::Dict::new();
                for (k, v) in crate::builtins_ext::probe_type_attrs(self, obj) {
                    d.set(Value::str(k), v)?;
                }
                return Ok(Value::dict(d));
            }
            Value::Builtin(_) | Value::NativeFn(_)
                if matches!(name, "__getattribute__" | "__setattr__" | "__delattr__")
                    && crate::builtins::class_name(obj).is_some() =>
            {
                // `tuple.__getattribute__(self, nome)` e companhia: os ganchos de atributo vêm de `object`.
                if let Some(v) = crate::typeattrs::object_attr(name) {
                    return Ok(v);
                }
            }
            Value::Builtin("object")
                if !matches!(name, "__name__" | "__qualname__" | "__mro__" | "__bases__" | "__module__") =>
            {
                if let Some(v) = crate::typeattrs::object_attr(name) {
                    return Ok(v);
                }
            }
            Value::Builtin(n) if name == "__init__" && EXC_CLASSES.iter().any(|(k, _)| k == n) => {
                return Ok(Value::Builtin("BaseException.__init__"));
            }
            Value::Builtin(n) if name == "__name__" || name == "__qualname__" => {
                return Ok(Value::str(n.rsplit('.').next().unwrap_or(n)))
            }
            Value::Builtin(_) | Value::NativeFn(_)
                if matches!(name, "__mro__" | "__bases__") && crate::builtins::class_name(obj).is_some() =>
            {
                let n = &crate::builtins::class_name(obj).unwrap_or("object");
                // Cadeia de bases: exceções pelo mapa de pais, `bool` -> `int`, e `object` no fim.
                let cls_of = |s: &'static str| crate::builtins::get(s).unwrap_or(Value::Builtin(s));
                let mut chain = vec![obj.clone()];
                let mut cur: &'static str = n;
                while let Some((_, parent)) = EXC_CLASSES.iter().find(|(e, _)| *e == cur) {
                    if parent.is_empty() {
                        break;
                    }
                    chain.push(cls_of(parent));
                    cur = parent;
                }
                if *n == "bool" {
                    chain.push(cls_of("int"));
                }
                if *n != "object" {
                    chain.push(cls_of("object"));
                }
                if name == "__bases__" {
                    let bases = if *n == "object" { Vec::new() } else { chain[1..2].to_vec() };
                    return Ok(Value::tuple(bases));
                }
                return Ok(Value::tuple(chain));
            }
            Value::Builtin(_) if name == "__module__" => return Ok(Value::str("builtins")),
            Value::Builtin(_) | Value::NativeFn(_) if name == "__doc__" => return Ok(Value::None),
            Value::NativeFn(f) => {
                if let Some(v) = crate::typeattrs::type_attr(f.name, name) {
                    return Ok(v);
                }
                if matches!(name, "__name__" | "__qualname__") {
                    return Ok(Value::str(f.name.rsplit('.').next().unwrap_or(f.name)));
                }
            }
            Value::Exception(e) if name == "__class__" => return Ok(Value::Builtin(e.kind)),
            Value::Function(f) => {
                if let Some(v) = f.attrs.borrow().get(name) {
                    return Ok(v.clone());
                }
                match name {
                    "__name__" => return Ok(Value::str(f.code.name.clone())),
                    "__qualname__" => return Ok(Value::str(f.code.qual())),
                    "__defaults__" => {
                        return Ok(if f.defaults.is_empty() { Value::None } else { Value::tuple(f.defaults.clone()) })
                    }
                    "__kwdefaults__" => {
                        if f.kwdefaults.is_empty() {
                            return Ok(Value::None);
                        }
                        let mut d = crate::object::Dict::new();
                        for (k, v) in &f.kwdefaults {
                            d.set(Value::str(k.clone()), v.clone())?;
                        }
                        return Ok(Value::dict(d));
                    }
                    "__doc__" => return Ok(f.code.doc.clone().map_or(Value::None, Value::str)),
                    "__annotations__" => {
                        let d = Value::dict(crate::object::Dict::new());
                        f.attrs.borrow_mut().insert("__annotations__".to_string(), d.clone());
                        return Ok(d);
                    }
                    "__code__" => {
                        let file = if f.code.filename.is_empty() {
                            self.script_name()
                        } else {
                            f.code.filename.clone()
                        };
                        return Ok(crate::tbobj::function_code(&f.code, &file));
                    }
                    "__module__" => {
                        let name = f.globals.borrow().get("__name__").cloned();
                        return Ok(name.unwrap_or_else(|| Value::str("__main__")));
                    }
                    "__dict__" => {
                        let mut d = crate::object::Dict::new();
                        for (k, v) in f.attrs.borrow().iter() {
                            d.set(Value::str(k.clone()), v.clone())?;
                        }
                        return Ok(Value::dict(d));
                    }
                    _ => {}
                }
            }
            Value::BoundFn(b) if b.1.attrs.borrow().contains_key(name) => {
                return Ok(b.1.attrs.borrow().get(name).cloned().unwrap_or(Value::None))
            }
            Value::Bound(b) => match name {
                "__self__" => return Ok(b.recv.clone()),
                "__name__" | "__qualname__" => return Ok(Value::str(b.name)),
                _ => {}
            },
            Value::BoundFn(b) => match name {
                "__doc__" | "__module__" | "__qualname__" | "__code__" | "__dict__" => {
                    return self.getattr(&Value::Function(b.1.clone()), name)
                }
                "__name__" => return Ok(Value::str(b.1.code.name.clone())),
                "__self__" => return Ok(b.0.clone()),
                "__func__" => return Ok(Value::Function(b.1.clone())),
                _ => {}
            },
            Value::Slice(s) => match name {
                "start" => return Ok(s.0.clone()),
                "stop" => return Ok(s.1.clone()),
                "step" => return Ok(s.2.clone()),
                _ => {}
            },
            _ => {}
        }
        match obj {
            Value::Exception(e) if name == "args" => {
                let n = if e.args.len() > 2 && matches!(e.args[0], Value::Int(_)) && exc_is_subclass(&e.kind, "OSError") { 2 } else { e.args.len() };
                Ok(Value::tuple(e.args[..n].to_vec()))
            }
            Value::Exception(_) if name == "with_traceback" => {
                Ok(Value::Ext(Rc::new(crate::classes::ExcWithTraceback { obj: obj.clone() })))
            }
            Value::Exception(e) if name == "__traceback__" => Ok(e.traceback.borrow().clone().unwrap_or(Value::None)),
            Value::Exception(e) if name == "code" && e.kind == "SystemExit" => Ok(match e.args.as_slice() {
                [] => Value::None,
                [one] => one.clone(),
                many => Value::tuple(many.to_vec()),
            }),
            Value::Exception(e) if name == "value" && e.kind == "StopIteration" => {
                Ok(e.args.first().cloned().unwrap_or(Value::None))
            }
            // `ImportError.name`/`.path`: o nome vem da mensagem que o import monta.
            Value::Exception(e) if matches!(name, "name" | "path") && exc_is_subclass(&e.kind, "ImportError") => {
                let msg = exc_str(e);
                let quoted = |after: &str| msg.split_once(after).and_then(|(_, r)| r.split('\'').next()).map(str::to_string);
                Ok(match name {
                    "name" => quoted("No module named '")
                        .or_else(|| msg.split_once("' from '").and_then(|(_, r)| r.split('\'').next()).map(str::to_string))
                        .map_or(Value::None, Value::str),
                    _ => Value::None,
                })
            }
            // `name`/`obj` de AttributeError, `name` de NameError, `name`/`path`/`name_from` de ImportError.
            Value::Exception(e)
                if matches!(name, "name" | "obj" | "name_from" | "path")
                    && ((matches!(name, "name" | "obj") && exc_is_subclass(e.kind, "AttributeError"))
                        || (name == "name" && exc_is_subclass(e.kind, "NameError"))
                        || (matches!(name, "name" | "path" | "name_from") && exc_is_subclass(e.kind, "ImportError"))) =>
            {
                Ok(e.extra_get(name).unwrap_or(Value::None))
            }
            Value::Exception(e) if exc_is_subclass(&e.kind, "SyntaxError") && matches!(
                name,
                "msg" | "filename" | "lineno" | "offset" | "text" | "end_lineno" | "end_offset" | "print_file_and_line"
            ) =>
            {
                let details = match e.args.get(1) {
                    Some(Value::Tuple(t)) => t.to_vec(),
                    _ => Vec::new(),
                };
                let at = |i: usize| details.get(i).cloned().unwrap_or(Value::None);
                Ok(match name {
                    "msg" => e.args.first().cloned().unwrap_or(Value::None),
                    "filename" => at(0),
                    "lineno" => at(1),
                    "offset" => at(2),
                    "text" => at(3),
                    "end_lineno" => at(4),
                    "end_offset" => at(5),
                    _ => Value::None,
                })
            }
            Value::Exception(e) if e.kind == "re.PatternError" && matches!(name, "msg" | "pattern" | "pos" | "lineno" | "colno") => {
                let at = |i: usize| e.args.get(i).cloned().unwrap_or(Value::None);
                Ok(match (name, at(2), at(3)) {
                    ("msg", _, _) => at(1),
                    ("pattern", p, _) => p,
                    ("pos", _, p) => p,
                    (_, Value::Str(p), Value::Int(pos)) => {
                        let upto: Vec<char> = p.as_str().chars().take(pos.max(0) as usize).collect();
                        match upto.iter().rposition(|c| *c == '\n') {
                            _ if name == "lineno" => Value::Int(upto.iter().filter(|c| **c == '\n').count() as i64 + 1),
                            Some(l) => Value::Int(pos - l as i64),
                            None => Value::Int(pos + 1),
                        }
                    }
                    _ => Value::None,
                })
            }
            Value::Exception(e) if matches!(name, "errno" | "strerror" | "filename") && exc_is_subclass(&e.kind, "OSError") => {
                let (errno, msg, file) = match e.args.as_slice() {
                    [errno, msg] => (errno.clone(), msg.clone(), Value::None),
                    [errno, msg, file, ..] => (errno.clone(), msg.clone(), file.clone()),
                    _ => (Value::None, Value::None, Value::None),
                };
                Ok(match name {
                    "errno" => errno,
                    "strerror" => msg,
                    _ => file,
                })
            }
            Value::Range(_) | Value::Builtin("Ellipsis") if matches!(name, "__reduce_ex__" | "__reduce__") => {
                let m = crate::modules::import_checked(self, "copyreg")?;
                match self.getattr(&Value::Module(m), "_builtin_reduce_ex")? {
                    Value::Function(f) => return Ok(Value::BoundFn(Rc::new((obj.clone(), f)))),
                    _ => return Err(missing()),
                }
            }
            Value::Range(r) if matches!(name, "start" | "stop" | "step") => Ok(Value::Int(match name {
                "start" => r.start,
                "stop" => r.stop,
                _ => r.step,
            })),
            Value::Int(_) | Value::Big(_) | Value::Bool(_) if matches!(name, "real" | "numerator") => {
                Ok(if let Value::Bool(b) = obj { Value::Int(i64::from(*b)) } else { obj.clone() })
            }
            Value::Int(_) | Value::Big(_) | Value::Bool(_) if name == "imag" => Ok(Value::Int(0)),
            Value::Int(_) | Value::Big(_) | Value::Bool(_) if name == "denominator" => Ok(Value::Int(1)),
            Value::Float(x) if name == "real" => Ok(Value::Float(*x)),
            Value::Float(_) if name == "imag" => Ok(Value::Float(0.0)),
            Value::Exception(_) if matches!(name, "__cause__" | "__context__" | "__suppress_context__") => {
                let (cause, context, suppress) = exc_chain(obj);
                Ok(match name {
                    "__cause__" => cause.unwrap_or(Value::None),
                    "__context__" => context.unwrap_or(Value::None),
                    _ => Value::Bool(suppress),
                })
            }
            Value::Str(_) | Value::Int(_) | Value::Big(_) | Value::Bool(_) | Value::Float(_) | Value::None
                | Value::Tuple(_) | Value::List(_) | Value::Dict(_) | Value::Bytes(_) | Value::Set(_)
                if name == "__doc__" =>
            {
                Ok(Value::None)
            }
            v if name == "__class__" && !matches!(v, Value::Instance(_) | Value::Class(_) | Value::Exception(_)) => {
                Ok(self.type_of(v))
            }
            Value::Module(m) => {
                if let Some(g) = self.module_globals.borrow().get(m.name) {
                    if let Some(v) = g.borrow().get(name) {
                        return Ok(v.clone());
                    }
                }
                if matches!(name, "__spec__" | "__loader__") {
                    if let Some(v) = self.module_spec(m, name)? {
                        return Ok(v);
                    }
                }
                if name == "__dict__" {
                    // Instantâneo dos atributos (os do módulo vivo valem mais que os copiados).
                    let mut all: std::collections::BTreeMap<String, Value> = m.attrs.borrow().clone();
                    if let Some(g) = self.module_globals.borrow().get(m.name) {
                        all.extend(g.borrow().iter().map(|(k, v)| (k.clone(), v.clone())));
                    }
                    let mut d = crate::object::Dict::new();
                    for (k, v) in all {
                        d.set(Value::str(k), v)?;
                    }
                    return Ok(crate::builtins_ext::module_dict_value(m.name, d));
                }
                match m.attrs.borrow().get(name) {
                    Some(v) => Ok(v.clone()),
                    None => Err(exc("AttributeError", format!("module '{}' has no attribute '{name}'", m.name))),
                }
            }
            Value::Native(n) => {
                let methods: &[&'static str] = match &*n.borrow() {
                    Native::File(f) => {
                        match name {
                            "buffer" if matches!(f.kind, FileKind::Stdin | FileKind::Stdout | FileKind::Stderr) => {
                                return Ok(crate::stdbuf::StdBuffer::value(n, f.kind));
                            }
                            "encoding" => return Ok(Value::str("utf-8")),
                            "errors" => return Ok(Value::str("surrogateescape")),
                            "newlines" => return Ok(Value::None),
                            "line_buffering" | "write_through" => return Ok(Value::Bool(false)),
                            "mode" => {
                                return Ok(Value::str(if matches!(f.kind, FileKind::Stdin | FileKind::Read) { "r" } else { "w" }))
                            }
                            _ => {}
                        }
                        &[
                            "write", "read", "readline", "readlines", "close", "flush", "isatty", "fileno", "writelines",
                            "readable", "writable", "seekable", "reconfigure",
                        ]
                    }
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
            Value::Ext(e) => {
                if let Some(m) = e.methods().iter().find(|m| **m == name) {
                    return Ok(Value::Bound(Rc::new(BoundMethod { recv: obj.clone(), name: m })));
                }
                match e.clone().getattr(self, name) {
                    Some(r) => r,
                    None => Err(missing()),
                }
            }
            _ => match crate::methods::lookup(obj, name) {
                Some((n, _)) => Ok(Value::Bound(Rc::new(BoundMethod { recv: obj.clone(), name: n }))),
                None => Err(missing()),
            },
        }
    }

    fn call_method(
        &mut self,
        recv: &Value,
        name: &'static str,
        args: Vec<Value>,
        kwargs: Vec<(String, Value)>,
    ) -> PyResult<Value> {
        if name == "reconfigure" {
            return Ok(Value::None);
        }
        if let Some((kw, _)) = kwargs.first() {
            return Err(type_error(format!("{name}() takes no keyword arguments ('{kw}' given)")));
        }
        let Value::Native(n) = recv else { return Err(internal("bound method without native receiver")) };
        match name {
            "isatty" | "seekable" => Ok(Value::Bool(false)),
            "readable" | "writable" => {
                let kind = match &*n.borrow() {
                    Native::File(f) => Some(f.kind),
                    _ => None,
                };
                let reading = matches!(kind, Some(FileKind::Stdin | FileKind::Read));
                Ok(Value::Bool(if name == "readable" { reading } else { !reading }))
            }
            "fileno" => match &*n.borrow() {
                Native::File(f) => match f.kind {
                    FileKind::Stdin => Ok(Value::Int(0)),
                    FileKind::Stdout => Ok(Value::Int(1)),
                    FileKind::Stderr => Ok(Value::Int(2)),
                    FileKind::Read => Err(exc("UnsupportedOperation", "fileno")),
                },
                _ => Err(internal("fileno on non-file")),
            },
            "writelines" => {
                let [lines] = one_arg(name, args)?;
                for l in collect(&lines)? {
                    let Value::Str(s) = &l else {
                        return Err(type_error(format!("write() argument must be str, not {}", l.type_name())));
                    };
                    self.write_to(recv, s.as_str())?;
                }
                Ok(Value::None)
            }
            "write" => {
                let [v] = one_arg(name, args)?;
                let Value::Str(s) = &v else {
                    return Err(type_error(format!("write() argument must be str, not {}", v.type_name())));
                };
                Ok(Value::Int(self.write_to(recv, s.as_str())? as i64))
            }
            "flush" => {
                if matches!(&*n.borrow(), Native::File(f) if matches!(f.kind, FileKind::Stdout)) {
                    self.flush_stdout();
                }
                Ok(Value::None)
            }
            "close" => {
                if let Native::File(f) = &mut *n.borrow_mut() {
                    f.closed = true;
                }
                Ok(Value::None)
            }
            "read" => {
                // `read(n)` do stdin devolve até `n` caracteres, sem puxar o resto do pipe.
                let limit = match args.first() {
                    Some(Value::Int(k)) if *k >= 0 => Some(*k as usize),
                    _ => None,
                };
                if let (Some(k), Native::File(f)) = (limit, &mut *n.borrow_mut()) {
                    if f.kind == FileKind::Stdin && !f.closed {
                        return Ok(Value::str(crate::stdin::text_chars(f, k)));
                    }
                }
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

    /// `__spec__`/`__loader__` de um módulo carregado de arquivo: construídos na primeira leitura
    /// por `importlib.machinery` e guardados nas globais do módulo.
    fn module_spec(&mut self, m: &Rc<crate::object::ModuleObj>, name: &str) -> PyResult<Option<Value>> {
        let Some(globals) = self.module_globals.borrow().get(m.name).cloned() else { return Ok(None) };
        let (file, is_package) = {
            let g = globals.borrow();
            match g.get("__file__") {
                Some(Value::Str(f)) => (f.as_str().to_string(), g.contains_key("__path__")),
                _ => return Ok(None),
            }
        };
        let Some(machinery) = crate::modules::import(self, "importlib.machinery") else { return Ok(None) };
        let make = machinery.attrs.borrow().get("_spec_for_module").cloned();
        let Some(make) = make else { return Ok(None) };
        let spec = self.call(&make, vec![Value::str(m.name), Value::str(file), Value::Bool(is_package)], Vec::new())?;
        let loader = self.getattr(&spec, "loader")?;
        let mut g = globals.borrow_mut();
        g.insert("__spec__".to_string(), spec.clone());
        g.insert("__loader__".to_string(), loader.clone());
        Ok(Some(if name == "__spec__" { spec } else { loader }))
    }

    /// `open(...)`: o `io.open` (em Python embutido) sobre os descritores do sandbox.
    fn open(&mut self, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
        let Some(io) = crate::modules::import(self, "io") else {
            return Err(internal("io module missing"));
        };
        let open = io.attrs.borrow().get("open").cloned().ok_or_else(|| internal("io.open missing"))?;
        self.call(&open, args, kwargs)
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
            // Uma lista (ou qualquer iterável que não é arquivo) vira iterador agora: cada linha lida
            // avança o mesmo iterador, em vez de recomeçar do início a cada `next`.
            let src = match &target {
                Value::List(_) | Value::Tuple(_) | Value::Range(_) | Value::Dict(_) | Value::Set(_) => {
                    crate::builtins::make_iter(&target)?
                }
                _ => target,
            };
            Native::CsvReader { reader: csv::Reader::new(d), src }
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
pub(crate) fn file_readline_native(n: &Rc<RefCell<Native>>) -> PyResult<Option<String>> {
    file_readline(n)
}

fn file_readline(n: &Rc<RefCell<Native>>) -> PyResult<Option<String>> {
    let mut b = n.borrow_mut();
    let Native::File(f) = &mut *b else { return Err(type_error("not a file")) };
    if f.closed {
        return Err(exc("ValueError", "I/O operation on closed file."));
    }
    if f.kind == FileKind::Stdin {
        // Incremental: um pipe vivo entrega as linhas conforme chegam (ver `stdin.rs`).
        return Ok(crate::stdin::text_line(f));
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
pub(crate) fn native_next(n: &Rc<RefCell<Native>>) -> PyResult<Option<Value>> {
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
pub(crate) fn raise_value(v: Value) -> PyResult<PyException> {
    match &v {
        Value::Exception(_) => Ok(PyException::from_value(&v)),
        Value::Builtin(name) => match EXC_CLASSES.iter().find(|(n, _)| n == name) {
            Some((n, _)) => Ok(PyException::from_value(&Value::Exception(Rc::new(ExcObj::new(n, Vec::new()))))),
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
                Some(Value::Big(n)) => Ok(Value::Float(crate::bigint::to_f64(n)?)),
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
                Value::Int(i) => Ok(i.checked_abs().map_or_else(
                    || crate::bigint::norm(num_traits::Signed::abs(&num_bigint::BigInt::from(i))),
                    Value::Int,
                )),
                Value::Big(n) => Ok(crate::bigint::norm(num_traits::Signed::abs(&*n))),
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

pub(crate) fn len(v: &Value) -> PyResult<i64> {
    Ok(match v {
        Value::Str(s) => s.len() as i64,
        Value::Bytes(b) => b.len() as i64,
        Value::ByteArray(b) => b.borrow().len() as i64,
        Value::List(l) => l.borrow().len() as i64,
        Value::Tuple(t) => t.len() as i64,
        Value::Dict(d) => d.borrow().len() as i64,
        Value::Set(s) => s.borrow().len() as i64,
        Value::Range(r) => r.len(),
        Value::Ext(e) if e.len().is_some() => e.len().unwrap_or(0) as i64,
        Value::Class(c) => {
            let mut vm = current().ok_or_else(|| internal("no vm"))?;
            match vm.meta_dunder(c, "__len__", Vec::new(), Vec::new()) {
                Some(r) => match r? {
                    Value::Int(n) => n,
                    _ => return Err(type_error("'__len__' should return an integer")),
                },
                None => return Err(type_error("object of type 'type' has no len()")),
            }
        }
        Value::Instance(_) => {
            let mut vm = current().ok_or_else(|| internal("no vm"))?;
            match vm.call_dunder(v, "__len__", Vec::new()) {
                Some(r) => match r? {
                    Value::Int(n) if n >= 0 => n,
                    Value::Int(_) => return Err(exc("ValueError", "__len__() should return >= 0")),
                    other => {
                        return Err(type_error(format!(
                            "'{}' object cannot be interpreted as an integer",
                            other.type_name()
                        )))
                    }
                },
                None => return Err(type_error(format!("object of type '{}' has no len()", v.type_name()))),
            }
        }
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
        Value::Big(_) => Ok(v.clone()),
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
                return Ok(crate::bigint::norm(crate::bigint::float_to_big(t).unwrap_or_default()));
            }
            Ok(Value::Int(t as i64))
        }
        Value::Str(s) => parse_int(s.as_str())
            .ok_or_else(|| exc("ValueError", format!("invalid literal for int() with base 10: {}", repr(v)))),
        _ => Err(type_error(format!(
            "int() argument must be a string, a bytes-like object or a real number, not '{}'",
            v.type_name()
        ))),
    }
}

/// Literal decimal do `int(str)`: espaços em volta, sinal opcional, `_` só entre dígitos.
pub(crate) fn parse_int(text: &str) -> Option<Value> {
    let t = text.trim_matches(char::is_whitespace);
    let (negative, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') || digits.contains("__") {
        return None;
    }
    let clean: String = digits.chars().filter(|c| *c != '_').collect();
    if !clean.chars().all(|c| c.is_ascii_digit() || c.to_digit(10).is_some()) {
        return None;
    }
    let ascii: String = clean.chars().map(|c| char::from_digit(c.to_digit(10).unwrap_or(0), 10).unwrap_or('0')).collect();
    let big = crate::bigint::parse(&ascii, 10)?;
    Some(crate::bigint::norm(if negative { -big } else { big }))
}

/// Índice normalizado de uma sequência de tamanho `len`, ou `None` se estiver fora.
fn normalize(i: i64, len: usize) -> Option<usize> {
    let len = len as i64;
    let i = if i < 0 { i + len } else { i };
    (0..len).contains(&i).then_some(i as usize)
}

/// `start, stop, step` já ajustados de uma fatia sobre uma sequência de tamanho `len`
/// (`PySlice_Unpack` seguido de `PySlice_AdjustIndices`).
pub(crate) fn slice_bounds(len: i64, s: &(Value, Value, Value)) -> PyResult<(i64, i64, i64)> {
    let index = |v: &Value| -> PyResult<Option<i64>> {
        match v {
            Value::None => Ok(None),
            Value::Int(i) => Ok(Some(*i)),
            Value::Bool(b) => Ok(Some(i64::from(*b))),
            _ => Err(type_error("slice indices must be integers or None or have an __index__ method")),
        }
    };
    let step = index(&s.2)?.unwrap_or(1);
    if step == 0 {
        return Err(exc("ValueError", "slice step cannot be zero"));
    }
    let adjust = |v: Option<i64>, default: i64| -> i64 {
        match v {
            None => default,
            Some(mut v) => {
                if v < 0 {
                    v += len;
                    if v < 0 {
                        v = if step < 0 { -1 } else { 0 };
                    }
                } else if v >= len {
                    v = if step < 0 { len - 1 } else { len };
                }
                v
            }
        }
    };
    let (lo_default, hi_default) = if step < 0 { (len - 1, -1) } else { (0, len) };
    let start = adjust(index(&s.0)?, lo_default);
    let stop = adjust(index(&s.1)?, hi_default);
    Ok((start, stop, step))
}

/// Os índices que uma fatia seleciona numa sequência de tamanho `len`.
pub(crate) fn slice_indices(len: usize, s: &(Value, Value, Value)) -> PyResult<Vec<usize>> {
    let (start, stop, step) = slice_bounds(len as i64, s)?;
    let mut out = Vec::new();
    let mut i = start;
    while (step > 0 && i < stop) || (step < 0 && i > stop) {
        out.push(i as usize);
        i += step;
    }
    Ok(out)
}

fn slice_of(container: &Value, s: &(Value, Value, Value)) -> PyResult<Value> {
    Ok(match container {
        Value::List(l) => {
            let items = l.borrow();
            Value::list(slice_indices(items.len(), s)?.into_iter().map(|i| items[i].clone()).collect())
        }
        Value::Tuple(t) => Value::tuple(slice_indices(t.len(), s)?.into_iter().map(|i| t[i].clone()).collect()),
        Value::Str(text) => {
            let chars: Vec<char> = text.as_str().chars().collect();
            Value::str(slice_indices(chars.len(), s)?.into_iter().map(|i| chars[i]).collect::<String>())
        }
        Value::Bytes(b) => Value::bytes(slice_indices(b.len(), s)?.into_iter().map(|i| b[i]).collect::<Vec<u8>>()),
        Value::ByteArray(b) => {
            let b = b.borrow();
            Value::bytearray(slice_indices(b.len(), s)?.into_iter().map(|i| b[i]).collect::<Vec<u8>>())
        }
        Value::Range(r) => {
            let items: Vec<Value> = slice_indices(r.len() as usize, s)?.into_iter().map(|i| Value::Int(r.item(i as i64))).collect();
            Value::list(items)
        }
        _ => return Err(type_error(format!("'{}' object is not subscriptable", container.type_name()))),
    })
}

fn subscript(container: &Value, index: &Value) -> PyResult<Value> {
    if let Value::Slice(s) = index {
        if matches!(container, Value::List(_) | Value::Tuple(_) | Value::Str(_) | Value::Bytes(_) | Value::ByteArray(_) | Value::Range(_)) {
            return slice_of(container, s);
        }
    }
    if let Value::Class(_) | Value::NativeFn(_) | Value::Builtin(_) = container {
        if let Some(mut vm) = current() {
            if let Some(r) = crate::generic::class_getitem(&mut vm, container, index) {
                return r;
            }
        }
    }
    if let Value::Instance(_) = container {
        if let Some(mut vm) = current() {
            if let Some(r) = vm.call_dunder(container, "__getitem__", vec![index.clone()]) {
                return r;
            }
        }
        return Err(type_error(format!("'{}' object is not subscriptable", container.type_name())));
    }
    let seq_index = |what: &str| -> PyResult<i64> {
        as_index(index)
            .ok_or_else(|| type_error(format!("{what} indices must be integers or slices, not {}", index.type_name())))
    };
    match container {
        Value::Ext(e) => match e.getitem(index) {
            Some(r) => r,
            None => Err(type_error(format!("'{}' object is not subscriptable", container.type_name()))),
        },
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
        Value::ByteArray(b) => {
            let i = as_index(index).ok_or_else(|| {
                type_error(format!("bytearray indices must be integers or slices, not {}", index.type_name()))
            })?;
            let b = b.borrow();
            normalize(i, b.len()).map(|i| Value::Int(i64::from(b[i]))).ok_or_else(|| exc("IndexError", "bytearray index out of range"))
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
                value: Some(Value::Exception(Rc::new(ExcObj::new("KeyError", vec![index.clone()])))),
                tb: Vec::new(),
            }),
        },
        // `__builtins__['compile']`: nos módulos importados do CPython ele é o dicionário de `builtins`;
        // aqui é sempre o módulo, que aceita a mesma consulta por nome.
        Value::Module(m) if m.name == "builtins" => match index {
            Value::Str(s) => match current().map(|mut vm| vm.getattr(container, s.as_str())) {
                Some(Ok(v)) => Ok(v),
                _ => Err(PyException {
                    kind: "KeyError",
                    msg: repr(index),
                    value: Some(Value::Exception(Rc::new(ExcObj::new("KeyError", vec![index.clone()])))),
                    tb: Vec::new(),
                }),
            },
            _ => Err(type_error(format!("'{}' object is not subscriptable", container.type_name()))),
        },
        _ => Err(type_error(format!("'{}' object is not subscriptable", container.type_name()))),
    }
}

pub(crate) fn store_subscript(container: &Value, index: &Value, value: Value) -> PyResult<()> {
    if let Value::Instance(_) = container {
        if let Some(mut vm) = current() {
            if let Some(r) = vm.call_dunder(container, "__setitem__", vec![index.clone(), value]) {
                return r.map(|_| ());
            }
        }
        return Err(type_error(format!("'{}' object does not support item assignment", container.type_name())));
    }
    if let (Value::List(l), Value::Slice(s)) = (container, index) {
        let new_items = collect(&value)?;
        let len = l.borrow().len();
        let (start, stop, step) = slice_bounds(len as i64, s)?;
        if step == 1 {
            let start = start as usize;
            let stop = (stop.max(start as i64)) as usize;
            l.borrow_mut().splice(start..stop, new_items);
            return Ok(());
        }
        let idxs = slice_indices(len, s)?;
        if idxs.len() != new_items.len() {
            return Err(exc(
                "ValueError",
                format!(
                    "attempt to assign sequence of size {} to extended slice of size {}",
                    new_items.len(),
                    idxs.len()
                ),
            ));
        }
        let mut items = l.borrow_mut();
        for (i, v) in idxs.into_iter().zip(new_items) {
            items[i] = v;
        }
        return Ok(());
    }
    if let (Value::ByteArray(b), Value::Slice(s)) = (container, index) {
        let new_bytes = match value.bytes_like() {
            Some(x) => x.to_vec(),
            None if matches!(value, Value::Int(_)) => return Err(type_error("can assign only bytes, buffers, or iterables of ints in range(0, 256)")),
            None => crate::methods::bytearraym::bytes_of_iterable(&value)?,
        };
        let len = b.borrow().len();
        let (start, stop, step) = slice_bounds(len as i64, s)?;
        if step == 1 {
            let start = start as usize;
            let stop = (stop.max(start as i64)) as usize;
            b.borrow_mut().splice(start..stop, new_bytes);
            return Ok(());
        }
        let idxs = slice_indices(len, s)?;
        if idxs.len() != new_bytes.len() {
            return Err(exc(
                "ValueError",
                format!(
                    "attempt to assign bytes of size {} to extended slice of size {}",
                    new_bytes.len(),
                    idxs.len()
                ),
            ));
        }
        let mut items = b.borrow_mut();
        for (i, v) in idxs.into_iter().zip(new_bytes) {
            items[i] = v;
        }
        return Ok(());
    }
    match container {
        Value::ByteArray(b) => {
            let i = as_index(index).ok_or_else(|| {
                type_error(format!("bytearray indices must be integers or slices, not {}", index.type_name()))
            })?;
            let byte = crate::methods::bytearraym::want_byte(&value)?;
            let mut items = b.borrow_mut();
            let len = items.len();
            let i = normalize(i, len).ok_or_else(|| exc("IndexError", "bytearray assignment index out of range"))?;
            items[i] = byte;
            Ok(())
        }
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

/// Os dois valores do topo da pilha são `int` ou `float` (os operadores correm sem despacho de objetos).
#[inline]
fn num_pair(stack: &[Slot]) -> bool {
    let n = stack.len();
    n >= 2
        && matches!(&stack[n - 1], Slot::Val(Value::Int(_) | Value::Float(_)))
        && matches!(&stack[n - 2], Slot::Val(Value::Int(_) | Value::Float(_)))
}

fn binary(op: Operator, a: &Value, b: &Value, inplace: bool) -> PyResult<Value> {
    // Caminho rápido: aritmética de `int` e `float` sem estouro nem divisão por zero.
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            let r = match op {
                Operator::Add => x.checked_add(*y),
                Operator::Sub => x.checked_sub(*y),
                Operator::Mult => x.checked_mul(*y),
                Operator::Mod if *y != 0 && *y != -1 => {
                    let m = x % y;
                    Some(if m != 0 && (m ^ y) < 0 { m + y } else { m })
                }
                Operator::FloorDiv if *y != 0 && *y != -1 => {
                    let d = x / y;
                    Some(if x % y != 0 && (x ^ y) < 0 { d - 1 } else { d })
                }
                Operator::BitAnd => Some(x & y),
                Operator::BitOr => Some(x | y),
                Operator::BitXor => Some(x ^ y),
                _ => None,
            };
            if let Some(r) = r {
                return Ok(Value::Int(r));
            }
        }
        (Value::Float(x), Value::Float(y)) => match op {
            Operator::Add => return Ok(Value::Float(x + y)),
            Operator::Sub => return Ok(Value::Float(x - y)),
            Operator::Mult => return Ok(Value::Float(x * y)),
            _ => {}
        },
        _ => {}
    }
    if op == Operator::BitOr
        && !matches!(a, Value::Set(_) | Value::Dict(_) | Value::Int(_) | Value::Bool(_))
        && crate::generic::is_type_like(a)
        && crate::generic::is_type_like(b)
        && !(matches!(a, Value::None) && matches!(b, Value::None))
    {
        return Ok(crate::generic::union(a, b));
    }
    if (matches!(a, Value::Instance(_)) || matches!(b, Value::Instance(_)))
        && let Some(r) = instance_binary(op, a, b, inplace)
    {
        return r;
    }
    if let Value::Ext(e) = a
        && let Some(r) = e.binop(op_symbol(op), b, false)
    {
        return r;
    }
    if let Value::Ext(e) = b
        && let Some(r) = e.binop(op_symbol(op), a, true)
    {
        return r;
    }
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
    if matches!(a, Value::Big(_)) || matches!(b, Value::Big(_)) {
        if let (Some(x), Some(y)) = (crate::bigint::as_big(a), crate::bigint::as_big(b)) {
            return crate::bigint::binary(op, &x, &y).map_err(|e| e.unwrap_or_else(|| unsupported(op, a, b, inplace)));
        }
        let float_of = |v: &Value| -> PyResult<Option<f64>> {
            Ok(match v {
                Value::Float(x) => Some(*x),
                Value::Big(n) => Some(crate::bigint::to_f64(n)?),
                Value::Int(i) => Some(*i as f64),
                Value::Bool(b) => Some(f64::from(u8::from(*b))),
                _ => None,
            })
        };
        if let (Some(x), Some(y)) = (float_of(a)?, float_of(b)?) {
            return float_binary(op, x, y).map_err(|e| e.unwrap_or_else(|| unsupported(op, a, b, inplace)));
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
            (Num::Int(x), Num::Int(y)) => match int_binary(op, x, y) {
                Ok(v) => Ok(v),
                // Estourou o `i64`: refaz a conta em precisão arbitrária.
                Err(Some(e)) if crate::bigint::is_overflow(&e) => {
                    crate::bigint::binary(op, &x.into(), &y.into()).map_err(|e| match e {
                        Some(e) => e,
                        None => unsupported(op, a, b, inplace),
                    })
                }
                Err(Some(e)) => Err(e),
                Err(None) => Err(unsupported(op, a, b, inplace)),
            },
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
        (Operator::Add, Value::ByteArray(x), Value::ByteArray(_) | Value::Bytes(_)) => {
            let extra = b.bytes_like().unwrap_or_else(|| Rc::from(&[][..]));
            if inplace {
                x.borrow_mut().extend_from_slice(&extra);
                Ok(a.clone())
            } else {
                let mut out = x.borrow().clone();
                out.extend_from_slice(&extra);
                Ok(Value::bytearray(out))
            }
        }
        (Operator::Add, Value::Bytes(x), Value::ByteArray(y)) => Ok(Value::bytes([&x[..], &y.borrow()[..]].concat())),
        (Operator::Add, Value::Bytes(_) | Value::ByteArray(_), _) => Err(type_error(format!(
            "can't concatenate {} and {}",
            a.type_name(),
            b.type_name()
        ))),
        (Operator::Mult, Value::ByteArray(x), n) | (Operator::Mult, n, Value::ByteArray(x)) if as_index(n).is_some() => {
            let count = as_index(n).unwrap_or(0);
            let out = repeat(&x.borrow()[..], count)?;
            if inplace {
                *x.borrow_mut() = out;
                Ok(if matches!(a, Value::ByteArray(_)) { a.clone() } else { b.clone() })
            } else {
                Ok(Value::bytearray(out))
            }
        }
        (Operator::Mult, seq, n) | (Operator::Mult, n, seq) if is_sequence(seq) && !is_sequence(n) => {
            match as_index(n) {
                Some(count) => repeat_value(seq, count),
                None => Err(type_error(format!("can't multiply sequence by non-int of type '{}'", n.type_name()))),
            }
        }
        (Operator::Mult, seq, other) if is_sequence(seq) => {
            Err(type_error(format!("can't multiply sequence by non-int of type '{}'", other.type_name())))
        }
        (Operator::Mod, Value::Str(s), args) => crate::format::percent_format(s.as_str(), args).map(Value::str),
        (Operator::Mod, Value::Bytes(b), args) => crate::format::bytes_percent_format(b, args).map(Value::bytes),
        (Operator::Mod, Value::ByteArray(b), args) => {
            crate::format::bytes_percent_format(&b.borrow(), args).map(Value::bytearray)
        }
        (Operator::BitOr | Operator::BitAnd | Operator::Sub | Operator::BitXor, Value::Set(x), Value::Set(y)) => {
            set_binary(op, x, y, inplace)
        }
        (Operator::BitOr, Value::Dict(x), Value::Dict(y)) => {
            let items: Vec<(Value, Value)> = y.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            if inplace {
                for (k, v) in items {
                    x.borrow_mut().set(k, v)?;
                }
                return Ok(a.clone());
            }
            let mut merged = x.borrow().clone();
            for (k, v) in items {
                merged.set(k, v)?;
            }
            Ok(Value::dict(merged))
        }
        _ => Err(unsupported(op, a, b, inplace)),
    }
}

/// `|`, `&`, `-` e `^` entre conjuntos (e as versões `|=`... que mudam o da esquerda).
fn set_binary(op: Operator, x: &Rc<RefCell<Set>>, y: &Rc<RefCell<Set>>, inplace: bool) -> PyResult<Value> {
    let xs: Vec<Value> = x.borrow().iter().cloned().collect();
    let ys: Vec<Value> = y.borrow().iter().cloned().collect();
    // O resultado tem o tipo do operando da esquerda; `frozenset |= x` não existe, o nome é religado.
    let frozen = x.borrow().is_frozen();
    let inplace = inplace && !frozen;
    let mut out = Set::new();
    match op {
        Operator::BitOr => {
            for v in xs.iter().chain(ys.iter()) {
                out.add(v.clone())?;
            }
        }
        Operator::BitAnd => {
            let yb = y.borrow();
            for v in &xs {
                if yb.contains(v)? {
                    out.add(v.clone())?;
                }
            }
        }
        Operator::Sub => {
            let yb = y.borrow();
            for v in &xs {
                if !yb.contains(v)? {
                    out.add(v.clone())?;
                }
            }
        }
        _ => {
            // `set_symmetric_difference`: cópia do da direita, alternando cada item do da esquerda; o `^=`
            // (`set_symmetric_difference_update`) altera o da esquerda com os itens do da direita.
            let (mut base, items) = if inplace { (x.borrow().clone(), &ys) } else { (y.borrow().clone(), &xs) };
            for v in items {
                if base.contains(v)? {
                    base.discard(v)?;
                } else {
                    base.add(v.clone())?;
                }
            }
            out = base;
        }
    }
    if inplace {
        *x.borrow_mut() = out;
        return Ok(Value::Set(x.clone()));
    }
    Ok(Value::set(out.with_frozen(frozen)))
}

fn is_sequence(v: &Value) -> bool {
    matches!(v, Value::Str(_) | Value::List(_) | Value::Tuple(_) | Value::Bytes(_) | Value::ByteArray(_))
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
        Value::ByteArray(b) => Value::bytearray(repeat(&b.borrow()[..], n)?),
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

/// Operador binário com instância de classe de usuário: `__add__`, depois `__radd__` do outro lado.
fn instance_binary(op: Operator, a: &Value, b: &Value, inplace: bool) -> Option<PyResult<Value>> {
    let sym = match op {
        Operator::Add => "+",
        Operator::Sub => "-",
        Operator::Mult => "*",
        Operator::Div => "/",
        Operator::FloorDiv => "//",
        Operator::Mod => "%",
        Operator::Pow => "**",
        Operator::BitAnd => "&",
        Operator::BitOr => "|",
        Operator::BitXor => "^",
        Operator::LShift => "<<",
        Operator::RShift => ">>",
        Operator::MatMult => "@",
    };
    let (fwd, rev, inp) = crate::classes::binop_dunder(sym)?;
    let mut vm = current()?;
    let mut tries: Vec<(&Value, &str, &Value)> = Vec::new();
    if matches!(a, Value::Instance(_)) {
        if inplace {
            tries.push((a, inp, b));
        }
        tries.push((a, fwd, b));
    }
    if matches!(b, Value::Instance(_)) {
        tries.push((b, rev, a));
    }
    for (recv, name, other) in tries {
        if let Some(r) = vm.call_dunder(recv, name, vec![other.clone()]) {
            match r {
                Ok(v) if crate::classes::is_not_implemented(&v) => {}
                other => return Some(other),
            }
        }
    }
    None
}

fn unary(op: UnaryOp, a: &Value) -> PyResult<Value> {
    if let Value::Instance(_) = a {
        let name = match op {
            UnaryOp::USub => Some("__neg__"),
            UnaryOp::UAdd => Some("__pos__"),
            UnaryOp::Invert => Some("__invert__"),
            UnaryOp::Not => None,
        };
        if let (Some(name), Some(mut vm)) = (name, current())
            && let Some(r) = vm.call_dunder(a, name, Vec::new())
        {
            return r;
        }
    }
    let bad = |sym: &str| type_error(format!("bad operand type for unary {sym}: '{}'", a.type_name()));
    match op {
        UnaryOp::Not => Ok(Value::Bool(!a.is_true())),
        UnaryOp::USub if matches!(a, Value::Big(_)) => Ok(crate::bigint::norm(-crate::bigint::as_big(a).unwrap_or_default())),
        UnaryOp::UAdd if matches!(a, Value::Big(_)) => Ok(a.clone()),
        UnaryOp::Invert if matches!(a, Value::Big(_)) => {
            Ok(crate::bigint::norm(-crate::bigint::as_big(a).unwrap_or_default() - 1))
        }
        UnaryOp::USub => match num(a) {
            Some(Num::Int(i)) => match int_neg(i) {
                Ok(n) => Ok(Value::Int(n)),
                Err(_) => Ok(crate::bigint::norm(-num_bigint::BigInt::from(i))),
            },
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
    if matches!(op, CmpOp::Lt | CmpOp::LtE | CmpOp::Gt | CmpOp::GtE)
        && (matches!(a, Value::Instance(_)) || matches!(b, Value::Instance(_)))
        && let Some(mut vm) = current()
    {
        let (fwd, rev) = match op {
            CmpOp::Lt => ("__lt__", "__gt__"),
            CmpOp::LtE => ("__le__", "__ge__"),
            CmpOp::Gt => ("__gt__", "__lt__"),
            _ => ("__ge__", "__le__"),
        };
        for (recv, name, other) in [(a, fwd, b), (b, rev, a)] {
            if let Some(r) = vm.call_dunder(recv, name, vec![other.clone()]) {
                let v = r?;
                if !crate::classes::is_not_implemented(&v) {
                    return Ok(v.is_true());
                }
            }
        }
    }
    if !matches!(op, CmpOp::Is | CmpOp::IsNot | CmpOp::In | CmpOp::NotIn) {
        if let Value::Ext(e) = a
            && let Some(r) = e.richcmp(cmp_symbol(op), b)
        {
            return r;
        }
        if let Value::Ext(e) = b {
            let mirrored = match op {
                CmpOp::Lt => ">",
                CmpOp::LtE => ">=",
                CmpOp::Gt => "<",
                CmpOp::GtE => "<=",
                _ => cmp_symbol(op),
            };
            if let Some(r) = e.richcmp(mirrored, a) {
                return r;
            }
        }
    }
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

/// O valor embutido por trás de uma instância de subclasse de `dict`, `list`, `str`... (ou o próprio valor).
pub(crate) fn unwrap_payload(v: &Value) -> Value {
    match v {
        Value::Instance(i) => i.payload.borrow().clone().unwrap_or_else(|| v.clone()),
        _ => v.clone(),
    }
}

/// Método mágico de uma subclasse de tipo embutido que a classe não redefine: age sobre o valor
/// embutido. `None` quando o nome não é um dos delegados.
pub(crate) fn payload_dunder(payload: &Value, name: &str, args: Vec<Value>) -> Option<PyResult<Value>> {
    let arg = |i: usize| args.get(i).map(unwrap_payload);
    let op = match name {
        "__getitem__" => return Some(subscript(payload, &args[0])),
        "__setitem__" => return Some(store_subscript(payload, &args[0], args[1].clone()).map(|_| Value::None)),
        "__delitem__" => {
            let mut vm = current()?;
            return Some(vm.delete_subscript(payload, &args[0]).map(|_| Value::None));
        }
        "__contains__" => return Some(contains(payload, &args[0]).map(Value::Bool)),
        "__len__" => return Some(len(payload).map(Value::Int)),
        "__iter__" => {
            let mut vm = current()?;
            return Some(vm.call(&crate::builtins::get("iter")?, vec![payload.clone()], Vec::new()));
        }
        "__lt__" => return Some(compare(CmpOp::Lt, payload, &arg(0)?).map(Value::Bool)),
        "__le__" => return Some(compare(CmpOp::LtE, payload, &arg(0)?).map(Value::Bool)),
        "__gt__" => return Some(compare(CmpOp::Gt, payload, &arg(0)?).map(Value::Bool)),
        "__ge__" => return Some(compare(CmpOp::GtE, payload, &arg(0)?).map(Value::Bool)),
        "__add__" | "__radd__" | "__iadd__" => Operator::Add,
        "__sub__" | "__rsub__" | "__isub__" => Operator::Sub,
        "__mul__" | "__rmul__" | "__imul__" => Operator::Mult,
        "__truediv__" | "__rtruediv__" => Operator::Div,
        "__floordiv__" | "__rfloordiv__" => Operator::FloorDiv,
        "__mod__" | "__rmod__" => Operator::Mod,
        "__pow__" | "__rpow__" => Operator::Pow,
        "__and__" | "__rand__" | "__iand__" => Operator::BitAnd,
        "__or__" | "__ror__" | "__ior__" => Operator::BitOr,
        "__xor__" | "__rxor__" | "__ixor__" => Operator::BitXor,
        _ => return None,
    };
    let other = arg(0)?;
    let reflected = name.starts_with("__r") && !matches!(name, "__rshift__");
    let inplace = matches!(name, "__iadd__" | "__isub__" | "__imul__" | "__iand__" | "__ior__" | "__ixor__");
    Some(if reflected { binary(op, &other, payload, false) } else { binary(op, payload, &other, inplace) })
}

/// Os pares de um mapeamento: `dict`, subclasse de `dict` ou objeto com `keys()` e `__getitem__`.
/// `None` quando `v` não é um mapeamento (e sim, talvez, um iterável de pares).
pub(crate) fn mapping_pairs(v: &Value) -> PyResult<Option<Vec<(Value, Value)>>> {
    match unwrap_payload(v) {
        Value::Dict(d) => Ok(Some(d.borrow().iter().map(|(k, x)| (k.clone(), x.clone())).collect())),
        Value::Instance(i) if i.class.lookup("keys").is_some() => {
            let mut vm = current().ok_or_else(|| internal("no vm"))?;
            let keys_fn = vm.getattr(v, "keys")?;
            let keys = vm.call(&keys_fn, Vec::new(), Vec::new())?;
            let mut out = Vec::new();
            for k in collect(&keys)? {
                let value = subscript(v, &k)?;
                out.push((k, value));
            }
            Ok(Some(out))
        }
        _ => Ok(None),
    }
}

/// `item in container`.
fn contains(container: &Value, item: &Value) -> PyResult<bool> {
    if let Value::Ext(e) = container {
        if let Some(r) = e.contains_item(item) {
            return r;
        }
        if let Some(items) = e.to_items() {
            return Ok(items.iter().any(|x| is(x, item) || py_eq(x, item)));
        }
    }
    if let Value::Class(c) = container
        && let Some(mut vm) = current()
        && let Some(r) = vm.meta_dunder(c, "__contains__", vec![item.clone()], Vec::new())
    {
        return Ok(r?.is_true());
    }
    if let Value::Instance(_) = container
        && let Some(mut vm) = current()
    {
        if let Some(r) = vm.call_dunder(container, "__contains__", vec![item.clone()]) {
            return Ok(r?.is_true());
        }
        let items = collect(container)?;
        return Ok(items.iter().any(|x| is(x, item) || py_eq(x, item)));
    }
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
        Value::Bytes(_) | Value::ByteArray(_) => match as_index(item) {
            Some(i) if (0..256).contains(&i) => Ok(container.bytes_like().is_some_and(|b| b.contains(&(i as u8)))),
            Some(_) => Err(exc("ValueError", "byte must be in range(0, 256)")),
            None => match item {
                Value::Bytes(_) | Value::ByteArray(_) => {
                    let b = container.bytes_like().unwrap_or_else(|| Rc::from(&[][..]));
                    let sub = item.bytes_like().unwrap_or_else(|| Rc::from(&[][..]));
                    Ok(sub.is_empty() || b.windows(sub.len()).any(|w| w == &sub[..]))
                }
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
    if matches!(a, Value::Big(_)) || matches!(b, Value::Big(_)) {
        let ord = match (a, b) {
            (Value::Float(x), Value::Big(n)) => Some(crate::bigint::cmp_float(n, *x).map(std::cmp::Ordering::reverse)),
            (Value::Big(n), Value::Float(x)) => Some(crate::bigint::cmp_float(n, *x)),
            _ => match (crate::bigint::as_big(a), crate::bigint::as_big(b)) {
                (Some(x), Some(y)) => Some(Some(x.cmp(&y))),
                _ => None,
            },
        };
        if let Some(ord) = ord {
            return Ok(ord.is_some_and(|o| apply(op, o)));
        }
    }
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
        (Value::Bytes(_) | Value::ByteArray(_), Value::Bytes(_) | Value::ByteArray(_)) => {
            let (x, y) = (a.bytes_like().unwrap_or_else(|| Rc::from(&[][..])), b.bytes_like().unwrap_or_else(|| Rc::from(&[][..])));
            Ok(apply(op, x[..].cmp(&y[..])))
        }
        (Value::List(x), Value::List(y)) => {
            let (x, y) = (x.borrow().clone(), y.borrow().clone());
            seq_order(op, &x, &y)
        }
        (Value::Tuple(x), Value::Tuple(y)) => seq_order(op, x, y),
        (Value::Set(x), Value::Set(y)) => {
            // `<=` é subconjunto, `<` subconjunto próprio; `>=` e `>` os inversos.
            let (x, y) = (x.borrow(), y.borrow());
            let (small, big, strict) = match op {
                CmpOp::Lt => (&*x, &*y, true),
                CmpOp::LtE => (&*x, &*y, false),
                CmpOp::Gt => (&*y, &*x, true),
                _ => (&*y, &*x, false),
            };
            if small.len() > big.len() || (strict && small.len() == big.len()) {
                return Ok(false);
            }
            for k in small.iter() {
                if !big.contains(k)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
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
            return compare(op, p, q);
        }
    }
    Ok(apply(op, x.len().cmp(&y.len())))
}

/// Resultado de repassar um `send`/`throw` ao sub-iterador de uma delegação (`await`, `yield from`).
pub(crate) enum Step {
    /// O sub-iterador entregou um valor: a delegação o devolve ao chamador e continua depois.
    Yield(Value),
    /// O sub-iterador terminou com este valor de retorno.
    Done(Value),
}

impl Vm {
    /// `await x`: o iterador aguardável de `x`.
    fn get_awaitable(&mut self, v: &Value) -> PyResult<Value> {
        let not_awaitable = || type_error(format!("object {} can't be used in 'await' expression", v.type_name()));
        match v {
            Value::Ext(e) if matches!(e.type_name(), "coroutine" | "async_generator_asend" | "coroutine_wrapper" | "generator") => Ok(v.clone()),
            Value::Ext(e) if e.methods().contains(&"__await__") => {
                e.clone().call_method(self, "__await__", Vec::new(), Vec::new())
            }
            Value::Instance(_) => match self.call_dunder(v, "__await__", Vec::new()) {
                Some(r) => {
                    let it = r?;
                    if matches!(it, Value::Ext(_) | Value::Instance(_)) {
                        Ok(it)
                    } else {
                        Err(type_error(format!("__await__() returned non-iterator of type '{}'", it.type_name())))
                    }
                }
                None => Err(not_awaitable()),
            },
            _ => Err(not_awaitable()),
        }
    }

    /// Passa `sent` ao sub-iterador que está no topo da pilha.
    fn delegate_step(&mut self, slot: &mut Slot, sent: Value) -> PyResult<Step> {
        let into_step = |r: PyResult<Value>| match r {
            Ok(v) => Ok(Step::Yield(v)),
            Err(e) if e.kind == "StopIteration" => Ok(Step::Done(crate::generator::stop_value(&e))),
            Err(e) => Err(e),
        };
        match slot {
            Slot::Iter(PyIter::Ext(e)) | Slot::Val(Value::Ext(e)) if e.methods().contains(&"send") => {
                let e = e.clone();
                into_step(e.call_method(self, "send", vec![sent], Vec::new()))
            }
            Slot::Iter(PyIter::Inst(v)) | Slot::Val(v @ Value::Instance(_)) => {
                let v = v.clone();
                if matches!(sent, Value::None) {
                    match self.call_dunder(&v, "__next__", Vec::new()) {
                        Some(r) => into_step(r),
                        None => Err(type_error(format!("'{}' object is not an iterator", v.type_name()))),
                    }
                } else {
                    let send = self.getattr(&v, "send")?;
                    into_step(self.call(&send, vec![sent], Vec::new()))
                }
            }
            Slot::Iter(it) => Ok(match it.next()? {
                Some(v) => Step::Yield(v),
                None => Step::Done(Value::None),
            }),
            _ => Err(internal("bad delegation iterator")),
        }
    }

    /// Repassa a exceção injetada (`throw`) ao sub-iterador que está no topo da pilha.
    fn delegate_throw(&mut self, stack: &mut [Slot], e: PyException) -> PyResult<Step> {
        let target: Option<Value> = match stack.last() {
            Some(Slot::Iter(PyIter::Ext(x))) | Some(Slot::Val(Value::Ext(x))) => Some(Value::Ext(x.clone())),
            Some(Slot::Iter(PyIter::Inst(v))) | Some(Slot::Val(v @ Value::Instance(_))) => Some(v.clone()),
            _ => None,
        };
        let Some(target) = target else { return Err(e) };
        if e.kind == "GeneratorExit" {
            if let Ok(close) = self.getattr(&target, "close") {
                self.call(&close, Vec::new(), Vec::new())?;
            }
            return Err(e);
        }
        let Ok(throw) = self.getattr(&target, "throw") else { return Err(e) };
        match self.call(&throw, vec![e.to_value()], Vec::new()) {
            Ok(v) => Ok(Step::Yield(v)),
            Err(x) if x.kind == "StopIteration" => Ok(Step::Done(crate::generator::stop_value(&x))),
            Err(x) => Err(x),
        }
    }

    /// `type(obj).nome` ligado a `obj`: o método dunder que o protocolo usa (`__aenter__`...).
    fn attr_of_type(&mut self, obj: &Value, name: &str) -> Option<Value> {
        match obj {
            Value::Instance(i) => {
                let attr = i.class.lookup(name)?;
                self.bind_class_attr(&attr, obj.clone(), &i.class).ok()
            }
            Value::Ext(e) if e.methods().contains(&name) => self.getattr(obj, name).ok(),
            _ => None,
        }
    }
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
                let err = vm.run(&Rc::new(code)).err().map(|e| format_traceback(&e));
                (String::from_utf8(vm.stdout.borrow().clone()).expect("utf-8"), err)
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
    fn type_methods_come_from_the_tables() {
        assert_eq!(out("print('abc'.upper(), 'AbC'.lower(), 'abc'.startswith('a'))"), "ABC abc True\n");
        assert!(error("'abc'.nope").contains("AttributeError: 'str' object has no attribute 'nope'"));
        assert!(error("'abc'.upper(1)").contains("TypeError: upper() takes exactly 0 argument"));
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
