//! O gancho do hospedeiro sobre o código que nasce de texto (`eval`, direto e indireto, e o construtor de `Function`):
//! o que o `Options::useTrustedTypes()` e o `canCompileStrings` do `JSGlobalObjectMethodTable` do JSC fazem no
//! WebKit. O hospedeiro registra uma função do programa (por `__pl_set_code_guard`); antes de compilar um texto o motor
//! a chama com o tipo da operação (`"eval"` ou `"Function"`) e o valor. Ela devolve `undefined` para seguir com o valor
//! original, ou o texto que o substitui (o código de um `TrustedScript`, ou o que a política padrão devolveu), e lança
//! o `EvalError` que o hospedeiro quiser. Sem função registrada, nada muda.
//!
//! O valor guardado fica em `thread_local` (zerado no fim do programa), como o resto do estado do hospedeiro.

use std::cell::RefCell;

use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::object_to_primitive::call_function;
use crate::wtf::text::wtf_string::String as WtfString;

thread_local! {
    static GUARD: RefCell<Option<JSValue>> = const { RefCell::new(None) };
}

/// Registra (ou, com `None`, remove) a função de guarda.
pub fn set_code_guard(function: Option<JSValue>) {
    GUARD.with(|guard| *guard.borrow_mut() = function);
}

/// Fim do programa: o gancho some com ele.
pub(crate) fn reset_for_program() {
    let _ = GUARD.try_with(|guard| guard.borrow_mut().take());
}

fn registered() -> Option<JSValue> {
    GUARD.try_with(|guard| *guard.borrow()).ok().flatten()
}

/// O argumento do `eval` depois da guarda: o próprio valor (inclusive o que não é texto, que o `eval` devolve como
/// está) ou o texto que a função de guarda escolheu. `Err(())` quando ela lançou: a exceção fica pendente no `VM`.
pub fn guard_eval(global_object: &JSGlobalObject, value: JSValue) -> Result<JSValue, ()> {
    let Some(function) = registered() else { return Ok(value) };
    let kind = JSValue::from_js_string(crate::runtime::js_string::js_string(global_object.vm(), &WtfString::from_utf8(b"eval")));
    match call_function(global_object, function, JSValue::undefined(), &[kind, value]) {
        Some(replacement) if replacement.is_undefined() => Ok(value),
        Some(replacement) => Ok(replacement),
        None => Err(()),
    }
}

/// O texto completo que o construtor de `Function` vai compilar (`(function anonymous(a\n) {\n...\n})`) depois da
/// guarda.
pub fn guard_function_source(global_object: &JSGlobalObject, source: WtfString) -> Result<WtfString, ()> {
    let Some(function) = registered() else { return Ok(source) };
    let vm = global_object.vm();
    let kind = JSValue::from_js_string(crate::runtime::js_string::js_string(vm, &WtfString::from_utf8(b"Function")));
    let text = JSValue::from_js_string(crate::runtime::js_string::js_string(vm, &source));
    match call_function(global_object, function, JSValue::undefined(), &[kind, text]) {
        Some(replacement) if replacement.is_undefined() => Ok(source),
        Some(replacement) => Ok(replacement.to_wtf_string()),
        None => Err(()),
    }
}
