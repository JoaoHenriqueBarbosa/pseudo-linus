//! Porte de `runtime/JSFinalizationRegistry.{h,cpp}`: a célula de `FinalizationRegistry`
//! (`JSInternalFieldObjectImpl<1>` com o `callback`, mais as tabelas de registros vivos e mortos).
//!
//! DIVERGÊNCIAS:
//! - `JSInternalFieldObjectImpl<1>` é um `JSNonFinalObject` com o `callback` num campo, como em
//!   `js_string_iterator.rs` (sem GC não há `WriteBarrier`).
//! - Sem GC nenhum alvo morre: `reconcileWeakReferencesAtGCEnd` (que move registros de vivos para mortos e
//!   agenda o `runFinalizationCleanup` no `DeferredWorkTimer`) e o próprio `runFinalizationCleanup` não
//!   existem, e as tabelas de mortos ficam vazias. `takeDeadHoldingsValue`, `deadCount` e `deadRegistrations`
//!   são portados porque o coletor, quando chegar, só precisa preenchê-las.
//! - `Locker<JSCellLock>` some (uma thread, `RefCell`). `UncheckedKeyHashMap<JSCell*, ...>` é um `HashMap`
//!   pelo `cell_id` do alvo ou do token; nada observável depende da ordem de iteração dele.
//! - `finishCreation` chama `currentScriptExecutionOwner(globalObject)` para criar o wrapper DOM do
//!   documento: é gancho do WebCore/Bun, sem efeito no motor.
//! - O `TypeInfo` é `ObjectType` (sem `JSType` próprio), então o tipo da célula sai da `Structure`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `const ClassInfo JSFinalizationRegistry::s_info`.
pub static JS_FINALIZATION_REGISTRY_S_INFO: ClassInfo =
    ClassInfo { class_name: "FinalizationRegistry", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `enum class Field : uint8_t { Callback }`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Callback = 0,
}

/// `struct Registration { JSCell* target; WriteBarrier<Unknown> holdings; }`: o alvo é o `cell_id`.
#[derive(Clone, Copy, Debug)]
struct Registration {
    target: usize,
    holdings: JSValue,
}

/// `struct LiveRegistration`: o que `liveRegistrations` lista.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveRegistration {
    pub target: usize,
    pub held_value: JSValue,
    /// `unregisterToken`: o `cell_id`, ou `None` (o `nullptr`).
    pub unregister_token: Option<usize>,
}

/// `struct DeadRegistration`: o que `deadRegistrations` lista.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeadRegistration {
    pub held_value: JSValue,
    pub unregister_token: Option<usize>,
}

/// Os campos privados de `JSFinalizationRegistry`: `m_liveRegistrations`, `m_deadRegistrations`,
/// `m_noUnregistrationLive`, `m_noUnregistrationDead` (`m_hasAlreadyScheduledWork` só importa para o coletor).
#[derive(Default)]
struct Registrations {
    live: HashMap<usize, Vec<Registration>>,
    dead: HashMap<usize, Vec<JSValue>>,
    no_unregistration_live: Vec<Registration>,
    no_unregistration_dead: Vec<JSValue>,
}

/// `class JSFinalizationRegistry final : public JSInternalFieldObjectImpl<1>`.
pub struct JSFinalizationRegistry {
    base: JSNonFinalObject,
    /// `internalField(Field::Callback)`.
    callback: JSValue,
    registrations: RefCell<Registrations>,
}

/// A referência à célula, o `*` do C++.
pub type JSFinalizationRegistryRef = Rc<JSFinalizationRegistry>;

