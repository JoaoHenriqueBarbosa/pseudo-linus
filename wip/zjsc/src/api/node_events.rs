//! `require("events")` do bun 1.4.2: o `EventEmitter` completo.
//!
//! O módulo é o `src/js/node/events.ts` do bun portado para JavaScript comum (`node_events.js`) e avaliado como
//! programa de nome `node:events`: as funções ficam visíveis na pilha, como as do módulo no bun, e a mensagem de
//! erro cita a expressão do próprio módulo (`evaluating 'this._events'`). Este
//! arquivo só entrega ao JavaScript o que ele não consegue sozinho: criar o erro com `code` no formato do bun,
//! o `util.inspect`, os ouvintes de um `EventTarget` e o `require` de outro embutido.

use crate::api::builtin_modules::{load, resolve, Resolved};
use crate::api::eval::{describe_exception, evaluate, js_text, key_of, named_program_source};
use crate::runtime::current_realm::current_global_object;
use crate::runtime::event_target::{is_target, kind_argument, listener_callbacks};
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_microtask::call_microtask;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::node_error::{throw_coded_error, throw_coded_range_error, throw_coded_type_error};
use crate::runtime::object_constructor::{construct_array_of, construct_empty_object};
use crate::runtime::timers::native_function;
use crate::runtime::util_inspect::inspect_with_options;
use crate::host_function;

const SOURCE: &str = include_str!("node_events.js");

/// `host.throwNodeError(tipo, código, mensagem)`: lança o erro com `code` que o JavaScript captura para devolver como
/// valor (o `$ERR_*` do bun).
fn throw_node_error_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let kind = rust_string(&call.argument(0).to_wtf_string());
    let code = rust_string(&call.argument(1).to_wtf_string());
    let message = rust_string(&call.argument(2).to_wtf_string());
    Err(match kind.as_str() {
        "TypeError" => throw_coded_type_error(global_object, &message, &code),
        "RangeError" => throw_coded_range_error(global_object, &message, &code),
        _ => throw_coded_error(global_object, &message, &code),
    })
}
host_function!(throw_node_error, throw_node_error_body);

/// `host.inspect(valor, opções)`: o `util.inspect`.
fn inspect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let text = inspect_with_options(global_object, call.argument(0), call.argument(1))?;
    Ok(js_text(global_object.vm(), &text))
}
host_function!(inspect, inspect_body);

/// `host.isEventTarget(valor)`.
fn is_event_target_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(js_boolean(is_target(call.argument(0))))
}
host_function!(is_event_target, is_event_target_body);

/// Os ouvintes de `addEventListener` de `target` no tipo `type`; `None` quando `target` não é um `EventTarget`.
fn event_target_callbacks(global_object: &JSGlobalObject, call: &HostCall) -> Result<Option<Vec<JSValue>>, crate::runtime::host_call::Thrown> {
    let kind = kind_argument(global_object, call.argument(1))?;
    Ok(listener_callbacks(call.argument(0), &kind))
}

/// `host.eventTargetListeners(alvo, tipo)`: `getEventListeners` de um `EventTarget` (`[]` quando não é um).
fn event_target_listeners_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let callbacks = event_target_callbacks(global_object, call)?.unwrap_or_default();
    Ok(construct_array_of(global_object, &callbacks).as_value())
}
host_function!(event_target_listeners, event_target_listeners_body);

/// `host.eventTargetListenerCount(alvo, tipo)`: o número de ouvintes, ou `undefined` quando `alvo` não é um `EventTarget`.
fn event_target_listener_count_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    Ok(event_target_callbacks(global_object, call)?.map_or_else(JSValue::undefined, |callbacks| js_number(callbacks.len() as u32)))
}
host_function!(event_target_listener_count, event_target_listener_count_body);

/// `host.requireBuiltin(nome)`: o `require` de outro módulo embutido.
fn require_builtin_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = rust_string(&call.argument(0).to_wtf_string());
    match resolve(&name) {
        Resolved::Builtin(entry) => load(global_object, entry),
        _ => Err(throw_coded_error(global_object, &format!("Cannot find module '{name}'"), "MODULE_NOT_FOUND")),
    }
}
host_function!(require_builtin, require_builtin_body);

/// Instalador de `node:events`: avalia o programa escondido e chama a fábrica com as funções do hospedeiro.
pub(crate) fn install_events_module(global_object: &JSGlobalObject) -> HostResult {
    let vm = global_object.vm();
    let realm = current_global_object();
    let factory = evaluate(&realm, &named_program_source(SOURCE, "node:events")).unwrap_or_else(|error| panic!("node_events.js: {}", describe_exception(&error)));
    let host = construct_empty_object(global_object);
    let members: [(&str, u32, NativeFunction); 6] = [
        ("throwNodeError", 3, throw_node_error),
        ("inspect", 2, inspect),
        ("isEventTarget", 1, is_event_target),
        ("eventTargetListeners", 2, event_target_listeners),
        ("eventTargetListenerCount", 2, event_target_listener_count),
        ("requireBuiltin", 1, require_builtin),
    ];
    for (name, length, function) in members {
        host.put_direct(vm, &key_of(vm, name), native_function(global_object, name, length, function).as_value(), 0);
    }
    // A fábrica só monta o módulo; falhar aqui é defeito do `node_events.js`, não do programa.
    Ok(call_microtask(global_object, factory, JSValue::undefined(), &[host.as_value()], "node:events não é uma função")
        .unwrap_or_else(|thrown| panic!("node_events.js: a fábrica lançou {thrown:?}")))
}
