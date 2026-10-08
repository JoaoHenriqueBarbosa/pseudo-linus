//! O objeto `frame` do CPython 3.13 (`sys._getframe`, `tb_frame`, o argumento dos rastreadores do
//! `sys.settrace`) e o `FrameLocalsProxy` de `frame.f_locals`.
//!
//! Um quadro vivo tem identidade: `sys._getframe() is sys._getframe()` e o `frame` que o
//! rastreador recebe são o mesmo objeto, e `f_trace`, `f_trace_lines` e `f_trace_opcodes` ficam
//! guardados nele (é o que o `bdb` usa para ligar o rastreio nos quadros de cima). A identidade vem
//! de um registro por thread, chaveado pelo `Env` do quadro quando a VM o informa, ou pela posição
//! (índice no empilhamento, código e linha do chamador) quando não informa. Quadros de traceback
//! (`tb_frame`) são avulsos: já terminaram, não entram no registro.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::{Rc, Weak};

use crate::compile::{comp_cell_depth, comp_hidden_parts, Code};
use crate::object::{py_addr, Dict, Env, ExcObj, ExtImage, ExtObject, Kw, Value};
use crate::vm::{exc, type_error, PyException, PyResult, Vm};

/// O que o chamador sabe de um quadro, do mais interno para o mais externo numa cadeia.
pub struct FrameLink {
    /// Linha em execução (no quadro mais externo, a linha da chamada).
    pub line: usize,
    pub name: String,
    pub file: Rc<str>,
    /// O código compilado; `None` no `<module>` e nos quadros de traceback.
    pub code: Option<Rc<Code>>,
    /// O escopo das variáveis locais, quando a VM o expõe.
    pub env: Option<Rc<Env>>,
    /// A linha do chamador no momento da chamada (parte da chave de identidade sem `env`).
    pub caller_line: usize,
}

/// Chave do registro: o `Env` (endereço) ou a posição no empilhamento.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    Env(usize),
    Pos(usize, usize, usize),
}

pub struct FrameObj {
    line: Cell<usize>,
    name: String,
    file: Rc<str>,
    code: Option<Rc<Code>>,
    code_object: Value,
    env: RefCell<Weak<Env>>,
    /// O escopo de um quadro de traceback: ele mantém o quadro vivo, e com ele as variáveis locais.
    held: RefCell<Option<Rc<Env>>>,
    back: RefCell<Value>,
    /// Ainda em execução (vem do empilhamento da VM); `false` nos quadros de traceback.
    live: bool,
    trace: RefCell<Value>,
    trace_lines: Cell<bool>,
    trace_opcodes: Cell<bool>,
    /// Última linha vista pelo gancho de instrução (`UNSEEN` até a primeira).
    last_line: Cell<usize>,
}

/// `last_line` de um quadro que o gancho de instrução ainda não observou.
const UNSEEN: usize = usize::MAX;

/// O estado de um `frame` como dado, para a imagem do heap (`ExtImage::Frame`): tudo o que o objeto guarda,
/// com os escopos e o código como o `Rc` que o objeto referencia.
pub struct FrameParts {
    pub line: usize,
    pub name: String,
    pub file: Rc<str>,
    pub code: Option<Rc<Code>>,
    pub code_object: Value,
    /// O escopo, se ainda está vivo.
    pub env: Option<Rc<Env>>,
    pub held: Option<Rc<Env>>,
    pub back: Value,
    pub live: bool,
    pub trace: Value,
    pub trace_lines: bool,
    pub trace_opcodes: bool,
    pub last_line: usize,
}

/// Refaz um `frame` da imagem do heap, sem `f_back` nem `f_trace` (podem apontar de volta para o próprio
/// quadro: [`restore_links`] os põe depois). Um quadro vivo com escopo volta ao registro por thread, com a
/// chave do `Env` novo, para `sys._getframe()` e os rastreadores continuarem a enxergar o mesmo objeto. Os
/// quadros vivos sem escopo (a chave é a posição na pilha) não voltam ao registro.
pub fn frame_from_image(p: FrameParts) -> Value {
    let env = p.env;
    let obj = Rc::new(FrameObj {
        line: Cell::new(p.line),
        name: p.name,
        file: p.file,
        code: p.code,
        code_object: p.code_object,
        env: RefCell::new(env.as_ref().map_or_else(Weak::new, Rc::downgrade)),
        held: RefCell::new(p.held),
        back: RefCell::new(Value::None),
        live: p.live,
        trace: RefCell::new(Value::None),
        trace_lines: Cell::new(p.trace_lines),
        trace_opcodes: Cell::new(p.trace_opcodes),
        last_line: Cell::new(p.last_line),
    });
    if let (true, Some(env)) = (p.live, &env) {
        REGISTRY.with(|reg| reg.borrow_mut().insert(Key::Env(Rc::as_ptr(env) as usize), Entry { obj: obj.clone() }));
    }
    Value::Ext(obj)
}

