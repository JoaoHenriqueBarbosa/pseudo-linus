//! Imagem do heap (fatias H1 e H2 de `wip/notes/python-fork.md`): o grafo de `Value` do interpretador
//! copiado para um tipo `Send`, e reconstruído como um grafo novo, sem `unsafe`.
//!
//! `Value` é feito de `Rc`, que não atravessa threads, e o filho de um `os.fork` nasce numa thread do
//! SO nova. Por isso o pai captura uma [`HeapImage`] (uma arena de nós endereçados por índice) e o
//! filho a reconstrói. O percurso guarda o endereço de cada `Rc` visitado, então o compartilhamento
//! (dois nomes para o mesmo objeto) e os ciclos (`a = []; a.append(a)`) sobrevivem à ida e volta.
//!
//! O percurso usa uma pilha de trabalho própria: uma lista aninhada em milhares de níveis não
//! consome pilha Rust. Todo `Value` é casado por variante, sem `_ =>`: um variant novo não compila
//! até ganhar tratamento aqui (ou o erro [`ImageError::Unsupported`], enquanto a fatia dele não vem).
//!
//! Além dos valores, a arena guarda o que só existe por trás deles: o `Code` compilado de cada função,
//! os escopos de closure (`Env`, as "células" deste interpretador) e a tabela de globais de cada módulo
//! (`Rc<RefCell<VarMap>>`, compartilhada por todas as funções dele).
//!
//! ## O `Code` vai por cópia, não por `Arc`
//! `Code` é imutável, mas não é `Send`: as constantes são `Value` e os nomes são `Rc<str>`. Então a
//! imagem copia os dados dele (instruções `Copy`, linhas, spans, nomes como `String`, constantes como
//! itens da arena, funções aninhadas como índices de outros nós `Code`), e o filho refaz um `Rc<Code>`
//! por nó. Duas funções que dividiam o mesmo `Rc<Code>` no pai continuam dividindo no filho, e os
//! nomes iguais viram o mesmo `Rc<str>` (o caminho rápido de `LocalMap` compara o ponteiro antes do texto).
//!
//! ## Em três passadas
//! Reconstruir um grafo com ciclos exige que o alvo exista antes de quem o aponta. Os campos
//! imutáveis de um objeto (as bases de uma classe, os `defaults` de uma função, o `parent` de um `Env`,
//! a classe de uma instância, os `args` de uma exceção) obrigam a ordem filhos antes dos pais; os campos
//! mutáveis (`dict` da classe e da instância, `attrs`, as variáveis de um `Env`, a cadeia de uma
//! exceção) são preenchidos depois, quando todo alvo existe. Todo ciclo passa por um campo mutável.
//!
//! 1. folhas imutáveis e cascas dos mutáveis sem dependência (`list`, `dict`, `set`, módulo, globais);
//! 2. os nós com campo imutável apontando para outro nó, em ordem de dependência (pilha explícita);
//! 3. o conteúdo mutável de todos.
//!
//! ## O que fica para as fatias seguintes
//! - `id()` e o hash por identidade vêm do endereço do `Rc` (`object::py_addr`), que o grafo novo não
//!   preserva; um `set` com função, classe ou instância sem `__hash__` guarda o hash do pai. A tabela
//!   de endereços que o filho consultaria não existe ainda (nem a H5 a traz): pendente.
//! - `__subclasses__` guarda referências fracas: só as subclasses que estão na imagem são refeitas. Com
//!   a `Vm` inteira como raiz (H5) toda subclasse viva está nela.
//! - `NativeFn` é refeita como um `Rc` novo com o mesmo ponteiro de função; tabelas da `Vm` que comparem
//!   o `Rc` por identidade são da H5.
//! - Objetos `Ext` sem `ExtObject::image` (`FrameObj`, `CodeObject`, a conexão `sqlite3`, os fluxos `zlib`,
//!   `yaml`, `imaging`, o resultado de `compile()`) continuam `Unsupported`, com o nome do tipo: a captura
//!   falha em vez de copiar errado.
//!
//! ## H3: iteradores, geradores, tracebacks, `weakref`
//! - Iteradores (`ExtImage::Lazy`): a posição e os valores de cada um (`IterNode` espelha o `PyIter` de um
//!   laço `for`). O quadro suspenso de um gerador (`FrameNode`: pilha de valores e de iteradores, blocos
//!   protegidos, `pc`, exceções em tratamento) é dado do `GenCore`, que o gerador, o `coroutine_wrapper` e os
//!   aguardáveis de `__anext__` compartilham: um nó só. O filho precisa de uma `Vm` onde assentar
//!   (`restore_in`); o `Code` e o `Env` do quadro são nós comuns.
//! - `traceback`: refeito de `TracebackObj::frames` (a cadeia com o `tb_next` atribuído já aplicado) e
//!   `TracebackObj::make`; o `tb_frame` guardado em cache e as visões cortadas por `tb_next` não voltam
//!   (o `tb_frame` é refeito sob demanda, como na primeira leitura).
//! - `weakref`: guarda o referente (se vivo) e o refaz fraco. O referente só continua vivo no filho se o
//!   restante do grafo o segura, como no pai.
//!
//! ## H4: `Native` e objetos nativos com estado `Send`
//! - `Value::Native` (arquivo, leitor e escritor de `csv`) é um nó com casca em branco na primeira passada.
//!   `sockets` e `ssl` são objetos Python (`_socket.py`, `_ssl.py`), logo o fd e o estado TLS entram no
//!   percurso comum; `FdGuard` guarda o número do fd, que a tabela herdada do filho já tem.
//! - `ExtImage::Opaque { tag, state, refs }`: o estado copiável (`Arc<dyn Any + Send + Sync>`) e os valores
//!   que o objeto aponta. `restore_opaque` é a tabela de `tag` para o módulo que refaz; um `tag` fora dela
//!   é `Malformed`. Cobertos: `std_buffer`, `dict_view`, `generic_alias`, `union_type`, os de `typeattrs`,
//!   os de `classes`, `hash`, `mersenne_twister`, `fd_guard`, `ucd` e os três de `re`.
//!
//! ## H5: o resto da `Vm`
//! `VmImage` (ver a doc dele). Faltam as fatias F: o quadro em execução e `frames_stack`.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use num_bigint::BigInt;

use crate::compile::{Code, Op, Span};
use crate::fold::{Fold, SumPhase};
use crate::generator::{AwaitMode, Callback, CallbackKind, Collect, GenCore, GenFlags, GenRole, Kind, Layer, Pull, ResumeUse, Resuming, Tail};
use crate::lazy::{IterParts, LazyParts};
use crate::modules::csv::{Dialect, Reader};
use crate::object::{
    AttrMap, BoundMethod, ClassObj, Dict, Env, ExcChain, ExcObj, ExtImage, FileKind, FuncObj, InstanceObj, LocalMap, ModuleObj,
    Native, NativeFn, NativeFnPtr, OpaqueImage, PyFile, Range, Set, SetTable, TableSlot, Value, VarMap,
};
use crate::vm::{Attempt, Block, CallLink, Callee, Chain, ChainKind, Dunder, Frame, Slot, TbEntry, TruthUse, Vm};

