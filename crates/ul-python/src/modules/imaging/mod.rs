//! `PIL._imaging`: o núcleo nativo do Pillow 11.1.0, portado da libImaging (MIT-CMU: Copyright ©
//! 1997-2011 Secret Labs AB, © 1995-2011 Fredrik Lundh e colaboradores, © 2010 Jeffrey A. Clark e
//! colaboradores). Os arquivos Python do Pillow vêm sem mudança (`py/PIL`); este módulo faz o papel
//! do `_imaging.c`, com o armazenamento, as conversões, o desenho, o redimensionamento e os codecs
//! nos submódulos.
//!
//! Objetos expostos: `ImagingCore` (a imagem), `ImagingDraw` (o `ImageDraw`), `ImagingFont` (fonte
//! bitmap do `ImageFont.load_default` sem FreeType), `PixelAccess` (o `Image.load()`) e os
//! codificadores e decodificadores `raw` e `zip`.

pub mod codec;
pub mod convert;
pub mod draw;
pub mod image;
pub mod math;
pub mod ops;
pub mod pack;
pub mod paste;
pub mod path;
pub mod reduce;
pub mod resample;

use std::cell::RefCell;
use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::native_util::bind;
use crate::object::{ExtObject, Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

use codec::{CodecState, JpegDecoder, JpegEncoder, RawDecoder, ZipDecoder, ZipEncoder};
use image::{Image, Palette, PixType};

pub const PILLOW_VERSION: &str = "11.1.0";

type Shared = Rc<RefCell<Image>>;

fn value_error(msg: impl Into<String>) -> crate::vm::PyException {
    exc("ValueError", msg.into())
}

fn verr(r: Result<Image, String>) -> PyResult<Image> {
    r.map_err(value_error)
}

// ---- conversão de argumentos (o `PyArg_ParseTuple` do original) ----

/// `i`: inteiro; `float` é recusado como no CPython.
fn int_arg(v: &Value) -> PyResult<i64> {
    match v {
        Value::Int(n) => Ok(*n),
        Value::Bool(b) => Ok(i64::from(*b)),
        Value::Float(_) => Err(type_error("'float' object cannot be interpreted as an integer")),
        Value::Big(_) => Err(exc("OverflowError", "signed integer is greater than maximum")),
        Value::Instance(i) if matches!(&*i.payload.borrow(), Some(Value::Int(_) | Value::Bool(_) | Value::Big(_))) => {
            let inner = i.payload.borrow().clone().unwrap_or(Value::None);
            int_arg(&inner)
        }
        other => Err(type_error(format!("'{}' object cannot be interpreted as an integer", other.type_name()))),
    }
}

fn i32_arg(v: &Value) -> PyResult<i32> {
    let n = int_arg(v)?;
    i32::try_from(n).map_err(|_| exc("OverflowError", "signed integer is greater than maximum"))
}

/// `f`/`d`: número.
fn float_arg(v: &Value) -> PyResult<f64> {
    match v {
        Value::Float(f) => Ok(*f),
        Value::Int(n) => Ok(*n as f64),
        Value::Bool(b) => Ok(f64::from(u8::from(*b))),
        Value::Instance(i) if matches!(&*i.payload.borrow(), Some(Value::Int(_) | Value::Bool(_) | Value::Float(_))) => {
            let inner = i.payload.borrow().clone().unwrap_or(Value::None);
            float_arg(&inner)
        }
        other => Err(type_error(format!("must be real number, not {}", other.type_name()))),
    }
}

fn str_arg(v: &Value) -> PyResult<String> {
    match v {
        Value::Str(s) => Ok(s.as_str().to_string()),
        other => Err(type_error(format!("argument must be str, not {}", other.type_name()))),
    }
}

fn bytes_arg(v: &Value) -> PyResult<Vec<u8>> {
    v.bytes_like()
        .map(|b| b.to_vec())
        .ok_or_else(|| type_error(format!("a bytes-like object is required, not '{}'", v.type_name())))
}

/// Uma tupla de exatamente `n` itens.
fn tuple_n(v: &Value, n: usize) -> PyResult<Vec<Value>> {
    match v {
        Value::Tuple(t) if t.len() == n => Ok(t.to_vec()),
        Value::Tuple(t) => Err(type_error(format!("function takes exactly {n} arguments ({} given)", t.len()))),
        other => Err(type_error(format!("argument must be sequence of length {n}, not {}", other.type_name()))),
    }
}

fn size_arg(v: &Value) -> PyResult<(i32, i32)> {
    let t = tuple_n(v, 2)?;
    Ok((i32_arg(&t[0])?, i32_arg(&t[1])?))
}

fn box4_i(v: &Value) -> PyResult<(i32, i32, i32, i32)> {
    let t = tuple_n(v, 4)?;
    Ok((i32_arg(&t[0])?, i32_arg(&t[1])?, i32_arg(&t[2])?, i32_arg(&t[3])?))
}

/// `_getxy`: tupla de dois números (o `float` é truncado).
fn getxy(v: &Value) -> PyResult<(i32, i32)> {
    let Value::Tuple(t) = v else {
        return Err(type_error("argument must be sequence of length 2"));
    };
    if t.len() != 2 {
        return Err(type_error("argument must be sequence of length 2"));
    }
    let one = |v: &Value| -> PyResult<i32> {
        match v {
            Value::Int(n) => Ok(*n as i32),
            Value::Bool(b) => Ok(i32::from(*b)),
            Value::Float(f) => Ok(*f as i32),
            _ => Err(type_error("an integer is required")),
        }
    };
    Ok((one(&t[0])?, one(&t[1])?))
}

/// `getlist`: os itens de uma sequência (`PySequence_Fast`).
fn getlist(v: &Value) -> PyResult<Vec<Value>> {
    match v {
        Value::List(l) => Ok(l.borrow().clone()),
        Value::Tuple(t) => Ok(t.to_vec()),
        Value::Str(_) | Value::Bytes(_) | Value::Instance(_) | Value::Ext(_) => crate::vm::iterate(v),
        _ => Err(type_error("argument must be a sequence")),
    }
}

/// `PyPath_Flatten`: lista de números ou de pares; devolve os pontos como pares de `double`.
fn flatten(v: &Value) -> PyResult<Vec<(f64, f64)>> {
    if let Some(xy) = path::as_path(v) {
        return Ok(xy.chunks(2).map(|p| (p[0], p[1])).collect());
    }
    let items = match v {
        Value::List(l) => l.borrow().clone(),
        Value::Tuple(t) => t.to_vec(),
        _ => return Err(type_error("argument must be sequence")),
    };
    let mut out = Vec::new();
    if items.is_empty() {
        return Ok(out);
    }
    if matches!(items[0], Value::Int(_) | Value::Float(_) | Value::Bool(_)) {
        if items.len() % 2 != 0 {
            return Err(type_error("wrong number of coordinates"));
        }
        for p in items.chunks(2) {
            out.push((float_arg(&p[0]).map_err(|_| type_error("expected float"))?, float_arg(&p[1]).map_err(|_| type_error("expected float"))?));
        }
        return Ok(out);
    }
    for it in &items {
        let pair = match it {
            Value::Tuple(t) => t.to_vec(),
            Value::List(l) => l.borrow().clone(),
            _ => return Err(type_error("expected a sequence of coordinates")),
        };
        if pair.len() != 2 {
            return Err(type_error("expected a sequence of coordinates"));
        }
        out.push((float_arg(&pair[0]).map_err(|_| type_error("expected float"))?, float_arg(&pair[1]).map_err(|_| type_error("expected float"))?));
    }
    Ok(out)
}

/// `getink`: a cor de Python empacotada nos 4 bytes que o armazenamento usa.
fn getink(color: &Value, im: &Image) -> PyResult<[u8; 4]> {
    let mut color = color.clone();
    let tuple_size = match &color {
        Value::Tuple(t) => t.len() as i64,
        _ => -1,
    };
    if tuple_size == 1 {
        if let Value::Tuple(t) = &color {
            color = t[0].clone();
        }
    }
    let as_int = |v: &Value| -> Option<i64> {
        match v {
            Value::Int(_) | Value::Bool(_) | Value::Instance(_) => int_arg(v).ok(),
            _ => None,
        }
    };
    let clip = |v: i64| v.clamp(0, 255) as u8;
    let mut r: i64 = 0;
    let mut r_is_int = false;
    if matches!(im.kind, PixType::Uint8 | PixType::Int32 | PixType::Special) {
        if let Some(n) = as_int(&color) {
            r = n;
            r_is_int = true;
        } else if let Value::Big(_) = color {
            return Err(exc("OverflowError", "Python int too large to convert to C long"));
        } else if im.bands == 1 {
            return Err(type_error("color must be int or single-element tuple"));
        } else if tuple_size == -1 {
            return Err(type_error("color must be int or tuple"));
        }
    }
    let items = |v: &Value| -> Vec<Value> {
        match v {
            Value::Tuple(t) => t.to_vec(),
            _ => Vec::new(),
        }
    };
    let iarg = |v: &Value| -> PyResult<i64> { int_arg(v) };
    match im.kind {
        PixType::Uint8 => {
            if im.bands == 1 {
                if !r_is_int {
                    return Err(type_error("color must be int or single-element tuple"));
                }
                Ok([clip(r), 0, 0, 0])
            } else {
                let (mut g, mut b, mut a): (i64, i64, i64);
                if r_is_int {
                    a = (r >> 24) & 0xff;
                    b = (r >> 16) & 0xff;
                    g = (r >> 8) & 0xff;
                    r &= 0xff;
                } else {
                    a = 255;
                    let t = items(&color);
                    if im.bands == 2 {
                        if tuple_size != 1 && tuple_size != 2 {
                            return Err(type_error("color must be int, or tuple of one or two elements"));
                        }
                        r = iarg(&t[0])?;
                        if t.len() > 1 {
                            a = iarg(&t[1])?;
                        }
                        g = r;
                        b = r;
                    } else {
                        if tuple_size != 3 && tuple_size != 4 {
                            return Err(type_error("color must be int, or tuple of one, three or four elements"));
                        }
                        r = iarg(&t[0])?;
                        g = iarg(&t[1])?;
                        b = iarg(&t[2])?;
                        if t.len() > 3 {
                            a = iarg(&t[3])?;
                        }
                    }
                }
                Ok([clip(r), clip(g), clip(b), clip(a)])
            }
        }
        PixType::Int32 => Ok((r as i32).to_le_bytes()),
        PixType::Float32 => {
            let f = float_arg(&color)?;
            Ok((f as f32).to_le_bytes())
        }
        PixType::Special => Ok([r as u8, (r >> 8) as u8, 0, 0]),
    }
}

/// `getpixel`: o pixel como valor de Python.
fn pixel_value(im: &Image, x: i32, y: i32) -> PyResult<Value> {
    let x = if x < 0 { im.xsize + x } else { x };
    let y = if y < 0 { im.ysize + y } else { y };
    if x < 0 || x >= im.xsize || y < 0 || y >= im.ysize {
        return Err(exc("IndexError", "image index out of range"));
    }
    let p = im.get_pixel(x, y);
    let b = |i: usize| Value::Int(i64::from(p[i]));
    Ok(match im.kind {
        PixType::Uint8 => match im.bands {
            1 => b(0),
            2 => Value::tuple(vec![b(0), b(1)]),
            3 => Value::tuple(vec![b(0), b(1), b(2)]),
            _ => Value::tuple(vec![b(0), b(1), b(2), b(3)]),
        },
        PixType::Int32 => Value::Int(i64::from(i32::from_le_bytes(p))),
        PixType::Float32 => Value::Float(f64::from(f32::from_le_bytes(p))),
        PixType::Special => Value::Int(i64::from(u16::from_le_bytes([p[0], p[1]]))),
    })
}

// ---- ImagingCore ----

pub struct Core {
    pub im: Shared,
}

fn new_core(im: Image) -> Value {
    Value::Ext(Rc::new(Core { im: Rc::new(RefCell::new(im)) }))
}

/// A imagem por trás de um `ImagingCore` (o `PyImaging_AsImaging`).
fn core_of(v: &Value) -> PyResult<Shared> {
    if let Value::Ext(e) = v {
        if let Some(c) = e.as_any().and_then(|a| a.downcast_ref::<Core>()) {
            return Ok(c.im.clone());
        }
    }
    Err(type_error(format!("argument must be ImagingCore, not {}", v.type_name())))
}

fn none() -> PyResult<Value> {
    Ok(Value::None)
}

impl Core {
    fn resize(&self, s: &[Option<Value>]) -> PyResult<Value> {
        let im = self.im.borrow();
        let (xsize, ysize) = size_arg(s[0].as_ref().unwrap_or(&Value::None))?;
        let filter = s[1].as_ref().map(i32_arg).transpose()?.unwrap_or(resample::NEAREST);
        let mut b = [0f32, 0.0, im.xsize as f32, im.ysize as f32];
        if let Some(v) = &s[2] {
            let t = tuple_n(v, 4)?;
            for (k, x) in t.iter().enumerate() {
                b[k] = float_arg(x)? as f32;
            }
        }
        if xsize < 1 || ysize < 1 {
            return Err(value_error("height and width must be > 0"));
        }
        if b[0] < 0.0 || b[1] < 0.0 {
            return Err(value_error("box offset can't be negative"));
        }
        if b[2] > im.xsize as f32 || b[3] > im.ysize as f32 {
            return Err(value_error("box can't exceed original image size"));
        }
        if b[2] - b[0] < 0.0 || b[3] - b[1] < 0.0 {
            return Err(value_error("box can't be empty"));
        }
        let out = if b[0] - (b[0] as i32) as f32 == 0.0
            && b[2] - b[0] == xsize as f32
            && b[1] - (b[1] as i32) as f32 == 0.0
            && b[3] - b[1] == ysize as f32
        {
            ops::crop(&im, b[0] as i32, b[1] as i32, b[2] as i32, b[3] as i32)
        } else if filter == resample::NEAREST {
            ops::resize_nearest(&im, xsize, ysize, b)
        } else {
            verr(resample::resample(&im, xsize, ysize, filter, b))?
        };
        Ok(new_core(out))
    }

    /// `_putdata(data, scale=1.0, offset=0.0)`.
    fn putdata(&self, data: &Value, scale: f64, offset: f64) -> PyResult<Value> {
        let items: Vec<Value> = match data {
            Value::Bytes(_) => Vec::new(),
            Value::List(l) => l.borrow().clone(),
            Value::Tuple(t) => t.to_vec(),
            Value::Ext(_) | Value::Instance(_) | Value::Range(_) | Value::ByteArray(_) => crate::vm::iterate(data)?,
            _ => return Err(type_error("argument must be a sequence")),
        };
        let mut im = self.im.borrow_mut();
        let n = if let Value::Bytes(b) = data { b.len() } else { items.len() };
        if n > (im.xsize as usize) * (im.ysize as usize) {
            return Err(type_error("too many data entries"));
        }
        let xsize = im.xsize as usize;
        let flat = |op: &Value| -> PyResult<f64> {
            match op {
                Value::Str(_) | Value::Tuple(_) | Value::List(_) | Value::Bytes(_) | Value::ByteArray(_) => {
                    Err(type_error("sequence must be flattened"))
                }
                // O original ignora o erro do `PyFloat_AsDouble` e segue com -1.
                other => Ok(float_arg(other).unwrap_or(-1.0)),
            }
        };
        let clip8 = |v: f64| -> u8 { if v <= 0.0 { 0 } else if v < 256.0 { v as u8 } else { 255 } };
        if im.is8() {
            if let Value::Bytes(p) = data {
                for (i, &v) in p.iter().enumerate() {
                    let (x, y) = (i % xsize, i / xsize);
                    let o = y * im.linesize as usize + x;
                    im.data[o] = if scale == 1.0 && offset == 0.0 {
                        v
                    } else {
                        (((f64::from(v) * scale + offset) as i32).clamp(0, 255)) as u8
                    };
                }
            } else if im.bands == 1 {
                let big = im.mode == "I;16B";
                let special = im.kind == PixType::Special;
                for (i, op) in items.iter().enumerate() {
                    let mut value = flat(op)?;
                    if scale != 1.0 || offset != 0.0 {
                        value = value * scale + offset;
                    }
                    let (x, y) = (i % xsize, i / xsize);
                    let row = y * im.linesize as usize;
                    if special {
                        let v = value as i32;
                        let (lo, hi) = if big { (1, 0) } else { (0, 1) };
                        im.data[row + x * 2 + lo] = (v % 256).clamp(0, 255) as u8;
                        im.data[row + x * 2 + hi] = (v >> 8).clamp(0, 255) as u8;
                    } else {
                        im.data[row + x] = clip8(value);
                    }
                }
            } else {
                let p = im.pixelsize as usize;
                for (i, op) in items.iter().enumerate() {
                    let ink = getink(op, &im)?;
                    let (x, y) = (i % xsize, i / xsize);
                    let o = y * im.linesize as usize + x * p;
                    im.data[o..o + p].copy_from_slice(&ink[..p]);
                }
            }
        } else {
            for (i, op) in items.iter().enumerate() {
                let (x, y) = ((i % xsize) as i32, (i / xsize) as i32);
                let bytes = match im.kind {
                    PixType::Int32 => ((flat(op)? * scale + offset) as i32).to_le_bytes(),
                    PixType::Float32 => ((flat(op)? * scale + offset) as f32).to_le_bytes(),
                    _ => getink(op, &im)?,
                };
                let o = im.offset(x, y);
                im.data[o..o + 4].copy_from_slice(&bytes);
            }
        }
        none()
    }

    fn histogram(&self, s: &[Option<Value>]) -> PyResult<Value> {
        let im = self.im.borrow();
        let minmax = match (&s[0], im.kind) {
            (Some(Value::Tuple(t)), PixType::Int32) if t.len() == 2 => Some(ops::MinMax::I(i32_arg(&t[0])?, i32_arg(&t[1])?)),
            (Some(Value::Tuple(t)), PixType::Float32) if t.len() == 2 => {
                Some(ops::MinMax::F(float_arg(&t[0])? as f32, float_arg(&t[1])? as f32))
            }
            _ => None,
        };
        let mask = s[1].as_ref().map(core_of).transpose()?;
        let mb = mask.as_ref().map(|m| m.borrow());
        let h = ops::histogram(&im, mb.as_deref(), minmax).map_err(value_error)?;
        Ok(Value::list(h.into_iter().map(Value::Int).collect()))
    }
}

impl ExtObject for Core {
    fn type_name(&self) -> &'static str {
        "ImagingCore"
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn methods(&self) -> &'static [&'static str] {
        &[
            "getpixel",
            "putpixel",
            "pixel_access",
            "convert",
            "copy",
            "crop",
            "paste",
            "resize",
            "reduce",
            "transpose",
            "getbbox",
            "getextrema",
            "histogram",
            "getband",
            "putband",
            "fillband",
            "split",
            "getpalette",
            "getpalettemode",
            "putpalette",
            "putpalettealpha",
            "putpalettealphas",
            "isblock",
            "putdata",
            "point",
            "point_transform",
        ]
    }

    /// `len(im.getdata())`: o número de pixels.
    fn len(&self) -> Option<usize> {
        let im = self.im.borrow();
        Some((im.xsize.max(0) as usize) * (im.ysize.max(0) as usize))
    }

    /// `im.getdata()[i]`: o pixel `i` em ordem de linhas.
    fn getitem(&self, key: &Value) -> Option<PyResult<Value>> {
        let im = self.im.borrow();
        let n = i64::from(im.xsize) * i64::from(im.ysize);
        Some(int_arg(key).and_then(|i| {
            let i = if i < 0 { i + n } else { i };
            if i < 0 || i >= n {
                return Err(exc("IndexError", "image index out of range"));
            }
            pixel_value(&im, (i % i64::from(im.xsize)) as i32, (i / i64::from(im.xsize)) as i32)
        }))
    }

    fn to_items(&self) -> Option<Vec<Value>> {
        let im = self.im.borrow();
        let mut out = Vec::with_capacity((im.xsize.max(0) * im.ysize.max(0)) as usize);
        for y in 0..im.ysize {
            for x in 0..im.xsize {
                out.push(pixel_value(&im, x, y).ok()?);
            }
        }
        Some(out)
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        let im = self.im.borrow();
        Some(Ok(match name {
            "mode" => Value::str(im.mode.clone()),
            "size" => Value::tuple(vec![Value::Int(i64::from(im.xsize)), Value::Int(i64::from(im.ysize))]),
            "bands" => Value::Int(i64::from(im.bands)),
            "id" => Value::Int(Rc::as_ptr(&self.im) as usize as i64),
            "ptr" => math::image_capsule(self.im.clone()),
            _ => return None,
        }))
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        let arg = |i: usize| args.get(i).cloned();
        match name {
            "getpixel" => {
                if args.len() != 1 {
                    return Err(type_error("argument 1 must be sequence of length 2"));
                }
                let (x, y) = getxy(&args[0])?;
                pixel_value(&self.im.borrow(), x, y)
            }
            "putpixel" => {
                let s = bind(name, args, kw, &["xy", "color"], 2)?;
                let t = tuple_n(s[0].as_ref().unwrap_or(&Value::None), 2)?;
                let (mut x, mut y) = (i32_arg(&t[0])?, i32_arg(&t[1])?);
                let mut im = self.im.borrow_mut();
                if x < 0 {
                    x += im.xsize;
                }
                if y < 0 {
                    y += im.ysize;
                }
                if x < 0 || x >= im.xsize || y < 0 || y >= im.ysize {
                    return Err(exc("IndexError", "image index out of range"));
                }
                let ink = getink(s[1].as_ref().unwrap_or(&Value::None), &im)?;
                im.put_pixel(x, y, ink);
                none()
            }
            "pixel_access" => {
                let readonly = arg(0).as_ref().map(int_arg).transpose()?.unwrap_or(0) != 0;
                Ok(Value::Ext(Rc::new(PixelAccess { im: self.im.clone(), readonly })))
            }
            "convert" => {
                let s = bind(name, args, kw, &["mode", "dither", "paletteimage"], 1)?;
                let mode = str_arg(s[0].as_ref().unwrap_or(&Value::None))?;
                let dither = s[1].as_ref().map(int_arg).transpose()?.unwrap_or(0) != 0;
                if s[2].as_ref().is_some_and(|v| !matches!(v, Value::None)) {
                    return Err(value_error("conversion with a palette image is not supported"));
                }
                Ok(new_core(verr(convert::convert(&self.im.borrow(), Some(&mode), dither))?))
            }
            "copy" => Ok(new_core(self.im.borrow().clone())),
            "crop" => {
                let (x0, y0, x1, y1) = box4_i(&arg(0).unwrap_or(Value::None))?;
                Ok(new_core(ops::crop(&self.im.borrow(), x0, y0, x1, y1)))
            }
            "paste" => {
                let s = bind(name, args, kw, &["source", "box", "mask"], 2)?;
                let (x0, y0, x1, y1) = box4_i(s[1].as_ref().unwrap_or(&Value::None))?;
                let mask = s[2].as_ref().map(core_of).transpose()?;
                let src = s[0].clone().unwrap_or(Value::None);
                let r = if let Ok(other) = core_of(&src) {
                    // Colar a imagem nela mesma: o original lê e escreve o mesmo buffer.
                    let copy = if Rc::ptr_eq(&other, &self.im) { Some(other.borrow().clone()) } else { None };
                    let mb = mask.as_ref().map(|m| m.borrow().clone());
                    let mut out = self.im.borrow_mut();
                    match copy {
                        Some(c) => paste::paste(&mut out, &c, mb.as_ref(), x0, y0, x1, y1),
                        None => paste::paste(&mut out, &other.borrow(), mb.as_ref(), x0, y0, x1, y1),
                    }
                } else {
                    let ink = getink(&src, &self.im.borrow())?;
                    let mb = mask.as_ref().map(|m| m.borrow().clone());
                    paste::fill2(&mut self.im.borrow_mut(), ink, mb.as_ref(), x0, y0, x1, y1)
                };
                r.map_err(value_error)?;
                none()
            }
            "resize" => {
                let s = bind(name, args, kw, &["size", "resample", "box"], 1)?;
                self.resize(&s)
            }
            "point" => {
                let list = arg(0).unwrap_or(Value::None);
                let mode = match arg(1) {
                    None | Some(Value::None) => None,
                    Some(m) => Some(str_arg(&m)?),
                };
                let items = getlist(&list)?;
                let im = self.im.borrow();
                let expect = |n: usize| if items.len() != n { Err(value_error("wrong number of lut entries")) } else { Ok(()) };
                let table = if mode.as_deref() == Some("F") {
                    expect(256)?;
                    let t = items.iter().map(|v| float_arg(v).map(|f| (f as f32).to_le_bytes())).collect::<PyResult<Vec<_>>>()?;
                    ops::PointTable::Word(t)
                } else if im.mode == "I" && mode.as_deref() == Some("L") {
                    expect(65536)?;
                    ops::PointTable::U8(items.iter().map(|v| int_arg(v).map(|i| image::clip8(i64::from(i as i32)))).collect::<PyResult<Vec<_>>>()?)
                } else {
                    let bands = match &mode {
                        Some(m) => image::mode_layout(m).map(|l| l.0 as usize).ok_or_else(|| value_error("unrecognized image mode"))?,
                        None => im.bands as usize,
                    };
                    expect(256 * bands)?;
                    let data = items.iter().map(|v| int_arg(v).map(|i| i as i32)).collect::<PyResult<Vec<i32>>>()?;
                    if mode.as_deref() == Some("I") {
                        ops::PointTable::Word(data.iter().map(|v| v.to_le_bytes()).collect())
                    } else if mode.is_some() && bands > 1 {
                        let mut lut = vec![0u8; 1024];
                        for i in 0..256 {
                            lut[i * 4] = image::clip8(i64::from(data[i]));
                            lut[i * 4 + 1] = image::clip8(i64::from(data[i + 256]));
                            lut[i * 4 + 2] = image::clip8(i64::from(data[i + 512]));
                            if data.len() > 768 {
                                lut[i * 4 + 3] = image::clip8(i64::from(data[i + 768]));
                            }
                        }
                        // Como no original, a tabela intercalada vale também quando o modo tem as
                        // mesmas bandas da imagem, caso em que `im_point_Nx8_Nx8` a lê em fatias de 256.
                        ops::PointTable::U8(lut)
                    } else {
                        ops::PointTable::U8(data.iter().map(|&v| image::clip8(i64::from(v))).collect())
                    }
                };
                let mode = mode.unwrap_or_else(|| im.mode.clone());
                Ok(new_core(verr(ops::point(&im, &mode, &table))?))
            }
            "point_transform" => {
                let scale = match arg(0) {
                    Some(v) => float_arg(&v)?,
                    None => 1.0,
                };
                let offset = match arg(1) {
                    Some(v) => float_arg(&v)?,
                    None => 0.0,
                };
                Ok(new_core(verr(ops::point_transform(&self.im.borrow(), scale, offset))?))
            }
            "reduce" => {
                let im = self.im.borrow();
                let (xscale, yscale) = size_arg(&arg(0).unwrap_or(Value::None))?;
                let mut b = [0, 0, im.xsize, im.ysize];
                if let Some(v) = arg(1) {
                    let (x0, y0, x1, y1) = box4_i(&v)?;
                    b = [x0, y0, x1, y1];
                }
                if xscale < 1 || yscale < 1 {
                    return Err(value_error("scale must be > 0"));
                }
                if b[0] < 0 || b[1] < 0 {
                    return Err(value_error("box offset can't be negative"));
                }
                if b[2] > im.xsize || b[3] > im.ysize {
                    return Err(value_error("box can't exceed original image size"));
                }
                if b[2] <= b[0] || b[3] <= b[1] {
                    return Err(value_error("box can't be empty"));
                }
                if xscale == 1 && yscale == 1 {
                    return Ok(new_core(ops::crop(&im, b[0], b[1], b[2], b[3])));
                }
                let out = reduce::reduce(&im, xscale, yscale, [b[0], b[1], b[2] - b[0], b[3] - b[1]]);
                Ok(new_core(verr(out)?))
            }
            "transpose" => {
                let op = i32_arg(&arg(0).unwrap_or(Value::None))?;
                Ok(new_core(verr(ops::transpose(&self.im.borrow(), op))?))
            }
            "getbbox" => {
                let alpha_only = arg(0).as_ref().map(int_arg).transpose()?.unwrap_or(1) != 0;
                Ok(match ops::getbbox(&self.im.borrow(), alpha_only) {
                    Some(b) => Value::tuple(b.iter().map(|&v| Value::Int(i64::from(v))).collect()),
                    None => Value::None,
                })
            }
            "getextrema" => Ok(match ops::getextrema(&self.im.borrow()).map_err(value_error)? {
                None => Value::None,
                Some(ops::Extrema::U8(a, b)) => Value::tuple(vec![Value::Int(i64::from(a)), Value::Int(i64::from(b))]),
                Some(ops::Extrema::I32(a, b)) => Value::tuple(vec![Value::Int(i64::from(a)), Value::Int(i64::from(b))]),
                Some(ops::Extrema::U16(a, b)) => Value::tuple(vec![Value::Int(i64::from(a)), Value::Int(i64::from(b))]),
                Some(ops::Extrema::F32(a, b)) => Value::tuple(vec![Value::Float(f64::from(a)), Value::Float(f64::from(b))]),
            }),
            "histogram" => {
                let s = bind(name, args, kw, &["extrema", "mask"], 0)?;
                self.histogram(&s)
            }
            "getband" => {
                let band = i32_arg(&arg(0).unwrap_or(Value::None))?;
                Ok(new_core(verr(ops::getband(&self.im.borrow(), band))?))
            }
            "putband" => {
                let other = core_of(&arg(0).unwrap_or(Value::None))?;
                let band = i32_arg(&arg(1).unwrap_or(Value::None))?;
                let src = other.borrow().clone();
                ops::putband(&mut self.im.borrow_mut(), &src, band).map_err(value_error)?;
                none()
            }
            "fillband" => {
                let band = i32_arg(&arg(0).unwrap_or(Value::None))?;
                let color = i32_arg(&arg(1).unwrap_or(Value::None))?;
                ops::fillband(&mut self.im.borrow_mut(), band, color).map_err(value_error)?;
                none()
            }
            "split" => {
                let im = self.im.borrow();
                let mut out = Vec::new();
                for b in 0..im.bands {
                    out.push(new_core(verr(ops::getband(&im, b))?));
                }
                Ok(Value::tuple(out))
            }
            "getpalette" => {
                let mode = arg(0).as_ref().map(str_arg).transpose()?.unwrap_or_else(|| "RGB".into());
                let rawmode = arg(1).as_ref().map(str_arg).transpose()?.unwrap_or_else(|| "RGB".into());
                let im = self.im.borrow();
                let Some(pal) = &im.palette else {
                    return Err(value_error("image has no palette"));
                };
                let Some(p) = pack::packer(&mode, &rawmode) else {
                    return Err(value_error("unrecognized raw mode"));
                };
                let mut out = vec![0u8; pal.size * p.bits as usize / 8];
                (p.f)(&mut out, &pal.colors, pal.size);
                Ok(Value::bytes(out))
            }
            "getpalettemode" => match &self.im.borrow().palette {
                Some(p) => Ok(Value::str(p.mode.clone())),
                None => Err(value_error("image has no palette")),
            },
            "putpalette" => {
                let pmode = str_arg(&arg(0).unwrap_or(Value::None))?;
                let rawmode = str_arg(&arg(1).unwrap_or(Value::None))?;
                let data = bytes_arg(&arg(2).unwrap_or(Value::None))?;
                let mut im = self.im.borrow_mut();
                if !matches!(im.mode.as_str(), "L" | "LA" | "P" | "PA") {
                    return Err(value_error("image has wrong mode"));
                }
                let Some(u) = pack::unpacker(&pmode, &rawmode) else {
                    return Err(value_error("unrecognized raw mode"));
                };
                let size = data.len() * 8 / u.bits as usize;
                if size > 256 {
                    return Err(value_error("invalid palette size"));
                }
                im.mode = if im.mode.len() == 2 { "PA".into() } else { "P".into() };
                let mut pal = Palette::new(&pmode);
                pal.size = size;
                (u.f)(&mut pal.colors, &data, size);
                im.palette = Some(pal);
                none()
            }
            "putpalettealpha" => {
                let index = i32_arg(&arg(0).unwrap_or(Value::None))?;
                let alpha = arg(1).as_ref().map(i32_arg).transpose()?.unwrap_or(0);
                let mut im = self.im.borrow_mut();
                let Some(pal) = im.palette.as_mut() else {
                    return Err(value_error("image has no palette"));
                };
                if !(0..256).contains(&index) {
                    return Err(value_error("palette index out of range"));
                }
                pal.mode = "RGBA".into();
                pal.colors[index as usize * 4 + 3] = alpha as u8;
                none()
            }
            "putpalettealphas" => {
                let values = bytes_arg(&arg(0).unwrap_or(Value::None))?;
                let mut im = self.im.borrow_mut();
                let Some(pal) = im.palette.as_mut() else {
                    return Err(value_error("image has no palette"));
                };
                if values.len() > 256 {
                    return Err(value_error("palette index out of range"));
                }
                pal.mode = "RGBA".into();
                for (i, v) in values.iter().enumerate() {
                    pal.colors[i * 4 + 3] = *v;
                }
                none()
            }
            "isblock" => Ok(Value::Bool(false)),
            "putdata" => {
                let scale = args.get(1).map(float_arg).transpose()?.unwrap_or(1.0);
                let offset = args.get(2).map(float_arg).transpose()?.unwrap_or(0.0);
                self.putdata(&arg(0).unwrap_or(Value::None), scale, offset)
            }
            _ => Err(exc("AttributeError", format!("'ImagingCore' object has no attribute '{name}'"))),
        }
    }
}