/// A terceira passada de um `frame` refeito: `f_back` e `f_trace`.
pub fn restore_links(frame: &Value, back: Value, trace: Value) {
    with_frame(frame, |f| {
        *f.back.borrow_mut() = back;
        *f.trace.borrow_mut() = trace;
    });
}

struct Entry {
    obj: Rc<FrameObj>,
}

thread_local! {
    static REGISTRY: RefCell<HashMap<Key, Entry>> = RefCell::new(HashMap::new());
    /// As globais em que roda cada quadro de módulo de `exec`/`eval` (chave: ponteiro do `Env`).
    static MODULE_GLOBALS: RefCell<HashMap<usize, Rc<RefCell<crate::object::VarMap>>>> = RefCell::new(HashMap::new());
    /// O quadro (ponteiro do `Env`) que está recebendo um evento `line` agora: só ele aceita `f_lineno = n`.
    static JUMP_WINDOW: Cell<Option<usize>> = const { Cell::new(None) };
    /// A linha pedida por `f_lineno = n` durante o evento, já resolvida para uma linha com código.
    static JUMP_TARGET: Cell<Option<usize>> = const { Cell::new(None) };
}

/// O quadro de módulo `env`, de `exec`/`eval`, roda nas globais `globals`.
pub fn bind_globals(env: &Rc<Env>, globals: &Rc<RefCell<crate::object::VarMap>>) {
    MODULE_GLOBALS.with(|m| m.borrow_mut().insert(Rc::as_ptr(env) as usize, globals.clone()));
}

/// O quadro de módulo `env` terminou.
pub fn unbind_globals(env: &Rc<Env>) {
    MODULE_GLOBALS.with(|m| m.borrow_mut().remove(&(Rc::as_ptr(env) as usize)));
}

/// Abre a janela em que o quadro `at` aceita `f_lineno = n` (o evento `line` vai ser entregue).
pub fn open_jump(at: usize) {
    JUMP_WINDOW.with(|w| w.set(Some(at)));
    JUMP_TARGET.with(|t| t.set(None));
}

/// Fecha a janela e devolve a linha para onde o rastreador mandou o quadro `at`, se mandou.
pub fn close_jump(at: usize) -> Option<usize> {
    JUMP_WINDOW.with(|w| w.set(None));
    let _ = at;
    JUMP_TARGET.with(Cell::take)
}

/// O quadro `at` caiu na linha `line` por um salto: o evento `line` dela já foi dado ao rastreador.
pub fn line_landed(at: usize, line: usize) {
    line_changed(Some(at), line);
}

impl FrameObj {
    fn new(link: &FrameLink, live: bool) -> FrameObj {
        let code_object = match &link.code {
            Some(c) => crate::tbobj::function_code(c, &link.file),
            None => crate::tbobj::code_object(&link.name, &link.file),
        };
        FrameObj {
            line: Cell::new(link.line),
            name: link.name.clone(),
            file: link.file.clone(),
            code: link.code.clone(),
            code_object,
            env: RefCell::new(link.env.as_ref().map_or_else(Weak::new, Rc::downgrade)),
            held: RefCell::new(if live { None } else { link.env.clone() }),
            back: RefCell::new(Value::None),
            live,
            trace: RefCell::new(Value::None),
            trace_lines: Cell::new(true),
            trace_opcodes: Cell::new(false),
            last_line: Cell::new(UNSEEN),
        }
    }

    fn env(&self) -> Option<Rc<Env>> {
        self.env.borrow().upgrade()
    }

