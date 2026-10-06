//! `ImagingCore.path` (`path.c` do Pillow 11.1.0): a lista de vértices que o `ImagePath.Path` e o
//! `ImageDraw` usam. Os pontos ficam num vetor de `double` alternando x e y, mutável pelo `map`,
//! `transform`, `compact` e pela atribuição por índice, como no original.
//!
//! O fatiamento com objeto `slice` herda o defeito do original: os índices são normalizados contra
//! um comprimento fixo de 4, não contra o número de vértices.
//!
//! Portado do Pillow (MIT-CMU: Copyright © 1996-1997 Secret Labs AB, © 1996 Fredrik Lundh).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use super::{float_arg, int_arg, value_error};
use crate::object::{ExtObject, Kw, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

pub struct PathObj {
    xy: RefCell<Vec<f64>>,
    mapping: Cell<bool>,
}

impl PathObj {
    fn count(&self) -> usize {
        self.xy.borrow().len() / 2
    }
}

fn new_path(xy: Vec<f64>) -> Value {
    Value::Ext(Rc::new(PathObj { xy: RefCell::new(xy), mapping: Cell::new(false) }))
}

/// O objeto `Path` por trás de um valor, se for um.
pub fn as_path(v: &Value) -> Option<Vec<f64>> {
    if let Value::Ext(e) = v {
        if let Some(p) = e.as_any().and_then(|a| a.downcast_ref::<PathObj>()) {
            return Some(p.xy.borrow().clone());
        }
    }
    None
}

/// Um par `(x, y)` no `PyArg_ParseTuple(op, "dd")`.
fn parse_dd(v: &Value) -> PyResult<(f64, f64)> {
    let items = match v {
        Value::Tuple(t) => t.to_vec(),
        _ => return Err(type_error(format!("argument must be tuple, not {}", v.type_name()))),
    };
    if items.len() != 2 {
        return Err(type_error(format!("function takes exactly 2 arguments ({} given)", items.len())));
    }
    Ok((float_arg(&items[0])?, float_arg(&items[1])?))
}

/// `PyPath_Flatten`: outro `Path`, ou uma sequência de números soltos e de pares.
pub fn flatten(data: &Value) -> PyResult<Vec<f64>> {
    if let Some(xy) = as_path(data) {
        return Ok(xy);
    }
    let items = match data {
        Value::List(l) => l.borrow().clone(),
        Value::Tuple(t) => t.to_vec(),
        Value::Str(_) | Value::Bytes(_) | Value::Ext(_) | Value::Instance(_) => {
            crate::vm::iterate(data)?
        }
        _ => return Err(type_error("argument must be sequence")),
    };
    let mut xy = Vec::with_capacity(items.len() * 2);
    for op in &items {
        match op {
            Value::Float(f) => xy.push(*f),
            // `(float)PyLong_AS_LONG(op)`: o inteiro passa por `float` de 32 bits.
            Value::Int(_) | Value::Bool(_) => xy.push(f64::from(int_arg(op)? as f32)),
            Value::Tuple(_) => match parse_dd(op) {
                Ok((x, y)) => {
                    xy.push(x);
                    xy.push(y);
                }
                Err(_) => return Err(value_error("incorrect coordinate type")),
            },
            _ => return Err(value_error("incorrect coordinate type")),
        }
    }
    if xy.len() & 1 != 0 {
        return Err(value_error("wrong number of coordinates"));
    }
    Ok(xy)
}

/// `PyPath_Create`: `Path(n)` com `n` vértices zerados, ou `Path(sequência)`.
pub fn path_create(_vm: &mut Vm, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
    if args.len() != 1 {
        return Err(type_error(format!("Path() takes exactly one argument ({} given)", args.len())));
    }
    if let Value::Int(n) = args[0] {
        if n < 0 {
            return Err(exc("MemoryError", ""));
        }
        return Ok(new_path(vec![0.0; n as usize * 2]));
    }
    Ok(new_path(flatten(&args[0])?))
}

fn pair(x: f64, y: f64) -> Value {
    Value::tuple(vec![Value::Float(x), Value::Float(y)])
}

impl PathObj {
    fn item(&self, i: i64) -> PyResult<Value> {
        let n = self.count() as i64;
        let i = if i < 0 { n + i } else { i };
        if i < 0 || i >= n {
            return Err(exc("IndexError", "path index out of range"));
        }
        let xy = self.xy.borrow();
        Ok(pair(xy[2 * i as usize], xy[2 * i as usize + 1]))
    }

    fn slice(&self, lo: i64, hi: i64) -> Value {
        let n = self.count() as i64;
        let lo = lo.clamp(0, n);
        let hi = hi.max(0).max(lo).min(n);
        new_path(self.xy.borrow()[2 * lo as usize..2 * hi as usize].to_vec())
    }
}

impl ExtObject for PathObj {
    fn type_name(&self) -> &'static str {
        "Path"
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn methods(&self) -> &'static [&'static str] {
        &["getbbox", "tolist", "compact", "map", "transform", "__setitem__"]
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "id" => Some(Ok(Value::Int(self.xy.as_ptr() as usize as i64))),
            _ => None,
        }
    }

    fn len(&self) -> Option<usize> {
        Some(self.count())
    }

    fn getitem(&self, key: &Value) -> Option<PyResult<Value>> {
        Some(match key {
            Value::Int(_) | Value::Bool(_) => int_arg(key).and_then(|i| self.item(i)),
            Value::Slice(s) => crate::vm::slice_bounds(4, s).and_then(|(start, stop, step)| {
                let len = if step > 0 && stop > start {
                    (stop - start + step - 1) / step
                } else if step < 0 && stop < start {
                    (start - stop - step - 1) / -step
                } else {
                    0
                };
                if len <= 0 {
                    Ok(new_path(Vec::new()))
                } else if step == 1 {
                    Ok(self.slice(start, stop))
                } else {
                    Err(type_error("slice steps not supported"))
                }
            }),
            _ => Err(type_error(format!("Path indices must be integers, not {}", key.type_name()))),
        })
    }

    fn to_items(&self) -> Option<Vec<Value>> {
        let xy = self.xy.borrow();
        Some(xy.chunks(2).map(|p| pair(p[0], p[1])).collect())
    }

    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, _kw: Kw) -> PyResult<Value> {
        match name {
            "getbbox" => {
                if !args.is_empty() {
                    return Err(type_error(format!("getbbox() takes no arguments ({} given)", args.len())));
                }
                let xy = self.xy.borrow();
                let (mut x0, mut y0, mut x1, mut y1) = (0.0, 0.0, 0.0, 0.0);
                if let [x, y, ..] = xy[..] {
                    (x0, y0, x1, y1) = (x, y, x, y);
                    for p in xy.chunks(2).skip(1) {
                        if p[0] < x0 {
                            x0 = p[0];
                        }
                        if p[0] > x1 {
                            x1 = p[0];
                        }
                        if p[1] < y0 {
                            y0 = p[1];
                        }
                        if p[1] > y1 {
                            y1 = p[1];
                        }
                    }
                }
                Ok(Value::tuple(vec![Value::Float(x0), Value::Float(y0), Value::Float(x1), Value::Float(y1)]))
            }
            "tolist" => {
                let flat = match args.first() {
                    Some(v) => int_arg(v)? != 0,
                    None => false,
                };
                let xy = self.xy.borrow();
                Ok(Value::list(if flat {
                    xy.iter().map(|&v| Value::Float(v)).collect()
                } else {
                    xy.chunks(2).map(|p| pair(p[0], p[1])).collect()
                }))
            }
            "compact" => {
                if self.mapping.get() {
                    return Err(value_error("Path compacted during mapping"));
                }
                let cityblock = match args.first() {
                    Some(v) => float_arg(v)?,
                    None => 2.0,
                };
                let mut xy = self.xy.borrow_mut();
                let n = xy.len() / 2;
                let mut j = 1;
                for i in 1..n {
                    if (xy[2 * j - 2] - xy[2 * i]).abs() + (xy[2 * j - 1] - xy[2 * i + 1]).abs() >= cityblock {
                        xy[2 * j] = xy[2 * i];
                        xy[2 * j + 1] = xy[2 * i + 1];
                        j += 1;
                    }
                }
                // Com zero vértices o original deixa `j = 1` e o contador fica em 1.
                let removed = n as i64 - j as i64;
                xy.resize(2 * j, 0.0);
                Ok(Value::Int(removed))
            }
            "map" => {
                let function = args.first().cloned().ok_or_else(|| type_error("map() takes exactly one argument (0 given)"))?;
                self.mapping.set(true);
                let n = self.count();
                for i in 0..n {
                    let (x, y) = {
                        let xy = self.xy.borrow();
                        (xy[2 * i], xy[2 * i + 1])
                    };
                    let r = vm.call_value(&function, vec![Value::Float(x), Value::Float(y)], Vec::new()).and_then(|item| parse_dd(&item));
                    match r {
                        Ok((x, y)) => {
                            let mut xy = self.xy.borrow_mut();
                            xy[2 * i] = x;
                            xy[2 * i + 1] = y;
                        }
                        Err(e) => {
                            self.mapping.set(false);
                            return Err(e);
                        }
                    }
                }
                self.mapping.set(false);
                Ok(Value::None)
            }
            "transform" => {
                let m = match args.first() {
                    Some(Value::Tuple(t)) if t.len() == 6 => t.iter().map(float_arg).collect::<PyResult<Vec<f64>>>()?,
                    Some(Value::Tuple(t)) => {
                        return Err(type_error(format!("transform() argument 1 must be sequence of length 6, not {}", t.len())))
                    }
                    Some(v) => return Err(type_error(format!("transform() argument 1 must be tuple, not {}", v.type_name()))),
                    None => return Err(type_error("transform() takes at least 1 argument (0 given)")),
                };
                let wrap = match args.get(1) {
                    Some(v) => float_arg(v)?,
                    None => 0.0,
                };
                let (a, b, c, d, e, f) = (m[0], m[1], m[2], m[3], m[4], m[5]);
                let mut xy = self.xy.borrow_mut();
                for p in xy.chunks_mut(2) {
                    if b == 0.0 && d == 0.0 {
                        p[0] = a * p[0] + c;
                        p[1] = e * p[1] + f;
                    } else {
                        let (x, y) = (p[0], p[1]);
                        p[0] = a * x + b * y + c;
                        p[1] = d * x + e * y + f;
                    }
                    if wrap != 0.0 {
                        p[0] %= wrap;
                    }
                }
                Ok(Value::None)
            }
            "__setitem__" => {
                let i = int_arg(args.first().unwrap_or(&Value::None))?;
                let n = self.count() as i64;
                if i < 0 || i >= n {
                    return Err(exc("IndexError", "path assignment index out of range"));
                }
                let (x, y) = parse_dd(args.get(1).unwrap_or(&Value::None))?;
                let mut xy = self.xy.borrow_mut();
                xy[2 * i as usize] = x;
                xy[2 * i as usize + 1] = y;
                Ok(Value::None)
            }
            _ => Err(exc("AttributeError", format!("'Path' object has no attribute '{name}'"))),
        }
    }
}