// ---- PixelAccess ----

pub struct PixelAccess {
    im: Shared,
    readonly: bool,
}

impl ExtObject for PixelAccess {
    fn type_name(&self) -> &'static str {
        "PixelAccess"
    }

    fn methods(&self) -> &'static [&'static str] {
        &["__setitem__", "putpixel", "getpixel"]
    }

    fn getitem(&self, key: &Value) -> Option<PyResult<Value>> {
        Some(getxy(key).and_then(|(x, y)| pixel_value(&self.im.borrow(), x, y)))
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let key = args.first().cloned().unwrap_or(Value::None);
        match name {
            "getpixel" => getxy(&key).and_then(|(x, y)| pixel_value(&self.im.borrow(), x, y)),
            "__setitem__" | "putpixel" => {
                if self.readonly {
                    return Err(value_error("Attempt to putpixel a read only image"));
                }
                let (mut x, mut y) = getxy(&key)?;
                let mut im = self.im.borrow_mut();
                if x < 0 {
                    x += im.xsize;
                }
                if y < 0 {
                    y += im.ysize;
                }
                if x < 0 || x >= im.xsize || y < 0 || y >= im.ysize {
                    return Err(exc("IndexError", "image index out of range"));
                }
                let color = args.get(1).cloned().unwrap_or(Value::None);
                let ink = getink(&color, &im)?;
                im.put_pixel(x, y, ink);
                none()
            }
            _ => Err(exc("AttributeError", format!("'PixelAccess' object has no attribute '{name}'"))),
        }
    }
}

