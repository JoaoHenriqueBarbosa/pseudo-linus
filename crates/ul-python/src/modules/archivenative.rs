//! Módulo nativo `_archive`: compressão e descompressão de bz2, xz e lzma de uma vez, sobre os
//! codecs do `ul-archive`. Os módulos `bz2` e `lzma` (em Python) montam as classes por cima.

use std::rc::Rc;

use ul_archive::codec::{self, DecodeError, Format, GzipHeader};

use crate::modules::ModuleBuilder;
use crate::modules::binascii::want_bytes;
use crate::native_util::{bind, want_int};
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{exc, PyResult, Vm};

fn level_arg(v: Option<&Value>, default: u32) -> PyResult<u32> {
    match v {
        None | Some(Value::None) => Ok(default),
        Some(v) => Ok(want_int(v)?.clamp(0, 9) as u32),
    }
}

fn compress_with(fname: &'static str, fmt: Format, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind(fname, args, kw, &["data", "level"], 1)?;
    let data = want_bytes(a[0].as_ref().unwrap_or(&Value::None))?;
    let level = level_arg(a[1].as_ref(), fmt.default_level())?;
    let level = if fmt == Format::Bzip2 { level.max(1) } else { level };
    codec::compress(fmt, &data, level, &GzipHeader::default())
        .map(Value::bytes)
        .map_err(|e| exc("OSError", e.to_string()))
}

fn decompress_with(fname: &'static str, fmt: Format, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind(fname, args, kw, &["data"], 1)?;
    let data = want_bytes(a[0].as_ref().unwrap_or(&Value::None))?;
    match codec::decompress(fmt, &data) {
        Ok(d) => Ok(Value::bytes(d.data)),
        Err(DecodeError::Truncated) => Err(exc("EOFError", "Compressed file ended before the end-of-stream marker was reached")),
        Err(DecodeError::NotFormat) => Err(exc("OSError", "Invalid data stream")),
        Err(DecodeError::Checksum) => Err(exc("OSError", "Invalid data stream")),
        Err(DecodeError::Corrupt(m)) => Err(exc("OSError", format!("Invalid data stream: {m}"))),
        Err(other) => Err(exc("OSError", format!("{other:?}"))),
    }
}

fn bz2_compress(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    compress_with("bz2_compress", Format::Bzip2, args, kw)
}

fn bz2_decompress(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    decompress_with("bz2_decompress", Format::Bzip2, args, kw)
}

fn xz_compress(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    compress_with("xz_compress", Format::Xz, args, kw)
}

fn xz_decompress(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    decompress_with("xz_decompress", Format::Xz, args, kw)
}

fn lzma_compress(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    compress_with("lzma_compress", Format::Lzma, args, kw)
}

fn lzma_decompress(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    decompress_with("lzma_decompress", Format::Lzma, args, kw)
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_archive")
        .func("bz2_compress", bz2_compress)
        .func("bz2_decompress", bz2_decompress)
        .func("xz_compress", xz_compress)
        .func("xz_decompress", xz_decompress)
        .func("lzma_compress", lzma_compress)
        .func("lzma_decompress", lzma_decompress)
        .build()
}
