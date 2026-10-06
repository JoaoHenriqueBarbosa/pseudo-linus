//! Modelo de objetos (fatia 9 de `docs/python3-port.md`): `Value`, `repr()`/`str()`, igualdade e
//! hash dos tipos embutidos, com a saída do CPython 3.13.
//!
//! Escalares (`None`, `bool`, `int`, `float`) ficam inline; o resto é compartilhado por `Rc`, e os
//! mutáveis (`list`, `dict`, `set`) ficam atrás de `RefCell`. Clonar um `Value` é copiar a
//! referência, como atribuir em Python.
//!
//! Identidade: só os tipos atrás de `Rc` têm identidade de objeto (`Rc::ptr_eq`). `int` segue o
//! cache de inteiros pequenos do CPython (-5 a 256 são sempre o mesmo objeto) e `float` nunca é
//! idêntico a outro; a diferença só aparece com NaN dentro de contêineres (`[x] == [x]` com `x` NaN
//! é verdadeiro no CPython e falso aqui).

mod dict;
mod float;
mod int;
mod list;
mod set;
// O nome `str` sombrearia o tipo primitivo neste módulo.
#[path = "str.rs"]
mod pystr;

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

pub use self::dict::Dict;
pub use self::float::{float_hash, float_repr, format_float_short};
pub use self::int::{HASH_MODULUS, int_add, int_hash, int_mul, int_neg, int_repr, int_sub};
pub use self::set::Set;
pub use self::pystr::{bytes_hash, bytes_repr, is_printable, str_repr, PyStr};

/// Valor Python.
#[derive(Clone)]
pub enum Value {
    None,
    Bool(bool),
    /// Faixa de `i64`; o que não cabe vira [`Value::Big`] (sempre normalizado, ver `bigint`).
    Int(i64),
    /// `int` fora da faixa de `i64`. Nunca guarda um valor que caiba em `Int`.
    Big(Rc<num_bigint::BigInt>),
    Float(f64),
    Str(Rc<PyStr>),
    Bytes(Rc<[u8]>),
    /// `bytearray`: bytes mutáveis, com identidade.
    ByteArray(Rc<RefCell<Vec<u8>>>),
    List(Rc<RefCell<Vec<Value>>>),
    Tuple(Rc<[Value]>),
    Dict(Rc<RefCell<Dict>>),
    Set(Rc<RefCell<Set>>),
    /// `range(start, stop, step)`, imutável e sem identidade observável nesta fase.
    Range(Range),
    /// Função embutida, pelo nome (`print`, `len`...); o interpretador resolve a chamada.
    Builtin(&'static str),
    /// Instância de exceção (`ValueError('x')`): classe pelo nome e os `args`.
    Exception(Rc<ExcObj>),
    /// Função definida por `def`.
    Function(Rc<FuncObj>),
    /// Módulo importado, com os atributos dele (ver `modules::import`).
    Module(Rc<ModuleObj>),
    /// Função nativa registrada numa tabela (módulo, método de tipo embutido ou builtin).
    NativeFn(Rc<NativeFn>),
    /// Objeto definido por um módulo nativo (`re.Pattern`, `zipfile.ZipFile`...): ver [`ExtObject`].
    Ext(Rc<dyn ExtObject>),
    /// Objeto nativo com estado (arquivo, leitor ou escritor de `csv`).
    Native(Rc<RefCell<Native>>),
    /// Método embutido preso ao receptor (`arquivo.write`).
    Bound(Rc<BoundMethod>),
    /// Classe de usuário.
    Class(Rc<ClassObj>),
    /// Instância de classe de usuário.
    Instance(Rc<InstanceObj>),
    /// Função de usuário presa ao receptor (`obj.metodo`): o `self` entra como primeiro argumento.
    BoundFn(Rc<(Value, Rc<FuncObj>)>),
    /// `slice(lo, hi, step)`, como em `x[1:3]`.
    Slice(Rc<(Value, Value, Value)>),
}

/// Argumentos nomeados de uma chamada.
pub type Kw = Vec<(String, Value)>;

/// Assinatura de toda função nativa: recebe a VM, os posicionais (o receptor vem primeiro nos
/// métodos de tipo) e os nomeados.
pub type NativeFnPtr = fn(&mut crate::vm::Vm, Vec<Value>, Kw) -> Result<Value, crate::vm::PyException>;

/// Função nativa com nome (o que `repr` e as mensagens de erro mostram).
pub struct NativeFn {
    pub name: &'static str,
    pub f: NativeFnPtr,
}

impl fmt::Debug for NativeFn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<built-in function {}>", self.name)
    }
}

/// Descritor que o acesso a atributo de classe e de instância desembrulha.
#[derive(Clone)]
pub enum Descriptor {
    /// `@staticmethod`: devolve a função sem prender receptor.
    Static(Value),
    /// `@classmethod`: prende a classe como primeiro argumento.
    Class(Value),
    /// `@property`: leitura chama `get`; atribuição chama `set`.
    Property { get: Value, set: Option<Value>, del: Option<Value> },
}

/// Objeto definido por um módulo nativo. O estado mutável fica dentro do implementador (`Cell`,
/// `RefCell`), porque o valor é compartilhado por `Rc` e os métodos recebem `&self`.
///
/// Só `type_name` e `call_method` são obrigatórios; o resto tem padrão "não suportado".
pub trait ExtObject {
    /// Descritor de classe (`staticmethod`, `classmethod`, `property`); `None` nos demais objetos.
    fn descriptor(&self) -> Option<Descriptor> {
        None
    }
    /// `type(obj).__name__` (ex.: `Pattern`, `Match`).
    fn type_name(&self) -> &'static str;
    /// `repr(obj)`.
    fn repr(&self) -> String {
        format!("<{} object>", self.type_name())
    }
    /// Nomes dos métodos: `obj.nome` devolve um método preso, chamado depois em [`call_method`].
    fn methods(&self) -> &'static [&'static str] {
        &[]
    }
    /// Atributo de dado (`match.string`, `zipinfo.filename`); `None` = não existe.
    fn getattr(&self, _vm: &mut crate::vm::Vm, _name: &str) -> Option<Result<Value, crate::vm::PyException>> {
        None
    }
    /// Atribui o atributo `name`; `None` = o objeto não aceita atributos novos.
    fn setattr(&self, _name: &str, _value: Value) -> Option<Result<(), crate::vm::PyException>> {
        None
    }
    /// Chama o método `name` (um dos de [`methods`]).
    fn call_method(
        &self,
        vm: &mut crate::vm::Vm,
        name: &str,
        args: Vec<Value>,
        kw: Kw,
    ) -> Result<Value, crate::vm::PyException>;
    /// Torna o objeto iterável: `Ok(None)` encerra o laço.
    fn is_iterable(&self) -> bool {
        false
    }
    fn iter_next(&self) -> Result<Option<Value>, crate::vm::PyException> {
        Ok(None)
    }
    /// `len(obj)`; `None` = não tem.
    fn len(&self) -> Option<usize> {
        None
    }
    /// `obj[key]`; `None` = não suporta.
    fn getitem(&self, _key: &Value) -> Option<Result<Value, crate::vm::PyException>> {
        None
    }
    /// `bool(obj)`.
    fn is_true(&self) -> bool {
        true
    }
    /// Operador binário com este objeto de um dos lados: `op` é o símbolo (`"+"`, `"-"`, `"/"`...) e
    /// `reflected` indica que o objeto é o operando da direita. `None` = não suporta.
    fn binop(&self, _op: &str, _other: &Value, _reflected: bool) -> Option<Result<Value, crate::vm::PyException>> {
        None
    }
    /// Comparação `self <op> other` (`"=="`, `"!="`, `"<"`, `"<="`, `">"`, `">="`). `None` = não suporta.
    fn richcmp(&self, _op: &str, _other: &Value) -> Option<Result<bool, crate::vm::PyException>> {
        None
    }
    /// Os itens de um objeto iterável que pode ser percorrido várias vezes (views de dicionário):
    /// cada `for` pega um instantâneo novo, ao contrário de `is_iterable`, que é um iterador único.
    fn to_items(&self) -> Option<Vec<Value>> {
        None
    }
    /// `item in obj` quando o objeto sabe responder sem percorrer.
    fn contains_item(&self, _item: &Value) -> Option<Result<bool, crate::vm::PyException>> {
        None
    }
    /// `hash(obj)` quando o objeto define o próprio (referências fracas); `None` = por identidade.
    fn hash_value(&self) -> Option<i64> {
        None
    }
    /// `obj == other` quando o objeto define a própria igualdade; `None` = por identidade.
    fn eq_value(&self, _other: &Value) -> Option<bool> {
        None
    }
    /// O objeto para o qual uma referência fraca aponta, se for uma e ainda estiver viva.
    fn referent(&self) -> Option<Value> {
        None
    }
}