// ---- ImagingDraw ----

pub struct DrawObj {
    im: Shared,
    blend: bool,
}

/// Os dois cantos de `draw_rectangle`/`draw_ellipse`/`draw_arc`..., já validados.
fn two_points(v: &Value) -> PyResult<(i32, i32, i32, i32)> {
    let p = flatten(v)?;
    if p.len() != 2 {
        return Err(type_error("coordinate list must contain exactly 2 coordinates"));
    }
    if p[1].0 < p[0].0 {
        return Err(value_error("x1 must be greater than or equal to x0"));
    }
    if p[1].1 < p[0].1 {
        return Err(value_error("y1 must be greater than or equal to y0"));
    }
    Ok((p[0].0 as i32, p[0].1 as i32, p[1].0 as i32, p[1].1 as i32))
}

impl ExtObject for DrawObj {
    fn type_name(&self) -> &'static str {
        "ImagingDraw"
    }

    fn methods(&self) -> &'static [&'static str] {
        &[
            "draw_ink",
            "draw_lines",
            "draw_points",
            "draw_polygon",
            "draw_rectangle",
            "draw_ellipse",
            "draw_arc",
            "draw_chord",
            "draw_pieslice",
            "draw_bitmap",
            "draw_outline",
        ]
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::None);
        let opt_i = |i: usize| -> PyResult<i32> { args.get(i).map(i32_arg).transpose().map(|v| v.unwrap_or(0)) };
        let op = self.blend;
        match name {
            "draw_ink" => {
                let ink = getink(&arg(0), &self.im.borrow())?;
                Ok(Value::Int(i64::from(i32::from_le_bytes(ink))))
            }
            "draw_lines" => {
                let pts = flatten(&arg(0))?;
                let ink = i32_arg(&arg(1))?;
                let width = opt_i(2)?;
                let mut im = self.im.borrow_mut();
                if width <= 1 {
                    for w in pts.windows(2) {
                        draw::line(&mut im, w[0].0 as i32, w[0].1 as i32, w[1].0 as i32, w[1].1 as i32, ink, op);
                    }
                    if pts.len() >= 2 {
                        let p = pts[pts.len() - 1];
                        draw::point(&mut im, p.0 as i32, p.1 as i32, ink, op);
                    }
                } else {
                    for w in pts.windows(2) {
                        draw::wide_line(&mut im, w[0].0 as i32, w[0].1 as i32, w[1].0 as i32, w[1].1 as i32, ink, width, op);
                    }
                }
                none()
            }
            "draw_points" => {
                let pts = flatten(&arg(0))?;
                let ink = i32_arg(&arg(1))?;
                let mut im = self.im.borrow_mut();
                for p in pts {
                    draw::point(&mut im, p.0 as i32, p.1 as i32, ink, op);
                }
                none()
            }
            "draw_polygon" => {
                let pts = flatten(&arg(0))?;
                let ink = i32_arg(&arg(1))?;
                if pts.len() < 2 {
                    return Err(type_error("coordinate list must contain at least 2 coordinates"));
                }
                let xy: Vec<i32> = pts.iter().flat_map(|p| [p.0 as i32, p.1 as i32]).collect();
                draw::polygon(&mut self.im.borrow_mut(), &xy, ink, opt_i(2)? != 0, opt_i(3)?, op);
                none()
            }
            "draw_rectangle" => {
                let (x0, y0, x1, y1) = two_points(&arg(0))?;
                let ink = i32_arg(&arg(1))?;
                draw::rectangle(&mut self.im.borrow_mut(), x0, y0, x1, y1, ink, opt_i(2)? != 0, opt_i(3)?, op);
                none()
            }
            "draw_ellipse" => {
                let (x0, y0, x1, y1) = two_points(&arg(0))?;
                let ink = i32_arg(&arg(1))?;
                draw::ellipse(&mut self.im.borrow_mut(), x0, y0, x1, y1, ink, opt_i(2)? != 0, opt_i(3)?, op);
                none()
            }
            "draw_arc" => {
                let (x0, y0, x1, y1) = two_points(&arg(0))?;
                let (start, end) = (float_arg(&arg(1))? as f32, float_arg(&arg(2))? as f32);
                let ink = i32_arg(&arg(3))?;
                draw::arc(&mut self.im.borrow_mut(), x0, y0, x1, y1, start, end, ink, opt_i(4)?, op);
                none()
            }
            "draw_chord" | "draw_pieslice" => {
                let (x0, y0, x1, y1) = two_points(&arg(0))?;
                let (start, end) = (float_arg(&arg(1))? as f32, float_arg(&arg(2))? as f32);
                let ink = i32_arg(&arg(3))?;
                let fill = i32_arg(&arg(4))? != 0;
                let width = opt_i(5)?;
                let mut im = self.im.borrow_mut();
                if name == "draw_chord" {
                    draw::chord(&mut im, x0, y0, x1, y1, start, end, ink, fill, width, op);
                } else {
                    draw::pieslice(&mut im, x0, y0, x1, y1, start, end, ink, fill, width, op);
                }
                none()
            }
            "draw_bitmap" => {
                let pts = flatten(&arg(0))?;
                if pts.len() != 1 {
                    return Err(type_error("coordinate list must contain exactly 1 coordinate"));
                }
                let bitmap = core_of(&arg(1))?.borrow().clone();
                let ink = i32_arg(&arg(2))?.to_le_bytes();
                let (x0, y0) = (pts[0].0 as i32, pts[0].1 as i32);
                paste::fill2(&mut self.im.borrow_mut(), ink, Some(&bitmap), x0, y0, x0 + bitmap.xsize, y0 + bitmap.ysize)
                    .map_err(value_error)?;
                none()
            }
            "draw_outline" => Err(type_error("expected outline object")),
            _ => Err(exc("AttributeError", format!("'ImagingDraw' object has no attribute '{name}'"))),
        }
    }
}

