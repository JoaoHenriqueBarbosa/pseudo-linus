//! O registro do coletor de ciclos: cada contêiner mutável que pode fechar um ciclo (`list`, `dict`, `set`, instância e
//! o `Env` de uma função) entra aqui quando nasce, como um `Weak`, para o coletor achar o lixo cíclico e para o
//! `gc.get_objects`/`gc.get_referrers` enxergarem o heap (desenho em `wip/notes/python-gc.md`).
//!
//! Só guarda `Weak`: o registro nunca prende um objeto. Os mortos são podados quando a geração passa do limite, que
//! dobra a cada poda, para um programa que cria milhões de contêineres curtos não cresça o vetor sem fim.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use crate::object::{Dict, Env, InstanceObj, Set, Value};

/// Um objeto registrado: o `Weak` do `Rc` que o segura.
pub(crate) enum Tracked {
    List(Weak<RefCell<Vec<Value>>>),
    Dict(Weak<RefCell<Dict>>),
    Set(Weak<RefCell<Set>>),
    Instance(Weak<InstanceObj>),
    Env(Weak<Env>),
}

/// O objeto vivo de um registro. O `Env` não é um valor do programa: o `gc.get_objects` não o mostra, o coletor o usa.
pub(crate) enum Live {
    Value(Value),
    Env(Rc<Env>),
}

impl Tracked {
    fn upgrade(&self) -> Option<Live> {
        Some(match self {
            Tracked::List(w) => Live::Value(Value::List(w.upgrade()?)),
            Tracked::Dict(w) => Live::Value(Value::Dict(w.upgrade()?)),
            Tracked::Set(w) => Live::Value(Value::Set(w.upgrade()?)),
            Tracked::Instance(w) => Live::Value(Value::Instance(w.upgrade()?)),
            Tracked::Env(w) => Live::Env(w.upgrade()?),
        })
    }

    fn alive(&self) -> bool {
        match self {
            Tracked::List(w) => w.strong_count() > 0,
            Tracked::Dict(w) => w.strong_count() > 0,
            Tracked::Set(w) => w.strong_count() > 0,
            Tracked::Instance(w) => w.strong_count() > 0,
            Tracked::Env(w) => w.strong_count() > 0,
        }
    }
}

/// As três gerações e a permanente (`gc.freeze`).
struct Registry {
    generations: [Vec<Tracked>; 3],
    frozen: Vec<Tracked>,
    /// Tamanho da geração 0 a partir do qual a poda dos mortos roda; dobra (ao dobro dos vivos) a cada poda.
    limit: usize,
}

/// A configuração e as contas do coletor (`gc.set_threshold`, `gc.get_count`, `gc.get_stats`).
struct State {
    enabled: bool,
    threshold: [usize; 3],
    /// Contêineres registrados desde a última coleta da geração 0 (o `count[0]` do CPython sem o desconto das mortes).
    allocated: usize,
    /// Coletas da geração 0 desde a última da 1, e da 1 desde a última da 2 (o `count[1]` e o `count[2]`).
    since: [usize; 2],
    /// Por geração: coletas feitas e objetos achados.
    collections: [usize; 3],
    collected: [usize; 3],
    /// Há coleta automática esperando o ponto seguro.
    due: bool,
    /// Uma coleta está rodando: a que vier por dentro dela (o `__del__` que aloca) espera.
    running: bool,
}

thread_local! {
    /// `gc.garbage` e `gc.callbacks` (as listas do módulo `gc` do programa) e as flags de `gc.set_debug`.
    static BOUND: RefCell<(Option<Value>, Option<Value>, i64)> = const { RefCell::new((None, None, 0)) };
    static STATE: RefCell<State> = const {
        RefCell::new(State {
            enabled: true,
            threshold: [2000, 10, 10],
            allocated: 0,
            since: [0, 0],
            collections: [0; 3],
            collected: [0; 3],
            due: false,
            running: false,
        })
    };
    static REGISTRY: RefCell<Registry> =
        const { RefCell::new(Registry { generations: [Vec::new(), Vec::new(), Vec::new()], frozen: Vec::new(), limit: 1024 }) };
    /// Dentro de uma poda ou de uma varredura o registro está emprestado: um objeto nascido ali (o `Weak` de um objeto
    /// criado por um `Drop`, por exemplo) fica para a próxima, sem pânico.
    static DEFERRED: Cell<usize> = const { Cell::new(0) };
}