/// Módulo: nome e atributos (preenchidos pelo construtor do módulo em `modules`).
pub struct ModuleObj {
    pub name: &'static str,
    pub attrs: RefCell<std::collections::BTreeMap<String, Value>>,
}

impl fmt::Debug for ModuleObj {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<module '{}'>", self.name)
    }
}

/// Método embutido com o receptor já escolhido.
#[derive(Debug)]
pub struct BoundMethod {
    pub recv: Value,
    pub name: &'static str,
}

/// De onde vem um arquivo de texto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Stdin,
    Stdout,
    Stderr,
    /// Aberto para leitura: o conteúdo já foi lido e dividido em linhas.
    Read,
}

/// Arquivo de texto do Python (`sys.stdin`, `sys.stdout`, retorno de `open`).
#[derive(Debug)]
pub struct PyFile {
    pub kind: FileKind,
    /// Linhas ainda não consumidas, com o terminador (`\n`, ou o original com `newline=''`).
    pub lines: Vec<String>,
    pub pos: usize,
    /// O conteúdo de leitura já foi carregado (o stdin só carrega no primeiro uso).
    pub loaded: bool,
    pub closed: bool,
    pub name: String,
}

/// Objetos nativos que guardam estado mutável.
#[derive(Debug)]
pub enum Native {
    File(PyFile),
    /// `csv.reader(arquivo)`: o leitor e a fonte de linhas.
    CsvReader { reader: crate::modules::csv::Reader, src: Value },
    /// `csv.writer(arquivo)`: o dialeto e o destino.
    CsvWriter { dialect: crate::modules::csv::Dialect, target: Value },
}

/// Escopo de execução: as variáveis de uma função (ou do corpo de uma classe) e o escopo de função
/// que a envolve. As funções internas guardam o `Env` onde nasceram, e é assim que enxergam e
/// alteram as variáveis do escopo externo (closures, `nonlocal`).
pub struct Env {
    pub vars: RefCell<std::collections::HashMap<String, Value>>,
    /// Ordem de criação dos nomes (só preenchida nos corpos de classe).
    pub order: RefCell<Vec<String>>,
    pub parent: Option<Rc<Env>>,
    /// Corpo de classe: as funções definidas nele não enxergam estas variáveis.
    pub is_class: bool,
    /// Escopo do módulo (as variáveis ficam nas globais da VM, não aqui).
    pub is_module: bool,
}

impl fmt::Debug for Env {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<env>")
    }
}

impl Env {
    pub fn new(parent: Option<Rc<Env>>, is_class: bool, is_module: bool) -> Rc<Env> {
        Rc::new(Env {
            vars: RefCell::new(std::collections::HashMap::new()),
            order: RefCell::new(Vec::new()),
            parent,
            is_class,
            is_module,
        })
    }

    /// Grava uma variável local; nos corpos de classe lembra a ordem em que os nomes nasceram.
    pub fn set(&self, name: &str, v: Value) {
        let fresh = self.vars.borrow_mut().insert(name.to_string(), v).is_none();
        if fresh && self.is_class {
            self.order.borrow_mut().push(name.to_string());
        }
    }

    /// O escopo que uma função definida aqui captura.
    pub fn capture(self: &Rc<Env>) -> Option<Rc<Env>> {
        if self.is_module {
            None
        } else if self.is_class {
            self.parent.clone()
        } else {
            Some(self.clone())
        }
    }
}

/// Função de usuário: o código compilado, os valores padrão dos últimos parâmetros posicionais, os
/// padrões dos parâmetros só-nomeados e o escopo externo capturado.
#[derive(Debug)]
pub struct FuncObj {
    pub code: Rc<crate::compile::Code>,
    pub defaults: Vec<Value>,
    pub kwdefaults: Vec<(String, Value)>,
    pub closure: Option<Rc<Env>>,
    /// Globais do módulo onde a função nasceu (cada módulo em Python embutido tem as suas).
    pub globals: Rc<RefCell<std::collections::HashMap<String, Value>>>,
    /// Atributos atribuídos à função (`f.cache_clear = ...`, `__name__`, `__wrapped__`...).
    pub attrs: RefCell<std::collections::BTreeMap<String, Value>>,
}

/// Classe definida por `class`: nome, bases (já resolvidas) e o espaço de nomes.
pub struct ClassObj {
    pub name: String,
    pub bases: Vec<Rc<ClassObj>>,
    /// A classe embutida mais próxima na herança (uma exceção como `Exception`), se houver.
    pub builtin_base: Option<&'static str>,
    /// Tipo de dados embutido de que a classe herda (`dict`, `list`, `tuple`, `str`...): as
    /// instâncias carregam um valor desse tipo em `InstanceObj::payload`.
    pub data_base: Option<&'static str>,
    /// Metaclasse (`class A(metaclass=M)` ou herdada das bases); `None` é o `type` padrão.
    pub meta: Option<Rc<ClassObj>>,
    /// A classe herda de `type`: ela é uma metaclasse.
    pub is_meta: bool,
    pub dict: RefCell<indexmap::IndexMap<String, Value>>,
}