    /// `frame.f_lineno = n` dentro de um evento `line`: pede à VM para continuar na primeira linha com
    /// código a partir de `n`. Fora de um rastreador, ou noutro quadro, é `ValueError`.
    fn jump_to(&self, value: &Value) -> Result<(), PyException> {
        let Value::Int(wanted) = value else {
            return Err(exc("ValueError", "lineno must be an integer"));
        };
        let at = self.env().map(|e| Rc::as_ptr(&e) as usize);
        let (Some(at), Some(code)) = (at, self.code.as_ref()) else {
            return Err(exc("ValueError", "f_lineno can only be set by a trace function"));
        };
        if !self.live || JUMP_WINDOW.with(Cell::get) != Some(at) {
            return Err(exc("ValueError", "f_lineno can only be set by a trace function"));
        }
        let first = code.lines.iter().copied().min().unwrap_or(0).min(code.first_line.max(1));
        let found = code.lines.iter().copied().filter(|l| *l as i64 >= *wanted).min();
        let Some(line) = found else {
            return Err(exc("ValueError", format!("line {wanted} comes after the current code block")));
        };
        if *wanted < first as i64 {
            return Err(exc("ValueError", format!("line {wanted} comes before the current code block")));
        }
        self.line.set(line);
        JUMP_TARGET.with(|t| t.set(Some(line)));
        Ok(())
    }

    fn globals(&self, vm: &mut Vm) -> Value {
        // Quadro de módulo de `exec`/`eval`: as globais em que ele roda.
        let bound = self.env().and_then(|e| MODULE_GLOBALS.with(|m| m.borrow().get(&(Rc::as_ptr(&e) as usize)).cloned()));
        if let Some(map) = bound {
            return crate::globalsview::view_for(&map, None);
        }
        // As globais do módulo cujo `__file__` é o do quadro; sem arquivo que case (o `-c`, `<string>`),
        // as do `__main__`. As globais correntes da VM não servem: dentro de uma função de outro módulo
        // elas são as daquele módulo, não as do quadro que se pediu.
        let found = vm.module_globals.borrow().values().find_map(|g| {
            let matches = match g.borrow().get("__file__") {
                Some(Value::Str(f)) => f.as_str() == &*self.file,
                _ => false,
            };
            matches.then(|| g.clone())
        });
        let main = || vm.module_globals.borrow().get("__main__").cloned();
        crate::globalsview::view_for(&found.or_else(main).unwrap_or_else(|| vm.globals.clone()), None)
    }

    /// `f_locals`: as globais no `<module>` e no corpo de módulo; nas funções, o proxy sobre as
    /// variáveis locais (um dict vazio se a VM não expôs o escopo do quadro).
    fn locals(&self, vm: &mut Vm) -> Value {
        match (self.env(), &self.code) {
            (Some(env), Some(code)) if !env.is_module => {
                Value::Ext(Rc::new(FrameLocalsProxy { env, code: code.clone() }))
            }
            (None, Some(_)) => Value::dict(Dict::default()),
            _ => self.globals(vm),
        }
    }
}

/// Posição do quadro `link` (de índice `outer` a partir do mais externo) no registro.
fn key_of(link: &FrameLink, outer: usize) -> Key {
    match &link.env {
        Some(e) => Key::Env(Rc::as_ptr(e) as usize),
        None => Key::Pos(outer, link.code.as_ref().map_or(0, |c| Rc::as_ptr(c) as usize), link.caller_line),
    }
}

/// O quadro `depth` níveis acima do mais interno de `chain` (0 é o mais interno), com identidade.
/// `fresh_innermost` descarta o registro do mais interno: ele acabou de ser chamado, e um quadro
/// anterior da mesma posição (mesmo código, mesmo ponto de chamada) não é o mesmo quadro.
pub fn live_frame_at(chain: Vec<FrameLink>, depth: usize, fresh_innermost: bool) -> Option<Value> {
    if depth >= chain.len() {
        return None;
    }
    let n = chain.len();
    let mut result = None;
    let mut back = Value::None;
    let mut positions: HashSet<Key> = HashSet::new();
    REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        for (i, link) in chain.into_iter().enumerate().rev() {
            let key = key_of(&link, n - 1 - i);
            if i == 0 && fresh_innermost {
                reg.remove(&key);
            }
            if matches!(key, Key::Pos(..)) {
                positions.insert(key);
            }
            let obj = match reg.get(&key) {
                Some(e) => e.obj.clone(),
                None => {
                    let obj = Rc::new(FrameObj::new(&link, true));
                    reg.insert(key, Entry { obj: obj.clone() });
                    obj
                }
            };
            obj.line.set(link.line);
            *obj.env.borrow_mut() = link.env.as_ref().map_or_else(Weak::new, Rc::downgrade);
            *obj.back.borrow_mut() = back;
            back = Value::Ext(obj);
            if i == depth {
                result = Some(back.clone());
            }
        }
        // Quadros que já saíram: o `Env` morreu, ou a posição não está mais no empilhamento.
        reg.retain(|k, e| match k {
            Key::Env(_) => e.obj.env.borrow().strong_count() > 0,
            Key::Pos(..) => positions.contains(k),
        });
    });
    result
}

