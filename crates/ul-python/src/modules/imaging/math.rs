//! `PIL._imagingmath` e `PIL._imagingmorph` (`_imagingmath.c` e `_imagingmorph.c` do Pillow
//! 11.1.0), mais a cápsula `PyCapsule` que o `Image.getim()` devolve.
//!
//! No original as operações do `_imagingmath` são ponteiros de função guardados em cápsulas com o
//! nome `"Pillow Math unary func"` ou `"Pillow Math binary func"`; o `ImageMath` escolhe a
//! cápsula pelo nome (`add_I`, `div_F`...) e chama `unop`/`binop` com as cápsulas das imagens. Aqui
//! a cápsula guarda o nome da operação, e a aritmética reproduz a do C: inteiros de 32 bits com
//! estouro em complemento de dois, `float` de 32 bits, divisão e resto por zero dando zero.
//!
//! Portado do Pillow (MIT-CMU: Copyright © 1999-2005 Secret Labs AB, © 2005 Fredrik Lundh, ©
//! 2014 Dov Grobgeld).

use std::rc::Rc;

use super::image::{Image, PixType};
use super::Shared;
use crate::modules::ModuleBuilder;
use crate::object::{ExtObject, Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

const IMAGING_MAGIC: &str = "Pillow Imaging";
const UNOP_MAGIC: &str = "Pillow Math unary func";
const BINOP_MAGIC: &str = "Pillow Math binary func";

enum Payload {
    Image(Shared),
    Unop(&'static str),
    Binop(&'static str),
}

/// `PyCapsule`: um ponteiro opaco com nome.
pub struct Capsule {
    payload: Payload,
}

impl Capsule {
    fn magic(&self) -> &'static str {
        match self.payload {
            Payload::Image(_) => IMAGING_MAGIC,
            Payload::Unop(_) => UNOP_MAGIC,
            Payload::Binop(_) => BINOP_MAGIC,
        }
    }
}

impl ExtObject for Capsule {
    fn type_name(&self) -> &'static str {
        "PyCapsule"
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn repr(&self) -> String {
        let addr = match &self.payload {
            Payload::Image(im) => Rc::as_ptr(im) as usize,
            Payload::Unop(n) | Payload::Binop(n) => n.as_ptr() as usize,
        };
        format!("<capsule object \"{}\" at 0x{:x}>", self.magic(), crate::object::py_addr(addr))
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        Err(exc("AttributeError", format!("'PyCapsule' object has no attribute '{name}'")))
    }
}

/// A cápsula do `ImagingCore.ptr`.
pub fn image_capsule(im: Shared) -> Value {
    Value::Ext(Rc::new(Capsule { payload: Payload::Image(im) }))
}

fn capsule(v: &Value) -> Option<&Capsule> {
    match v {
        Value::Ext(e) => e.as_any().and_then(|a| a.downcast_ref::<Capsule>()),
        _ => None,
    }
}

/// `PyCapsule_IsValid(v, IMAGING_MAGIC)` seguido do `PyCapsule_GetPointer`.
fn image_of(v: &Value) -> PyResult<Shared> {
    match capsule(v).map(|c| &c.payload) {
        Some(Payload::Image(im)) => Ok(im.clone()),
        _ => Err(type_error(format!("Expected '{IMAGING_MAGIC}' Capsule"))),
    }
}

const UNARY: &[&str] = &["abs_I", "neg_I", "invert_I", "abs_F", "neg_F"];
const BINARY: &[&str] = &[
    "add_I", "sub_I", "diff_I", "mul_I", "div_I", "mod_I", "min_I", "max_I", "pow_I", "and_I", "or_I", "xor_I",
    "lshift_I", "rshift_I", "eq_I", "ne_I", "lt_I", "le_I", "gt_I", "ge_I", "add_F", "sub_F", "diff_F", "mul_F",
    "div_F", "mod_F", "min_F", "max_F", "pow_F", "eq_F", "ne_F", "lt_F", "le_F", "gt_F", "ge_F",
];

/// `powi`: `pow` em `double` mais 0,5, saturado no intervalo de `int32`.
fn powi(x: i32, y: i32) -> i32 {
    let v = f64::from(x).powf(f64::from(y)) + 0.5;
    if v.is_nan() {
        // `pow` com resultado fora do domínio dá `EDOM`, e o original devolve 0.
        return 0;
    }
    v.clamp(-2147483648.0, 2147483647.0) as i32
}

fn int_unop(op: &str, a: i32) -> i32 {
    match op {
        "abs_I" => a.wrapping_abs(),
        "neg_I" => a.wrapping_neg(),
        _ => !a,
    }
}

fn int_binop(op: &str, a: i32, b: i32) -> i32 {
    match op {
        "add_I" => a.wrapping_add(b),
        "sub_I" => a.wrapping_sub(b),
        "mul_I" => a.wrapping_mul(b),
        "div_I" => if b != 0 { a.wrapping_div(b) } else { 0 },
        "mod_I" => if b != 0 { a.wrapping_rem(b) } else { 0 },
        "pow_I" => powi(a, b),
        "diff_I" => a.wrapping_sub(b).wrapping_abs(),
        "and_I" => a & b,
        "or_I" => a | b,
        "xor_I" => a ^ b,
        "lshift_I" => a.wrapping_shl(b as u32),
        "rshift_I" => a.wrapping_shr(b as u32),
        "min_I" => a.min(b),
        "max_I" => a.max(b),
        "eq_I" => i32::from(a == b),
        "ne_I" => i32::from(a != b),
        "lt_I" => i32::from(a < b),
        "le_I" => i32::from(a <= b),
        "gt_I" => i32::from(a > b),
        _ => i32::from(a >= b),
    }
}

fn float_unop(op: &str, a: f32) -> f32 {
    if op == "abs_F" { a.abs() } else { -a }
}

fn float_binop(op: &str, a: f32, b: f32) -> f32 {
    match op {
        "add_F" => a + b,
        "sub_F" => a - b,
        "mul_F" => a * b,
        "div_F" => if b != 0.0 { a / b } else { 0.0 },
        // `fmod` em `double` com os operandos promovidos, e o resultado volta a `float`.
        "mod_F" => if b != 0.0 { (f64::from(a) % f64::from(b)) as f32 } else { 0.0 },
        "pow_F" => a.powf(b),
        "diff_F" => (a - b).abs(),
        "min_F" => if a < b { a } else { b },
        "max_F" => if a > b { a } else { b },
        "eq_F" => f32::from(u8::from(a == b)),
        "ne_F" => f32::from(u8::from(a != b)),
        "lt_F" => f32::from(u8::from(a < b)),
        "le_F" => f32::from(u8::from(a <= b)),
        "gt_F" => f32::from(u8::from(a > b)),
        _ => f32::from(u8::from(a >= b)),
    }
}

fn word(im: &Image, x: i32, y: i32) -> [u8; 4] {
    let o = im.offset(x, y);
    [im.data[o], im.data[o + 1], im.data[o + 2], im.data[o + 3]]
}

/// Aplica a operação pixel a pixel no tamanho de `out`, lendo as entradas antes de escrever (a
/// saída pode ser a mesma imagem de uma das entradas).
fn apply(op: &str, out: &Shared, ins: &[Shared]) {
    let float = op.ends_with("_F");
    let (xs, ys) = {
        let o = out.borrow();
        (o.xsize, o.ysize)
    };
    let mut result = Vec::with_capacity((xs.max(0) * ys.max(0)) as usize);
    {
        let ins: Vec<std::cell::Ref<Image>> = ins.iter().map(|i| i.borrow()).collect();
        for y in 0..ys {
            for x in 0..xs {
                let v = if float {
                    let a = f32::from_le_bytes(word(&ins[0], x, y));
                    let r = if ins.len() == 1 {
                        float_unop(op, a)
                    } else {
                        float_binop(op, a, f32::from_le_bytes(word(&ins[1], x, y)))
                    };
                    r.to_le_bytes()
                } else {
                    let a = i32::from_le_bytes(word(&ins[0], x, y));
                    let r = if ins.len() == 1 {
                        int_unop(op, a)
                    } else {
                        int_binop(op, a, i32::from_le_bytes(word(&ins[1], x, y)))
                    };
                    r.to_le_bytes()
                };
                result.push(v);
            }
        }
    }
    let mut o = out.borrow_mut();
    let mut it = result.into_iter();
    for y in 0..ys {
        for x in 0..xs {
            let off = o.offset(x, y);
            if let Some(v) = it.next() {
                o.data[off..off + 4].copy_from_slice(&v);
            }
        }
    }
}

fn unop(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    if args.len() != 3 {
        return Err(type_error(format!("function takes exactly 3 arguments ({} given)", args.len())));
    }
    let op = match capsule(&args[0]).map(|c| &c.payload) {
        Some(Payload::Unop(name)) => *name,
        _ => return Err(type_error(format!("Expected '{UNOP_MAGIC}' Capsule"))),
    };
    let (out, im1) = (image_of(&args[1])?, image_of(&args[2])?);
    apply(op, &out, &[im1]);
    Ok(Value::None)
}

fn binop(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    if args.len() != 4 {
        return Err(type_error(format!("function takes exactly 4 arguments ({} given)", args.len())));
    }
    let op = match capsule(&args[0]).map(|c| &c.payload) {
        Some(Payload::Binop(name)) => *name,
        _ => return Err(type_error(format!("Expected '{BINOP_MAGIC}' Capsule"))),
    };
    let (out, im1, im2) = (image_of(&args[1])?, image_of(&args[2])?, image_of(&args[3])?);
    apply(op, &out, &[im1, im2]);
    Ok(Value::None)
}

pub fn build_math(_vm: &mut Vm) -> Rc<ModuleObj> {
    let mut b = ModuleBuilder::new("PIL._imagingmath").func("unop", unop).func("binop", binop);
    for &name in UNARY {
        b = b.value(name, Value::Ext(Rc::new(Capsule { payload: Payload::Unop(name) })));
    }
    for &name in BINARY {
        b = b.value(name, Value::Ext(Rc::new(Capsule { payload: Payload::Binop(name) })));
    }
    b.build()
}

// ---- _imagingmorph ----

const LUT_SIZE: usize = 1 << 9;

/// O `s#` do `PyArg_ParseTuple`: `bytes` (ou `str` em UTF-8).
fn lut_arg(v: &Value) -> PyResult<Vec<u8>> {
    let lut = match v {
        Value::Bytes(b) => b.to_vec(),
        Value::Str(s) => s.as_str().as_bytes().to_vec(),
        other => return Err(type_error(format!("argument 1 must be read-only bytes-like object, not {}", other.type_name()))),
    };
    if lut.len() < LUT_SIZE {
        return Err(exc("RuntimeError", "The morphology LUT has the wrong size"));
    }
    Ok(lut)
}

fn check_l(im: &Image) -> PyResult<()> {
    if im.kind != PixType::Uint8 || im.bands != 1 {
        return Err(exc("RuntimeError", "Unsupported image type"));
    }
    Ok(())
}

/// O índice de 9 bits da vizinhança 3x3 de `(x, y)`.
fn neighborhood(im: &Image, x: i32, y: i32) -> usize {
    let bit = |xx: i32, yy: i32| usize::from(im.data[im.offset(xx, yy)] & 1);
    bit(x - 1, y - 1)
        | bit(x, y - 1) << 1
        | bit(x + 1, y - 1) << 2
        | bit(x - 1, y) << 3
        | bit(x, y) << 4
        | bit(x + 1, y) << 5
        | bit(x - 1, y + 1) << 6
        | bit(x, y + 1) << 7
        | bit(x + 1, y + 1) << 8
}

fn coord(x: i32, y: i32) -> Value {
    Value::tuple(vec![Value::Int(i64::from(x)), Value::Int(i64::from(y))])
}

fn morph_apply(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    if args.len() != 3 {
        return Err(type_error(format!("function takes exactly 3 arguments ({} given)", args.len())));
    }
    let lut = lut_arg(&args[0])?;
    let (src, dst) = (image_of(&args[1])?, image_of(&args[2])?);
    // Cópia da entrada: a saída pode ser a mesma imagem.
    let imgin = src.borrow().clone();
    let mut imgout = dst.borrow_mut();
    check_l(&imgin)?;
    check_l(&imgout)?;
    let (width, height) = (imgin.xsize, imgin.ysize);
    let mut changed = 0i64;
    for y in 0..height {
        let first = imgout.offset(0, y);
        imgout.data[first] = 0;
        let last = imgout.offset(width - 1, y);
        imgout.data[last] = 0;
        if y == 0 || y == height - 1 {
            for x in 0..width {
                let o = imgout.offset(x, y);
                imgout.data[o] = 0;
            }
            continue;
        }
        for x in 1..width - 1 {
            let idx = neighborhood(&imgin, x, y);
            let v = 255u8.wrapping_mul(lut[idx] & 1);
            let o = imgout.offset(x, y);
            imgout.data[o] = v;
            changed += i64::from((imgin.data[imgin.offset(x, y)] & 1) != (v & 1));
        }
    }
    Ok(Value::Int(changed))
}

fn morph_match(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    if args.len() != 2 {
        return Err(type_error(format!("function takes exactly 2 arguments ({} given)", args.len())));
    }
    let lut = lut_arg(&args[0])?;
    let src = image_of(&args[1])?;
    let im = src.borrow();
    check_l(&im)?;
    let mut out = Vec::new();
    for y in 1..im.ysize - 1 {
        for x in 1..im.xsize - 1 {
            if lut[neighborhood(&im, x, y)] != 0 {
                out.push(coord(x, y));
            }
        }
    }
    Ok(Value::list(out))
}

fn get_on_pixels(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    if args.len() != 1 {
        return Err(type_error(format!("function takes exactly 1 argument ({} given)", args.len())));
    }
    let src = image_of(&args[0])?;
    let im = src.borrow();
    let mut out = Vec::new();
    for y in 0..im.ysize {
        for x in 0..im.xsize {
            if im.data[im.offset(x, y)] != 0 {
                out.push(coord(x, y));
            }
        }
    }
    Ok(Value::list(out))
}

pub fn build_morph(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("PIL._imagingmorph")
        .func("apply", morph_apply)
        .func("match", morph_match)
        .func("get_on_pixels", get_on_pixels)
        .build()
}
