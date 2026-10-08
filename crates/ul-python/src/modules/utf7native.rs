//! `_utf7`: a decodificação com estado do codec `utf-7`, que o `codecs.utf_7_decode` e o decodificador
//! incremental usam (a de `bytes.decode` passa direto por `textcodec`).

use std::rc::Rc;

use crate::modules::binascii::want_bytes;
use crate::modules::ModuleBuilder;
use crate::native_util::bind;
use crate::object::{Kw, ModuleObj, Value};
use crate::textcodec::decode_utf7;
use crate::vm::{type_error, PyResult, Vm};

/// `decode(data, errors, final)`: `(texto, bytes consumidos)`.
fn decode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("utf_7_decode", args, kw, &["data", "errors", "final"], 1)?;
    let data = want_bytes(s[0].as_ref().unwrap())?;
    let errors = match &s[1] {
        None | Some(Value::None) => "strict".to_string(),
        Some(Value::Str(e)) => e.as_str().to_string(),
        Some(other) => {
            return Err(type_error(format!("utf_7_decode() argument 2 must be str or None, not {}", other.type_name())))
        }
    };
    let final_ = s[2].as_ref().is_some_and(|v| v.is_true());
    let (text, consumed) = decode_utf7(&data, &errors, final_)?;
    Ok(Value::tuple(vec![Value::str(text), Value::Int(consumed as i64)]))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_utf7").func("decode", decode).build()
}
