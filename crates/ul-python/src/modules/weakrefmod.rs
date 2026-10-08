//! `_weakref`: referências fracas de verdade sobre os `Rc` da VM. `ref(obj)` não mantém `obj` vivo;
//! chamar a referência devolve o objeto, ou `None` depois que o último dono o soltou.
//!
//! Só se pode apontar para o que o CPython também permite (instâncias, classes, funções, módulos e
//! objetos nativos); `list`, `int`, `str` etc. dão `TypeError`, igual lá. A função de retorno
//! (`callback`) roda na primeira vez que alguém observa a referência já morta, e não no instante
//! exato da morte: a VM conta referências e não tem gancho de coleta que possa chamar Python.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use crate::modules::ModuleBuilder;
use crate::native_util::bind;
use crate::object::{ClassObj, ExtObject, FuncObj, InstanceObj, Kw, ModuleObj, Value};
use crate::vm::{type_error, PyException, PyResult, Vm};

enum Target {
    Instance(Weak<InstanceObj>),
    Class(Weak<ClassObj>),
    Function(Weak<FuncObj>),
    Module(Weak<ModuleObj>),
    Ext(Weak<dyn ExtObject>),
    /// Tipo ou função embutida (`int`, `len`): vive enquanto o interpretador vive, então a referência
    /// nunca morre (no CPython os tipos estáticos são imortais).
    Static(Value),
}

pub struct WeakRef {
    target: Target,
    me: Weak<WeakRef>,
    callback: RefCell<Option<Value>>,
    /// O hash fica fixo na primeira vez que o referente estava vivo (como no CPython).
    hash: RefCell<Option<i64>>,
}

impl WeakRef {
    fn upgrade(&self) -> Option<Value> {
        match &self.target {
            Target::Instance(w) => w.upgrade().map(Value::Instance),
            Target::Class(w) => w.upgrade().map(Value::Class),
            Target::Function(w) => w.upgrade().map(Value::Function),
            Target::Module(w) => w.upgrade().map(Value::Module),
            Target::Ext(w) => w.upgrade().map(Value::Ext),
            Target::Static(v) => Some(v.clone()),
        }
    }
}

impl ExtObject for WeakRef {
    fn type_name(&self) -> &'static str {
        "ReferenceType"
    }

    fn image(&self) -> Option<crate::object::ExtImage> {
        Some(crate::object::ExtImage::WeakRef {
            target: self.upgrade(),
            callback: self.callback.borrow().clone(),
            hash: *self.hash.borrow(),
        })
    }

    fn repr(&self) -> String {
        let at = crate::object::py_addr(self.me.as_ptr() as usize);
        match self.upgrade() {
            Some(v) => {
                // `weakref_repr` do CPython acrescenta o `__name__` do referente quando ele tem um (tipo e
                // função; módulo não, conferido no oráculo).
                let name = match &v {
                    Value::Class(c) => Some(c.name.clone()),
                    Value::Function(f) => Some(f.code.name.to_string()),
                    Value::Builtin(n) => Some(n.to_string()),
                    Value::NativeFn(n) => Some(n.name.to_string()),
                    _ => None,
                };
                let suffix = name.map(|n| format!(" ({n})")).unwrap_or_default();
                format!("<weakref at {at:#x}; to '{}' at {:#x}{suffix}>", v.type_name(), referent_addr(&v))
            }
            None => format!("<weakref at {at:#x}; dead>"),
        }
    }

    fn methods(&self) -> &'static [&'static str] {
        &["__call__"]
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<Result<Value, PyException>> {
        match name {
            "__callback__" => Some(Ok(self.callback.borrow().clone().unwrap_or(Value::None))),
            _ => None,
        }
    }

    fn call_method(&self, vm: &mut Vm, _name: &str, _args: Vec<Value>, _kw: Kw) -> Result<Value, PyException> {
        if let Some(v) = self.upgrade() {
            return Ok(v);
        }
        let cb = self.callback.borrow_mut().take();
        if let (Some(cb), Some(me)) = (cb, self.me.upgrade()) {
            vm.call(&cb, vec![Value::Ext(me)], Vec::new())?;
        }
        Ok(Value::None)
    }

    fn hash_value(&self) -> Option<i64> {
        if let Some(h) = *self.hash.borrow() {
            return Some(h);
        }
        let v = self.upgrade()?;
        let h = crate::object::hash(&v).ok()?;
        *self.hash.borrow_mut() = Some(h);
        Some(h)
    }

    fn eq_value(&self, other: &Value) -> Option<bool> {
        let Value::Ext(o) = other else { return Some(false) };
        let me = self.me.upgrade()?;
        if std::ptr::addr_eq(Rc::as_ptr(o), Rc::as_ptr(&me)) {
            return Some(true);
        }
        // Duas referências só são iguais por valor quando as duas estão vivas; mortas, só por identidade.
        match (self.upgrade(), o.referent()) {
            (Some(a), Some(b)) => Some(crate::object::py_eq(&a, &b)),
            _ => Some(false),
        }
    }

    fn referent(&self) -> Option<Value> {
        self.upgrade()
    }
}