impl std::ops::Deref for JSFinalizationRegistry {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSFinalizationRegistry {
    /// `createStructure(vm, globalObject, prototype)`: `TypeInfo(ObjectType, StructureFlags)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::ObjectType, JSNonFinalObject::STRUCTURE_FLAGS),
            &JS_FINALIZATION_REGISTRY_S_INFO,
        )
    }

    /// `create(vm, structure, callback)`: `callback` é o `JSObject` chamável.
    pub fn create(vm: &VM, structure: &StructureRef, callback: JSValue) -> JSFinalizationRegistryRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(JSFinalizationRegistry {
            base: JSNonFinalObject::new(vm, Rc::clone(structure)),
            callback,
            registrations: RefCell::new(Registrations::default()),
        });
        cell.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::FinalizationRegistry(Rc::clone(&cell)));
        cell
    }

    /// `dynamicDowncast<JSFinalizationRegistry>(value)`.
    pub fn from_value(value: &JSValue) -> Option<JSFinalizationRegistryRef> {
        match value {
            JSValue::Cell(cell_id) => match cell_registry::get(*cell_id) {
                Some(CellEntry::FinalizationRegistry(cell)) => Some(cell),
                _ => None,
            },
            _ => None,
        }
    }

    /// `internalField(field).get()`.
    pub fn internal_field(&self, field: Field) -> JSValue {
        match field {
            Field::Callback => self.callback,
        }
    }

    /// `callback()`.
    pub fn callback(&self) -> JSValue {
        self.internal_field(Field::Callback)
    }

    /// `registerTarget(vm, target, holdings, token)`: `token` é `undefined` ou uma célula.
    pub fn register_target(&self, target: JSValue, holdings: JSValue, token: JSValue) {
        let registration = Registration { target: target.as_cell(), holdings };
        let mut registrations = self.registrations.borrow_mut();
        if token.is_undefined() {
            registrations.no_unregistration_live.push(registration);
        } else {
            debug_assert!(token.is_cell());
            registrations.live.entry(token.as_cell()).or_default().push(registration);
        }
    }

    /// `unregister(vm, token)`: `true` se algum registro (vivo ou morto) tinha o token.
    pub fn unregister(&self, token: JSValue) -> bool {
        let mut registrations = self.registrations.borrow_mut();
        let removed_live = registrations.live.remove(&token.as_cell()).is_some();
        let removed_dead = registrations.dead.remove(&token.as_cell()).is_some();
        removed_live | removed_dead
    }

    /// `takeDeadHoldingsValue()`: o próximo valor guardado de um alvo morto, ou `None` (o `JSValue()`).
    pub fn take_dead_holdings_value(&self) -> Option<JSValue> {
        let mut registrations = self.registrations.borrow_mut();
        if let Some(holdings) = registrations.no_unregistration_dead.pop() {
            return Some(holdings);
        }
        let token = *registrations.dead.keys().next()?;
        let bucket = registrations.dead.get_mut(&token)?;
        debug_assert!(!bucket.is_empty());
        let result = bucket.pop();
        if bucket.is_empty() {
            registrations.dead.remove(&token);
        }
        result
    }

    /// `liveCount(locker)`.
    pub fn live_count(&self) -> usize {
        let registrations = self.registrations.borrow();
        registrations.no_unregistration_live.len() + registrations.live.values().map(Vec::len).sum::<usize>()
    }

    /// `liveRegistrations(locker)`.
    pub fn live_registrations(&self) -> Vec<LiveRegistration> {
        let registrations = self.registrations.borrow();
        let mut result: Vec<LiveRegistration> = registrations
            .no_unregistration_live
            .iter()
            .map(|registration| LiveRegistration {
                target: registration.target,
                held_value: registration.holdings,
                unregister_token: None,
            })
            .collect();
        for (token, bucket) in &registrations.live {
            for registration in bucket {
                result.push(LiveRegistration {
                    target: registration.target,
                    held_value: registration.holdings,
                    unregister_token: Some(*token),
                });
            }
        }
        result
    }

    /// `deadCount(locker)`.
    pub fn dead_count(&self) -> usize {
        let registrations = self.registrations.borrow();
        registrations.no_unregistration_dead.len() + registrations.dead.values().map(Vec::len).sum::<usize>()
    }

    /// `deadRegistrations(locker)`.
    pub fn dead_registrations(&self) -> Vec<DeadRegistration> {
        let registrations = self.registrations.borrow();
        let mut result: Vec<DeadRegistration> = registrations
            .no_unregistration_dead
            .iter()
            .map(|held_value| DeadRegistration { held_value: *held_value, unregister_token: None })
            .collect();
        for (token, bucket) in &registrations.dead {
            for held_value in bucket {
                result.push(DeadRegistration { held_value: *held_value, unregister_token: Some(*token) });
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_weak_set::JSWeakSet;

    fn object(vm: &VM) -> JSValue {
        JSWeakSet::create(vm, &JSWeakSet::create_structure(vm, None, JSValue::Null)).as_value()
    }

    #[test]
    fn register_and_unregister_by_token() {
        let vm = VM::new();
        let structure = JSFinalizationRegistry::create_structure(&vm, None, JSValue::Null);
        let registry = JSFinalizationRegistry::create(&vm, &structure, object(&vm));
        let (target, token) = (object(&vm), object(&vm));
        registry.register_target(target, JSValue::Int32(1), token);
        registry.register_target(object(&vm), JSValue::Int32(2), JSValue::Undefined);
        assert_eq!(registry.live_count(), 2);
        assert_eq!(registry.live_registrations().len(), 2);
        assert!(registry.unregister(token));
        assert!(!registry.unregister(token));
        assert_eq!(registry.live_count(), 1);
        assert_eq!(registry.dead_count(), 0);
        assert_eq!(registry.take_dead_holdings_value(), None);
    }
}
