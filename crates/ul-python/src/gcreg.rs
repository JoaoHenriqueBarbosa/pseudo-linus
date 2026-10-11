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

thread_local! {
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

/// Quantos objetos vivos há na geração `generation`.
pub(crate) fn count(generation: usize) -> usize {
    REGISTRY
        .try_with(|r| r.try_borrow().map_or(0, |r| r.generations.get(generation).map_or(0, |g| g.iter().filter(|t| t.alive()).count())))
        .unwrap_or(0)
}

/// `gc.freeze()`: tudo o que está registrado passa para a geração permanente.
pub(crate) fn freeze() {
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