impl fmt::Debug for ClassObj {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<class '{}'>", self.name)
    }
}

impl ClassObj {
    /// O módulo onde a classe foi definida (`__module__`), `__main__` por padrão.
    pub fn module(&self) -> String {
        match self.dict.borrow().get("__module__") {
            Some(Value::Str(s)) => s.as_str().to_string(),
            _ => "__main__".to_string(),
        }
    }

    /// Ordem de resolução de métodos (`__mro__`): linearização C3. Herança simples não paga o
    /// merge; se as bases forem inconsistentes, cai na busca em profundidade sem repetir.
    pub fn mro(self: &Rc<Self>) -> Vec<Rc<ClassObj>> {
        let mut out: Vec<Rc<ClassObj>> = vec![self.clone()];
        if self.bases.len() <= 1 {
            for b in &self.bases {
                out.extend(b.mro());
            }
            return out;
        }
        let mut seqs: Vec<Vec<Rc<ClassObj>>> = self.bases.iter().map(|b| b.mro()).collect();
        seqs.push(self.bases.clone());
        loop {
            seqs.retain(|s| !s.is_empty());
            if seqs.is_empty() {
                return out;
            }
            let pick = seqs.iter().map(|s| s[0].clone()).find(|cand| {
                !seqs.iter().any(|s| s[1..].iter().any(|x| Rc::ptr_eq(x, cand)))
            });
            let Some(next) = pick else { break };
            for s in seqs.iter_mut() {
                if Rc::ptr_eq(&s[0], &next) {
                    s.remove(0);
                }
            }
            out.push(next);
        }
        for b in &self.bases {
            for c in b.mro() {
                if !out.iter().any(|x| Rc::ptr_eq(x, &c)) {
                    out.push(c);
                }
            }
        }
        out
    }

    /// Procura `name` na classe e nas bases, na ordem do MRO.
    pub fn lookup(self: &Rc<Self>, name: &str) -> Option<Value> {
        self.mro().iter().find_map(|c| c.dict.borrow().get(name).cloned())
    }
}

/// Instância de uma classe de usuário.
pub struct InstanceObj {
    pub class: Rc<ClassObj>,
    pub dict: RefCell<indexmap::IndexMap<String, Value>>,
    /// Espelho vivo de `__dict__`: o `dict` entregue ao usuário, sincronizado com `dict` em cada acesso.
    pub view: RefCell<Option<Rc<RefCell<Dict>>>>,
    /// O valor embutido de uma instância cuja classe herda de `dict`, `list`, `tuple`, `str`, `int`...
    pub payload: RefCell<Option<Value>>,
}

impl InstanceObj {
    /// O `__dict__` vivo: criado na primeira leitura e compartilhado nas seguintes.
    pub fn live_dict(&self) -> Value {
        if let Some(v) = self.view.borrow().as_ref() {
            self.sync_from_view();
            return Value::Dict(v.clone());
        }
        let mut d = Dict::new();
        for (k, v) in self.dict.borrow().iter() {
            let _ = d.set(Value::str(k.clone()), v.clone());
        }
        let rc = Rc::new(RefCell::new(d));
        *self.view.borrow_mut() = Some(rc.clone());
        Value::Dict(rc)
    }

    /// Traz para `dict` o que o usuário escreveu no `__dict__` (chaves não textuais ficam de fora).
    pub fn sync_from_view(&self) {
        let Some(v) = self.view.borrow().clone() else { return };
        let mut fresh = indexmap::IndexMap::new();
        for (k, val) in v.borrow().iter() {
            if let Value::Str(s) = k {
                fresh.insert(s.as_str().to_string(), val.clone());
            }
        }
        *self.dict.borrow_mut() = fresh;
    }

    /// Reflete no `__dict__` vivo uma mudança feita direto em `dict`.
    pub fn sync_to_view(&self) {
        let Some(v) = self.view.borrow().clone() else { return };
        let mut d = Dict::new();
        for (k, val) in self.dict.borrow().iter() {
            let _ = d.set(Value::str(k.clone()), val.clone());
        }
        *v.borrow_mut() = d;
    }
}

impl fmt::Debug for InstanceObj {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{} object>", self.class.name)
    }
}

/// Instância de uma exceção embutida.
#[derive(Debug)]
pub struct ExcObj {
    pub kind: &'static str,
    pub args: Vec<Value>,
    /// `__traceback__`: preenchido quando a exceção é capturada por um `except`.
    pub traceback: RefCell<Option<Value>>,
}

impl ExcObj {
    pub fn new(kind: &'static str, args: Vec<Value>) -> ExcObj {
        ExcObj { kind, args, traceback: RefCell::new(None) }
    }
}

/// Classes de exceção embutidas e a classe pai de cada uma (`BaseException` não tem).
pub const EXC_CLASSES: &[(&str, &str)] = &[
    ("BaseException", ""),
    ("Exception", "BaseException"),
    ("ArithmeticError", "Exception"),
    ("ZeroDivisionError", "ArithmeticError"),
    ("OverflowError", "ArithmeticError"),
    ("LookupError", "Exception"),
    ("IndexError", "LookupError"),
    ("KeyError", "LookupError"),
    ("TypeError", "Exception"),
    ("ValueError", "Exception"),
    ("NameError", "Exception"),
    ("AssertionError", "Exception"),
    ("RuntimeError", "Exception"),
    ("NotImplementedError", "RuntimeError"),
    ("SystemError", "Exception"),
    ("AttributeError", "Exception"),
    ("UnboundLocalError", "NameError"),
    ("RecursionError", "RuntimeError"),
    ("ImportError", "Exception"),
    ("ModuleNotFoundError", "ImportError"),
    ("OSError", "Exception"),
    ("FileNotFoundError", "OSError"),
    ("PermissionError", "OSError"),
    ("FileExistsError", "OSError"),
    ("IsADirectoryError", "OSError"),
    ("NotADirectoryError", "OSError"),
    ("TimeoutError", "OSError"),
    ("ConnectionError", "OSError"),
    ("EOFError", "Exception"),
    ("MemoryError", "Exception"),
    ("BufferError", "Exception"),
    ("FloatingPointError", "ArithmeticError"),
    ("KeyboardInterrupt", "BaseException"),
    ("SystemExit", "BaseException"),
    ("UnicodeError", "ValueError"),
    ("UnicodeEncodeError", "UnicodeError"),
    ("SyntaxError", "Exception"),
    ("IndentationError", "SyntaxError"),
    ("TabError", "IndentationError"),
    ("ProcessLookupError", "OSError"),
    ("ChildProcessError", "OSError"),
    ("BlockingIOError", "OSError"),
    ("InterruptedError", "OSError"),
    ("BrokenPipeError", "ConnectionError"),
    ("ConnectionAbortedError", "ConnectionError"),
    ("ConnectionRefusedError", "ConnectionError"),
    ("ConnectionResetError", "ConnectionError"),
    ("StopAsyncIteration", "Exception"),
    ("GeneratorExit", "BaseException"),
    ("Warning", "Exception"),
    ("UserWarning", "Warning"),
    ("DeprecationWarning", "Warning"),
    ("PendingDeprecationWarning", "Warning"),
    ("SyntaxWarning", "Warning"),
    ("RuntimeWarning", "Warning"),
    ("FutureWarning", "Warning"),
    ("ImportWarning", "Warning"),
    ("UnicodeWarning", "Warning"),
    ("BytesWarning", "Warning"),
    ("ResourceWarning", "Warning"),
    ("EncodingWarning", "Warning"),
    ("re.error", "Exception"),
    ("struct.error", "Exception"),
    ("binascii.Error", "ValueError"),
    ("zlib.error", "Exception"),
    ("zipfile.BadZipFile", "Exception"),
    ("subprocess.CalledProcessError", "Exception"),
    ("subprocess.TimeoutExpired", "Exception"),
    ("urllib.error.URLError", "OSError"),
    ("http.client.HTTPException", "Exception"),
    ("StopIteration", "Exception"),
    ("UnicodeDecodeError", "UnicodeError"),
    ("_csv.Error", "Exception"),
    ("json.decoder.JSONDecodeError", "ValueError"),
];