fn track(t: Tracked) {
    let _ = REGISTRY.try_with(|r| {
        let Ok(mut r) = r.try_borrow_mut() else {
            let _ = DEFERRED.try_with(|d| d.set(d.get() + 1));
            return;
        };
        if r.generations[0].len() >= r.limit {
            r.generations[0].retain(Tracked::alive);
            r.limit = (r.generations[0].len() * 2).max(1024);
        }
        r.generations[0].push(t);
    });
    let _ = STATE.try_with(|s| {
        let Ok(mut s) = s.try_borrow_mut() else { return };
        s.allocated += 1;
        if s.enabled && !s.running && !s.due && s.allocated > s.threshold[0] {
            s.due = true;
            crate::finalize::request_safe_point();
        }
    });
}

pub(crate) fn track_list(l: &Rc<RefCell<Vec<Value>>>) {
    track(Tracked::List(Rc::downgrade(l)));
}

pub(crate) fn track_dict(d: &Rc<RefCell<Dict>>) {
    track(Tracked::Dict(Rc::downgrade(d)));
}

pub(crate) fn track_set(s: &Rc<RefCell<Set>>) {
    track(Tracked::Set(Rc::downgrade(s)));
}

pub(crate) fn track_instance(i: &Rc<InstanceObj>) {
    track(Tracked::Instance(Rc::downgrade(i)));
}

pub(crate) fn track_env(e: &Rc<Env>) {
    track(Tracked::Env(Rc::downgrade(e)));
}

/// Os objetos vivos da geração `generation` (`None`: todas, e a permanente), em ordem de criação.
pub(crate) fn live(generation: Option<usize>) -> Vec<Live> {
    REGISTRY
        .try_with(|r| {
            let Ok(r) = r.try_borrow() else { return Vec::new() };
            let mut out = Vec::new();
            let mut take = |list: &Vec<Tracked>| out.extend(list.iter().filter_map(Tracked::upgrade));
            match generation {
                Some(g) if g < 3 => take(&r.generations[g]),
                Some(_) => {}
                None => {
                    take(&r.frozen);
                    for g in 0..3 {
                        take(&r.generations[g]);
                    }
                }
            }
            out
        })
        .unwrap_or_default()
}

/// Os dois valores são o mesmo objeto (o mesmo `Rc`), como o `is`.
pub(crate) fn same_object(a: &Value, b: &Value) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
        && matches!(a, Value::List(_) | Value::Tuple(_) | Value::Dict(_) | Value::Set(_) | Value::Instance(_) | Value::Function(_)
            | Value::Class(_) | Value::Module(_) | Value::Ext(_) | Value::Native(_) | Value::Bound(_) | Value::BoundFn(_)
            | Value::Exception(_) | Value::ByteArray(_) | Value::Slice(_) | Value::NativeFn(_))
        && crate::builtins::id_of(a) == crate::builtins::id_of(b)
}

/// `gc.is_tracked(v)`: o objeto está no coletor. Escalares, `bytearray`, `range` e o `object()` puro não estão; a tupla
/// e o dict só estão se guardam algo rastreado (o CPython os destrava lazily: o dict que um dia guardou um contêiner
/// segue rastreado, o que daqui se vê pelo conteúdo de agora).
pub(crate) fn is_tracked(v: &Value) -> bool {
    match v {
        Value::None | Value::Bool(_) | Value::Int(_) | Value::Big(_) | Value::Float(_) | Value::Str(_) | Value::Bytes(_)
        | Value::ByteArray(_) | Value::Range(_) => false,
        Value::Tuple(t) => t.iter().any(is_tracked),
        Value::Dict(d) => d.borrow().iter().any(|(k, v)| is_tracked(k) || is_tracked(v)),
        Value::Instance(i) => i.class().name != "object",
        Value::Ext(e) => e.type_name() != "object",
        _ => true,
    }
}