// ---- ImagingFont (fonte bitmap do PIL) ----

#[derive(Clone, Copy, Default)]
struct Glyph {
    dx: i32,
    dy: i32,
    dx0: i32,
    dy0: i32,
    dx1: i32,
    dy1: i32,
    sx0: i32,
    sy0: i32,
    sx1: i32,
    sy1: i32,
}

pub struct FontObj {
    bitmap: Image,
    glyphs: Vec<Glyph>,
    baseline: i32,
    ysize: i32,
}

impl FontObj {
    /// `_font_text_asBytes`: `str` em latin-1, ou `bytes`.
    fn text(v: &Value) -> PyResult<Vec<u8>> {
        match v {
            Value::Str(s) => s
                .as_str()
                .chars()
                .map(|c| {
                    u8::try_from(u32::from(c)).map_err(|_| {
                        exc("UnicodeEncodeError", format!("'latin-1' codec can't encode character '\\u{:04x}'", u32::from(c)))
                    })
                })
                .collect(),
            Value::Bytes(b) => Ok(b.to_vec()),
            _ => Ok(Vec::new()),
        }
    }

    fn width(&self, text: &[u8]) -> i32 {
        text.iter().map(|&c| self.glyphs[usize::from(c)].dx).sum()
    }
}

impl ExtObject for FontObj {
    fn type_name(&self) -> &'static str {
        "ImagingFont"
    }

    fn methods(&self) -> &'static [&'static str] {
        &["getmask", "getsize"]
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let text = Self::text(args.first().unwrap_or(&Value::None))?;
        match name {
            "getsize" => Ok(Value::tuple(vec![Value::Int(i64::from(self.width(&text))), Value::Int(i64::from(self.ysize))])),
            "getmask" => {
                let Some(mut im) = Image::new(&self.bitmap.mode, self.width(&text), self.ysize) else {
                    return Err(exc("MemoryError", ""));
                };
                let (mut x, mut b) = (0, self.baseline);
                for &c in &text {
                    let g = self.glyphs[usize::from(c)];
                    let bm = ops::crop(&self.bitmap, g.sx0, g.sy0, g.sx1, g.sy1);
                    if paste::paste(&mut im, &bm, None, g.dx0 + x, g.dy0 + b, g.dx1 + x, g.dy1 + b).is_err() {
                        return none();
                    }
                    x += g.dx;
                    b += g.dy;
                }
                Ok(new_core(im))
            }
            _ => Err(exc("AttributeError", format!("'ImagingFont' object has no attribute '{name}'"))),
        }
    }
}

