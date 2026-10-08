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
use crate::generator::{Begun, Callback, CallbackKind, Collect, GenCore, Layer, Pull, ResumeUse, Resumed, Resuming};
use crate::modules::{csv, json};
use crate::object::{
    exc_is_subclass, exc_str, int_add, int_mul, int_neg, int_sub, is, py_eq, repr, to_str, BoundMethod, Dict, Env,
    ExcObj, FileKind, FuncObj, Native, ObjError, PyFile, PyStr, Range, Set, Value, EXC_CLASSES, str_cmp,
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
/// da instrução que falhou. O quinto item é o escopo e o código do quadro de uma função, que o
/// `tb_frame` mostra em `f_locals` e `f_code` (o traceback mantém o quadro vivo, como no CPython).
pub type TbEntry = (usize, String, Rc<str>, crate::compile::Span, Option<Rc<crate::frameobj::FrameHold>>);

/// Desde o 3.12 (PEP 709) as compreensões de lista, conjunto e dicionário não têm quadro próprio:
/// o traceback mostra a linha de dentro com o nome da função que as contém. (`<genexpr>` mantém o seu.)
/// Módulos da biblioteca embutida que no CPython são código C (`_io`, `_socket`, `_csv`...) ou que só
/// existem aqui: os quadros deles não entram no traceback, como não entrariam no do CPython.
fn native_in_cpython(filename: &str, qual: &str) -> bool {
    let Some(name) = filename.strip_prefix("/usr/lib/python3.13/") else { return false };
    // Funções que no CPython vêm de módulos em C (`_functools`, `_collections`, `_heapq`...): o
    // módulo `.py` existe lá, mas estes nomes nunca aparecem como quadro num traceback.
    let top = qual.split('.').next().unwrap_or(qual);
    match name {
        // Módulos inteiros em C no CPython (`_contextvars`, `_datetime`, `_decimal`, `_struct`):
        // o `.py` do Debian só reexporta, então tudo o que o embutido define é interior de C.
        "contextvars.py" | "datetime.py" | "decimal.py" | "struct.py" => return true,
        // No `os.py` do CPython só estes nomes são Python; o resto vem do `posix` (C).
        "os.py" => {
            return !matches!(
                top,
                "PathLike" | "_get_exports_list" | "fsencode" | "fsdecode" | "makedirs" | "removedirs" | "renames"
                    | "walk" | "execl" | "execle" | "execlp" | "execlpe" | "execvp" | "execvpe" | "_execvpe"
                    | "get_exec_path" | "_Environ" | "_createenviron" | "getenv" | "getenvb" | "_check_bytes"
                    | "fdopen" | "popen" | "_wrap_close" | "_exists" | "_init_posix" | "_build_supports"
            )
        }
        _ => {}
    }
    if C_ACCELERATED.iter().any(|(file, names)| *file == name && names.contains(&top)) {
        return true;
    }
    if sysabi::sys::try_current().is_some() {
        // Shim embutido de um módulo que no CPython é C (`marshal`, `sys`, `_socket`...): não há `.py`
        // dele no disco do Debian, e um quadro de C nunca aparece no traceback.
        if !stdlib_file_exists(filename) {
            return true;
        }
        // Auxiliar que só o embutido tem: o `.py` do Debian não define esse nome, então o CPython
        // nunca tem um quadro dele (o `<module>` do arquivo conta, ele aparece no import).
        if !top.starts_with('<') && !python_defines(filename, top) {
            return true;
        }
    }
    matches!(
        name,
        "io.py"
            | "itertools.py"
            | "operator.py"
            | "bisect.py"
            | "_socket.py"
            | "_posixsubprocess.py"
            | "_ssl.py"
            | "_imp.py"
            | "_net.py"
            | "_csv.py"
            | "_random.py"
            | "_thread.py"
            | "_gsched.py"
            | "_string.py"
            | "_memoryview.py"
            | "_complex.py"
            | "_lsprof.py"
            | "_tracemalloc.py"
            | "_ast.py"
            | "_tokenize.py"
            | "_archivefile.py"
            | "_asyncio.py"
            | "_match.py"
            | "_excgroup.py"
            // Módulos que no Debian não têm `.py` (são C): a lista vale também sem o kernel, onde o
            // teste de existência no disco acima não roda.
            | "sys.py"
            | "posix.py"
            | "time.py"
            | "marshal.py"
            | "gc.py"
            | "atexit.py"
            | "termios.py"
            | "fcntl.py"
            | "pwd.py"
            | "grp.py"
            | "resource.py"
            | "faulthandler.py"
            | "zlib.py"
            | "cmath.py"
            | "_signal.py"
            | "_stat.py"
            | "_queue.py"
    )
}

/// Nomes que o `.py` do Debian define mas que o CPython sobrepõe com o módulo em C (`_functools`,
/// `_collections`, `_heapq`, `_pickle`, `_asyncio`...): o `.py` existe, mas estes nomes nunca aparecem
/// como quadro num traceback. Os nomes que o `.py` do Debian nem define saem sozinhos, por
/// `python_defines`.
const C_ACCELERATED: &[(&str, &[&str])] = &[
    ("pickle.py", &["Pickler", "Unpickler", "dump", "dumps", "load", "loads"]),
    ("bz2.py", &["BZ2Compressor", "BZ2Decompressor"]),
    ("lzma.py", &["LZMACompressor", "LZMADecompressor", "is_check_supported"]),
    ("asyncio/futures.py", &["Future"]),
    (
        "asyncio/tasks.py",
        &[
            "Task", "current_task", "all_tasks", "_register_task", "_register_eager_task", "_unregister_task",
            "_unregister_eager_task", "_enter_task", "_leave_task", "_swap_current_task",
        ],
    ),
    ("asyncio/events.py", &["get_running_loop", "_get_running_loop", "_set_running_loop", "get_event_loop"]),
    ("functools.py", &["reduce", "partial", "cmp_to_key", "_lru_cache_wrapper", "_make_key", "_HashedSeq"]),
    ("collections/__init__.py", &["deque", "defaultdict", "OrderedDict", "_count_elements"]),
    (
        "heapq.py",
        &[
            "heappush", "heappop", "heapify", "heapreplace", "heappushpop", "_siftdown", "_siftup", "_heappop_max",
            "_heapify_max", "_heapreplace_max", "_siftdown_max", "_siftup_max",
        ],
    ),
    // `RLock` é o `_thread.RLock`; `excepthook`, `_ExceptHookArgs` e `stack_size` vêm do `_thread`.
    ("threading.py", &["RLock", "excepthook", "_ExceptHookArgs", "stack_size"]),
    ("queue.py", &["_PySimpleQueue"]),
    ("statistics.py", &["_normal_dist_inv_cdf"]),
];

/// O `.py` da stdlib no disco define `name` (`def`, `async def` ou `class`, em qualquer nível de
/// indentação: o CPython define muita coisa dentro de `try`/`if`). Com cache por arquivo.
fn python_defines(path: &str, name: &str) -> bool {
    thread_local! {
        static NAMES: RefCell<std::collections::HashMap<String, std::collections::HashSet<String>>> = RefCell::new(Default::default());
    }
    NAMES.with(|n| {
        if let Some(set) = n.borrow().get(path) {
            return set.contains(name);
        }
        let text = sysabi::sys::read_file(path.as_bytes()).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        // Fora os corpos de classe: um `def` dentro de `if`, `try` ou de outra função (o `fsencode` do `_fscodec`)
        // conta, um método não (o `def close(self)` do `_wrap_close` não faz do `os.close` uma função Python).
        let mut set = std::collections::HashSet::new();
        let mut body_indent: Option<usize> = None;
        for line in text.lines() {
            let l = line.trim_start();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            let indent = line.len() - l.len();
            if body_indent.is_some_and(|b| indent > b) {
                continue;
            }
            body_indent = None;
            let Some(rest) = l.strip_prefix("async def ").or_else(|| l.strip_prefix("def ")).or_else(|| l.strip_prefix("class ")) else {
                continue;
            };
            let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(rest.len());
            set.insert(rest[..end].to_string());
            body_indent = l.starts_with("class ").then_some(indent);
        }
        let found = set.contains(name);
        n.borrow_mut().insert(path.to_string(), set);
        found
    })
}

/// Se o arquivo da stdlib existe no disco (com cache: a imagem do Debian não muda durante o processo).
fn stdlib_file_exists(path: &str) -> bool {
    thread_local! {
        static SEEN: RefCell<std::collections::HashMap<String, bool>> = RefCell::new(Default::default());
    }
    SEEN.with(|s| {
        if let Some(&known) = s.borrow().get(path) {
            return known;
        }
        let exists = sysabi::sys::stat(path.as_bytes()).is_ok_and(|st| st.mode & 0o170_000 == 0o100_000);
        s.borrow_mut().insert(path.to_string(), exists);
        exists
    })
}

/// O código é de um módulo embutido que no CPython seria C: o rastreador, o perfil, `sys._getframe`,
/// `f_back` e o traceback não enxergam quadros dele. Só o código marcado como interno conta (um
/// `.py` da stdlib lido do disco pelo usuário é Python de verdade).
pub(crate) fn code_is_native(code: &Code) -> bool {
    code.internal && native_in_cpython(&code.filename, &code.qual())
}

/// A função de nível de módulo do `os.py` embutido que no CPython vem do `posix` (C): o `os.py` do Debian não
/// a define, então `type()`, `repr()` e `__module__` a mostram como `builtin_function_or_method` do `posix`.
pub(crate) fn code_is_posix_builtin(code: &Code) -> bool {
    if !code.internal || code.filename != "/usr/lib/python3.13/os.py" || sysabi::sys::try_current().is_none() {
        return false;
    }
    let name = code.qual();
    !name.starts_with('<') && !python_defines(&code.filename, &name)
}

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
            // Instância de exceção de usuário: desde o 3.13 o traceback só qualifica o nome quando a
            // classe não vem de `__main__` (`pkg.mod.Erro: ...`, mas `Erro: ...` no script).
            Value::Instance(i) if i.class().builtin_base.is_some() => PyException {
                kind: match i.class().module().as_str() {
                    "__main__" | "builtins" => crate::object::intern(&i.class().qualname()),
                    m => crate::object::intern(&format!("{m}.{}", i.class().qualname())),
                },
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

    /// A exceção está voltando a subir (`raise` sem argumento, fim de `finally`/`with`)? Isso não
    /// gera o evento `exception` do `sys.settrace`, que só o `raise` novo e o erro de instrução geram.
    pub(crate) fn is_reraise(&self) -> bool {
        self.tb.last().is_some_and(|t| t.0 == usize::MAX)
    }

    /// Tira a marca de re-raise, se houver. `true`: o quadro que está saindo não deve se acrescentar.
    pub(crate) fn take_reraise_mark(&mut self) -> bool {
        if self.is_reraise() {
            self.tb.pop();
            return true;
        }
        false
    }
}

/// Entrada sentinela no fim de `tb`: "este quadro já está no traceback" (ver `PyException::reraised`).
#[allow(non_snake_case)]
fn RERAISE_MARK() -> TbEntry {
    (usize::MAX, String::new(), Rc::from(""), crate::compile::Span::default(), None)
}

/// O destino de um `f_lineno = n`: a primeira instrução da linha `line` do código.
fn jump_index(code: &Code, line: usize) -> Option<usize> {
    code.lines.iter().position(|l| *l == line)
}

/// O quadro de função que a entrada de traceback guarda: o escopo e o código (módulos e corpos de
/// classe ficam de fora, as variáveis deles são as globais).
fn tb_frame_of(code: &Rc<Code>, env: &Rc<Env>) -> Option<Rc<crate::frameobj::FrameHold>> {
    // O quadro de módulo também: sem o código dele o `tb_lasti` não acha a instrução, e o `traceback` do
    // Debian perde os acentos circunflexos (o `exec` do `bdb` roda o script como módulo).
    (!env.is_class).then(|| crate::frameobj::FrameHold::new(env.clone(), code.clone()))
}

impl PyException {
}

/// Grava `tb` como o `__traceback__` da exceção `value` (embutida ou instância de classe de usuário).
pub(crate) fn attach_traceback(value: &Value, tb: Value) {
    match value {
        Value::Exception(x) => *x.traceback.borrow_mut() = Some(tb),
        Value::Instance(i) => {
            i.dict.borrow_mut().insert("__traceback__".to_string(), tb);
        }
        _ => {}
    }
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
        // Dois caminhos (`link`, `rename`): `texto: 'a' -> 'b'` vira `(errno, texto, a, None, b)`.
        Some(_) if rest.contains("' -> '") && rest.ends_with('\'') => {
            let (text, files) = rest.split_once(": '")?;
            let (a, b) = files[..files.len() - 1].split_once("' -> '")?;
            args.push(Value::str(text.to_string()));
            args.push(Value::str(a.to_string()));
            args.push(Value::None);
            args.push(Value::str(b.to_string()));
        }
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

/// O desfecho de um programa: `Ok` ou a exceção que subiu ao topo com a linha onde ela nasceu. É o que
/// `Vm::run` devolve e o que o filho de um `os.fork` calcula ao terminar o quadro retomado.
pub(crate) fn run_outcome(result: PyResult<Value>) -> Result<(), RuntimeError> {
    match result {
        Ok(_) => Ok(()),
        Err(e) => Err(RuntimeError { lineno: e.tb.last().map_or(0, |t| t.0), exc: e }),
    }
}

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

/// Prefixo que o `eval` põe antes da expressão para compilá-la como módulo.
const EVAL_PREFIX: &str = "__eval_value__ = (";

/// `syntax_exc` de um erro do `eval`/`compile(mode='eval')`: o parser viu `__eval_value__ = (expr)`, então
/// as colunas da primeira linha descontam o prefixo e o texto é o da expressão como o usuário a escreveu.
pub fn eval_syntax_exc(mut e: crate::parser::ParseError, filename: &str, src: &str) -> PyException {
    let src = src.trim();
    if e.lineno == 1 {
        let prefix = EVAL_PREFIX.len();
        e.offset = e.offset.saturating_sub(prefix).max(1);
    }
    if e.end_lineno == 1 {
        e.end_offset = e.end_offset.saturating_sub(EVAL_PREFIX.len()).max(1);
    }
    syntax_exc(e, filename, src)
}

/// Exceção da classe embutida `kind` com a mensagem.
pub fn exc(kind: &'static str, msg: impl Into<String>) -> PyException {
    PyException { kind, msg: msg.into(), value: None, tb: Vec::new() }
}

/// Dá ao `AttributeError` de um `obj.nome` o `name` e o `obj` que o CPython guarda (e que as sugestões usam).
/// Auxiliar escondido de módulo embutido (`_socket._fds`), visível só para código embutido e para o escopo
/// sintético dos parâmetros de tipo (PEP 695), que lê `_typing._Lazy` como o CPython lê os avaliadores em C.
fn internal_private_attr(code: &Code, obj: &Value, name: &str) -> Option<Value> {
    match obj {
        Value::Module(m) if code.internal || code.type_params_role & crate::pep695::SCOPE_MASK != 0 => {
            crate::modules::pysrc::private_attr(m.name, name)
        }
        _ => None,
    }
}

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
                push(crate::compile::comp_hidden_parts(k).map_or(&**k, |(_, plain)| plain));
            }
            if let Some(obj) = en.vars.borrow().get("self").cloned() {
                self_has |= self.clone().load_attr(&obj, &name).is_ok();
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

thread_local! {
    /// Erro de um `__repr__`/`__str__` de usuário, que o `repr()` interno (sem `Result`) não consegue devolver:
    /// a instrução em andamento o levanta assim que termina.
    static TEXT_ERROR: RefCell<Option<PyException>> = const { RefCell::new(None) };
    static TEXT_ERROR_SET: Cell<bool> = const { Cell::new(false) };
    /// Profundidade de chamadas de quem pediu o texto: só instruções desse quadro (ou de fora dele) levantam o erro.
    static TEXT_ERROR_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// Guarda o primeiro erro de conversão para texto, para o laço de instruções levantá-lo.
/// `BaseException.__new__(cls, *args)`: a exceção com `args`, sem passar pelo `__init__`.
fn base_exception_new(_vm: &mut Vm, args: Vec<Value>, _kw: crate::object::Kw) -> PyResult<Value> {
    let Some(cls) = args.first() else {
        return Err(type_error("BaseException.__new__(): not enough arguments"));
    };
    let rest = args[1..].to_vec();
    match cls {
        Value::Builtin(n) if EXC_CLASSES.iter().any(|(k, _)| k == n) => {
            Ok(Value::Exception(Rc::new(ExcObj::new(n, rest))))
        }
        Value::Class(c) if c.builtin_base.is_some() => {
            let inst = crate::object::InstanceObj::new_rc(c, None);
            inst.dict.borrow_mut().insert("args".to_string(), Value::tuple(rest));
            Ok(Value::Instance(inst))
        }
        other => Err(type_error(format!(
            "BaseException.__new__(X): X is not a type object ({})",
            other.type_name()
        ))),
    }
}

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

/// Os textos registrados (arquivo e fonte), para o filho de um `os.fork` refazer o que os tracebacks leem.
pub(crate) fn sources_snapshot() -> Vec<(String, String)> {
    SOURCES.with(|s| s.borrow().iter().map(|(file, text)| (file.clone(), text.to_string())).collect())
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
        let list: Vec<(usize, &str, &str, crate::compile::Span)> = frames.iter().map(|(l, n, o, s, _)| (*l, n.as_str(), &**o, *s)).collect();
        push_frames(&mut out, file, src, &list);
    }
    let pe = PyException::from_value(v);
    if pe.msg.is_empty() {
        out.push_str(pe.kind);
        out.push('\n');
    } else {
        out.push_str(&format!("{}: {}{}\n", pe.kind, pe.msg, hint_of(v)));
    }
    out.push_str(&notes_of(v));
    out
}

/// Traceback com o nome do arquivo; com `src` (execução de arquivo) cada quadro mostra a linha fonte
/// sem a indentação, como o CPython faz fora do `-c`.
pub fn format_traceback_in(err: &RuntimeError, file: &str, src: Option<&str>) -> String {
    let mut out = match &err.exc.value {
        Some(v) => chain_prefix(v, file, src, &mut Vec::new()),
        None => String::new(),
    };
    out.push_str("Traceback (most recent call last):\n");
    if err.exc.tb.is_empty() {
        push_frame(&mut out, file, src, err.lineno, "<module>", "", crate::compile::Span::default());
    }
    let list: Vec<(usize, &str, &str, crate::compile::Span)> = err.exc.tb.iter().rev().map(|(l, n, o, s, _)| (*l, n.as_str(), &**o, *s)).collect();
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
    if let Some(v) = &err.exc.value {
        out.push_str(&notes_of(v));
    }
    out
}

/// As linhas de `exc.__notes__` (de `add_note`), que vêm depois da linha final.
fn notes_of(v: &Value) -> String {
    let notes = match v {
        Value::Exception(e) => e.extra_get("__notes__"),
        Value::Instance(i) => i.dict.borrow().get("__notes__").cloned(),
        _ => None,
    };
    let items = match notes {
        Some(Value::List(l)) => l.borrow().clone(),
        Some(Value::Tuple(t)) => t.to_vec(),
        Some(other) => vec![other],
        None => return String::new(),
    };
    let mut out = String::new();
    for n in items {
        let text = match &n {
            Value::Str(s) => s.as_str().to_string(),
            other => crate::object::repr(other),
        };
        for line in text.split('\n') {
            out.push_str(line);
            out.push('\n');
        }
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
                let Some(c) = crate::object::units(&s.as_str()[*pos..]).next() else { return Ok(None) };
                *pos += c.len();
                Some(Value::str(c))
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
        Value::Instance(i) => {
            let mut vm = current().ok_or_else(|| internal("no vm"))?;
            match vm.call_dunder(v, "__iter__", Vec::new()) {
                Some(r) => iterator_from_dunder(r?)?,
                // Subclasse de tipo embutido (`class ConvertingDict(dict)` com `__getitem__` próprio): sem
                // `__iter__` de usuário, quem itera é o valor embutido, nunca o protocolo antigo de sequência.
                None if i.payload.borrow().is_some() => get_iter(&unwrap_payload(v))?,
                // Protocolo antigo de sequência: `__getitem__(0)`, `__getitem__(1)`... um item por passo, até
                // `IndexError`.
                None if i.class().lookup("__getitem__").is_some() => get_iter(&crate::lazy::OldSeqIter::new(v.clone()))?,
                None => return Err(type_error(format!("'{}' object is not iterable", v.type_name()))),
            }
        }
        _ => return Err(type_error(format!("'{}' object is not iterable", v.type_name()))),
    })
}

/// O iterador que o `__iter__` de uma instância devolveu: instância com `__next__` (ou valor embutido
/// guardado) ou um iterador embutido; qualquer outro valor é erro, como em `PyObject_GetIter`.
fn iterator_from_dunder(returned: Value) -> PyResult<PyIter> {
    let non_iterator = || type_error(format!("iter() returned non-iterator of type '{}'", returned.type_name()));
    match &returned {
        Value::Instance(i) if i.class().lookup("__next__").is_none() && i.payload.borrow().is_none() => Err(non_iterator()),
        Value::Instance(_) => Ok(PyIter::Inst(returned)),
        Value::List(_)
        | Value::Tuple(_)
        | Value::Str(_)
        | Value::Range(_)
        | Value::Dict(_)
        | Value::Set(_)
        | Value::Bytes(_)
        | Value::ByteArray(_)
        | Value::Int(_)
        | Value::Big(_)
        | Value::Float(_)
        | Value::Bool(_)
        | Value::None => Err(non_iterator()),
        other => get_iter(other),
    }
}

/// `a < b` com a semântica do Python (usado por `sorted`, `min`, `max`, `list.sort`).
pub fn py_lt(a: &Value, b: &Value) -> PyResult<bool> {
    compare(CmpOp::Lt, a, b)
}

/// Se o `__lt__`/`__eq__`... do tipo embutido de `a` aceita `b`; senão o método devolve `NotImplemented`
/// (`'a'.__lt__(5)`, `(1).__eq__('x')`). `ordering` é falso para `==` e `!=`.
pub(crate) fn rich_compare_accepts(a: &Value, b: &Value, ordering: bool) -> bool {
    let (a, b) = (unwrap_payload(a), unwrap_payload(b));
    match (&a, &b) {
        (Value::Bool(_) | Value::Int(_) | Value::Big(_) | Value::Float(_), Value::Bool(_) | Value::Int(_) | Value::Big(_) | Value::Float(_))
        | (Value::Str(_), Value::Str(_))
        | (Value::Bytes(_) | Value::ByteArray(_), Value::Bytes(_) | Value::ByteArray(_))
        | (Value::List(_), Value::List(_))
        | (Value::Tuple(_), Value::Tuple(_))
        | (Value::Set(_), Value::Set(_)) => true,
        (Value::Dict(_), Value::Dict(_)) | (Value::Range(_), Value::Range(_)) | (Value::None, Value::None) => !ordering,
        _ => false,
    }
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

/// Operador binário `a <op> b` (`op` pelo símbolo: `"+"`, `"-"`, `"*"`, `"/"`, `"//"`, `"%"`, `"**"`).
pub fn py_binary(sym: &str, a: &Value, b: &Value) -> PyResult<Value> {
    py_binary_in(sym, a, b, false)
}

/// `a <op>= b` (ou `a <op> b` com `inplace` falso): o mesmo operador de [`py_binary`], com a variante
/// que muta o operando esquerdo quando ele é mutável (`list +=`, `set |=`).
pub fn py_binary_in(sym: &str, a: &Value, b: &Value, inplace: bool) -> PyResult<Value> {
    let op = match sym {
        "+" => Operator::Add,
        "-" => Operator::Sub,
        "*" => Operator::Mult,
        "/" => Operator::Div,
        "//" => Operator::FloorDiv,
        "%" => Operator::Mod,
        "**" => Operator::Pow,
        "&" => Operator::BitAnd,
        "|" => Operator::BitOr,
        "^" => Operator::BitXor,
        "<<" => Operator::LShift,
        ">>" => Operator::RShift,
        "@" => Operator::MatMult,
        _ => return Err(type_error(format!("unsupported operator {sym}"))),
    };
    binary(op, a, b, inplace)
}

/// Todos os itens de um iterável (também para funções nativas que consomem uma sequência inteira).
pub fn iterate(v: &Value) -> PyResult<Vec<Value>> {
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

/// Marcador de [`Op::LoadMethod`] quando o atributo já veio resolvido: não há objeto a passar.
const NO_SELF: &str = "<no self>";

/// O `__next__` em Python do iterador de usuário que está no topo da pilha de um `for`, com o iterador.
/// O quadro em execução de `run_frames`: o chamado, ou o próprio quadro mais externo quando não há.
fn running<'a>(frame: &'a mut Frame, child: &'a mut Option<Callee>) -> &'a mut Frame {
    match child.as_mut() {
        Some(c) => &mut c.frame,
        None => frame,
    }
}

/// Aplica no chamador o desfecho de um passo da máquina (`Next`): o valor, o salto e o quadro novo (que o
/// laço empilha), ou o erro, que chega ao chamador na instrução que abriu o passo (o `pc` já tinha avançado).
fn land_step(caller: &mut Frame, stepped: PyResult<Next>) -> (Option<Callee>, Option<PyException>) {
    match stepped {
        Ok(next) => {
            let (jump, callee) = next.land(&mut caller.stack);
            if let Some(target) = jump {
                caller.pc = target;
            }
            (callee, None)
        }
        Err(e) => {
            caller.pc -= 1;
            (None, Some(e))
        }
    }
}

fn user_next(stack: &[Slot]) -> Option<(Rc<FuncObj>, Value)> {
    let Some(Slot::Iter(PyIter::Inst(iterator @ Value::Instance(i)))) = stack.last() else { return None };
    match i.class().lookup("__next__") {
        Some(Value::Function(f)) => Some((f, iterator.clone())),
        _ => None,
    }
}

/// O valor `depth` posições abaixo do topo da pilha é uma instância (0 é o topo).
fn instance_at(stack: &[Slot], depth: usize) -> bool {
    stack.len().checked_sub(depth + 1).is_some_and(|n| matches!(&stack[n], Slot::Val(Value::Instance(_))))
}

fn value_at(stack: &[Slot], depth: usize) -> Option<Value> {
    match stack.len().checked_sub(depth + 1).map(|n| &stack[n]) {
        Some(Slot::Val(v)) => Some(v.clone()),
        _ => None,
    }
}

/// O método `name` da classe de `v` quando `v` é instância e ele é uma função Python.
pub(crate) fn dunder_function(v: &Value, name: &str) -> Option<Rc<FuncObj>> {
    let Value::Instance(i) = v else { return None };
    match i.class().lookup(name) {
        Some(Value::Function(f)) => Some(f),
        _ => None,
    }
}

/// O método que decide a verdade da instância no topo da pilha, quando é função Python: `__bool__`, e
/// só sem ele `__len__` (o `bool` do resultado diz qual dos dois).
fn truth_method(stack: &[Slot]) -> Option<(Rc<FuncObj>, bool)> {
    let Some(Slot::Val(Value::Instance(i))) = stack.last() else { return None };
    match i.class().lookup("__bool__") {
        Some(Value::Function(f)) => Some((f, false)),
        Some(_) => None,
        None => match i.class().lookup("__len__") {
            Some(Value::Function(f)) => Some((f, true)),
            _ => None,
        },
    }
}

/// A verdade que `__bool__` devolveu: só `bool` vale.
fn bool_result(returned: &Value) -> PyResult<bool> {
    match returned {
        Value::Bool(b) => Ok(*b),
        other => Err(type_error(format!("__bool__ should return bool, returned {}", other.type_name()))),
    }
}

/// A função de classe que `obj.name(...)` chamaria com `obj` na frente, quando a busca é a comum:
/// instância de classe de usuário, nome ausente do dict da instância, sem `__getattribute__` e sem
/// `__dict__` vivo pendente. Qualquer outro caso fica com a busca completa (`None`).
fn plain_method(obj: &Value, name: &str) -> Option<Rc<FuncObj>> {
    let Value::Instance(inst) = obj else { return None };
    if inst.view.borrow().is_some() || inst.dict.borrow().contains_key(name) {
        return None;
    }
    let class = inst.class();
    let Some(Value::Function(f)) = class.lookup(name) else { return None };
    if f.attrs.borrow().contains_key("__no_bind__") || class.lookup("__getattribute__").is_some() {
        return None;
    }
    // Função de módulo em C guardada na classe (`_default_algorithm = hashlib.sha256`): não se liga.
    if f.is_c_module_function() {
        return None;
    }
    Some(f)
}

/// Estado do interpretador. Todos os campos são compartilhados (`Rc`), então clonar a `Vm` é barato
/// e as cópias enxergam o mesmo estado: é assim que um gerador se retoma sozinho e que as funções
/// livres (`binary`, `compare`, `repr`...) chamam de volta o Python (`__add__`, `__repr__`...).
#[derive(Clone)]
pub struct Vm {
    pub(crate) globals: Rc<RefCell<crate::object::VarMap>>,
    /// Buffer do stdout, descarregado pelo chamador no fim.
    pub stdout: Rc<RefCell<Vec<u8>>>,
    /// O que foi escrito no stderr sem pseudo-processo (o interpretador embutido nos testes): vai para o
    /// `Outcome.stderr`, antes do traceback final.
    pub stderr_capture: RefCell<String>,
    /// Exceções sendo tratadas (a mais recente por último), para `raise` sem argumento.
    pub(crate) handled: Rc<RefCell<Vec<Value>>>,
    /// Profundidade de chamadas de função em andamento.
    pub(crate) depth: Rc<std::cell::Cell<usize>>,
    /// Linha da instrução em execução (para `sys._getframe` e `warnings`).
    pub(crate) cur_line: Rc<std::cell::Cell<usize>>,
    /// Funções em andamento (a mais interna por último), cada uma com a linha do chamador e o
    /// escopo das variáveis locais da chamada.
    pub(crate) frames: Rc<RefCell<Vec<(Rc<Code>, usize, Rc<Env>)>>>,
    /// Os quadros de função suspensos esperando o retorno de um chamado (o mais interno por último).
    /// Cada `run_loop` só mexe do índice em que entrou para cima; o quadro em execução fica com ele.
    pub(crate) frames_stack: Rc<RefCell<Vec<Callee>>>,
    /// Quantos `run_loop` estão ativos na pilha Rust desta thread. Vale 1 quando só o laço mais externo
    /// roda (o estado todo está em dados e o `os.fork` pode copiá-lo); com mais, há recursão Rust viva
    /// (callback de `sorted(key=)`, retomada de gerador...) que a imagem do heap não alcança.
    pub(crate) rust_nest: Rc<std::cell::Cell<usize>>,
    /// `sys.argv`.
    pub(crate) argv: Rc<Vec<String>>,
    /// `sys.stdin`, `sys.stdout` e `sys.stderr`, criados uma vez.
    pub(crate) std_files: [Rc<RefCell<Native>>; 3],
    /// Módulos já importados, por nome.
    pub(crate) modules: Rc<RefCell<HashMap<String, Rc<crate::object::ModuleObj>>>>,
    /// Entradas de `sys.modules` que não são módulos (a instância de uma subclasse de
    /// `types.ModuleType`, o importador do `six`...): `import nome` devolve o próprio objeto.
    pub(crate) foreign_modules: Rc<RefCell<HashMap<String, Value>>>,
    /// Globais vivas dos módulos carregados de arquivo (por nome): `mod.x` lê e grava aqui, então
    /// o módulo e quem o importou enxergam o mesmo estado.
    pub(crate) module_globals: Rc<RefCell<HashMap<&'static str, Rc<RefCell<crate::object::VarMap>>>>>,
}

/// Sobe `Vm::rust_nest` na entrada de um `run_loop` e desce na saída, inclusive por unwind.
struct NestGuard(Rc<std::cell::Cell<usize>>);

impl NestGuard {
    fn enter(counter: &Rc<std::cell::Cell<usize>>) -> NestGuard {
        counter.set(counter.get() + 1);
        NestGuard(counter.clone())
    }
}

impl Drop for NestGuard {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
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

/// Instala `vm` como a `Vm` desta thread: é o que o filho de um `os.fork` faz ao nascer.
pub(crate) fn set_current(vm: &Vm) {
    CURRENT.with(|c| *c.borrow_mut() = Some(vm.clone()));
}

/// Limite de recursão (`sys.getrecursionlimit()` do CPython).
const MAX_DEPTH: usize = 1000;

/// O estado de sinais do processo Python (fatia F2): se o programa registrou um tratador (ou armou um
/// timer de `signal.alarm`/`setitimer`), a VM passa a consultar os sinais capturados. Cada processo tem a sua
/// thread do interpretador, então o estado é por thread. Os timers moram no kernel: o `fork(2)` os zera no
/// filho e o `execve` os mantém.
pub(crate) struct SignalState {
    armed: Cell<bool>,
    /// Uma nativa mandou um sinal ao próprio processo (`os.kill`): a próxima instrução já consulta os
    /// capturados, sem esperar o intervalo de consulta (o ponto de verificação do CPython logo depois da chamada).
    now: Cell<bool>,
}

thread_local! {
    static SIGNALS: SignalState = const { SignalState { armed: Cell::new(false), now: Cell::new(false) } };
}

/// Pede que o laço consulte os sinais capturados na próxima instrução.
pub(crate) fn request_signal_check() {
    SIGNALS.with(|s| s.now.set(true));
}

/// A volta `tick` do laço é a de consultar os sinais capturados (a cada 8192 instruções, ou quando pedido).
fn signal_check_due(tick: u32) -> bool {
    tick & 0x1fff == 0 || SIGNALS.with(|s| s.now.replace(false))
}

/// O programa registrou algum tratador de sinal?
pub(crate) fn signals_armed() -> bool {
    SIGNALS.with(|s| s.armed.get())
}

/// Liga a consulta dos sinais capturados (um tratador foi registrado, ou um timer foi armado).
pub(crate) fn arm_signals() {
    SIGNALS.with(|s| s.armed.set(true));
}

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

/// O estado de um quadro de execução, tudo o que o `run_loop` lê e escreve: o código, o ambiente,
/// a pilha de valores, os blocos protegidos e o `pc`. Um gerador o guarda entre um `yield` e o
/// `next` seguinte.
pub(crate) struct Frame {
    pub(crate) code: Rc<Code>,
    pub(crate) env: Rc<Env>,
    pub(crate) stack: Vec<Slot>,
    pub(crate) blocks: Vec<Block>,
    pub(crate) pc: usize,
    /// As exceções em tratamento dentro do quadro enquanto ele está suspenso (só os geradores
    /// suspendem; num quadro em execução elas vivem em `Vm::handled`).
    pub(crate) handled: Vec<Value>,
    /// A altura de `Vm::handled` de fora no momento em que o quadro se suspendeu.
    pub(crate) handled_base: usize,
}

impl Frame {
    pub(crate) fn new(code: Rc<Code>, env: Rc<Env>) -> Frame {
        // A pilha de quase todo quadro cabe em poucas posições: uma alocação só, sem crescer aos poucos.
        Frame { code, env, stack: Vec::with_capacity(8), blocks: Vec::new(), pc: 0, handled: Vec::new(), handled_base: 0 }
    }
}

/// O que a abertura de uma chamada guardou para o fechamento dela (`Vm::end_call`).
pub(crate) struct CallLink {
    pub(crate) func: Option<Rc<FuncObj>>,
    /// A linha do chamador, devolvida a `cur_line` na saída.
    pub(crate) caller_line: usize,
    /// A altura de `Vm::handled` na entrada: `return` dentro de um `except` não fecha o tratador.
    pub(crate) handled_len: usize,
    pub(crate) profiled: bool,
    /// As globais do chamador, quando a função é de outro módulo: voltam a `Vm::globals` no fechamento.
    pub(crate) caller_globals: Option<Rc<RefCell<crate::object::VarMap>>>,
    /// A instância de `Classe(...)` cujo `__init__` este quadro executa: é ela que a chamada entrega.
    pub(crate) instance: Option<Value>,
    /// Quando o quadro executa o `__next__` de um `for`: o destino do `ForIter`, para onde o laço
    /// salta (descartando o iterador) se o chamado terminar com `StopIteration`.
    pub(crate) on_stop: Option<usize>,
    /// Quando o quadro executa o método mágico de um operador, de uma subscrição ou de um teste de
    /// verdade: o que fazer com o valor que ele devolve (`Vm::finish_dunder`).
    pub(crate) then: Option<Dunder>,
    /// Quando o quadro é o de um gerador ou de uma corrente retomado pelo laço (sem `func`): o que fechar
    /// a retomada e o que fazer com o valor entregue.
    pub(crate) resuming: Option<crate::generator::Resuming>,
}

/// Uma tentativa de uma cadeia de operador: o método mágico `name` de `recv`, com `other` de argumento.
pub(crate) struct Attempt {
    pub(crate) recv: Value,
    pub(crate) name: &'static str,
    pub(crate) other: Value,
    /// O resultado vale ao contrário (`!=` derivado de um `__eq__`).
    pub(crate) invert: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum ChainKind {
    Binary { op: Operator, inplace: bool },
    /// `<`, `<=`, `>`, `>=`.
    Order(CmpOp),
    /// `==` e `!=`.
    Equal(CmpOp),
}

/// Um operador com método mágico em Python: as tentativas que faltam (a de `NotImplemented` passa à
/// seguinte) e o que vale quando todas se esgotam.
pub(crate) struct Chain {
    pub(crate) kind: ChainKind,
    pub(crate) a: Value,
    pub(crate) b: Value,
    /// As tentativas restantes, a próxima por último.
    pub(crate) rest: Vec<Attempt>,
    /// `invert` da tentativa em curso.
    pub(crate) invert: bool,
}

/// O que um salto condicional ou um `not` faz com a verdade do operando.
#[derive(Clone, Copy)]
pub(crate) enum TruthUse {
    /// `PopJumpIf*`: o operando já saiu da pilha; salta quando a verdade é `when`.
    Jump { target: usize, when: bool },
    /// `JumpIf*OrPop`: o operando continua na pilha; salta quando a verdade é `when`, senão ele sai.
    JumpKeep { target: usize, when: bool },
    Not,
}

/// O que fazer com o valor que o quadro de um método mágico devolve.
pub(crate) enum Dunder {
    Chain(Chain),
    /// `a[i]`: o valor vai para a pilha.
    Push,
    /// `a[i] = v`: o valor é descartado.
    Discard,
    /// `x in c` / `x not in c`, e o `__exit__` de um `with`: a verdade do valor (negada em `not in`).
    Boolean { negate: bool },
    /// `__bool__` (ou `__len__`, quando `len`) de um teste de verdade.
    Truth { how: TruthUse, len: bool },
    /// `__iter__` de um `for`: o iterador vai para a pilha.
    Iter,
    /// Um callback Python de uma nativa (a função do `map`, o predicado do `filter`, a chave de
    /// `min`/`max`/`sorted`, o `__next__` ou o `__getitem__` de uma fonte, o `__iter__` de uma coleta): o valor
    /// que ele devolve volta à máquina de `Vm::drive` (`Vm::resume_callback`).
    Callback(Box<Callback>),
    /// O `signal._dispatch` de sinais capturados, empilhado entre duas instruções do chamador: o valor é
    /// descartado e a instrução interrompida roda quando o quadro volta. Uma exceção do tratador sobe no
    /// ponto interrompido (o `pc` do chamador não avançou, então não recua).
    Signal,
    /// O corpo de um módulo importado: ao fechar, o módulo é concluído (ou removido de `sys.modules`, se o corpo
    /// levantou) e a cadeia do `import` segue (`Vm::close_import`).
    Import(Box<crate::modrun::ImportRun>),
    /// O código de um `exec` ou `eval`: ao fechar, as globais e o `locals` recebem o que ele criou e o valor sai
    /// (`Vm::close_exec`).
    Exec(Box<crate::builtins_ext::ExecRun>),
}

impl Dunder {
    /// O quadro é um corpo de módulo ou de `exec`/`eval`: o código acaba sem `Return` e devolve `None`.
    fn is_body(&self) -> bool {
        matches!(self, Dunder::Import(_) | Dunder::Exec(_))
    }
}

/// Como um quadro terminou, para o laço aplicar no quadro do chamador: uma chamada comum (o resultado, o
/// destino do `for` e o método mágico em curso), a retomada de um gerador (o núcleo, o que fazer com o
/// desfecho e o desfecho) ou um corpo de módulo (o `Dunder::Import` ou `Dunder::Exec`, o fecho da chamada, o
/// escopo do quadro e o resultado).
enum Closed {
    Call(PyResult<Value>, Option<usize>, Option<Dunder>),
    Resumed(Rc<GenCore>, ResumeUse, PyResult<Resumed>),
    Body(Dunder, CallLink, Rc<Env>, PyResult<Value>),
}

/// O início de uma retomada de gerador no laço: o quadro a executar, ou o desfecho que já saiu pronto
/// (gerador terminado), com o `ResumeUse` devolvido.
enum Resumption {
    Frame(Callee),
    Ready(Resumed, ResumeUse),
}

/// O desfecho de `Vm::inject_delegated`: a exceção a levantar no gerador de fora, o desfecho já aplicado à pilha
/// dele, ou o quadro do sub-gerador a executar (o de fora espera em `frames_stack`).
enum Injected {
    Raise(PyException),
    Applied,
    Sub(Callee),
}

/// O que o laço pede para entregar um item a um consumidor (`Vm::fetch`): o quadro de um gerador a executar,
/// ou o item (`None` é o fim da fonte) que saiu sem rodar quadro, com o consumidor devolvido.
enum Fetch {
    Frame(Callee),
    Ready(Option<Value>, ResumeUse),
}

/// O que acontece numa cadeia preguiçosa (`Vm::drive`): pedir um item a uma fonte, ou receber um item
/// ou o fim dela.
enum Event {
    Want(Rc<dyn crate::object::ExtObject>),
    Got(Value),
    Ended,
}

/// O que entra na conta de uma coleta (`Vm::collect_run`): o item seguinte, o fim da fonte, ou a chave que um
/// quadro acabou de devolver para o item.
enum Input {
    Item(Value),
    End,
    Key(Value, Value),
}

/// Onde a fonte de uma coleta começa: já é o objeto de que o laço puxa, ou é uma instância cujo `__iter__` em
/// Python o laço executa antes (`Callback` com `CallbackKind::Start`).
enum Root {
    Ready(Value),
    Iterable(Rc<FuncObj>, Value),
}

/// O núcleo de gerador ou de corrente que a posição da pilha guarda (iterador de `for`, sub-iterador de
/// `yield from`, aguardável de `await`).
fn slot_core(slot: &Slot) -> Option<Rc<GenCore>> {
    match slot {
        Slot::Iter(PyIter::Ext(x)) | Slot::Val(Value::Ext(x)) => crate::generator::core_of(x),
        _ => None,
    }
}

/// A cadeia de `enumerate`/`zip`/`map`/`filter` sobre gerador que a posição da pilha guarda como iterador
/// de `for`: o laço a puxa em quadros, camada por camada (`Vm::drive`).
fn slot_chain(slot: &Slot) -> Option<Value> {
    match slot {
        Slot::Iter(PyIter::Ext(x)) => chain_root(&Value::Ext(x.clone())).then(|| Value::Ext(x.clone())),
        _ => None,
    }
}

/// `v` é uma cadeia ou uma fonte que precisa de quadro para dar o próximo item (gerador no fundo, callback
/// ou método de usuário em Python), mas não é um gerador (o gerador sozinho não conta).
fn chain_root(v: &Value) -> bool {
    matches!(v, Value::Ext(x) if crate::generator::core_of(x).is_none() && crate::lazy::needs_frames(x))
}

/// `v` é um gerador, ou uma cadeia ou fonte que precisa de quadro: o que o laço de quadros esgota ou puxa
/// sem recursar.
fn suspendable(v: &Value) -> bool {
    matches!(v, Value::Ext(x) if crate::lazy::needs_frames(x))
}

/// O que sai de `Vm::pull` fora de um laço de instruções: o valor, ou o quadro a executar.
pub(crate) fn entered_of(next: Next) -> PyResult<Entered> {
    match next {
        Next::Value(v) => Ok(Entered::Done(v)),
        Next::Nothing => Ok(Entered::Done(Value::None)),
        Next::Spawn(callee) => Ok(Entered::Frame(callee)),
        _ => Err(internal("a pull outside a loop cannot jump")),
    }
}

/// O desfecho de um método mágico, para o laço de instruções aplicar no quadro do chamador.
pub(crate) enum Next {
    Value(Value),
    Iter(PyIter),
    Nothing,
    /// Tira o topo da pilha (o operando que um `JumpIf*OrPop` guardou).
    Drop,
    Jump(usize),
    /// O `for` acabou: descarta o iterador (o topo da pilha) e salta para o fim do laço.
    Exit(usize),
    Spawn(Callee),
}

impl Next {
    /// Aplica na pilha o que ela mesma resolve; o salto e o quadro novo ficam para o chamador.
    fn land(self, stack: &mut Vec<Slot>) -> (Option<usize>, Option<Callee>) {
        match self {
            Next::Value(v) => stack.push(Slot::Val(v)),
            Next::Iter(it) => stack.push(Slot::Iter(it)),
            Next::Drop => {
                stack.pop();
            }
            Next::Nothing => {}
            Next::Jump(t) => return (Some(t), None),
            Next::Exit(t) => {
                stack.pop();
                return (Some(t), None);
            }
            Next::Spawn(c) => return (None, Some(c)),
        }
        (None, None)
    }
}

impl Chain {
    /// O resultado do operador quando a tentativa em curso devolveu `v` (que não é `NotImplemented`).
    fn conclude(&self, v: Value) -> Value {
        match self.kind {
            ChainKind::Binary { .. } => v,
            ChainKind::Order(_) | ChainKind::Equal(_) => Value::Bool(v.is_true() != self.invert),
        }
    }

    /// Todas as tentativas devolveram `NotImplemented` (ou não existiam): vale o operador embutido.
    fn fallback(&self) -> PyResult<Value> {
        match self.kind {
            ChainKind::Binary { op, inplace } => binary_native(op, &self.a, &self.b, inplace),
            ChainKind::Order(op) => compare_native(op, &self.a, &self.b).map(Value::Bool),
            ChainKind::Equal(op) => {
                let equal = crate::classes::payload_eq(&self.a, &self.b).unwrap_or_else(|| crate::object::py_eq_native(&self.a, &self.b));
                Ok(Value::Bool(equal != (op == CmpOp::NotEq)))
            }
        }
    }
}

/// Um quadro de função aberto por uma chamada, com o necessário para fechá-la.
pub(crate) struct Callee {
    pub(crate) frame: Frame,
    pub(crate) link: CallLink,
}

/// O resultado de iniciar uma chamada: o valor já pronto, ou o quadro a executar.
pub(crate) enum Entered {
    Done(Value),
    Frame(Callee),
}

pub(crate) fn internal(msg: &str) -> PyException {
    exc("SystemError", msg.to_string())
}

/// O escopo da célula de alvos de uma compreensão inline, guardado em `locals` sob `key`.
fn cell_scope(locals: &Env, key: &str) -> PyResult<Rc<Env>> {
    locals.vars.borrow().get(key).and_then(crate::classes::cell_env).ok_or_else(|| internal("comprehension cell is missing"))
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

    /// A saída do interpretador, até o `atexit`: finaliza o que a última instrução soltou e roda as funções
    /// registradas em `atexit` (se o módulo foi importado). A chamada é o laço mais externo desta fase
    /// (`MainGuard`), como `run` é o do programa: um `os.fork` dentro de uma função de `atexit` copia o estado e
    /// o filho a retoma (`fork::RunTail::verdict`). O resto da saída é `finalize_at_exit`.
    pub(crate) fn run_exit_hooks(&mut self) {
        self.run_finalizers();
        let hook = self
            .modules
            .borrow()
            .get("atexit")
            .and_then(|m| m.attrs.borrow().get("_run_exitfuncs").cloned());
        if let Some(f) = hook {
            let _main = crate::fork::MainGuard::enter(self.fork_entry());
            let _ = self.call(&f, Vec::new(), Vec::new());
        }
    }

    /// A contabilidade do laço mais externo de uma fase do programa (o que o filho de um `os.fork` restaura).
    fn fork_entry(&self) -> crate::fork::Entry {
        crate::fork::Entry { depth: self.depth.get(), frames: self.frames.borrow().len(), line: self.cur_line.get() }
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
            globals: Rc::new(RefCell::new(crate::object::VarMap::from_iter([("__name__".into(), Value::str("__main__"))]))),
            stdout: Rc::new(RefCell::new(Vec::new())),
            stderr_capture: RefCell::new(String::new()),
            handled: Rc::new(RefCell::new(Vec::new())),
            depth: Rc::new(std::cell::Cell::new(0)),
            cur_line: Rc::new(std::cell::Cell::new(0)),
            frames: Rc::new(RefCell::new(Vec::new())),
            frames_stack: Rc::new(RefCell::new(Vec::new())),
            rust_nest: Rc::new(std::cell::Cell::new(0)),
            argv: Rc::new(argv),
            modules: Rc::new(RefCell::new(HashMap::new())),
            foreign_modules: Rc::new(RefCell::new(HashMap::new())),
            module_globals: Rc::new(RefCell::new(HashMap::new())),
            std_files: [file(FileKind::Stdin, "<stdin>"), file(FileKind::Stdout, "<stdout>"), file(FileKind::Stderr, "<stderr>")],
        };
        CURRENT.with(|c| *c.borrow_mut() = Some(vm.clone()));
        // O script principal como módulo: o `__main__` enxerga as globais dele de qualquer módulo.
        vm.module_globals.borrow_mut().insert("__main__", vm.globals.clone());
        vm
    }


    /// Executa o código de um módulo.
    pub fn run(&mut self, code: &Rc<Code>) -> Result<(), RuntimeError> {
        self.run_body(code, false)
    }

    /// `run` do corpo de um módulo embutido: com `listed`, o quadro entra em `frames` enquanto o corpo roda, como o
    /// quadro `<module>` de um módulo importado no CPython (é dele que `sys._getframe(1)` tira o `__name__` de quem
    /// chamou, por exemplo no `namedtuple` chamado no nível do módulo).
    pub(crate) fn run_body(&mut self, code: &Rc<Code>, listed: bool) -> Result<(), RuntimeError> {
        let env = Env::new(None, false, true);
        // O laço mais externo do programa: é o único onde o `os.fork` copia o estado e o filho retoma.
        let entry = self.fork_entry();
        let result = {
            let _main = crate::fork::MainGuard::enter(entry);
            let depth = self.frames.borrow().len();
            if listed {
                let caller_line = self.cur_line.get();
                self.frames.borrow_mut().push((code.clone(), caller_line, env.clone()));
                crate::frameobj::bind_globals(&env, &self.globals);
            }
            let result = self.exec(code, &env);
            if listed {
                crate::frameobj::unbind_globals(&env);
                self.frames.borrow_mut().truncate(depth);
            }
            result
        };
        run_outcome(result)
    }

    /// Executa o código de um módulo, de uma função ou de um corpo de classe até o `Return`.
    pub(crate) fn exec(&mut self, code: &Rc<Code>, env: &Rc<Env>) -> PyResult<Value> {
        let mut frame = Frame::new(code.clone(), env.clone());
        self.run_frame(&mut frame)
    }

    /// Executa um quadro até o `Return`.
    pub(crate) fn run_frame(&mut self, frame: &mut Frame) -> PyResult<Value> {
        match self.run_loop(frame, None)? {
            Exit::Return(v) => Ok(v),
            Exit::Yield(_) => Err(internal("yield outside generator")),
        }
    }

    /// O laço de instruções, com o estado do quadro (pilha, blocos protegidos, `pc`) vindo de fora:
    /// um gerador guarda esse estado entre um `yield` e o `next` seguinte. As chamadas a funções
    /// Python simples não recursam: o quadro do chamado vira o quadro em execução e o do chamador
    /// espera em `frames_stack` (o quadro recebido de fora é sempre o mais externo).
    pub(crate) fn run_loop(&mut self, frame: &mut Frame, inject: Option<PyException>) -> PyResult<Exit> {
        let mark = self.frames_stack.borrow().len();
        let (depth0, frames0, line0) = (self.depth.get(), self.frames.borrow().len(), self.cur_line.get());
        let globals0 = self.globals.clone();
        // A guarda desce o contador também quando um desvio do kernel (`_exit`) desempilha a thread.
        let _nest = NestGuard::enter(&self.rust_nest);
        let result = self.run_frames(frame, inject, mark, None);
        // Uma saída com erro interno deixa chamados abertos: a contabilidade volta ao que era na entrada.
        if self.depth.get() != depth0 {
            self.depth.set(depth0);
            self.frames.borrow_mut().truncate(frames0);
            self.cur_line.set(line0);
        }
        self.globals = globals0;
        self.frames_stack.borrow_mut().truncate(mark);
        result
    }

    /// O laço mais externo do filho de um `os.fork`: retoma na instrução seguinte à chamada que criou o
    /// processo, com `0` no lugar do resultado dela. `outer` é o quadro do programa, `suspended` os
    /// quadros que esperavam em `frames_stack` e `child` o que estava em execução (`None`: o próprio
    /// `outer`). A contabilidade de saída volta ao que `Vm::run` tinha na entrada (`entry`).
    pub(crate) fn run_resumed(
        &mut self,
        outer: &mut Frame,
        suspended: Vec<Callee>,
        mut child: Option<Callee>,
        entry: crate::fork::Entry,
    ) -> PyResult<Exit> {
        *self.frames_stack.borrow_mut() = suspended;
        let running = match child.as_mut() {
            Some(c) => &mut c.frame,
            None => &mut *outer,
        };
        running.stack.push(Slot::Val(Value::Int(0)));
        let _nest = NestGuard::enter(&self.rust_nest);
        let result = self.run_frames(outer, None, 0, child);
        if self.depth.get() != entry.depth {
            self.depth.set(entry.depth);
            self.frames.borrow_mut().truncate(entry.frames);
            self.cur_line.set(entry.line);
        }
        self.frames_stack.borrow_mut().clear();
        result
    }

    fn run_frames(&mut self, frame: &mut Frame, inject: Option<PyException>, mark: usize, start: Option<Callee>) -> PyResult<Exit> {
        let mut pending = inject;
        let mut signal_tick: u32 = 0;
        // `throw` num gerador parado numa delegação (`await`/`yield from`) vai para o sub-iterador.
        if pending.is_some() {
            let Frame { code, stack, pc, .. } = &mut *frame;
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
        // O chamado em execução; `None` é o próprio `frame`. O filho de um `os.fork` entra com o que o pai tinha.
        let mut child: Option<Callee> = start;
        loop {
            // O que a instrução decide e só pode ser aplicado depois de soltar o empréstimo do quadro.
            let mut spawn: Option<Callee> = None;
            let mut suspend: Option<crate::fork::SuspendRequest> = None;
            let mut finish: Option<PyResult<Value>> = None;
            // `finish` é um `yield` (e não um `return`) de um gerador retomado pelo laço.
            let mut yielded = false;
            let is_child = child.is_some();
            let resuming = child.as_ref().is_some_and(|c| c.link.resuming.is_some());
            // O quadro de um corpo de módulo ou de `exec`/`eval` acaba sem `Return`, ao fim do código.
            let body_frame = child.as_ref().is_some_and(|c| c.link.then.as_ref().is_some_and(Dunder::is_body));
            let cur: &mut Frame = match child.as_mut() {
                Some(c) => &mut c.frame,
                None => &mut *frame,
            };
            let Frame { code, env, stack, blocks, pc, .. } = cur;
            let code: &Rc<Code> = code;
            let env: &Rc<Env> = env;
            let ended = *pc >= code.ops.len();
            if ended {
                if is_child && !body_frame {
                    return Err(internal("function code without return"));
                }
                if !is_child {
                    // O quadro mais externo de uma thread verde é um marcador: a função da thread acaba
                    // sempre em `_gt_finish`, nunca devolvendo para ele. Só o laço mais externo (`rust_nest == 1`) tem
                    // o marcador: o corpo de um módulo importado dentro da thread (`run_loop` aninhado) acaba sem
                    // `Return`, ao fim do código, e devolve normalmente.
                    if crate::gthread::in_secondary() && self.rust_nest.get() == 1 {
                        return Err(internal("thread function returned without finishing"));
                    }
                    return Ok(Exit::Return(Value::None));
                }
                // Corpo de módulo como quadro do laço: o fim do código devolve `None`, como o `Return` de uma função.
                stack.push(Slot::Val(Value::None));
            }
            let op = if ended { Op::Return } else { code.ops[*pc] };
            if !ended {
                self.cur_line.set(code.lines[*pc]);
            }
            // Ponto seguro da finalização: logo depois da instrução que soltou a última referência de um
            // objeto com `__del__`, o `__del__` roda (nunca com uma exceção em voo).
            if crate::finalize::pending() && pending.is_none() {
                self.run_finalizers();
            }
            // Sinais capturados chegam entre instruções, como no CPython (só depois de um `signal.signal`). O
            // tratador ganha quadro neste laço (o quadro em execução espera em `frames_stack`), então um `fork`
            // ou uma troca de thread dentro dele enxergam o estado todo em dados. Vem antes do evento `line`:
            // a instrução interrompida o dispara uma vez só, quando o tratador voltar.
            if signals_armed() && pending.is_none() && !ended {
                signal_tick = signal_tick.wrapping_add(1);
                if signal_check_due(signal_tick) {
                    match self.signal_frame() {
                        Ok(callee) => spawn = callee,
                        Err(e) => pending = Some(e),
                    }
                }
            }
            // A fatia de tempo da thread verde que roda venceu: o `_gsched.preempt` ganha quadro aqui, como um
            // tratador de sinal, e passa a vez a outra thread pronta (a troca da GIL no fim do `switchinterval`).
            if pending.is_none() && spawn.is_none() && !ended && self.rust_nest.get() == 1 && crate::gthread::slice_expired() {
                match self.preempt_frame() {
                    Ok(callee) => spawn = callee,
                    Err(e) => pending = Some(e),
                }
            }
            // A volta de laço (salto para trás para a linha do cabeçalho) herda a linha da última
            // instrução do corpo no CPython: não abre linha nova, e o `line` do alvo vem do `back_edge`.
            // O `continue` tem linha própria, que gera `line`.
            let backward_jump = matches!(op, Op::Jump(t) if (t as usize) < *pc && code.lines[t as usize] == code.lines[*pc]);
            if crate::tracing::active() && pending.is_none() && spawn.is_none() && !backward_jump && !ended {
                // O rastreador pode mover o quadro (`frame.f_lineno = n`, o `jump` do pdb): a execução
                // continua na primeira instrução da linha pedida, sem um novo evento `line` para ela.
                let at = Rc::as_ptr(env) as usize;
                crate::frameobj::open_jump(at);
                let traced = crate::tracing::line(self, code, code.lines[*pc]);
                let target = crate::frameobj::close_jump(at);
                match traced {
                    Err(e) => pending = Some(e),
                    Ok(()) => {
                        if let Some(index) = target.and_then(|line| jump_index(code, line)) {
                            crate::frameobj::line_landed(at, code.lines[index]);
                            *pc = index;
                            continue;
                        }
                    }
                }
            }
            // As escritas pela visão de `globals()` entram na tabela antes da instrução.
            if crate::globalsview::ARMED.load(std::sync::atomic::Ordering::Relaxed) {
                crate::globalsview::sync_pull();
            }
            let result = if let Some(e) = pending.take() {
                Err(e)
            } else if spawn.is_some() {
                // O tratador de um sinal acabou de ganhar quadro: a instrução interrompida roda quando ele voltar.
                Ok(Some(*pc))
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
                        Some(Slot::Val(v)) if is_child => {
                            crate::frameobj::finish(code, env);
                            finish = Some(Ok(v));
                            Ok(None)
                        }
                        Some(Slot::Val(v)) => {
                            crate::frameobj::finish(code, env);
                            return Ok(Exit::Return(v));
                        }
                        _ => Err(internal("bad value stack")),
                    },
                    Op::Yield => match stack.pop() {
                        // Gerador retomado pelo laço: o quadro volta ao gerador e o valor ao chamador (o fecho
                        // fica depois do `match`, onde o quadro já não está emprestado).
                        Some(Slot::Val(v)) if resuming => {
                            yielded = true;
                            finish = Some(Ok(v));
                            Ok(None)
                        }
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
                    Op::Jump(t) => {
                        if (t as usize) < *pc && crate::tracing::active() {
                            crate::tracing::back_edge(self, code);
                        }
                        Ok(Some(t as usize))
                    }
                    Op::Pop => {
                        stack.pop();
                        Ok(None)
                    }
                    Op::StoreName(i) => match stack.pop() {
                        Some(Slot::Val(v)) => {
                            let name = &code.names[i as usize];
                            let armed = crate::globalsview::ARMED.load(std::sync::atomic::Ordering::Relaxed);
                            let copy = armed.then(|| v.clone());
                            {
                                let mut g = self.globals.borrow_mut();
                                match g.get_mut(name) {
                                    Some(slot) => *slot = v,
                                    None => {
                                        g.insert(name.clone(), v);
                                    }
                                }
                            }
                            if let Some(c) = copy {
                                crate::globalsview::push(&self.globals, name, Some(&c));
                            }
                            Ok(None)
                        }
                        _ => Err(internal("bad value stack")),
                    },
                    Op::ForIter(t) => match user_next(stack) {
                        // `__next__` escrito em Python: o quadro dele é empilhado, sem recursão.
                        Some((next_fn, iterator)) => match self.enter_function(&next_fn, vec![iterator], Vec::new()) {
                            Ok(Entered::Done(v)) => {
                                stack.push(Slot::Val(v));
                                Ok(None)
                            }
                            Ok(Entered::Frame(mut c)) => {
                                c.link.on_stop = Some(t as usize);
                                spawn = Some(c);
                                Ok(None)
                            }
                            Err(e) => Err(e),
                        },
                        // Gerador retomado pelo `for`: o quadro dele é empilhado, sem recursão.
                        None => match stack.last().and_then(slot_core) {
                            Some(core) => self.resume_op(code, stack, &core, None, ResumeUse::ForIter { exit: t as usize }, &mut spawn),
                            // `enumerate`/`zip`/`map`/`filter` sobre gerador: as camadas descem em quadros.
                            None if stack.last().and_then(slot_chain).is_some() => {
                                let Some(root) = stack.last().and_then(slot_chain) else { return Err(internal("chain vanished")) };
                                self.pull(&root, ResumeUse::ForIter { exit: t as usize }).map(|next| {
                                    let (jump, callee) = next.land(stack);
                                    spawn = callee;
                                    jump
                                })
                            }
                            None => match stack.last_mut() {
                                Some(Slot::Iter(it)) => {
                                    // O `for` sobre o stdin pode parar por falta de entrada: a espera roda em quadros e o
                                    // `FOR_ITER` se repete (o iterador continua na pilha).
                                    let reads_stdin = matches!(&*it, PyIter::Native(_));
                                    if reads_stdin {
                                        crate::stdin::set_replayable(true);
                                    }
                                    let next = it.next();
                                    if reads_stdin {
                                        crate::stdin::set_replayable(false);
                                    }
                                    match next {
                                    Ok(Some(v)) => {
                                        stack.push(Slot::Val(v));
                                        Ok(None)
                                    }
                                    Ok(None) => {
                                        // Gerador que terminou com `return valor`: o `FOR_ITER` vê o `StopIteration`.
                                        let stop = crate::generator::take_stop_value(it);
                                        stack.pop();
                                        match stop {
                                            Some(v) => crate::tracing::exception(self, code, &crate::generator::stop_iteration(v))
                                                .map(|()| Some(t as usize)),
                                            None => Ok(Some(t as usize)),
                                        }
                                    }
                                    Err(e) if reads_stdin && crate::fork::is_suspend(&e) => match crate::fork::take_request() {
                                        // O stdin parou o `for`: o quadro de espera roda e o `FOR_ITER` se repete no mesmo `pc`.
                                        crate::fork::SuspendRequest::Wait(fd) => self.stdin_wait_frame(fd, None).map(|mut callee| {
                                            callee.link.then = Some(Dunder::Signal);
                                            spawn = Some(callee);
                                            Some(*pc)
                                        }),
                                        request => {
                                            suspend = Some(request);
                                            Ok(None)
                                        }
                                    },
                                    Err(e) => Err(e),
                                    }
                                }
                                _ => Err(internal("FOR_ITER without iterator")),
                            },
                        },
                    },
                    // `yield from` e `await` sobre um gerador ou uma corrente: o sub-quadro é empilhado.
                    Op::Delegate(end) if stack.len().checked_sub(2).and_then(|i| slot_core(&stack[i])).is_some() => {
                        let core = stack.len().checked_sub(2).and_then(|i| slot_core(&stack[i]));
                        match (stack.pop(), core) {
                            (Some(Slot::Val(sent)), Some(core)) => {
                                self.resume_op(code, stack, &core, Some(sent), ResumeUse::Delegate { end: end as usize }, &mut spawn)
                            }
                            _ => Err(internal("bad value stack")),
                        }
                    }
                    // Métodos mágicos de operador, subscrição e verdade que são funções Python: o quadro
                    // deles é empilhado, sem recursão.
                    Op::PopJumpIfFalse(_) | Op::PopJumpIfTrue(_) | Op::JumpIfFalseOrPop(_) | Op::JumpIfTrueOrPop(_) | Op::Unary(UnaryOp::Not)
                        if truth_method(stack).is_some() =>
                    {
                        self.dunder_op(code, op, stack, env, &mut spawn)
                    }
                    Op::Binary { .. } | Op::Compare(_) if instance_at(stack, 0) || instance_at(stack, 1) => {
                        self.dunder_op(code, op, stack, env, &mut spawn)
                    }
                    Op::Subscript | Op::StoreSubscript | Op::DeleteSubscript if instance_at(stack, 1) => {
                        self.dunder_op(code, op, stack, env, &mut spawn)
                    }
                    // Unários, `iter()` de um `for`, gravação e remoção de atributo e abertura de `with`
                    // sobre instância: o método mágico em Python também ganha quadro.
                    Op::Unary(UnaryOp::USub | UnaryOp::UAdd | UnaryOp::Invert)
                    | Op::GetIter
                    | Op::StoreAttr(_)
                    | Op::DeleteAttr(_)
                    | Op::WithEnter
                    | Op::AsyncWithEnter
                        if instance_at(stack, 0) =>
                    {
                        self.dunder_op(code, op, stack, env, &mut spawn)
                    }
                    // `__exit__` / `__aexit__` ligados a função Python, chamados com a exceção em curso.
                    Op::WithExcept | Op::AsyncWithExceptCall if matches!(stack.last(), Some(Slot::Val(Value::BoundFn(_)))) => {
                        self.dunder_op(code, op, stack, env, &mut spawn)
                    }
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
                    Op::LoadAttr(i) if matches!(stack.last(), Some(Slot::Val(Value::Instance(_)))) => {
                        let Some(Slot::Val(obj)) = stack.pop() else { return Err(internal("bad value stack")) };
                        let Value::Instance(inst) = &obj else { return Err(internal("bad value stack")) };
                        let name = &code.names[i as usize];
                        // `property`, descritor com `__get__` e `__getattr__` em Python abrem quadro, sem recursar.
                        let read = self.instance_getattr_call(&obj, inst, name).and_then(|got| match got {
                            Ok(v) => Ok(Entered::Done(v)),
                            Err((f, args)) => self.enter_function(&f, args, Vec::new()),
                        });
                        match read {
                            Ok(Entered::Done(v)) => {
                                stack.push(Slot::Val(v));
                                Ok(None)
                            }
                            Ok(Entered::Frame(c)) => {
                                spawn = Some(c);
                                Ok(None)
                            }
                            Err(e) => Err(tag_attribute_error(e, &obj, name)),
                        }
                    }
                    Op::LoadMethod(i) => match stack.last() {
                        Some(Slot::Val(obj)) => match plain_method(obj, &code.names[i as usize]) {
                            Some(f) => {
                                let obj = match stack.pop() {
                                    Some(Slot::Val(v)) => v,
                                    _ => return Err(internal("bad value stack")),
                                };
                                stack.push(Slot::Val(Value::Function(f)));
                                stack.push(Slot::Val(obj));
                                Ok(None)
                            }
                            None => self.step(code, op, stack, env),
                        },
                        _ => self.step(code, op, stack, env),
                    },
                    // `*g` com `g` gerador ou cadeia sobre gerador (`[*g]`, `(*g,)`, `f(*g)`): a fonte é esgotada em quadros
                    // e a lista em construção, que fica na pilha, é estendida no fim (o CPython esgota antes de usar).
                    Op::ListExtend if extend_source(stack).is_some() => {
                        let Some(root) = extend_source(stack) else { return Err(internal("generator vanished")) };
                        stack.pop();
                        let Some(Slot::Val(target)) = stack.last() else { return Err(internal("ListExtend without list")) };
                        let collect = Collect { items: Vec::new(), call: target.clone(), kwargs: Vec::new(), fold: None };
                        self.pull(&root, ResumeUse::Collect(Box::new(collect))).map(|next| {
                            let (_, callee) = next.land(stack);
                            spawn = callee;
                            None
                        })
                    }
                    // `import`: o corpo de um módulo que ainda não foi importado ganha quadro neste laço (e a cadeia de
                    // pais de `a.b.c` desce um quadro por módulo), então `fork` e troca de thread dentro dele enxergam o
                    // estado todo em dados.
                    Op::Import(_) | Op::ImportRel { .. } => match self.import_entered(code, op) {
                        Ok(Entered::Done(v)) => {
                            stack.push(Slot::Val(v));
                            Ok(None)
                        }
                        Ok(Entered::Frame(c)) => {
                            spawn = Some(c);
                            Ok(None)
                        }
                        Err(e) => Err(e),
                    },
                    // As chamadas: função Python simples ganha quadro novo neste mesmo laço; o resto roda e devolve o valor.
                    Op::Call { .. } | Op::CallMethod { .. } | Op::CallEx { .. } => {
                        // Uma leitura do stdin pode parar por falta de entrada: guarda a chamada para o laço repeti-la
                        // depois de esperar o descritor (ver `crate::stdin`).
                        let mut retry: Option<(Value, Vec<Value>, Vec<(String, Value)>)> = None;
                        let entered = pop_call(code, stack, op).and_then(|(func, args, kwargs)| {
                            let reader = crate::stdin::is_reader(&func);
                            if reader {
                                retry = Some((func.clone(), args.clone(), kwargs.clone()));
                                crate::stdin::set_replayable(true);
                            }
                            let entered = self.call_or_enter(&func, args, kwargs);
                            if reader {
                                crate::stdin::set_replayable(false);
                            }
                            entered
                        });
                        match entered {
                            Ok(Entered::Done(v)) => {
                                stack.push(Slot::Val(v));
                                Ok(None)
                            }
                            Ok(Entered::Frame(c)) => {
                                spawn = Some(c);
                                Ok(None)
                            }
                            // A nativa pediu para suspender (`os.fork`): o pedido se completa depois deste
                            // `match`, quando o empréstimo do quadro em execução já acabou.
                            Err(e) if crate::fork::is_suspend(&e) => match crate::fork::take_request() {
                                // O stdin parou a leitura: o quadro de espera repete a chamada e entrega o valor dela.
                                crate::fork::SuspendRequest::Wait(fd) => self.stdin_wait_frame(fd, retry).map(|callee| {
                                    spawn = Some(callee);
                                    None
                                }),
                                request => {
                                    suspend = Some(request);
                                    Ok(None)
                                }
                            },
                            Err(e) => Err(e),
                        }
                    }
                    _ => self.step(code, op, stack, env),
                }
            };
            let result = match result {
                Ok(_) if finish.is_none() && TEXT_ERROR_SET.with(Cell::get) && self.depth.get() <= TEXT_ERROR_DEPTH.with(Cell::get) => {
                    take_text_error().map_or(Ok(None), Err)
                }
                other => other,
            };
            let result = match result {
                Err(e) if crate::tracing::active() && !e.is_reraise() => match crate::tracing::exception(self, code, &e) {
                    Ok(()) => Err(e),
                    Err(e2) => Err(e2),
                },
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
                                _ if native_in_cpython(&code.filename, &code.qual()) => {}
                                _ => entries.insert(0, (code.lines[*pc], code.name.clone(), Rc::from(code.filename.as_str()), code.spans[*pc], tb_frame_of(code, env))),
                            }
                        }
                        let filename = self.script_name();
                        let tb = crate::tbobj::TracebackObj::make(entries, &filename);
                        attach_traceback(&value, tb);
                        stack.push(Slot::Val(value));
                        *pc = b.handler;
                    }
                    None => {
                        if !e.take_reraise_mark() {
                            match e.tb.last_mut() {
                                Some(last) if is_inlined_comp(&last.1) => last.1 = code.name.clone(),
                                _ if native_in_cpython(&code.filename, &code.qual()) => {}
                                _ => e.tb.push((code.lines[*pc], code.name.clone(), Rc::from(code.filename.as_str()), code.spans[*pc], tb_frame_of(code, env))),
                            }
                        }
                        // O quadro acaba: o traceback (`FrameHold`) é quem o guarda agora, e leva os locais ao morrer.
                        crate::frameobj::finish(code, env);
                        if is_child {
                            finish = Some(Err(e));
                        } else {
                            return Err(e);
                        }
                    }
                },
            }
            if let Some(request) = suspend.take() {
                if let crate::fork::SuspendRequest::Green(green) = request {
                    // Troca de thread verde: o quadro em execução passa a ser o da outra thread. Um erro
                    // (alvo inválido) chega antes de qualquer troca, então nasce na própria chamada.
                    match self.green_switch(green, &mut *frame, &mut child, mark) {
                        Ok(crate::gthread::Landing::Exit) => return Ok(Exit::Return(Value::None)),
                        Ok(crate::gthread::Landing::Resume(v)) => running(&mut *frame, &mut child).stack.push(Slot::Val(v)),
                        Ok(crate::gthread::Landing::Started) => {}
                        Err(e) => {
                            running(&mut *frame, &mut child).pc -= 1;
                            pending = Some(e);
                        }
                    }
                } else {
                    // O `pc` já aponta a instrução seguinte à chamada e a nativa já consumiu os argumentos.
                    let landed = self.complete_suspend(request, &*frame, child.as_ref());
                    let landing = running(&mut *frame, &mut child);
                    match landed {
                        Ok(v) => landing.stack.push(Slot::Val(v)),
                        Err(e) => {
                            // O erro nasce na própria chamada: o `pc` volta a ela.
                            landing.pc -= 1;
                            pending = Some(e);
                        }
                    }
                }
            }
            if let Some(callee) = spawn.take() {
                if let Some(caller) = child.take() {
                    self.frames_stack.borrow_mut().push(caller);
                }
                child = Some(callee);
                // `throw`/`close`: a exceção nasce no ponto em que o gerador parou.
                if let Some(r) = child.as_mut().and_then(|c| c.link.resuming.as_mut()) {
                    pending = r.inject.take();
                }
                // Parado numa delegação, o gerador de fora repassa a exceção ao sub-iterador: o quadro dele é
                // empilhado (a cadeia de `yield from` desce um quadro por nível) e o desfecho volta ao de fora.
                while let Some(e) = pending.take() {
                    let Some(outer) = child.as_mut() else {
                        pending = Some(e);
                        break;
                    };
                    match self.inject_delegated(outer, e) {
                        Injected::Raise(e) => {
                            pending = Some(e);
                            break;
                        }
                        Injected::Applied => break,
                        Injected::Sub(sub) => {
                            if let Some(caller) = child.take() {
                                self.frames_stack.borrow_mut().push(caller);
                            }
                            child = Some(sub);
                            pending = child.as_mut().and_then(|c| c.link.resuming.as_mut()).and_then(|r| r.inject.take());
                        }
                    }
                }
            } else if let Some(outcome) = finish.take() {
                let Some(mut done) = child.take() else { return Err(internal("return without a callee frame")) };
                let on_stop = done.link.on_stop;
                let then = done.link.then.take();
                let closed = match done.link.resuming.take() {
                    // Gerador retomado pelo laço: o quadro volta ao gerador e o desfecho é aplicado ao chamador.
                    Some(Resuming { tail, use_, .. }) => {
                        let core = tail.core.clone();
                        let exit = match outcome {
                            Ok(v) if yielded => Ok(Exit::Yield(v)),
                            Ok(v) => Ok(Exit::Return(v)),
                            Err(e) => Err(e),
                        };
                        let mut ended = core.end(self, tail, done.frame, exit);
                        if use_.closes() {
                            ended = core.settle_close(ended);
                        }
                        Closed::Resumed(core, use_, ended)
                    }
                    None => match then {
                        Some(body) if body.is_body() => Closed::Body(body, done.link, done.frame.env.clone(), outcome),
                        then => Closed::Call(self.end_call(done.link, outcome), on_stop, then),
                    },
                };
                child = {
                    let mut suspended = self.frames_stack.borrow_mut();
                    if suspended.len() > mark { suspended.pop() } else { None }
                };
                let caller: &mut Frame = match child.as_mut() {
                    Some(c) => &mut c.frame,
                    None => &mut *frame,
                };
                let mut next_spawn: Option<Callee> = None;
                match closed {
                    // O gerador de uma coleta ou de uma cadeia preguiçosa entregou um item (ou acabou): o item sobe
                    // pelas camadas e chega ao consumidor, que pede o seguinte ou termina.
                    Closed::Resumed(_, ResumeUse::Pull(pull), ended) => {
                        let Pull { root, layers, then } = *pull;
                        let stepped = match ended {
                            Ok(resumed) => {
                                let event = match resumed {
                                    Resumed::Yield(v) => Event::Got(v),
                                    Resumed::Return(_) => Event::Ended,
                                };
                                self.drive(&root, layers, then, event).and_then(|fetched| self.settle(&root, fetched))
                            }
                            Err(e) => Err(e),
                        };
                        let (callee, error) = land_step(caller, stepped);
                        next_spawn = callee;
                        if error.is_some() {
                            pending = error;
                        }
                    }
                    // O corpo de um módulo ou o código de um `exec`/`eval` acabou: o módulo é concluído (ou removido de
                    // `sys.modules`), as globais recebem o que ele criou e a cadeia do `import` segue, no quadro do chamador.
                    Closed::Body(body, link, env, outcome) => {
                        let stepped = self.close_body(body, link, &env, outcome);
                        let (callee, error) = land_step(caller, stepped);
                        next_spawn = callee;
                        if error.is_some() {
                            pending = error;
                        }
                    }
                    Closed::Resumed(core, use_, ended) => {
                        let applied = ended.and_then(|resumed| self.apply_resumed(&caller.code.clone(), &mut caller.stack, &core, resumed, use_));
                        match applied {
                            Ok(Some(target)) => caller.pc = target,
                            Ok(None) => {}
                            Err(e) => {
                                // O erro chega ao chamador na instrução que retomou o gerador.
                                caller.pc -= 1;
                                pending = Some(e);
                            }
                        }
                    }
                    Closed::Call(outcome, on_stop, then) => match (outcome, on_stop, then) {
                        (Ok(v), _, Some(then)) => {
                            let stepped = self.finish_dunder(then, v);
                            let (callee, error) = land_step(caller, stepped);
                            next_spawn = callee;
                            if error.is_some() {
                                pending = error;
                            }
                        }
                        (Ok(v), _, None) => caller.stack.push(Slot::Val(v)),
                        // O tratador de um sinal levantou: a exceção nasce na instrução interrompida, que ainda não rodou.
                        (Err(e), _, Some(Dunder::Signal)) => pending = Some(e),
                        // O `__next__` ou o `__getitem__` de uma fonte folha acabou com o fim dela (`StopIteration`,
                        // no protocolo antigo também `IndexError`): a máquina da coleta segue com o fim.
                        (Err(e), _, Some(Dunder::Callback(callback)))
                            if matches!(&callback.what, CallbackKind::Advance { leaf } if crate::lazy::leaf_stops(&**leaf, &e)) =>
                        {
                            let stepped = self.resume_callback(*callback, None);
                            let (callee, error) = land_step(caller, stepped);
                            next_spawn = callee;
                            if error.is_some() {
                                pending = error;
                            }
                        }
                        // O `__next__` de um `for` acabou: descarta o iterador e salta para o fim do laço.
                        (Err(e), Some(exit), _) if e.kind == "StopIteration" => {
                            // O `FOR_ITER` vê o `StopIteration` antes de engoli-lo: é evento `exception` no `for`.
                            let traced = if crate::tracing::active() { crate::tracing::exception(self, &caller.code, &e) } else { Ok(()) };
                            match traced {
                                Ok(()) => {
                                    caller.stack.pop();
                                    caller.pc = exit;
                                }
                                Err(e2) => {
                                    caller.pc -= 1;
                                    pending = Some(e2);
                                }
                            }
                        }
                        (Err(e), _, _) => {
                            // O erro chega ao chamador na instrução da chamada (o `pc` já tinha avançado).
                            caller.pc -= 1;
                            pending = Some(e);
                        }
                    },
                }
                // Outra tentativa da cadeia de um operador: o chamador espera de novo em `frames_stack`.
                if let Some(callee) = next_spawn {
                    if let Some(caller) = child.take() {
                        self.frames_stack.borrow_mut().push(caller);
                    }
                    child = Some(callee);
                }
            }
        }
    }

    /// `__context__` implícito: a exceção em tratamento quando `e` é levantada.
    pub(crate) fn link_context(&self, e: &mut PyException) {
        let Some(ctx) = self.handled.borrow().last().cloned() else { return };
        let value = e.to_value();
        e.value = Some(value.clone());
        exc_set_context(&value, &ctx);
    }

    /// Os sinais capturados que chegaram, entregues ao `signal._dispatch` do programa: a função e a lista de
    /// números. `None` quando nada chegou, ou o programa não registrou tratador.
    fn pending_dispatch(&mut self) -> Option<(Value, Value)> {
        if !signals_armed() || sysabi::sys::try_current().is_none() {
            return None;
        }
        if crate::globalsview::ARMED.load(std::sync::atomic::Ordering::Relaxed) {
            crate::globalsview::sync_pull();
        }
        // Alarme vencido: o SIGALRM chega ao processo como qualquer outro sinal (padrão: termina; capturado: tratador).
        let caught = sysabi::sys::current().take_caught_signals();
        if caught.is_empty() {
            return None;
        }
        // O `_dispatch` é auxiliar do `_signal` e não aparece no `dir()` dele: vem das globais completas.
        crate::modules::import(self, "_signal")?;
        let dispatch = crate::modules::pysrc::private_attr("_signal", "_dispatch")?;
        let list = Value::list(caught.into_iter().map(|s| Value::Int(i64::from(s.0))).collect());
        Some((dispatch, list))
    }

    /// Roda os tratadores de `signal.signal` dos sinais capturados que chegaram, recursivamente: o caminho das
    /// nativas que esperam (`time.sleep`, `wait4`, `fcntl`: o tratador roda e a espera recomeça). Um tratador
    /// que levanta (`KeyboardInterrupt`...) interrompe quem chamou.
    pub(crate) fn deliver_signals(&mut self) -> PyResult<()> {
        if let Some((dispatch, list)) = self.pending_dispatch() {
            self.call(&dispatch, vec![list], Vec::new())?;
        }
        Ok(())
    }

    /// O mesmo entre instruções, para o laço: o quadro do `_dispatch` (com `Dunder::Signal`), que o laço empilha
    /// sem recursar. `None` quando não há nada a entregar.
    fn signal_frame(&mut self) -> PyResult<Option<Callee>> {
        let Some((dispatch, list)) = self.pending_dispatch() else { return Ok(None) };
        self.discarded_frame(&dispatch, vec![list])
    }

    /// O quadro do `_gsched.preempt` quando a fatia da thread verde vence (só com o `_gsched` carregado: sem
    /// ele não há outra thread). O `preempt` mesmo recusa a troca no meio do escalonador (`_gsched._atomic`).
    fn preempt_frame(&mut self) -> PyResult<Option<Callee>> {
        let Some(preempt) = self.modules.borrow().get("_gsched").and_then(|m| m.attrs.borrow().get("preempt").cloned()) else {
            return Ok(None);
        };
        self.discarded_frame(&preempt, Vec::new())
    }

    /// Um quadro que o laço empilha entre instruções e cujo resultado descarta (`Dunder::Signal`): a instrução
    /// interrompida segue quando ele volta.
    fn discarded_frame(&mut self, func: &Value, args: Vec<Value>) -> PyResult<Option<Callee>> {
        Ok(match self.enter_callable(func, args, Vec::new())? {
            Entered::Frame(mut callee) => {
                callee.link.then = Some(Dunder::Signal);
                Some(callee)
            }
            Entered::Done(_) => None,
        })
    }

    pub(crate) fn call_function(&mut self, f: &Rc<FuncObj>, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
        match self.enter_function(f, args, kwargs)? {
            Entered::Done(v) => Ok(v),
            Entered::Frame(mut callee) => {
                let result = self.run_frame(&mut callee.frame);
                self.end_call(callee.link, result)
            }
        }
    }

    /// Chama um valor chamável; a função Python simples (ou o método dela já ligado) e a classe de
    /// usuário com `__init__` em Python devolvem o quadro pronto para o laço de instruções executar,
    /// sem recursar.
    pub(crate) fn call_or_enter(&mut self, func: &Value, mut args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Entered> {
        // Função Python e método ligado a função: nenhum dos casos especiais abaixo (nativa de coleta, `exec`,
        // `next`, `__import__`, método de gerador) os reconhece, e sem perfil nenhum evento `c_*` cabe aqui.
        if matches!(func, Value::Function(_) | Value::BoundFn(_)) && !crate::tracing::profiling() {
            return self.enter_callable(func, args, kwargs);
        }
        // Função de módulo que no CPython é C: um `c_call`/`c_return` por fora e nada por dentro, o
        // que pede a chamada recursiva, não o quadro entregue ao laço de instruções.
        if crate::tracing::reports_native_python_call(self, func) {
            return crate::tracing::c_call(self, func, |vm| vm.call_value(func, args, kwargs)).map(Entered::Done);
        }
        // Com o perfil ligado, `sorted`, `min`, `sum`... geram `c_call` antes e `c_return` depois dos quadros do
        // callback: a coleta em quadros do laço não tem onde pô-los, então a nativa segue pela chamada recursiva.
        if !crate::tracing::reports_c_call(self, func) {
            if let Some((root, collect)) = collect_source(func, &args, &kwargs)? {
                return self.start_pull(root, ResumeUse::Collect(Box::new(collect))).and_then(entered_of);
            }
        }
        match func {
            // `exec` e `eval`: o código roda como quadro deste laço (o perfil que quer `c_call` fica com a chamada recursiva).
            _ if (crate::builtins_ext::is_builtin(func, "exec") || crate::builtins_ext::is_builtin(func, "eval"))
                && !crate::tracing::reports_c_call(self, func) =>
            {
                let eval = crate::builtins_ext::is_builtin(func, "eval");
                crate::builtins_ext::enter_exec(self, eval, args, kwargs)
            }
            // `__import__`: o corpo do módulo também ganha quadro.
            _ if crate::builtins_ext::is_builtin(func, "__import__") && !crate::tracing::reports_c_call(self, func) => {
                crate::builtins_ext::enter_import(self, args, kwargs)
            }
            // `next(g)`, `g.send(v)` e `g.__next__()` sobre gerador ou corrente: o quadro dele é empilhado.
            _ if crate::fold::builtin_name(func) == Some("next")
                && kwargs.is_empty()
                && (1..=2).contains(&args.len())
                && value_core(&args[0]).is_some() =>
            {
                let default = args.get(1).cloned();
                let Some(core) = value_core(&args[0]) else { return Err(internal("generator vanished")) };
                self.resume_entered(&core, None, None, ResumeUse::Next { default })
            }
            // `next(it)` sobre `enumerate`/`zip`/`map`/`filter` que termina num gerador: as camadas descem em quadros.
            _ if crate::fold::builtin_name(func) == Some("next")
                && kwargs.is_empty()
                && (1..=2).contains(&args.len())
                && chain_root(&args[0]) =>
            {
                let default = args.get(1).cloned();
                self.pull(&args[0], ResumeUse::Next { default }).and_then(entered_of)
            }
            Value::Bound(b) if is_resume_call(b, &args, &kwargs) => {
                let Some(core) = value_core(&b.recv) else { return Err(internal("generator vanished")) };
                self.resume_entered(&core, args.pop(), None, ResumeUse::Next { default: None })
            }
            // `g.throw(e)` e `g.close()`: a exceção entra no quadro do gerador, sem recursar.
            // Parado numa delegação, o quadro do sub-iterador é empilhado em `run_frames` (`inject_delegated`).
            Value::Bound(b) if is_inject_call(b, &args, &kwargs) => {
                let Some(core) = value_core(&b.recv) else { return Err(internal("generator vanished")) };
                if b.name == "close" {
                    if core.close_if_plain() {
                        return Ok(Entered::Done(Value::None));
                    }
                    self.resume_entered(&core, None, Some(exc("GeneratorExit", "")), ResumeUse::Close)
                } else {
                    let e = crate::generator::raise_for_throw(self, args.remove(0))?;
                    self.resume_entered(&core, None, Some(e), ResumeUse::Next { default: None })
                }
            }
            _ => self.enter_callable(func, args, kwargs),
        }
    }

    /// Abre a chamada de `func` como quadro para o laço executar, quando ela roda Python: função, método ligado a
    /// função, classe de usuário com `__init__` em Python (o quadro do `__init__`) e instância com `__call__` em
    /// Python. O resto (nativas) é chamado na hora.
    pub(crate) fn enter_callable(&mut self, func: &Value, mut args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Entered> {
        match func {
            Value::Function(f) => self.enter_function(f, args, kwargs),
            Value::BoundFn(b) => {
                args.insert(0, b.0.clone());
                self.enter_function(&b.1, args, kwargs)
            }
            Value::Class(c) if c.meta.as_ref().and_then(|m| m.lookup("__call__")).is_none() => {
                match self.begin_instance(c, args, kwargs)? {
                    crate::classes::Built::Done(v) => Ok(Entered::Done(v)),
                    crate::classes::Built::Init { obj, init, args, kw } => match self.enter_function(&init, args, kw)? {
                        Entered::Frame(mut callee) => {
                            callee.link.instance = Some(obj);
                            Ok(Entered::Frame(callee))
                        }
                        Entered::Done(v) => crate::classes::init_returned(obj, &v).map(Entered::Done),
                    },
                }
            }
            Value::Instance(i) => match i.class().lookup("__call__") {
                Some(Value::Function(f)) => {
                    args.insert(0, func.clone());
                    self.enter_function(&f, args, kwargs)
                }
                _ => self.call(func, args, kwargs).map(Entered::Done),
            },
            _ => self.call(func, args, kwargs).map(Entered::Done),
        }
    }

    /// Retoma o gerador `core` e devolve o quadro dele para o laço executar (ou o valor, se não há o que
    /// rodar). `sent` é o argumento de `send`; `inject`, a exceção de `throw`/`close`; `use_` diz o que o
    /// laço faz com o desfecho (`Next` ou `Close`).
    fn resume_entered(&mut self, core: &Rc<GenCore>, sent: Option<Value>, inject: Option<PyException>, use_: ResumeUse) -> PyResult<Entered> {
        match self.enter_resume(core, sent, inject, use_)? {
            Resumption::Frame(callee) => Ok(Entered::Frame(callee)),
            Resumption::Ready(resumed, ResumeUse::Close) => core.settle_close(Ok(resumed)).map(|_| Entered::Done(Value::None)),
            Resumption::Ready(Resumed::Yield(v), _) => Ok(Entered::Done(v)),
            Resumption::Ready(Resumed::Return(v), use_) => next_exhausted(v, use_).map(Entered::Done),
        }
    }

    /// Entrega o próximo item de `root` (um gerador, ou uma cadeia de `enumerate`/`zip`/`map`/`filter` que termina
    /// num) ao consumidor `then` (`for`, `next`, coleta ou conta por item), como o laço de instruções aplica:
    /// o quadro a executar, o valor ou o salto.
    fn pull(&mut self, root: &Value, then: ResumeUse) -> PyResult<Next> {
        let fetched = self.fetch(root, then)?;
        self.settle(root, fetched)
    }

    /// O que fazer com o que `fetch`/`drive` devolveram: o quadro vira o próximo passo; um item (ou o fim)
    /// que saiu sem rodar quadro é entregue ao consumidor.
    fn settle(&mut self, root: &Value, fetched: Fetch) -> PyResult<Next> {
        match fetched {
            Fetch::Frame(callee) => Ok(Next::Spawn(callee)),
            Fetch::Ready(item, then) => self.deliver(root, item, then),
        }
    }

    /// Pede o próximo item de `root` a partir da fonte inteira (nenhuma camada descida).
    fn fetch(&mut self, root: &Value, then: ResumeUse) -> PyResult<Fetch> {
        let Value::Ext(source) = root else { return Err(internal("pull from a value that is not an object")) };
        self.drive(root, Vec::new(), then, Event::Want(source.clone()))
    }

    /// A máquina de uma cadeia preguiçosa, sem recursão no gerador: `Want` desce uma camada (ou empilha o
    /// quadro do gerador, e então devolve o `Fetch::Frame`, com as camadas e o consumidor guardados no
    /// `ResumeUse::Pull` do quadro), `Got` e `Ended` sobem pela camada de cima até o consumidor. As camadas
    /// que não envolvem gerador (uma fonte comum) resolvem na hora, pelo próprio iterador da fonte.
    fn drive(&mut self, root: &Value, mut layers: Vec<Layer>, then: ResumeUse, mut event: Event) -> PyResult<Fetch> {
        loop {
            event = match event {
                Event::Want(target) => match crate::generator::core_of(&target).filter(|c| c.kind() == crate::generator::Kind::Generator) {
                    Some(core) => match self.enter_resume(&core, None, None, ResumeUse::ForIter { exit: 0 })? {
                        Resumption::Frame(mut callee) => {
                            // Sem camadas, o consumidor de `for`/`next` fica direto no quadro; a coleta sempre
                            // passa pelo `Pull`, que sabe pedir o item seguinte à mesma fonte.
                            let use_ = if layers.is_empty() && !matches!(then, ResumeUse::Collect(_)) {
                                then
                            } else {
                                ResumeUse::Pull(Box::new(Pull { root: root.clone(), layers, then }))
                            };
                            if let Some(resuming) = callee.link.resuming.as_mut() {
                                resuming.use_ = use_;
                            }
                            return Ok(Fetch::Frame(callee));
                        }
                        Resumption::Ready(Resumed::Yield(v), _) => Event::Got(v),
                        Resumption::Ready(Resumed::Return(_), _) => Event::Ended,
                    },
                    None => match crate::lazy::source_count(&*target) {
                        Some(0) => Event::Ended,
                        Some(_) => {
                            let layer = crate::lazy::layer_for(&target).ok_or_else(|| internal("lazy iterator without a layer"))?;
                            layers.push(layer);
                            pull_child(&*target, 0)?
                        }
                        None => match crate::lazy::leaf(&*target) {
                            // Fonte folha que roda Python (`__next__` de usuário, `__getitem__` do protocolo antigo
                            // de sequência): o método ganha quadro, e o desfecho volta por `CallbackKind::Advance`.
                            Some(crate::lazy::Leaf::Ended) => Event::Ended,
                            Some(crate::lazy::Leaf::Call(method, args)) => match self.enter_function(&method, args, Vec::new())? {
                                Entered::Done(v) => leaf_event(&*target, Some(v)),
                                Entered::Frame(mut callee) => {
                                    let pull = Pull { root: root.clone(), layers, then };
                                    callee.link.then = Some(Dunder::Callback(Box::new(Callback { pull, what: CallbackKind::Advance { leaf: target } })));
                                    return Ok(Fetch::Frame(callee));
                                }
                            },
                            None => match target.iter_next()? {
                                Some(v) => Event::Got(v),
                                None => Event::Ended,
                            },
                        },
                    },
                },
                Event::Got(item) => match layers.pop() {
                    None => return Ok(Fetch::Ready(Some(item), then)),
                    Some(Layer::Enumerate(it)) => Event::Got(crate::lazy::enumerate_item(&*it, item)?),
                    Some(Layer::Filter(it)) => match crate::lazy::filter_verdict(&*it, &item)? {
                        crate::lazy::Verdict::Keep(keep) => kept_event(&mut layers, it, item, keep)?,
                        // O predicado em Python ganha quadro; o desfecho volta por `CallbackKind::Kept`.
                        crate::lazy::Verdict::Call(func) => match self.enter_callable(&func, vec![item.clone()], Vec::new())? {
                            Entered::Done(v) => kept_event(&mut layers, it, item, v.is_true())?,
                            Entered::Frame(mut callee) => {
                                let pull = Pull { root: root.clone(), layers, then };
                                callee.link.then = Some(Dunder::Callback(Box::new(Callback { pull, what: CallbackKind::Kept { it, item } })));
                                return Ok(Fetch::Frame(callee));
                            }
                        },
                    },
                    Some(Layer::Many { it, at, mut got }) => {
                        got.push(item);
                        if at + 1 < crate::lazy::source_count(&*it).unwrap_or(0) {
                            let next = pull_child(&*it, at + 1)?;
                            layers.push(Layer::Many { it, at: at + 1, got });
                            next
                        } else {
                            match crate::lazy::join_sources(&*it, got)? {
                                crate::lazy::Joined::Item(v) => Event::Got(v),
                                // A função do `map` em Python ganha quadro; o desfecho volta por `CallbackKind::Mapped`.
                                crate::lazy::Joined::Call(func, args) => match self.enter_callable(&func, args, Vec::new())? {
                                    Entered::Done(v) => Event::Got(v),
                                    Entered::Frame(mut callee) => {
                                        let pull = Pull { root: root.clone(), layers, then };
                                        callee.link.then = Some(Dunder::Callback(Box::new(Callback { pull, what: CallbackKind::Mapped })));
                                        return Ok(Fetch::Frame(callee));
                                    }
                                },
                            }
                        }
                    }
                    // `zip(strict=True)` achou mais um item depois de a primeira fonte acabar.
                    Some(Layer::Check { at, .. }) => return Err(crate::lazy::zip_mismatch(at, "longer")),
                },
                Event::Ended => match layers.pop() {
                    None => return Ok(Fetch::Ready(None, then)),
                    Some(Layer::Many { it, at, .. }) => match crate::lazy::zip_ended(&*it, at)? {
                        crate::lazy::ZipEnd::Ends => Event::Ended,
                        crate::lazy::ZipEnd::Check => {
                            let next = pull_child(&*it, 1)?;
                            layers.push(Layer::Check { it, at: 1 });
                            next
                        }
                    },
                    Some(Layer::Check { it, at }) => {
                        if at + 1 < crate::lazy::source_count(&*it).unwrap_or(0) {
                            let next = pull_child(&*it, at + 1)?;
                            layers.push(Layer::Check { it, at: at + 1 });
                            next
                        } else {
                            Event::Ended
                        }
                    }
                    Some(Layer::Enumerate(_) | Layer::Filter(_)) => Event::Ended,
                },
            };
        }
    }

    /// Entrega o item (`None` é o fim da fonte) ao consumidor `then`. `for` e `next` recebem o valor ou o
    /// fim; a coleta o põe na lista ou na conta e pede o seguinte à mesma fonte, até acabar ou a conta decidir.
    fn deliver(&mut self, root: &Value, item: Option<Value>, then: ResumeUse) -> PyResult<Next> {
        match then {
            ResumeUse::ForIter { exit } => Ok(match item {
                Some(v) => Next::Value(v),
                None => Next::Exit(exit),
            }),
            ResumeUse::Next { default } => match item {
                Some(v) => Ok(Next::Value(v)),
                None => next_exhausted(Value::None, ResumeUse::Next { default }).map(Next::Value),
            },
            ResumeUse::Collect(collect) => {
                let input = item.map_or(Input::End, Input::Item);
                self.collect_run(root, collect, input)
            }
            _ => Err(internal("a pull ended in a use it cannot deliver")),
        }
    }

    /// A coleta: `input` entra na conta (ou na lista), e a máquina pede o item seguinte à mesma fonte até a
    /// fonte acabar ou a conta decidir. Uma chave que roda Python (`Flow::Key`) ganha quadro, e a coleta
    /// espera nele (`CallbackKind::Keyed`); no fim, a função é chamada com a lista (ou a conta dá o resultado).
    fn collect_run(&mut self, root: &Value, mut collect: Box<Collect>, mut input: Input) -> PyResult<Next> {
        loop {
            let flow = match (input, collect.fold.as_mut()) {
                (Input::Item(v), Some(fold)) => fold.feed(self, v)?,
                (Input::Item(v), None) => {
                    collect.items.push(v);
                    crate::fold::Flow::More
                }
                (Input::Key(item, key), Some(fold)) => fold.keyed(self, item, key)?,
                (Input::End, Some(fold)) => fold.finish(self)?,
                (Input::End, None) => return self.collect_end(*collect),
                (Input::Key(..), None) => return Err(internal("a key without a fold")),
            };
            match flow {
                crate::fold::Flow::Done(v) => return Ok(Next::Value(v)),
                crate::fold::Flow::Key { func, item } => match self.enter_callable(&func, vec![item.clone()], Vec::new())? {
                    Entered::Done(key) => {
                        input = Input::Key(item, key);
                        continue;
                    }
                    Entered::Frame(mut callee) => {
                        let pull = Pull { root: root.clone(), layers: Vec::new(), then: ResumeUse::Collect(collect) };
                        callee.link.then = Some(Dunder::Callback(Box::new(Callback { pull, what: CallbackKind::Keyed { item } })));
                        return Ok(Next::Spawn(callee));
                    }
                },
                crate::fold::Flow::More => {}
            }
            match self.fetch(root, ResumeUse::Collect(collect))? {
                Fetch::Frame(callee) => return Ok(Next::Spawn(callee)),
                Fetch::Ready(next, ResumeUse::Collect(back)) => {
                    input = next.map_or(Input::End, Input::Item);
                    collect = back;
                }
                Fetch::Ready(..) => return Err(internal("collect lost its state")),
            }
        }
    }

    /// O fim de uma coleta sem conta: a função é chamada com a lista, ou a lista de destino (o `*g` de uma lista
    /// em construção) é estendida.
    fn collect_end(&mut self, mut collect: Collect) -> PyResult<Next> {
        if let Value::List(target) = &collect.call {
            target.borrow_mut().extend(std::mem::take(&mut collect.items));
            return Ok(Next::Nothing);
        }
        let list = Value::list(std::mem::take(&mut collect.items));
        self.call(&collect.call, vec![list], std::mem::take(&mut collect.kwargs)).map(Next::Value)
    }

    /// O callback de `callback` devolveu `returned` (`None`: a fonte folha terminou) e a máquina de `drive` ou a
    /// coleta segue de onde parou.
    fn resume_callback(&mut self, callback: Callback, returned: Option<Value>) -> PyResult<Next> {
        let Callback { pull: Pull { root, mut layers, then }, what } = callback;
        let missing = || internal("a callback ended without a value");
        let event = match what {
            CallbackKind::Mapped => Event::Got(returned.ok_or_else(missing)?),
            CallbackKind::Kept { it, item } => kept_event(&mut layers, it, item, returned.ok_or_else(missing)?.is_true())?,
            CallbackKind::Advance { leaf } => leaf_event(&*leaf, returned),
            CallbackKind::Keyed { item } => {
                let ResumeUse::Collect(collect) = then else { return Err(internal("a key outside a collection")) };
                return self.collect_run(&root, collect, Input::Key(item, returned.ok_or_else(missing)?));
            }
            CallbackKind::Start => {
                let source = iterator_root(returned.ok_or_else(missing)?)?;
                return self.pull(&source, then);
            }
        };
        let fetched = self.drive(&root, layers, then, event)?;
        self.settle(&root, fetched)
    }

    /// Abre a coleta de `root`: a fonte já pronta, ou a instância cujo `__iter__` em Python roda antes em quadro.
    fn start_pull(&mut self, root: Root, then: ResumeUse) -> PyResult<Next> {
        match root {
            Root::Ready(source) => self.pull(&source, then),
            Root::Iterable(iter, instance) => match self.enter_function(&iter, vec![instance.clone()], Vec::new())? {
                Entered::Done(v) => {
                    let source = iterator_root(v)?;
                    self.pull(&source, then)
                }
                Entered::Frame(mut callee) => {
                    let pull = Pull { root: instance, layers: Vec::new(), then };
                    callee.link.then = Some(Dunder::Callback(Box::new(Callback { pull, what: CallbackKind::Start })));
                    Ok(Next::Spawn(callee))
                }
            },
        }
    }

    /// Abre a retomada de `core`: as globais passam a ser as do gerador até o fim (`GenCore::end` as
    /// devolve) e o quadro dele, saído do gerador, é o chamado que o laço executa. A exceção injetada de
    /// `throw`/`close` vai no `Resuming` e o laço a levanta ao empilhar o quadro; parado numa delegação
    /// (`yield from`, `await`) o gerador repassa a exceção ao sub-iterador por `inject_delegated`. A retomada
    /// recursiva de `GenCore::resume` só atende os chamadores nativos.
    fn enter_resume(&mut self, core: &Rc<GenCore>, sent: Option<Value>, inject: Option<PyException>, use_: ResumeUse) -> PyResult<Resumption> {
        let globals = core.globals();
        let caller_globals = (!Rc::ptr_eq(&self.globals, &globals)).then(|| std::mem::replace(&mut self.globals, globals));
        match GenCore::begin(core, self, sent, inject, caller_globals)? {
            Begun::Ready(done) => Ok(Resumption::Ready(done, use_)),
            Begun::Run(frame, tail, inject) => {
                let link = CallLink {
                    func: None,
                    caller_line: tail.caller_line,
                    handled_len: 0,
                    profiled: false,
                    caller_globals: None,
                    instance: None,
                    on_stop: None,
                    then: None,
                    resuming: Some(Resuming { tail, use_, inject }),
                };
                Ok(Resumption::Frame(Callee { frame, link }))
            }
        }
    }

    /// Retoma `core` a pedido de uma instrução do laço: o quadro do gerador vira o `spawn`, ou o desfecho
    /// que já saiu pronto é aplicado na hora.
    fn resume_op(
        &mut self,
        code: &Rc<Code>,
        stack: &mut Vec<Slot>,
        core: &Rc<GenCore>,
        sent: Option<Value>,
        use_: ResumeUse,
        spawn: &mut Option<Callee>,
    ) -> PyResult<Option<usize>> {
        match self.enter_resume(core, sent, None, use_)? {
            Resumption::Frame(callee) => {
                *spawn = Some(callee);
                Ok(None)
            }
            Resumption::Ready(resumed, use_) => self.apply_resumed(code, stack, core, resumed, use_),
        }
    }

    /// O desfecho de uma retomada, aplicado na pilha do chamador: devolve o salto, se houver (o `pc` do
    /// chamador já está na instrução seguinte à que retomou).
    fn apply_resumed(
        &mut self,
        code: &Rc<Code>,
        stack: &mut Vec<Slot>,
        core: &Rc<GenCore>,
        resumed: Resumed,
        use_: ResumeUse,
    ) -> PyResult<Option<usize>> {
        match (resumed, use_) {
            // `close()` sempre devolve `None` (o desfecho já passou por `GenCore::settle_close`).
            (_, ResumeUse::Close) => {
                stack.push(Slot::Val(Value::None));
                Ok(None)
            }
            // O fechamento do sub-gerador passou: a exceção original é levantada no de fora (um erro do
            // fechamento já chegou aqui como `Err`).
            (_, ResumeUse::DelegateExit(exit)) => Err(exit),
            (_, ResumeUse::Collect(_) | ResumeUse::Pull(_)) => Err(internal("collect applied as a plain resumption")),
            // O sub-gerador tratou a exceção e entregou outro valor: o de fora o reentrega, voltando ao `Yield`
            // da delegação (`end - 2`: `Delegate`, `Yield`, `DelegateNext`).
            (Resumed::Yield(v), ResumeUse::DelegateThrow { end }) => {
                stack.push(Slot::Val(v));
                Ok(Some(end - 2))
            }
            (Resumed::Yield(v), _) => {
                stack.push(Slot::Val(v));
                Ok(None)
            }
            // O `for` acabou: descarta o iterador; um `return valor` do gerador é o `StopIteration` que o
            // `FOR_ITER` vê antes de engolir.
            (Resumed::Return(_), ResumeUse::ForIter { exit }) => {
                stack.pop();
                if let Some(v) = core.take_returned() {
                    crate::tracing::exception(self, code, &crate::generator::stop_iteration(v))?;
                }
                Ok(Some(exit))
            }
            (Resumed::Return(v), use_ @ ResumeUse::Next { .. }) => {
                stack.push(Slot::Val(next_exhausted(v, use_)?));
                Ok(None)
            }
            (Resumed::Return(v), ResumeUse::Delegate { end } | ResumeUse::DelegateThrow { end }) => {
                stack.pop();
                stack.push(Slot::Val(v));
                Ok(Some(end))
            }
        }
    }
}

/// O valor de `next(g)` quando `g` terminou: o `default`, ou o `StopIteration(retorno)`.
fn next_exhausted(returned: Value, use_: ResumeUse) -> PyResult<Value> {
    match use_ {
        ResumeUse::Next { default: Some(default) } => Ok(default),
        _ => Err(crate::generator::stop_iteration(returned)),
    }
}

/// O núcleo de gerador ou de corrente que `v` é.
fn value_core(v: &Value) -> Option<Rc<GenCore>> {
    match v {
        Value::Ext(x) => crate::generator::core_of(x),
        _ => None,
    }
}

/// A fonte que `list(g)`, `tuple(g)`, `sorted(g, ...)`, `s.join(g)` esgotam, ou que `sum`, `set`, `dict`, `min`,
/// `max`, `any`, `all` e `lista.extend` consomem item a item (ver `fold`), com a coleta que o laço faz. A fonte é
/// qualquer uma que precise de quadro (`frame_root`): gerador, cadeia de `enumerate`/`zip`/`map`/`filter` com
/// gerador ou callback Python, iterador ou iterável de usuário em Python, ou, com uma `key=` em Python, qualquer
/// iterável. As que o CPython alimenta com a sequência inteira antes de usar (`PySequence_List`/
/// `PySequence_Fast`) juntam tudo e então chamam a função; as outras seguem a conta por item, com efeitos no meio.
fn collect_source(func: &Value, args: &[Value], kwargs: &[(String, Value)]) -> PyResult<Option<(Root, Collect)>> {
    let (source, collect) = match crate::fold::for_call(func, args, kwargs) {
        Some((source, fold)) => (source, Collect { items: Vec::new(), call: Value::None, kwargs: Vec::new(), fold: Some(fold) }),
        None => {
            let [arg] = args else { return Ok(None) };
            let fits = match crate::fold::builtin_name(func) {
                Some("list" | "tuple") => kwargs.is_empty(),
                Some("sorted") => true,
                _ => matches!(func, Value::Bound(b) if b.name == "join" && kwargs.is_empty() && matches!(b.recv, Value::Str(_) | Value::Bytes(_) | Value::ByteArray(_))),
            };
            if !fits {
                return Ok(None);
            }
            (arg.clone(), Collect { items: Vec::new(), call: func.clone(), kwargs: kwargs.to_vec(), fold: None })
        }
    };
    Ok(frame_root(source, collect.fold.as_ref())?.map(|root| (root, collect)))
}

/// A fonte que o `*g` de um `ListExtend` vai esgotar: o topo da pilha é um gerador (ou cadeia sobre gerador) e
/// logo abaixo está a lista em construção.
fn extend_source(stack: &[Slot]) -> Option<Value> {
    let [.., Slot::Val(Value::List(_)), Slot::Val(source)] = stack else { return None };
    suspendable(source).then(|| source.clone())
}

/// O próximo passo de uma cadeia preguiçosa: pedir a fonte `idx` de `it` quando ela precisa de quadro (gerador,
/// cadeia com callback Python, fonte folha), ou puxar o item dela direto.
fn pull_child(it: &dyn crate::object::ExtObject, idx: usize) -> PyResult<Event> {
    if let Some(source) = crate::lazy::frame_source(it, idx) {
        return Ok(Event::Want(source));
    }
    Ok(match crate::lazy::source_next(it, idx)? {
        Some(v) => Event::Got(v),
        None => Event::Ended,
    })
}

/// O `filter` `it` recebeu o veredito do predicado sobre `item`: com `keep` o item sobe pelas camadas, senão a
/// camada volta à pilha e pede o seguinte à fonte.
fn kept_event(layers: &mut Vec<Layer>, it: Rc<dyn crate::object::ExtObject>, item: Value, keep: bool) -> PyResult<Event> {
    if keep {
        return Ok(Event::Got(item));
    }
    let next = pull_child(&*it, 0)?;
    layers.push(Layer::Filter(it));
    Ok(next)
}

/// A fonte folha `leaf` entregou `got` (`None`: acabou): o estado dela avança e o evento sobe pelas camadas.
fn leaf_event(leaf: &dyn crate::object::ExtObject, got: Option<Value>) -> Event {
    match crate::lazy::leaf_settle(leaf, got) {
        Some(v) => Event::Got(v),
        None => Event::Ended,
    }
}

/// A fonte que o iterador devolvido por um `__iter__` em Python passa a ser numa coleta: o gerador (ou a cadeia) que
/// o laço puxa em quadros, ou o iterador vivo dentro de um `IterBox`.
fn iterator_root(returned: Value) -> PyResult<Value> {
    if suspendable(&returned) {
        return Ok(returned);
    }
    iterator_from_dunder(returned).map(crate::lazy::IterBox::new)
}

/// Onde a coleta de `source` começa quando ela precisa de quadro: o gerador, a cadeia ou a fonte folha de
/// sempre; a instância cujo `__iter__` está em Python; a instância do protocolo antigo de sequência
/// (`__getitem__` em Python); ou, com uma conta que chama uma chave em Python, o iterador vivo de qualquer
/// iterável (`get_iter`, na hora, como o `PyObject_GetIter` que o CPython faz antes do primeiro item).
fn frame_root(source: Value, fold: Option<&crate::fold::Fold>) -> PyResult<Option<Root>> {
    if suspendable(&source) {
        return Ok(Some(Root::Ready(source)));
    }
    // `dict(x)` trata um mapeamento (`keys`) antes de um iterável de pares: só o caminho comum decide.
    if matches!(fold, Some(crate::fold::Fold::Dict { .. })) {
        return Ok(None);
    }
    if let Value::Instance(i) = &source {
        let class = i.class();
        match (class.lookup("__iter__"), class.lookup("__getitem__")) {
            (Some(Value::Function(iter)), _) => return Ok(Some(Root::Iterable(iter, source))),
            // Subclasse de `dict`, `list`... herda o `__iter__` do tipo embutido: o `__getitem__` em Python não
            // vira protocolo antigo (o `ConvertingDict` do `logging.config`).
            (None, Some(Value::Function(_))) if class.builtin_base.is_none() && class.data_base.is_none() => {
                return Ok(Some(Root::Ready(crate::lazy::OldSeqIter::new(source))));
            }
            _ => {}
        }
    }
    if fold.is_some_and(crate::fold::Fold::calls_python) {
        return get_iter(&source).map(|it| Some(Root::Ready(crate::lazy::IterBox::new(it))));
    }
    Ok(None)
}

/// `g.throw(e)` ou `g.close()` sobre um gerador ou uma corrente.
fn is_inject_call(b: &BoundMethod, args: &[Value], kwargs: &[(String, Value)]) -> bool {
    kwargs.is_empty()
        && match b.name {
            "throw" => !args.is_empty(),
            "close" => args.is_empty(),
            _ => false,
        }
        && value_core(&b.recv).is_some()
}

/// `g.send(v)` ou `g.__next__()` sobre um gerador, com os argumentos que o método aceita.
fn is_resume_call(b: &BoundMethod, args: &[Value], kwargs: &[(String, Value)]) -> bool {
    kwargs.is_empty()
        && match b.name {
            "send" => args.len() == 1,
            "__next__" => args.is_empty(),
            _ => false,
        }
        && value_core(&b.recv).is_some()
}

impl Vm {
    /// Liga os argumentos e abre a chamada. Gerador e corrotina terminam aqui, com o valor. A função
    /// de outro módulo troca `Vm::globals` pelas dela até o fechamento (`end_call`).
    fn enter_function(&mut self, f: &Rc<FuncObj>, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Entered> {
        if Rc::ptr_eq(&self.globals, &f.globals) {
            return self.open_call(f, args, kwargs);
        }
        let caller = std::mem::replace(&mut self.globals, f.globals.clone());
        match self.open_call(f, args, kwargs) {
            Ok(Entered::Frame(mut callee)) => {
                callee.link.caller_globals = Some(caller);
                Ok(Entered::Frame(callee))
            }
            other => {
                self.globals = caller;
                other
            }
        }
    }

    fn open_call(&mut self, f: &Rc<FuncObj>, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Entered> {
        let env = self.bind_params(f, args, kwargs)?;
        let code = f.code.clone();
        if code.is_generator || code.is_async {
            return Ok(Entered::Done(crate::generator::new_generator(self.clone(), code, env)));
        }
        // O quadro do módulo também gasta uma unidade do limite no CPython (`py_recursion_remaining`),
        // mas aqui ele não entra em `depth`: os demais podem ir até `limit - 1`.
        if self.depth.get() >= RECURSION_LIMIT.with(|c| c.get()) {
            return Err(exc("RecursionError", "maximum recursion depth exceeded"));
        }
        self.depth.set(self.depth.get() + 1);
        let caller_line = self.cur_line.get();
        self.frames.borrow_mut().push((code.clone(), caller_line, env.clone()));
        // `return` dentro de um `except` sai sem fechar o tratador: a pilha volta ao tamanho de antes.
        let handled_len = self.handled.borrow().len();
        let profiled = crate::modules::lsprof::enter(&code);
        if let Err(e) = crate::tracing::enter(self, &code) {
            self.frames.borrow_mut().pop();
            self.cur_line.set(caller_line);
            self.depth.set(self.depth.get() - 1);
            return Err(e);
        }
        let link = CallLink {
            func: Some(f.clone()),
            caller_line,
            handled_len,
            profiled,
            caller_globals: None,
            instance: None,
            on_stop: None,
            then: None,
            resuming: None,
        };
        Ok(Entered::Frame(Callee { frame: Frame::new(code, env), link }))
    }

    /// Fecha uma chamada aberta por `enter_function`, com o resultado do corpo.
    fn end_call(&mut self, link: CallLink, mut result: PyResult<Value>) -> PyResult<Value> {
        if let Err(e) = crate::tracing::leave(self, &result) {
            result = Err(e);
        }
        if link.profiled {
            crate::modules::lsprof::leave();
        }
        // Função embutida no CPython (escrita em Python aqui): o traceback não mostra o interior dela.
        if let Err(e) = &mut result {
            if link.func.as_ref().is_some_and(|f| f.attrs.borrow().contains_key("__no_bind__")) {
                while e.tb.last().is_some_and(|t| t.2.starts_with("/usr/lib/python3.13/")) {
                    e.tb.pop();
                }
            }
        }
        self.handled.borrow_mut().truncate(link.handled_len);
        self.frames.borrow_mut().pop();
        self.cur_line.set(link.caller_line);
        self.depth.set(self.depth.get() - 1);
        if let Some(globals) = link.caller_globals {
            self.globals = globals;
        }
        match (result, link.instance) {
            (Ok(returned), Some(obj)) => crate::classes::init_returned(obj, &returned),
            (other, _) => other,
        }
    }

    /// Abre o método mágico em Python da instrução (operador binário, comparação, subscrição ou
    /// teste de verdade) como quadro novo; sem esse método, a instrução segue pelo `step`.
    fn dunder_op(
        &mut self,
        code: &Rc<Code>,
        instr: Op,
        stack: &mut Vec<Slot>,
        env: &Rc<Env>,
        spawn: &mut Option<Callee>,
    ) -> PyResult<Option<usize>> {
        match self.begin_dunder(code, instr, stack)? {
            Some(next) => {
                let (jump, callee) = next.land(stack);
                *spawn = callee;
                Ok(jump)
            }
            None => self.step(code, instr, stack, env),
        }
    }

    /// Tira os operandos da pilha e começa o método mágico da instrução; `None` (a pilha intacta)
    /// quando nenhum deles é função Python, e o caminho recursivo cuida do resto.
    fn begin_dunder(&mut self, code: &Rc<Code>, instr: Op, stack: &mut Vec<Slot>) -> PyResult<Option<Next>> {
        match instr {
            Op::Binary { op, inplace } => {
                let (Some(a), Some(b)) = (value_at(stack, 1), value_at(stack, 0)) else { return Ok(None) };
                // `"..." % instância` é formatação, resolvida no `step`.
                if op == Operator::Mod && matches!(a, Value::Str(_)) {
                    return Ok(None);
                }
                let attempts = binary_tries(op, &a, &b, inplace);
                self.begin_chain(stack, ChainKind::Binary { op, inplace }, a, b, attempts)
            }
            Op::Compare(cop) => {
                let (Some(a), Some(b)) = (value_at(stack, 1), value_at(stack, 0)) else { return Ok(None) };
                match cop {
                    CmpOp::Lt | CmpOp::LtE | CmpOp::Gt | CmpOp::GtE => {
                        let attempts = order_attempts(cop, &a, &b);
                        self.begin_chain(stack, ChainKind::Order(cop), a, b, attempts)
                    }
                    // O `richcmp` de uma extensão decide antes do `__eq__` de usuário.
                    CmpOp::Eq | CmpOp::NotEq if !matches!(a, Value::Ext(_)) && !matches!(b, Value::Ext(_)) => {
                        let attempts = equal_attempts(cop, &a, &b);
                        self.begin_chain(stack, ChainKind::Equal(cop), a, b, attempts)
                    }
                    CmpOp::In | CmpOp::NotIn => {
                        let Some(f) = dunder_function(&b, "__contains__") else { return Ok(None) };
                        stack.truncate(stack.len() - 2);
                        self.open_dunder(&f, vec![b, a], Dunder::Boolean { negate: cop == CmpOp::NotIn }).map(Some)
                    }
                    _ => Ok(None),
                }
            }
            Op::Subscript => {
                let (Some(container), Some(index)) = (value_at(stack, 1), value_at(stack, 0)) else { return Ok(None) };
                let Some(f) = dunder_function(&container, "__getitem__") else { return Ok(None) };
                stack.truncate(stack.len() - 2);
                self.open_dunder(&f, vec![container, index], Dunder::Push).map(Some)
            }
            Op::StoreSubscript => {
                let (Some(container), Some(index), Some(value)) = (value_at(stack, 1), value_at(stack, 0), value_at(stack, 2)) else {
                    return Ok(None);
                };
                let Some(f) = dunder_function(&container, "__setitem__") else { return Ok(None) };
                stack.truncate(stack.len() - 3);
                self.open_dunder(&f, vec![container, index, value], Dunder::Discard).map(Some)
            }
            Op::DeleteSubscript => {
                let (Some(container), Some(index)) = (value_at(stack, 1), value_at(stack, 0)) else { return Ok(None) };
                let Some(f) = dunder_function(&container, "__delitem__") else { return Ok(None) };
                stack.truncate(stack.len() - 2);
                self.open_dunder(&f, vec![container, index], Dunder::Discard).map(Some)
            }
            Op::PopJumpIfFalse(_) | Op::PopJumpIfTrue(_) | Op::JumpIfFalseOrPop(_) | Op::JumpIfTrueOrPop(_) | Op::Unary(UnaryOp::Not) => {
                let (Some((f, len)), Some(operand)) = (truth_method(stack), value_at(stack, 0)) else { return Ok(None) };
                let how = match instr {
                    Op::PopJumpIfFalse(t) => TruthUse::Jump { target: t as usize, when: false },
                    Op::PopJumpIfTrue(t) => TruthUse::Jump { target: t as usize, when: true },
                    Op::JumpIfFalseOrPop(t) => TruthUse::JumpKeep { target: t as usize, when: false },
                    Op::JumpIfTrueOrPop(t) => TruthUse::JumpKeep { target: t as usize, when: true },
                    _ => TruthUse::Not,
                };
                if !matches!(how, TruthUse::JumpKeep { .. }) {
                    stack.pop();
                }
                self.open_dunder(&f, vec![operand], Dunder::Truth { how, len }).map(Some)
            }
            Op::Unary(op) => {
                let Some(operand) = value_at(stack, 0) else { return Ok(None) };
                let Some(f) = unary_dunder(op).and_then(|name| dunder_function(&operand, name)) else { return Ok(None) };
                stack.pop();
                self.open_dunder(&f, vec![operand], Dunder::Push).map(Some)
            }
            Op::GetIter => {
                let Some(operand) = value_at(stack, 0) else { return Ok(None) };
                let Some(f) = dunder_function(&operand, "__iter__") else { return Ok(None) };
                stack.pop();
                self.open_dunder(&f, vec![operand], Dunder::Iter).map(Some)
            }
            Op::StoreAttr(i) => {
                let (Some(obj), Some(value)) = (value_at(stack, 0), value_at(stack, 1)) else { return Ok(None) };
                stack.truncate(stack.len() - 2);
                let call = self.store_attr_call(&obj, &code.names[i as usize], value)?;
                self.open_pending(call).map(Some)
            }
            Op::DeleteAttr(i) => {
                let Some(obj) = value_at(stack, 0) else { return Ok(None) };
                stack.pop();
                let call = self.delete_attr_call(&obj, &code.names[i as usize])?;
                self.open_pending(call).map(Some)
            }
            Op::WithEnter => {
                let Some(manager @ Value::Instance(_)) = value_at(stack, 0) else { return Ok(None) };
                let Value::Instance(inst) = &manager else { return Ok(None) };
                let class = inst.class();
                let (Some(enter), Some(exit)) = (dunder_function(&manager, "__enter__"), class.lookup("__exit__")) else {
                    return Ok(None);
                };
                let exit = self.bind_class_attr(&exit, manager.clone(), &class)?;
                stack.pop();
                stack.push(Slot::Val(exit));
                self.open_dunder(&enter, vec![manager], Dunder::Push).map(Some)
            }
            Op::AsyncWithEnter => {
                let Some(manager) = value_at(stack, 0) else { return Ok(None) };
                let (Some(enter), Some(exit)) = (dunder_function(&manager, "__aenter__"), dunder_function(&manager, "__aexit__")) else {
                    return Ok(None);
                };
                stack.pop();
                stack.push(Slot::Val(Value::BoundFn(Rc::new((manager.clone(), exit)))));
                self.open_dunder(&enter, vec![manager], Dunder::Push).map(Some)
            }
            Op::WithExcept | Op::AsyncWithExceptCall => {
                let (Some(Value::BoundFn(exit)), Some(raised)) = (value_at(stack, 0), value_at(stack, 1)) else { return Ok(None) };
                stack.pop();
                let (traceback, then) = match instr {
                    Op::WithExcept => (exception_traceback(&raised), Dunder::Boolean { negate: false }),
                    _ => (Value::None, Dunder::Push),
                };
                let args = vec![exit.0.clone(), self.type_of(&raised), raised, traceback];
                self.open_dunder(&exit.1, args, then).map(Some)
            }
            _ => Ok(None),
        }
    }

    /// Abre o método mágico em Python que a gravação ou a remoção de atributo anotou (ou nada, se ela
    /// já se resolveu); o valor devolvido é descartado.
    fn open_pending(&mut self, call: crate::classes::PendingCall) -> PyResult<Next> {
        match call {
            Some((f, args)) => self.open_dunder(&f, args, Dunder::Discard),
            None => Ok(Next::Nothing),
        }
    }

    /// Começa a cadeia de tentativas de um operador, se alguma é função Python (senão `None`).
    fn begin_chain(&mut self, stack: &mut Vec<Slot>, kind: ChainKind, a: Value, b: Value, mut attempts: Vec<Attempt>) -> PyResult<Option<Next>> {
        if !attempts.iter().any(|at| dunder_function(&at.recv, at.name).is_some()) {
            return Ok(None);
        }
        stack.truncate(stack.len() - 2);
        attempts.reverse();
        self.advance_chain(Chain { kind, a, b, rest: attempts, invert: false }, None).map(Some)
    }

    /// Abre o quadro do método mágico `f`; o que ele devolver será tratado por `finish_dunder`.
    fn open_dunder(&mut self, f: &Rc<FuncObj>, args: Vec<Value>, then: Dunder) -> PyResult<Next> {
        match self.enter_function(f, args, Vec::new())? {
            Entered::Done(v) => self.finish_dunder(then, v),
            Entered::Frame(mut callee) => {
                callee.link.then = Some(then);
                Ok(Next::Spawn(callee))
            }
        }
    }

    /// O método mágico devolveu `v`: o que a instrução faz com ele.
    fn finish_dunder(&mut self, then: Dunder, v: Value) -> PyResult<Next> {
        match then {
            Dunder::Chain(chain) => self.advance_chain(chain, Some(v)),
            Dunder::Push => Ok(Next::Value(v)),
            Dunder::Discard | Dunder::Signal => Ok(Next::Nothing),
            Dunder::Boolean { negate } => Ok(Next::Value(Value::Bool(v.is_true() != negate))),
            Dunder::Iter => iterator_from_dunder(v).map(Next::Iter),
            Dunder::Callback(callback) => self.resume_callback(*callback, Some(v)),
            Dunder::Import(_) | Dunder::Exec(_) => Err(internal("a module body closed as a method")),
            Dunder::Truth { how, len } => {
                let truth = if len { len_result(v)? != 0 } else { bool_result(&v)? };
                Ok(match how {
                    TruthUse::Jump { target, when } if truth == when => Next::Jump(target),
                    TruthUse::JumpKeep { target, when } if truth == when => Next::Jump(target),
                    TruthUse::Jump { .. } => Next::Nothing,
                    TruthUse::JumpKeep { .. } => Next::Drop,
                    TruthUse::Not => Next::Value(Value::Bool(!truth)),
                })
            }
        }
    }

    /// Segue a cadeia de um operador: `returned` é o que a tentativa anterior devolveu (`NotImplemented`
    /// passa à próxima); esgotadas as tentativas, vale o operador embutido.
    fn advance_chain(&mut self, mut chain: Chain, returned: Option<Value>) -> PyResult<Next> {
        if let Some(v) = returned
            && !crate::classes::is_not_implemented(&v)
        {
            return Ok(Next::Value(chain.conclude(v)));
        }
        while let Some(at) = chain.rest.pop() {
            chain.invert = at.invert;
            if let Some(f) = dunder_function(&at.recv, at.name) {
                return self.open_dunder(&f, vec![at.recv, at.other], Dunder::Chain(chain));
            }
            match self.call_dunder(&at.recv, at.name, vec![at.other]) {
                None => {}
                Some(Err(e)) => return Err(e),
                Some(Ok(v)) if crate::classes::is_not_implemented(&v) => {}
                Some(Ok(v)) => return Ok(Next::Value(chain.conclude(v))),
            }
        }
        chain.fallback().map(Next::Value)
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
                vars.items.reserve(args.len() + 4);
                for (p, v) in code.params.iter().zip(args) {
                    vars.insert(p.clone(), v);
                }
            }
            return Ok(env);
        }
        // Quase tão comum: só posicionais, faltando apenas parâmetros com padrão (e os só-nomeados,
        // se houver, todos com padrão). Sem `*args`/`**kw`.
        if kwargs.is_empty()
            && args.len() <= code.params.len()
            && args.len() + f.defaults.len() >= code.params.len()
            && code.vararg.is_none()
            && code.kwarg.is_none()
            && code.kwonly.iter().all(|k| f.kwdefaults.iter().any(|(n, _)| n.as_str() == &**k))
        {
            let env = Env::new(f.closure.clone(), false, false);
            {
                let mut vars = env.vars.borrow_mut();
                vars.items.reserve(code.params.len() + code.kwonly.len() + 4);
                let given = args.len();
                let first_default = code.params.len() - f.defaults.len();
                for (p, v) in code.params.iter().zip(args) {
                    vars.insert(p.clone(), v);
                }
                for (i, p) in code.params.iter().enumerate().skip(given) {
                    vars.insert(p.clone(), f.defaults[i - first_default].clone());
                }
                for k in &code.kwonly {
                    if let Some((_, v)) = f.kwdefaults.iter().find(|(n, _)| n.as_str() == &**k) {
                        vars.insert(k.clone(), v.clone());
                    }
                }
            }
            return Ok(env);
        }
        let name = f.qualname();
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
        // Poucos nomes: um `Vec` sai mais barato que um mapa com hasher.
        let mut kwonly_vals: Vec<(Rc<str>, Value)> = Vec::new();
        let mut extra_kw: Vec<(String, Value)> = Vec::new();
        let mut posonly_given: Vec<String> = Vec::new();
        for (k, v) in kwargs {
            match params.iter().position(|p| **p == *k) {
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
                None if code.kwonly.iter().any(|x| **x == *k) => {
                    if kwonly_vals.iter().any(|(n, _)| **n == *k) {
                        return Err(type_error(format!("{name}() got multiple values for argument '{k}'")));
                    }
                    let key = code.kwonly.iter().find(|x| ***x == *k).cloned().unwrap_or_else(|| Rc::from(k.as_str()));
                    kwonly_vals.push((key, v));
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
                    missing.push(params[i].to_string());
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
            if !kwonly_vals.iter().any(|(n, _)| n == k) {
                match f.kwdefaults.iter().find(|(n, _)| n.as_str() == &**k) {
                    Some((_, v)) => {
                        kwonly_vals.push((k.clone(), v.clone()));
                    }
                    None => missing_kw.push(k.to_string()),
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
            vars.items.reserve(n + code.kwonly.len() + 6);
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
                let mut d = Dict::default();
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
                let armed = crate::globalsview::ARMED.load(std::sync::atomic::Ordering::Relaxed);
                let copy = armed.then(|| v.clone());
                {
                    let mut g = self.globals.borrow_mut();
                    match g.get_mut(name) {
                        Some(slot) => *slot = v,
                        None => {
                            g.insert(name.clone(), v);
                        }
                    }
                }
                if let Some(c) = copy {
                    crate::globalsview::push(&self.globals, name, Some(&c));
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
                    let text = crate::format::percent_format_with(fmt.as_str(), &b, crate::format::PercentKind::Str, &mut |conv, v| match conv {
                        's' => self.str_of(v).map(Value::str),
                        'r' | 'a' => self.repr_of(v).map(Value::str),
                        'e' | 'E' | 'f' | 'F' | 'g' | 'G' => self.call(&crate::builtins::get("float").unwrap_or(Value::Builtin("float")), vec![v.clone()], Vec::new()),
                        _ => self.call(&crate::builtins::get("int").unwrap_or(Value::Builtin("int")), vec![v.clone()], Vec::new()),
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
            Op::Call { .. } | Op::CallMethod { .. } | Op::CallEx { .. } => return Err(internal("call outside the instruction loop")),
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
                let mut d = Dict::default();
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
            Op::SetupTry(_) | Op::PopBlock | Op::Nop | Op::Return => {}
            Op::Locals => {
                stack.push(Slot::Val(Value::dict(crate::frameobj::locals_dict(code, locals)?)));
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
                        let d = Value::dict(Dict::default());
                        if locals.is_module {
                            self.globals.borrow_mut().insert("__annotations__".into(), d.clone());
                        } else {
                            locals.set("__annotations__", d.clone());
                        }
                        match d {
                            Value::Dict(d) => d,
                            _ => return Err(internal("annotations dict")),
                        }
                    }
                };
                dict.borrow_mut().set(Value::str(name.to_string()), ann)?;
            }
            Op::StoreLocal(i) => {
                let v = pop(stack)?;
                locals.set_rc(&code.names[i as usize], v);
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
                let mut d = Dict::default();
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
                    self.globals.borrow_mut().shift_remove(name);
                    if crate::globalsview::ARMED.load(std::sync::atomic::Ordering::Relaxed) {
                        crate::globalsview::push(&self.globals, name, None);
                    }
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
            Op::ClearLocal(i) => {
                locals.vars.borrow_mut().remove(&code.names[i as usize]);
            }
            Op::EnterCell { key, parent } => {
                let parent = match parent {
                    Some(p) => Some(cell_scope(locals, &code.names[p as usize])?),
                    None => locals.capture(),
                };
                locals.set_rc(&code.names[key as usize], crate::classes::cell_value(Env::new(parent, false, false)));
            }
            Op::LoadCell { key, name } => {
                let cell = cell_scope(locals, &code.names[key as usize])?;
                let name = &code.names[name as usize];
                let found = cell.vars.borrow().get(name).cloned();
                match found {
                    Some(v) => stack.push(Slot::Val(v)),
                    None => {
                        return Err(exc(
                            "UnboundLocalError",
                            format!("cannot access local variable '{name}' where it is not associated with a value"),
                        ))
                    }
                }
            }
            Op::StoreCell { key, name } => {
                let v = pop(stack)?;
                cell_scope(locals, &code.names[key as usize])?.set_rc(&code.names[name as usize], v);
            }
            Op::BindCell(key) => {
                let Value::Function(f) = pop(stack)? else {
                    return Err(internal("BindCell without function"));
                };
                let closure = Some(cell_scope(locals, &code.names[key as usize])?);
                let bound = FuncObj {
                    code: f.code.clone(),
                    defaults: f.defaults.clone(),
                    kwdefaults: f.kwdefaults.clone(),
                    closure,
                    globals: f.globals.clone(),
                    attrs: RefCell::new(f.attrs.borrow().clone()),
                };
                stack.push(Slot::Val(Value::Function(Rc::new(bound))));
            }
            Op::DeleteGlobal(i) => {
                let name = &code.names[i as usize];
                if self.globals.borrow_mut().shift_remove(name).is_none() {
                    return Err(exc("NameError", format!("name '{name}' is not defined")));
                }
                if crate::globalsview::ARMED.load(std::sync::atomic::Ordering::Relaxed) {
                    crate::globalsview::push(&self.globals, name, None);
                }
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
                let items = iterate(&it)?;
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
                for item in iterate(&v)? {
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
            Op::CallKwSet | Op::CallKwMerge => {
                let callee = |stack: &[Slot]| match stack.len().checked_sub(3).map(|i| &stack[i]) {
                    Some(Slot::Val(f)) => Ok(f.clone()),
                    _ => Err(internal("bad value stack")),
                };
                let pairs = if matches!(op, Op::CallKwSet) {
                    let value = pop(stack)?;
                    let key = pop(stack)?;
                    vec![(key, value)]
                } else {
                    let m = pop(stack)?;
                    match mapping_pairs(&m)? {
                        Some(pairs) => pairs,
                        None => {
                            let f = callee(stack)?;
                            return Err(type_error(format!(
                                "{} argument after ** must be a mapping, not {}",
                                self.function_str(&f),
                                m.type_name()
                            )));
                        }
                    }
                };
                let Value::Dict(d) = top(stack)?.clone() else { return Err(internal("CallKw without dict")) };
                for (k, v) in pairs {
                    if d.borrow().contains(&k)? {
                        let f = callee(stack)?;
                        return Err(type_error(format!(
                            "{} got multiple values for keyword argument '{}'",
                            self.function_str(&f),
                            crate::object::to_str(&k)
                        )));
                    }
                    d.borrow_mut().set(k, v)?;
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
                let items = iterate(&v)?;
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
                let tb = exception_traceback(&exc_value);
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
                    || matches!(&e, Value::Instance(i) if i.class().mro().iter().any(|c| c.name == "StopAsyncIteration"));
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
                let entered = self.call(&enter, Vec::new(), Vec::new())?;
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
            Op::Import(_) | Op::ImportRel { .. } => return Err(internal("import outside the instruction loop")),
            Op::ImportStar => {
                let Value::Module(m) = pop(stack)? else {
                    return Err(internal("import * from a non-module"));
                };
                let own = m.attrs.borrow().clone();
                // Na ordem do dict do módulo (as globais dele); o que só existe nos atributos nativos vem depois.
                let mut ordered: Vec<(String, Value)> = Vec::new();
                if let Some(g) = self.module_globals.borrow().get(m.name) {
                    ordered.extend(g.borrow().iter().map(|(k, v)| (k.to_string(), v.clone())));
                }
                let seen: std::collections::HashSet<String> = ordered.iter().map(|(k, _)| k.clone()).collect();
                ordered.extend(own.into_iter().filter(|(k, _)| !seen.contains(k)));
                let attrs: indexmap::IndexMap<String, Value> = ordered.into_iter().collect();
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
                            globals.insert(n.into(), v.clone());
                        }
                    }
                    None => {
                        for (n, v) in attrs {
                            if !n.starts_with('_') {
                                globals.insert(n.into(), v);
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
                    // `import_from`: só o `AttributeError` cai para o submódulo; o resto (um `__getattr__`
                    // de módulo que falha de outro jeito) sobe.
                    Err(e) if e.kind != "AttributeError" && matches!(obj, Value::Module(_)) => return Err(e),
                    Err(_) if internal_private_attr(code, &obj, name).is_some() => {
                        stack.push(Slot::Val(internal_private_attr(code, &obj, name).unwrap_or(Value::None)));
                    }
                    Err(_) => {
                        let module = match &obj {
                            Value::Module(m) => m.name,
                            _ => "?",
                        };
                        // `from pacote import submodulo`: importa o submódulo.
                        let foreign_name = match &obj {
                            Value::Module(_) => None,
                            other => match self.load_attr(other, "__name__") {
                                Ok(Value::Str(s)) => Some(s.as_str().to_string()),
                                _ => None,
                            },
                        };
                        if let Some(parent) = foreign_name {
                            let full = format!("{parent}.{name}");
                            match crate::modules::import_value(self, &full) {
                                Ok(sub) => {
                                    stack.push(Slot::Val(sub));
                                    return Ok(None);
                                }
                                Err(e) if !(e.kind == "ModuleNotFoundError") => return Err(e),
                                Err(_) => {}
                            }
                        }
                        if let Value::Module(m) = &obj {
                            if m.attrs.borrow().contains_key("__path__")
                                || self.module_globals.borrow().get(m.name).is_some_and(|g| g.borrow().contains_key("__path__"))
                                || crate::modules::is_embedded_package(m.name)
                            {
                                let full = format!("{module}.{name}");
                                match crate::modules::import_value(self, &full) {
                                    Ok(sub) => {
                                        stack.push(Slot::Val(sub));
                                        return Ok(None);
                                    }
                                    // O submódulo existe mas falhou ao rodar: a exceção dele sobe.
                                    Err(e) if !(e.kind == "ModuleNotFoundError" && e.msg == format!("No module named '{full}'")) => {
                                        return Err(e);
                                    }
                                    Err(_) => {}
                                }
                            }
                        }
                        // O `PyModule_GetFilenameObject` lê o `__file__` do dict do módulo, sem passar pelo
                        // `__getattr__` dele (PEP 562).
                        let file = match &obj {
                            Value::Module(m) => {
                                let live = self.module_globals.borrow().get(m.name).cloned();
                                live.and_then(|g| g.borrow().get("__file__").cloned()).or_else(|| m.attrs.borrow().get("__file__").cloned())
                            }
                            other => self.load_attr(other, "__file__").ok(),
                        };
                        let location = match file {
                            Some(Value::Str(p)) => p.as_str().to_string(),
                            _ => "unknown location".to_string(),
                        };
                        let msg = format!("cannot import name '{name}' from '{module}' ({location})");
                        let x = ExcObj::new("ImportError", vec![Value::str(msg.clone())]);
                        {
                            let mut extra = x.extra.borrow_mut();
                            extra.push(("name", Value::str(module.to_string())));
                            extra.push(("name_from", Value::str(&**name)));
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
                let v = match self.load_attr(&obj, name) {
                    Err(e) => match internal_private_attr(code, &obj, name) {
                        Some(v) => v,
                        None => Err(tag_attribute_error(e, &obj, name))?,
                    },
                    Ok(v) => v,
                };
                stack.push(Slot::Val(v));
            }
            Op::LoadMethod(i) => {
                let obj = pop(stack)?;
                let name = &code.names[i as usize];
                if let Some(f) = plain_method(&obj, name) {
                    stack.push(Slot::Val(Value::Function(f)));
                    stack.push(Slot::Val(obj));
                } else {
                    let v = match self.load_attr(&obj, name) {
                        Err(e) => match internal_private_attr(code, &obj, name) {
                            Some(v) => v,
                            None => Err(tag_attribute_error(e, &obj, name))?,
                        },
                        Ok(v) => v,
                    };
                    stack.push(Slot::Val(v));
                    stack.push(Slot::Val(Value::Builtin(NO_SELF)));
                }
            }
            Op::UnpackSequence(n) => {
                let v = pop(stack)?;
                let n = n as usize;
                let items = iterate(&v)?;
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

    /// Chama um valor chamável; com `sys.setprofile` ligado, as nativas geram `c_call`, `c_return` e `c_exception`.
    pub(crate) fn call(&mut self, func: &Value, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
        if crate::tracing::reports_c_call(self, func) {
            return crate::tracing::c_call(self, func, |vm| vm.call_value(func, args, kwargs));
        }
        self.call_value(func, args, kwargs)
    }

    fn call_value(&mut self, func: &Value, args: Vec<Value>, kwargs: Vec<(String, Value)>) -> PyResult<Value> {
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
                return match i.class().lookup("__call__") {
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
                // `types.MethodType(função, instância)`, na ordem do CPython.
                return match args.as_slice() {
                    [Value::Function(f), recv] if !matches!(recv, Value::None) => {
                        Ok(Value::BoundFn(Rc::new((recv.clone(), f.clone()))))
                    }
                    [_, Value::None] => Err(type_error("instance must not be None")),
                    [func, recv] if crate::builtins::is_callable(func) => {
                        Ok(Value::Ext(Rc::new(crate::classes::BoundCallable { recv: recv.clone(), func: func.clone() })))
                    }
                    [func, _] => Err(type_error(format!("first argument must be callable, not {}", func.type_name()))),
                    _ => Err(type_error(format!("method expected 2 arguments, got {}", args.len()))),
                };
            }
            Value::Builtin("cell") => {
                if let Some((k, _)) = kwargs.first() {
                    return Err(type_error(format!("cell() takes no keyword arguments ('{k}' given)")));
                }
                return crate::classes::new_cell(&args);
            }
            // `types.GenericAlias(origem, args)`, o mesmo que `origem[args]`.
            Value::Builtin("GenericAlias") => {
                if !kwargs.is_empty() {
                    return Err(type_error("GenericAlias() takes no keyword arguments"));
                }
                return match args.as_slice() {
                    [origin, key] => Ok(crate::generic::GenericAlias::make(origin.clone(), key)),
                    _ => Err(type_error(format!("GenericAlias expected 2 arguments, got {}", args.len()))),
                };
            }
            Value::Builtin(name @ ("staticmethod" | "classmethod" | "property" | "super" | "type" | "object")) => {
                return self.call_class_builtin(name, args, kwargs);
            }
            _ => {}
        }
        if let Value::Bound(b) = func {
            if let Value::Ext(e) = &b.recv {
                // `__aiter__` de um iterador assíncrono nativo e `__await__` do aguardável de um
                // gerador assíncrono devolvem o próprio objeto.
                if matches!(b.name, "__aiter__" | "__await__") && args.is_empty() {
                    let probe = e.clone().call_method(self, b.name, Vec::new(), Vec::new());
                    if matches!(&probe, Err(x) if x.msg.ends_with("returns self")) {
                        return Ok(b.recv.clone());
                    }
                    return probe;
                }
                let e = e.clone();
                // Um mágico que o objeto herda de `object` ou do tipo e não implementa (`iter([]).__eq__`).
                if !e.methods().contains(&b.name) {
                    if let Some((_, f)) = crate::methods::lookup(&b.recv, b.name) {
                        let mut full = Vec::with_capacity(args.len() + 1);
                        full.push(b.recv.clone());
                        full.extend(args);
                        return f(self, full, kwargs);
                    }
                }
                return e.call_method(self, b.name, args, kwargs);
            }
            // `f.__call__(...)` de uma função chama a função (o método-wrapper `__call__` do tipo `function`).
            if b.name == "__call__" && matches!(b.recv, Value::Function(_)) {
                return self.call(&b.recv, args, kwargs);
            }
            // `f.__get__(obj, tipo)`: o método ligado a `obj`, ou a própria função quando `obj` é `None`.
            if let (Value::Function(f), "__get__") = (&b.recv, b.name) {
                return match args.as_slice() {
                    [Value::None, ..] if args.len() <= 2 => Ok(b.recv.clone()),
                    [obj] | [obj, _] => Ok(Value::BoundFn(Rc::new((obj.clone(), f.clone())))),
                    _ => Err(type_error(format!("expected 1 or 2 arguments, got {}", args.len()))),
                };
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
            // `ImportError(msg, name=..., path=...)`: o `kwlist` do `ImportError_init` do CPython.
            let mut import_extra: Vec<(&'static str, Value)> = Vec::new();
            if exc_is_subclass(kind, "ImportError") {
                for (k, v) in &kwargs {
                    let key = match k.as_str() {
                        "name" => "name",
                        "path" => "path",
                        "name_from" => "name_from",
                        _ => {
                            return Err(type_error(format!("ImportError() got an unexpected keyword argument '{k}'")))
                        }
                    };
                    import_extra.push((key, v.clone()));
                }
                let e = ExcObj::new(kind, args);
                *e.extra.borrow_mut() = import_extra;
                return Ok(Value::Exception(Rc::new(e)));
            }
            // `AttributeError(msg, name=..., obj=...)` e `NameError(msg, name=...)`: o `kwlist` do `_init` do CPython.
            let member_keys: &[&'static str] = if exc_is_subclass(kind, "AttributeError") {
                &["name", "obj"]
            } else if exc_is_subclass(kind, "NameError") {
                &["name"]
            } else {
                &[]
            };
            if !member_keys.is_empty() && !kwargs.is_empty() {
                let mut members: Vec<(&'static str, Value)> = Vec::new();
                for (k, v) in &kwargs {
                    match member_keys.iter().find(|m| **m == k.as_str()) {
                        Some(key) => members.push((*key, v.clone())),
                        None => return Err(type_error(format!("'{k}' is an invalid keyword argument for {name}()"))),
                    }
                }
                let e = ExcObj::new(kind, args);
                *e.extra.borrow_mut() = members;
                return Ok(Value::Exception(Rc::new(e)));
            }
            if let Some((kw, _)) = kwargs.first() {
                return Err(type_error(format!("{name}() takes no keyword arguments ('{kw}' given)")));
            }
            // `OSError(errno, msg)` escolhe a subclasse pelo errno, como `OSError_new` do CPython.
            let kind = match (*kind, args.as_slice()) {
                ("OSError", [Value::Int(code), _, ..]) => match *code {
                    1 | 13 => "PermissionError",
                    2 => "FileNotFoundError",
                    3 => "ProcessLookupError",
                    4 => "InterruptedError",
                    10 => "ChildProcessError",
                    11 | 114 | 115 => "BlockingIOError",
                    17 => "FileExistsError",
                    20 => "NotADirectoryError",
                    21 => "IsADirectoryError",
                    32 | 108 => "BrokenPipeError",
                    103 => "ConnectionAbortedError",
                    104 => "ConnectionResetError",
                    110 => "TimeoutError",
                    111 => "ConnectionRefusedError",
                    _ => "OSError",
                },
                (k, _) => k,
            };
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
                    if inst.class().builtin_base.is_some() {
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

    pub(crate) fn handled_len(&self) -> usize {
        self.handled.borrow().len()
    }

    /// Tira da pilha as exceções tratadas a partir de `at` (as de um gerador que se suspende ou
    /// termina dentro de um `except`).
    pub(crate) fn handled_split(&self, at: usize) -> Vec<Value> {
        let mut h = self.handled.borrow_mut();
        let at = at.min(h.len());
        h.split_off(at)
    }

    pub(crate) fn handled_extend(&self, v: Vec<Value>) {
        self.handled.borrow_mut().extend(v);
    }

    /// Acrescenta ao buffer do stdout com a política do CPython: num terminal, descarrega a cada
    /// quebra de linha; num pipe ou arquivo, em blocos de 8 KiB. O resto sai no `flush` ou no fim.
    pub(crate) fn push_stdout(&self, data: &[u8]) -> PyResult<()> {
        self.stdout.borrow_mut().extend_from_slice(data);
        let Some(sys) = sysabi::sys::try_current() else { return Ok(()) };
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
                self.flush_stdout()?;
            }
            return Ok(());
        }
        const BLOCK: usize = 8192;
        let mut buf = self.stdout.borrow_mut();
        if buf.len() >= BLOCK {
            let n = buf.len() - buf.len() % BLOCK;
            sysabi::sys::write_all(sysabi::Fd::STDOUT, &buf[..n]).map_err(|e| crate::modules::osnative::os_error(e, None))?;
            buf.drain(..n);
        }
        Ok(())
    }

    /// Escreve `text` (codificação interna dos `str`) no stdout em UTF-8, com os erros do
    /// `sys.stdout` (`surrogateescape`): surrogate solitário fora de U+DC80..U+DCFF levanta
    /// `UnicodeEncodeError` sem escrever nada do texto.
    pub(crate) fn write_stdout_text(&self, text: &str) -> PyResult<()> {
        self.push_stdout(&crate::textcodec::encode_utf8(text, "surrogateescape")?)
    }

    /// Descarrega o stdout pendente (`print(flush=True)`, `sys.stdout.flush()`). Sem pseudo-processo
    /// (os testes de unidade), o buffer fica como está para o chamador ler. Como o `BufferedWriter`,
    /// uma escrita que falha (`EPIPE` com o leitor já fechado) levanta o `OSError` e deixa o buffer.
    pub(crate) fn flush_stdout(&self) -> PyResult<()> {
        if sysabi::sys::try_current().is_none() {
            return Ok(());
        }
        let mut buf = self.stdout.borrow_mut();
        if !buf.is_empty() {
            sysabi::sys::write_all(sysabi::Fd::STDOUT, &buf).map_err(|e| crate::modules::osnative::os_error(e, None))?;
            buf.clear();
        }
        Ok(())
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
        let to_real_stdout = file.is_none() && self.redirected_stdout().is_none();
        for a in &args {
            let part = to_str(a);
            let encoded = if to_real_stdout { crate::textcodec::encode_utf8(&part, "surrogateescape").err() } else { None };
            if let Some(e) = take_text_error().or(encoded) {
                failure = Some(e);
                break;
            }
            parts.push(part);
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
                    let m = self.load_attr(&f, "flush")?;
                    self.call(&m, Vec::new(), Vec::new())?;
                }
            }
            None => match self.redirected_stdout() {
                Some(f) => {
                    self.write_to(&f, &text)?;
                }
                None => {
                    self.write_stdout_text(&text)?;
                    if flush {
                        self.flush_stdout()?;
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
            return Ok(crate::object::code_points(text).count());
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
            FileKind::Stdout => self.write_stdout_text(text)?,
            FileKind::Stderr => {
                // O stderr do CPython é sem buffer e independe do stdout: com stdout em pipe, o que está
                // pendente só sai no fim (ou a cada 8 KiB), depois do que o stderr já escreveu.
                if sysabi::sys::try_current().is_none() {
                    self.stderr_capture.borrow_mut().push_str(text);
                } else {
                    // O `sys.stderr` do CPython usa `backslashreplace`: surrogate solitário sai como `\ud800`.
                    let data = crate::textcodec::encode_utf8(text, "backslashreplace")?;
                    let _ = sysabi::sys::write_all(sysabi::Fd::STDERR, &data);
                }
            }
            _ => return Err(exc("UnsupportedOperation", "not writable")),
        }
        Ok(crate::object::code_points(text).count())
    }

    /// `_PyObject_FunctionStr` do CPython: `modulo.qualname()`, sem o módulo quando é `builtins`.
    fn function_str(&mut self, f: &Value) -> String {
        let text = |v: PyResult<Value>| match v {
            Ok(Value::Str(s)) => Some(s.as_str().to_string()),
            _ => None,
        };
        let Some(qualname) = text(self.load_attr(f, "__qualname__")) else { return crate::object::to_str(f) };
        match text(self.load_attr(f, "__module__")) {
            Some(module) if module != "builtins" => format!("{module}.{qualname}()"),
            _ => format!("{qualname}()"),
        }
    }

    pub(crate) fn load_attr(&mut self, obj: &Value, name: &str) -> PyResult<Value> {
        let missing = || {
            crate::object::no_attribute(obj.type_name(), name)
        };
        match obj {
            Value::Instance(inst) => return self.instance_getattr(obj, inst, name),
            Value::Class(c) => {
                if let Some(descriptor) = crate::builtins_ext::emulated_class_attr(c, name) {
                    return Ok(descriptor);
                }
                return self.class_getattr(c, name);
            }
            // `(1).__new__` é o `int.__new__`: o método estático do tipo, lido pela instância.
            Value::Int(_) | Value::Big(_) | Value::Float(_) | Value::Bool(_) | Value::Str(_) | Value::Bytes(_)
            | Value::List(_) | Value::Tuple(_) | Value::Dict(_) | Value::Set(_) | Value::ByteArray(_) | Value::Range(_)
            | Value::Slice(_) | Value::None
                if name == "__new__" =>
            {
                let ty = self.type_of(obj);
                return self.load_attr(&ty, "__new__");
            }
            // Os construtores alternativos do tipo, lidos pela instância (`(0).from_bytes`, `(1.5).fromhex`).
            Value::Int(_) | Value::Big(_) | Value::Float(_) | Value::Bool(_) | Value::Str(_) | Value::Bytes(_)
            | Value::Dict(_) | Value::ByteArray(_)
                if matches!(name, "from_bytes" | "fromhex" | "maketrans" | "fromkeys")
                    && crate::typeattrs::type_attr(obj.type_name(), name).is_some() =>
            {
                return crate::typeattrs::type_attr(obj.type_name(), name).ok_or_else(|| missing());
            }
            Value::Builtin(n) if crate::object::native_type_method(n, name).is_some() => {
                let method = crate::object::native_type_method(n, name).unwrap_or_default();
                return Ok(Value::Ext(Rc::new(crate::classes::NativeTypeMethod { owner: n, name: method })));
            }
            Value::Builtin(_) | Value::NativeFn(_)
                if name == "__flags__" && builtin_type_name(obj).and_then(crate::builtins_ext::builtin_type_flags).is_some() =>
            {
                let flags = builtin_type_name(obj).and_then(crate::builtins_ext::builtin_type_flags);
                return Ok(Value::Int(flags.unwrap_or_default()));
            }
            Value::Builtin(_) | Value::NativeFn(_)
                if name == "__dict__" && (crate::builtins::class_name(obj).is_some() || matches!(obj, Value::Builtin("type"))) =>
            {
                return crate::builtins_ext::type_own_dict(self, obj);
            }
            // `function.__code__` (um `getset_descriptor`) e `function.__globals__` (um `member_descriptor`): os
            // campos de dados do tipo `function`, lidos pelo tipo (o `types.GetSetDescriptorType` sai daqui).
            Value::Builtin("function")
                if matches!(
                    name,
                    "__code__" | "__globals__" | "__defaults__" | "__kwdefaults__" | "__annotations__" | "__closure__"
                        | "__builtins__" | "__type_params__"
                ) =>
            {
                let key = crate::builtins_ext::own_type_keys("function").and_then(|keys| keys.into_iter().find(|k| *k == name));
                if let Some(descriptor) = key.and_then(|k| crate::typeattrs::descriptor_for_kind("function", k, obj)) {
                    return Ok(descriptor);
                }
            }
            // `function.__get__` e `type.__subclasscheck__`: os slots e métodos que a tabela do CPython lista
            // para esses tipos são descritores (o `inspect` liga `__init__` por `type(f).__get__`, e o
            // `typing._ProtocolMeta` chama `type.__subclasscheck__(cls, other)`).
            Value::Builtin(n @ ("function" | "type"))
                if !matches!(name, "__getattribute__" | "__setattr__" | "__delattr__")
                    && !(*n == "type" && matches!(name, "__call__" | "__init__" | "__repr__" | "__or__" | "__ror__"))
                    && crate::builtins_ext::type_var_kind(n, name)
                        .is_some_and(|k| matches!(k, "wrapper_descriptor" | "method_descriptor")) =>
            {
                return Ok(crate::typeattrs::unbound(n, crate::object::intern(name)));
            }
            Value::Builtin(_) | Value::NativeFn(_)
                if matches!(name, "__getattribute__" | "__setattr__" | "__delattr__")
                    && crate::builtins::class_name(obj).is_some() =>
            {
                // `tuple.__getattribute__(self, nome)` e companhia: os ganchos de atributo vêm de `object`
                // (o descritor do tipo embutido é o `slot wrapper` de `object`; o próprio `object` fica nativo,
                // para uma sobrescrita do usuário não recursar).
                let tname = match obj {
                    Value::Builtin(n) => *n,
                    Value::NativeFn(f) => f.name,
                    _ => "",
                };
                let descriptor = Some(tname).filter(|t| *t != "object").and_then(|t| crate::typeattrs::type_attr(t, name));
                if let Some(v) = descriptor.or_else(|| crate::typeattrs::object_attr(name)) {
                    return Ok(v);
                }
            }
            Value::Builtin("object")
                if !matches!(name, "__name__" | "__qualname__" | "__mro__" | "__bases__" | "__module__" | "__doc__" | "__text_signature__") =>
            {
                if let Some(v) = crate::typeattrs::object_type_attr(name) {
                    return Ok(v);
                }
            }
            Value::Builtin(n) if name == "__init__" && EXC_CLASSES.iter().any(|(k, _)| k == n) => {
                return Ok(Value::Builtin("BaseException.__init__"));
            }
            // `BaseException.__reduce__(e)` e `__setstate__(e, state)`: as funções de `copyreg`; o
            // `__reduce_ex__` é o de `object` (`copy.copy` o lê pela classe).
            Value::Builtin(n) if name == "__reduce_ex__" && EXC_CLASSES.iter().any(|(k, _)| k == n) => {
                if let Some(v) = crate::typeattrs::object_attr(name) {
                    return Ok(v);
                }
            }
            Value::Builtin(n)
                if matches!(
                    name,
                    "__reduce__" | "__setstate__" | "__repr__" | "__str__" | "add_note" | "with_traceback" | "args"
                        | "__cause__" | "__context__" | "__suppress_context__" | "__traceback__"
                ) && EXC_CLASSES.iter().any(|(k, _)| k == n) =>
            {
                if let Some(v) = crate::typeattrs::exception_attr(n, name) {
                    return Ok(v);
                }
            }
            Value::Builtin(n) if name == "__new__" && EXC_CLASSES.iter().any(|(k, _)| k == n) => {
                return Ok(Value::NativeFn(Rc::new(crate::object::NativeFn { name: "__new__", f: base_exception_new })));
            }
            Value::Builtin(n) if name == "__name__" || name == "__qualname__" => {
                return Ok(Value::str(n.rsplit('.').next().unwrap_or(n)))
            }
            Value::Builtin(_) | Value::NativeFn(_)
                if matches!(name, "__mro__" | "__bases__")
                    && (crate::builtins::class_name(obj).is_some() || matches!(obj, Value::Builtin("type"))) =>
            {
                // `type` é o metatipo: fora de `class_name`, mas o `__mro__` dele é `(type, object)`.
                let n = &crate::builtins::class_name(obj).unwrap_or("type");
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
            Value::Builtin("type") if name == "__new__" => {
                return Ok(Value::NativeFn(Rc::new(crate::object::NativeFn { name: "__new__", f: crate::classes::type_new })));
            }
            // Classe embutida com módulo próprio (`_csv.Error`, `binascii.Error`): o nome vem qualificado pelo módulo.
            Value::Builtin(b) if name == "__module__" && b.contains('.') && !b.contains("__") => {
                return Ok(Value::str(b.rsplit_once('.').map_or("builtins", |(module, _)| module)))
            }
            // Os tipos de anotação (`list[int]`, `int | str`) nascem em `types`.
            Value::Builtin("GenericAlias" | "UnionType") if name == "__module__" => return Ok(Value::str("types")),
            // Tipos embutidos escritos como função nativa (`slice`) têm o mesmo `__module__` dos demais.
            Value::Builtin(_) | Value::NativeFn(_)
                if name == "__module__" && (matches!(obj, Value::Builtin(_)) || crate::builtins::class_name(obj).is_some()) =>
            {
                return Ok(Value::str("builtins"))
            }
            // Funções e tipos embutidos: a docstring do CPython (`builtins` na tabela).
            Value::Builtin(b) if name == "__doc__" => {
                return Ok(crate::modules::cpydocs::builtin_doc(b).map_or(Value::None, Value::str));
            }
            // `float.__text_signature__`: a assinatura que o CPython tira do `tp_doc` do tipo.
            Value::Builtin(_) | Value::NativeFn(_)
                if name == "__text_signature__"
                    && crate::builtins::class_name(obj).and_then(crate::typeattrs::type_text_signature).is_some() =>
            {
                return Ok(crate::builtins::class_name(obj).and_then(crate::typeattrs::type_text_signature).map_or(Value::None, Value::str));
            }
            Value::Builtin(b) if name == "__text_signature__" && crate::builtins::class_name(obj).is_none() => {
                return Ok(crate::modules::cpydocs::module_function_signature("builtins", b).map_or(Value::None, Value::str));
            }
            Value::NativeFn(f) if name == "__doc__" => {
                let doc = crate::modules::cpydocs::native_doc(f).or_else(|| {
                    let is_type = crate::typeattrs::TYPES.contains(&f.name) || crate::object::is_builtin_type(f.name);
                    is_type.then(|| crate::modules::cpydocs::builtin_doc(f.name)).flatten()
                });
                return Ok(doc.map_or(Value::None, Value::str));
            }
            Value::NativeFn(f) => {
                if let Some(v) = crate::typeattrs::type_attr(f.name, name) {
                    return Ok(v);
                }
                if matches!(name, "__name__" | "__qualname__") {
                    return Ok(Value::str(f.name.rsplit('.').next().unwrap_or(f.name)));
                }
                if name == "__text_signature__" && crate::builtins::class_name(obj).is_none() {
                    let sig = crate::modules::cpydocs::native_signature(f)
                        .or_else(|| crate::builtins::get(f.name).and_then(|_| crate::modules::cpydocs::module_function_signature("builtins", f.name)));
                    return Ok(sig.map_or(Value::None, Value::str));
                }
                // Função embutida de módulo (`len`, `math.sqrt`): `__module__` e `__self__` são o módulo
                // dono; as `builtins` valem para o que a tabela de `builtins` tem.
                if matches!(name, "__module__" | "__self__") && crate::builtins::class_name(obj).is_none() {
                    let owner = crate::modules::cpydocs::native_owner(f)
                        .or_else(|| crate::builtins::get(f.name).map(|_| "builtins".to_string()));
                    return match (owner, name) {
                        (Some(owner), "__module__") => Ok(Value::str(owner)),
                        (Some(owner), _) => crate::modules::import_checked(self, &owner).map(Value::Module),
                        // Sem módulo dono (os tratadores de erro de codec): `m_module` e `m_self` NULL.
                        (None, _) => Ok(Value::None),
                    };
                }
            }
            Value::Exception(e) if name == "__class__" => return Ok(Value::Builtin(e.kind)),
            // Função de módulo em C do CPython escrita em Python aqui: `__module__` é o módulo C e `__self__` é ele.
            Value::Function(f) if matches!(name, "__module__" | "__self__") && f.c_owner().is_some() => {
                let owner = f.c_owner().unwrap_or_default();
                return if name == "__module__" { Ok(Value::str(owner)) } else { crate::modules::import_checked(self, &owner).map(Value::Module) };
            }
            // A assinatura de texto da função de C vem da tabela gerada no oráculo, pelo módulo C dono.
            Value::Function(f) if name == "__text_signature__" && f.c_owner().is_some() => {
                let owner = f.c_owner().unwrap_or_default();
                return Ok(crate::modules::cpydocs::module_function_signature(&owner, f.plain_qual()).map_or(Value::None, Value::str));
            }
            // O `__text_signature__` de uma função de C: a tabela gerada no oráculo, pelo módulo C e pelo nome.
            Value::Function(f) if name == "__text_signature__" && f.c_owner().is_some() => {
                let owner = f.c_owner().unwrap_or_default();
                return Ok(crate::modules::cpydocs::module_function_signature(&owner, f.plain_qual()).map_or(Value::None, Value::str));
            }
            Value::Function(f) => {
                if let Some(v) = f.attrs.borrow().get(name) {
                    return Ok(v.clone());
                }
                match name {
                    // Uma função de C não tem código, globais, padrões, anotações nem `__dict__`: cai no AttributeError.
                    "__code__" | "__globals__" | "__defaults__" | "__kwdefaults__" | "__annotations__" | "__dict__" | "__closure__"
                    | "__builtins__" | "__type_params__"
                        if f.is_c_function() => {}
                    // `f.__call__(...)` chama `f` (o `unittest.mock` testa `__call__` para saber se é chamável):
                    // no CPython é o `method-wrapper` do slot `tp_call` de `function`.
                    "__call__" => return Ok(Value::Bound(Rc::new(BoundMethod { recv: obj.clone(), name: "__call__" }))),
                    // `function.__get__`: o `method-wrapper` do descritor, que `singledispatchmethod` chama.
                    "__get__" if !f.is_c_function() => {
                        return Ok(Value::Bound(Rc::new(BoundMethod { recv: obj.clone(), name: "__get__" })))
                    }
                    "__name__" => return Ok(Value::str(f.code.name.clone())),
                    "__qualname__" => return Ok(Value::str(f.qualname())),
                    "__defaults__" => {
                        return Ok(if f.defaults.is_empty() { Value::None } else { Value::tuple(f.defaults.clone()) })
                    }
                    "__kwdefaults__" => {
                        if f.kwdefaults.is_empty() {
                            return Ok(Value::None);
                        }
                        let mut d = Dict::default();
                        for (k, v) in &f.kwdefaults {
                            d.set(Value::str(k.clone()), v.clone())?;
                        }
                        return Ok(Value::dict(d));
                    }
                    "__doc__" => return Ok(f.code.doc.clone().map_or(Value::None, Value::str)),
                    "__globals__" => return Ok(crate::globalsview::view_for(&f.globals, None)),
                    "__builtins__" => {
                        if let Some(builtins) = crate::modules::builtins_dict(self) {
                            return Ok(builtins);
                        }
                    }
                    // Uma `cell` por variável livre (`co_freevars`); sem nenhuma, `None`.
                    "__closure__" => return Ok(crate::classes::function_closure(f)),
                    "__type_params__" => return Ok(Value::tuple(Vec::new())),
                    "__annotations__" => {
                        let d = Value::dict(Dict::default());
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
                        let mut d = Dict::default();
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
            Value::Bound(b) => {
                // `__self__`, `__name__`, `__qualname__`, `__objclass__` (só nos wrappers de slot), `__module__`
                // e `__text_signature__` do método embutido ligado.
                if let Some(v) = crate::typeattrs::bound_method_attr(&b.recv, b.name, name) {
                    return Ok(v);
                }
                match name {
                    // Os métodos das extensões em C do Pillow (`Font.render`, `ImagingCore.getpixel`...)
                    // têm `ml_doc` NULL.
                    "__doc__" if matches!(b.recv, Value::Ext(_)) => return Ok(Value::None),
                    // `[].append.__doc__`, `(0).__add__.__doc__`: o do método do tipo embutido do receptor.
                    "__doc__" if crate::typeattrs::TYPES.contains(&b.recv.type_name()) => {
                        return Ok(crate::typeattrs::method_doc(b.recv.type_name(), b.name).map_or(Value::None, Value::str))
                    }
                    _ => {}
                }
            }
            Value::BoundFn(b) => match name {
                "__doc__" | "__module__" | "__qualname__" | "__code__" | "__dict__" => {
                    return self.load_attr(&Value::Function(b.1.clone()), name)
                }
                "__name__" => return Ok(Value::str(b.1.code.name.clone())),
                "__self__" => return Ok(b.0.clone()),
                "__func__" => return Ok(Value::Function(b.1.clone())),
                "__call__" => return Ok(obj.clone()),
                _ => {}
            },
            Value::Slice(s) => match name {
                "start" => return Ok(s.0.clone()),
                "stop" => return Ok(s.1.clone()),
                "step" => return Ok(s.2.clone()),
                "indices" => return Ok(Value::Ext(Rc::new(crate::classes::SliceIndices(s.clone())))),
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
            Value::Exception(_) if name == "add_note" => {
                Ok(Value::Ext(Rc::new(crate::classes::ExcAddNote { obj: obj.clone() })))
            }
            Value::Exception(e) if name == "__notes__" && e.extra_get("__notes__").is_some() => {
                Ok(e.extra_get("__notes__").unwrap_or(Value::None))
            }
            Value::Exception(e) if name == "__traceback__" => Ok(e.traceback.borrow().clone().unwrap_or(Value::None)),
            // Atributo que o programa gravou na exceção (`e.x = 5`), e o `__dict__` com todos eles.
            Value::Exception(e) if e.dict_get(name).is_some() => Ok(e.dict_get(name).unwrap_or(Value::None)),
            Value::Exception(e) if name == "__dict__" => {
                let mut d = Dict::default();
                for (k, v) in e.dict_items() {
                    d.set(Value::str(k), v)?;
                }
                Ok(Value::dict(d))
            }
            // `BaseException.__reduce__`, `__reduce_ex__` (o de `object`) e `__setstate__`: métodos embutidos
            // ligados (a lógica vive em `copyreg`, chamada por dentro do nativo).
            Value::Exception(_) if matches!(name, "__reduce__" | "__reduce_ex__" | "__setstate__" | "__getstate__") => {
                crate::classes::exception_method(obj, name).ok_or_else(|| missing())
            }
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
                if let Some(v) = e.extra_get(name) {
                    return Ok(v);
                }
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
            // `UnicodeEncodeError(encoding, object, start, end, reason)`, o `UnicodeDecodeError` e o
            // `UnicodeTranslateError(object, start, end, reason)`.
            Value::Exception(e)
                if matches!(name, "encoding" | "object" | "start" | "end" | "reason")
                    && matches!(
                        (e.kind, e.args.len()),
                        ("UnicodeEncodeError" | "UnicodeDecodeError", 5) | ("UnicodeTranslateError", 4)
                    ) =>
            {
                let fields: &[&str] = if e.kind == "UnicodeTranslateError" {
                    &["object", "start", "end", "reason"]
                } else {
                    &["encoding", "object", "start", "end", "reason"]
                };
                match fields.iter().position(|n| *n == name) {
                    Some(index) => Ok(e.args[index].clone()),
                    None => Err(missing()),
                }
            }
            Value::Exception(e) if name == "filename2" && exc_is_subclass(&e.kind, "OSError") => {
                Ok(e.args.get(4).cloned().unwrap_or(Value::None))
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
            // A função embutida de módulo (`codecs.encode`) também: o `__reduce__` devolve o nome, que o
            // pickle grava como global.
            Value::Builtin("Ellipsis") | Value::NativeFn(_)
                if matches!(name, "__reduce_ex__" | "__reduce__")
                    && (!matches!(obj, Value::NativeFn(_)) || crate::builtins::class_name(obj).is_none()) =>
            {
                let reducer = if name == "__reduce__" { "_builtin_reduce" } else { "_builtin_reduce_ex" };
                match crate::modules::pysrc::copyreg_helper(self, reducer)? {
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
                Ok(crate::methods::dunder::plain_int(obj))
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
                | Value::ByteArray(_) | Value::Range(_) | Value::Slice(_)
                if name == "__doc__" =>
            {
                // A docstring do tipo, como a que o CPython lê pela instância.
                Ok(crate::modules::cpydocs::builtin_doc(obj.type_name()).map_or(Value::None, Value::str))
            }
            v if name == "__class__" && !matches!(v, Value::Instance(_) | Value::Class(_) | Value::Exception(_)) => {
                Ok(self.type_of(v))
            }
            Value::Module(m) => {
                if let Some(found) = self.module_class_data_attr(m, name) {
                    return found;
                }
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
                    // O dict vivo das globais do módulo (escrever nele muda o módulo); os atributos de módulo
                    // nativo entram como complemento.
                    let live = self.module_globals.borrow().get(m.name).cloned();
                    if let Some(map) = live {
                        // `__loader__` e `__spec__` fazem parte do dict do módulo desde a criação.
                        if !map.borrow().contains_key("__spec__") {
                            let _ = self.module_spec(m, "__spec__");
                        }
                        let view = crate::globalsview::view_for(&map, Some(m.attrs.borrow().clone()));
                        crate::builtins_ext::module_dict_register(m.name, &view);
                        return Ok(view);
                    }
                    // Instantâneo dos atributos de um módulo só nativo.
                    let all: std::collections::BTreeMap<String, Value> = m.attrs.borrow().clone();
                    let mut d = Dict::default();
                    for (k, v) in all {
                        d.set(Value::str(k), v)?;
                    }
                    return Ok(crate::builtins_ext::module_dict_value(m.name, d));
                }
                let found = m.attrs.borrow().get(name).cloned();
                match found {
                    Some(v) => Ok(v),
                    // `object.__getstate__` (3.11+) também vale para o módulo: vive em `copyreg`.
                    None if name == "__getstate__" => {
                        match crate::modules::pysrc::copyreg_helper(self, "_object_getstate")? {
                            Value::Function(f) => Ok(Value::BoundFn(Rc::new((obj.clone(), f)))),
                            _ => self.module_missing_attr(m, name),
                        }
                    }
                    None => match self.module_class_attr(m, name) {
                        Some(attr) => attr,
                        None => self.module_missing_attr(m, name),
                    },
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
                            "errors" => {
                                // O `sys.stderr` do CPython usa `backslashreplace`; stdin e stdout, `surrogateescape`.
                                let errors = if f.kind == FileKind::Stderr { "backslashreplace" } else { "surrogateescape" };
                                return Ok(Value::str(errors));
                            }
                            "newlines" => return Ok(Value::None),
                            // O tipo do `sys.stdout` é o `_io.TextIOWrapper` (tipo de heap em C, com `__module__` no dict).
                            "__module__" => return Ok(Value::str("_io")),
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
                if e.type_name() == "object" {
                    if let Some(m) = crate::classes::plain_object_method(obj, name) {
                        return Ok(m);
                    }
                    // Atributos do tipo que não dependem da instância: o `__doc__` de `object` e os métodos de
                    // classe e estáticos (`__init_subclass__`, `__subclasshook__`, `__new__`).
                    if matches!(name, "__doc__" | "__init_subclass__" | "__subclasshook__" | "__new__") {
                        let ty = self.type_of(obj);
                        return self.load_attr(&ty, name);
                    }
                }
                if let Some(m) = e.methods().iter().find(|m| **m == name) {
                    return Ok(Value::Bound(Rc::new(BoundMethod { recv: obj.clone(), name: m })));
                }
                match e.clone().getattr(self, name) {
                    Some(r) => r,
                    // O que o `dir()` do tipo lista e o objeto não implementa: os mágicos de `object` e do tipo
                    // (`iter([]).__eq__`), ligados ao objeto como método embutido, mais `__doc__` e `__new__`.
                    None => match crate::methods::lookup(obj, name) {
                        Some((n, _)) => Ok(Value::Bound(Rc::new(BoundMethod { recv: obj.clone(), name: n }))),
                        None => crate::typeattrs::inherited_type_attr(obj, name).ok_or_else(missing),
                    },
                }
            }
            // Os tipos mutáveis não têm hash: `[].__hash__` é `None`, não um método.
            Value::List(_) | Value::Dict(_) | Value::ByteArray(_) if name == "__hash__" => Ok(Value::None),
            Value::Set(s) if name == "__hash__" && !s.borrow().is_frozen() => Ok(Value::None),
            _ => match crate::methods::lookup(obj, name) {
                Some((n, _)) => Ok(Value::Bound(Rc::new(BoundMethod { recv: obj.clone(), name: n }))),
                None => crate::methods::value_attr(obj, name)
                    .or_else(|| crate::typeattrs::inherited_type_attr(obj, name))
                    .ok_or_else(missing),
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
                for l in iterate(&lines)? {
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
                    self.flush_stdout()?;
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
                if let Some(k) = limit {
                    if matches!(&*n.borrow(), Native::File(f) if f.kind == FileKind::Stdin && !f.closed) {
                        return Ok(Value::str(crate::stdin::text_chars(n, k)?));
                    }
                }
                if matches!(&*n.borrow(), Native::File(f) if f.kind == FileKind::Stdin && !f.closed) {
                    // Uma operação só: parar por falta de entrada não deixa linha consumida para trás.
                    return Ok(Value::str(crate::stdin::text_all(n)?));
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
                let rows = if name == "writerow" { vec![row] } else { iterate(&row)? };
                let mut last = Value::None;
                for r in rows {
                    let fields = match &r {
                        Value::Str(_) | Value::Int(_) | Value::Float(_) | Value::Bool(_) | Value::None => {
                            return Err(exc("_csv.Error", format!("iterable expected, not {}", r.type_name())))
                        }
                        _ => iterate(&r)?,
                    };
                    let line = csv::writerow(&dialect, &fields).map_err(|e| exc("_csv.Error", e.msg))?;
                    last = Value::Int(self.write_to(&target, &line)? as i64);
                }
                Ok(if name == "writerow" { last } else { Value::None })
            }
            _ => Err(internal("unknown method")),
        }
    }

    /// O fim do `module_getattro` do CPython (PEP 562): o atributo ausente consulta o `__getattr__` do dict
    /// do módulo, chamado com o nome; sem ele, o `AttributeError` distingue o módulo ainda em importação.
    fn module_missing_attr(&mut self, m: &Rc<crate::object::ModuleObj>, name: &str) -> PyResult<Value> {
        let live = self.module_globals.borrow().get(m.name).cloned();
        let hook = live
            .as_ref()
            .and_then(|g| g.borrow().get("__getattr__").cloned())
            .or_else(|| m.attrs.borrow().get("__getattr__").cloned());
        if let Some(hook) = hook {
            return self.call(&hook, vec![Value::str(name)], Vec::new());
        }
        if let Some(found) = self.module_class_getattr(m, name) {
            return found;
        }
        let dict_name = live.as_ref().and_then(|g| g.borrow().get("__name__").cloned());
        let msg = match dict_name {
            Some(Value::Str(n)) if crate::modules::is_initializing(m.name) => format!(
                "partially initialized module '{}' has no attribute '{name}' (most likely due to a circular import)",
                n.as_str()
            ),
            Some(Value::Str(n)) => format!("module '{}' has no attribute '{name}'", n.as_str()),
            Some(_) => format!("module has no attribute '{name}'"),
            None => format!("module '{}' has no attribute '{name}'", m.name),
        };
        Err(exc("AttributeError", msg))
    }

    /// `__spec__`/`__loader__` de um módulo carregado de arquivo: construídos na primeira leitura
    /// por `_frozen_importlib_external` e guardados nas globais do módulo.
    fn module_spec(&mut self, m: &Rc<crate::object::ModuleObj>, name: &str) -> PyResult<Option<Value>> {
        let Some(globals) = self.module_globals.borrow().get(m.name).cloned() else {
            // Módulo nativo em Rust: os embutidos do executável do CPython ganham o spec do
            // `BuiltinImporter`, guardado nos atributos do próprio módulo.
            if !crate::object::BUILTIN_MODULES.contains(&m.name) {
                return Ok(None);
            }
            let Some(machinery) = crate::modules::import(self, "_frozen_importlib_external") else { return Ok(None) };
            let make = machinery.attrs.borrow().get("_spec_for_module").cloned();
            let Some(make) = make else { return Ok(None) };
            let spec = self.call(&make, vec![Value::str(m.name), Value::str(""), Value::Bool(false)], Vec::new())?;
            let loader = self.load_attr(&spec, "loader")?;
            let mut attrs = m.attrs.borrow_mut();
            attrs.insert("__spec__".into(), spec.clone());
            attrs.insert("__loader__".into(), loader.clone());
            return Ok(Some(if name == "__spec__" { spec } else { loader }));
        };
        let (file, is_package) = {
            let g = globals.borrow();
            match g.get("__file__") {
                Some(Value::Str(f)) => (f.as_str().to_string(), g.contains_key("__path__")),
                _ if crate::object::BUILTIN_MODULES.contains(&m.name) => (String::new(), false),
                _ => return Ok(None),
            }
        };
        let Some(machinery) = crate::modules::import(self, "_frozen_importlib_external") else { return Ok(None) };
        let make = machinery.attrs.borrow().get("_spec_for_module").cloned();
        let Some(make) = make else { return Ok(None) };
        // O `site` e os demais congelados rodam do texto do disco, mas o spec é o do `FrozenImporter`.
        let frozen = file.starts_with("/usr/lib/python3.13/") && crate::object::FROZEN_MODULES.contains(&m.name);
        let spec = self.call(&make, vec![Value::str(m.name), Value::str(file), Value::Bool(is_package), Value::Bool(frozen)], Vec::new())?;
        let loader = self.load_attr(&spec, "loader")?;
        let mut g = globals.borrow_mut();
        // No CPython `__loader__` e `__spec__` já estão no dict desde a criação do módulo, logo depois do
        // `__package__`: inseridos aqui, vão para essa posição.
        let mut at = g.get_index_of("__package__").map(|p| p + 1);
        for (key, val) in [("__loader__", loader.clone()), ("__spec__", spec.clone())] {
            let fresh = !g.contains_key(key);
            g.insert(key.into(), val);
            if let (true, Some(pos)) = (fresh, at) {
                let last = g.len() - 1;
                g.move_index(last, pos.min(last));
                at = Some(pos + 1);
            }
        }
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
        let one_char = |k: &str, v: &Value| -> PyResult<Option<u32>> {
            match v {
                Value::None => Ok(None),
                Value::Str(s) if s.len() == 1 => Ok(s.cp_at(0)),
                Value::Str(s) => Err(type_error(format!("\"{k}\" must be a unicode character or None, not a string of length {}", s.len()))),
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


fn file_readline(n: &Rc<RefCell<Native>>) -> PyResult<Option<String>> {
    if matches!(&*n.borrow(), Native::File(f) if f.kind == FileKind::Stdin && !f.closed) {
        // Incremental: um pipe vivo entrega as linhas conforme chegam (ver `stdin.rs`).
        return crate::stdin::text_line(n);
    }
    let mut b = n.borrow_mut();
    let Native::File(f) = &mut *b else { return Err(type_error("not a file")) };
    if f.closed {
        return Err(exc("ValueError", "I/O operation on closed file."));
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
                Some(v) => iterate(v)?,
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
                    let folded = crate::modules::unicodedata::fold_decimal_digits(s.as_str());
                    let t = folded.as_deref().unwrap_or(s.as_str()).trim().replace('_', "");
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
            let items = if args.len() == 1 { iterate(&args[0])? } else { args.clone() };
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
            for x in iterate(first)? {
                acc = binary(Operator::Add, &acc, &x, false)?;
            }
            Ok(acc)
        }
        "sorted" => {
            let [v] = one_arg(name, args)?;
            let mut items = iterate(&v)?;
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
            let mut items = iterate(&v)?;
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
            let out = iterate(&v)?
                .into_iter()
                .enumerate()
                .map(|(i, x)| Value::Tuple(vec![Value::Int(start + i as i64), x].into()))
                .collect();
            Ok(list_of(out))
        }
        "zip" => {
            let cols: Vec<Vec<Value>> = args.iter().map(iterate).collect::<PyResult<_>>()?;
            let n = cols.iter().map(Vec::len).min().unwrap_or(0);
            let out = (0..n).map(|i| Value::Tuple(cols.iter().map(|c| c[i].clone()).collect::<Vec<_>>().into())).collect();
            Ok(list_of(out))
        }
        "any" | "all" => {
            let [v] = one_arg(name, args)?;
            let items = iterate(&v)?;
            Ok(Value::Bool(if name == "any" { items.iter().any(Value::is_true) } else { items.iter().all(Value::is_true) }))
        }
        "ord" => crate::builtins::ord_of(&one_arg(name, args)?[0]),
        _ => crate::builtins::chr_of(&one_arg(name, args)?[0]),
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
                Some(r) => len_result(r?)?,
                None => return Err(type_error(format!("object of type '{}' has no len()", v.type_name()))),
            }
        }
        _ => return Err(type_error(format!("object of type '{}' has no len()", v.type_name()))),
    })
}

/// O valor que `__len__` devolveu como comprimento: inteiro não negativo, como o CPython exige.
fn len_result(returned: Value) -> PyResult<i64> {
    match returned {
        Value::Int(n) if n >= 0 => Ok(n),
        Value::Int(_) => Err(exc("ValueError", "__len__() should return >= 0")),
        other => Err(type_error(format!("'{}' object cannot be interpreted as an integer", other.type_name()))),
    }
}

/// Inteiro de um índice (`__index__`): `int` e `bool`.
fn as_index(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        Value::Bool(b) => Some(i64::from(*b)),
        // Instância de subclasse de `int`: vale o inteiro que ela carrega.
        Value::Instance(i) => match &*i.payload.borrow() {
            Some(inner @ (Value::Int(_) | Value::Bool(_))) => as_index(inner),
            _ => None,
        },
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
            .or_else(|| parse_int(&crate::modules::unicodedata::fold_decimal_digits(s.as_str())?))
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
            let (start, stop, step) = slice_bounds(text.len() as i64, s)?;
            if step == 1 {
                // `s[a:b]` custa o tamanho do trecho, não o da string (parsers fatiam em laço).
                Value::str(if stop > start { text.slice(start as usize, stop as usize) } else { "" })
            } else if text.is_ascii() {
                let bytes = text.as_str().as_bytes();
                let mut out = String::new();
                let mut i = start;
                while (step > 0 && i < stop) || (step < 0 && i > stop) {
                    out.push(char::from(bytes[i as usize]));
                    i += step;
                }
                Value::str(out)
            } else {
                let units: Vec<&str> = crate::object::units(text.as_str()).collect();
                Value::str(slice_indices(units.len(), s)?.into_iter().map(|i| units[i]).collect::<String>())
            }
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

pub(crate) fn subscript(container: &Value, index: &Value) -> PyResult<Value> {
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
                .and_then(|i| s.unit_at(i))
                .map(Value::str)
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
            Value::Str(s) => match current().map(|mut vm| vm.load_attr(container, s.as_str())) {
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
    if let Value::Ext(e) = container {
        if e.methods().contains(&"__setitem__") {
            if let Some(mut vm) = current() {
                return e.call_method(&mut vm, "__setitem__", vec![index.clone(), value], Vec::new()).map(|_| ());
            }
        }
    }
    if let (Value::List(l), Value::Slice(s)) = (container, index) {
        let new_items = iterate(&value)?;
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

/// Os valores do topo da pilha como chamada: o chamável, os posicionais e os nomeados de um `Call`,
/// um `CallMethod` (o espaço do `self` que o `LoadMethod` deixa vazio sai) ou um `CallEx`.
fn pop_call(code: &Code, stack: &mut Vec<Slot>, op: Op) -> PyResult<(Value, Vec<Value>, Vec<(String, Value)>)> {
    fn pop(stack: &mut Vec<Slot>) -> PyResult<Value> {
        match stack.pop() {
            Some(Slot::Val(v)) => Ok(v),
            _ => Err(internal("bad value stack")),
        }
    }
    let (count, method, kwnames) = match op {
        Op::Call { argc, kwnames } => (argc as usize, false, kwnames),
        Op::CallMethod { argc, kwnames } => (argc as usize + 1, true, kwnames),
        Op::CallEx { kwargs } => {
            let kw = if kwargs { Some(pop(stack)?) } else { None };
            let args = pop(stack)?;
            let func = pop(stack)?;
            let positional = iterate(&args)?;
            let mut named: Vec<(String, Value)> = Vec::new();
            if let Some(Value::Dict(d)) = kw {
                for (k, v) in d.borrow().iter() {
                    let Value::Str(s) = k else {
                        return Err(type_error("keywords must be strings"));
                    };
                    named.push((s.as_str().to_string(), v.clone()));
                }
            }
            return Ok((func, positional, named));
        }
        _ => return Err(internal("not a call instruction")),
    };
    if stack.len() <= count {
        return Err(internal("bad value stack"));
    }
    let at = stack.len() - count - 1;
    let mut drained = stack.drain(at..);
    let func = match drained.next() {
        Some(Slot::Val(v)) => v,
        _ => return Err(internal("bad value stack")),
    };
    let mut values = Vec::with_capacity(count);
    for s in drained {
        match s {
            Slot::Val(Value::Builtin(NO_SELF)) if method && values.is_empty() => {}
            Slot::Val(v) => values.push(v),
            _ => return Err(internal("bad value stack")),
        }
    }
    // Os nomes vêm de uma tupla constante: os pares saem direto dela, sem lista intermediária.
    let kwargs: Vec<(String, Value)> = match kwnames.map(|i| &code.consts[i as usize]) {
        Some(Value::Tuple(t)) => {
            let kw_values = values.split_off(values.len() - t.len());
            t.iter().map(to_str).zip(kw_values).collect()
        }
        _ => Vec::new(),
    };
    Ok((func, values, kwargs))
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
    binary_native(op, a, b, inplace)
}

/// O resto de `binary`, depois dos métodos mágicos de instâncias.
fn binary_native(op: Operator, a: &Value, b: &Value, inplace: bool) -> PyResult<Value> {
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
                    let items = iterate(b)?;
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
        // Qualquer objeto com buffer (`memoryview`): como o `bytes_concat` e o `bytearray_concat` do CPython,
        // o resultado tem o tipo da esquerda (`header + m[0:n]` do `multiprocessing.connection`). Cobre
        // também `bytes + bytes`, `bytes + bytearray` e `bytearray + bytes`.
        (Operator::Add, Value::Bytes(_) | Value::ByteArray(_), _) => match (a, b.bytes_like()) {
            (Value::Bytes(x), Some(extra)) => Ok(Value::bytes([&x[..], &extra[..]].concat())),
            (Value::ByteArray(x), Some(extra)) if inplace => {
                x.borrow_mut().extend_from_slice(&extra);
                Ok(a.clone())
            }
            (Value::ByteArray(x), Some(extra)) => {
                let mut out = x.borrow().clone();
                out.extend_from_slice(&extra);
                Ok(Value::bytearray(out))
            }
            _ => Err(type_error(format!("can't concatenate {} and {}", a.type_name(), b.type_name()))),
        },
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

/// As tentativas de `a OP b` com instância, na ordem: o `__iop__` (forma aumentada) e o `__op__` de
/// `a`, depois o `__rop__` de `b`.
fn binary_tries(op: Operator, a: &Value, b: &Value, inplace: bool) -> Vec<Attempt> {
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
    let Some((fwd, rev, inp)) = crate::classes::binop_dunder(sym) else { return Vec::new() };
    let attempt = |recv: &Value, name, other: &Value| Attempt { recv: recv.clone(), name, other: other.clone(), invert: false };
    let mut tries = Vec::new();
    if matches!(a, Value::Instance(_)) && inplace {
        tries.push(attempt(a, inp, b));
    }
    // `slot_nb_*` do CPython: o operando direito de subclasse estrita que redefine o refletido o
    // tenta antes do método direto do esquerdo.
    if right_overrides(a, b, rev) {
        tries.push(attempt(b, rev, a));
        tries.push(attempt(a, fwd, b));
        return tries;
    }
    if matches!(a, Value::Instance(_)) {
        tries.push(attempt(a, fwd, b));
    }
    if matches!(b, Value::Instance(_)) {
        tries.push(attempt(b, rev, a));
    }
    tries
}

/// O operando direito é de subclasse estrita do esquerdo e o refletido `name` dele não é o mesmo
/// que o esquerdo enxerga (`method_is_overloaded` do CPython).
fn right_overrides(a: &Value, b: &Value, name: &str) -> bool {
    let (Value::Instance(left), Value::Instance(right)) = (a, b) else { return false };
    if !crate::classes::right_is_subclass(a, b) {
        return false;
    }
    match (right.class().lookup(name), left.class().lookup(name)) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(r), Some(l)) => !crate::object::is(&l, &r),
    }
}

/// Os métodos de `<`, `<=`, `>`, `>=`: o direto do operando da esquerda e o refletido do da direita.
fn order_dunders(op: CmpOp) -> (&'static str, &'static str) {
    match op {
        CmpOp::Lt => ("__lt__", "__gt__"),
        CmpOp::LtE => ("__le__", "__ge__"),
        CmpOp::Gt => ("__gt__", "__lt__"),
        _ => ("__ge__", "__le__"),
    }
}

/// As tentativas de `a < b` e afins, na ordem: o `richcompare` do operando direito vai primeiro
/// quando ele é de subclasse estrita do esquerdo (`do_richcompare`).
fn order_attempts(op: CmpOp, a: &Value, b: &Value) -> Vec<Attempt> {
    let (forward, reflected) = order_dunders(op);
    let mut attempts = vec![
        Attempt { recv: a.clone(), name: forward, other: b.clone(), invert: false },
        Attempt { recv: b.clone(), name: reflected, other: a.clone(), invert: false },
    ];
    if crate::classes::right_is_subclass(a, b) {
        attempts.reverse();
    }
    attempts
}

/// As tentativas de `a == b` e `a != b`: o método de cada lado que é função Python. O `!=` usa o
/// `__ne__` da classe e, sem ele, o `__eq__` invertido (o `object.__ne__`). O direito de subclasse
/// estrita do esquerdo vai primeiro.
fn equal_attempts(op: CmpOp, a: &Value, b: &Value) -> Vec<Attempt> {
    let not_equal = op == CmpOp::NotEq;
    let sides = if crate::classes::right_is_subclass(a, b) { [(b, a), (a, b)] } else { [(a, b), (b, a)] };
    sides
        .into_iter()
        .filter_map(|(recv, other)| {
            let (name, invert) = if not_equal && dunder_function(recv, "__ne__").is_some() {
                ("__ne__", false)
            } else if dunder_function(recv, "__eq__").is_some() {
                ("__eq__", not_equal)
            } else {
                return None;
            };
            Some(Attempt { recv: recv.clone(), name, other: other.clone(), invert })
        })
        .collect()
}

/// Operador binário com instância de classe de usuário: `__add__`, depois `__radd__` do outro lado.
fn instance_binary(op: Operator, a: &Value, b: &Value, inplace: bool) -> Option<PyResult<Value>> {
    let mut vm = current()?;
    for at in binary_tries(op, a, b, inplace) {
        if let Some(r) = vm.call_dunder(&at.recv, at.name, vec![at.other]) {
            match r {
                Ok(v) if crate::classes::is_not_implemented(&v) => {}
                other => return Some(other),
            }
        }
    }
    None
}

/// O `__traceback__` da exceção em curso, o terceiro argumento do `__exit__` de um `with`.
fn exception_traceback(raised: &Value) -> Value {
    match raised {
        Value::Exception(x) => x.traceback.borrow().clone(),
        Value::Instance(i) => i.dict.borrow().get("__traceback__").cloned(),
        _ => None,
    }
    .unwrap_or(Value::None)
}

/// O método mágico de um operador unário (`not` não tem: é a verdade do operando).
fn unary_dunder(op: UnaryOp) -> Option<&'static str> {
    match op {
        UnaryOp::USub => Some("__neg__"),
        UnaryOp::UAdd => Some("__pos__"),
        UnaryOp::Invert => Some("__invert__"),
        UnaryOp::Not => None,
    }
}

pub(crate) fn unary(op: UnaryOp, a: &Value) -> PyResult<Value> {
    if let Value::Instance(_) = a {
        if let (Some(name), Some(mut vm)) = (unary_dunder(op), current())
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
        for at in order_attempts(op, a, b) {
            if let Some(r) = vm.call_dunder(&at.recv, at.name, vec![at.other]) {
                let v = r?;
                if !crate::classes::is_not_implemented(&v) {
                    return Ok(v.is_true());
                }
            }
        }
    }
    compare_native(op, a, b)
}

/// O resto de `compare`, depois dos métodos mágicos de ordem de instâncias.
fn compare_native(op: CmpOp, a: &Value, b: &Value) -> PyResult<bool> {
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
        // Os operadores unários e as funções numéricas de uma subclasse de `int` ou `float` (`IntEnum`,
        // `IntFlag`): o `-signal.SIGKILL` e o `abs()` agem sobre o valor embutido.
        "__neg__" => return Some(unary(UnaryOp::USub, payload)),
        "__pos__" => return Some(unary(UnaryOp::UAdd, payload)),
        "__invert__" => return Some(unary(UnaryOp::Invert, payload)),
        "__abs__" => {
            let mut vm = current()?;
            return Some(vm.call(&crate::builtins::get("abs")?, vec![payload.clone()], Vec::new()));
        }
        "__divmod__" | "__rdivmod__" => {
            let mut vm = current()?;
            let other = arg(0)?;
            let pair = if name == "__divmod__" { vec![payload.clone(), other] } else { vec![other, payload.clone()] };
            return Some(vm.call(&crate::builtins::get("divmod")?, pair, Vec::new()));
        }
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
        "__lshift__" | "__rlshift__" => Operator::LShift,
        "__rshift__" | "__rrshift__" => Operator::RShift,
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
        Value::Instance(i) if i.class().lookup("keys").is_some() => {
            let mut vm = current().ok_or_else(|| internal("no vm"))?;
            let keys_fn = vm.load_attr(v, "keys")?;
            let keys = vm.call(&keys_fn, Vec::new(), Vec::new())?;
            let mut out = Vec::new();
            for k in iterate(&keys)? {
                let value = subscript(v, &k)?;
                out.push((k, value));
            }
            Ok(Some(out))
        }
        _ => Ok(None),
    }
}

/// `item in container`.
pub(crate) fn contains(container: &Value, item: &Value) -> PyResult<bool> {
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
        return contains_by_iteration(container, item);
    }
    let member = |items: &[Value]| items.iter().any(|x| is(x, item) || py_eq(x, item));
    match container {
        Value::List(l) => Ok(member(&l.borrow()[..])),
        Value::Tuple(t) => Ok(member(&t[..])),
        Value::Str(s) => match item {
            Value::Str(sub) => Ok(!crate::object::match_offsets(s.as_str(), sub.as_str(), 1).is_empty()),
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
            _ => Ok(member(&iterate(container)?)),
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
        _ => contains_by_iteration(container, item),
    }
}

/// O nome do tipo embutido que `v` é (`int`, `type`, `function`), para as tabelas geradas no oráculo.
fn builtin_type_name(v: &Value) -> Option<&'static str> {
    match v {
        Value::Builtin(n) => Some(*n),
        _ => crate::builtins::class_name(v),
    }
}

/// O `PySequence_Contains` sem `__contains__`: consome o iterador só até achar o item, como o
/// CPython faz com um gerador.
fn contains_by_iteration(container: &Value, item: &Value) -> PyResult<bool> {
    let mut it = get_iter(container).map_err(|e| {
        if e.kind == "TypeError" {
            type_error(format!("argument of type '{}' is not iterable", container.type_name()))
        } else {
            e
        }
    })?;
    while let Some(x) = it.next()? {
        if is(&x, item) || py_eq(&x, item) {
            return Ok(true);
        }
    }
    Ok(false)
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
        (Value::Str(x), Value::Str(y)) => Ok(apply(op, str_cmp(x.as_str(), y.as_str()))),
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
                    let send = self.load_attr(&v, "send")?;
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

    /// `throw`/`close` num gerador de fora parado numa delegação (`gen_throw` do CPython): a exceção segue ao
    /// sub-iterador que está no topo da pilha de `outer`. Um sub-gerador ou sub-corrente ganha quadro
    /// (`Injected::Sub`, com a exceção em `Resuming::inject`; o desfecho volta pelo `ResumeUse::DelegateThrow`
    /// ou `DelegateExit`); um sub-iterador comum (`yield from` sobre lista, instância com `throw`) responde na
    /// hora, e um `GeneratorExit` o fecha antes. Sem `throw` no sub-iterador a exceção é levantada no de fora.
    fn inject_delegated(&mut self, outer: &mut Callee, e: PyException) -> Injected {
        if outer.link.resuming.is_none() {
            return Injected::Raise(e);
        }
        let Frame { code, stack, pc, .. } = &mut outer.frame;
        let Some(Op::DelegateNext(l)) = code.ops.get(*pc).copied() else { return Injected::Raise(e) };
        let Some(Op::Delegate(end)) = code.ops.get(l as usize).copied() else { return Injected::Raise(e) };
        let end = end as usize;
        let Some(sub) = stack.last().and_then(slot_core) else {
            return match self.delegate_throw(stack, e) {
                Ok(Step::Yield(v)) => {
                    stack.push(Slot::Val(v));
                    *pc = l as usize + 1;
                    Injected::Applied
                }
                Ok(Step::Done(v)) => {
                    stack.pop();
                    stack.push(Slot::Val(v));
                    *pc = end;
                    Injected::Applied
                }
                Err(e) => Injected::Raise(e),
            };
        };
        let (inject, use_) = if e.kind == "GeneratorExit" {
            if sub.close_if_plain() {
                return Injected::Raise(e);
            }
            (exc("GeneratorExit", ""), ResumeUse::DelegateExit(e))
        } else {
            (e, ResumeUse::DelegateThrow { end })
        };
        match self.enter_resume(&sub, None, Some(inject), use_) {
            Ok(Resumption::Frame(callee)) => Injected::Sub(callee),
            Ok(Resumption::Ready(resumed, use_)) => {
                let settled = if use_.closes() { sub.settle_close(Ok(resumed)) } else { Ok(resumed) };
                match settled.and_then(|r| self.apply_resumed(code, stack, &sub, r, use_)) {
                    Ok(Some(target)) => {
                        *pc = target;
                        Injected::Applied
                    }
                    Ok(None) => Injected::Applied,
                    Err(x) => Injected::Raise(x),
                }
            }
            Err(x) => Injected::Raise(x),
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
            if let Ok(close) = self.load_attr(&target, "close") {
                self.call(&close, Vec::new(), Vec::new())?;
            }
            return Err(e);
        }
        let Ok(throw) = self.load_attr(&target, "throw") else { return Err(e) };
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
                let class = i.class();
                let attr = class.lookup(name)?;
                self.bind_class_attr(&attr, obj.clone(), &class).ok()
            }
            Value::Ext(e) if e.methods().contains(&name) => self.load_attr(obj, name).ok(),
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