/// Um quadro de traceback (`tb_frame`): terminou, não tem identidade nem escopo.
pub fn detached_frame(link: &FrameLink) -> Value {
    Value::Ext(Rc::new(FrameObj::new(link, false)))
}

/// O quadro de um gerador, de uma corrente ou de um gerador assíncrono (`gi_frame`, `cr_frame`,
/// `ag_frame`): o mesmo objeto que os rastreadores recebem a cada retomada. Parado, mostra a linha
/// em que parou (`link.line`) e não tem `f_back`; em execução, fica como a cadeia viva o deixou.
pub fn generator_frame(link: &FrameLink, running: bool) -> Value {
    REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        let obj = reg.entry(key_of(link, 0)).or_insert_with(|| Entry { obj: Rc::new(FrameObj::new(link, true)) }).obj.clone();
        if !running {
            obj.line.set(link.line);
            *obj.back.borrow_mut() = Value::None;
        }
        Value::Ext(obj)
    })
}

/// O gerador de ambiente `env` suspendeu: o quadro dele deixa de ter `f_back`.
pub fn suspend_frame(env: &Rc<Env>) {
    REGISTRY.with(|reg| {
        if let Some(e) = reg.borrow().get(&Key::Env(Rc::as_ptr(env) as usize)) {
            *e.obj.back.borrow_mut() = Value::None;
        }
    });
}

/// Chave do quadro em execução: o `Env` (ponteiro) numa função, a raiz no `<module>`.
fn key_of_env(env: Option<usize>) -> Key {
    env.map_or(Key::Pos(0, 0, 0), Key::Env)
}

/// Uma função cujo escopo `env` uma função de dentro capturou devolveu: como no CPython, só as células
/// (`co_cellvars`) sobrevivem ao quadro, e os demais locais morrem aqui. Se alguém tem o objeto `frame`
/// dela (`sys._getframe`, traceback), o quadro inteiro continua vivo, com todos os locais.
pub fn release_locals(code: &Code, env: &Rc<Env>) {
    if !code.is_function || Rc::strong_count(env) == 1 {
        return;
    }
    if REGISTRY.with(|reg| reg.borrow().contains_key(&Key::Env(Rc::as_ptr(env) as usize))) {
        return;
    }
    // O que sai é solto depois do empréstimo: um `__del__` pode voltar a este escopo.
    let dead: Vec<Value> = {
        let mut vars = env.vars.borrow_mut();
        let names: Vec<Rc<str>> = vars.keys().filter(|k| !code.cellvars.contains(k)).cloned().collect();
        names.iter().filter_map(|k| vars.remove(k)).collect()
    };
    drop(dead);
}

/// O quadro em execução (`env` como em [`line_changed`]) já tem identidade e um `f_trace`? Não cria
/// o quadro: quem ninguém pediu por `sys._getframe` nem recebeu num evento não tem rastreador.
pub fn is_traced(env: Option<usize>) -> bool {
    REGISTRY.with(|reg| reg.borrow().get(&key_of_env(env)).is_some_and(|e| !matches!(*e.obj.trace.borrow(), Value::None)))
}

/// O `f_trace` de um quadro (`None` se `frame` não for um `frame`).
pub fn trace_of(frame: &Value) -> Value {
    with_frame(frame, |f| f.trace.borrow().clone()).unwrap_or(Value::None)
}

pub fn set_trace_of(frame: &Value, tracer: Value) {
    with_frame(frame, |f| *f.trace.borrow_mut() = tracer);
}