fn font_new(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let bitmap = core_of(args.first().unwrap_or(&Value::None))?.borrow().clone();
    let data = bytes_arg(args.get(1).unwrap_or(&Value::None))?;
    if data.len() != 256 * 20 {
        return Err(value_error("descriptor table has wrong size"));
    }
    let s16 = |o: usize| i32::from(i16::from_be_bytes([data[o], data[o + 1]]));
    let (mut y0, mut y1) = (0, 0);
    let mut glyphs = Vec::with_capacity(256);
    for i in 0..256 {
        let o = i * 20;
        let mut g = Glyph {
            dx: s16(o),
            dy: s16(o + 2),
            dx0: s16(o + 4),
            dy0: s16(o + 6),
            dx1: s16(o + 8),
            dy1: s16(o + 10),
            sx0: s16(o + 12),
            sy0: s16(o + 14),
            sx1: s16(o + 16),
            sy1: s16(o + 18),
        };
        if g.sx0 < 0 {
            g.dx0 -= g.sx0;
            g.sx0 = 0;
        }
        if g.sy0 < 0 {
            g.dy0 -= g.sy0;
            g.sy0 = 0;
        }
        if g.sx1 > bitmap.xsize {
            g.dx1 -= g.sx1 - bitmap.xsize;
            g.sx1 = bitmap.xsize;
        }
        if g.sy1 > bitmap.ysize {
            g.dy1 -= g.sy1 - bitmap.ysize;
            g.sy1 = bitmap.ysize;
        }
        y0 = y0.min(g.dy0);
        y1 = y1.max(g.dy1);
        glyphs.push(g);
    }
    Ok(Value::Ext(Rc::new(FontObj { bitmap, glyphs, baseline: -y0, ysize: y1 - y0 })))
}

