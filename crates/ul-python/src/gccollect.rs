//! O coletor de ciclos: eliminação por tentativa sobre as contagens de `Rc` (desenho em `wip/notes/python-gc.md`).
//!
//! 1. Os nós são os objetos registrados (`gcreg`) das gerações coletadas, mais os intermediários que eles seguram e
//!    que não se registram (tupla, função, método ligado, `slice`), achados por travessia.
//! 2. `gc_refs(nó) = strong_count(nó) - 1` (o `-1` é a referência que este coletor guarda) menos as referências
//!    internas: cada aresta de um nó para outro nó do conjunto subtrai uma. O que sobra é o que segura por fora.
//! 3. Quem tem `gc_refs > 0` é raiz; tudo o que as raízes alcançam vive; o resto é lixo cíclico.
//! 4. O lixo é quebrado esvaziando os contêineres (o `tp_clear`): as contagens caem e o `Drop` normal libera.
//!
//! As arestas têm de ser exatas: cada uma é um `Rc` forte que o nó de fato guarda. Aresta a mais subtrai demais e
//! coletaria um objeto vivo (por isso `gc_refs < 0` aborta a coleta); aresta a menos só deixa o objeto como raiz. Na
//! dúvida (empréstimo ativo, estrutura que não se percorre) o objeto fica como raiz.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::gcreg::Live;
use crate::object::{BoundMethod, Dict, Env, FuncObj, InstanceObj, Set, Value};

enum Handle {
    List(Rc<RefCell<Vec<Value>>>),
    Dict(Rc<RefCell<Dict>>),
    Set(Rc<RefCell<Set>>),
    Instance(Rc<InstanceObj>),
    Env(Rc<Env>),
    Tuple(Rc<[Value]>),
    Function(Rc<FuncObj>),
    BoundFn(Rc<(Value, Rc<FuncObj>)>),
    Bound(Rc<BoundMethod>),
    Slice(Rc<(Value, Value, Value)>),
}

