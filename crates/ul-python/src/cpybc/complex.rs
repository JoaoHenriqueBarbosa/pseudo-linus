//! `complex` constante do `co_consts`: o literal `2j` e o que o otimizador de AST dobra (`1 + 2j`, `-2j`).
//!
//! O tipo `complex` do programa em execução vive em `modules/py/_complex.py` (o literal compila para uma chamada
//! de `complex(re, im)`); aqui só existe o valor que o objeto `code` mostra em `co_consts`: `repr`, igualdade e
//! `hash` do `Objects/complexobject.c`. A aritmética do dobramento segue `_Py_c_sum`, `_Py_c_diff`, `_Py_c_prod` e
//! `_Py_c_quot`.

use std::any::Any;
use std::rc::Rc;

use super::{Cv, Key, Res, Unsupported};
use crate::ast::{Constant, Operator, UnaryOp};
use crate::object::{float_hash, ExtImage, ExtObject, OpaqueImage, Value};

/// Um `complex` guardado em `co_consts`.
pub(super) struct ComplexConst {
    pub(super) re: f64,
    pub(super) im: f64,
}

/// `_PyHASH_IMAG` do `Include/internal/pycore_pyhash.h`.
const HASH_IMAG: u64 = 1_000_003;

impl ExtObject for ComplexConst {
    fn type_name(&self) -> &'static str {
        "complex"
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }

    fn repr(&self) -> String {
        crate::ast::constant_repr(&Constant::Complex(self.re, self.im))
    }

    fn is_true(&self) -> bool {
        self.re != 0.0 || self.im != 0.0
    }

    fn hash_value(&self) -> Option<i64> {
        let combined = (float_hash(self.re) as u64).wrapping_add(HASH_IMAG.wrapping_mul(float_hash(self.im) as u64)) as i64;
        Some(if combined == -1 { -2 } else { combined })
    }

    fn eq_value(&self, other: &Value) -> Option<bool> {
        let (re, im) = parts(other)?;
        Some(self.re == re && self.im == im)
    }

    /// `co_consts[i] == 2j`: o literal em execução é uma instância de `complex` de `_complex.py`, e o `richcmp` da
    /// extensão decide a igualdade antes do `__eq__` dela.
    fn richcmp(&self, op: &str, other: &Value) -> Option<Result<bool, crate::vm::PyException>> {
        let same = self.eq_value(other)?;
        match op {
            "==" => Some(Ok(same)),
            "!=" => Some(Ok(!same)),
            _ => None,
        }
    }

    fn getattr(&self, _vm: &mut crate::vm::Vm, name: &str) -> Option<Result<Value, crate::vm::PyException>> {
        match name {
            "real" => Some(Ok(Value::Float(self.re))),
            "imag" => Some(Ok(Value::Float(self.im))),
            _ => None,
        }
    }

    fn image(&self) -> Option<ExtImage> {
        OpaqueImage::image("complex_const", (self.re.to_bits(), self.im.to_bits()), Vec::new())
    }
}

/// Refaz o `complex` constante a partir da imagem do heap (o inverso de [`ExtObject::image`]).
pub(crate) fn restore_image(_tag: &str, state: &(dyn Any + Send + Sync), _refs: Vec<Value>) -> Option<Value> {
    let &(re, im) = state.downcast_ref::<(u64, u64)>()?;
    Some(Cv::complex(f64::from_bits(re), f64::from_bits(im)).value)
}

/// As partes real e imaginária de um valor numérico (`bool`, `int`, `float` ou outro `complex` constante).
fn parts(v: &Value) -> Option<(f64, f64)> {
    match v {
        Value::Bool(b) => Some((f64::from(u8::from(*b)), 0.0)),
        Value::Int(n) => Some((*n as f64, 0.0)),
        Value::Float(x) => Some((*x, 0.0)),
        Value::Ext(e) => e.as_any()?.downcast_ref::<ComplexConst>().map(|c| (c.re, c.im)),
        // O `complex` de `_complex.py` (o literal em execução) guarda `real` e `imag` como `float`.
        Value::Instance(i) if i.class().name == "complex" => {
            let slots = i.dict.borrow();
            match (slots.get("real"), slots.get("imag")) {
                (Some(Value::Float(re)), Some(Value::Float(im))) => Some((*re, *im)),
                _ => None,
            }
        }
        _ => None,
    }
}