/// Por que uma imagem não pôde ser feita ou refeita.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageError {
    /// O valor é de um tipo que esta fatia ainda não cobre (nome do tipo).
    Unsupported(&'static str),
    /// A imagem não é a de um percurso válido (índice fora da arena, nó nunca preenchido, ciclo entre
    /// imutáveis). O capturador não produz isso.
    Malformed(&'static str),
}

/// Uma referência dentro da imagem: o escalar inline, ou o índice de um nó.
#[derive(Debug, Clone, Copy)]
enum Item {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    Range(i64, i64, i64),
    /// Função embutida pelo nome: já é dado `Send`, não precisa de nó.
    Builtin(&'static str),
    Node(u32),
}

/// Pares nome e valor na ordem em que o objeto de origem os iterou.
type Pairs = Vec<(String, Item)>;

struct FunctionImage {
    /// Nó `Code`.
    code: u32,
    defaults: Vec<Item>,
    kwdefaults: Pairs,
    /// Nó `Env`.
    closure: Option<u32>,
    /// Nó `Globals`.
    globals: u32,
    attrs: Pairs,
}

struct CodeImage {
    ops: Vec<Op>,
    lines: Vec<usize>,
    spans: Vec<Span>,
    consts: Vec<Item>,
    names: Vec<String>,
    name: String,
    qualname: String,
    params: Vec<String>,
    posonly: usize,
    vararg: Option<String>,
    kwonly: Vec<String>,
    kwarg: Option<String>,
    varnames: Vec<String>,
    cellvars: Vec<String>,
    freevars: Vec<String>,
    is_function: bool,
    is_class: bool,
    is_generator: bool,
    is_async: bool,
    /// Nós `Code`.
    functions: Vec<u32>,
    filename: String,
    doc: Option<String>,
    first_line: usize,
    uses_class_cell: bool,
    internal: bool,
    type_params_role: u8,
    future_flags: i64,
    /// O bytecode do CPython que o `dis` lê (`Code::cpy`).
    cpy: Option<Box<EmittedImage>>,
}

/// O `Emitted` de `cpybc` como dado: as constantes são itens da arena e os nomes, texto.
struct EmittedImage {
    code: Vec<u8>,
    consts: Vec<Item>,
    names: Vec<String>,
    linetable: Vec<u8>,
    exceptiontable: Vec<u8>,
    stacksize: usize,
    first_line: i32,
    synthetic: bool,
}

struct EnvImage {
    vars: Pairs,
    order: Vec<String>,
    /// Nó `Env`.
    parent: Option<u32>,
    is_class: bool,
    is_module: bool,
    finished: bool,
}struct ClassImage {
    name: String,
    qualname: String,
    /// Nós `Class`.
    bases: Vec<u32>,
    builtin_base: Option<&'static str>,
    data_base: Option<&'static str>,
    /// Nó `Class`.
    meta: Option<u32>,
    is_meta: bool,
    dict: Pairs,
    /// Endereços das subclasses vivas na captura; `finish` os troca por `subclasses`.
    sub_addrs: Vec<usize>,
    /// Nós `Class` das subclasses que estão na imagem, em ordem de criação.
    subclasses: Vec<u32>,
}

struct InstanceImage {
    /// Nó `Class`.
    class: u32,
    dict: Pairs,
    /// O `dict` vivo de `__dict__`, se alguém o pediu.
    view: Option<Item>,
    payload: Option<Item>,
    /// O `__del__` já rodou no pai: o objeto continua finalizado no filho.
    finalized: bool,
}

struct ExcImage {
    kind: &'static str,
    args: Vec<Item>,
    traceback: Option<Item>,
    cause: Option<Item>,
    context: Option<Item>,
    suppress: bool,
    extra: Vec<(&'static str, Item)>,
}

/// Um iterador de laço `for` (`PyIter`) com os valores já convertidos em itens.
enum IterNode {
    List(Item, usize),
    Tuple(Item, usize),
    Str(Item, usize),
    Range { next: i64, step: i64, remaining: i64 },
    Items(Vec<Item>, usize),
    Native(Item),
    Ext(Item),
    Inst(Item),
}

/// Um iterador preguiçoso embutido (ver [`LazyParts`]) com os valores convertidos em itens.
enum LazyNode {
    Seq { kind: &'static str, items: Vec<Item>, pos: usize },
    Reversed { kind: &'static str, items: Vec<Item>, next: usize },
    Map { func: Item, iters: Vec<IterNode> },
    Filter { func: Item, iter: IterNode },
    Zip { iters: Vec<IterNode>, strict: bool },
    Enumerate { iter: IterNode, next_index: i64 },
    CallIter { func: Item, sentinel: Item, done: bool },
    Boxed { iter: IterNode },
    OldSeq { obj: Item, index: i64, done: bool },
}

enum SlotNode {
    Val(Item),
    Iter(IterNode),
}

/// Um `Frame` (pilha de valores, blocos protegidos, `pc`) como dado. Serve ao quadro suspenso de um
/// gerador e, na fatia F, ao quadro em execução no `os.fork`.
struct FrameNode {
    /// Nó `Code`.
    code: u32,
    /// Nó `Env`.
    env: u32,
    stack: Vec<SlotNode>,
    /// `(handler, depth, handled)` de cada bloco protegido.
    blocks: Vec<(usize, usize, usize)>,
    pc: usize,
    handled: Vec<Item>,
    handled_base: usize,
}

/// O que um `CallLink` guarda, como dado: a função, as linhas, as globais do chamador e o que fazer com o
/// valor que o quadro devolver.
struct LinkNode {
    /// Nó `Function`; ausente no quadro de um gerador retomado pelo laço (`resuming`).
    func: Option<u32>,
    caller_line: usize,
    handled_len: usize,
    profiled: bool,
    /// Nó `Globals`.
    caller_globals: Option<u32>,
    instance: Option<Item>,
    on_stop: Option<usize>,
    then: Option<DunderNode>,
    resuming: Option<ResumingNode>,
}

/// Uma retomada de gerador em curso no laço (`generator::Resuming`): o gerador, o que `GenCore::end` precisa
/// para fechá-la e o que o laço fará com o valor entregue.
struct ResumingNode {
    /// Nó `GenCore`.
    core: u32,
    base: usize,
    caller_line: usize,
    /// Nó `Globals`.
    caller_globals: Option<u32>,
    use_: UseNode,
}

/// `generator::ResumeUse` com o valor como item.
enum UseNode {
    ForIter { exit: usize },
    Next { default: Option<Item> },
    Delegate { end: usize },
    DelegateThrow { end: usize },
    /// A exceção original, como item (`PyException::to_value`).
    DelegateExit(Item),
    Close,
    Collect { items: Vec<Item>, call: Item, kwargs: Pairs, fold: Option<FoldNode> },
    /// `Pull`: a fonte, as camadas já descidas e o consumidor.
    Pull(PullNode),
}

/// `generator::Pull` com os valores como itens.
struct PullNode {
    root: Item,
    layers: Vec<LayerNode>,
    then: Box<UseNode>,
}

/// `generator::Callback` (o callback em quadro e o que fazer com o valor dele) com os valores como itens.
struct CallbackNode {
    pull: PullNode,
    what: WhatNode,
}

/// `generator::CallbackKind` com os valores como itens.
enum WhatNode {
    Mapped,
    Kept { it: Item, item: Item },
    Keyed { item: Item },
    Advance { leaf: Item },
    Start,
}

/// `fold::Fold` com os valores como itens.
enum FoldNode {
    Sum { acc: Item, phase: SumPhase },
    Set { target: Item, frozen: bool },
    Dict { target: Item, kwargs: Pairs, index: usize },
    MinMax { max: bool, key: Option<Item>, default: Option<Item>, best: Option<(Item, Item)> },
    Any,
    All,
    Extend { target: Item },
    Sorted { key: Option<Item>, reverse: bool, items: Vec<Item>, pairs: Vec<(Item, Item)> },
}

/// `generator::Layer` com o iterador preguiçoso como item.
enum LayerNode {
    Enumerate(Item),
    Filter(Item),
    Many { it: Item, at: usize, got: Vec<Item> },
    Check { it: Item, at: usize },
}

struct AttemptNode {
    recv: Item,
    name: &'static str,
    other: Item,
    invert: bool,
}

struct ChainNode {
    kind: ChainKind,
    a: Item,
    b: Item,
    rest: Vec<AttemptNode>,
    invert: bool,
}

/// O `Dunder` de um quadro de método mágico (ver `Vm::finish_dunder`) com os valores como itens.
enum DunderNode {
    Chain(ChainNode),
    Push,
    Discard,
    Boolean { negate: bool },
    Truth { how: TruthUse, len: bool },
    Iter,
    Callback(Box<CallbackNode>),
    Signal,
    Import(Box<ImportNode>),
    Exec(Box<ExecNode>),
}

/// `modrun::ImportRun`: o módulo cujo corpo roda e a cadeia do `import` que falta.
struct ImportNode {
    name: String,
    /// O módulo, como item.
    module: Item,
    /// `(rest, result)` de `modrun::ImportPlan`.
    plan: Option<(Vec<String>, String)>,
}

/// `builtins_ext::ExecRun`: o que o `exec`/`eval` em curso devolve às globais e ao `locals` quando o quadro fecha.
struct ExecNode {
    eval: bool,
    /// Nó `Globals` do quadro.
    globals: u32,
    ns: Option<NamespacesNode>,
    back: Option<(Item, Item, Vec<(Item, Item)>)>,
}

struct NamespacesNode {
    gdict: Item,
    was: Vec<String>,
    lwas: Vec<String>,
    separate: Option<Item>,
    /// Nó `Globals`.
    backup: Option<u32>,
}

/// Um quadro aberto por uma chamada (`Callee`): o `Frame` e o `CallLink` dele. É o que o `os.fork` copia de
/// `frames_stack` e do chamado em execução.
struct CalleeNode {
    frame: FrameNode,
    link: LinkNode,
}

/// Um objeto `frame` (`FrameObj`) com o código e os escopos como nós.
struct FrameObjNode {
    line: usize,
    name: String,
    file: String,
    /// Nós `Code`, `Env` e `Env`.
    code: Option<u32>,
    code_object: Item,
    env: Option<u32>,
    held: Option<u32>,
    back: Item,
    live: bool,
    trace: Item,
    trace_lines: bool,
    trace_opcodes: bool,
    last_line: usize,
}

/// O estado compartilhado de um gerador (`GenCore`).
struct CoreNode {
    kind: Kind,
    /// Nó `Globals`.
    globals: u32,
    frame: FrameNode,
    flags: GenFlags,
    returned: Option<Item>,
}

enum ModeNode {
    Send(Item),
    Throw(Item),
    Close,
}

enum RoleNode {
    Object,
    CoroWrapper,
    Await { mode: Option<ModeNode>, started: bool, done: bool, closing: bool },
}

/// Uma entrada de traceback com o escopo e o código do quadro como nós.
struct TbNode {
    line: usize,
    name: String,
    file: String,
    span: Span,
    /// Nós `Env` e `Code`.
    held: Option<(u32, u32)>,
}

struct FileNode {
    kind: FileKind,
    lines: Vec<String>,
    pos: usize,
    loaded: bool,
    closed: bool,
    name: String,
    raw: Vec<u8>,
    raw_eof: bool,
}

/// Um `Native` (arquivo, leitor ou escritor de `csv`) como dado.
enum NativeNode {
    File(FileNode),
    CsvReader { reader: Reader, src: Item },
    CsvWriter { dialect: Dialect, target: Item },
}

/// Um objeto nativo coberto por [`ExtImage`], com os valores já convertidos em itens.
enum ExtNode {
    StaticMethod(Item),
    ClassMethod(Item),
    Property { get: Item, set: Option<Item>, del: Option<Item>, doc: Item },
    PropertyCopy { obj: Item, which: &'static str },
    PlainObject,
    /// Nó `Env`.
    ClassCell(u32),
    Lazy(LazyNode),
    /// Nó `GenCore` e o papel do objeto sobre ele.
    Generator { core: u32, role: RoleNode },
    AsyncGenWrapped(Item),
    WeakRef { target: Option<Item>, callback: Option<Item>, hash: Option<i64> },
    Opaque { tag: &'static str, state: Arc<dyn std::any::Any + Send + Sync>, refs: Vec<Item> },
    Traceback { entries: Vec<TbNode>, filename: String },
    /// Nó `Code` opcional do objeto `code` de uma função ou de um quadro.
    CodeObject { name: String, filename: String, code: Option<u32> },
    /// Nó `Code` do módulo compilado por `compile()`.
    CodeSource { src: String, filename: String, code: u32 },
    Frame(Box<FrameObjNode>),
}

enum Node {
    /// Reservado pelo percurso e ainda não preenchido.
    Pending,
    Big(BigInt),
    Str(String),
    Bytes(Vec<u8>),
    ByteArray(Vec<u8>),
    List(Vec<Item>),
    Tuple(Vec<Item>),
    /// Hash, chave e valor, na ordem de inserção.
    Dict(Vec<(i64, Item, Item)>),
    /// `set` ou `frozenset`, com a tabela de posições inteira (a ordem de iteração depende dela).
    Set(SetTable<Item>),
    NativeFn(&'static str, NativeFnPtr),
    Slice(Item, Item, Item),
    Bound { recv: Item, name: &'static str },
    /// `recv` e o nó `Function`.
    BoundFn { recv: Item, func: u32 },
    Function(Box<FunctionImage>),
    Code(Box<CodeImage>),
    Env(Box<EnvImage>),
    /// A tabela de globais de um módulo (`Rc<RefCell<VarMap>>`).
    Globals(Pairs),
    Module { name: &'static str, attrs: Pairs },
    Class(Box<ClassImage>),
    Instance(Box<InstanceImage>),
    Exception(Box<ExcImage>),
    Ext(ExtNode),
    /// `Value::Native`: arquivo, leitor ou escritor de `csv`.
    Native(Box<NativeNode>),
    /// O estado compartilhado de um gerador.
    GenCore(Box<CoreNode>),
    /// Um quadro em execução ou suspenso (raiz da imagem de um `os.fork`; ninguém aponta para ele).
    Frame(Box<FrameNode>),
    /// Um chamado de `frames_stack` ou em execução (raiz da imagem de um `os.fork`).
    Callee(Box<CalleeNode>),
}

/// O heap de um conjunto de raízes, pronto para cruzar threads.
pub struct HeapImage {
    nodes: Vec<Node>,
    roots: Vec<Item>,
    /// O `id()` que cada objeto tinha na captura, por nó: o filho de um `os.fork` o mantém.
    ids: Vec<(u32, i64)>,
}

// Compilar isto prova que a imagem é `Send`: é a razão de ela existir.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<HeapImage>();
};

/// O que falta preencher de um nó reservado.
enum Pending {
    Val(Value),
    Code(Rc<Code>),
    Env(Rc<Env>),
    Globals(Rc<RefCell<VarMap>>),
    Ext(ExtImage),
    Core(Rc<GenCore>),
    Traceback(Vec<TbEntry>, Rc<str>),
}

/// Uma raiz da imagem. A `Vm` guarda, além de `Value`s, tabelas de globais, código e escopos; o `os.fork`
/// leva também os quadros da execução.
pub(crate) enum Root<'a> {
    Value(Value),
    Globals(Rc<RefCell<VarMap>>),
    Code(Rc<Code>),
    Env(Rc<Env>),
    Frame(&'a Frame),
    Callee(&'a Callee),
}

struct Encoder {
    nodes: Vec<Node>,
    /// Endereço do `Rc` visitado para o índice do nó dele.
    seen: HashMap<usize, u32>,
    /// Nós reservados cujo conteúdo falta preencher.
    work: Vec<(u32, Pending)>,
    /// O `id()` de cada objeto no momento em que ganhou nó.
    ids: Vec<(u32, i64)>,
}

fn address<T: ?Sized>(rc: &Rc<T>) -> usize {
    Rc::as_ptr(rc) as *const () as usize
}

fn names_of(names: &[Rc<str>]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

impl Encoder {
    /// Uma raiz da imagem: o `Value` ou o nó de globais, de código ou de escopo.
    fn root(&mut self, root: &Root) -> Result<Item, ImageError> {
        Ok(match root {
            Root::Value(v) => self.item(v)?,
            Root::Globals(g) => Item::Node(self.deferred(address(g), || Pending::Globals(g.clone()))),
            Root::Code(c) => Item::Node(self.deferred(address(c), || Pending::Code(c.clone()))),
            Root::Env(e) => Item::Node(self.env_ref(e)),
            Root::Frame(f) => {
                let node = Node::Frame(Box::new(self.frame(f)?));
                self.push(node)
            }
            Root::Callee(c) => {
                let node = Node::Callee(Box::new(self.callee(c)?));
                self.push(node)
            }
        })
    }

    /// Um nó que nenhum outro aponta (raiz de quadro): entra na arena sem endereço de origem.
    fn push(&mut self, node: Node) -> Item {
        self.nodes.push(node);
        Item::Node((self.nodes.len() - 1) as u32)
    }

    /// O índice do nó de `key`, criado por `build` na primeira visita.
    fn reserve(&mut self, key: usize, build: impl FnOnce() -> (Node, Option<Pending>)) -> u32 {
        if let Some(&index) = self.seen.get(&key) {
            return index;
        }
        let index = self.nodes.len() as u32;
        let (node, work) = build();
        self.nodes.push(node);
        self.seen.insert(key, index);
        if let Some(work) = work {
            self.work.push((index, work));
        }
        index
    }

    /// Nó sem filhos: preenchido na hora.
    fn leaf(&mut self, key: usize, make: impl FnOnce() -> Node) -> Item {
        Item::Node(self.reserve(key, || (make(), None)))
    }

    /// Nó com filhos: reservado agora, preenchido por `drain` (a pilha própria, sem recursão Rust).
    fn deferred(&mut self, key: usize, pending: impl FnOnce() -> Pending) -> u32 {
        self.reserve(key, || (Node::Pending, Some(pending())))
    }

    fn env_ref(&mut self, env: &Rc<Env>) -> u32 {
        self.deferred(address(env), || Pending::Env(env.clone()))
    }

    /// O item de `v`; um objeto que ganha nó agora guarda também o `id()` que tem.
    fn item(&mut self, v: &Value) -> Result<Item, ImageError> {
        let before = self.nodes.len();
        let item = self.item_of(v)?;
        if let (true, Item::Node(index)) = (self.nodes.len() > before, item) {
            self.ids.push((index, crate::builtins::id_of(v)));
        }
        Ok(item)
    }

    fn item_of(&mut self, v: &Value) -> Result<Item, ImageError> {
        Ok(match v {
            Value::None => Item::None,
            Value::Bool(b) => Item::Bool(*b),
            Value::Int(n) => Item::Int(*n),
            Value::Float(f) => Item::Float(*f),
            Value::Range(r) => Item::Range(r.start, r.stop, r.step),
            Value::Builtin(name) => Item::Builtin(*name),
            Value::Big(b) => self.leaf(address(b), || Node::Big((**b).clone())),
            Value::Str(s) => self.leaf(address(s), || Node::Str(s.as_str().to_string())),
            Value::Bytes(b) => self.leaf(address(b), || Node::Bytes(b.to_vec())),
            Value::ByteArray(b) => self.leaf(address(b), || Node::ByteArray(b.borrow().clone())),
            Value::NativeFn(f) => self.leaf(address(f), || Node::NativeFn(f.name, f.f)),
            Value::List(l) => Item::Node(self.deferred(address(l), || Pending::Val(v.clone()))),
            Value::Tuple(t) => Item::Node(self.deferred(address(t), || Pending::Val(v.clone()))),
            Value::Dict(d) => Item::Node(self.deferred(address(d), || Pending::Val(v.clone()))),
            Value::Set(s) => Item::Node(self.deferred(address(s), || Pending::Val(v.clone()))),
            Value::Exception(e) => Item::Node(self.deferred(address(e), || Pending::Val(v.clone()))),
            Value::Function(f) => Item::Node(self.deferred(address(f), || Pending::Val(v.clone()))),
            Value::Module(m) => Item::Node(self.deferred(address(m), || Pending::Val(v.clone()))),
            Value::Bound(b) => Item::Node(self.deferred(address(b), || Pending::Val(v.clone()))),
            Value::Class(c) => Item::Node(self.deferred(address(c), || Pending::Val(v.clone()))),
            Value::Instance(i) => Item::Node(self.deferred(address(i), || Pending::Val(v.clone()))),
            Value::BoundFn(b) => Item::Node(self.deferred(address(b), || Pending::Val(v.clone()))),
            Value::Slice(s) => Item::Node(self.deferred(address(s), || Pending::Val(v.clone()))),
            Value::Ext(e) => {
                let key = address(e);
                if let Some(&index) = self.seen.get(&key) {
                    return Ok(Item::Node(index));
                }
                // O traceback é de `tbobj`, que guarda a cadeia em campos privados: a imagem lê o que ele
                // expõe (`frames`, já com o `tb_next` atribuído aplicado) e refaz com `make`.
                if let Some(tb) = e.as_any().and_then(|a| a.downcast_ref::<crate::tbobj::TracebackObj>()) {
                    let (entries, filename) = tb.frames();
                    return Ok(Item::Node(self.deferred(key, || Pending::Traceback(entries, filename))));
                }
                let image = e.image().ok_or(ImageError::Unsupported(e.type_name()))?;
                Item::Node(self.deferred(key, || Pending::Ext(image)))
            }
            Value::Native(n) => Item::Node(self.deferred(address(n), || Pending::Val(v.clone()))),
        })
    }

    fn items(&mut self, values: &[Value]) -> Result<Vec<Item>, ImageError> {
        let mut out = Vec::with_capacity(values.len());
        for v in values {
            out.push(self.item(v)?);
        }
        Ok(out)
    }

    fn opt_item(&mut self, value: Option<&Value>) -> Result<Option<Item>, ImageError> {
        match value {
            Some(v) => Ok(Some(self.item(v)?)),
            None => Ok(None),
        }
    }

    fn pairs(&mut self, entries: Vec<(String, Value)>) -> Result<Pairs, ImageError> {
        let mut out = Vec::with_capacity(entries.len());
        for (k, v) in entries {
            let item = self.item(&v)?;
            out.push((k, item));
        }
        Ok(out)
    }

    /// O índice do nó de um valor que a imagem sabe que é um nó (classe, função).
    fn node_index(&mut self, v: &Value) -> Result<u32, ImageError> {
        match self.item(v)? {
            Item::Node(i) => Ok(i),
            _ => Err(ImageError::Malformed("expected a node reference")),
        }
    }

    /// Preenche os nós reservados até a pilha de trabalho esvaziar.
    fn drain(&mut self) -> Result<(), ImageError> {
        while let Some((index, pending)) = self.work.pop() {
            let node = match pending {
                Pending::Val(value) => self.value_node(&value)?,
                Pending::Code(code) => Node::Code(Box::new(self.code(&code)?)),
                Pending::Env(env) => Node::Env(Box::new(self.env(&env)?)),
                Pending::Globals(map) => {
                    let entries: Vec<(String, Value)> = map.borrow().iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
                    Node::Globals(self.pairs(entries)?)
                }
                Pending::Ext(image) => Node::Ext(self.ext(image)?),
                Pending::Core(core) => Node::GenCore(Box::new(self.core(&core)?)),
                Pending::Traceback(entries, filename) => Node::Ext(self.traceback(entries, &filename)),
            };
            self.nodes[index as usize] = node;
        }
        Ok(())
    }

    fn value_node(&mut self, value: &Value) -> Result<Node, ImageError> {
        Ok(match value {
            Value::List(l) => {
                let snapshot = l.borrow().clone();
                Node::List(self.items(&snapshot)?)
            }
            Value::Tuple(t) => Node::Tuple(self.items(t)?),
            Value::Dict(d) => {
                let entries: Vec<(i64, Value, Value)> = d.borrow().iter_hashed().map(|(h, k, v)| (h, k.clone(), v.clone())).collect();
                let mut out = Vec::with_capacity(entries.len());
                for (h, k, v) in &entries {
                    out.push((*h, self.item(k)?, self.item(v)?));
                }
                Node::Dict(out)
            }
            Value::Set(s) => {
                let mut failure = None;
                let table = s.borrow().export_table(|v| match self.item(v) {
                    Ok(item) => item,
                    Err(e) => {
                        failure.get_or_insert(e);
                        Item::None
                    }
                });
                if let Some(e) = failure {
                    return Err(e);
                }
                Node::Set(table)
            }
            Value::Slice(s) => Node::Slice(self.item(&s.0)?, self.item(&s.1)?, self.item(&s.2)?),
            Value::Bound(b) => Node::Bound { recv: self.item(&b.recv)?, name: b.name },
            Value::BoundFn(b) => {
                let recv = self.item(&b.0)?;
                let func = self.node_index(&Value::Function(b.1.clone()))?;
                Node::BoundFn { recv, func }
            }
            Value::Function(f) => Node::Function(Box::new(self.function(f)?)),
            Value::Module(m) => {
                let attrs: Vec<(String, Value)> = m.attrs.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                Node::Module { name: m.name, attrs: self.pairs(attrs)? }
            }
            Value::Class(c) => Node::Class(Box::new(self.class(c)?)),
            Value::Instance(i) => Node::Instance(Box::new(self.instance(i)?)),
            Value::Exception(e) => Node::Exception(Box::new(self.exception(e)?)),
            Value::Native(n) => Node::Native(Box::new(self.native(&n.borrow())?)),
            Value::None
            | Value::Bool(_)
            | Value::Int(_)
            | Value::Big(_)
            | Value::Float(_)
            | Value::Str(_)
            | Value::Bytes(_)
            | Value::ByteArray(_)
            | Value::Range(_)
            | Value::Builtin(_)
            | Value::NativeFn(_)
            | Value::Ext(_) => return Err(ImageError::Malformed("work item has no deferred content")),
        })
    }

    fn function(&mut self, f: &Rc<FuncObj>) -> Result<FunctionImage, ImageError> {
        let code = self.deferred(address(&f.code), || Pending::Code(f.code.clone()));
        let defaults = self.items(&f.defaults)?;
        let kwdefaults = self.pairs(f.kwdefaults.clone())?;
        let closure = match &f.closure {
            Some(env) => Some(self.env_ref(env)),
            None => None,
        };
        let globals = self.deferred(address(&f.globals), || Pending::Globals(f.globals.clone()));
        let attrs: Vec<(String, Value)> = f.attrs.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let attrs = self.pairs(attrs)?;
        Ok(FunctionImage { code, defaults, kwdefaults, closure, globals, attrs })
    }

    fn code(&mut self, c: &Code) -> Result<CodeImage, ImageError> {
        let consts = self.items(&c.consts)?;
        let cpy = match &c.cpy {
            Some(e) => Some(Box::new(self.emitted(e)?)),
            None => None,
        };
        let mut functions = Vec::with_capacity(c.functions.len());
        for f in &c.functions {
            functions.push(self.deferred(address(f), || Pending::Code(f.clone())));
        }
        Ok(CodeImage {
            ops: c.ops.clone(),
            lines: c.lines.clone(),
            spans: c.spans.clone(),
            consts,
            names: names_of(&c.names),
            name: c.name.clone(),
            qualname: c.qualname.clone(),
            params: names_of(&c.params),
            posonly: c.posonly,
            vararg: c.vararg.as_ref().map(|s| s.to_string()),
            kwonly: names_of(&c.kwonly),
            kwarg: c.kwarg.as_ref().map(|s| s.to_string()),
            varnames: names_of(&c.varnames),
            cellvars: names_of(&c.cellvars),
            freevars: names_of(&c.freevars),
            is_function: c.is_function,
            is_class: c.is_class,
            is_generator: c.is_generator,
            is_async: c.is_async,
            functions,
            filename: c.filename.clone(),
            doc: c.doc.clone(),
            first_line: c.first_line,
            uses_class_cell: c.uses_class_cell,
            internal: c.internal,
            type_params_role: c.type_params_role,
            future_flags: c.future_flags,
            cpy,
        })
    }

    fn emitted(&mut self, e: &crate::cpybc::Emitted) -> Result<EmittedImage, ImageError> {
        Ok(EmittedImage {
            code: e.code.clone(),
            consts: self.items(&e.consts)?,
            names: names_of(&e.names),
            linetable: e.linetable.clone(),
            exceptiontable: e.exceptiontable.clone(),
            stacksize: e.stacksize,
            first_line: e.first_line,
            synthetic: e.synthetic,
        })
    }

    fn env(&mut self, e: &Rc<Env>) -> Result<EnvImage, ImageError> {
        let vars: Vec<(String, Value)> = e.vars.borrow().iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let vars = self.pairs(vars)?;
        let parent = match &e.parent {
            Some(p) => Some(self.env_ref(p)),
            None => None,
        };
        Ok(EnvImage {
            vars,
            order: e.order.borrow().clone(),
            parent,
            is_class: e.is_class,
            is_module: e.is_module,
            finished: e.finished.get(),
        })
    }

    fn class(&mut self, c: &Rc<ClassObj>) -> Result<ClassImage, ImageError> {
        let mut bases = Vec::with_capacity(c.bases.len());
        for b in &c.bases {
            bases.push(self.node_index(&Value::Class(b.clone()))?);
        }
        let meta = match &c.meta {
            Some(m) => Some(self.node_index(&Value::Class(m.clone()))?),
            None => None,
        };
        let entries: Vec<(String, Value)> = c.dict.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let dict = self.pairs(entries)?;
        let sub_addrs: Vec<usize> = c.subclasses.borrow().iter().filter_map(|w| w.upgrade()).map(|s| address(&s)).collect();
        Ok(ClassImage {
            name: c.name.clone(),
            qualname: c.qualname.clone(),
            bases,
            builtin_base: c.builtin_base,
            data_base: c.data_base,
            meta,
            is_meta: c.is_meta,
            dict,
            sub_addrs,
            subclasses: Vec::new(),
        })
    }

    fn instance(&mut self, i: &Rc<InstanceObj>) -> Result<InstanceImage, ImageError> {
        let class = self.node_index(&Value::Class(i.class()))?;
        let entries: Vec<(String, Value)> = i.dict.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let dict = self.pairs(entries)?;
        let view = i.view.borrow().clone();
        let view = match view {
            Some(rc) => Some(self.item(&Value::Dict(rc))?),
            None => None,
        };
        let payload = i.payload.borrow().clone();
        let payload = self.opt_item(payload.as_ref())?;
        Ok(InstanceImage { class, dict, view, payload, finalized: i.finalized.get() })
    }

    fn exception(&mut self, e: &Rc<ExcObj>) -> Result<ExcImage, ImageError> {
        let args = self.items(&e.args)?;
        let traceback = e.traceback.borrow().clone();
        let traceback = self.opt_item(traceback.as_ref())?;
        let chain: ExcChain = e.chain.borrow().clone();
        let cause = self.opt_item(chain.cause.as_ref())?;
        let context = self.opt_item(chain.context.as_ref())?;
        let extra_values: Vec<(&'static str, Value)> = e.extra.borrow().clone();
        let mut extra = Vec::with_capacity(extra_values.len());
        for (k, v) in &extra_values {
            extra.push((*k, self.item(v)?));
        }
        Ok(ExcImage { kind: e.kind, args, traceback, cause, context, suppress: chain.suppress, extra })
    }

    fn ext(&mut self, image: ExtImage) -> Result<ExtNode, ImageError> {
        Ok(match image {
            ExtImage::StaticMethod(f) => ExtNode::StaticMethod(self.item(&f)?),
            ExtImage::ClassMethod(f) => ExtNode::ClassMethod(self.item(&f)?),
            ExtImage::Property { get, set, del, doc } => ExtNode::Property {
                get: self.item(&get)?,
                set: self.opt_item(set.as_ref())?,
                del: self.opt_item(del.as_ref())?,
                doc: self.item(&doc)?,
            },
            ExtImage::PropertyCopy { obj, which } => ExtNode::PropertyCopy { obj: self.item(&obj)?, which },
            ExtImage::PlainObject => ExtNode::PlainObject,
            ExtImage::ClassCell(env) => ExtNode::ClassCell(self.env_ref(&env)),
            ExtImage::Lazy(parts) => ExtNode::Lazy(self.lazy(parts)?),
            ExtImage::Generator { core, role } => {
                let core_index = self.deferred(address(&core), || Pending::Core(core.clone()));
                ExtNode::Generator { core: core_index, role: self.role(role)? }
            }
            ExtImage::AsyncGenWrapped(v) => ExtNode::AsyncGenWrapped(self.item(&v)?),
            ExtImage::WeakRef { target, callback, hash } => ExtNode::WeakRef {
                target: self.opt_item(target.as_ref())?,
                callback: self.opt_item(callback.as_ref())?,
                hash,
            },
            ExtImage::Opaque(OpaqueImage { tag, state, refs }) => ExtNode::Opaque { tag, state, refs: self.items(&refs)? },
            ExtImage::CodeObject { name, filename, code } => {
                ExtNode::CodeObject { name, filename: filename.to_string(), code: code.as_ref().map(|c| self.code_ref(c)) }
            }
            ExtImage::CodeSource { src, filename, code } => ExtNode::CodeSource { src, filename, code: self.code_ref(&code) },
            ExtImage::Frame(p) => ExtNode::Frame(Box::new(FrameObjNode {
                line: p.line,
                name: p.name,
                file: p.file.to_string(),
                code: p.code.as_ref().map(|c| self.code_ref(c)),
                code_object: self.item(&p.code_object)?,
                env: p.env.as_ref().map(|e| self.env_ref(e)),
                held: p.held.as_ref().map(|e| self.env_ref(e)),
                back: self.item(&p.back)?,
                live: p.live,
                trace: self.item(&p.trace)?,
                trace_lines: p.trace_lines,
                trace_opcodes: p.trace_opcodes,
                last_line: p.last_line,
            })),
        })
    }

    fn iter(&mut self, parts: IterParts) -> Result<IterNode, ImageError> {
        Ok(match parts {
            IterParts::List(v, i) => IterNode::List(self.item(&v)?, i),
            IterParts::Tuple(v, i) => IterNode::Tuple(self.item(&v)?, i),
            IterParts::Str(v, i) => IterNode::Str(self.item(&v)?, i),
            IterParts::Range { next, step, remaining } => IterNode::Range { next, step, remaining },
            IterParts::Items(items, i) => IterNode::Items(self.items(&items)?, i),
            IterParts::Native(v) => IterNode::Native(self.item(&v)?),
            IterParts::Ext(v) => IterNode::Ext(self.item(&v)?),
            IterParts::Inst(v) => IterNode::Inst(self.item(&v)?),
        })
    }

    fn iters(&mut self, list: Vec<IterParts>) -> Result<Vec<IterNode>, ImageError> {
        list.into_iter().map(|p| self.iter(p)).collect()
    }

    fn lazy(&mut self, parts: LazyParts) -> Result<LazyNode, ImageError> {
        Ok(match parts {
            LazyParts::Seq { kind, items, pos } => LazyNode::Seq { kind, items: self.items(&items)?, pos },
            LazyParts::Reversed { kind, items, next } => LazyNode::Reversed { kind, items: self.items(&items)?, next },
            LazyParts::Map { func, iters } => LazyNode::Map { func: self.item(&func)?, iters: self.iters(iters)? },
            LazyParts::Filter { func, iter } => LazyNode::Filter { func: self.item(&func)?, iter: self.iter(iter)? },
            LazyParts::Zip { iters, strict } => LazyNode::Zip { iters: self.iters(iters)?, strict },
            LazyParts::Enumerate { iter, next_index } => LazyNode::Enumerate { iter: self.iter(iter)?, next_index },
            LazyParts::CallIter { func, sentinel, done } => {
                LazyNode::CallIter { func: self.item(&func)?, sentinel: self.item(&sentinel)?, done }
            }
            LazyParts::Boxed { iter } => LazyNode::Boxed { iter: self.iter(iter)? },
            LazyParts::OldSeq { obj, index, done } => LazyNode::OldSeq { obj: self.item(&obj)?, index, done },
        })
    }

    /// O quadro como dado: os valores da pilha, os iteradores de `for` e as exceções em tratamento.
    fn frame(&mut self, f: &Frame) -> Result<FrameNode, ImageError> {
        let code = self.deferred(address(&f.code), || Pending::Code(f.code.clone()));
        let env = self.env_ref(&f.env);
        let mut stack = Vec::with_capacity(f.stack.len());
        for slot in &f.stack {
            stack.push(match slot {
                Slot::Val(v) => SlotNode::Val(self.item(v)?),
                Slot::Iter(it) => SlotNode::Iter(self.iter(crate::lazy::iter_parts(it))?),
            });
        }
        Ok(FrameNode {
            code,
            env,
            stack,
            blocks: f.blocks.iter().map(|b| (b.handler, b.depth, b.handled)).collect(),
            pc: f.pc,
            handled: self.items(&f.handled)?,
            handled_base: f.handled_base,
        })
    }

    fn code_ref(&mut self, code: &Rc<Code>) -> u32 {
        self.deferred(address(code), || Pending::Code(code.clone()))
    }

    /// Um chamado como dado: o quadro e o fechamento da chamada.
    fn callee(&mut self, c: &Callee) -> Result<CalleeNode, ImageError> {
        let link: &CallLink = &c.link;
        let func = match &link.func {
            Some(f) => Some(self.node_index(&Value::Function(f.clone()))?),
            None => None,
        };
        let caller_globals = link.caller_globals.as_ref().map(|g| self.deferred(address(g), || Pending::Globals(g.clone())));
        let instance = self.opt_item(link.instance.as_ref())?;
        let then = match &link.then {
            Some(d) => Some(self.dunder(d)?),
            None => None,
        };
        let resuming = match &link.resuming {
            Some(r) => Some(self.resuming(r)?),
            None => None,
        };
        Ok(CalleeNode {
            frame: self.frame(&c.frame)?,
            link: LinkNode {
                func,
                caller_line: link.caller_line,
                handled_len: link.handled_len,
                profiled: link.profiled,
                caller_globals,
                instance,
                on_stop: link.on_stop,
                then,
                resuming,
            },
        })
    }

    fn resuming(&mut self, r: &Resuming) -> Result<ResumingNode, ImageError> {
        let tail = &r.tail;
        let core = self.deferred(address(&tail.core), || Pending::Core(tail.core.clone()));
        let caller_globals = tail.caller_globals.as_ref().map(|g| self.deferred(address(g), || Pending::Globals(g.clone())));
        let use_ = self.use_node(&r.use_)?;
        Ok(ResumingNode { core, base: tail.base, caller_line: tail.caller_line, caller_globals, use_ })
    }

    fn use_node(&mut self, use_: &ResumeUse) -> Result<UseNode, ImageError> {
        Ok(match use_ {
            ResumeUse::ForIter { exit } => UseNode::ForIter { exit: *exit },
            ResumeUse::Next { default } => UseNode::Next { default: self.opt_item(default.as_ref())? },
            ResumeUse::Delegate { end } => UseNode::Delegate { end: *end },
            ResumeUse::DelegateThrow { end } => UseNode::DelegateThrow { end: *end },
            ResumeUse::DelegateExit(exit) => UseNode::DelegateExit(self.item(&exit.to_value())?),
            ResumeUse::Close => UseNode::Close,
            ResumeUse::Collect(c) => {
                let mut kwargs = Pairs::with_capacity(c.kwargs.len());
                for (name, value) in &c.kwargs {
                    kwargs.push((name.clone(), self.item(value)?));
                }
                let fold = match &c.fold {
                    Some(f) => Some(self.fold(f)?),
                    None => None,
                };
                UseNode::Collect { items: self.items(&c.items)?, call: self.item(&c.call)?, kwargs, fold }
            }
            ResumeUse::Pull(p) => UseNode::Pull(self.pull_node(p)?),
        })
    }

    fn pull_node(&mut self, p: &Pull) -> Result<PullNode, ImageError> {
        let mut layers = Vec::with_capacity(p.layers.len());
        for layer in &p.layers {
            layers.push(self.layer(layer)?);
        }
        Ok(PullNode { root: self.item(&p.root)?, layers, then: Box::new(self.use_node(&p.then)?) })
    }

    fn callback_node(&mut self, s: &Callback) -> Result<CallbackNode, ImageError> {
        let what = match &s.what {
            CallbackKind::Mapped => WhatNode::Mapped,
            CallbackKind::Kept { it, item } => {
                WhatNode::Kept { it: self.item(&Value::Ext(it.clone()))?, item: self.item(item)? }
            }
            CallbackKind::Keyed { item } => WhatNode::Keyed { item: self.item(item)? },
            CallbackKind::Advance { leaf } => WhatNode::Advance { leaf: self.item(&Value::Ext(leaf.clone()))? },
            CallbackKind::Start => WhatNode::Start,
        };
        Ok(CallbackNode { pull: self.pull_node(&s.pull)?, what })
    }

    fn fold(&mut self, f: &Fold) -> Result<FoldNode, ImageError> {
        Ok(match f {
            Fold::Sum { acc, phase } => FoldNode::Sum { acc: self.item(acc)?, phase: *phase },
            Fold::Set { target, frozen } => FoldNode::Set { target: self.item(target)?, frozen: *frozen },
            Fold::Dict { target, kwargs, index } => {
                FoldNode::Dict { target: self.item(target)?, kwargs: self.pairs(kwargs.clone())?, index: *index }
            }
            Fold::MinMax { max, key, default, best } => FoldNode::MinMax {
                max: *max,
                key: self.opt_item(key.as_ref())?,
                default: self.opt_item(default.as_ref())?,
                best: match best {
                    Some((k, v)) => Some((self.item(k)?, self.item(v)?)),
                    None => None,
                },
            },
            Fold::Any => FoldNode::Any,
            Fold::All => FoldNode::All,
            Fold::Extend { target } => FoldNode::Extend { target: self.item(target)? },
            Fold::Sorted { key, reverse, items, pairs } => {
                let mut kept = Vec::with_capacity(pairs.len());
                for (k, v) in pairs {
                    kept.push((self.item(k)?, self.item(v)?));
                }
                FoldNode::Sorted { key: self.opt_item(key.as_ref())?, reverse: *reverse, items: self.items(items)?, pairs: kept }
            }
        })
    }

    fn layer(&mut self, layer: &Layer) -> Result<LayerNode, ImageError> {
        Ok(match layer {
            Layer::Enumerate(it) => LayerNode::Enumerate(self.item(&Value::Ext(it.clone()))?),
            Layer::Filter(it) => LayerNode::Filter(self.item(&Value::Ext(it.clone()))?),
            Layer::Many { it, at, got } => {
                LayerNode::Many { it: self.item(&Value::Ext(it.clone()))?, at: *at, got: self.items(got)? }
            }
            Layer::Check { it, at } => LayerNode::Check { it: self.item(&Value::Ext(it.clone()))?, at: *at },
        })
    }

    fn dunder(&mut self, d: &Dunder) -> Result<DunderNode, ImageError> {
        Ok(match d {
            Dunder::Chain(c) => {
                let mut rest = Vec::with_capacity(c.rest.len());
                for a in &c.rest {
                    rest.push(AttemptNode { recv: self.item(&a.recv)?, name: a.name, other: self.item(&a.other)?, invert: a.invert });
                }
                DunderNode::Chain(ChainNode { kind: c.kind, a: self.item(&c.a)?, b: self.item(&c.b)?, rest, invert: c.invert })
            }
            Dunder::Push => DunderNode::Push,
            Dunder::Discard => DunderNode::Discard,
            Dunder::Boolean { negate } => DunderNode::Boolean { negate: *negate },
            Dunder::Truth { how, len } => DunderNode::Truth { how: *how, len: *len },
            Dunder::Iter => DunderNode::Iter,
            Dunder::Callback(s) => DunderNode::Callback(Box::new(self.callback_node(s)?)),
            Dunder::Signal => DunderNode::Signal,
            Dunder::Import(run) => DunderNode::Import(Box::new(ImportNode {
                name: run.name.clone(),
                module: self.item(&Value::Module(run.module.clone()))?,
                plan: run.plan.as_ref().map(|p| (p.rest.clone(), p.result.clone())),
            })),
            Dunder::Exec(run) => {
                let globals = self.deferred(address(&run.globals), || Pending::Globals(run.globals.clone()));
                let ns = match &run.ns {
                    Some(ns) => Some(NamespacesNode {
                        gdict: self.item(&ns.gdict)?,
                        was: ns.was.clone(),
                        lwas: ns.lwas.clone(),
                        separate: self.opt_item(ns.separate.as_ref())?,
                        backup: ns.backup.as_ref().map(|b| self.deferred(address(b), || Pending::Globals(b.clone()))),
                    }),
                    None => None,
                };
                let back = match &run.back {
                    Some(b) => {
                        let mut before = Vec::with_capacity(b.before.len());
                        for (k, v) in &b.before {
                            before.push((self.item(k)?, self.item(v)?));
                        }
                        Some((self.item(&b.mapping)?, self.item(&b.snapshot)?, before))
                    }
                    None => None,
                };
                DunderNode::Exec(Box::new(ExecNode { eval: run.eval, globals, ns, back }))
            }
        })
    }

    fn core(&mut self, c: &Rc<GenCore>) -> Result<CoreNode, ImageError> {
        let globals_rc = c.globals();
        let globals = self.deferred(address(&globals_rc), || Pending::Globals(globals_rc.clone()));
        let (frame, flags, returned) = c.inspect(|frame, flags, returned| -> Result<_, ImageError> {
            Ok((self.frame(frame)?, flags, self.opt_item(returned)?))
        })?;
        Ok(CoreNode { kind: c.kind(), globals, frame, flags, returned })
    }

    fn role(&mut self, role: GenRole) -> Result<RoleNode, ImageError> {
        Ok(match role {
            GenRole::Object => RoleNode::Object,
            GenRole::CoroWrapper => RoleNode::CoroWrapper,
            GenRole::Await { mode, started, done, closing } => {
                let mode = match mode {
                    Some(AwaitMode::Send(v)) => Some(ModeNode::Send(self.item(&v)?)),
                    Some(AwaitMode::Throw(v)) => Some(ModeNode::Throw(self.item(&v)?)),
                    Some(AwaitMode::Close) => Some(ModeNode::Close),
                    None => None,
                };
                RoleNode::Await { mode, started, done, closing }
            }
        })
    }

    fn traceback(&mut self, entries: Vec<TbEntry>, filename: &str) -> ExtNode {
        let entries = entries
            .into_iter()
            .map(|(line, name, file, span, held)| TbNode {
                line,
                name,
                file: file.to_string(),
                span,
                held: held.map(|h| {
                    let env = self.env_ref(&h.env);
                    let code = h.code.clone();
                    (env, self.deferred(address(&code), || Pending::Code(code.clone())))
                }),
            })
            .collect();
        ExtNode::Traceback { entries, filename: filename.to_string() }
    }

    fn native(&mut self, n: &Native) -> Result<NativeNode, ImageError> {
        Ok(match n {
            Native::File(f) => NativeNode::File(FileNode {
                kind: f.kind,
                lines: f.lines.clone(),
                pos: f.pos,
                loaded: f.loaded,
                closed: f.closed,
                name: f.name.clone(),
                raw: f.raw.clone(),
                raw_eof: f.raw_eof,
            }),
            Native::CsvReader { reader, src } => NativeNode::CsvReader { reader: reader.clone(), src: self.item(src)? },
            Native::CsvWriter { dialect, target } => {
                NativeNode::CsvWriter { dialect: dialect.clone(), target: self.item(target)? }
            }
        })
    }

    /// Troca os endereços das subclasses pelos nós que a imagem tem; as que ficaram de fora somem.
    fn finish(&mut self) {
        let seen = &self.seen;
        for node in &mut self.nodes {
            if let Node::Class(c) = node {
                let addresses = std::mem::take(&mut c.sub_addrs);
                c.subclasses = addresses.into_iter().filter_map(|a| seen.get(&a).copied()).collect();
            }
        }
    }
}

/// O que a reconstrução já refez, por índice de nó.
struct Rebuilt {
    values: Vec<Option<Value>>,
    aux: Vec<Option<Aux>>,
    /// Nomes internados: o mesmo texto vira o mesmo `Rc<str>`.
    names: RefCell<HashMap<String, Rc<str>>>,
    /// A `Vm` que os geradores refeitos compartilham (cópia dela com as globais do módulo de cada um).
    shell: Option<Vm>,
}

/// Os nós que não são `Value`.
#[derive(Clone)]
enum Aux {
    Code(Rc<Code>),
    Env(Rc<Env>),
    Globals(Rc<RefCell<VarMap>>),
    Core(Rc<GenCore>),
    /// Um quadro de raiz (`Node::Frame`, `Node::Callee`): refeito à parte, por `Rebuilt::frame`.
    Marker,
}

/// O resultado de refazer um nó da segunda passada.
enum Made {
    Value(Value),
    Aux(Aux),
}

impl Rebuilt {
    fn built(&self, index: usize) -> bool {
        self.values[index].is_some() || self.aux[index].is_some()
    }

    fn value(&self, item: Item) -> Result<Value, ImageError> {
        Ok(match item {
            Item::None => Value::None,
            Item::Bool(b) => Value::Bool(b),
            Item::Int(n) => Value::Int(n),
            Item::Float(f) => Value::Float(f),
            Item::Range(start, stop, step) => Value::Range(Range { start, stop, step }),
            Item::Builtin(name) => Value::Builtin(name),
            Item::Node(i) => self
                .values
                .get(i as usize)
                .and_then(Clone::clone)
                .ok_or(ImageError::Malformed("reference to a node not built yet"))?,
        })
    }

    fn values_of(&self, items: &[Item]) -> Result<Vec<Value>, ImageError> {
        items.iter().map(|i| self.value(*i)).collect()
    }

    fn opt(&self, item: &Option<Item>) -> Result<Option<Value>, ImageError> {
        match item {
            Some(i) => Ok(Some(self.value(*i)?)),
            None => Ok(None),
        }
    }

    fn pairs(&self, entries: &[(String, Item)]) -> Result<Vec<(String, Value)>, ImageError> {
        let mut out = Vec::with_capacity(entries.len());
        for (k, v) in entries {
            out.push((k.clone(), self.value(*v)?));
        }
        Ok(out)
    }

    fn name(&self, text: &str) -> Rc<str> {
        let mut names = self.names.borrow_mut();
        if let Some(rc) = names.get(text) {
            return rc.clone();
        }
        let rc: Rc<str> = Rc::from(text);
        names.insert(text.to_string(), rc.clone());
        rc
    }

    fn core(&self, index: u32) -> Result<Rc<GenCore>, ImageError> {
        match self.aux.get(index as usize) {
            Some(Some(Aux::Core(c))) => Ok(c.clone()),
            _ => Err(ImageError::Malformed("generator reference is not a built core")),
        }
    }

    fn iter(&self, node: &IterNode) -> Result<IterParts, ImageError> {
        Ok(match node {
            IterNode::List(i, pos) => IterParts::List(self.value(*i)?, *pos),
            IterNode::Tuple(i, pos) => IterParts::Tuple(self.value(*i)?, *pos),
            IterNode::Str(i, pos) => IterParts::Str(self.value(*i)?, *pos),
            IterNode::Range { next, step, remaining } => IterParts::Range { next: *next, step: *step, remaining: *remaining },
            IterNode::Items(items, pos) => IterParts::Items(self.values_of(items)?, *pos),
            IterNode::Native(i) => IterParts::Native(self.value(*i)?),
            IterNode::Ext(i) => IterParts::Ext(self.value(*i)?),
            IterNode::Inst(i) => IterParts::Inst(self.value(*i)?),
        })
    }

    fn py_iter(&self, node: &IterNode) -> Result<crate::vm::PyIter, ImageError> {
        crate::lazy::iter_from_parts(self.iter(node)?).ok_or(ImageError::Malformed("iterator holds a value of the wrong type"))
    }

    fn iters(&self, nodes: &[IterNode]) -> Result<Vec<IterParts>, ImageError> {
        nodes.iter().map(|n| self.iter(n)).collect()
    }

    /// A pilha de valores de um quadro: os valores e os iteradores de `for`.
    fn slots(&self, stack: &[SlotNode]) -> Result<Vec<Slot>, ImageError> {
        stack
            .iter()
            .map(|slot| match slot {
                SlotNode::Val(i) => Ok(Slot::Val(self.value(*i)?)),
                SlotNode::Iter(it) => Ok(Slot::Iter(self.py_iter(it)?)),
            })
            .collect()
    }

    fn blocks(blocks: &[(usize, usize, usize)]) -> Vec<Block> {
        blocks.iter().map(|&(handler, depth, handled)| Block { handler, depth, handled }).collect()
    }

    /// O `Frame` de um quadro como dado, com o código e o escopo que já existem.
    fn frame(&self, node: &FrameNode) -> Result<Frame, ImageError> {
        let mut frame = Frame::new(self.code(node.code)?, self.env(node.env)?);
        frame.stack = self.slots(&node.stack)?;
        frame.blocks = Rebuilt::blocks(&node.blocks);
        frame.pc = node.pc;
        frame.handled = self.values_of(&node.handled)?;
        frame.handled_base = node.handled_base;
        Ok(frame)
    }

    /// O chamado de `frames_stack` (ou o em execução) como dado.
    fn callee(&self, node: &CalleeNode) -> Result<Callee, ImageError> {
        let link = &node.link;
        let then = match &link.then {
            Some(d) => Some(self.dunder(d)?),
            None => None,
        };
        let caller_globals = match link.caller_globals {
            Some(g) => Some(self.globals(g)?),
            None => None,
        };
        let func = match link.func {
            Some(f) => Some(self.function(f)?),
            None => None,
        };
        let resuming = match &link.resuming {
            Some(r) => Some(self.resuming(r)?),
            None => None,
        };
        let frame = self.frame(&node.frame)?;
        // O quadro de `exec`/`eval` volta a ser o `f_globals` das globais em que roda (registro por thread).
        if let Some(Dunder::Exec(run)) = &then {
            crate::frameobj::bind_globals(&frame.env, &run.globals);
        }
        Ok(Callee {
            frame,
            link: CallLink {
                func,
                caller_line: link.caller_line,
                handled_len: link.handled_len,
                profiled: link.profiled,
                caller_globals,
                instance: self.opt(&link.instance)?,
                on_stop: link.on_stop,
                then,
                resuming,
            },
        })
    }

    fn resuming(&self, node: &ResumingNode) -> Result<Resuming, ImageError> {
        let caller_globals = match node.caller_globals {
            Some(g) => Some(self.globals(g)?),
            None => None,
        };
        let use_ = self.use_of(&node.use_)?;
        Ok(Resuming { tail: Tail { core: self.core(node.core)?, base: node.base, caller_line: node.caller_line, caller_globals }, use_, inject: None })
    }

    fn use_of(&self, node: &UseNode) -> Result<ResumeUse, ImageError> {
        Ok(match node {
            UseNode::ForIter { exit } => ResumeUse::ForIter { exit: *exit },
            UseNode::Next { default } => ResumeUse::Next { default: self.opt(default)? },
            UseNode::Delegate { end } => ResumeUse::Delegate { end: *end },
            UseNode::DelegateThrow { end } => ResumeUse::DelegateThrow { end: *end },
            UseNode::DelegateExit(exit) => ResumeUse::DelegateExit(crate::vm::PyException::from_value(&self.value(*exit)?)),
            UseNode::Close => ResumeUse::Close,
            UseNode::Collect { items, call, kwargs, fold } => {
                let fold = match fold {
                    Some(f) => Some(self.fold_of(f)?),
                    None => None,
                };
                ResumeUse::Collect(Box::new(Collect {
                    items: self.values_of(items)?,
                    call: self.value(*call)?,
                    kwargs: self.pairs(kwargs)?,
                    fold,
                }))
            }
            UseNode::Pull(p) => ResumeUse::Pull(Box::new(self.pull_of(p)?)),
        })
    }

    fn pull_of(&self, node: &PullNode) -> Result<Pull, ImageError> {
        let mut layers = Vec::with_capacity(node.layers.len());
        for layer in &node.layers {
            layers.push(self.layer_of(layer)?);
        }
        Ok(Pull { root: self.value(node.root)?, layers, then: self.use_of(&node.then)? })
    }

    fn callback_of(&self, node: &CallbackNode) -> Result<Callback, ImageError> {
        let object = |item: Item| -> Result<Rc<dyn crate::object::ExtObject>, ImageError> {
            match self.value(item)? {
                Value::Ext(e) => Ok(e),
                _ => Err(ImageError::Malformed("a callback step that points at a non-object")),
            }
        };
        let what = match &node.what {
            WhatNode::Mapped => CallbackKind::Mapped,
            WhatNode::Kept { it, item } => CallbackKind::Kept { it: object(*it)?, item: self.value(*item)? },
            WhatNode::Keyed { item } => CallbackKind::Keyed { item: self.value(*item)? },
            WhatNode::Advance { leaf } => CallbackKind::Advance { leaf: object(*leaf)? },
            WhatNode::Start => CallbackKind::Start,
        };
        Ok(Callback { pull: self.pull_of(&node.pull)?, what })
    }

    fn fold_of(&self, node: &FoldNode) -> Result<Fold, ImageError> {
        Ok(match node {
            FoldNode::Sum { acc, phase } => Fold::Sum { acc: self.value(*acc)?, phase: *phase },
            FoldNode::Set { target, frozen } => Fold::Set { target: self.value(*target)?, frozen: *frozen },
            FoldNode::Dict { target, kwargs, index } => {
                Fold::Dict { target: self.value(*target)?, kwargs: self.pairs(kwargs)?, index: *index }
            }
            FoldNode::MinMax { max, key, default, best } => Fold::MinMax {
                max: *max,
                key: self.opt(key)?,
                default: self.opt(default)?,
                best: match best {
                    Some((k, v)) => Some((self.value(*k)?, self.value(*v)?)),
                    None => None,
                },
            },
            FoldNode::Any => Fold::Any,
            FoldNode::All => Fold::All,
            FoldNode::Extend { target } => Fold::Extend { target: self.value(*target)? },
            FoldNode::Sorted { key, reverse, items, pairs } => {
                let mut kept = Vec::with_capacity(pairs.len());
                for (k, v) in pairs {
                    kept.push((self.value(*k)?, self.value(*v)?));
                }
                Fold::Sorted { key: self.opt(key)?, reverse: *reverse, items: self.values_of(items)?, pairs: kept }
            }
        })
    }

    fn layer_of(&self, node: &LayerNode) -> Result<Layer, ImageError> {
        let object = |item: Item| -> Result<Rc<dyn crate::object::ExtObject>, ImageError> {
            match self.value(item)? {
                Value::Ext(e) => Ok(e),
                _ => Err(ImageError::Malformed("a lazy layer that is not an object")),
            }
        };
        Ok(match node {
            LayerNode::Enumerate(it) => Layer::Enumerate(object(*it)?),
            LayerNode::Filter(it) => Layer::Filter(object(*it)?),
            LayerNode::Many { it, at, got } => Layer::Many { it: object(*it)?, at: *at, got: self.values_of(got)? },
            LayerNode::Check { it, at } => Layer::Check { it: object(*it)?, at: *at },
        })
    }

    fn dunder(&self, d: &DunderNode) -> Result<Dunder, ImageError> {
        Ok(match d {
            DunderNode::Chain(c) => {
                let mut rest = Vec::with_capacity(c.rest.len());
                for a in &c.rest {
                    rest.push(Attempt { recv: self.value(a.recv)?, name: a.name, other: self.value(a.other)?, invert: a.invert });
                }
                Dunder::Chain(Chain { kind: c.kind, a: self.value(c.a)?, b: self.value(c.b)?, rest, invert: c.invert })
            }
            DunderNode::Push => Dunder::Push,
            DunderNode::Discard => Dunder::Discard,
            DunderNode::Boolean { negate } => Dunder::Boolean { negate: *negate },
            DunderNode::Truth { how, len } => Dunder::Truth { how: *how, len: *len },
            DunderNode::Iter => Dunder::Iter,
            DunderNode::Callback(s) => Dunder::Callback(Box::new(self.callback_of(s)?)),
            DunderNode::Signal => Dunder::Signal,
            DunderNode::Import(n) => {
                let Value::Module(module) = self.value(n.module)? else {
                    return Err(ImageError::Malformed("an import step that points at a non-module"));
                };
                Dunder::Import(Box::new(crate::modrun::ImportRun {
                    name: n.name.clone(),
                    guard: crate::modules::Initializing::enter(module.name),
                    module,
                    plan: n.plan.as_ref().map(|(rest, result)| crate::modrun::ImportPlan { rest: rest.clone(), result: result.clone() }),
                }))
            }
            DunderNode::Exec(n) => {
                let ns = match &n.ns {
                    Some(ns) => Some(crate::builtins_ext::Namespaces {
                        gdict: self.value(ns.gdict)?,
                        was: ns.was.clone(),
                        lwas: ns.lwas.clone(),
                        separate: self.opt(&ns.separate)?,
                        backup: match ns.backup {
                            Some(b) => Some(self.globals(b)?),
                            None => None,
                        },
                    }),
                    None => None,
                };
                let back = match &n.back {
                    Some((mapping, snapshot, before)) => {
                        let mut pairs = Vec::with_capacity(before.len());
                        for (k, v) in before {
                            pairs.push((self.value(*k)?, self.value(*v)?));
                        }
                        Some(crate::builtins_ext::MappingBack { mapping: self.value(*mapping)?, snapshot: self.value(*snapshot)?, before: pairs })
                    }
                    None => None,
                };
                Dunder::Exec(Box::new(crate::builtins_ext::ExecRun { eval: n.eval, globals: self.globals(n.globals)?, ns, back }))
            }
        })
    }

    fn lazy(&self, node: &LazyNode) -> Result<LazyParts, ImageError> {
        Ok(match node {
            LazyNode::Seq { kind, items, pos } => LazyParts::Seq { kind: *kind, items: self.values_of(items)?, pos: *pos },
            LazyNode::Reversed { kind, items, next } => {
                LazyParts::Reversed { kind: *kind, items: self.values_of(items)?, next: *next }
            }
            LazyNode::Map { func, iters } => LazyParts::Map { func: self.value(*func)?, iters: self.iters(iters)? },
            LazyNode::Filter { func, iter } => LazyParts::Filter { func: self.value(*func)?, iter: self.iter(iter)? },
            LazyNode::Zip { iters, strict } => LazyParts::Zip { iters: self.iters(iters)?, strict: *strict },
            LazyNode::Enumerate { iter, next_index } => LazyParts::Enumerate { iter: self.iter(iter)?, next_index: *next_index },
            LazyNode::CallIter { func, sentinel, done } => {
                LazyParts::CallIter { func: self.value(*func)?, sentinel: self.value(*sentinel)?, done: *done }
            }
            LazyNode::Boxed { iter } => LazyParts::Boxed { iter: self.iter(iter)? },
            LazyNode::OldSeq { obj, index, done } => LazyParts::OldSeq { obj: self.value(*obj)?, index: *index, done: *done },
        })
    }

    fn role(&self, node: &RoleNode) -> Result<GenRole, ImageError> {
        Ok(match node {
            RoleNode::Object => GenRole::Object,
            RoleNode::CoroWrapper => GenRole::CoroWrapper,
            RoleNode::Await { mode, started, done, closing } => {
                let mode = match mode {
                    Some(ModeNode::Send(i)) => Some(AwaitMode::Send(self.value(*i)?)),
                    Some(ModeNode::Throw(i)) => Some(AwaitMode::Throw(self.value(*i)?)),
                    Some(ModeNode::Close) => Some(AwaitMode::Close),
                    None => None,
                };
                GenRole::Await { mode, started: *started, done: *done, closing: *closing }
            }
        })
    }

    fn class(&self, index: u32) -> Result<Rc<ClassObj>, ImageError> {
        match self.values.get(index as usize) {
            Some(Some(Value::Class(c))) => Ok(c.clone()),
            _ => Err(ImageError::Malformed("class reference is not a built class")),
        }
    }

    fn function(&self, index: u32) -> Result<Rc<FuncObj>, ImageError> {
        match self.values.get(index as usize) {
            Some(Some(Value::Function(f))) => Ok(f.clone()),
            _ => Err(ImageError::Malformed("function reference is not a built function")),
        }
    }

    fn code(&self, index: u32) -> Result<Rc<Code>, ImageError> {
        match self.aux.get(index as usize) {
            Some(Some(Aux::Code(c))) => Ok(c.clone()),
            _ => Err(ImageError::Malformed("code reference is not a built code")),
        }
    }

    fn env(&self, index: u32) -> Result<Rc<Env>, ImageError> {
        match self.aux.get(index as usize) {
            Some(Some(Aux::Env(e))) => Ok(e.clone()),
            _ => Err(ImageError::Malformed("env reference is not a built env")),
        }
    }

    fn globals(&self, index: u32) -> Result<Rc<RefCell<VarMap>>, ImageError> {
        match self.aux.get(index as usize) {
            Some(Some(Aux::Globals(g))) => Ok(g.clone()),
            _ => Err(ImageError::Malformed("globals reference is not a built table")),
        }
    }
}

fn push_dep(out: &mut Vec<usize>, item: &Item) {
    if let Item::Node(i) = item {
        out.push(*i as usize);
    }
}

fn push_opt_dep(out: &mut Vec<usize>, item: &Option<Item>) {
    if let Some(item) = item {
        push_dep(out, item);
    }
}

fn iter_deps(it: &IterNode, out: &mut Vec<usize>) {
    match it {
        IterNode::List(i, _) | IterNode::Tuple(i, _) | IterNode::Str(i, _) => push_dep(out, i),
        IterNode::Native(i) | IterNode::Ext(i) | IterNode::Inst(i) => push_dep(out, i),
        IterNode::Items(items, _) => items.iter().for_each(|i| push_dep(out, i)),
        IterNode::Range { .. } => {}
    }
}

fn lazy_deps(lazy: &LazyNode, out: &mut Vec<usize>) {
    match lazy {
        LazyNode::Seq { items, .. } | LazyNode::Reversed { items, .. } => items.iter().for_each(|i| push_dep(out, i)),
        LazyNode::Map { func, iters } => {
            push_dep(out, func);
            iters.iter().for_each(|it| iter_deps(it, out));
        }
        LazyNode::Filter { func, iter } => {
            push_dep(out, func);
            iter_deps(iter, out);
        }
        LazyNode::Zip { iters, .. } => iters.iter().for_each(|it| iter_deps(it, out)),
        LazyNode::Enumerate { iter, .. } => iter_deps(iter, out),
        LazyNode::CallIter { func, sentinel, .. } => {
            push_dep(out, func);
            push_dep(out, sentinel);
        }
        LazyNode::Boxed { iter } => iter_deps(iter, out),
        LazyNode::OldSeq { obj, .. } => push_dep(out, obj),
    }
}

impl HeapImage {
    /// Copia o grafo alcançável a partir de `roots`. As raízes compartilham a mesma arena: um objeto
    /// visto por duas delas vira um nó só.
    pub fn capture(roots: &[Value]) -> Result<HeapImage, ImageError> {
        let roots: Vec<Root> = roots.iter().cloned().map(Root::Value).collect();
        HeapImage::capture_roots(&roots)
    }

    /// Como [`HeapImage::capture`], com raízes que nem sempre são um `Value` (a tabela de globais de um
    /// módulo, um `Code`, um escopo): é o que a imagem da `Vm` precisa.
    pub(crate) fn capture_roots(roots: &[Root]) -> Result<HeapImage, ImageError> {
        let mut enc = Encoder { nodes: Vec::new(), seen: HashMap::new(), work: Vec::new(), ids: Vec::new() };
        let mut items = Vec::with_capacity(roots.len());
        for root in roots {
            items.push(enc.root(root)?);
            enc.drain()?;
        }
        enc.finish();
        Ok(HeapImage { nodes: enc.nodes, roots: items, ids: enc.ids })
    }

    /// Quantos nós (objetos com identidade, mais o `Code`, os escopos e as globais) a imagem tem.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Reconstrói as raízes como um grafo `Rc` novo, na ordem em que foram capturadas. Uma imagem com
    /// gerador precisa de [`HeapImage::restore_in`].
    pub fn restore(&self) -> Result<Vec<Value>, ImageError> {
        self.restore_in(None)
    }

    /// Como [`HeapImage::restore`], com a `Vm` que os geradores refeitos compartilham: cada um recebe uma
    /// cópia dela (as mesmas tabelas) com as globais do módulo onde nasceu.
    pub fn restore_in(&self, shell: Option<&Vm>) -> Result<Vec<Value>, ImageError> {
        let mut rb = self.rebuilt(shell.cloned());
        self.build_shells(&mut rb)?;
        self.complete(&mut rb)?;
        self.roots.iter().map(|r| rb.value(*r)).collect()
    }

    fn rebuilt(&self, shell: Option<Vm>) -> Rebuilt {
        let count = self.nodes.len();
        Rebuilt { values: vec![None; count], aux: vec![None; count], names: RefCell::new(HashMap::new()), shell }
    }

    /// Passadas 2 e 3, depois das cascas (e, na imagem da `Vm`, depois de a `Vm` existir).
    fn complete(&self, rb: &mut Rebuilt) -> Result<(), ImageError> {
        self.build_dependents(rb)?;
        for index in 0..self.nodes.len() {
            self.fill(index, rb)?;
        }
        Ok(())
    }

    /// Passada 1: folhas imutáveis e as cascas dos nós que não dependem de outro nó, para os ciclos fecharem.
    fn build_shells(&self, rb: &mut Rebuilt) -> Result<(), ImageError> {
        for (index, node) in self.nodes.iter().enumerate() {
            match node {
                Node::Pending => return Err(ImageError::Malformed("node never filled")),
                Node::Big(b) => rb.values[index] = Some(Value::Big(Rc::new(b.clone()))),
                Node::Str(s) => rb.values[index] = Some(Value::str(s.clone())),
                Node::Bytes(b) => rb.values[index] = Some(Value::bytes(b.clone())),
                Node::ByteArray(b) => rb.values[index] = Some(Value::bytearray(b.clone())),
                Node::List(_) => rb.values[index] = Some(Value::list(Vec::new())),
                Node::Dict(_) => rb.values[index] = Some(Value::dict(Dict::default())),
                Node::Set(t) if !t.frozen => rb.values[index] = Some(Value::set(Set::new())),
                Node::NativeFn(name, f) => {
                    rb.values[index] = Some(Value::NativeFn(Rc::new(NativeFn { name: *name, f: *f })));
                }
                Node::Globals(_) => {
                    rb.aux[index] = Some(Aux::Globals(Rc::new(RefCell::new(VarMap::default()))));
                }
                Node::Module { name, .. } => {
                    let module = ModuleObj { name: *name, attrs: RefCell::new(BTreeMap::new()) };
                    rb.values[index] = Some(Value::Module(Rc::new(module)));
                }
                // A casca é um arquivo em branco; a terceira passada põe o conteúdo verdadeiro.
                Node::Native(_) => {
                    let blank = PyFile {
                        kind: FileKind::Stdout,
                        lines: Vec::new(),
                        pos: 0,
                        loaded: true,
                        closed: false,
                        name: String::new(),
                        raw: Vec::new(),
                        raw_eof: false,
                    };
                    rb.values[index] = Some(Value::Native(Rc::new(RefCell::new(Native::File(blank)))));
                }
                // Quadros de raiz: ninguém aponta para eles, a `Vm` os refaz com `Rebuilt::frame`.
                Node::Frame(_) | Node::Callee(_) => rb.aux[index] = Some(Aux::Marker),
                Node::Set(_)
                | Node::Tuple(_)
                | Node::Slice(..)
                | Node::Bound { .. }
                | Node::BoundFn { .. }
                | Node::Function(_)
                | Node::Code(_)
                | Node::Env(_)
                | Node::Class(_)
                | Node::Instance(_)
                | Node::Exception(_)
                | Node::Ext(_)
                | Node::GenCore(_) => {}
            }
        }
        Ok(())
    }

    /// Os nós de que `index` depende para existir (campos imutáveis): filhos antes dos pais.
    fn deps(&self, index: usize) -> Vec<usize> {
        let mut out = Vec::new();
        match &self.nodes[index] {
            Node::Tuple(items) => items.iter().for_each(|i| push_dep(&mut out, i)),
            Node::Set(t) if t.frozen => {
                for slot in &t.slots {
                    if let TableSlot::Active(_, item) = slot {
                        push_dep(&mut out, item);
                    }
                }
            }
            Node::Slice(a, b, c) => {
                for item in [a, b, c] {
                    push_dep(&mut out, item);
                }
            }
            Node::Bound { recv, .. } => push_dep(&mut out, recv),
            Node::BoundFn { recv, func } => {
                push_dep(&mut out, recv);
                out.push(*func as usize);
            }
            Node::Function(f) => {
                out.push(f.code as usize);
                f.defaults.iter().for_each(|i| push_dep(&mut out, i));
                f.kwdefaults.iter().for_each(|(_, i)| push_dep(&mut out, i));
                if let Some(env) = f.closure {
                    out.push(env as usize);
                }
            }
            Node::Code(c) => {
                c.consts.iter().for_each(|i| push_dep(&mut out, i));
                if let Some(e) = &c.cpy {
                    e.consts.iter().for_each(|i| push_dep(&mut out, i));
                }
                out.extend(c.functions.iter().map(|f| *f as usize));
            }
            Node::Env(e) => {
                if let Some(parent) = e.parent {
                    out.push(parent as usize);
                }
            }
            Node::Class(c) => {
                out.extend(c.bases.iter().map(|b| *b as usize));
                if let Some(meta) = c.meta {
                    out.push(meta as usize);
                }
            }
            Node::Instance(i) => out.push(i.class as usize),
            Node::Exception(e) => e.args.iter().for_each(|i| push_dep(&mut out, i)),
            Node::Ext(x) => match x {
                ExtNode::StaticMethod(i) | ExtNode::ClassMethod(i) => push_dep(&mut out, i),
                ExtNode::Property { get, set, del, .. } => {
                    push_dep(&mut out, get);
                    push_opt_dep(&mut out, set);
                    push_opt_dep(&mut out, del);
                }
                ExtNode::PropertyCopy { obj, .. } => push_dep(&mut out, obj),
                ExtNode::ClassCell(env) => out.push(*env as usize),
                ExtNode::PlainObject => {}
                ExtNode::Lazy(lazy) => lazy_deps(lazy, &mut out),
                ExtNode::Generator { core, role } => {
                    out.push(*core as usize);
                    if let RoleNode::Await { mode: Some(ModeNode::Send(i) | ModeNode::Throw(i)), .. } = role {
                        push_dep(&mut out, i);
                    }
                }
                ExtNode::AsyncGenWrapped(i) => push_dep(&mut out, i),
                ExtNode::WeakRef { target, callback, .. } => {
                    push_opt_dep(&mut out, target);
                    push_opt_dep(&mut out, callback);
                }
                ExtNode::Opaque { refs, .. } => refs.iter().for_each(|i| push_dep(&mut out, i)),
                ExtNode::Traceback { entries, .. } => {
                    for (env, code) in entries.iter().filter_map(|e| e.held) {
                        out.push(env as usize);
                        out.push(code as usize);
                    }
                }
                ExtNode::CodeObject { code, .. } => out.extend(code.iter().map(|c| *c as usize)),
                ExtNode::CodeSource { code, .. } => out.push(*code as usize),
                // `f_back` e `f_trace` são da terceira passada: podem apontar de volta para o quadro.
                ExtNode::Frame(f) => {
                    out.extend([f.code, f.env, f.held].into_iter().flatten().map(|n| n as usize));
                    push_dep(&mut out, &f.code_object);
                }
            },
            Node::GenCore(c) => {
                out.push(c.frame.code as usize);
                out.push(c.frame.env as usize);
                out.push(c.globals as usize);
            }
            Node::Pending
            | Node::Big(_)
            | Node::Str(_)
            | Node::Bytes(_)
            | Node::ByteArray(_)
            | Node::List(_)
            | Node::Dict(_)
            | Node::Set(_)
            | Node::NativeFn(..)
            | Node::Globals(_)
            | Node::Native(_)
            | Node::Frame(_)
            | Node::Callee(_)
            | Node::Module { .. } => {}
        }
        out
    }

    /// Passada 2: os nós com campo imutável apontando para outro nó, filhos antes dos pais. Um ciclo só
    /// passa por campo mutável, então entre estes o grafo é acíclico; a pilha tem teto para uma imagem
    /// malformada não girar.
    fn build_dependents(&self, rb: &mut Rebuilt) -> Result<(), ImageError> {
        let deps: Vec<Vec<usize>> = (0..self.nodes.len()).map(|i| self.deps(i)).collect();
        let mut budget = self.nodes.len() + 1 + deps.iter().map(Vec::len).sum::<usize>();
        for root in 0..self.nodes.len() {
            let mut stack = vec![root];
            while let Some(&top) = stack.last() {
                if rb.built(top) {
                    stack.pop();
                    continue;
                }
                let mut waiting = false;
                for &child in &deps[top] {
                    if child >= self.nodes.len() {
                        return Err(ImageError::Malformed("reference outside the arena"));
                    }
                    if !rb.built(child) {
                        waiting = true;
                        budget = budget.checked_sub(1).ok_or(ImageError::Malformed("cycle between immutables"))?;
                        stack.push(child);
                    }
                }
                if waiting {
                    continue;
                }
                match self.make(top, rb)? {
                    Made::Value(v) => rb.values[top] = Some(v),
                    Made::Aux(a) => rb.aux[top] = Some(a),
                }
                stack.pop();
            }
        }
        Ok(())
    }

    /// Refaz um nó da segunda passada (todas as dependências dele já existem).
    fn make(&self, index: usize, rb: &Rebuilt) -> Result<Made, ImageError> {
        Ok(match &self.nodes[index] {
            Node::Tuple(items) => Made::Value(Value::tuple(rb.values_of(items)?)),
            Node::Set(table) if table.frozen => Made::Value(Value::set(import_set(table, rb)?)),
            Node::Slice(a, b, c) => Made::Value(Value::Slice(Rc::new((rb.value(*a)?, rb.value(*b)?, rb.value(*c)?)))),
            Node::Bound { recv, name } => Made::Value(Value::Bound(Rc::new(BoundMethod { recv: rb.value(*recv)?, name: *name }))),
            Node::BoundFn { recv, func } => Made::Value(Value::BoundFn(Rc::new((rb.value(*recv)?, rb.function(*func)?)))),
            Node::Function(f) => {
                let closure = match f.closure {
                    Some(env) => Some(rb.env(env)?),
                    None => None,
                };
                Made::Value(Value::Function(Rc::new(FuncObj {
                    code: rb.code(f.code)?,
                    defaults: rb.values_of(&f.defaults)?,
                    kwdefaults: rb.pairs(&f.kwdefaults)?,
                    closure,
                    globals: rb.globals(f.globals)?,
                    attrs: RefCell::new(BTreeMap::new()),
                })))
            }
            Node::Code(c) => {
                let mut functions = Vec::with_capacity(c.functions.len());
                for f in &c.functions {
                    functions.push(rb.code(*f)?);
                }
                let names = |list: &[String]| -> Vec<Rc<str>> { list.iter().map(|n| rb.name(n)).collect() };
                Made::Aux(Aux::Code(Rc::new(Code {
                    ops: c.ops.clone(),
                    lines: c.lines.clone(),
                    spans: c.spans.clone(),
                    consts: rb.values_of(&c.consts)?,
                    names: names(&c.names),
                    name: c.name.clone(),
                    qualname: c.qualname.clone(),
                    params: names(&c.params),
                    posonly: c.posonly,
                    vararg: c.vararg.as_deref().map(|n| rb.name(n)),
                    kwonly: names(&c.kwonly),
                    kwarg: c.kwarg.as_deref().map(|n| rb.name(n)),
                    varnames: names(&c.varnames),
                    cellvars: names(&c.cellvars),
                    freevars: names(&c.freevars),
                    is_function: c.is_function,
                    is_class: c.is_class,
                    is_generator: c.is_generator,
                    is_async: c.is_async,
                    functions,
                    filename: c.filename.clone(),
                    doc: c.doc.clone(),
                    first_line: c.first_line,
                    uses_class_cell: c.uses_class_cell,
                    internal: c.internal,
                    type_params_role: c.type_params_role,
                    future_flags: c.future_flags,
                    cpy: match &c.cpy {
                        Some(e) => Some(Rc::new(crate::cpybc::Emitted {
                            code: e.code.clone(),
                            consts: rb.values_of(&e.consts)?,
                            names: e.names.iter().map(|n| rb.name(n)).collect(),
                            linetable: e.linetable.clone(),
                            exceptiontable: e.exceptiontable.clone(),
                            stacksize: e.stacksize,
                            first_line: e.first_line,
                            synthetic: e.synthetic,
                        })),
                        None => None,
                    },
                })))
            }
            Node::Env(e) => {
                let parent = match e.parent {
                    Some(p) => Some(rb.env(p)?),
                    None => None,
                };
                Made::Aux(Aux::Env(Rc::new(Env {
                    vars: RefCell::new(LocalMap::default()),
                    order: RefCell::new(Vec::new()),
                    parent,
                    is_class: e.is_class,
                    is_module: e.is_module,
                    finished: std::cell::Cell::new(e.finished),
                    // Os `FrameHold` refeitos da imagem contam de novo ao nascer.
                    holds: std::cell::Cell::new(0),
                })))
            }
            Node::Class(c) => {
                let mut bases = Vec::with_capacity(c.bases.len());
                for b in &c.bases {
                    bases.push(rb.class(*b)?);
                }
                let meta = match c.meta {
                    Some(m) => Some(rb.class(m)?),
                    None => None,
                };
                Made::Value(Value::Class(Rc::new(ClassObj {
                    name: c.name.clone(),
                    qualname: c.qualname.clone(),
                    bases,
                    builtin_base: c.builtin_base,
                    data_base: c.data_base,
                    meta,
                    is_meta: c.is_meta,
                    dict: RefCell::new(AttrMap::default()),
                    subclasses: RefCell::new(Vec::new()),
                })))
            }
            // Nasce finalizada: se a reconstrução falhar no meio, o objeto pela metade não enfileira `__del__`.
            // A terceira passada põe o estado verdadeiro.
            Node::Instance(i) => Made::Value(Value::Instance(Rc::new(InstanceObj {
                class_cell: RefCell::new(rb.class(i.class)?),
                dict: RefCell::new(AttrMap::default()),
                view: RefCell::new(None),
                payload: RefCell::new(None),
                finalized: std::cell::Cell::new(true),
            }))),
            Node::Exception(e) => Made::Value(Value::Exception(Rc::new(ExcObj::new(e.kind, rb.values_of(&e.args)?)))),
            Node::Ext(x) => Made::Value(make_ext(x, rb)?),
            Node::GenCore(c) => {
                let shell = rb.shell.as_ref().ok_or(ImageError::Malformed("a generator needs the Vm to restore into"))?;
                let frame = Frame::new(rb.code(c.frame.code)?, rb.env(c.frame.env)?);
                Made::Aux(Aux::Core(GenCore::rebuilt(shell, c.kind, rb.globals(c.globals)?, frame, c.flags)))
            }
            Node::Pending
            | Node::Big(_)
            | Node::Str(_)
            | Node::Bytes(_)
            | Node::ByteArray(_)
            | Node::List(_)
            | Node::Dict(_)
            | Node::Set(_)
            | Node::NativeFn(..)
            | Node::Globals(_)
            | Node::Native(_)
            | Node::Frame(_)
            | Node::Callee(_)
            | Node::Module { .. } => return Err(ImageError::Malformed("shell without a node to build")),
        })
    }

    /// Passada 3: o conteúdo mutável do nó `index`, agora que todo alvo existe.
    fn fill(&self, index: usize, rb: &Rebuilt) -> Result<(), ImageError> {
        match (&self.nodes[index], &rb.values[index], &rb.aux[index]) {
            (Node::List(items), Some(Value::List(l)), _) => *l.borrow_mut() = rb.values_of(items)?,
            (Node::Dict(entries), Some(Value::Dict(d)), _) => {
                let mut restored = Vec::with_capacity(entries.len());
                for (h, k, v) in entries {
                    restored.push((*h, rb.value(*k)?, rb.value(*v)?));
                }
                *d.borrow_mut() = Dict::from_hashed(restored);
            }
            (Node::Set(table), Some(Value::Set(s)), _) if !table.frozen => *s.borrow_mut() = import_set(table, rb)?,
            (Node::Globals(entries), _, Some(Aux::Globals(globals))) => {
                let mut map = VarMap::default();
                for (k, v) in entries {
                    map.insert(rb.name(k), rb.value(*v)?);
                }
                *globals.borrow_mut() = map;
            }
            (Node::Module { attrs, .. }, Some(Value::Module(m)), _) => {
                *m.attrs.borrow_mut() = rb.pairs(attrs)?.into_iter().collect();
            }
            (Node::Function(f), Some(Value::Function(func)), _) => {
                *func.attrs.borrow_mut() = rb.pairs(&f.attrs)?.into_iter().collect();
            }
            (Node::Env(e), _, Some(Aux::Env(env))) => {
                let mut vars = LocalMap::default();
                for (k, v) in &e.vars {
                    vars.insert(rb.name(k), rb.value(*v)?);
                }
                *env.vars.borrow_mut() = vars;
                *env.order.borrow_mut() = e.order.clone();
            }
            (Node::Class(c), Some(Value::Class(class)), _) => {
                let mut dict = AttrMap::default();
                for (k, v) in &c.dict {
                    dict.insert(k.clone(), rb.value(*v)?);
                }
                *class.dict.borrow_mut() = dict;
                let mut subclasses = Vec::with_capacity(c.subclasses.len());
                for s in &c.subclasses {
                    subclasses.push(Rc::downgrade(&rb.class(*s)?));
                }
                *class.subclasses.borrow_mut() = subclasses;
            }
            (Node::Instance(i), Some(Value::Instance(obj)), _) => {
                let mut dict = AttrMap::default();
                for (k, v) in &i.dict {
                    dict.insert(k.clone(), rb.value(*v)?);
                }
                *obj.dict.borrow_mut() = dict;
                *obj.view.borrow_mut() = match rb.opt(&i.view)? {
                    Some(Value::Dict(d)) => Some(d),
                    Some(_) => return Err(ImageError::Malformed("instance view is not a dict")),
                    None => None,
                };
                *obj.payload.borrow_mut() = rb.opt(&i.payload)?;
                obj.finalized.set(i.finalized);
                // Quem ainda não foi finalizado entra no registro da finalização da saída, como se tivesse
                // sido criado agora.
                if !i.finalized && obj.class().finalizer().is_some() {
                    crate::finalize::register(obj);
                }
            }
            (Node::Exception(e), Some(Value::Exception(exc)), _) => {
                *exc.traceback.borrow_mut() = rb.opt(&e.traceback)?;
                *exc.chain.borrow_mut() =
                    ExcChain { cause: rb.opt(&e.cause)?, context: rb.opt(&e.context)?, suppress: e.suppress };
                let mut extra = Vec::with_capacity(e.extra.len());
                for (k, v) in &e.extra {
                    extra.push((*k, rb.value(*v)?));
                }
                *exc.extra.borrow_mut() = extra;
            }
            (Node::Ext(ExtNode::Property { doc, .. }), Some(Value::Ext(ext)), _) => {
                let doc = rb.value(*doc)?;
                if !matches!(doc, Value::None) {
                    let _ = ext.setattr("__doc__", doc);
                }
            }
            (Node::Native(n), Some(Value::Native(cell)), _) => {
                *cell.borrow_mut() = match &**n {
                    NativeNode::File(f) => Native::File(PyFile {
                        kind: f.kind,
                        lines: f.lines.clone(),
                        pos: f.pos,
                        loaded: f.loaded,
                        closed: f.closed,
                        name: f.name.clone(),
                        raw: f.raw.clone(),
                        raw_eof: f.raw_eof,
                    }),
                    NativeNode::CsvReader { reader, src } => Native::CsvReader { reader: reader.clone(), src: rb.value(*src)? },
                    NativeNode::CsvWriter { dialect, target } => {
                        Native::CsvWriter { dialect: dialect.clone(), target: rb.value(*target)? }
                    }
                };
            }
            (Node::GenCore(c), _, Some(Aux::Core(core))) => {
                let stack = rb.slots(&c.frame.stack)?;
                let handled = rb.values_of(&c.frame.handled)?;
                let returned = rb.opt(&c.returned)?;
                core.edit(|frame, ret| {
                    frame.stack = stack;
                    frame.blocks = Rebuilt::blocks(&c.frame.blocks);
                    frame.pc = c.frame.pc;
                    frame.handled = handled;
                    frame.handled_base = c.frame.handled_base;
                    *ret = returned;
                });
            }
            (Node::Ext(ExtNode::Frame(f)), Some(ext @ Value::Ext(_)), _) => {
                crate::frameobj::restore_links(ext, rb.value(f.back)?, rb.value(f.trace)?);
            }
            _ => {}
        }
        Ok(())
    }
}

/// A `Vm` do pai como dado `Send` (fatia H5): o heap alcançável das tabelas dela mais os campos que não
/// são `Value` (o `stdout` ainda não descarregado, `argv`, a linha e a profundidade corrente) e o estado
/// por thread que o filho precisa ter igual (os textos-fonte dos tracebacks, o limite de recursão, os
/// rastreadores de `sys.settrace` e `sys.setprofile`, e se há tratador de sinal).
///
/// Para o `os.fork` (fatia F1) a imagem leva também os quadros da execução: o quadro do programa, os
/// chamados que esperavam em `frames_stack` e o que estava em execução (`VmImage::capture_fork`). Eles
/// vêm como as últimas raízes, na ordem, e voltam em [`Resume`].
/// `rust_nest` nasce 0 no filho (o `run_loop` do filho o sobe a 1). O alarme de `signal.alarm` não passa
/// ao filho, como no `fork(2)`.
///
/// Os demais `thread_local!` não entram: o filho roda numa thread do SO nova, que os recebe no valor
/// inicial (o cache `SEEN` vazio, `TEXT_ERROR*` limpos), como a seção 2.1 do desenho pede.
pub struct VmImage {
    heap: HeapImage,
    /// Quantas exceções em tratamento (`Vm::handled`) vêm nas raízes.
    handled: usize,
    /// Os nomes, na ordem das raízes, de `modules`, `foreign_modules` e `module_globals`.
    module_names: Vec<String>,
    foreign_names: Vec<String>,
    module_global_names: Vec<&'static str>,
    /// Os módulos embutidos com globais completas à parte (`pysrc::private_attr`), na ordem das raízes.
    private_names: Vec<&'static str>,
    /// A linha do chamador de cada entrada de `Vm::frames` (código e escopo vêm nas raízes).
    frame_lines: Vec<usize>,
    stdout: Vec<u8>,
    stderr_capture: String,
    argv: Vec<String>,
    depth: usize,
    cur_line: usize,
    sources: Vec<(String, String)>,
    recursion_limit: usize,
    signals_armed: bool,
    /// Os quadros da execução, quando a imagem é a de um `os.fork`.
    fork: Option<ForkMeta>,
}

/// O que as últimas raízes de uma imagem de `os.fork` são.
struct ForkMeta {
    /// Quantos chamados esperavam em `frames_stack`.
    suspended: usize,
    /// Havia um chamado em execução (senão o quadro em execução é o do programa).
    has_child: bool,
    entry: crate::fork::Entry,
}

/// O estado de execução que o filho de um `os.fork` retoma (ver `Vm::run_resumed`).
pub(crate) struct Resume {
    pub(crate) outer: Frame,
    pub(crate) suspended: Vec<Callee>,
    pub(crate) child: Option<Callee>,
    pub(crate) entry: crate::fork::Entry,
}

/// Os quadros que `VmImage::capture_fork` copia.
struct ForkFrames<'a> {
    outer: &'a Frame,
    suspended: &'a [Callee],
    child: Option<&'a Callee>,
    entry: crate::fork::Entry,
}

const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<VmImage>();
};

fn node_of(item: Item) -> Result<u32, ImageError> {
    match item {
        Item::Node(i) => Ok(i),
        _ => Err(ImageError::Malformed("a root that must be a node is a scalar")),
    }
}

impl VmImage {
    /// Captura a `Vm` e o estado por thread. Roda inteira na thread do pai, com a `Vm` em mãos.
    pub fn capture(vm: &Vm) -> Result<VmImage, ImageError> {
        VmImage::build(vm, None)
    }

    /// Como [`VmImage::capture`], com os quadros da execução de um `os.fork`: `outer` é o quadro do
    /// programa, `Vm::frames_stack` guarda os chamados que esperam e `child` o que está em execução.
    pub(crate) fn capture_fork(
        vm: &Vm,
        outer: &Frame,
        child: Option<&Callee>,
        entry: crate::fork::Entry,
    ) -> Result<VmImage, ImageError> {
        let suspended = vm.frames_stack.borrow();
        VmImage::build(vm, Some(ForkFrames { outer, suspended: &suspended, child, entry }))
    }

    fn build(vm: &Vm, fork: Option<ForkFrames<'_>>) -> Result<VmImage, ImageError> {
        let mut roots = vec![Root::Globals(vm.globals.clone())];
        roots.extend(vm.std_files.iter().map(|f| Root::Value(Value::Native(f.clone()))));
        let handled: Vec<Value> = vm.handled.borrow().clone();
        let handled_count = handled.len();
        roots.extend(handled.into_iter().map(Root::Value));

        let modules = vm.modules.borrow();
        let mut module_names: Vec<String> = modules.keys().cloned().collect();
        module_names.sort();
        roots.extend(module_names.iter().map(|n| Root::Value(Value::Module(modules[n].clone()))));

        let foreign = vm.foreign_modules.borrow();
        let mut foreign_names: Vec<String> = foreign.keys().cloned().collect();
        foreign_names.sort();
        roots.extend(foreign_names.iter().map(|n| Root::Value(foreign[n].clone())));

        let module_globals = vm.module_globals.borrow();
        let mut module_global_names: Vec<&'static str> = module_globals.keys().copied().collect();
        module_global_names.sort_unstable();
        roots.extend(module_global_names.iter().map(|n| Root::Globals(module_globals[n].clone())));
        // As globais completas dos módulos embutidos filtrados (o código embutido as lê por `private_attr`).
        let private = crate::modules::pysrc::private_snapshot();
        let private_names: Vec<&'static str> = private.iter().map(|(n, _)| *n).collect();
        roots.extend(private.into_iter().map(|(_, g)| Root::Globals(g)));

        let mut frame_lines = Vec::new();
        for (code, line, env) in vm.frames.borrow().iter() {
            roots.push(Root::Code(code.clone()));
            roots.push(Root::Env(env.clone()));
            frame_lines.push(*line);
        }
        // Os rastreadores de `sys.settrace` e `sys.setprofile` (`None` quando não há).
        roots.push(Root::Value(crate::tracing::get()));
        roots.push(Root::Value(crate::tracing::get_profile()));
        let meta = fork.as_ref().map(|f| ForkMeta { suspended: f.suspended.len(), has_child: f.child.is_some(), entry: f.entry });
        if let Some(f) = &fork {
            roots.push(Root::Frame(f.outer));
            roots.extend(f.suspended.iter().map(Root::Callee));
            roots.extend(f.child.map(Root::Callee));
        }
        let heap = HeapImage::capture_roots(&roots)?;
        Ok(VmImage {
            heap,
            handled: handled_count,
            module_names,
            foreign_names,
            module_global_names,
            private_names,
            frame_lines,
            stdout: vm.stdout.borrow().clone(),
            stderr_capture: vm.stderr_capture.borrow().clone(),
            argv: vm.argv.to_vec(),
            depth: vm.depth.get(),
            cur_line: vm.cur_line.get(),
            sources: crate::vm::sources_snapshot(),
            recursion_limit: crate::vm::RECURSION_LIMIT.with(|l| l.get()),
            signals_armed: crate::vm::signals_armed(),
            fork: meta,
        })
    }

    /// Quantos nós o heap da imagem tem.
    pub fn len(&self) -> usize {
        self.heap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }

    /// Refaz a `Vm` e instala o estado por thread na thread de quem chama (a do filho): os textos-fonte,
    /// o limite de recursão e a própria `Vm` como a corrente da thread. A `Vm` nasce com as cascas
    /// (globais, arquivos padrão) no lugar antes de o resto do heap existir, porque os geradores refeitos
    /// compartilham as tabelas dela.
    pub fn restore(&self) -> Result<Vm, ImageError> {
        Ok(self.restore_all()?.0)
    }

    /// Refaz a `Vm` de um `os.fork` e o estado de execução que o filho retoma. Além do que [`VmImage::restore`]
    /// instala, o filho passa a ver o `id()` e o hash por identidade que os objetos tinham no pai.
    pub(crate) fn restore_fork(&self) -> Result<(Vm, Resume), ImageError> {
        let (vm, resume) = self.restore_all()?;
        Ok((vm, resume.ok_or(ImageError::Malformed("the Vm image has no frames to resume"))?))
    }

    fn restore_all(&self) -> Result<(Vm, Option<Resume>), ImageError> {
        let heap = &self.heap;
        let mut rb = heap.rebuilt(None);
        heap.build_shells(&mut rb)?;
        let mut roots = heap.roots.iter().copied();
        let mut next = || roots.next().ok_or(ImageError::Malformed("the Vm image has too few roots"));

        let globals = rb.globals(node_of(next()?)?)?;
        let mut std_files = Vec::with_capacity(3);
        for _ in 0..3 {
            match rb.value(next()?)? {
                Value::Native(file) => std_files.push(file),
                _ => return Err(ImageError::Malformed("a standard file root is not a native file")),
            }
        }
        let std_files: [Rc<RefCell<Native>>; 3] =
            std_files.try_into().map_err(|_| ImageError::Malformed("the Vm image needs three standard files"))?;
        let vm = Vm {
            globals,
            stdout: Rc::new(RefCell::new(self.stdout.clone())),
            stderr_capture: RefCell::new(self.stderr_capture.clone()),
            handled: Rc::new(RefCell::new(Vec::new())),
            depth: Rc::new(std::cell::Cell::new(self.depth)),
            cur_line: Rc::new(std::cell::Cell::new(self.cur_line)),
            frames: Rc::new(RefCell::new(Vec::new())),
            frames_stack: Rc::new(RefCell::new(Vec::new())),
            rust_nest: Rc::new(std::cell::Cell::new(0)),
            argv: Rc::new(self.argv.clone()),
            std_files,
            modules: Rc::new(RefCell::new(HashMap::new())),
            foreign_modules: Rc::new(RefCell::new(HashMap::new())),
            module_globals: Rc::new(RefCell::new(HashMap::new())),
        };
        rb.shell = Some(vm.clone());
        heap.complete(&mut rb)?;

        let mut handled = Vec::with_capacity(self.handled);
        for _ in 0..self.handled {
            handled.push(rb.value(next()?)?);
        }
        *vm.handled.borrow_mut() = handled;
        for name in &self.module_names {
            match rb.value(next()?)? {
                Value::Module(m) => {
                    vm.modules.borrow_mut().insert(name.clone(), m);
                }
                _ => return Err(ImageError::Malformed("a module root is not a module")),
            }
        }
        for name in &self.foreign_names {
            let value = rb.value(next()?)?;
            vm.foreign_modules.borrow_mut().insert(name.clone(), value);
        }
        for name in &self.module_global_names {
            let table = rb.globals(node_of(next()?)?)?;
            vm.module_globals.borrow_mut().insert(*name, table);
        }
        for name in &self.private_names {
            let table = rb.globals(node_of(next()?)?)?;
            crate::modules::pysrc::private_install(*name, table);
        }
        for line in &self.frame_lines {
            let code = rb.code(node_of(next()?)?)?;
            let env = rb.env(node_of(next()?)?)?;
            vm.frames.borrow_mut().push((code, *line, env));
        }
        let trace = rb.value(next()?)?;
        let profile = rb.value(next()?)?;
        let resume = match &self.fork {
            Some(meta) => {
                let outer = match &heap.nodes[node_of(next()?)? as usize] {
                    Node::Frame(f) => rb.frame(f)?,
                    _ => return Err(ImageError::Malformed("the program frame root is not a frame")),
                };
                let mut suspended = Vec::with_capacity(meta.suspended);
                for _ in 0..meta.suspended {
                    suspended.push(heap.callee_root(next()?, &rb)?);
                }
                let child = if meta.has_child { Some(heap.callee_root(next()?, &rb)?) } else { None };
                Some(Resume { outer, suspended, child, entry: meta.entry })
            }
            None => None,
        };

        for (file, text) in &self.sources {
            crate::vm::register_source(file, text);
        }
        crate::vm::RECURSION_LIMIT.with(|l| l.set(self.recursion_limit));
        crate::tracing::set((!matches!(trace, Value::None)).then_some(trace));
        crate::tracing::set_profile((!matches!(profile, Value::None)).then_some(profile));
        if self.signals_armed {
            crate::vm::arm_signals();
        }
        if self.fork.is_some() {
            crate::object::install_inherited_ids(heap.inherited_ids(&rb));
        }
        crate::vm::set_current(&vm);
        Ok((vm, resume))
    }
}

impl HeapImage {
    /// O quadro de chamado de uma raiz (`Node::Callee`).
    fn callee_root(&self, item: Item, rb: &Rebuilt) -> Result<Callee, ImageError> {
        match &self.nodes[node_of(item)? as usize] {
            Node::Callee(c) => rb.callee(c),
            _ => Err(ImageError::Malformed("a call frame root is not a call frame")),
        }
    }

    /// O `id()` de cada objeto refeito para o que ele tinha no pai: `id()` e hash por identidade iguais
    /// nos dois lados do `fork`. Só objetos que ganharam nó entram (os escalares têm `id()` fixo).
    fn inherited_ids(&self, rb: &Rebuilt) -> HashMap<usize, usize> {
        self.ids
            .iter()
            .filter_map(|&(index, parent)| {
                let rebuilt = rb.values.get(index as usize)?.as_ref()?;
                Some((crate::builtins::id_of(rebuilt) as usize, parent as usize))
            })
            .collect()
    }
}

/// Refaz um objeto nativo da segunda passada (todas as dependências dele já existem).
fn make_ext(x: &ExtNode, rb: &Rebuilt) -> Result<Value, ImageError> {
    let basic = |image: ExtImage| crate::classes::ext_from_image(image).ok_or(ImageError::Malformed("not a basic native object"));
    match x {
        ExtNode::StaticMethod(i) => basic(ExtImage::StaticMethod(rb.value(*i)?)),
        ExtNode::ClassMethod(i) => basic(ExtImage::ClassMethod(rb.value(*i)?)),
        // O `__doc__` próprio entra na terceira passada: pode apontar para um ciclo.
        ExtNode::Property { get, set, del, .. } => {
            basic(ExtImage::Property { get: rb.value(*get)?, set: rb.opt(set)?, del: rb.opt(del)?, doc: Value::None })
        }
        ExtNode::PropertyCopy { obj, which } => basic(ExtImage::PropertyCopy { obj: rb.value(*obj)?, which: *which }),
        ExtNode::PlainObject => basic(ExtImage::PlainObject),
        ExtNode::ClassCell(env) => basic(ExtImage::ClassCell(rb.env(*env)?)),
        ExtNode::Lazy(lazy) => {
            crate::lazy::lazy_from_parts(rb.lazy(lazy)?).ok_or(ImageError::Malformed("iterator holds a value of the wrong type"))
        }
        ExtNode::Generator { core, role } => Ok(crate::generator::wrap_core(rb.core(*core)?, rb.role(role)?)),
        ExtNode::AsyncGenWrapped(i) => Ok(crate::generator::wrap_async_value(rb.value(*i)?)),
        ExtNode::WeakRef { target, callback, hash } => {
            crate::modules::weakrefmod::weakref_from_image(rb.opt(target)?, rb.opt(callback)?, *hash)
                .ok_or(ImageError::Malformed("weak reference to a value that cannot be weakly referenced"))
        }
        ExtNode::Opaque { tag, state, refs } => restore_opaque(tag, state.as_ref(), rb.values_of(refs)?),
        ExtNode::Traceback { entries, filename } => {
            let mut out = Vec::with_capacity(entries.len());
            for e in entries {
                let held = match e.held {
                    Some((env, code)) => Some(crate::frameobj::FrameHold::new(rb.env(env)?, rb.code(code)?)),
                    None => None,
                };
                out.push((e.line, e.name.clone(), rb.name(&e.file), e.span, held));
            }
            Ok(crate::tbobj::TracebackObj::make(out, filename))
        }
        ExtNode::CodeObject { name, filename, code } => {
            let code = code.map(|c| rb.code(c)).transpose()?;
            Ok(crate::tbobj::code_object_from_image(name.clone(), rb.name(filename), code))
        }
        ExtNode::CodeSource { src, filename, code } => {
            Ok(crate::builtins_ext::code_source_from_image(src.clone(), filename.clone(), rb.code(*code)?))
        }
        // `f_back` e `f_trace` entram na terceira passada (`Rebuilt::fill`).
        ExtNode::Frame(f) => Ok(crate::frameobj::frame_from_image(crate::frameobj::FrameParts {
            line: f.line,
            name: f.name.clone(),
            file: rb.name(&f.file),
            code: f.code.map(|c| rb.code(c)).transpose()?,
            code_object: rb.value(f.code_object)?,
            env: f.env.map(|e| rb.env(e)).transpose()?,
            held: f.held.map(|e| rb.env(e)).transpose()?,
            back: Value::None,
            live: f.live,
            trace: Value::None,
            trace_lines: f.trace_lines,
            trace_opcodes: f.trace_opcodes,
            last_line: f.last_line,
        })),
    }
}

/// Quem refaz um objeto `Opaque`: recebe o `tag` do `image()`, o estado `Send` e os valores refeitos.
/// `None` quando o estado é de outro tipo (imagem malformada).
type OpaqueRestore = fn(&str, &(dyn std::any::Any + Send + Sync), Vec<Value>) -> Option<Value>;

/// A tabela de `tag` para o módulo que refaz. Um `tag` que o `image()` de algum tipo emite e esta tabela
/// não lista é um erro de reconstrução (`Malformed`), nunca um objeto copiado errado.
fn restore_opaque(tag: &str, state: &(dyn std::any::Any + Send + Sync), refs: Vec<Value>) -> Result<Value, ImageError> {
    let restore: OpaqueRestore = match tag {
        "std_buffer" => crate::stdbuf::restore_image,
        "dict_view" => crate::dictview::restore_image,
        "generic_alias" | "union_type" => crate::generic::restore_image,
        "type_class_method" | "new_fn" | "class_getitem" | "unbound" => crate::typeattrs::restore_image,
        "native_type_method" | "getset_descriptor" | "member_descriptor" | "slice_indices" | "subclasses_call"
        | "alt_ctor" | "file_exit" | "exc_with_traceback" | "exc_add_note" | "instance_dunder"
        | "builtin_super_method" | "super_proxy" | "plain_object_method" | "object_class_method" | "shim_method"
        | "shim_new" | "bound_callable" => {
            crate::classes::restore_image
        }
        "hash" | "hmac" => crate::modules::hashlib::restore_image,
        "mersenne_twister" => crate::modules::mtrandom::restore_image,
        "fd_guard" => crate::modules::osnative::restore_image,
        "ucd" => crate::modules::unicodedata::restore_image,
        "re_pattern" | "re_match" | "re_finditer" => crate::modules::re::restore_image,
        "complex_const" => crate::cpybc::restore_complex_const,
        _ => return Err(ImageError::Malformed("unknown opaque object tag")),
    };
    restore(tag, state, refs).ok_or(ImageError::Malformed("opaque object state of another type"))
}

fn import_set(table: &SetTable<Item>, rb: &Rebuilt) -> Result<Set, ImageError> {
    let mut failure = None;
    let set = Set::import_table(table.clone(), |item| match rb.value(item) {
        Ok(v) => v,
        Err(e) => {
            failure.get_or_insert(e);
            Value::None
        }
    });
    failure.map_or(Ok(set), Err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{ExtObject, Kw};
    use crate::vm::{PyException, Vm};

    fn roundtrip(roots: &[Value]) -> Vec<Value> {
        let image = HeapImage::capture(roots).ok().expect("capture");
        // A imagem atravessa uma thread: é o que o `os.fork` faz com ela.
        std::thread::spawn(move || image.restore().expect("restore").into_iter().map(|_| ()).count()).join().expect("thread");
        HeapImage::capture(roots).ok().expect("capture").restore().expect("restore")
    }

    fn list_of(v: &Value) -> Vec<Value> {
        let Value::List(l) = v else { panic!("not a list") };
        l.borrow().clone()
    }

    fn int(v: &Value) -> i64 {
        let Value::Int(n) = v else { panic!("not an int") };
        *n
    }

    fn text(v: &Value) -> String {
        let Value::Str(s) = v else { panic!("not a str") };
        s.as_str().to_string()
    }

    /// Roda `src` como módulo principal e devolve a VM com as globais preenchidas.
    fn run(src: &str) -> Vm {
        let module = crate::parser::parse_module(src).ok().expect("parse");
        let code = Rc::new(crate::compile::compile_module(&module).ok().expect("compile"));
        let mut vm = Vm::new();
        assert!(vm.run(&code).is_ok());
        vm
    }

    fn global(vm: &Vm, name: &str) -> Value {
        vm.globals.borrow().get(name).cloned().expect("global")
    }

    /// Captura as globais `names` juntas e as refaz.
    fn restore_globals(vm: &Vm, names: &[&str]) -> Vec<Value> {
        let roots: Vec<Value> = names.iter().map(|n| global(vm, n)).collect();
        roundtrip(&roots)
    }

    fn call(vm: &mut Vm, f: &Value, args: Vec<Value>) -> Value {
        vm.call(f, args, Vec::new()).ok().expect("call")
    }

    fn attr(vm: &mut Vm, obj: &Value, name: &str) -> Value {
        vm.load_attr(obj, name).ok().expect("attribute")
    }

    fn class_of(v: &Value) -> Rc<ClassObj> {
        let Value::Class(c) = v else { panic!("not a class") };
        c.clone()
    }

    fn instance_of(v: &Value) -> Rc<InstanceObj> {
        let Value::Instance(i) = v else { panic!("not an instance") };
        i.clone()
    }

    fn function_of(v: &Value) -> Rc<FuncObj> {
        let Value::Function(f) = v else { panic!("not a function") };
        f.clone()
    }

    #[test]
    fn scalars_round_trip() {
        let big = Value::Big(Rc::new(BigInt::from(i64::MAX) * 4));
        let out = roundtrip(&[
            Value::None,
            Value::Bool(true),
            Value::Int(-7),
            Value::Float(1.5),
            Value::Float(f64::NAN),
            big,
            Value::str("olá"),
            Value::bytes(b"\x00\xff".to_vec()),
            Value::bytearray(b"ab".to_vec()),
            Value::Range(Range { start: 1, stop: 9, step: 2 }),
        ]);
        assert!(matches!(out[0], Value::None));
        assert!(matches!(out[1], Value::Bool(true)));
        assert_eq!(int(&out[2]), -7);
        assert!(matches!(out[3], Value::Float(f) if f == 1.5));
        assert!(matches!(out[4], Value::Float(f) if f.is_nan()));
        assert!(matches!(&out[5], Value::Big(b) if **b == BigInt::from(i64::MAX) * 4));
        assert_eq!(text(&out[6]), "olá");
        assert!(matches!(&out[7], Value::Bytes(b) if &b[..] == b"\x00\xff"));
        assert!(matches!(&out[8], Value::ByteArray(b) if b.borrow().as_slice() == b"ab"));
        assert!(matches!(out[9], Value::Range(r) if (r.start, r.stop, r.step) == (1, 9, 2)));
    }

    #[test]
    fn escaped_code_points_survive() {
        // Um surrogate solto fica guardado como o par U+10FFFF mais o char (ver `PyStr`).
        let s = Value::str(crate::object::cp_to_str(0xD800));
        let out = roundtrip(&[s.clone()]);
        let (Value::Str(a), Value::Str(b)) = (&s, &out[0]) else { panic!("not a str") };
        assert_eq!(a.as_str(), b.as_str());
        assert_eq!(crate::object::code_points(b.as_str()).collect::<Vec<_>>(), vec![0xD800]);
    }

    #[test]
    fn sharing_is_preserved_and_copies_are_independent() {
        let shared = Value::list(vec![Value::Int(1)]);
        let original = Value::list(vec![shared.clone(), shared.clone(), Value::list(vec![Value::Int(1)])]);
        let out = roundtrip(&[original.clone(), shared.clone()]);
        let items = list_of(&out[0]);
        let (Value::List(a), Value::List(b), Value::List(c), Value::List(root1)) = (&items[0], &items[1], &items[2], &out[1]) else {
            panic!("not lists")
        };
        assert!(Rc::ptr_eq(a, b));
        assert!(Rc::ptr_eq(a, root1));
        assert!(!Rc::ptr_eq(a, c));
        // O grafo novo não compartilha nada com o original.
        let Value::List(orig_shared) = &shared else { unreachable!() };
        assert!(!Rc::ptr_eq(a, orig_shared));
        a.borrow_mut().push(Value::Int(2));
        assert_eq!(orig_shared.borrow().len(), 1);
        assert_eq!(b.borrow().len(), 2);
    }

    #[test]
    fn list_containing_itself() {
        let a = Value::list(Vec::new());
        let Value::List(cell) = &a else { unreachable!() };
        cell.borrow_mut().push(a.clone());
        let out = roundtrip(std::slice::from_ref(&a));
        let Value::List(copy) = &out[0] else { panic!("not a list") };
        let inner = copy.borrow()[0].clone();
        let Value::List(inner) = inner else { panic!("not a list") };
        assert!(Rc::ptr_eq(copy, &inner));
        assert_eq!(copy.borrow().len(), 1);
        // Desfaz o ciclo para os `Rc` não vazarem nos testes.
        copy.borrow_mut().clear();
        cell.borrow_mut().clear();
    }

    #[test]
    fn cycle_through_a_tuple() {
        // t = ([], 5); t[0].append(t)
        let l = Value::list(Vec::new());
        let t = Value::tuple(vec![l.clone(), Value::Int(5)]);
        let Value::List(cell) = &l else { unreachable!() };
        cell.borrow_mut().push(t.clone());
        let out = roundtrip(&[t]);
        let Value::Tuple(copy) = &out[0] else { panic!("not a tuple") };
        let Value::List(inner) = &copy[0] else { panic!("not a list") };
        let back = inner.borrow()[0].clone();
        let Value::Tuple(back) = back else { panic!("not a tuple") };
        assert!(Rc::ptr_eq(copy, &back));
        assert_eq!(int(&copy[1]), 5);
        inner.borrow_mut().clear();
        cell.borrow_mut().clear();
    }

    #[test]
    fn nested_tuples_build_children_first() {
        // Os filhos são alcançados depois do pai no percurso (índice maior), e ainda assim constroem antes.
        let leaf = Value::tuple(vec![Value::Int(1)]);
        let mid = Value::tuple(vec![leaf.clone(), leaf.clone()]);
        let top = Value::tuple(vec![mid.clone(), leaf]);
        let out = roundtrip(&[top]);
        let Value::Tuple(top) = &out[0] else { panic!("not a tuple") };
        let (Value::Tuple(mid), Value::Tuple(leaf_b)) = (&top[0], &top[1]) else { panic!("not tuples") };
        let (Value::Tuple(l0), Value::Tuple(l1)) = (&mid[0], &mid[1]) else { panic!("not tuples") };
        assert!(Rc::ptr_eq(l0, l1));
        assert!(Rc::ptr_eq(l0, leaf_b));
    }

    #[test]
    fn deep_nesting_does_not_use_the_rust_stack() {
        let mut v = Value::list(Vec::new());
        for _ in 0..200_000 {
            v = Value::list(vec![v]);
        }
        let image = HeapImage::capture(&[v.clone()]).ok().expect("capture");
        assert_eq!(image.len(), 200_001);
        let out = image.restore().ok().expect("restore");
        assert_eq!(list_of(&out[0]).len(), 1);
        // Desmonta as duas cadeias sem recursão (o `Drop` recursivo estouraria a pilha).
        for root in [v, out.into_iter().next().expect("root")] {
            let mut cur = root;
            while let Value::List(l) = cur {
                let next = l.borrow_mut().pop();
                match next {
                    Some(n) => cur = n,
                    None => break,
                }
            }
        }
    }

    #[test]
    fn dict_keeps_insertion_order_and_sharing() {
        let value = Value::list(vec![Value::Int(1)]);
        let mut d = Dict::default();
        d.set(Value::str("b"), value.clone()).ok().expect("set");
        d.set(Value::Int(1), Value::None).ok().expect("set");
        d.set(Value::tuple(vec![Value::Int(2), Value::str("k")]), value).ok().expect("set");
        d.set(Value::str("gone"), Value::Int(0)).ok().expect("set");
        d.remove(&Value::str("gone")).ok().expect("remove");
        let out = roundtrip(&[Value::dict(d)]);
        let Value::Dict(copy) = &out[0] else { panic!("not a dict") };
        let copy = copy.borrow();
        assert_eq!(copy.len(), 3);
        let keys: Vec<&Value> = copy.keys().collect();
        assert_eq!(text(keys[0]), "b");
        assert_eq!(int(keys[1]), 1);
        assert!(matches!(keys[2], Value::Tuple(t) if int(&t[0]) == 2 && text(&t[1]) == "k"));
        let (Value::List(a), Value::List(b)) = (copy.get(&Value::str("b")).ok().flatten().expect("b"), copy.values().nth(2).cloned().expect("v"))
        else {
            panic!("not lists")
        };
        assert!(Rc::ptr_eq(&a, &b));
    }

    #[test]
    fn set_keeps_the_exact_iteration_order() {
        let mut s = Set::new();
        for n in [8, 1, 100, 17, 33, 64, 5, 9, 1000, 7, 2] {
            s.add(Value::Int(n)).ok().expect("add");
        }
        for n in [17, 5] {
            s.discard(&Value::Int(n)).ok().expect("discard");
        }
        s.add(Value::Int(25)).ok().expect("add");
        let before: Vec<i64> = s.iter().map(int).collect();
        let out = roundtrip(&[Value::set(s)]);
        let Value::Set(copy) = &out[0] else { panic!("not a set") };
        assert!(!copy.borrow().is_frozen());
        assert_eq!(copy.borrow().iter().map(int).collect::<Vec<_>>(), before);
        assert!(copy.borrow().contains(&Value::Int(25)).ok().expect("contains"));
        assert!(!copy.borrow().contains(&Value::Int(17)).ok().expect("contains"));
    }

    #[test]
    fn frozenset_inside_set_and_dict_keys() {
        let mut inner = Set::new();
        inner.add(Value::Int(1)).ok().expect("add");
        inner.add(Value::str("x")).ok().expect("add");
        let fz = Value::frozenset(inner);
        let mut outer = Set::new();
        outer.add(fz.clone()).ok().expect("add");
        outer.add(Value::tuple(vec![fz.clone(), Value::Int(3)])).ok().expect("add");
        let mut d = Dict::default();
        d.set(fz.clone(), Value::Int(9)).ok().expect("set");
        let out = roundtrip(&[Value::set(outer), Value::dict(d), fz]);
        let Value::Set(outer) = &out[0] else { panic!("not a set") };
        let Value::Dict(d) = &out[1] else { panic!("not a dict") };
        let Value::Set(fz) = &out[2] else { panic!("not a frozenset") };
        assert!(fz.borrow().is_frozen());
        assert_eq!(outer.borrow().len(), 2);
        assert!(outer.borrow().contains(&out[2]).ok().expect("contains"));
        assert_eq!(int(&d.borrow().get(&out[2]).ok().flatten().expect("value")), 9);
        let tuple_member = outer.borrow().iter().find(|v| matches!(v, Value::Tuple(_))).cloned().expect("tuple");
        let Value::Tuple(t) = tuple_member else { unreachable!() };
        assert!(matches!(&t[0], Value::Set(s) if Rc::ptr_eq(s, fz)));
    }

    /// Um objeto nativo que nenhuma fatia cobre.
    struct Opaque;

    impl ExtObject for Opaque {
        fn type_name(&self) -> &'static str {
            "opaque"
        }
    }

    #[test]
    fn unsupported_values_are_reported_by_type() {
        let err = HeapImage::capture(&[Value::list(vec![Value::Ext(Rc::new(Opaque))])]).err().expect("error");
        assert_eq!(err, ImageError::Unsupported("opaque"));
    }

    fn seven(_: &mut Vm, _: Vec<Value>, _: Kw) -> Result<Value, PyException> {
        Ok(Value::Int(7))
    }

    #[test]
    fn module_native_functions_bound_methods_and_slices() {
        let items = Value::list(vec![Value::Int(1)]);
        let module = Rc::new(ModuleObj { name: "image_mod", attrs: RefCell::new(BTreeMap::new()) });
        {
            let mut attrs = module.attrs.borrow_mut();
            attrs.insert("self".to_string(), Value::Module(module.clone()));
            attrs.insert("items".to_string(), items.clone());
            attrs.insert("seven".to_string(), Value::NativeFn(Rc::new(NativeFn { name: "seven", f: seven })));
            attrs.insert("bound".to_string(), Value::Bound(Rc::new(BoundMethod { recv: items.clone(), name: "append" })));
            attrs.insert("slice".to_string(), Value::Slice(Rc::new((Value::Int(1), Value::None, Value::Int(2)))));
            attrs.insert("len".to_string(), Value::Builtin("len"));
        }
        let out = roundtrip(&[Value::Module(module.clone()), items]);
        let Value::Module(copy) = &out[0] else { panic!("not a module") };
        assert_eq!(copy.name, "image_mod");
        assert!(!Rc::ptr_eq(copy, &module));
        let attrs = copy.attrs.borrow();
        assert!(matches!(attrs.get("self"), Some(Value::Module(m)) if Rc::ptr_eq(m, copy)));
        let (Some(Value::List(items)), Value::List(root_items)) = (attrs.get("items"), &out[1]) else { panic!("not lists") };
        assert!(Rc::ptr_eq(items, root_items));
        assert!(matches!(attrs.get("bound"), Some(Value::Bound(b)) if b.name == "append"
            && matches!(&b.recv, Value::List(r) if Rc::ptr_eq(r, items))));
        assert!(matches!(attrs.get("slice"), Some(Value::Slice(s)) if int(&s.0) == 1 && matches!(s.1, Value::None) && int(&s.2) == 2));
        assert!(matches!(attrs.get("len"), Some(Value::Builtin("len"))));
        let Some(Value::NativeFn(native)) = attrs.get("seven") else { panic!("not a native function") };
        assert_eq!(native.name, "seven");
        let mut vm = Vm::new();
        assert_eq!(int(&(native.f)(&mut vm, Vec::new(), Vec::new()).ok().expect("call")), 7);
        drop(attrs);
        // Desfaz os ciclos para os `Rc` não vazarem nos testes.
        copy.attrs.borrow_mut().clear();
        module.attrs.borrow_mut().clear();
    }

    #[test]
    fn functions_keep_closures_defaults_attrs_and_shared_code() {
        let mut vm = run(
            "def make(n):\n    count = [n]\n    def inc(step=1, *, scale=2):\n        count[0] += step * scale\n        return count[0]\n    return inc\n\
             def outer():\n    a = 1\n    def mid():\n        b = 2\n        def inner():\n            return a + b\n        return inner\n    return mid()\n\
             def add(a, b=5):\n    return a + b\n\
             def anon():\n    return lambda: 1\n\
             f = make(10)\ndeep = outer()\nadd.tag = 'x'\nla = anon()\nlb = anon()\n",
        );
        let out = restore_globals(&vm, &["f", "deep", "add", "la", "lb"]);
        // A closure restaurada tem o `count` próprio: duas chamadas avançam só a cópia.
        assert_eq!(int(&call(&mut vm, &out[0], Vec::new())), 12);
        let kw = vec![("scale".to_string(), Value::Int(1))];
        assert_eq!(int(&vm.call(&out[0], vec![Value::Int(3)], kw).ok().expect("call")), 15);
        let original = global(&vm, "f");
        assert_eq!(int(&call(&mut vm, &original, Vec::new())), 12);
        // Três níveis de escopo.
        assert_eq!(int(&call(&mut vm, &out[1], Vec::new())), 3);
        // `defaults` e atributos da função.
        assert_eq!(int(&call(&mut vm, &out[2], vec![Value::Int(1)])), 6);
        assert_eq!(int(&call(&mut vm, &out[2], vec![Value::Int(1), Value::Int(2)])), 3);
        let add = function_of(&out[2]);
        assert_eq!(add.defaults.len(), 1);
        assert_eq!(text(add.attrs.borrow().get("tag").expect("tag")), "x");
        // O `Code` de duas funções do mesmo `def` é um só, e o texto compilado é igual ao do original.
        let (la, lb) = (function_of(&out[3]), function_of(&out[4]));
        assert!(Rc::ptr_eq(&la.code, &lb.code));
        let original_add = function_of(&global(&vm, "add"));
        assert_eq!(add.code.ops, original_add.code.ops);
        assert_eq!(add.code.lines, original_add.code.lines);
        assert_eq!(add.code.params.iter().map(|p| p.to_string()).collect::<Vec<_>>(), vec!["a", "b"]);
        assert!(!Rc::ptr_eq(&add.code, &original_add.code));
        // As globais são uma tabela só, e ela contém as próprias funções (ciclo).
        let f = function_of(&out[0]);
        assert!(Rc::ptr_eq(&f.globals, &add.globals));
        assert!(!Rc::ptr_eq(&add.globals, &vm.globals));
        assert!(matches!(add.globals.borrow().get("add"), Some(Value::Function(a)) if Rc::ptr_eq(a, &add)));
    }

    #[test]
    fn classes_keep_bases_metaclass_descriptors_and_instances() {
        let mut vm = run(
            "class Base:\n    kind = 'base'\n    def __init__(self, x):\n        self.x = x\n    def get(self):\n        return self.x\n\
             \x20   @property\n    def double(self):\n        return self.x * 2\n\
             \x20   @classmethod\n    def make(cls, x):\n        return cls(x)\n\
             \x20   @staticmethod\n    def twice(v):\n        return v * 2\n\
             class Child(Base):\n    def get(self):\n        return super().get() + 1\n\
             class Meta(type):\n    tag = 'm'\n\
             class WithMeta(metaclass=Meta):\n    pass\n\
             class P:\n    __slots__ = ('a', 'b')\n    def __init__(self):\n        self.a = 1\n        self.b = [self.a]\n\
             obj = Child(20)\nobj.me = obj\nbound = obj.get\np = P()\n",
        );
        let out = restore_globals(&vm, &["obj", "Child", "Base", "Meta", "WithMeta", "P", "p", "bound"]);
        let (obj, child, base) = (instance_of(&out[0]), class_of(&out[1]), class_of(&out[2]));
        // Herança, MRO e a classe da instância.
        assert!(Rc::ptr_eq(&obj.class(), &child));
        assert!(Rc::ptr_eq(&child.bases[0], &base));
        assert!(Rc::ptr_eq(&child.mro()[1], &base));
        assert!(matches!(obj.dict.borrow().get("me"), Some(Value::Instance(me)) if Rc::ptr_eq(me, &obj)));
        // Subclasses fracas apontam para a cópia.
        let subclasses = base.subclasses.borrow();
        assert_eq!(subclasses.len(), 1);
        assert!(Rc::ptr_eq(&subclasses[0].upgrade().expect("alive"), &child));
        drop(subclasses);
        // Métodos, `super()` (célula `__class__`), `property`, `classmethod` e `staticmethod`.
        let get = attr(&mut vm, &out[0], "get");
        assert_eq!(int(&call(&mut vm, &get, Vec::new())), 21);
        assert_eq!(int(&attr(&mut vm, &out[0], "double")), 40);
        assert_eq!(text(&attr(&mut vm, &out[1], "kind")), "base");
        let make = attr(&mut vm, &out[1], "make");
        let made = call(&mut vm, &make, vec![Value::Int(5)]);
        assert!(matches!(&made, Value::Instance(i) if Rc::ptr_eq(&i.class(), &child)));
        assert_eq!(int(&attr(&mut vm, &made, "x")), 5);
        let twice = attr(&mut vm, &out[2], "twice");
        assert_eq!(int(&call(&mut vm, &twice, vec![Value::Int(4)])), 8);
        // O método preso carrega a instância copiada.
        let Value::BoundFn(bound) = &out[7] else { panic!("not a bound method") };
        assert!(matches!(&bound.0, Value::Instance(i) if Rc::ptr_eq(i, &obj)));
        assert_eq!(int(&call(&mut vm, &out[7], Vec::new())), 21);
        // A cópia é independente do original.
        obj.set_own("x", Value::Int(1));
        assert_eq!(int(&call(&mut vm, &get, Vec::new())), 2);
        let original_get = {
            let original = global(&vm, "obj");
            attr(&mut vm, &original, "get")
        };
        assert_eq!(int(&call(&mut vm, &original_get, Vec::new())), 21);
        // Metaclasse.
        let (meta, with_meta) = (class_of(&out[3]), class_of(&out[4]));
        assert!(meta.is_meta);
        assert!(with_meta.meta.as_ref().is_some_and(|m| Rc::ptr_eq(m, &meta)));
        // `__slots__` e o estado da instância.
        let (slotted, p) = (class_of(&out[5]), instance_of(&out[6]));
        assert!(slotted.slots_allow("a"));
        assert!(!slotted.slots_allow("z"));
        assert_eq!(int(p.dict.borrow().get("a").expect("a")), 1);
        assert_eq!(list_of(p.dict.borrow().get("b").expect("b")).len(), 1);
    }

    #[test]
    fn complex_numbers_are_instances_of_a_python_class() {
        let mut vm = run("z = 3 + 4j\nw = z * z\n");
        // A classe mora no `builtins`, e por ele a imagem alcança os módulos carregados (alguns com geradores
        // vivos): a restauração precisa de uma `Vm`, como a do `os.fork`.
        let out = restore_globals_in(&vm, &["z", "w"]);
        let (z, w) = (instance_of(&out[0]), instance_of(&out[1]));
        assert!(Rc::ptr_eq(&z.class(), &w.class()));
        assert!(matches!(attr(&mut vm, &out[0], "real"), Value::Float(f) if f == 3.0));
        assert!(matches!(attr(&mut vm, &out[0], "imag"), Value::Float(f) if f == 4.0));
        assert!(matches!(attr(&mut vm, &out[1], "real"), Value::Float(f) if f == -7.0));
        assert!(matches!(attr(&mut vm, &out[1], "imag"), Value::Float(f) if f == 24.0));
        // É outra classe, não a do original.
        let original = instance_of(&global(&vm, "z"));
        assert!(!Rc::ptr_eq(&z.class(), &original.class()));
    }

    #[test]
    fn user_exception_subclass_keeps_its_args() {
        let mut vm = run("class MyErr(Exception):\n    pass\ne = MyErr('boom', 3)\n");
        let out = restore_globals(&vm, &["e", "MyErr"]);
        let (e, class) = (instance_of(&out[0]), class_of(&out[1]));
        assert!(Rc::ptr_eq(&e.class(), &class));
        let args = attr(&mut vm, &out[0], "args");
        assert_eq!(crate::object::repr(&args), "('boom', 3)");
    }

    #[test]
    fn exceptions_keep_args_chain_and_extra_through_cycles() {
        let a = Rc::new(ExcObj::new("ValueError", vec![Value::str("a"), Value::Int(1)]));
        let b = Rc::new(ExcObj::new("KeyError", vec![Value::str("b")]));
        a.chain.borrow_mut().context = Some(Value::Exception(b.clone()));
        {
            let mut chain = b.chain.borrow_mut();
            chain.cause = Some(Value::Exception(a.clone()));
            chain.suppress = true;
        }
        b.extra.borrow_mut().push(("name", Value::str("n")));
        let out = roundtrip(&[Value::Exception(a.clone()), Value::Exception(b.clone())]);
        let (Value::Exception(ra), Value::Exception(rb)) = (&out[0], &out[1]) else { panic!("not exceptions") };
        assert!(!Rc::ptr_eq(ra, &a));
        assert_eq!((ra.kind, rb.kind), ("ValueError", "KeyError"));
        assert_eq!(crate::object::repr(&out[0]), crate::object::repr(&Value::Exception(a.clone())));
        assert!(matches!(&ra.chain.borrow().context, Some(Value::Exception(c)) if Rc::ptr_eq(c, rb)));
        assert!(matches!(&rb.chain.borrow().cause, Some(Value::Exception(c)) if Rc::ptr_eq(c, ra)));
        assert!(rb.chain.borrow().suppress);
        assert!(ra.chain.borrow().cause.is_none());
        assert_eq!(text(&rb.extra_get("name").expect("name")), "n");
        // Desfaz os ciclos para os `Rc` não vazarem nos testes.
        for e in [&a, &b, ra, rb] {
            *e.chain.borrow_mut() = ExcChain::default();
        }
    }

    // ---- H3: iteradores, geradores, tracebacks, weakref ----

    fn same_ext(a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::Ext(x), Value::Ext(y)) => std::ptr::addr_eq(Rc::as_ptr(x), Rc::as_ptr(y)),
            _ => false,
        }
    }

    /// Um passo do iterador `v`.
    fn step(v: &Value) -> Option<Value> {
        let Value::Ext(e) = v else { panic!("not an iterator") };
        e.iter_next().ok().expect("step")
    }

    /// O texto de cada item que o iterador `v` ainda entrega.
    fn drain(v: &Value) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(item) = step(v) {
            out.push(crate::object::repr(&item));
        }
        out
    }

    /// Como `restore_globals`, para uma imagem com gerador, que precisa de uma `Vm` onde assentar.
    fn restore_globals_in(vm: &Vm, names: &[&str]) -> Vec<Value> {
        let roots: Vec<Value> = names.iter().map(|n| global(vm, n)).collect();
        HeapImage::capture(&roots).ok().expect("capture").restore_in(Some(vm)).ok().expect("restore")
    }

    #[test]
    fn lazy_iterators_resume_at_their_position() {
        let vm = run(
            "a = iter([10, 20, 30])\nnext(a)\n\
             m = map(lambda v: v + 1, [1, 2, 3])\nnext(m)\n\
             z = zip('ab', [1, 2, 3])\n\
             e = enumerate(['x', 'y'], 5)\nnext(e)\n\
             f = filter(None, [0, 1, 0, 2])\n\
             r = reversed([1, 2, 3])\nnext(r)\n\
             k = iter(range(3))\nnext(k)\n\
             d = iter({'p': 1, 'q': 2})\nnext(d)\n",
        );
        let out = restore_globals_in(&vm, &["a", "m", "z", "e", "f", "r", "k", "d"]);
        let expected: [&[&str]; 8] = [
            &["20", "30"],
            &["3", "4"],
            &["('a', 1)", "('b', 2)"],
            &["(6, 'y')"],
            &["1", "2"],
            &["2", "1"],
            &["1", "2"],
            &["'q'"],
        ];
        for (value, want) in out.iter().zip(expected) {
            assert_eq!(drain(value), want);
        }
        // O original continua de onde estava: a cópia não o consumiu.
        assert_eq!(drain(&global(&vm, "a")), ["20", "30"]);
    }

    #[test]
    fn generator_resumes_where_it_stopped() {
        let mut vm = run(
            "def gen(n):\n    for i in range(n):\n        got = yield i * 10\n    return 'done'\n\
             def guarded():\n    try:\n        x = yield 1\n        yield x\n    finally:\n        pass\n\
             g = gen(4)\nnext(g)\nnext(g)\nboth = [g, g]\n\
             h = guarded()\nnext(h)\n",
        );
        let out = restore_globals_in(&vm, &["g", "both", "h"]);
        // O laço `for` suspenso guarda o iterador de `range` na pilha do quadro.
        assert_eq!(step(&out[0]).map(|v| int(&v)), Some(20));
        assert_eq!(step(&out[0]).map(|v| int(&v)), Some(30));
        assert!(step(&out[0]).is_none());
        // O mesmo gerador visto por dois nomes continua um objeto só.
        let pair = list_of(&out[1]);
        assert!(same_ext(&pair[0], &pair[1]));
        // O original não foi consumido pela cópia.
        assert_eq!(step(&global(&vm, "g")).map(|v| int(&v)), Some(20));
        // `send` entrega o valor no ponto do `yield`, dentro de um bloco protegido.
        let Value::Ext(h) = &out[2] else { panic!("not a generator") };
        let sent = h.call_method(&mut vm, "send", vec![Value::Int(7)], Vec::new()).ok().expect("send");
        assert_eq!(int(&sent), 7);
    }

    #[test]
    fn generator_needs_a_vm_to_restore_into() {
        let vm = run("def gen():\n    yield 1\ng = gen()\n");
        let image = HeapImage::capture(&[global(&vm, "g")]).ok().expect("capture");
        assert_eq!(image.restore().err(), Some(ImageError::Malformed("a generator needs the Vm to restore into")));
    }

    #[test]
    fn traceback_round_trips_with_the_exception_that_owns_it() {
        let mut vm = run(
            "def boom():\n    return 1 / 0\ntry:\n    boom()\nexcept ZeroDivisionError as e:\n    saved = e\n    tb = e.__traceback__\n",
        );
        let out = restore_globals(&vm, &["saved", "tb"]);
        let original = global(&vm, "tb");
        assert!(matches!(original, Value::Ext(_)));
        let line = |vm: &mut Vm, v: &Value| int(&attr(vm, v, "tb_lineno"));
        assert_eq!(line(&mut vm, &out[1]), line(&mut vm, &original));
        let next = attr(&mut vm, &out[1], "tb_next");
        let next_original = attr(&mut vm, &original, "tb_next");
        assert_eq!(line(&mut vm, &next), line(&mut vm, &next_original));
        // A exceção e o traceback capturados juntos continuam apontando um para o outro.
        let owned = attr(&mut vm, &out[0], "__traceback__");
        assert!(same_ext(&owned, &out[1]));
    }

    #[test]
    fn weak_references_follow_their_referent() {
        let mut vm = run("import _weakref\nclass C:\n    pass\nc = C()\nr = _weakref.ref(c)\ndead = _weakref.ref(C())\n");
        let out = restore_globals(&vm, &["c", "r", "dead"]);
        let alive = call(&mut vm, &out[1], Vec::new());
        assert!(matches!((&alive, &out[0]), (Value::Instance(a), Value::Instance(b)) if Rc::ptr_eq(a, b)));
        assert!(matches!(call(&mut vm, &out[2], Vec::new()), Value::None));
    }

    // ---- H4: Native e objetos nativos com estado ----

    #[test]
    fn native_files_and_csv_round_trip() {
        let vm = Vm::new();
        let source = Value::list(vec![Value::str("a,b")]);
        let mut reader = Reader::new(Dialect::default());
        reader.line_num = 3;
        let csv_reader = Value::Native(Rc::new(RefCell::new(Native::CsvReader { reader, src: source.clone() })));
        let dialect = Dialect { delimiter: u32::from(b';'), ..Dialect::default() };
        let csv_writer = Value::Native(Rc::new(RefCell::new(Native::CsvWriter { dialect, target: source.clone() })));
        let stdout = Value::Native(vm.std_files[1].clone());
        let out = roundtrip(&[csv_reader, csv_writer, stdout, source]);
        let (Value::Native(r), Value::Native(w), Value::Native(f), Value::List(l)) = (&out[0], &out[1], &out[2], &out[3]) else {
            panic!("unexpected roots")
        };
        match &*r.borrow() {
            Native::CsvReader { reader, src } => {
                assert_eq!(reader.line_num, 3);
                assert!(matches!(src, Value::List(s) if Rc::ptr_eq(s, l)));
            }
            _ => panic!("not a csv reader"),
        }
        match &*w.borrow() {
            Native::CsvWriter { dialect, target } => {
                assert_eq!(dialect.delimiter, u32::from(b';'));
                assert!(matches!(target, Value::List(s) if Rc::ptr_eq(s, l)));
            }
            _ => panic!("not a csv writer"),
        }
        match &*f.borrow() {
            Native::File(file) => {
                assert_eq!(file.name, "<stdout>");
                assert_eq!(file.kind, FileKind::Stdout);
            }
            _ => panic!("not a file"),
        }
    }

    #[test]
    fn native_hash_and_re_objects_round_trip() {
        let mut vm = run(
            "import hashlib\nimport re\nh = hashlib.sha256()\nh.update(b'abc')\n\
             p = re.compile(r'(?P<w>[a-z]+)\\d')\nm = p.match('ab12')\n\
             it = p.finditer('x1 y2')\nnext(it)\n",
        );
        let out = restore_globals(&vm, &["h", "p", "m", "it"]);
        let digest = |vm: &mut Vm, v: &Value| {
            let f = attr(vm, v, "hexdigest");
            crate::object::repr(&call(vm, &f, Vec::new()))
        };
        let original = global(&vm, "h");
        assert_eq!(digest(&mut vm, &out[0]), digest(&mut vm, &original));
        // O resumo refeito é uma cópia: alimentá-lo não mexe no original.
        let update = attr(&mut vm, &out[0], "update");
        call(&mut vm, &update, vec![Value::bytes(b"d".to_vec())]);
        assert_ne!(digest(&mut vm, &out[0]), digest(&mut vm, &original));
        // O padrão é recompilado; o `Match` e o `finditer` apontam para o padrão refeito.
        let original_pattern = global(&vm, "p");
        let (restored, original) = (attr(&mut vm, &out[1], "pattern"), attr(&mut vm, &original_pattern, "pattern"));
        assert_eq!(crate::object::repr(&restored), crate::object::repr(&original));
        let group = attr(&mut vm, &out[2], "group");
        assert_eq!(crate::object::repr(&call(&mut vm, &group, vec![Value::str("w")])), "'ab'");
        assert!(same_ext(&attr(&mut vm, &out[2], "re"), &out[1]));
        assert_eq!(drain(&out[3]), ["<re.Match object; span=(3, 5), match='y2'>"]);
    }

    // ---- H5: a Vm ----

    #[test]
    fn vm_image_rebuilds_the_vm_in_another_thread() {
        let vm = run("import sys\nimport re\nimport hashlib\nx = [1, 2]\ndef f():\n    return x\ng = f\nh = hashlib.sha256(b'abc')\n");
        vm.stdout.borrow_mut().extend_from_slice(b"pending");
        let modules = vm.modules.borrow().len();
        let image = VmImage::capture(&vm).ok().expect("capture");
        assert!(!image.is_empty());
        // A imagem atravessa a thread, e a `Vm` refeita nasce nela: é o que o filho do `os.fork` faz.
        let report = std::thread::spawn(move || {
            let mut vm = image.restore().ok().expect("restore");
            let f = vm.globals.borrow().get("g").cloned().expect("g");
            let result = vm.call(&f, Vec::new(), Vec::new()).ok().expect("call");
            let main = vm.module_globals.borrow().get("__main__").cloned().expect("__main__");
            (
                crate::object::repr(&result),
                vm.stdout.borrow().clone(),
                Rc::ptr_eq(&main, &vm.globals),
                vm.modules.borrow().len(),
                crate::vm::current().is_some(),
            )
        })
        .join()
        .expect("thread");
        assert_eq!(report.0, "[1, 2]");
        assert_eq!(report.1, b"pending");
        assert!(report.2);
        assert_eq!(report.3, modules);
        assert!(report.4);
    }
}
