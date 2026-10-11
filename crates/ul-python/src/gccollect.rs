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
use crate::frameobj::{FrameHold, FrameObj};
use crate::object::{BoundMethod, Dict, Env, ExcObj, ExtObject, FuncObj, InstanceObj, Set, Value};
use crate::tbobj::TracebackObj;
use crate::vm::TbEntry;

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
    Exception(Rc<ExcObj>),
    /// As entradas de um traceback (compartilhadas pelos nós `traceback` de uma mesma exceção).
    Entries(Rc<Vec<TbEntry>>),
    /// O quadro que um traceback ou um objeto `frame` segura vivo (o escopo, as variáveis).
    Hold(Rc<FrameHold>),
    /// Um objeto `traceback` do programa.
    Traceback(Rc<dyn ExtObject>),
    /// Um objeto `frame` do programa.
    Frame(Rc<dyn ExtObject>),
}

/// Uma aresta de saída: o que um nó segura.
pub(crate) enum Child<'a> {
    Val(&'a Value),
    Env(&'a Rc<Env>),
    Dict(&'a Rc<RefCell<Dict>>),
    Func(&'a Rc<FuncObj>),
    Entries(&'a Rc<Vec<TbEntry>>),
    Hold(&'a Rc<FrameHold>),
}

fn ext_key(e: &Rc<dyn ExtObject>) -> usize {
    Rc::as_ptr(e) as *const () as usize
}

/// O objeto nativo que o coletor sabe percorrer: `traceback` ou `frame`.
fn ext_handle(e: &Rc<dyn ExtObject>) -> Option<Handle> {
    let any = e.as_any()?;
    if any.is::<TracebackObj>() {
        Some(Handle::Traceback(e.clone()))
    } else if any.is::<FrameObj>() {
        Some(Handle::Frame(e.clone()))
    } else {
        None
    }
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
            Value::Exception(x) => Handle::Exception(x.clone()),
            Value::Ext(e) => return ext_handle(e),
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
            Handle::Exception(x) => Rc::as_ptr(x) as *const u8 as usize,
            Handle::Entries(x) => Rc::as_ptr(x) as *const u8 as usize,
            Handle::Hold(x) => Rc::as_ptr(x) as *const u8 as usize,
            Handle::Traceback(x) | Handle::Frame(x) => ext_key(x),
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
            Handle::Exception(x) => Rc::strong_count(x),
            Handle::Entries(x) => Rc::strong_count(x),
            Handle::Hold(x) => Rc::strong_count(x),
            Handle::Traceback(x) | Handle::Frame(x) => Rc::strong_count(x),
        }
    }

    /// Só os tipos que não se registram entram por travessia; os registrados vêm do registro, na geração certa.
    fn is_intermediate(&self) -> bool {
        matches!(
            self,
            Handle::Tuple(_)
                | Handle::Function(_)
                | Handle::BoundFn(_)
                | Handle::Bound(_)
                | Handle::Slice(_)
                | Handle::Exception(_)
                | Handle::Entries(_)
                | Handle::Hold(_)
                | Handle::Traceback(_)
                | Handle::Frame(_)
        )
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
            Handle::Exception(e) => {
                let (Ok(tb), Ok(chain), Ok(extra)) = (e.traceback.try_borrow(), e.chain.try_borrow(), e.extra.try_borrow()) else {
                    return false;
                };
                e.args.iter().for_each(|v| f(Child::Val(v)));
                if let Some(t) = tb.as_ref() {
                    f(Child::Val(t));
                }
                if let Some(c) = chain.cause.as_ref() {
                    f(Child::Val(c));
                }
                if let Some(c) = chain.context.as_ref() {
                    f(Child::Val(c));
                }
                extra.iter().for_each(|(_, v)| f(Child::Val(v)));
            }
            Handle::Entries(entries) => {
                for entry in entries.iter() {
                    if let Some(h) = entry.4.as_ref() {
                        f(Child::Hold(h));
                    }
                }
            }
            Handle::Hold(h) => f(Child::Env(&h.env)),
            Handle::Traceback(t) => {
                let Some(tb) = t.as_any().and_then(|a| a.downcast_ref::<TracebackObj>()) else { return false };
                return tb.gc_children(f);
            }
            Handle::Frame(fr) => {
                let Some(frame) = fr.as_any().and_then(|a| a.downcast_ref::<FrameObj>()) else { return false };
                return frame.gc_children(f);
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
        Child::Entries(e) => Some(Handle::Entries((*e).clone())),
        Child::Hold(h) => Some(Handle::Hold((*h).clone())),
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
            Value::Exception(x) => Some(Rc::as_ptr(x) as *const u8 as usize),
            Value::Ext(e) => ext_handle(e).map(|h| h.key()),
            _ => None,
        },
        Child::Entries(e) => Some(Rc::as_ptr(e) as *const u8 as usize),
        Child::Hold(h) => Some(Rc::as_ptr(h) as *const u8 as usize),
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

/// O que o chamador (o `gc` do programa) pede à coleta.
pub(crate) struct Options {
    /// `DEBUG_SAVEALL`: o lixo vai para `gc.garbage` em vez de ser esvaziado.
    pub(crate) save_all: bool,
    /// A lista `gc.garbage`, quando o módulo `gc` está carregado.
    pub(crate) garbage: Option<Value>,
}

/// O `deduce_unreachable`: acerta `gc_refs` dos nós (referências fortes menos as internas ao conjunto) e marca o que
/// as raízes alcançam. `false`: a contagem não fecha (aresta a mais), nada pode ser coletado.
fn deduce(nodes: &mut [Node], index: &HashMap<usize, usize>) -> bool {
    for n in nodes.iter_mut() {
        n.gc_refs = n.handle.strong() as isize - 1;
        n.reachable = false;
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
            return false;
        }
    }
    let mut stack: Vec<usize> = (0..nodes.len()).filter(|&i| nodes[i].gc_refs > 0 || nodes[i].opaque).collect();
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
    true
}

/// Coleta as gerações `0..=generation`: devolve quantos objetos do CPython o lixo somava (ver `weight`). A ordem é a do
/// `gc_collect_main`: acha o lixo, limpa as referências fracas a ele e chama as funções de retorno, finaliza (o `__del__`
/// uma vez por objeto), confere o que a finalização ressuscitou e só então quebra os ciclos (ou guarda em `gc.garbage`).
pub(crate) fn collect(vm: &mut crate::vm::Vm, generation: usize, options: &Options) -> usize {
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
        // Um objeto registrado duas vezes não entra de novo: cada nó guarda uma só referência.
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
    if !deduce(&mut nodes, &index) {
        return 0;
    }
    // O lixo, em ordem de criação, com a sua própria indexação para a reanálise depois da finalização.
    let mut garbage: Vec<Node> = Vec::new();
    for n in nodes {
        if !n.reachable {
            garbage.push(n);
        }
    }
    if garbage.is_empty() {
        return 0;
    }
    let mut garbage_index: HashMap<usize, usize> = garbage.iter().enumerate().map(|(i, n)| (n.handle.key(), i)).collect();
    // `handle_weakrefs`: as referências fracas ao lixo são limpas antes de qualquer `__del__`, e as funções de retorno
    // das que sobrevivem rodam agora.
    let addrs: std::collections::HashSet<usize> = garbage
        .iter()
        .filter(|n| matches!(n.handle, Handle::Instance(_) | Handle::Function(_)))
        .map(|n| match &n.handle {
            Handle::Instance(i) => Rc::as_ptr(i) as usize,
            Handle::Function(f) => Rc::as_ptr(f) as usize,
            _ => 0,
        })
        .collect();
    for r in crate::modules::weakrefmod::clear_targets(&addrs) {
        r.fire(vm);
    }
    // `finalize_garbage`: o `__del__` de cada instância que ainda não foi finalizada, uma vez só.
    let instances: Vec<Rc<InstanceObj>> = garbage
        .iter()
        .filter_map(|n| match &n.handle {
            Handle::Instance(i) if !i.finalized.get() && i.class().finalizer().is_some() => Some(i.clone()),
            _ => None,
        })
        .collect();
    let finalized_any = !instances.is_empty();
    for inst in &instances {
        vm.finalize(inst);
    }
    drop(instances);
    // `handle_resurrected_objects`: o que a finalização (ou uma função de retorno) passou a segurar por fora volta a viver,
    // com tudo o que alcança.
    if finalized_any {
        let ok = deduce(&mut garbage, &garbage_index);
        if !ok {
            // Aresta a mais na reanálise: não se arrisca quebrar nada.
            return 0;
        }
        garbage.retain(|n| !n.reachable);
        garbage_index = garbage.iter().enumerate().map(|(i, n)| (n.handle.key(), i)).collect();
        let _ = &garbage_index;
    }
    let collected = weight(garbage.iter().map(|n| &n.handle));
    if options.save_all {
        // `DEBUG_SAVEALL`: o lixo vai para `gc.garbage` e fica vivo (o `tp_clear` não roda).
        if let Some(Value::List(list)) = options.garbage.as_ref() {
            let mut saved = Vec::new();
            for n in &garbage {
                let v = match &n.handle {
                    Handle::List(x) => Some(Value::List(x.clone())),
                    Handle::Dict(x) => Some(Value::Dict(x.clone())),
                    Handle::Set(x) => Some(Value::Set(x.clone())),
                    Handle::Instance(x) => Some(Value::Instance(x.clone())),
                    Handle::Tuple(x) => Some(Value::Tuple(x.clone())),
                    Handle::Function(x) => Some(Value::Function(x.clone())),
                    Handle::BoundFn(x) => Some(Value::BoundFn(x.clone())),
                    Handle::Bound(x) => Some(Value::Bound(x.clone())),
                    Handle::Slice(x) => Some(Value::Slice(x.clone())),
                    Handle::Exception(x) => Some(Value::Exception(x.clone())),
                    Handle::Traceback(x) | Handle::Frame(x) => Some(Value::Ext(x.clone())),
                    Handle::Env(_) | Handle::Entries(_) | Handle::Hold(_) => None,
                };
                saved.extend(v);
            }
            if let Ok(mut list) = list.try_borrow_mut() {
                list.extend(saved);
            }
        }
        return collected;
    }
    let mut graveyard: Vec<Box<dyn std::any::Any>> = Vec::new();
    for n in &garbage {
        n.handle.clear(&mut graveyard);
    }
    drop(graveyard);
    drop(garbage);
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
    let mut frames = std::collections::HashSet::new();
    for h in handles {
        total += match h {
            Handle::Env(_) | Handle::Entries(_) | Handle::Frame(_) => 0,
            // A exceção e a tupla dos `args` (que o CPython conta à parte), o `frame` (uma vez por quadro, mesmo
            // compartilhado por vários tracebacks) e o(s) `traceback`.
            Handle::Exception(e) => 1 + usize::from(!e.args.is_empty()),
            Handle::Hold(h) => usize::from(frames.insert(Rc::as_ptr(&h.env) as usize)),
            Handle::Traceback(t) => t.as_any().and_then(|a| a.downcast_ref::<TracebackObj>()).map_or(1, TracebackObj::gc_weight),
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