fn referent_addr(v: &Value) -> usize {
    let ptr = match v {
        Value::Instance(i) => Rc::as_ptr(i) as usize,
        Value::Class(c) => Rc::as_ptr(c) as usize,
        Value::Function(f) => Rc::as_ptr(f) as usize,
        Value::Module(m) => Rc::as_ptr(m) as usize,
        Value::Ext(e) => Rc::as_ptr(e) as *const () as usize,
        _ => return 0,
    };
    crate::object::py_addr(ptr)
}

fn make_ref(obj: &Value, callback: Option<Value>) -> PyResult<Value> {
    let target = match obj {
        Value::Instance(i) => {
            let class = i.class();
            if !class.slots_allow("__weakref__") {
                return Err(type_error(format!("cannot create weak reference to '{}' object", class.name)));
            }
            Target::Instance(Rc::downgrade(i))
        }
        Value::Class(c) => Target::Class(Rc::downgrade(c)),
        Value::Function(f) => Target::Function(Rc::downgrade(f)),
        Value::Module(m) => Target::Module(Rc::downgrade(m)),
        Value::Ext(e) => Target::Ext(Rc::downgrade(e)),
        v @ (Value::Builtin(_) | Value::NativeFn(_)) => Target::Static(v.clone()),
        other => return Err(type_error(format!("cannot create weak reference to '{}' object", other.type_name()))),
    };
    Ok(new_ref(target, callback, None))
}

fn new_ref(target: Target, callback: Option<Value>, hash: Option<i64>) -> Value {
    let r = Rc::new_cyclic(|me| WeakRef { target, me: me.clone(), callback: RefCell::new(callback), hash: RefCell::new(hash) });
    Value::Ext(r)
}

/// Refaz uma referência fraca a partir da imagem do heap: sem referente (já morto na captura) ela
/// nasce apontando para um objeto que ninguém segura, como uma referência morta.
pub(crate) fn weakref_from_image(target: Option<Value>, callback: Option<Value>, hash: Option<i64>) -> Option<Value> {
    let target = match target {
        Some(Value::Instance(i)) => Target::Instance(Rc::downgrade(&i)),
        Some(Value::Class(c)) => Target::Class(Rc::downgrade(&c)),
        Some(Value::Function(f)) => Target::Function(Rc::downgrade(&f)),
        Some(Value::Module(m)) => Target::Module(Rc::downgrade(&m)),
        Some(Value::Ext(e)) => Target::Ext(Rc::downgrade(&e)),
        Some(v @ (Value::Builtin(_) | Value::NativeFn(_))) => Target::Static(v),
        Some(_) => return None,
        None => Target::Module(Weak::new()),
    };
    Some(new_ref(target, callback, hash))
}

fn weak_ref(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("ref", args, kw, &["object", "callback"], 1)?;
    let cb = match a.get(1) {
        Some(Some(Value::None)) | Some(None) | None => None,
        Some(Some(v)) => Some(v.clone()),
    };
    let Some(obj) = a.first().and_then(|o| o.as_ref()) else { return Err(type_error("ref() missing required argument 'object'")) };
    make_ref(obj, cb)
}

/// `_weakref.proxy(obj, callback=None)`: os tipos de proxy vivem em `weakref.py`, carregado só na primeira
/// chamada, para o `collections` poder importar o nome no arranque sem trazer o `weakref` junto (no CPython o
/// `sys.modules` do arranque não tem `weakref`).
fn weak_proxy(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let module = crate::modules::import_checked(vm, "weakref")?;
    let proxy = vm.load_attr(&Value::Module(module), "proxy")?;
    vm.call(&proxy, args, kw)
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_weakref").func("ref", weak_ref).func("proxy", weak_proxy).build()
}