/// `issubclass(kind, base)` entre exceções embutidas.
pub fn exc_is_subclass(kind: &str, base: &str) -> bool {
    let mut cur = kind;
    loop {
        if cur == base {
            return true;
        }
        match EXC_CLASSES.iter().find(|(n, _)| *n == cur) {
            Some((_, parent)) if !parent.is_empty() => cur = parent,
            _ => return false,
        }
    }
}

/// `str(exc)`: vazio sem args, o próprio arg com um, a tupla com vários (`KeyError` usa o repr).
pub fn exc_str(e: &ExcObj) -> String {
    match e.args.as_slice() {
        [] => String::new(),
        [one] if e.kind == "KeyError" => repr(one),
        [one] => to_str(one),
        [Value::Str(msg), Value::Tuple(d)] if exc_is_subclass(&e.kind, "SyntaxError") => {
            let file = match d.first() {
                Some(Value::Str(f)) => Some(f.as_str().rsplit('/').next().unwrap_or("").to_string()),
                _ => None,
            };
            let line = match d.get(1) {
                Some(Value::Int(n)) => Some(*n),
                _ => None,
            };
            match (file, line) {
                (Some(f), Some(n)) => format!("{} ({f}, line {n})", msg.as_str()),
                (Some(f), None) => format!("{} ({f})", msg.as_str()),
                (None, Some(n)) => format!("{} (line {n})", msg.as_str()),
                (None, None) => msg.as_str().to_string(),
            }
        }
        [Value::Int(errno), msg, rest @ ..] if rest.len() <= 1 && exc_is_subclass(&e.kind, "OSError") => match rest {
            [file] => format!("[Errno {errno}] {}: {}", to_str(msg), repr(file)),
            _ => format!("[Errno {errno}] {}", to_str(msg)),
        },
        many => repr(&Value::tuple(many.to_vec())),
    }
}

/// `repr(exc)`: `ValueError('x')`.
pub fn exc_repr(e: &ExcObj) -> String {
    match e.args.as_slice() {
        [one] => format!("{}({})", e.kind, repr(one)),
        [] => format!("{}()", e.kind),
        many => {
            let t = repr(&Value::tuple(many.to_vec()));
            format!("{}{}", e.kind, t)
        }
    }
}

/// Valor de um `range` (`Objects/rangeobject.c`), com `step != 0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: i64,
    pub stop: i64,
    pub step: i64,
}

impl Range {
    /// `len(range)` (`compute_range_length`).
    pub fn len(&self) -> i64 {
        let (lo, hi, step) = if self.step > 0 {
            (i128::from(self.start), i128::from(self.stop), i128::from(self.step))
        } else {
            (i128::from(self.stop), i128::from(self.start), -i128::from(self.step))
        };
        if lo >= hi {
            0
        } else {
            ((hi - lo - 1) / step + 1) as i64
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Elemento `i` (já normalizado, `0 <= i < len`).
    pub fn item(&self, i: i64) -> i64 {
        self.start + i * self.step
    }

    /// `x in range` para inteiros.
    pub fn contains_int(&self, x: i64) -> bool {
        let in_bounds = if self.step > 0 { self.start <= x && x < self.stop } else { self.stop < x && x <= self.start };
        in_bounds && (i128::from(x) - i128::from(self.start)) % i128::from(self.step) == 0
    }
}

/// Erros do modelo de objetos. O interpretador (fatia 11) converte em exceções Python.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjError {
    /// `TypeError` com a mensagem do CPython.
    TypeError(String),
    /// Erro interno: resultado inteiro fora de `i64`. Some na fatia 19 (ver `int`).
    IntOverflow,
}

impl Value {
    pub fn str(text: impl Into<String>) -> Value {
        Value::Str(Rc::new(PyStr::new(text)))
    }

    pub fn bytes(data: impl Into<Vec<u8>>) -> Value {
        Value::Bytes(Rc::from(data.into()))
    }

    pub fn bytearray(data: impl Into<Vec<u8>>) -> Value {
        Value::ByteArray(Rc::new(RefCell::new(data.into())))
    }

    /// Os bytes de um valor "bytes-like" (`bytes` ou `bytearray`), copiados.
    pub fn bytes_like(&self) -> Option<Rc<[u8]>> {
        match self {
            Value::Bytes(b) => Some(b.clone()),
            Value::ByteArray(b) => Some(Rc::from(b.borrow().as_slice())),
            // `memoryview` (classe em Python): os bytes da visão.
            Value::Instance(i) if i.class.name == "memoryview" => crate::vm::memoryview_bytes(self),
            _ => None,
        }
    }

    pub fn list(items: Vec<Value>) -> Value {
        Value::List(Rc::new(RefCell::new(items)))
    }

    pub fn tuple(items: Vec<Value>) -> Value {
        Value::Tuple(Rc::from(items))
    }

    pub fn dict(d: Dict) -> Value {
        Value::Dict(Rc::new(RefCell::new(d)))
    }

    pub fn set(s: Set) -> Value {
        Value::Set(Rc::new(RefCell::new(s)))
    }

    /// `type(v).__name__`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::None => "NoneType",
            Value::Bool(_) => "bool",
            Value::Int(_) | Value::Big(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "str",
            Value::Bytes(_) => "bytes",
            Value::ByteArray(_) => "bytearray",
            Value::List(_) => "list",
            Value::Tuple(_) => "tuple",
            Value::Dict(_) => "dict",
            Value::Set(_) => "set",
            Value::Range(_) => "range",
            Value::Builtin(name) if is_builtin_type(name) => "type",
            Value::NativeFn(n) if is_builtin_type(n.name) => "type",
            Value::Builtin(_) | Value::NativeFn(_) => "builtin_function_or_method",
            Value::Ext(e) => e.type_name(),
            Value::Exception(e) => e.kind,
            Value::Function(_) => "function",
            Value::Module(_) => "module",
            Value::Native(n) => match &*n.borrow() {
                Native::File(_) => "TextIOWrapper",
                Native::CsvReader { .. } => "_csv.reader",
                Native::CsvWriter { .. } => "_csv.writer",
            },
            Value::Bound(_) => "builtin_function_or_method",
            Value::Class(_) => "type",
            Value::Instance(i) => intern(&i.class.name),
            Value::BoundFn(_) => "method",
            Value::Slice(_) => "slice",
        }
    }

