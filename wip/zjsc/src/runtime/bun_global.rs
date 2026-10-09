//! O global `Bun` do bun. Só o que o porte já sustenta: `Bun.inspect(value)`, o formatador do `console.log` (um
//! `Buffer` sai como `<Buffer 61 62 63>`). O resto da API do bun (servidor,
//! arquivos, transpilador) não é do motor JavaScript e entra quando o runtime ganhar essas camadas.
//!
//! Propriedade de dados comum do global; na ordem de chaves vem entre `global` e `File`, o que a tabela `ORDER` de
//! `js_global_object_init.rs` já resolve.

use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::identifier::Identifier;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::put_direct_native_function_with_display_name;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::property_name::PropertyName;
use crate::runtime::console_client::format_arguments;
use crate::runtime::host_call::Thrown;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::wtf::text::wtf_string::String as WtfString;

/// `Bun.inspect(value, options)`.
fn inspect_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    // O formatador do `console.log` (strings entre aspas duplas, arrays e objetos longos em linhas), que é o do bun;
    // `util.inspect` tem outro estilo. As opções (`depth`, `colors`) ainda não são lidas.
    let Some(text) = format_arguments(global_object, &[call.argument(0)]) else { return Err(Thrown::Pending) };
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(text.as_bytes()))))
}

host_function!(bun_inspect, inspect_body);

pub fn install_bun(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let bun = construct_empty_object(global_object);
    put_direct_native_function_with_display_name(
        vm,
        global_object,
        &bun,
        &Identifier::from_span(vm, b"inspect"),
        &WtfString::from_latin1(b"inspect"),
        2,
        bun_inspect as NativeFunction,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        0,
    );
    global_object.put_direct(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"Bun")), bun.as_value(), 0);
}
