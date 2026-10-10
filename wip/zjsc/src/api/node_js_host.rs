//! O que os módulos embutidos escritos em JavaScript (`events`, `stream`) pedem ao hospedeiro: o objeto `host` com as
//! funções nativas que o JavaScript não faz sozinho (criar o erro com `code` no formato do bun, o `util.inspect`, o
//! `require` de outro embutido), o `node_internal.js` (erros, validadores e afins, montado uma vez por programa) e a
//! avaliação do fonte como o programa de nome `node:<módulo>`, que devolve a fábrica chamada com o `host`.

use crate::api::builtin_modules::{cached, load, resolve, Resolved};
use crate::api::eval::{describe_exception, evaluate, js_text, key_of, named_program_source};
use crate::host_function;
use crate::runtime::current_realm::current_global_object;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_microtask::call_microtask;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_object::JSObjectRef;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::node_error::{throw_coded_error, throw_coded_range_error, throw_coded_type_error};
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::timers::native_function;
use crate::runtime::util_inspect::inspect_with_options;

const INTERNAL_SOURCE: &str = include_str!("node_internal.js");

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

/// `host.requireBuiltin(nome)`: o `require` de outro módulo embutido.
fn require_builtin_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = rust_string(&call.argument(0).to_wtf_string());
    match resolve(&name) {
        Resolved::Builtin(entry) => load(global_object, entry),
        _ => Err(throw_coded_error(global_object, &format!("Cannot find module '{name}'"), "MODULE_NOT_FOUND")),
    }
}
host_function!(require_builtin, require_builtin_body);

/// Põe as funções `members` (nome, `length`, corpo) em `host`.
pub(crate) fn put_functions(global_object: &JSGlobalObject, host: &JSObjectRef, members: &[(&str, u32, NativeFunction)]) {
    let vm = global_object.vm();
    for (name, length, function) in members {
        host.put_direct(vm, &key_of(vm, name), native_function(global_object, name, *length, *function).as_value(), 0);
    }
}

/// O `host` sem o `internal`: o que o próprio `node_internal.js` recebe.
fn base_host(global_object: &JSGlobalObject) -> JSObjectRef {
    let host = construct_empty_object(global_object);
    put_functions(global_object, &host, &[("throwNodeError", 3, throw_node_error), ("inspect", 2, inspect), ("requireBuiltin", 1, require_builtin)]);
    host
}

/// Avalia `source` como o programa `node:<name>` e chama a fábrica que ele devolve com `host`. Falhar aqui é defeito do
/// fonte embutido, não do programa do usuário.
pub(crate) fn run_factory(global_object: &JSGlobalObject, source: &str, name: &str, host: &JSObjectRef) -> JSValue {
    let realm = current_global_object();
    let factory = evaluate(&realm, &named_program_source(source, &format!("node:{name}")))
        .unwrap_or_else(|error| panic!("node:{name}: {}", describe_exception(&error)));
    call_microtask(global_object, factory, JSValue::undefined(), &[host.as_value()], "o módulo embutido não é uma função")
        .unwrap_or_else(|thrown| panic!("node:{name}: a fábrica lançou {thrown:?}"))
}

/// O `node_internal.js`: erros com código, validadores, `addAbortListener` e afins, uma vez por programa.
pub(crate) fn internal(global_object: &JSGlobalObject) -> HostResult {
    cached("internal:node", || Ok(run_factory(global_object, INTERNAL_SOURCE, "internal", &base_host(global_object))))
}

/// O `host` completo de um módulo: o básico mais o `internal` e as funções próprias do módulo.
pub(crate) fn module_host(global_object: &JSGlobalObject, extra: &[(&str, u32, NativeFunction)]) -> Result<JSObjectRef, crate::runtime::host_call::Thrown> {
    let host = base_host(global_object);
    let vm = global_object.vm();
    host.put_direct(vm, &key_of(vm, "internal"), internal(global_object)?, 0);
    put_functions(global_object, &host, extra);
    Ok(host)
}