    /// `bool(v)`.
    pub fn is_true(&self) -> bool {
        match self {
            Value::None => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Big(_) => true,
            Value::Float(x) => *x != 0.0,
            Value::Str(s) => !s.is_empty(),
            Value::Bytes(b) => !b.is_empty(),
            Value::ByteArray(b) => !b.borrow().is_empty(),
            Value::List(l) => !l.borrow().is_empty(),
            Value::Tuple(t) => !t.is_empty(),
            Value::Dict(d) => !d.borrow().is_empty(),
            Value::Set(s) => !s.borrow().is_empty(),
            Value::Range(r) => !r.is_empty(),
            Value::Ext(e) => e.is_true(),
            Value::Builtin(_)
            | Value::Exception(_)
            | Value::Function(_)
            | Value::Module(_)
            | Value::NativeFn(_)
            | Value::Native(_)
            | Value::Bound(_)
            | Value::Class(_)
            | Value::BoundFn(_)
            | Value::Slice(_) => true,
            Value::Instance(_) => crate::vm::instance_truth(self),
        }
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&repr(self))
    }
}

/// `módulo.` para o `repr` de uma classe (vazio para `builtins`).
fn module_prefix(c: &ClassObj) -> String {
    match c.module().as_str() {
        "builtins" => String::new(),
        m => format!("{m}."),
    }
}

/// Embutidos que no CPython são classes (`str`, `int`, `range`...), não funções: o `repr` deles é
/// `<class 'str'>` e o tipo é `type`.
pub fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "bool" | "int" | "float" | "str" | "list" | "tuple" | "dict" | "set" | "range" | "NoneType" | "function"
            | "frozenset" | "bytes" | "bytearray" | "generator" | "module" | "slice" | "builtin_function_or_method"
            | "dict_keys" | "dict_values" | "dict_items" | "coroutine" | "async_generator" | "coroutine_wrapper"
    ) || EXC_CLASSES.iter().any(|(n, _)| *n == name)
}

thread_local! {
    static INTERNED: RefCell<std::collections::HashSet<&'static str>> =
        RefCell::new(std::collections::HashSet::new());
}

/// Texto com tempo de vida `'static`, um só por conteúdo (nomes de classes de usuário, que as
/// mensagens de erro e `type_name` precisam devolver como `&'static str`).
pub fn intern(name: &str) -> &'static str {
    INTERNED.with(|set| {
        let mut set = set.borrow_mut();
        if let Some(s) = set.get(name) {
            return *s;
        }
        let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
        set.insert(leaked);
        leaked
    })
}

/// Endereço de um objeto compartilhado, para identidade e para a pilha do `repr`.
fn addr<T: ?Sized>(rc: &Rc<T>) -> usize {
    Rc::as_ptr(rc) as *const () as usize
}

/// Pilha de contêineres em impressão (`Py_ReprEnter`/`Py_ReprLeave`), que corta a recursão de um
/// contêiner que contém a si mesmo com `[...]`, `{...}`, `(...)` ou `set(...)`.
#[derive(Default)]
pub(crate) struct ReprStack(Vec<usize>);

impl ReprStack {
    /// Falso se o objeto já está sendo impresso mais acima.
    fn enter(&mut self, id: usize) -> bool {
        if self.0.contains(&id) {
            return false;
        }
        self.0.push(id);
        true
    }

    fn leave(&mut self, id: usize) {
        if let Some(pos) = self.0.iter().rposition(|&x| x == id) {
            self.0.remove(pos);
        }
    }
}

/// `repr(v)`.
pub fn repr(v: &Value) -> String {
    let mut out = String::new();
    repr_into(v, &mut out, &mut ReprStack::default());
    out
}

/// `str(v)`: o próprio texto para `str`; para os demais tipos embutidos é igual ao `repr` (no
/// Python 3, inclusive `float` e `bytes`).
pub fn to_str(v: &Value) -> String {
    match v {
        Value::Str(s) => s.as_str().to_string(),
        Value::Exception(e) => exc_str(e),
        Value::Instance(_) => match crate::vm::instance_text(v, true) {
            Some(text) => text,
            None => repr(v),
        },
        _ => repr(v),
    }
}

pub(crate) fn repr_into(v: &Value, out: &mut String, stack: &mut ReprStack) {
    match v {
        Value::None => out.push_str("None"),
        Value::Bool(true) => out.push_str("True"),
        Value::Bool(false) => out.push_str("False"),
        Value::Int(i) => out.push_str(&int_repr(*i)),
        Value::Big(b) => out.push_str(&b.to_string()),
        Value::Float(x) => out.push_str(&float_repr(*x)),
        Value::Str(s) => out.push_str(&str_repr(s.as_str())),
        Value::Bytes(b) => out.push_str(&bytes_repr(b)),
        Value::ByteArray(b) => {
            out.push_str("bytearray(");
            out.push_str(&bytes_repr(&b.borrow()));
            out.push(')');
        }
        Value::List(l) => list::list_repr(&l.borrow(), addr(l), out, stack),
        Value::Tuple(t) => list::tuple_repr(t, addr(t), out, stack),
        Value::Dict(d) => dict::dict_repr(&d.borrow(), addr(d), out, stack),
        Value::Set(s) => set::set_repr(&s.borrow(), addr(s), out, stack),
        Value::Range(r) if r.step == 1 => out.push_str(&format!("range({}, {})", r.start, r.stop)),
        Value::Range(r) => out.push_str(&format!("range({}, {}, {})", r.start, r.stop, r.step)),
        Value::Builtin("Ellipsis" | "NotImplemented") => {
            if let Value::Builtin(n) = v {
                out.push_str(n);
            }
        }
        Value::Builtin(name) if is_builtin_type(name) => out.push_str(&format!("<class '{name}'>")),
        Value::Builtin(name) => out.push_str(&format!("<built-in function {name}>")),
        Value::Exception(e) => out.push_str(&exc_repr(e)),
        Value::Function(f) => out.push_str(&format!("<function {} at {:#x}>", f.code.name, addr(f))),
        Value::Module(m) => out.push_str(&format!("<module '{}'>", m.name)),
        Value::NativeFn(n) if is_builtin_type(n.name) => out.push_str(&format!("<class '{}'>", n.name)),
        Value::NativeFn(n) => out.push_str(&format!("<built-in function {}>", n.name)),
        Value::Ext(e) => out.push_str(&e.repr()),
        Value::Native(n) => match &*n.borrow() {
            Native::File(f) => out.push_str(&format!("<_io.TextIOWrapper name='{}' mode='r' encoding='utf-8'>", f.name)),
            Native::CsvReader { .. } => out.push_str("<_csv.reader object>"),
            Native::CsvWriter { .. } => out.push_str("<_csv.writer object>"),
        },
        Value::Bound(b) => out.push_str(&format!("<built-in method {} of {} object at {:#x}>", b.name, b.recv.type_name(), addr(b))),
        Value::Class(c) => out.push_str(&format!("<class '{}{}'>", module_prefix(c), c.name)),
        Value::Instance(i) => match crate::vm::instance_text(v, false) {
            Some(text) => out.push_str(&text),
            None => out.push_str(&format!("<{}{} object at {:#x}>", module_prefix(&i.class), i.class.name, addr(i))),
        },
        Value::Slice(s) => {
            out.push_str("slice(");
            repr_into(&s.0, out, stack);
            out.push_str(", ");
            repr_into(&s.1, out, stack);
            out.push_str(", ");
            repr_into(&s.2, out, stack);
            out.push(')');
        }
        Value::BoundFn(b) => {
            out.push_str(&format!("<bound method {} of ", b.1.code.name));
            repr_into(&b.0, out, stack);
            out.push('>');
        }
    }
}