/// Uma aresta de saída: o que um nó segura.
enum Child<'a> {
    Val(&'a Value),
    Env(&'a Rc<Env>),
    Dict(&'a Rc<RefCell<Dict>>),
    Func(&'a Rc<FuncObj>),
}

impl Handle {
    fn from_value(v: &Value) -> Option<Handle> {
        Some(match v {
            Value::List(x) => Handle::List(x.clone()),
            Value::Dict(x) => Handle::Dict(x.clone()),
            Value::Set(x) => Handle::Set(x.clone()),
            Value::Instance(x) => Handle::Instance(x.clone()),
            Value::Tuple(x) if !x.is_empty() => Handle::Tuple(x.clone()),
            Value::Function(x) => Handle::Function(x.clone()),
            Value::BoundFn(x) => Handle::BoundFn(x.clone()),
            Value::Bound(x) => Handle::Bound(x.clone()),
            Value::Slice(x) => Handle::Slice(x.clone()),
            _ => return None,
        })
    }

    fn key(&self) -> usize {
        match self {
            Handle::List(x) => Rc::as_ptr(x) as *const u8 as usize,
            Handle::Dict(x) => Rc::as_ptr(x) as *const u8 as usize,
            Handle::Set(x) => Rc::as_ptr(x) as *const u8 as usize,
            Handle::Instance(x) => Rc::as_ptr(x) as *const u8 as usize,
            Handle::Env(x) => Rc::as_ptr(x) as *const u8 as usize,
            Handle::Tuple(x) => Rc::as_ptr(x) as *const Value as usize,
            Handle::Function(x) => Rc::as_ptr(x) as *const u8 as usize,
            Handle::BoundFn(x) => Rc::as_ptr(x) as *const u8 as usize,
            Handle::Bound(x) => Rc::as_ptr(x) as *const u8 as usize,
            Handle::Slice(x) => Rc::as_ptr(x) as *const u8 as usize,
        }
    }

    fn strong(&self) -> usize {
        match self {
            Handle::List(x) => Rc::strong_count(x),
            Handle::Dict(x) => Rc::strong_count(x),
            Handle::Set(x) => Rc::strong_count(x),
            Handle::Instance(x) => Rc::strong_count(x),
            Handle::Env(x) => Rc::strong_count(x),
            Handle::Tuple(x) => Rc::strong_count(x),
            Handle::Function(x) => Rc::strong_count(x),
            Handle::BoundFn(x) => Rc::strong_count(x),
            Handle::Bound(x) => Rc::strong_count(x),
            Handle::Slice(x) => Rc::strong_count(x),
        }
    }

    /// Só os tipos que não se registram entram por travessia; os registrados vêm do registro, na geração certa.
    fn is_intermediate(&self) -> bool {
        matches!(self, Handle::Tuple(_) | Handle::Function(_) | Handle::BoundFn(_) | Handle::Bound(_) | Handle::Slice(_))
    }

    /// Chama `f` em cada referência forte que o nó guarda. `false`: o nó está emprestado e não dá para percorrer.
    fn visit(&self, f: &mut dyn FnMut(Child)) -> bool {
        match self {
            Handle::List(l) => match l.try_borrow() {
                Ok(l) => l.iter().for_each(|v| f(Child::Val(v))),
                Err(_) => return false,
            },
            Handle::Dict(d) => match d.try_borrow() {
                Ok(d) => d.iter().for_each(|(k, v)| {
                    f(Child::Val(k));
                    f(Child::Val(v));
                }),
                Err(_) => return false,
            },
            Handle::Set(s) => match s.try_borrow() {
                Ok(s) => s.iter().for_each(|v| f(Child::Val(v))),
                Err(_) => return false,
            },
            Handle::Instance(i) => {
                let (Ok(dict), Ok(payload), Ok(view)) = (i.dict.try_borrow(), i.payload.try_borrow(), i.view.try_borrow()) else {
                    return false;
                };
                dict.values().for_each(|v| f(Child::Val(v)));
                if let Some(p) = payload.as_ref() {
                    f(Child::Val(p));
                }
                if let Some(d) = view.as_ref() {
                    f(Child::Dict(d));
                }
            }
            Handle::Env(e) => {
                let Ok(vars) = e.vars.try_borrow() else { return false };
                vars.items.iter().for_each(|(_, v)| f(Child::Val(v)));
                if let Some(p) = e.parent.as_ref() {
                    f(Child::Env(p));
                }
            }
            Handle::Tuple(t) => t.iter().for_each(|v| f(Child::Val(v))),
            Handle::Function(func) => {
                let (Ok(attrs), Ok(annotations)) = (func.attrs.try_borrow(), func.annotations.try_borrow()) else { return false };
                func.defaults.iter().for_each(|v| f(Child::Val(v)));
                func.kwdefaults.iter().for_each(|(_, v)| f(Child::Val(v)));
                if let Some(c) = func.closure.as_ref() {
                    f(Child::Env(c));
                }
                f(Child::Dict(&attrs));
                if let Some(a) = annotations.as_ref() {
                    f(Child::Val(a));
                }
            }
            Handle::BoundFn(b) => {
                f(Child::Val(&b.0));
                f(Child::Func(&b.1));
            }
            Handle::Bound(b) => f(Child::Val(&b.recv)),
            Handle::Slice(s) => {
                f(Child::Val(&s.0));
                f(Child::Val(&s.1));
                f(Child::Val(&s.2));
            }
        }
        true
    }

    /// O contêiner perde o que guarda (o `tp_clear`); o que saiu vai para `graveyard`, que só cai depois, sem
    /// empréstimo nenhum aberto.
    fn clear(&self, graveyard: &mut Vec<Box<dyn std::any::Any>>) {
        match self {
            Handle::List(l) => {
                if let Ok(mut l) = l.try_borrow_mut() {
                    graveyard.push(Box::new(std::mem::take(&mut *l)));
                }
            }
            Handle::Dict(d) => {
                if let Ok(mut d) = d.try_borrow_mut() {
                    graveyard.push(Box::new(std::mem::take(&mut *d)));
                }
            }
            Handle::Set(s) => {
                if let Ok(mut s) = s.try_borrow_mut() {
                    graveyard.push(Box::new(std::mem::take(&mut *s)));
                }
            }
            Handle::Instance(i) => {
                if let Ok(mut d) = i.dict.try_borrow_mut() {
                    graveyard.push(Box::new(std::mem::take(&mut *d)));
                }
                if let Ok(mut p) = i.payload.try_borrow_mut() {
                    graveyard.push(Box::new(p.take()));
                }
                if let Ok(mut v) = i.view.try_borrow_mut() {
                    graveyard.push(Box::new(v.take()));
                }
            }
            Handle::Env(e) => {
                if let Ok(mut v) = e.vars.try_borrow_mut() {
                    graveyard.push(Box::new(std::mem::take(&mut *v)));
                }
            }
            // Os intermediários caem sozinhos quando o contêiner que os segurava se esvazia.
            _ => {}
        }
    }
}

fn child_handle(c: &Child) -> Option<Handle> {
    match c {
        Child::Val(v) => Handle::from_value(v),
        Child::Env(e) => Some(Handle::Env((*e).clone())),
        Child::Dict(d) => Some(Handle::Dict((*d).clone())),
        Child::Func(f) => Some(Handle::Function((*f).clone())),
    }
}

fn child_key(c: &Child) -> Option<usize> {
    match c {
        Child::Val(v) => match v {
            Value::List(x) => Some(Rc::as_ptr(x) as *const u8 as usize),
            Value::Dict(x) => Some(Rc::as_ptr(x) as *const u8 as usize),
            Value::Set(x) => Some(Rc::as_ptr(x) as *const u8 as usize),
            Value::Instance(x) => Some(Rc::as_ptr(x) as *const u8 as usize),
            Value::Tuple(x) if !x.is_empty() => Some(Rc::as_ptr(x) as *const Value as usize),
            Value::Function(x) => Some(Rc::as_ptr(x) as *const u8 as usize),
            Value::BoundFn(x) => Some(Rc::as_ptr(x) as *const u8 as usize),
            Value::Bound(x) => Some(Rc::as_ptr(x) as *const u8 as usize),
            Value::Slice(x) => Some(Rc::as_ptr(x) as *const u8 as usize),
            _ => None,
        },
        Child::Env(e) => Some(Rc::as_ptr(e) as *const u8 as usize),
        Child::Dict(d) => Some(Rc::as_ptr(d) as *const u8 as usize),
        Child::Func(f) => Some(Rc::as_ptr(f) as *const u8 as usize),
    }
}

struct Node {
    handle: Handle,
    gc_refs: isize,
    /// O nó estava emprestado: não se percorre, e vale como raiz.
    opaque: bool,
    reachable: bool,
}

/// O `__del__` de uma instância ainda não finalizada: enquanto a fatia da finalização não existe, ela e tudo o que ela
/// alcança ficam vivos.
fn has_pending_finalizer(h: &Handle) -> bool {
    matches!(h, Handle::Instance(i) if !i.finalized.get() && i.class().finalizer().is_some())
}

/// Coleta as gerações `0..=generation`: devolve quantos objetos do CPython o lixo achado somava (ver `weight`).
pub(crate) fn collect(generation: usize) -> usize {
    let members = crate::gcreg::live_up_to(generation);
    let mut nodes: Vec<Node> = Vec::with_capacity(members.len());
    let mut index: HashMap<usize, usize> = HashMap::with_capacity(members.len());
    for live in members {
        let handle = match live {
            Live::Value(v) => match Handle::from_value(&v) {
                Some(h) => h,
                None => continue,
            },
            Live::Env(e) => Handle::Env(e),
        };
        // Um objeto registrado duas vezes (ou já achado) não entra de novo: cada nó guarda uma só referência.
        if index.contains_key(&handle.key()) {
            continue;
        }
        index.insert(handle.key(), nodes.len());
        nodes.push(Node { handle, gc_refs: 0, opaque: false, reachable: false });
    }
    // Descoberta: os intermediários que os nós seguram.
    let mut at = 0;
    while at < nodes.len() {
        let mut found: Vec<Handle> = Vec::new();
        let ok = nodes[at].handle.visit(&mut |c| {
            if let Some(k) = child_key(&c) {
                if !index.contains_key(&k) {
                    if let Some(h) = child_handle(&c) {
                        if h.is_intermediate() && !found.iter().any(|x| x.key() == k) {
                            found.push(h);
                        }
                    }
                }
            }
        });
        nodes[at].opaque = !ok;
        for h in found {
            index.insert(h.key(), nodes.len());
            nodes.push(Node { handle: h, gc_refs: 0, opaque: false, reachable: false });
        }
        at += 1;
    }
    for n in &mut nodes {
        n.gc_refs = n.handle.strong() as isize - 1;
    }
    let mut internal = vec![0isize; nodes.len()];
    for n in nodes.iter().filter(|n| !n.opaque) {
        n.handle.visit(&mut |c| {
            if let Some(&i) = child_key(&c).and_then(|k| index.get(&k)) {
                internal[i] += 1;
            }
        });
    }
    for (n, sub) in nodes.iter_mut().zip(&internal) {
        n.gc_refs -= sub;
        if n.gc_refs < 0 {
            // Aresta a mais: a contagem não fecha, então nada se coleta.
            return 0;
        }
    }
    // Alcançabilidade a partir das raízes.
    let mut stack: Vec<usize> = (0..nodes.len()).filter(|&i| nodes[i].gc_refs > 0 || nodes[i].opaque || has_pending_finalizer(&nodes[i].handle)).collect();
    for &i in &stack {
        nodes[i].reachable = true;
    }
    while let Some(i) = stack.pop() {
        let mut next = Vec::new();
        nodes[i].handle.visit(&mut |c| {
            if let Some(&j) = child_key(&c).and_then(|k| index.get(&k)) {
                next.push(j);
            }
        });
        for j in next {
            if !nodes[j].reachable {
                nodes[j].reachable = true;
                stack.push(j);
            }
        }
    }
    let garbage: Vec<usize> = (0..nodes.len()).filter(|&i| !nodes[i].reachable).collect();
    let collected = weight(garbage.iter().map(|&i| &nodes[i].handle));
    let mut graveyard: Vec<Box<dyn std::any::Any>> = Vec::new();
    for &i in &garbage {
        nodes[i].handle.clear(&mut graveyard);
    }
    drop(graveyard);
    drop(nodes);
    collected
}

/// Quantos objetos do CPython o lixo soma: instância, lista, dict, set, tupla, método e `slice` valem 1; a função
/// vale 1 mais a tupla de closure e as células (uma por variável livre, contada uma vez por escopo); o `Env` não é
/// objeto no CPython.
fn weight<'a>(handles: impl Iterator<Item = &'a Handle> + Clone) -> usize {
    // O que daqui é parte de outro objeto do CPython não conta à parte: o `__dict__` vazio de uma função (o CPython só o
    // cria quando alguém o pede) e o valor embutido de uma instância de subclasse de `list`, `dict` ou `set`.
    let mut part_of_another = std::collections::HashSet::new();
    for h in handles.clone() {
        match h {
            Handle::Function(f) => {
                if let Ok(attrs) = f.attrs.try_borrow() {
                    if attrs.borrow().len() == 0 {
                        part_of_another.insert(Rc::as_ptr(&*attrs) as *const u8 as usize);
                    }
                }
            }
            Handle::Instance(i) => {
                if let Ok(payload) = i.payload.try_borrow() {
                    match payload.as_ref() {
                        Some(Value::List(x)) => part_of_another.insert(Rc::as_ptr(x) as *const u8 as usize),
                        Some(Value::Dict(x)) => part_of_another.insert(Rc::as_ptr(x) as *const u8 as usize),
                        Some(Value::Set(x)) => part_of_another.insert(Rc::as_ptr(x) as *const u8 as usize),
                        _ => false,
                    };
                }
            }
            _ => {}
        }
    }
    let mut total = 0;
    let mut cells = std::collections::HashSet::new();
    for h in handles {
        total += match h {
            Handle::Env(_) => 0,
            Handle::Function(f) => {
                if let (Some(env), false) = (f.closure.as_ref(), f.code.freevars.is_empty()) {
                    for name in &f.code.freevars {
                        cells.insert((Rc::as_ptr(env) as usize, name.clone()));
                    }
                    2
                } else {
                    1
                }
            }
            other if part_of_another.contains(&other.key()) => 0,
            _ => 1,
        };
    }
    total + cells.len()
}