impl Cv {
    pub(super) fn complex(re: f64, im: f64) -> Cv {
        Cv { key: Key::Complex(re.to_bits(), im.to_bits()), value: Value::Ext(Rc::new(ComplexConst { re, im })) }
    }
}

/// `(re, im)` de uma constante numérica dobrável (`int`, `bool`, `float`, `complex`); `None` nos outros tipos.
fn operand(k: &Key) -> Option<(f64, f64)> {
    match k {
        Key::Complex(re, im) => Some((f64::from_bits(*re), f64::from_bits(*im))),
        Key::Int(n) => Some((*n as f64, 0.0)),
        Key::Bool(b) => Some((f64::from(u8::from(*b)), 0.0)),
        Key::Float(bits) => Some((f64::from_bits(*bits), 0.0)),
        _ => None,
    }
}

/// `_Py_c_quot`; `None` é a divisão por zero (o CPython deixa a expressão como está).
fn quot(a: (f64, f64), b: (f64, f64)) -> Option<(f64, f64)> {
    let (abs_re, abs_im) = (b.0.abs(), b.1.abs());
    if abs_re >= abs_im {
        if abs_re == 0.0 {
            return None;
        }
        let ratio = b.1 / b.0;
        let denom = b.0 + b.1 * ratio;
        Some(((a.0 + a.1 * ratio) / denom, (a.1 - a.0 * ratio) / denom))
    } else if abs_im >= abs_re {
        let ratio = b.0 / b.1;
        let denom = b.0 * ratio + b.1;
        Some(((a.0 * ratio + a.1) / denom, (a.1 * ratio - a.0) / denom))
    } else {
        Some((f64::NAN, f64::NAN))
    }
}

/// O dobramento de `l op r` quando um dos lados é `complex` (e o outro é número). `Ok(None)`: a operação falha em
/// tempo de execução; `Err`: dobraria, mas o emissor não calcula (`**`).
pub(super) fn fold_binop(op: Operator, l: &Key, r: &Key) -> Res<Option<Cv>> {
    let (Some(a), Some(b)) = (operand(l), operand(r)) else { return Ok(None) };
    let (re, im) = match op {
        Operator::Add => (a.0 + b.0, a.1 + b.1),
        Operator::Sub => (a.0 - b.0, a.1 - b.1),
        Operator::Mult => (a.0 * b.0 - a.1 * b.1, a.0 * b.1 + a.1 * b.0),
        Operator::Div => match quot(a, b) {
            Some(q) => q,
            None => return Ok(None),
        },
        Operator::Pow => return Err(Unsupported),
        // `//`, `%`, `@` e as operações de bits dão `TypeError` em tempo de execução.
        _ => return Ok(None),
    };
    Ok(Some(Cv::complex(re, im)))
}

/// O dobramento de `-x` e `+x` quando `x` é `complex`.
pub(super) fn fold_unary(op: UnaryOp, k: &Key) -> Option<Cv> {
    let Key::Complex(re, im) = k else { return None };
    let (re, im) = (f64::from_bits(*re), f64::from_bits(*im));
    match op {
        UnaryOp::USub => Some(Cv::complex(-re, -im)),
        UnaryOp::UAdd => Some(Cv::complex(re, im)),
        _ => None,
    }
}

/// A verdade de um `complex` constante.
pub(super) fn truth(re: u64, im: u64) -> bool {
    f64::from_bits(re) != 0.0 || f64::from_bits(im) != 0.0
}