/// `f_trace_lines`: os eventos `line` do quadro estão ligados?
pub fn trace_lines_of(frame: &Value) -> bool {
    with_frame(frame, |f| f.trace_lines.get()).unwrap_or(true)
}

/// Quadro recém-chamado: o primeiro `line` dispara na primeira instrução.
pub fn arm_first_line(frame: &Value) {
    with_frame(frame, |f| f.last_line.set(0));
}

/// O gancho de instrução chegou à linha `line` do quadro em execução (`Some(ponteiro do Env)` numa
/// função, `None` no `<module>`). Diz se a linha mudou desde a última observada. Quadro ainda não
/// observado só guarda a linha (é o chamador de `pdb.set_trace()`: o resto da linha da chamada não
/// gera evento). Quadro que ninguém pediu por `sys._getframe` não está no registro e não tem `f_trace`.
pub fn line_changed(env: Option<usize>, line: usize) -> bool {
    let frame = REGISTRY.with(|reg| reg.borrow().get(&key_of_env(env)).map(|e| e.obj.clone()));
    frame.is_some_and(|f| {
        let before = f.last_line.replace(line);
        before != UNSEEN && before != line
    })
}

fn with_frame<R>(frame: &Value, f: impl FnOnce(&FrameObj) -> R) -> Option<R> {
    let Value::Ext(e) = frame else { return None };
    e.as_any().and_then(|a| a.downcast_ref::<FrameObj>()).map(f)
}

/// Volta de laço (salto para trás) no quadro de chave `env`: o CPython dispara `line` ao chegar no
/// alvo mesmo quando ele está na mesma linha de onde se saltou, então a próxima instrução conta como
/// linha nova. Quadro ainda não observado continua assim.
pub fn back_edge(env: Option<usize>) {
    let frame = REGISTRY.with(|reg| reg.borrow().get(&key_of_env(env)).map(|e| e.obj.clone()));
    if let Some(f) = frame.filter(|f| f.last_line.get() != UNSEEN) {
        f.last_line.set(0);
    }
}

fn read_only(name: &str) -> PyException {
    exc("AttributeError", format!("attribute '{name}' of 'frame' objects is not writable"))
}

fn want_bool(v: &Value) -> PyResult<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        _ => Err(type_error("attribute value type must be bool")),
    }
}

impl ExtObject for FrameObj {
    fn type_name(&self) -> &'static str {
        "frame"
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn image(&self) -> Option<ExtImage> {
        Some(ExtImage::Frame(FrameParts {
            line: self.line.get(),
            name: self.name.clone(),
            file: self.file.clone(),
            code: self.code.clone(),
            code_object: self.code_object.clone(),
            env: self.env(),
            held: self.held.borrow().clone(),
            back: self.back.borrow().clone(),
            live: self.live,
            trace: self.trace.borrow().clone(),
            trace_lines: self.trace_lines.get(),
            trace_opcodes: self.trace_opcodes.get(),
            last_line: self.last_line.get(),
        }))
    }

    fn repr(&self) -> String {
        format!(
            "<frame at {:#x}, file '{}', line {}, code {}>",
            py_addr(self as *const Self as usize),
            self.file,
            self.line.get(),
            self.name
        )
    }