/// Os objetos vivos das gerações `0..=generation`, em ordem de criação.
pub(crate) fn live_up_to(generation: usize) -> Vec<Live> {
    let mut out = Vec::new();
    for g in 0..=generation.min(2) {
        out.extend(live(Some(g)));
    }
    out
}

/// Depois de coletar `generation`: os sobreviventes das gerações coletadas passam para a seguinte (a 2 fica na 2), os
/// mortos saem do registro e as contas andam como as do CPython (`count[0..=g]` zeram, `count[g+1]` cresce).
fn advance(generation: usize, found: usize) {
    let _ = REGISTRY.try_with(|r| {
        let Ok(mut r) = r.try_borrow_mut() else { return };
        let target = (generation + 1).min(2);
        let mut moved = Vec::new();
        for g in 0..=generation.min(2) {
            moved.extend(std::mem::take(&mut r.generations[g]).into_iter().filter(Tracked::alive));
        }
        // A geração de destino mantém a ordem de criação: os que já estavam nela vêm antes dos promovidos.
        let mut kept = std::mem::take(&mut r.generations[target]);
        kept.extend(moved);
        r.generations[target] = kept;
        r.limit = (r.generations[0].len() * 2).max(1024);
    });
    let _ = STATE.try_with(|s| {
        let Ok(mut s) = s.try_borrow_mut() else { return };
        s.collections[generation] += 1;
        s.collected[generation] += found;
        s.allocated = 0;
        for g in 0..generation.min(2) {
            s.since[g] = 0;
        }
        if generation < 2 {
            s.since[generation] += 1;
        }
    });
}

/// `gc.py` entrega as duas listas do módulo e, a cada `set_debug`, as flags.
pub(crate) fn bind(garbage: Option<Value>, callbacks: Option<Value>, debug: Option<i64>) {
    let _ = BOUND.try_with(|b| {
        let Ok(mut b) = b.try_borrow_mut() else { return };
        if garbage.is_some() {
            b.0 = garbage;
        }
        if callbacks.is_some() {
            b.1 = callbacks;
        }
        if let Some(d) = debug {
            b.2 = d;
        }
    });
}

/// O que o módulo `gc` do programa guarda e a coleta usa: a lista `garbage`, as `callbacks` e as flags de `set_debug`.
fn gc_module_state(_vm: &mut crate::vm::Vm) -> (Option<Value>, Vec<Value>, i64) {
    BOUND
        .try_with(|b| {
            let Ok(b) = b.try_borrow() else { return (None, Vec::new(), 0) };
            let callbacks = match &b.1 {
                Some(Value::List(l)) => l.borrow().clone(),
                _ => Vec::new(),
            };
            (b.0.clone(), callbacks, b.2)
        })
        .unwrap_or((None, Vec::new(), 0))
}

/// Chama cada `gc.callbacks` com a fase (`start`/`stop`) e o `info` dela; o erro de uma delas vai ao `unraisablehook`.
fn run_callbacks(vm: &mut crate::vm::Vm, callbacks: &[Value], phase: &str, generation: usize, collected: usize) {
    for cb in callbacks {
        let mut info = Dict::default();
        let _ = info.set(Value::str("generation"), Value::Int(generation as i64));
        let _ = info.set(Value::str("collected"), Value::Int(collected as i64));
        let _ = info.set(Value::str("uncollectable"), Value::Int(0));
        if let Err(e) = vm.call(cb, vec![Value::str(phase), Value::dict(info)], Vec::new()) {
            vm.write_unraisable(cb, e);
        }
    }
}

/// `gc.collect(generation)`: devolve quantos objetos inalcançáveis achou. Uma coleta por dentro de outra (o `__del__`
/// que chama `gc.collect()`) não faz nada, como no CPython.
pub(crate) fn collect_generation(vm: &mut crate::vm::Vm, generation: usize) -> usize {
    let busy = STATE.try_with(|s| s.try_borrow_mut().map_or(true, |mut s| std::mem::replace(&mut s.running, true))).unwrap_or(true);
    if busy {
        return 0;
    }
    let (garbage, callbacks, debug) = gc_module_state(vm);
    run_callbacks(vm, &callbacks, "start", generation, 0);
    let options = crate::gccollect::Options { save_all: debug & 32 != 0, garbage };
    let found = crate::gccollect::collect(vm, generation, &options);
    advance(generation, found);
    let _ = STATE.try_with(|s| {
        s.try_borrow_mut().map(|mut s| {
            s.running = false;
            s.due = false;
        })
    });
    run_callbacks(vm, &callbacks, "stop", generation, found);
    found
}