/// `a is b` (ver a nota de identidade no topo do módulo).
pub fn is(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::None, Value::None) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y && (-5..=256).contains(x),
        (Value::Big(x), Value::Big(y)) => Rc::ptr_eq(x, y),
        (Value::Str(x), Value::Str(y)) => Rc::ptr_eq(x, y),
        (Value::Bytes(x), Value::Bytes(y)) => Rc::ptr_eq(x, y),
        (Value::ByteArray(x), Value::ByteArray(y)) => Rc::ptr_eq(x, y),
        (Value::List(x), Value::List(y)) => Rc::ptr_eq(x, y),
        (Value::Tuple(x), Value::Tuple(y)) => Rc::ptr_eq(x, y),
        (Value::Dict(x), Value::Dict(y)) => Rc::ptr_eq(x, y),
        (Value::Set(x), Value::Set(y)) => Rc::ptr_eq(x, y),
        (Value::Builtin(x), Value::Builtin(y)) => x == y,
        (Value::Exception(x), Value::Exception(y)) => Rc::ptr_eq(x, y),
        (Value::Function(x), Value::Function(y)) => Rc::ptr_eq(x, y),
        (Value::Module(x), Value::Module(y)) => Rc::ptr_eq(x, y),
        (Value::NativeFn(x), Value::NativeFn(y)) => x.name == y.name && x.f as usize == y.f as usize,
        (Value::Ext(x), Value::Ext(y)) => std::ptr::addr_eq(Rc::as_ptr(x), Rc::as_ptr(y)),
        (Value::Native(x), Value::Native(y)) => Rc::ptr_eq(x, y),
        (Value::Bound(x), Value::Bound(y)) => Rc::ptr_eq(x, y),
        (Value::Class(x), Value::Class(y)) => Rc::ptr_eq(x, y),
        (Value::Instance(x), Value::Instance(y)) => Rc::ptr_eq(x, y),
        (Value::BoundFn(x), Value::BoundFn(y)) => Rc::ptr_eq(x, y),
        (Value::Slice(x), Value::Slice(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// `range_equals`: mesma sequência de valores, não os mesmos argumentos.
fn range_eq(a: &Range, b: &Range) -> bool {
    let len = a.len();
    if len != b.len() {
        return false;
    }
    if len == 0 {
        return true;
    }
    if a.start != b.start {
        return false;
    }
    len == 1 || a.step == b.step
}

/// Visão numérica de `bool`, `int` e `float` para comparação entre tipos.
enum Num {
    Int(i64),
    Float(f64),
}

fn as_num(v: &Value) -> Option<Num> {
    match v {
        Value::Bool(b) => Some(Num::Int(i64::from(*b))),
        Value::Int(i) => Some(Num::Int(*i)),
        Value::Float(x) => Some(Num::Float(*x)),
        _ => None,
    }
}

/// `int == float` exato, como o `float_richcompare` (sem arredondar o inteiro para `double`).
fn int_float_eq(i: i64, x: f64) -> bool {
    // 2**63 é exato em `double`; fora de [-2**63, 2**63) nenhum `i64` é igual.
    const LIMIT: f64 = 9_223_372_036_854_775_808.0;
    x.is_finite() && x.fract() == 0.0 && (-LIMIT..LIMIT).contains(&x) && x as i64 == i
}

/// `a == b` dos tipos embutidos.
pub fn py_eq(a: &Value, b: &Value) -> bool {
    if matches!(a, Value::Instance(_)) || matches!(b, Value::Instance(_)) {
        if let Some(r) = crate::vm::instance_eq(a, b) {
            return r;
        }
    }
    if matches!(a, Value::Big(_)) || matches!(b, Value::Big(_)) {
        return match (a, b) {
            (Value::Float(x), Value::Big(n)) | (Value::Big(n), Value::Float(x)) => {
                crate::bigint::cmp_float(n, *x) == Some(std::cmp::Ordering::Equal)
            }
            _ => match (crate::bigint::as_big(a), crate::bigint::as_big(b)) {
                (Some(x), Some(y)) => x == y,
                _ => false,
            },
        };
    }
    if let (Some(x), Some(y)) = (as_num(a), as_num(b)) {
        return match (x, y) {
            (Num::Int(x), Num::Int(y)) => x == y,
            (Num::Float(x), Num::Float(y)) => x == y,
            (Num::Int(i), Num::Float(x)) | (Num::Float(x), Num::Int(i)) => int_float_eq(i, x),
        };
    }
    match (a, b) {
        (Value::None, Value::None) => true,
        (Value::Str(x), Value::Str(y)) => Rc::ptr_eq(x, y) || x.as_str() == y.as_str(),
        (Value::Bytes(x), Value::Bytes(y)) => x[..] == y[..],
        (Value::ByteArray(x), Value::ByteArray(y)) => Rc::ptr_eq(x, y) || *x.borrow() == *y.borrow(),
        (Value::ByteArray(x), Value::Bytes(y)) | (Value::Bytes(y), Value::ByteArray(x)) => x.borrow()[..] == y[..],
        (Value::List(x), Value::List(y)) => Rc::ptr_eq(x, y) || list::seq_eq(&x.borrow(), &y.borrow()),
        (Value::Tuple(x), Value::Tuple(y)) => Rc::ptr_eq(x, y) || list::seq_eq(x, y),
        (Value::Dict(x), Value::Dict(y)) => Rc::ptr_eq(x, y) || dict::dict_eq(&x.borrow(), &y.borrow()),
        (Value::Set(x), Value::Set(y)) => Rc::ptr_eq(x, y) || set::set_eq(&x.borrow(), &y.borrow()),
        (Value::Range(x), Value::Range(y)) => range_eq(x, y),
        (Value::Builtin(x), Value::Builtin(y)) => x == y,
        (Value::Exception(x), Value::Exception(y)) => Rc::ptr_eq(x, y),
        (Value::Function(x), Value::Function(y)) => Rc::ptr_eq(x, y),
        (Value::Module(x), Value::Module(y)) => Rc::ptr_eq(x, y),
        (Value::NativeFn(x), Value::NativeFn(y)) => x.name == y.name && x.f as usize == y.f as usize,
        (Value::Ext(x), Value::Ext(y)) => x.eq_value(b).or_else(|| y.eq_value(a)).unwrap_or_else(|| std::ptr::addr_eq(Rc::as_ptr(x), Rc::as_ptr(y))),
        (Value::Native(x), Value::Native(y)) => Rc::ptr_eq(x, y),
        (Value::Bound(x), Value::Bound(y)) => Rc::ptr_eq(x, y),
        (Value::Class(x), Value::Class(y)) => Rc::ptr_eq(x, y),
        (Value::Instance(x), Value::Instance(y)) => Rc::ptr_eq(x, y),
        (Value::BoundFn(x), Value::BoundFn(y)) => Rc::ptr_eq(x, y),
        (Value::Slice(x), Value::Slice(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// Hash fixo de `None` desde o 3.12 (`none_hash`), independente do endereço.
const NONE_HASH: i64 = 0xFCA8_6420;

/// `hash(v)`; contêineres mutáveis dão `TypeError: unhashable type: 'list'`.
pub fn hash(v: &Value) -> Result<i64, ObjError> {
    match v {
        Value::None => Ok(NONE_HASH),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Int(i) => Ok(int_hash(*i)),
        Value::Big(b) => Ok(crate::bigint::hash(b)),
        Value::Float(x) => Ok(float_hash(*x)),
        Value::Str(s) => Ok(s.hash()),
        Value::Bytes(b) => Ok(bytes_hash(b)),
        Value::Tuple(t) => tuple_hash(t),
        // `range_hash`: hash de `(len, start, step)`, com `None` no que não distingue a sequência.
        Value::Range(r) => {
            let len = r.len();
            let start = if len == 0 { Value::None } else { Value::Int(r.start) };
            let step = if len <= 1 { Value::None } else { Value::Int(r.step) };
            tuple_hash(&[Value::Int(len), start, step])
        }
        // O CPython usa o endereço; aqui basta um valor estável por função.
        Value::Builtin(name) => Ok(PyStr::new(*name).hash()),
        Value::Exception(e) => Ok((Rc::as_ptr(e) as usize >> 4) as i64),
        Value::Function(f) => Ok((Rc::as_ptr(f) as usize >> 4) as i64),
        Value::Module(m) => Ok((Rc::as_ptr(m) as usize >> 4) as i64),
        Value::NativeFn(n) => Ok(PyStr::new(n.name).hash()),
        Value::Ext(e) => Ok(e.hash_value().unwrap_or((Rc::as_ptr(e) as *const () as usize >> 4) as i64)),
        Value::Native(n) => Ok((Rc::as_ptr(n) as usize >> 4) as i64),
        Value::Bound(b) => Ok((Rc::as_ptr(b) as usize >> 4) as i64),
        Value::Class(c) => Ok((Rc::as_ptr(c) as usize >> 4) as i64),
        Value::Instance(i) => match crate::vm::instance_hash(v) {
            Some(h) => Ok(h),
            // `__hash__ = None` na classe: instâncias não são hasheáveis.
            None if matches!(i.class.lookup("__hash__"), Some(Value::None)) => {
                Err(ObjError::TypeError(format!("unhashable type: '{}'", i.class.name)))
            }
            None => Ok((Rc::as_ptr(i) as usize >> 4) as i64),
        },
        Value::BoundFn(b) => Ok((Rc::as_ptr(b) as usize >> 4) as i64),
        Value::Slice(s) => Ok((Rc::as_ptr(s) as usize >> 4) as i64),
        Value::List(_) | Value::Dict(_) | Value::Set(_) | Value::ByteArray(_) => {
            Err(ObjError::TypeError(format!("unhashable type: '{}'", v.type_name())))
        }
    }
}

/// `tuplehash` do 3.13 (variante do xxHash de 64 bits).
fn tuple_hash(items: &[Value]) -> Result<i64, ObjError> {
    const PRIME_1: u64 = 11_400_714_785_074_694_791;
    const PRIME_2: u64 = 14_029_467_366_897_019_727;
    const PRIME_5: u64 = 2_870_177_450_012_600_261;
    let mut acc = PRIME_5;
    for item in items {
        let lane = hash(item)? as u64;
        acc = acc.wrapping_add(lane.wrapping_mul(PRIME_2));
        acc = acc.rotate_left(31);
        acc = acc.wrapping_mul(PRIME_1);
    }
    acc = acc.wrapping_add((items.len() as u64) ^ (PRIME_5 ^ 3_527_539));
    if acc == u64::MAX {
        return Ok(1_546_275_796);
    }
    Ok(acc as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> Value {
        Value::str(text)
    }

    fn dict_of(pairs: Vec<(Value, Value)>) -> Value {
        let mut d = Dict::new();
        for (k, v) in pairs {
            d.set(k, v).unwrap();
        }
        Value::dict(d)
    }

    fn set_of(items: Vec<Value>) -> Value {
        let mut st = Set::new();
        for item in items {
            st.add(item).unwrap();
        }
        Value::set(st)
    }

    #[test]
    fn scalar_reprs() {
        assert_eq!(repr(&Value::None), "None");
        assert_eq!(repr(&Value::Bool(true)), "True");
        assert_eq!(repr(&Value::Int(-42)), "-42");
        assert_eq!(repr(&Value::Float(0.1)), "0.1");
        assert_eq!(repr(&Value::Float(1.0)), "1.0");
        assert_eq!(repr(&Value::Float(1e16)), "1e+16");
        assert_eq!(repr(&Value::Float(1e-5)), "1e-05");
        assert_eq!(repr(&Value::Float(-0.0)), "-0.0");
        assert_eq!(repr(&Value::Float(f64::NAN)), "nan");
        assert_eq!(repr(&s("it's")), "\"it's\"");
        assert_eq!(repr(&s("a'\"b")), "'a\\'\"b'");
        assert_eq!(repr(&s("\u{feff}é\t")), "'\\ufeffé\\t'");
        assert_eq!(repr(&Value::bytes(b"a'\x00\xff".to_vec())), "b\"a'\\x00\\xff\"");
    }

    #[test]
    fn str_versus_repr() {
        assert_eq!(to_str(&s("a\nb")), "a\nb");
        assert_eq!(to_str(&Value::Float(2.5)), "2.5");
        assert_eq!(to_str(&Value::list(vec![s("x")])), "['x']");
    }

    #[test]
    fn container_reprs() {
        assert_eq!(repr(&Value::list(vec![])), "[]");
        assert_eq!(repr(&Value::tuple(vec![])), "()");
        assert_eq!(repr(&Value::tuple(vec![Value::Int(1)])), "(1,)");
        assert_eq!(repr(&Value::tuple(vec![Value::Int(1), s("a")])), "(1, 'a')");
        assert_eq!(repr(&dict_of(vec![])), "{}");
        assert_eq!(repr(&Value::set(Set::new())), "set()");
        let nested = Value::list(vec![
            Value::Int(1),
            Value::tuple(vec![Value::None, Value::list(vec![])]),
            dict_of(vec![(s("k"), Value::list(vec![Value::Float(0.5)]))]),
        ]);
        assert_eq!(repr(&nested), "[1, (None, []), {'k': [0.5]}]");
    }

    #[test]
    fn recursive_reprs() {
        let l = Value::list(vec![Value::Int(1)]);
        if let Value::List(inner) = &l {
            inner.borrow_mut().push(l.clone());
        }
        assert_eq!(repr(&l), "[1, [...]]");

        let d = dict_of(vec![]);
        if let Value::Dict(inner) = &d {
            inner.borrow_mut().set(s("self"), d.clone()).unwrap();
        }
        assert_eq!(repr(&d), "{'self': {...}}");

        // Tupla que contém uma lista que contém a tupla.
        let l = Value::list(vec![]);
        let t = Value::tuple(vec![l.clone()]);
        if let Value::List(inner) = &l {
            inner.borrow_mut().push(t.clone());
        }
        assert_eq!(repr(&t), "([(...)],)");

        // O mesmo objeto repetido sem recursão não é cortado.
        let shared = Value::list(vec![Value::Int(0)]);
        assert_eq!(repr(&Value::list(vec![shared.clone(), shared])), "[[0], [0]]");
    }

    #[test]
    fn dict_keeps_insertion_order_and_first_key() {
        let mut d = Dict::new();
        d.set(s("b"), Value::Int(1)).unwrap();
        d.set(s("a"), Value::Int(2)).unwrap();
        d.set(Value::Int(1), s("x")).unwrap();
        d.set(Value::Bool(true), s("y")).unwrap();
        d.set(Value::Float(1.0), s("z")).unwrap();
        assert_eq!(repr(&Value::dict(d.clone())), "{'b': 1, 'a': 2, 1: 'z'}");
        assert_eq!(d.remove(&s("b")).unwrap().map(|v| repr(&v)), Some("1".to_string()));
        d.set(s("b"), Value::Int(3)).unwrap();
        assert_eq!(repr(&Value::dict(d.clone())), "{'a': 2, 1: 'z', 'b': 3}");
        assert_eq!(
            d.set(Value::list(vec![]), Value::None),
            Err(ObjError::TypeError("unhashable type: 'list'".to_string()))
        );
    }

    #[test]
    fn set_order_follows_cpython_table() {
        // `s = set()` seguido de `s.add` com 100, 1 e 8: posições 100 & 7 = 4, 1 e 8 & 7 = 0.
        let st = set_of(vec![Value::Int(100), Value::Int(1), Value::Int(8)]);
        assert_eq!(repr(&st), "{8, 1, 100}");
        // `s.add` com 50, 40, 30, 20, 10 e 0: o 10 colide com o 50 e vai para a posição 3; o quinto
        // elemento enche a tabela de 8 (fill * 5 >= mask * 3) e ela passa a 32 posições.
        let st = set_of((0..6).rev().map(|i| Value::Int(i * 10)).collect());
        assert_eq!(repr(&st), "{0, 40, 10, 50, 20, 30}");
        let st = set_of(vec![Value::Int(1), Value::Bool(true), Value::Float(1.0)]);
        assert_eq!(repr(&st), "{1}");
    }

    #[test]
    fn equality() {
        assert!(py_eq(&Value::Int(1), &Value::Float(1.0)));
        assert!(py_eq(&Value::Bool(true), &Value::Int(1)));
        assert!(!py_eq(&Value::Int(1), &s("1")));
        assert!(!py_eq(&Value::Float(f64::NAN), &Value::Float(f64::NAN)));
        assert!(!py_eq(&Value::Int(i64::MAX), &Value::Float(9_223_372_036_854_775_808.0)));
        assert!(py_eq(
            &Value::list(vec![Value::Int(1), s("a")]),
            &Value::list(vec![Value::Float(1.0), s("a")])
        ));
        assert!(!py_eq(&Value::list(vec![]), &Value::tuple(vec![])));
        let a = dict_of(vec![(s("x"), Value::Int(1)), (s("y"), Value::Int(2))]);
        let b = dict_of(vec![(s("y"), Value::Int(2)), (s("x"), Value::Int(1))]);
        assert!(py_eq(&a, &b));
    }

    #[test]
    fn hashes_match_cpython() {
        assert_eq!(hash(&Value::Int(-1)), Ok(-2));
        assert_eq!(hash(&Value::Int(-2)), Ok(-2));
        assert_eq!(hash(&Value::Int((1 << 61) - 1)), Ok(0));
        assert_eq!(hash(&Value::Int(i64::MAX)), Ok(3)); // 2**63 - 1 == 4 * (2**61 - 1) + 3
        assert_eq!(hash(&Value::Float(1.5)), Ok(1_152_921_504_606_846_977));
        assert_eq!(hash(&Value::Float(2.0)), Ok(2));
        assert_eq!(hash(&Value::Float(-1.0)), Ok(-2));
        assert_eq!(hash(&Value::Float(f64::INFINITY)), Ok(314_159));
        assert_eq!(hash(&Value::None), Ok(0xFCA8_6420));
        assert_eq!(hash(&s("")), Ok(0));
        assert_eq!(hash(&s("a")), hash(&Value::bytes(b"a".to_vec())));
        assert_eq!(hash(&Value::tuple(vec![])), Ok(5_740_354_900_026_072_187));
        assert_eq!(
            hash(&Value::dict(Dict::new())),
            Err(ObjError::TypeError("unhashable type: 'dict'".to_string()))
        );
    }

    #[test]
    fn str_indexes_by_code_point() {
        let st = PyStr::new("aé😀b");
        assert_eq!(st.len(), 4);
        assert_eq!(st.char_at(1), Some('é'));
        assert_eq!(st.char_at(2), Some('😀'));
        assert_eq!(st.char_at(4), None);
        assert_eq!(st.slice(1, 3), "é😀");
        assert_eq!(st.slice(3, 99), "b");
        assert_eq!(PyStr::new("abc").slice(2, 1), "");
    }

    #[test]
    fn int_overflow_is_internal_error() {
        assert_eq!(int_add(i64::MAX, 1), Err(ObjError::IntOverflow));
        assert_eq!(int_neg(i64::MIN), Err(ObjError::IntOverflow));
        assert_eq!(int_mul(-3, 7), Ok(-21));
        assert_eq!(int_sub(0, 5), Ok(-5));
    }
}
