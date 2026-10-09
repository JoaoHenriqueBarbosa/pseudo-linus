//! Porte de `runtime/BooleanConstructor.h`, `BooleanConstructorInlines.h` e `BooleanConstructor.cpp`: o
//! construtor `Boolean` (no C++ um `JSFunction` sobre um `NativeExecutable` com `call` e `construct`
//! próprios) e `constructBooleanFromImmediateBoolean` (em `boolean_object.rs`).
//!
//! DIVERGÊNCIA: a estrutura é a `hostFunctionStructure` do global, que `JSFunction::create_native` usa
//! (o `BooleanConstructor::createStructure` só troca o `ClassInfo`, que o porte nem distingue).

use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::internal_function::get_derived_structure_in_realm;
use crate::runtime::js_function::{JSFunction, JSFunctionRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::js_boolean;
use crate::runtime::js_wrapper_object::JSWrapperObject;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::vm::VM;

// ECMA 15.6.1
fn call_boolean_constructor_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(call.argument(0).to_boolean()))
}

// ECMA 15.6.2
fn construct_with_boolean_constructor_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let boolean = js_boolean(call.argument(0).to_boolean());

    let boolean_structure = get_derived_structure_in_realm(global_object, call.new_target(), call.callee(), |realm| {
        realm.boolean_object_structure()
    })?;

    let object = JSWrapperObject::create(vm, boolean_structure);
    object.set_internal_value(boolean);
    Ok(object.as_value())
}

crate::host_function!(call_boolean_constructor, call_boolean_constructor_body);
crate::host_function!(construct_with_boolean_constructor, construct_with_boolean_constructor_body);

/// `class BooleanConstructor final : public JSFunction`: espaço de nomes de `create`.
pub struct BooleanConstructor;

impl BooleanConstructor {
    /// `create(vm, structure, booleanPrototype)`: o `NativeExecutable` com `callBooleanConstructor` e
    /// `constructWithBooleanConstructor` (comprimento 1, nome `Boolean`) e o `finishCreation`.
    pub fn create(vm: &VM, global_object: &JSGlobalObject, boolean_prototype: &JSWrapperObject) -> JSFunctionRef {
        let constructor = JSFunction::create_native(
            vm,
            global_object,
            1,
            vm.property_names.boolean.string().string(),
            call_boolean_constructor,
            ImplementationVisibility::Public,
            Intrinsic::BooleanConstructorIntrinsic,
            construct_with_boolean_constructor,
        );
        constructor.put_direct(
            vm,
            &PropertyName::from_identifier(&vm.property_names.prototype),
            boolean_prototype.as_value(),
            READ_ONLY | DONT_ENUM | DONT_DELETE,
        );
        constructor
    }
}