// ---- codecs ----

enum DecKind {
    Raw(RawDecoder),
    Zip(ZipDecoder),
    Jpeg(Box<JpegDecoder>),
}

struct DecState {
    st: CodecState,
    kind: DecKind,
    im: Option<Shared>,
}

pub struct DecoderObj {
    inner: RefCell<DecState>,
}

impl ExtObject for DecoderObj {
    fn type_name(&self) -> &'static str {
        "ImagingDecoder"
    }

    fn methods(&self) -> &'static [&'static str] {
        &["decode", "setimage", "cleanup", "setfd"]
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "pulls_fd" => Some(Ok(Value::Bool(false))),
            _ => None,
        }
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        let mut d = self.inner.borrow_mut();
        let d = &mut *d;
        match name {
            "setimage" => {
                let im = core_of(args.first().unwrap_or(&Value::None))?;
                let ext = match args.get(1) {
                    Some(v) => box4_i(v)?,
                    None => (0, 0, 0, 0),
                };
                d.st.setimage(&im.borrow(), ext, false).map_err(value_error)?;
                d.im = Some(im);
                none()
            }
            "decode" => {
                let data = bytes_arg(args.first().unwrap_or(&Value::None))?;
                let Some(im) = d.im.clone() else {
                    return Err(value_error("decoder has no image"));
                };
                let mut im = im.borrow_mut();
                let status = match &mut d.kind {
                    DecKind::Raw(r) => r.decode(&mut im, &mut d.st, &data),
                    DecKind::Zip(z) => z.decode(&mut im, &mut d.st, &data),
                    DecKind::Jpeg(j) => j.decode(&mut im, &mut d.st, &data),
                };
                Ok(Value::tuple(vec![Value::Int(i64::from(status)), Value::Int(i64::from(d.st.errcode))]))
            }
            "cleanup" => none(),
            "setfd" => none(),
            _ => Err(exc("AttributeError", format!("'ImagingDecoder' object has no attribute '{name}'"))),
        }
    }
}

enum EncKind {
    Raw,
    Zip(Box<ZipEncoder>),
    Jpeg(Box<JpegEncoder>),
}

struct EncState {
    st: CodecState,
    kind: EncKind,
    im: Option<Shared>,
}

pub struct EncoderObj {
    inner: RefCell<EncState>,
}

impl EncState {
    fn step(&mut self, buf: &mut [u8]) -> PyResult<i32> {
        let Some(im) = self.im.clone() else {
            return Err(value_error("encoder has no image"));
        };
        let im = im.borrow();
        Ok(match &mut self.kind {
            EncKind::Raw => codec::raw_encode(&im, &mut self.st, buf),
            EncKind::Zip(z) => z.encode(&im, &mut self.st, buf),
            EncKind::Jpeg(j) => j.encode(&im, &mut self.st, buf),
        })
    }
}