    fn methods(&self) -> &'static [&'static str] {
        &["clear"]
    }

    fn getattr(&self, vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        Some(Ok(match name {
            "f_lineno" => Value::Int(self.line.get() as i64),
            "f_lasti" => Value::Int(self.code.as_ref().map_or(0, |c| crate::tbobj::synthetic_lasti(c, self.line.get())) as i64),
            "f_code" => self.code_object.clone(),
            "f_back" => self.back.borrow().clone(),
            "f_globals" => self.globals(vm),
            "f_locals" => self.locals(vm),
            "f_builtins" => crate::modules::builtins_dict(vm).unwrap_or_else(|| Value::dict(Dict::default())),
            "f_trace" => self.trace.borrow().clone(),
            "f_trace_lines" => Value::Bool(self.trace_lines.get()),
            "f_trace_opcodes" => Value::Bool(self.trace_opcodes.get()),
            _ => return None,
        }))
    }

    fn setattr(&self, name: &str, value: Value) -> Option<Result<(), PyException>> {
        Some(match name {
            "f_trace" => {
                *self.trace.borrow_mut() = value;
                Ok(())
            }
            "f_trace_lines" => want_bool(&value).map(|b| self.trace_lines.set(b)),
            "f_trace_opcodes" => want_bool(&value).map(|b| self.trace_opcodes.set(b)),
            "f_lineno" => self.jump_to(&value),
            "f_back" | "f_code" | "f_globals" | "f_locals" | "f_builtins" | "f_lasti" => Err(read_only(name)),
            _ => return None,
        })
    }

    fn delattr(&self, name: &str) -> Option<Result<(), PyException>> {
        Some(match name {
            "f_trace" => {
                *self.trace.borrow_mut() = Value::None;
                Ok(())
            }
            "f_trace_lines" | "f_trace_opcodes" | "f_lineno" => Err(type_error("cannot delete attribute")),
            "f_back" | "f_code" | "f_globals" | "f_locals" | "f_builtins" | "f_lasti" => Err(read_only(name)),
            _ => return None,
        })
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        debug_assert_eq!(name, "clear");
        if !args.is_empty() {
            return Err(type_error(format!("frame.clear() takes no arguments ({} given)", args.len())));
        }
        if self.live {
            return Err(exc("RuntimeError", "cannot clear an executing frame"));
        }
        *self.trace.borrow_mut() = Value::None;
        if let Some(env) = self.env() {
            env.vars.borrow_mut().retain(|_, _| false);
        }
        Ok(Value::None)
    }
}

/// As variáveis ligadas de um escopo: numa função, a ordem de `co_varnames` (parâmetros, depois os
/// locais na ordem da primeira aparição no corpo), as células que não são parâmetros e as variáveis
/// livres, como o `locals()` do CPython; num corpo de classe, a ordem de criação dos nomes. É a ordem
/// de `locals()` e do proxy. O alvo de uma compreensão inline em curso aparece com o nome dele, no
/// lugar da variável de fora de mesmo nome (a mais interna vale); o corpo de classe não o mostra.
pub fn locals_dict(code: &Code, env: &Env) -> PyResult<Dict> {
    let vars = env.vars.borrow();
    // Os alvos que uma função de dentro da compreensão fecha moram na célula dela, não em `vars`.
    let cells: Vec<(usize, Vec<(Rc<str>, Value)>)> = vars
        .iter()
        .filter_map(|(k, v)| Some((comp_cell_depth(k)?, crate::classes::cell_env(v)?)))
        .map(|(depth, cell)| (depth, cell.vars.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect()))
        .collect();
    let mut named: BTreeMap<&str, &Value> = BTreeMap::new();
    let mut targets: Vec<(usize, &str, &Value)> = Vec::new();
    for (depth, items) in &cells {
        targets.extend(items.iter().map(|(name, v)| (*depth, &**name, v)));
    }
    for (k, v) in vars.iter() {
        match comp_hidden_parts(k) {
            Some((depth, name)) => targets.push((depth, name, v)),
            None if comp_cell_depth(k).is_some() => {}
            None => {
                named.insert(k, v);
            }
        }
    }
    if !env.is_class {
        targets.sort_by_key(|t| t.0);
        named.extend(targets.into_iter().map(|(_, name, v)| (name, v)));
    }
    let mut d = Dict::default();
    if env.is_class {
        // Um nome apagado e criado de novo vai para o fim, como no dict do CPython.
        let order = env.order.borrow();
        let mut seen: HashSet<&str> = HashSet::new();
        let mut last_first: Vec<&str> = order.iter().rev().map(String::as_str).filter(|n| seen.insert(*n)).collect();
        last_first.reverse();
        for n in last_first {
            if let Some(v) = named.remove(n) {
                d.set(Value::str(n), v.clone())?;
            }
        }
    } else {
        // No módulo o `co_varnames` só guarda os alvos escondidos das compreensões: a ordem do espaço de nomes não é a dele.
        for n in code.varnames.iter().chain(&code.cellvars).filter(|_| code.is_function) {
            if let Some(v) = named.remove(&**n) {
                d.set(Value::str(&**n), v.clone())?;
            }
        }
        for n in &code.freevars {
            if let Some(v) = free_value(env, n) {
                d.set(Value::str(&**n), v)?;
            }
        }
        named.retain(|k, _| !k.starts_with('.'));
    }
    for (k, v) in named {
        d.set(Value::str(k), v.clone())?;
    }
    Ok(d)
}

