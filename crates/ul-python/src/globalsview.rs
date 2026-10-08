//! Visões vivas das globais: `globals()`, `frame.f_globals` e `module.__dict__` devolvem o mesmo `dict` a cada
//! chamada, e escrever nele (`globals()['x'] = 1`, `globals().update(...)`, `del globals()['x']`) muda as globais
//! de verdade, assim como definir um nome aparece no dict.
//!
//! As globais ficam numa tabela própria (`VarMap`), não num `dict`, então cada visão guarda o `dict` e a tabela
//! e os mantém iguais: a tabela empurra cada nome gravado para o dict (`push`) e o dict puxa para a tabela o que
//! mudou desde a última sincronização (`sync_pull`, antes de cada instrução, só enquanto existir alguma visão).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::object::{Dict, Value, VarMap};

/// Ligada quando a primeira visão é criada; sem ela o laço da VM não paga nada.
pub static ARMED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// O contador global de mutações de dict na última varredura completa de `sync_pull` deste interpretador.
    /// É por thread, como as visões: cada processo Python do sandbox é uma thread do host, e a varredura de um
    /// não vale pelas visões do outro.
    static SEEN: Cell<u64> = const { Cell::new(0) };
}

struct View {
    map: Rc<RefCell<VarMap>>,
    dict: Rc<RefCell<Dict>>,
    /// `generation` do dict na última sincronização.
    generation: u64,
    /// Nomes (chaves `str`) que o dict tinha na última sincronização.
    keys: HashSet<String>,
}

thread_local! {
    static VIEWS: RefCell<Vec<View>> = const { RefCell::new(Vec::new()) };
}

fn name_of(key: &Value) -> Option<String> {
    match key {
        Value::Str(s) => Some(s.as_str().to_string()),
        _ => None,
    }
}

/// Copia a tabela para o dict na ordem de inserção dela; os `extras` que a tabela não tem entram depois, em
/// ordem alfabética.
fn refresh(view: &mut View, extras: &BTreeMap<String, Value>) {
    let mut all: Vec<(String, Value)> = Vec::new();
    {
        let map = view.map.borrow();
        all.extend(map.iter().map(|(k, v)| (k.to_string(), v.clone())));
        all.extend(extras.iter().filter(|(k, _)| !map.contains_key(k.as_str())).map(|(k, v)| (k.clone(), v.clone())));
    }
    let mut d = view.dict.borrow_mut();
    let present: HashSet<&str> = all.iter().map(|(k, _)| k.as_str()).collect();
    let stale: Vec<String> = view.keys.iter().filter(|k| !present.contains(k.as_str())).cloned().collect();
    for k in stale {
        let _ = d.remove(&Value::str(k));
    }
    for (k, v) in &all {
        let _ = d.set(Value::str(k.clone()), v.clone());
    }
    view.keys = all.into_iter().map(|(k, _)| k).collect();
    view.generation = d.generation;
}

/// O dict vivo das globais `map`; `extras` são nomes que só existem no módulo nativo (não na tabela).
pub fn view_for(map: &Rc<RefCell<VarMap>>, extras: Option<BTreeMap<String, Value>>) -> Value {
    sync_pull();
    let extras = extras.unwrap_or_default();
    let found = VIEWS.with(|views| {
        let mut views = views.borrow_mut();
        views.iter_mut().find(|v| Rc::ptr_eq(&v.map, map)).map(|view| {
            refresh(view, &extras);
            Value::Dict(view.dict.clone())
        })
    });
    if let Some(v) = found {
        return v;
    }
    let dict = Rc::new(RefCell::new(Dict::default()));
    let mut view = View { map: map.clone(), dict: dict.clone(), generation: 0, keys: HashSet::new() };
    refresh(&mut view, &extras);
    VIEWS.with(|views| views.borrow_mut().push(view));
    ARMED.store(true, Ordering::Relaxed);
    Value::Dict(dict)
}

/// A tabela de globais que `dict` está espelhando, se for uma visão.
pub fn map_of_dict(dict: &Value) -> Option<Rc<RefCell<VarMap>>> {
    let Value::Dict(d) = dict else { return None };
    VIEWS.with(|views| views.borrow().iter().find(|v| Rc::ptr_eq(&v.dict, d)).map(|v| v.map.clone()))
}

/// Aplica nas tabelas o que mudou nos dicts desde a última sincronização.
///
/// O contador global de mutações de dict (`dict_generation_now`) é o filtro barato: se não andou desde a
/// última varredura completa deste interpretador, nenhum dict mudou e não há o que olhar (uma leitura atômica
/// e uma `Cell`). Só uma mutação de dict de verdade paga a varredura das visões.
pub fn sync_pull() {
    let now = crate::object::dict_generation_now();
    if now == SEEN.with(Cell::get) {
        return;
    }
    let mut complete = true;
    VIEWS.with(|views| {
        let Ok(mut views) = views.try_borrow_mut() else {
            complete = false;
            return;
        };
        for view in views.iter_mut() {
            let Ok(d) = view.dict.try_borrow() else {
                complete = false;
                continue;
            };
            if d.generation == view.generation {
                continue;
            }
            let mut present: HashSet<String> = HashSet::new();
            let mut map = view.map.borrow_mut();
            for (k, v) in d.iter() {
                let Some(name) = name_of(k) else { continue };
                present.insert(name.clone());
                let unchanged = matches!(map.get(name.as_str()), Some(old) if crate::object::is(old, v));
                if !unchanged {
                    map.insert(Rc::from(name.as_str()), v.clone());
                }
            }
            for gone in view.keys.iter().filter(|k| !present.contains(*k)) {
                map.shift_remove(gone.as_str());
            }
            view.keys = present;
            view.generation = d.generation;
        }
    });
    if complete {
        SEEN.with(|s| s.set(now));
    }
}

/// Uma global foi gravada (`Some`) ou apagada (`None`) na tabela `map`: reflete no dict da visão.
pub fn push(map: &Rc<RefCell<VarMap>>, name: &str, value: Option<&Value>) {
    sync_pull();
    VIEWS.with(|views| {
        let Ok(mut views) = views.try_borrow_mut() else { return };
        for view in views.iter_mut().filter(|v| Rc::ptr_eq(&v.map, map)) {
            let Ok(mut d) = view.dict.try_borrow_mut() else { continue };
            match value {
                Some(v) => {
                    let _ = d.set(Value::str(name.to_string()), v.clone());
                    view.keys.insert(name.to_string());
                }
                None => {
                    let _ = d.remove(&Value::str(name.to_string()));
                    view.keys.remove(name);
                }
            }
            view.generation = d.generation;
        }
    });
}
