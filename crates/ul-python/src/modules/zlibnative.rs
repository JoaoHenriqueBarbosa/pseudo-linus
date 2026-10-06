//! `_zlib`: o deflate/inflate que `zlib`, `gzip` e `zipfile` (em Python embutido) usam. Compressão e
//! descompressão vêm do `flate2` com o backend `zlib-rs` (Rust puro, mesmo backend do `ul-archive`).
//!
//! Funções soltas (`compress`, `decompress`) e dois objetos de fluxo (`compressobj`,
//! `decompressobj`). O argumento `wbits` segue o zlib: positivo de 9 a 15 é zlib, negativo é deflate
//! cru, 25 a 31 (16 mais o tamanho da janela) é gzip e 40 a 47 (32 mais) detecta zlib ou gzip pelo
//! cabeçalho. Os erros saem como `ValueError`, e o módulo `zlib` em Python os converte em `zlib.error`.

use std::cell::RefCell;
use std::rc::Rc;

use flate2::{Decompress, FlushDecompress, Status};
use zdeflate::{Deflate, Flush, Status as ZStatus, Strategy};

use crate::modules::ModuleBuilder;
use crate::native_util::{bind, no_kwargs, want_int};
use crate::object::{ExtObject, Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

fn want_data(v: &Value) -> PyResult<Vec<u8>> {
    match v {
        Value::Bytes(b) => Ok(b.to_vec()),
        other => Err(type_error(format!("a bytes-like object is required, not '{}'", other.type_name()))),
    }
}

fn level_of(n: i64) -> PyResult<i32> {
    match n {
        -1..=9 => Ok(n as i32),
        _ => Err(exc("ValueError", "Bad compression level")),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Wrap {
    Zlib(u8),
    Raw(u8),
    Gzip(u8),
    Auto(u8),
}

fn wrap_of(wbits: i64) -> PyResult<Wrap> {
    let bad = || exc("ValueError", "Invalid initialization option");
    Ok(match wbits {
        8..=15 => Wrap::Zlib(wbits.max(9) as u8),
        -15..=-8 => Wrap::Raw((-wbits).max(9) as u8),
        24..=31 => Wrap::Gzip((wbits - 16).max(9) as u8),
        40..=47 => Wrap::Auto((wbits - 32).max(9) as u8),
        _ => return Err(bad()),
    })
}

fn strategy_of(n: i64) -> Strategy {
    match n {
        1 => Strategy::Filtered,
        2 => Strategy::HuffmanOnly,
        3 => Strategy::Rle,
        4 => Strategy::Fixed,
        _ => Strategy::Default,
    }
}

fn new_compress(level: i32, wrap: Wrap, mem_level: i32, strategy: Strategy) -> PyResult<Deflate> {
    let window_bits = match wrap {
        Wrap::Zlib(w) => i32::from(w),
        Wrap::Raw(w) => -i32::from(w),
        Wrap::Gzip(w) => i32::from(w) + 16,
        Wrap::Auto(_) => return Err(exc("ValueError", "Invalid initialization option")),
    };
    Deflate::new(level, window_bits, mem_level, strategy).map_err(|_| exc("ValueError", "Invalid initialization option"))
}

fn new_decompress(wrap: Wrap, head: &[u8]) -> Decompress {
    match wrap {
        Wrap::Zlib(w) => Decompress::new_with_window_bits(true, w),
        Wrap::Raw(w) => Decompress::new_with_window_bits(false, w),
        Wrap::Gzip(w) => Decompress::new_gzip(w),
        Wrap::Auto(w) => {
            if head.starts_with(&[0x1f, 0x8b]) {
                Decompress::new_gzip(w)
            } else {
                Decompress::new_with_window_bits(true, w)
            }
        }
    }
}

fn run_compress(c: &mut Deflate, input: &[u8], flush: Flush) -> PyResult<Vec<u8>> {
    let mut out: Vec<u8> = Vec::new();
    let mut pos = 0;
    let mut buf = vec![0u8; 32768];
    loop {
        let p = c.deflate(&input[pos..], &mut buf, flush);
        pos += p.consumed;
        out.extend_from_slice(&buf[..p.produced]);
        match p.status {
            Ok(ZStatus::StreamEnd) => break,
            Ok(ZStatus::Ok) => {
                // Sobrou espaço na saída e a entrada acabou: nada mais a escoar.
                if pos >= input.len() && p.produced < buf.len() {
                    break;
                }
            }
            Err(zdeflate::Error::Buf) => break,
            Err(_) => return Err(exc("ValueError", "Error -2 while compressing data: inconsistent stream state")),
        }
    }
    Ok(out)
}

fn compress(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("compress", args, kw, &["data", "level", "wbits"], 1)?;
    let data = want_data(s[0].as_ref().unwrap())?;
    let level = level_of(s[1].as_ref().map(want_int).transpose()?.unwrap_or(-1))?;
    let wrap = wrap_of(s[2].as_ref().map(want_int).transpose()?.unwrap_or(15))?;
    let mut c = new_compress(level, wrap, 8, Strategy::Default)?;
    Ok(Value::bytes(run_compress(&mut c, &data, Flush::Finish)?))
}

fn decompress_error(e: &flate2::DecompressError) -> crate::vm::PyException {
    let msg = e.message().unwrap_or("invalid data");
    exc("ValueError", format!("Error -3 while decompressing data: {msg}"))
}

/// Alimenta `input` ao decompressor; devolve (saída, bytes consumidos, chegou ao fim).
fn run_decompress(d: &mut Decompress, input: &[u8], max_out: usize) -> PyResult<(Vec<u8>, usize, bool)> {
    let mut out: Vec<u8> = Vec::new();
    let start = d.total_in();
    let mut ended = false;
    loop {
        let consumed = (d.total_in() - start) as usize;
        if max_out > 0 && out.len() >= max_out {
            break;
        }
        let want = if max_out > 0 { max_out - out.len() } else { input.len().max(4096) * 4 };
        out.reserve(want);
        let room = if max_out > 0 { want } else { out.capacity() - out.len() };
        let _ = room;
        let before_out = out.len();
        let status = d
            .decompress_vec(&input[consumed..], &mut out, FlushDecompress::None)
            .map_err(|e| decompress_error(&e))?;
        let now_consumed = (d.total_in() - start) as usize;
        match status {
            Status::StreamEnd => {
                ended = true;
                break;
            }
            Status::BufError => {
                if out.len() == before_out && now_consumed == consumed {
                    break;
                }
            }
            Status::Ok => {
                if now_consumed >= input.len() && out.len() < out.capacity() {
                    break;
                }
            }
        }
    }
    let consumed = (d.total_in() - start) as usize;
    Ok((out, consumed, ended))
}

fn decompress(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("decompress", args, kw, &["data", "wbits", "bufsize"], 1)?;
    let data = want_data(s[0].as_ref().unwrap())?;
    let wrap = wrap_of(s[1].as_ref().map(want_int).transpose()?.unwrap_or(15))?;
    let mut d = new_decompress(wrap, &data);
    let (out, _, ended) = run_decompress(&mut d, &data, 0)?;
    if !ended {
        return Err(exc(
            "ValueError",
            "Error -5 while decompressing data: incomplete or truncated stream",
        ));
    }
    Ok(Value::bytes(out))
}

struct CompObj {
    state: RefCell<Option<Deflate>>,
}

impl ExtObject for CompObj {
    fn type_name(&self) -> &'static str {
        "Compress"
    }

    fn methods(&self) -> &'static [&'static str] {
        &["compress", "flush"]
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        no_kwargs(name, &kw)?;
        let mut guard = self.state.borrow_mut();
        let Some(c) = guard.as_mut() else {
            return Err(exc("ValueError", "Inconsistent stream state"));
        };
        match name {
            "compress" => {
                let data = want_data(args.first().ok_or_else(|| type_error("compress() takes exactly one argument (0 given)"))?)?;
                Ok(Value::bytes(run_compress(c, &data, Flush::None)?))
            }
            _ => {
                let mode = args.first().map(want_int).transpose()?.unwrap_or(4);
                let flush = match mode {
                    0 => return Ok(Value::bytes(Vec::new())),
                    1 => Flush::Partial,
                    2 => Flush::Sync,
                    3 => Flush::Full,
                    _ => Flush::Finish,
                };
                let out = run_compress(c, &[], flush)?;
                if mode == 4 {
                    *guard = None;
                }
                Ok(Value::bytes(out))
            }
        }
    }
}

fn compressobj(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("compressobj", args, kw, &["level", "method", "wbits", "memLevel", "strategy", "zdict"], 0)?;
    let level = level_of(s[0].as_ref().map(want_int).transpose()?.unwrap_or(-1))?;
    let wrap = wrap_of(s[2].as_ref().map(want_int).transpose()?.unwrap_or(15))?;
    let mem = s[3].as_ref().map(want_int).transpose()?.unwrap_or(8) as i32;
    let strategy = strategy_of(s[4].as_ref().map(want_int).transpose()?.unwrap_or(0));
    Ok(Value::Ext(Rc::new(CompObj { state: RefCell::new(Some(new_compress(level, wrap, mem, strategy)?)) })))
}

struct DecompObj {
    wrap: Wrap,
    state: RefCell<Option<Decompress>>,
    unused: RefCell<Vec<u8>>,
    tail: RefCell<Vec<u8>>,
    eof: RefCell<bool>,
}

impl ExtObject for DecompObj {
    fn type_name(&self) -> &'static str {
        "Decompress"
    }

    fn methods(&self) -> &'static [&'static str] {
        &["decompress", "flush"]
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "unused_data" => Some(Ok(Value::bytes(self.unused.borrow().clone()))),
            "unconsumed_tail" => Some(Ok(Value::bytes(self.tail.borrow().clone()))),
            "eof" => Some(Ok(Value::Bool(*self.eof.borrow()))),
            _ => None,
        }
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        match name {
            "decompress" => {
                let s = bind("decompress", args, kw, &["data", "max_length"], 1)?;
                let data = want_data(s[0].as_ref().unwrap())?;
                let max = s[1].as_ref().map(want_int).transpose()?.unwrap_or(0);
                if max < 0 {
                    return Err(exc("ValueError", "max_length must be non-negative"));
                }
                if *self.eof.borrow() {
                    self.unused.borrow_mut().extend_from_slice(&data);
                    return Ok(Value::bytes(Vec::new()));
                }
                let mut input = std::mem::take(&mut *self.tail.borrow_mut());
                input.extend_from_slice(&data);
                let mut guard = self.state.borrow_mut();
                if guard.is_none() {
                    if input.is_empty() {
                        return Ok(Value::bytes(Vec::new()));
                    }
                    *guard = Some(new_decompress(self.wrap, &input));
                }
                let d = guard.as_mut().unwrap();
                let (out, consumed, ended) = run_decompress(d, &input, max as usize)?;
                let rest = input[consumed..].to_vec();
                if ended {
                    *self.eof.borrow_mut() = true;
                    *self.unused.borrow_mut() = rest;
                } else {
                    *self.tail.borrow_mut() = rest;
                }
                Ok(Value::bytes(out))
            }
            _ => {
                let mut guard = self.state.borrow_mut();
                let Some(d) = guard.as_mut() else { return Ok(Value::bytes(Vec::new())) };
                let input = std::mem::take(&mut *self.tail.borrow_mut());
                let (out, consumed, ended) = run_decompress(d, &input, 0)?;
                if ended {
                    *self.eof.borrow_mut() = true;
                    *self.unused.borrow_mut() = input[consumed..].to_vec();
                }
                Ok(Value::bytes(out))
            }
        }
    }
}

fn decompressobj(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("decompressobj", args, kw, &["wbits", "zdict"], 0)?;
    let wrap = wrap_of(s[0].as_ref().map(want_int).transpose()?.unwrap_or(15))?;
    let state = match wrap {
        Wrap::Auto(_) => None,
        other => Some(new_decompress(other, &[])),
    };
    Ok(Value::Ext(Rc::new(DecompObj {
        wrap,
        state: RefCell::new(state),
        unused: RefCell::new(Vec::new()),
        tail: RefCell::new(Vec::new()),
        eof: RefCell::new(false),
    })))
}

fn adler32(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("adler32", args, kw, &["data", "value"], 1)?;
    let data = want_data(s[0].as_ref().unwrap())?;
    let init = s[1].as_ref().map(want_int).transpose()?.unwrap_or(1) as u32;
    let (mut a, mut b) = (init & 0xffff, init >> 16);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += u32::from(x);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    Ok(Value::Int(i64::from((b << 16) | a)))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_zlib")
        .func("compress", compress)
        .func("decompress", decompress)
        .func("compressobj", compressobj)
        .func("decompressobj", decompressobj)
        .func("adler32", adler32)
        .build()
}
