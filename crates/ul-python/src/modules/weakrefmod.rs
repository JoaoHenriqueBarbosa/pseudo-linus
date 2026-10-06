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
        }
    }
}

impl ExtObject for WeakRef {
    fn type_name(&self) -> &'static str {
        "ReferenceType"
    }

    fn repr(&self) -> String {
        let at = self.me.as_ptr() as usize;
        match self.upgrade() {
            Some(v) => format!("<weakref at {at:#x}; to '{}' at {:#x}>", v.type_name(), referent_addr(&v)),
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
    match v {
        Value::Instance(i) => Rc::as_ptr(i) as usize,
        Value::Class(c) => Rc::as_ptr(c) as usize,
        Value::Function(f) => Rc::as_ptr(f) as usize,
        Value::Module(m) => Rc::as_ptr(m) as usize,
        Value::Ext(e) => Rc::as_ptr(e) as *const () as usize,
        _ => 0,
    }
}

fn make_ref(obj: &Value, callback: Option<Value>) -> PyResult<Value> {
    let target = match obj {
        Value::Instance(i) => {
            if !i.class.slots_allow("__weakref__") {
                return Err(type_error(format!("cannot create weak reference to '{}' object", i.class.name)));
            }
            Target::Instance(Rc::downgrade(i))
        }
        Value::Class(c) => Target::Class(Rc::downgrade(c)),
        Value::Function(f) => Target::Function(Rc::downgrade(f)),
        Value::Module(m) => Target::Module(Rc::downgrade(m)),
        Value::Ext(e) => Target::Ext(Rc::downgrade(e)),
        other => return Err(type_error(format!("cannot create weak reference to '{}' object", other.type_name()))),
    };
    let r = Rc::new_cyclic(|me| WeakRef { target, me: me.clone(), callback: RefCell::new(callback), hash: RefCell::new(None) });
    Ok(Value::Ext(r))
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

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_weakref").func("ref", weak_ref).build()
}