impl ExtObject for EncoderObj {
    fn type_name(&self) -> &'static str {
        "ImagingEncoder"
    }

    fn methods(&self) -> &'static [&'static str] {
        &["encode", "encode_to_file", "setimage", "cleanup", "setfd"]
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "pushes_fd" => Some(Ok(Value::Bool(false))),
            _ => None,
        }
    }

    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "setimage" => {
                let mut e = self.inner.borrow_mut();
                let im = core_of(args.first().unwrap_or(&Value::None))?;
                let ext = match args.get(1) {
                    Some(v) => box4_i(v)?,
                    None => (0, 0, 0, 0),
                };
                e.st.setimage(&im.borrow(), ext, true).map_err(|m| exc("SystemError", m))?;
                e.im = Some(im);
                none()
            }
            "encode" => {
                let bufsize = args.first().map(int_arg).transpose()?.unwrap_or(16384).max(0) as usize;
                let mut buf = vec![0u8; bufsize];
                let mut e = self.inner.borrow_mut();
                let status = e.step(&mut buf)?;
                buf.truncate(status.max(0) as usize);
                Ok(Value::tuple(vec![Value::Int(i64::from(status)), Value::Int(i64::from(e.st.errcode)), Value::bytes(buf)]))
            }
            "encode_to_file" => {
                let fh = args.first().cloned().unwrap_or(Value::None);
                int_arg(&fh)?;
                let bufsize = args.get(1).map(int_arg).transpose()?.unwrap_or(16384).max(0) as usize;
                let os = crate::modules::import_checked(vm, "os")?;
                let write = vm.getattr(&Value::Module(os), "write")?;
                let mut buf = vec![0u8; bufsize];
                loop {
                    let (status, errcode) = {
                        let mut e = self.inner.borrow_mut();
                        let s = e.step(&mut buf)?;
                        (s, e.st.errcode)
                    };
                    if status > 0 {
                        let mut chunk = &buf[..status as usize];
                        while !chunk.is_empty() {
                            let n = vm.call_value(&write, vec![fh.clone(), Value::bytes(chunk.to_vec())], Vec::new())?;
                            let n = int_arg(&n)?.max(0) as usize;
                            chunk = &chunk[n.min(chunk.len())..];
                        }
                    }
                    if errcode != 0 {
                        return Ok(Value::Int(i64::from(errcode)));
                    }
                }
            }
            "cleanup" | "setfd" => none(),
            _ => Err(exc("AttributeError", format!("'ImagingEncoder' object has no attribute '{name}'"))),
        }
    }
}

fn unpacker_for(mode: &str, rawmode: &str) -> PyResult<pack::Codec> {
    pack::unpacker(mode, rawmode).ok_or_else(|| value_error("unknown raw mode for given image mode"))
}

fn packer_for(mode: &str, rawmode: &str) -> PyResult<pack::Codec> {
    pack::packer(mode, rawmode).ok_or_else(|| value_error("No packer found from {mode} to {rawmode}".replace("{mode}", mode).replace("{rawmode}", rawmode)))
}

fn mode_and_raw(args: &[Value]) -> PyResult<(String, String)> {
    let mode = str_arg(args.first().unwrap_or(&Value::None))?;
    let rawmode = str_arg(args.get(1).unwrap_or(&Value::None))?;
    Ok((mode, rawmode))
}

fn raw_decoder(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let (mode, rawmode) = mode_and_raw(&args)?;
    let stride = args.get(2).map(i32_arg).transpose()?.unwrap_or(0);
    let ystep = args.get(3).map(i32_arg).transpose()?.unwrap_or(1);
    let u = unpacker_for(&mode, &rawmode)?;
    let mut st = CodecState::new(u.f, u.bits as i32);
    st.ystep = ystep;
    Ok(Value::Ext(Rc::new(DecoderObj { inner: RefCell::new(DecState { st, kind: DecKind::Raw(RawDecoder::new(stride)), im: None }) })))
}

/// `PyImaging_JpegDecoderNew(mode, rawmode, jpegmode, scale=1, draft=0)`.
fn jpeg_decoder(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let (mode, mut rawmode) = mode_and_raw(&args)?;
    let jpegmode = match args.get(2) {
        None | Some(Value::None) => String::new(),
        Some(v) => str_arg(v)?,
    };
    let scale = args.get(3).map(i32_arg).transpose()?.unwrap_or(1);
    let draft = args.get(4).map(i32_arg).transpose()?.unwrap_or(0) != 0;
    // Com as extensões do libjpeg-turbo o Pillow pede `RGBX`, o formato nativo de 4 bytes.
    if rawmode == "RGB" {
        rawmode = "RGBX".into();
    }
    let u = unpacker_for(&mode, &rawmode)?;
    let st = CodecState::new(u.f, u.bits as i32);
    let kind = DecKind::Jpeg(Box::new(JpegDecoder::new(&rawmode, &jpegmode, scale, draft)));
    Ok(Value::Ext(Rc::new(DecoderObj { inner: RefCell::new(DecState { st, kind, im: None }) })))
}

/// `PyImaging_JpegEncoderNew(mode, rawmode, quality, progressive, smooth, optimize, keep_rgb,
/// streamtype, xdpi, ydpi, subsampling, restart_marker_blocks, restart_marker_rows, qtables,
/// comment, extra, exif)`.
fn jpeg_encoder(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let (mode, mut rawmode) = mode_and_raw(&args)?;
    let int = |i: usize| -> PyResult<i64> {
        match args.get(i) {
            None | Some(Value::None) => Ok(0),
            Some(v) => int_arg(v),
        }
    };
    let opt_bytes = |i: usize| -> PyResult<Option<Vec<u8>>> {
        match args.get(i) {
            None | Some(Value::None) => Ok(None),
            Some(v) => bytes_arg(v).map(Some),
        }
    };
    let quality = int(2)?;
    let smooth = int(4)?;
    let streamtype = int(7)?;
    // Suavização de entrada e fluxos abreviados (só tabelas, só imagem) ficam fora do porte.
    if smooth != 0 || streamtype != 0 {
        return Err(exc("OSError", "encoder error -8 when writing image file"));
    }
    if rawmode == "RGB" {
        rawmode = "RGBX".into();
    }
    let p = packer_for(&mode, &rawmode)?;
    let mut o = zjpeg::EncodeOptions::new(0, 0, zjpeg::InputSpace::Rgb);
    o.quality = (quality != -1).then_some(quality as i32);
    o.progressive = int(3)? != 0;
    o.optimize = int(5)? != 0;
    o.keep_rgb = int(6)? != 0;
    let (xdpi, ydpi) = (int(8)?, int(9)?);
    if xdpi > 0 && ydpi > 0 {
        o.dpi = Some((xdpi as u16, ydpi as u16));
    }
    o.subsampling = int(10)? as i32;
    o.restart_interval = int(11)?.max(0) as usize;
    o.restart_in_rows = int(12)?.max(0) as usize;
    if let Some(q) = args.get(13).filter(|v| !matches!(v, Value::None)) {
        let mut tables = Vec::new();
        for t in getlist(q)? {
            let vals = getlist(&t)?;
            if vals.len() != 64 {
                return Err(value_error("Invalid quantization table"));
            }
            let mut a = [0u32; 64];
            for (k, v) in vals.iter().enumerate() {
                a[k] = int_arg(v)? as u32;
            }
            tables.push(a);
        }
        if !tables.is_empty() {
            o.qtables = Some(tables);
        }
    }
    // `z#`: aceita `str` (em UTF-8) além de `bytes`.
    o.comment = match args.get(14) {
        None | Some(Value::None) => None,
        Some(Value::Str(s)) => Some(s.as_str().as_bytes().to_vec()),
        Some(v) => Some(bytes_arg(v)?),
    }
    .filter(|c| !c.is_empty());
    o.extra = opt_bytes(15)?.unwrap_or_default();
    o.exif = opt_bytes(16)?.unwrap_or_default();
    let st = CodecState::new(p.f, p.bits as i32);
    let kind = EncKind::Jpeg(Box::new(JpegEncoder::new(o, &rawmode)));
    Ok(Value::Ext(Rc::new(EncoderObj { inner: RefCell::new(EncState { st, kind, im: None }) })))
}

fn zip_decoder(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let (mode, rawmode) = mode_and_raw(&args)?;
    let interlaced = args.get(2).map(i32_arg).transpose()?.unwrap_or(0) != 0;
    let u = unpacker_for(&mode, &rawmode)?;
    let st = CodecState::new(u.f, u.bits as i32);
    Ok(Value::Ext(Rc::new(DecoderObj { inner: RefCell::new(DecState { st, kind: DecKind::Zip(ZipDecoder::new(interlaced)), im: None }) })))
}

fn raw_encoder(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let (mode, rawmode) = mode_and_raw(&args)?;
    let stride = args.get(2).map(i32_arg).transpose()?.unwrap_or(0);
    let ystep = args.get(3).map(i32_arg).transpose()?.unwrap_or(1);
    let p = packer_for(&mode, &rawmode)?;
    let mut st = CodecState::new(p.f, p.bits as i32);
    st.ystep = ystep;
    st.count = stride;
    Ok(Value::Ext(Rc::new(EncoderObj { inner: RefCell::new(EncState { st, kind: EncKind::Raw, im: None }) })))
}