/// O ponto seguro do laço: se o limiar da geração 0 venceu, coleta a geração mais velha que também venceu
/// (a 2 depois de `threshold[2]` coletas da 1, a 1 depois de `threshold[1]` da 0).
pub(crate) fn collect_if_due(vm: &mut crate::vm::Vm) {
    let generation = STATE
        .try_with(|s| {
            let s = s.try_borrow().ok()?;
            if !s.due || s.running || !s.enabled {
                return None;
            }
            Some(if s.since[1] > s.threshold[2] {
                2
            } else if s.since[0] > s.threshold[1] {
                1
            } else {
                0
            })
        })
        .ok()
        .flatten();
    match generation {
        Some(g) => {
            collect_generation(vm, g);
        }
        None => {
            let _ = STATE.try_with(|s| s.try_borrow_mut().map(|mut s| s.due = false));
        }
    }
}

/// `gc.get_count()`.
pub(crate) fn counts() -> [usize; 3] {
    STATE.try_with(|s| s.try_borrow().map_or([0; 3], |s| [s.allocated, s.since[0], s.since[1]])).unwrap_or([0; 3])
}

/// `gc.get_stats()`: por geração, coletas, objetos achados.
pub(crate) fn stats() -> [(usize, usize); 3] {
    STATE
        .try_with(|s| s.try_borrow().map_or([(0, 0); 3], |s| [0, 1, 2].map(|g| (s.collections[g], s.collected[g]))))
        .unwrap_or([(0, 0); 3])
}

/// `gc.enable`/`disable`/`set_threshold` (com `None`: só lê).
pub(crate) fn configure(enabled: Option<bool>, threshold: Option<[usize; 3]>) -> (bool, [usize; 3]) {
    STATE
        .try_with(|s| {
            let Ok(mut s) = s.try_borrow_mut() else { return (true, [2000, 10, 10]) };
            if let Some(e) = enabled {
                s.enabled = e;
            }
            if let Some(t) = threshold {
                s.threshold = t;
            }
            (s.enabled, s.threshold)
        })
        .unwrap_or((true, [2000, 10, 10]))
}

/// Quantos objetos vivos há na geração `generation`.
pub(crate) fn count(generation: usize) -> usize {
    REGISTRY
        .try_with(|r| r.try_borrow().map_or(0, |r| r.generations.get(generation).map_or(0, |g| g.iter().filter(|t| t.alive()).count())))
        .unwrap_or(0)
}

/// `gc.freeze()`: tudo o que está registrado passa para a geração permanente.
pub(crate) fn freeze() {
    let _ = STATE.try_with(|s| s.try_borrow_mut().map(|mut s| {
        s.allocated = 0;
        s.since = [0, 0];
    }));
    let _ = REGISTRY.try_with(|r| {
        let Ok(mut r) = r.try_borrow_mut() else { return };
        let r = &mut *r;
        for g in 0..3 {
            let moved = std::mem::take(&mut r.generations[g]);
            r.frozen.extend(moved.into_iter().filter(Tracked::alive));
        }
    });
}

/// `gc.unfreeze()`: a geração permanente volta para a mais velha.
pub(crate) fn unfreeze() {
    let _ = REGISTRY.try_with(|r| {
        let Ok(mut r) = r.try_borrow_mut() else { return };
        let r = &mut *r;
        let moved = std::mem::take(&mut r.frozen);
        r.generations[2].extend(moved.into_iter().filter(Tracked::alive));
    });
}

/// `gc.get_freeze_count()`.
pub(crate) fn frozen_count() -> usize {
    REGISTRY.try_with(|r| r.try_borrow().map_or(0, |r| r.frozen.iter().filter(|t| t.alive()).count())).unwrap_or(0)
}
