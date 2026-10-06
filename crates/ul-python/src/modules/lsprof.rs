//! Coletor do `_lsprof`: conta chamadas e tempo das funções escritas em Python (as nativas não
//! entram). O estado vive numa `thread_local`, então a `Vm` não carrega nada e, com o perfil
//! desligado, o gancho em `call_function` custa a leitura de um `Cell<bool>`.
//!
//! `start`/`stop` ligam e desligam, `clear` zera, `dump` devolve uma lista de
//! `(arquivo, linha, nome, chamadas, recursivas, total, interno, [(arquivo, linha, nome, chamadas,
//! recursivas, total, interno)])`, que o `_lsprof.py` transforma nos objetos que o `cProfile` espera.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use crate::compile::Code;
use crate::modules::ModuleBuilder;
use crate::native_util::no_kwargs;
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{PyResult, Vm};

type Key = (String, usize, String);

#[derive(Default, Clone)]
struct Stat {
    calls: u64,
    recursive: u64,
    total: f64,
    inline: f64,
}

#[derive(Default)]
struct Entry {
    stat: Stat,
    subcalls: HashMap<Key, Stat>,
    /// Chamadas desta função em andamento (para não somar o tempo total de recursões duas vezes).
    active: u32,
}

struct Frame {
    key: Key,
    start: Instant,
    child: f64,
    outermost: bool,
}

#[derive(Default)]
struct State {
    entries: HashMap<Key, Entry>,
    stack: Vec<Frame>,
}

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Marca a entrada numa função Python. O retorno é `true` se o perfil estava ligado, e então o
/// chamador deve chamar [`leave`] ao sair.
pub(crate) fn enter(code: &Rc<Code>) -> bool {
    if !ENABLED.with(Cell::get) {
        return false;
    }
    let key: Key = (code.filename.clone(), code.lines.first().copied().unwrap_or(0), code.qual().to_string());
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let entry = s.entries.entry(key.clone()).or_default();
        let outermost = entry.active == 0;
        if !outermost {
            entry.stat.recursive += 1;
        }
        entry.active += 1;
        s.stack.push(Frame { key, start: Instant::now(), child: 0.0, outermost });
    });
    true
}

/// Fecha a chamada aberta por [`enter`].
pub(crate) fn leave() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(frame) = s.stack.pop() else { return };
        let elapsed = frame.start.elapsed().as_secs_f64();
        let inline = (elapsed - frame.child).max(0.0);
        let parent = s.stack.last().map(|p| p.key.clone());
        if let Some(p) = s.stack.last_mut() {
            p.child += elapsed;
        }
        let recursive = !frame.outermost;
        if let Some(e) = s.entries.get_mut(&frame.key) {
            e.active = e.active.saturating_sub(1);
            e.stat.calls += 1;
            e.stat.inline += inline;
            if frame.outermost {
                e.stat.total += elapsed;
            }
        }
        if let Some(pk) = parent {
            if let Some(pe) = s.entries.get_mut(&pk) {
                let sub = pe.subcalls.entry(frame.key).or_default();
                sub.calls += 1;
                sub.inline += inline;
                if recursive {
                    sub.recursive += 1;
                } else {
                    sub.total += elapsed;
                }
            }
        }
    });
}

fn start(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    ENABLED.with(|e| e.set(true));
    Ok(Value::None)
}

fn stop(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    ENABLED.with(|e| e.set(false));
    Ok(Value::None)
}

fn clear(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.entries.clear();
    });
    Ok(Value::None)
}

fn row(key: &Key, st: &Stat) -> Vec<Value> {
    vec![
        Value::str(key.0.clone()),
        Value::Int(key.1 as i64),
        Value::str(key.2.clone()),
        Value::Int(st.calls as i64),
        Value::Int(st.recursive as i64),
        Value::Float(st.total),
        Value::Float(st.inline),
    ]
}

fn dump(_vm: &mut Vm, _args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("dump", &kw)?;
    let out = STATE.with(|s| {
        let s = s.borrow();
        let mut keys: Vec<&Key> = s.entries.keys().collect();
        keys.sort();
        keys.into_iter()
            .map(|k| {
                let e = &s.entries[k];
                let mut subs: Vec<(&Key, &Stat)> = e.subcalls.iter().collect();
                subs.sort_by(|a, b| a.0.cmp(b.0));
                let subs: Vec<Value> = subs.into_iter().map(|(sk, st)| Value::tuple(row(sk, st))).collect();
                let mut r = row(k, &e.stat);
                r.push(Value::list(subs));
                Value::tuple(r)
            })
            .collect::<Vec<Value>>()
    });
    Ok(Value::list(out))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_prof")
        .func("start", start)
        .func("stop", stop)
        .func("clear", clear)
        .func("dump", dump)
        .build()
}