fn zip_encoder(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let (mode, rawmode) = mode_and_raw(&args)?;
    let optimize = args.get(2).map(int_arg).transpose()?.unwrap_or(0) != 0;
    let level = args.get(3).map(i32_arg).transpose()?.unwrap_or(-1);
    let ctype = args.get(4).map(i32_arg).transpose()?.unwrap_or(-1);
    let dict = match args.get(5) {
        None | Some(Value::None) => None,
        Some(v) => Some(bytes_arg(v)?),
    };
    let p = packer_for(&mode, &rawmode)?;
    let st = CodecState::new(p.f, p.bits as i32);
    let z = ZipEncoder::new(rawmode.starts_with('P'), optimize, level, ctype, dict);
    Ok(Value::Ext(Rc::new(EncoderObj { inner: RefCell::new(EncState { st, kind: EncKind::Zip(Box::new(z)), im: None }) })))
}

fn getcodecstatus(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let code = i32_arg(args.first().unwrap_or(&Value::None))?;
    Ok(match code {
        1 => Value::str("end of data"),
        -1 => Value::str("image buffer overrun error"),
        -2 => Value::str("decoding error"),
        -3 => Value::str("unknown error"),
        -8 => Value::str("bad configuration"),
        -9 => Value::str("out of memory error"),
        _ => Value::None,
    })
}

// ---- fábricas ----

fn new_image(mode: &str, xsize: i32, ysize: i32) -> PyResult<Image> {
    if xsize < 0 || ysize < 0 {
        return Err(value_error("bad image size"));
    }
    Image::new(mode, xsize, ysize).ok_or_else(|| value_error("unrecognized image mode"))
}

fn fill(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("fill", args, kw, &["mode", "size", "color"], 1)?;
    let mode = str_arg(s[0].as_ref().unwrap_or(&Value::None))?;
    let (xsize, ysize) = match &s[1] {
        Some(v) => size_arg(v)?,
        None => (256, 256),
    };
    let mut im = new_image(&mode, xsize, ysize)?;
    let ink = match &s[2] {
        Some(c) => getink(c, &im)?,
        None => [0; 4],
    };
    im.fill(ink);
    Ok(new_core(im))
}

fn new(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let mode = str_arg(args.first().unwrap_or(&Value::None))?;
    let (xsize, ysize) = size_arg(args.get(1).unwrap_or(&Value::None))?;
    Ok(new_core(new_image(&mode, xsize, ysize)?))
}

fn draw_new(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let im = core_of(args.first().unwrap_or(&Value::None))?;
    let blend = args.get(1).map(int_arg).transpose()?.unwrap_or(0) != 0;
    Ok(Value::Ext(Rc::new(DrawObj { im, blend })))
}

/// `ImagingMerge`: junta bandas `L` numa imagem do modo pedido.
fn merge(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let mode = str_arg(args.first().unwrap_or(&Value::None))?;
    let bands: Vec<Image> = args[1..].iter().filter(|v| !matches!(v, Value::None)).map(|v| core_of(v).map(|c| c.borrow().clone())).collect::<PyResult<_>>()?;
    let Some(first) = bands.first() else {
        return Err(value_error("wrong number of bands"));
    };
    let mut out = new_image(&mode, first.xsize, first.ysize)?;
    if out.bands as usize != bands.len() {
        return Err(value_error("wrong number of bands"));
    }
    for b in &bands {
        if b.bands != 1 || b.kind != PixType::Uint8 {
            return Err(value_error("image has wrong mode"));
        }
        if b.xsize != first.xsize || b.ysize != first.ysize {
            return Err(value_error("images do not match"));
        }
    }
    if out.bands == 1 {
        out.data.copy_from_slice(&bands[0].data);
        return Ok(new_core(out));
    }
    for (k, b) in bands.iter().enumerate() {
        ops::putband(&mut out, b, k as i32).map_err(value_error)?;
    }
    Ok(new_core(out))
}

// ---- arena de memória (`ImagingMemoryArena`): só a configuração e as estatísticas ----

thread_local! {
    static ARENA: RefCell<[i64; 3]> = const { RefCell::new([1, 16 * 1024 * 1024, 0]) };
}

fn one_int(fname: &str, args: &[Value]) -> PyResult<i64> {
    match args {
        [v] => int_arg(v),
        _ => Err(type_error(format!("{fname}() takes exactly one argument ({} given)", args.len()))),
    }
}

fn set_alignment(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let n = one_int("set_alignment", &args)?;
    if !(1..=128).contains(&n) {
        return Err(value_error("alignment should be from 1 to 128"));
    }
    if n & (n - 1) != 0 {
        return Err(value_error("alignment should be power of two"));
    }
    ARENA.with(|a| a.borrow_mut()[0] = n);
    none()
}

fn set_block_size(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let n = one_int("set_block_size", &args)?;
    if n <= 0 {
        return Err(value_error("block_size should be greater than 0"));
    }
    if n & 0xfff != 0 {
        return Err(value_error("block_size should be multiple of 4096"));
    }
    ARENA.with(|a| a.borrow_mut()[1] = n);
    none()
}

fn set_blocks_max(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let n = one_int("set_blocks_max", &args)?;
    if n < 0 {
        return Err(value_error("blocks_max should be greater than 0"));
    }
    if n > i64::from(i32::MAX) / 16 {
        return Err(value_error("blocks_max is too large"));
    }
    ARENA.with(|a| a.borrow_mut()[2] = n);
    none()
}

fn arena_get(k: usize) -> Value {
    Value::Int(ARENA.with(|a| a.borrow()[k]))
}

fn get_alignment(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(arena_get(0))
}

fn get_block_size(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(arena_get(1))
}

fn get_blocks_max(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    Ok(arena_get(2))
}

/// `get_stats()`: as imagens daqui não passam por blocos reaproveitáveis, então os contadores
/// ficam zerados.
fn get_stats(vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    let d = vm.call_value(&Value::Builtin("dict"), Vec::new(), Vec::new())?;
    for k in ["new_count", "allocated_blocks", "reused_blocks", "reallocated_blocks", "freed_blocks", "blocks_cached"] {
        crate::vm::store_subscript(&d, &Value::str(k), Value::Int(0))?;
    }
    Ok(d)
}

fn noop(_vm: &mut Vm, _args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    none()
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("PIL._imaging")
        .func("set_alignment", set_alignment)
        .func("set_block_size", set_block_size)
        .func("set_blocks_max", set_blocks_max)
        .func("get_alignment", get_alignment)
        .func("get_block_size", get_block_size)
        .func("get_blocks_max", get_blocks_max)
        .func("get_stats", get_stats)
        .func("reset_stats", noop)
        .func("clear_cache", noop)
        .func("new", new)
        .func("fill", fill)
        .func("draw", draw_new)
        .func("font", font_new)
        .func("merge", merge)
        .func("raw_decoder", raw_decoder)
        .func("raw_encoder", raw_encoder)
        .func("zip_decoder", zip_decoder)
        .func("jpeg_encoder", jpeg_encoder)
        .func("jpeg_decoder", jpeg_decoder)
        .func("zip_encoder", zip_encoder)
        .func("getcodecstatus", getcodecstatus)
        .func("path", path::path_create)
        .value("__file__", Value::str("/usr/lib/python3/dist-packages/PIL/_imaging.cpython-313-x86_64-linux-gnu.so"))
        .value("PILLOW_VERSION", Value::str(PILLOW_VERSION))
        .value("zlib_version", Value::str("1.3.1"))
        .value("HAVE_LIBJPEGTURBO", Value::Bool(false))
        .value("HAVE_LIBIMAGEQUANT", Value::Bool(false))
        .value("HAVE_ZLIBNG", Value::Bool(false))
        .value("HAVE_XCB", Value::Bool(false))
        .value("DEFAULT_STRATEGY", Value::Int(0))
        .value("FILTERED", Value::Int(1))
        .value("HUFFMAN_ONLY", Value::Int(2))
        .value("RLE", Value::Int(3))
        .value("FIXED", Value::Int(4))
        .build()
}