/// O valor da variável livre `name`: a do escopo de função mais próximo, por fora de `env`, que a liga.
fn free_value(env: &Env, name: &str) -> Option<Value> {
    let mut cur = env.parent.clone();
    while let Some(e) = cur {
        if let Some(v) = e.vars.borrow().get(name) {
            return Some(v.clone());
        }
        cur = e.parent.clone();
    }
    None
}

/// `FrameLocalsProxy` (3.13): mapeamento sobre as variáveis locais vivas do quadro. Ler vê o valor
/// de agora, escrever muda a variável; remover é `TypeError`.
struct FrameLocalsProxy {
    env: Rc<Env>,
    code: Rc<Code>,
}

fn key_error(key: &Value) -> PyException {
    PyException::from_value(&Value::Exception(Rc::new(ExcObj::new("KeyError", vec![key.clone()]))))
}

fn cannot_remove() -> PyException {
    type_error("cannot remove variables from FrameLocalsProxy")
}

impl FrameLocalsProxy {
    fn snapshot(&self) -> PyResult<Dict> {
        locals_dict(&self.code, &self.env)
    }

    fn store(&self, key: &Value, value: Value) -> PyResult<()> {
        let Value::Str(name) = key else {
            return Err(type_error("FrameLocalsProxy keys must be strings"));
        };
        self.env.set(name.as_str(), value);
        Ok(())
    }
}

/// Os métodos de leitura devolvem o que o `dict` devolveria sobre o instantâneo.
const READ_METHODS: &[&str] = &["keys", "values", "items", "get", "copy", "__contains__", "__len__"];

impl ExtObject for FrameLocalsProxy {
    fn type_name(&self) -> &'static str {
        "FrameLocalsProxy"
    }

    fn repr(&self) -> String {
        self.snapshot().map_or_else(|_| "{}".into(), |d| crate::object::repr(&Value::dict(d)))
    }

    fn methods(&self) -> &'static [&'static str] {
        &[
            "keys", "values", "items", "get", "copy", "__contains__", "__len__", "__setitem__", "__delitem__",
            "update", "setdefault", "pop",
        ]
    }

    fn len(&self) -> Option<usize> {
        self.snapshot().ok().map(|d| d.len())
    }

    fn is_true(&self) -> bool {
        self.len().is_some_and(|n| n > 0)
    }

    fn getitem(&self, key: &Value) -> Option<PyResult<Value>> {
        Some(self.snapshot().and_then(|d| match d.get(key)? {
            Some(v) => Ok(v),
            None => Err(key_error(key)),
        }))
    }

    fn to_items(&self) -> Option<Vec<Value>> {
        self.snapshot().ok().map(|d| d.keys().cloned().collect())
    }

    fn contains_item(&self, item: &Value) -> Option<PyResult<bool>> {
        Some(self.snapshot().and_then(|d| Ok(d.contains(item)?)))
    }

    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        if READ_METHODS.contains(&name) {
            let snapshot = Value::dict(self.snapshot()?);
            let method = vm.load_attr(&snapshot, name)?;
            return vm.call(&method, args, kw);
        }
        match name {
            "__setitem__" => {
                let [key, value] = <[Value; 2]>::try_from(args)
                    .map_err(|_| type_error("FrameLocalsProxy.__setitem__ expected 2 arguments"))?;
                self.store(&key, value).map(|()| Value::None)
            }
            "__delitem__" | "pop" => Err(cannot_remove()),
            "setdefault" => {
                let key = args.first().ok_or_else(|| type_error("setdefault expected at least 1 argument, got 0"))?;
                if let Some(v) = self.snapshot()?.get(key)? {
                    return Ok(v);
                }
                let default = args.get(1).cloned().unwrap_or(Value::None);
                self.store(key, default.clone())?;
                Ok(default)
            }
            // `update` aceita tudo o que o `dict.update` aceita: aplica num instantâneo e grava o resultado.
            _ => {
                let snapshot = Value::dict(self.snapshot()?);
                let method = vm.load_attr(&snapshot, "update")?;
                vm.call(&method, args, kw)?;
                if let Value::Dict(d) = &snapshot {
                    let items: Vec<(Value, Value)> = d.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    for (k, v) in items {
                        self.store(&k, v)?;
                    }
                }
                Ok(Value::None)
            }
        }
    }
}
