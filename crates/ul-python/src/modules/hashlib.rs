//! `_hashimpl`: os resumos, o HMAC e os derivadores de chave por trás de `_hashlib`, `_blake2`, `_md5`,
//! `_sha1`, `_sha2` e `_sha3` (módulos em Python que reproduzem a superfície dos de C do CPython). Os
//! algoritmos vivem em `ul_common::hash`; aqui ficam só os objetos nativos (`HASH`, `HASHXOF`, `HMAC`) e as
//! funções que os criam. É módulo de apoio: só código embutido o importa.

use std::cell::RefCell;
use std::rc::Rc;

use ul_common::codec::hex_lower as hex_of;
use ul_common::hash::blake2::{self, Params};
use ul_common::hash::{self, Algo, Hasher, Hmac};

use crate::modules::ModuleBuilder;
use crate::native_util::{bind, exactly, no_kwargs, want_int, want_str};
use crate::object::{ExtObject, Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

fn want_hash_input(v: &Value) -> PyResult<Vec<u8>> {
    match v {
        Value::Bytes(_) | Value::ByteArray(_) | Value::Instance(_) if v.bytes_like().is_some() => {
            Ok(v.bytes_like().map(|b| b.to_vec()).unwrap_or_default())
        }
        Value::Str(_) => Err(type_error("Strings must be encoded before hashing")),
        _ => Err(type_error("object supporting the buffer API required")),
    }
}

/// O argumento obrigatório `i` que o `bind` já garantiu presente.
fn slot(s: &[Option<Value>], i: usize) -> PyResult<&Value> {
    s[i].as_ref().ok_or_else(|| type_error("missing required argument"))
}

/// `int` não negativo que cabe em 64 bits (o `node_offset` do BLAKE2b passa de `i64`).
fn want_u64(v: &Value) -> PyResult<u64> {
    match v {
        Value::Big(b) => b.to_string().parse::<u64>().map_err(|_| exc("OverflowError", "Python int too large to convert to C unsigned long")),
        other => u64::try_from(want_int(other)?).map_err(|_| exc("OverflowError", "can't convert negative int to unsigned")),
    }
}

fn unsupported(name: &str) -> crate::vm::PyException {
    exc("ValueError", format!("unsupported hash type {name}"))
}

fn algo_named(name: &str) -> PyResult<Algo> {
    Algo::from_name(name).ok_or_else(|| unsupported(name))
}

// ---------------------------------------------------------------------------
// HASH e HASHXOF
// ---------------------------------------------------------------------------

struct HashObj {
    algo: Algo,
    hasher: RefCell<Hasher>,
}

/// Refaz um `HASH`, `HASHXOF` ou `HMAC` a partir da imagem do heap: o algoritmo e o estado do resumo.
pub(crate) fn restore_image(tag: &str, state: &(dyn std::any::Any + Send + Sync), _refs: Vec<Value>) -> Option<Value> {
    if tag == "hmac" {
        let (algo, mac) = state.downcast_ref::<(Algo, Hmac)>()?;
        return Some(Value::Ext(Rc::new(HmacObj { algo: *algo, mac: RefCell::new(mac.clone()) })));
    }
    let (algo, hasher) = state.downcast_ref::<(Algo, Hasher)>()?;
    Some(make_hash(*algo, hasher.clone()))
}

fn make_hash(algo: Algo, hasher: Hasher) -> Value {
    Value::Ext(Rc::new(HashObj { algo, hasher: RefCell::new(hasher) }))
}

/// `digest()` e `hexdigest()`: o tamanho é o do algoritmo, ou o argumento `length` nos de saída livre.
fn finish(algo: Algo, hasher: &Hasher, method: &str, args: Vec<Value>, kw: Kw) -> PyResult<Vec<u8>> {
    if !algo.is_xof() {
        no_kwargs(method, &kw)?;
        exactly(method, &args, 0)?;
        return Ok(hasher.finalize(0));
    }
    let s = bind(method, args, kw, &["length"], 0)?;
    let Some(length) = &s[0] else {
        return Err(type_error(format!("{method}() missing required argument 'length' (pos 1)")));
    };
    let length = want_int(length)?;
    if length < 0 {
        return Err(exc("ValueError", "negative digest length"));
    }
    if length > (1 << 29) {
        return Err(exc("ValueError", "length is too large"));
    }
    Ok(hasher.finalize(length as usize))
}

impl ExtObject for HashObj {
    fn type_name(&self) -> &'static str {
        if self.algo.is_xof() { "HASHXOF" } else { "HASH" }
    }

    fn image(&self) -> Option<crate::object::ExtImage> {
        crate::object::OpaqueImage::image("hash", (self.algo, self.hasher.borrow().clone()), Vec::new())
    }

    fn repr(&self) -> String {
        format!(
            "<{} _hashlib.{} object @ {:#x}>",
            self.algo.name(),
            self.type_name(),
            crate::object::py_addr(self as *const Self as usize)
        )
    }

    fn methods(&self) -> &'static [&'static str] {
        &["update", "digest", "hexdigest", "copy"]
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "name" => Some(Ok(Value::str(self.algo.name()))),
            "digest_size" => Some(Ok(Value::Int(self.algo.digest_size() as i64))),
            "block_size" => Some(Ok(Value::Int(self.algo.block_size() as i64))),
            _ => None,
        }
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        match name {
            "update" => {
                no_kwargs("update", &kw)?;
                exactly("update", &args, 1)?;
                let data = want_hash_input(&args[0])?;
                self.hasher.borrow_mut().update(&data);
                Ok(Value::None)
            }
            "digest" => Ok(Value::bytes(finish(self.algo, &self.hasher.borrow(), "digest", args, kw)?)),
            "hexdigest" => Ok(Value::str(hex_of(&finish(self.algo, &self.hasher.borrow(), "hexdigest", args, kw)?))),
            "copy" => {
                no_kwargs("copy", &kw)?;
                exactly("copy", &args, 0)?;
                Ok(make_hash(self.algo, self.hasher.borrow().clone()))
            }
            _ => Err(crate::object::no_attribute(self.type_name(), name)),
        }
    }
}

/// `new(name, data=b'')`: o resumo de nome OpenSSL `name`, já alimentado com `data`.
/// Os dados opcionais com que `new(name, data)` e `hmac_new(name, key, msg)` já alimentam o objeto.
fn fed<T>(mut state: T, data: &Option<Value>, update: fn(&mut T, &[u8])) -> PyResult<T> {
    if let Some(v) = data {
        update(&mut state, &want_hash_input(v)?);
    }
    Ok(state)
}

fn new(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("new", args, kw, &["name", "data"], 1)?;
    let algo = algo_named(want_str("new", slot(&s, 0)?)?)?;
    Ok(make_hash(algo, fed(algo.hasher(), &s[1], Hasher::update)?))
}

/// `blake2(kind, data, digest_size, key, salt, person, fanout, depth, leaf_size, node_offset, node_depth,
/// inner_size, last_node)`: um BLAKE2b (`kind == "b"`) ou BLAKE2s com o bloco de parâmetros completo. Os
/// limites de cada campo são conferidos em Python (`_blake2`), com as mensagens do CPython.
fn blake2_new(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    const NAMES: [&str; 13] = [
        "kind", "data", "digest_size", "key", "salt", "person", "fanout", "depth", "leaf_size", "node_offset", "node_depth",
        "inner_size", "last_node",
    ];
    let s = bind("blake2", args, kw, &NAMES, NAMES.len())?;
    let size = want_int(slot(&s, 2)?)? as usize;
    let params = Params {
        digest_length: size,
        key: want_hash_input(slot(&s, 3)?)?,
        salt: want_hash_input(slot(&s, 4)?)?,
        person: want_hash_input(slot(&s, 5)?)?,
        fanout: want_int(slot(&s, 6)?)? as u8,
        depth: want_int(slot(&s, 7)?)? as u8,
        leaf_length: want_int(slot(&s, 8)?)? as u32,
        node_offset: want_u64(slot(&s, 9)?)?,
        node_depth: want_int(slot(&s, 10)?)? as u8,
        inner_length: want_int(slot(&s, 11)?)? as u8,
        last_node: slot(&s, 12)?.is_true(),
    };
    let (algo, mut hasher) = if want_str("blake2", slot(&s, 0)?)? == "b" {
        (Algo::Blake2b(size), Hasher::Blake2b(blake2::b::State::new(&params)))
    } else {
        (Algo::Blake2s(size), Hasher::Blake2s(blake2::s::State::new(&params)))
    };
    hasher.update(&want_hash_input(slot(&s, 1)?)?);
    Ok(make_hash(algo, hasher))
}

// ---------------------------------------------------------------------------
// HMAC
// ---------------------------------------------------------------------------

struct HmacObj {
    algo: Algo,
    mac: RefCell<Hmac>,
}

impl ExtObject for HmacObj {
    fn type_name(&self) -> &'static str {
        "HMAC"
    }

    fn image(&self) -> Option<crate::object::ExtImage> {
        crate::object::OpaqueImage::image("hmac", (self.algo, self.mac.borrow().clone()), Vec::new())
    }

    fn repr(&self) -> String {
        format!("<{} HMAC object @ {:#x}>", self.algo.name(), crate::object::py_addr(self as *const Self as usize))
    }

    fn methods(&self) -> &'static [&'static str] {
        &["update", "digest", "hexdigest", "copy"]
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "name" => Some(Ok(Value::str(format!("hmac-{}", self.algo.name())))),
            "digest_size" => Some(Ok(Value::Int(self.algo.digest_size() as i64))),
            "block_size" => Some(Ok(Value::Int(self.algo.block_size() as i64))),
            _ => None,
        }
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        no_kwargs(name, &kw)?;
        match name {
            "update" => {
                exactly("update", &args, 1)?;
                let data = want_hash_input(&args[0])?;
                self.mac.borrow_mut().update(&data);
                Ok(Value::None)
            }
            "digest" => {
                exactly("digest", &args, 0)?;
                Ok(Value::bytes(self.mac.borrow().finalize()))
            }
            "hexdigest" => {
                exactly("hexdigest", &args, 0)?;
                Ok(Value::str(hex_of(&self.mac.borrow().finalize())))
            }
            "copy" => {
                exactly("copy", &args, 0)?;
                Ok(Value::Ext(Rc::new(HmacObj { algo: self.algo, mac: RefCell::new(self.mac.borrow().clone()) })))
            }
            _ => Err(crate::object::no_attribute("HMAC", name)),
        }
    }
}

fn keyed(name: &str, key: &[u8]) -> PyResult<(Algo, Hmac)> {
    let algo = algo_named(name)?;
    let mac = Hmac::new(algo, key).ok_or_else(|| unsupported(name))?;
    Ok((algo, mac))
}

/// `hmac_new(name, key, msg=b'')`.
fn hmac_new(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("hmac_new", args, kw, &["name", "key", "msg"], 2)?;
    let name = want_str("hmac_new", slot(&s, 0)?)?;
    let (algo, mac) = keyed(name, &want_hash_input(slot(&s, 1)?)?)?;
    let mac = fed(mac, &s[2], Hmac::update)?;
    Ok(Value::Ext(Rc::new(HmacObj { algo, mac: RefCell::new(mac) })))
}

/// `hmac_digest(name, key, msg)`.
fn hmac_digest(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("hmac_digest", args, kw, &["name", "key", "msg"], 3)?;
    let name = want_str("hmac_digest", slot(&s, 0)?)?;
    let key = want_hash_input(slot(&s, 1)?)?;
    let msg = want_hash_input(slot(&s, 2)?)?;
    let algo = algo_named(name)?;
    if algo.is_xof() {
        return Err(unsupported(name));
    }
    Ok(Value::bytes(hash::hmac(algo, &key, &msg)))
}

// ---------------------------------------------------------------------------
// PBKDF2 e scrypt
// ---------------------------------------------------------------------------

/// `pbkdf2(name, password, salt, iterations, dklen)`: `dklen` já resolvido (o padrão é o tamanho do
/// resumo, resolvido em Python).
fn pbkdf2(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("pbkdf2", args, kw, &["name", "password", "salt", "iterations", "dklen"], 5)?;
    let name = want_str("pbkdf2", slot(&s, 0)?)?;
    let algo = algo_named(name)?;
    if algo.is_xof() {
        return Err(unsupported(name));
    }
    let password = want_hash_input(slot(&s, 1)?)?;
    let salt = want_hash_input(slot(&s, 2)?)?;
    let iterations = want_int(slot(&s, 3)?)?;
    let dklen = want_int(slot(&s, 4)?)?;
    if iterations < 1 || dklen < 1 {
        return Err(exc("ValueError", "invalid pbkdf2 parameters"));
    }
    Ok(Value::bytes(hash::pbkdf2(algo, &password, &salt, iterations as u64, dklen as usize)))
}

/// `scrypt(password, salt, n, r, p, dklen)`, com os parâmetros já validados em Python.
fn scrypt(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = bind("scrypt", args, kw, &["password", "salt", "n", "r", "p", "dklen"], 6)?;
    let password = want_hash_input(slot(&s, 0)?)?;
    let salt = want_hash_input(slot(&s, 1)?)?;
    let (n, r, p, dklen) = (want_u64(slot(&s, 2)?)?, want_u64(slot(&s, 3)?)?, want_u64(slot(&s, 4)?)?, want_u64(slot(&s, 5)?)?);
    Ok(Value::bytes(hash::scrypt(&password, &salt, n, r as u32, p as u32, dklen as usize)))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_hashimpl")
        .func("new", new)
        .func("blake2", blake2_new)
        .func("hmac_new", hmac_new)
        .func("hmac_digest", hmac_digest)
        .func("pbkdf2", pbkdf2)
        .func("scrypt", scrypt)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::repr;

    #[test]
    fn python_object() {
        let mut vm = Vm::new();
        let h = new(&mut vm, vec![Value::str("md5"), Value::bytes(b"a".to_vec())], Vec::new()).unwrap();
        let Value::Ext(obj) = &h else { panic!("esperava um objeto Ext") };
        obj.call_method(&mut vm, "update", vec![Value::bytes(b"bc".to_vec())], Vec::new()).unwrap();
        let hexd = obj.call_method(&mut vm, "hexdigest", Vec::new(), Vec::new()).unwrap();
        assert_eq!(repr(&hexd), "'900150983cd24fb0d6963f7d28e17f72'");
        let copy = obj.call_method(&mut vm, "copy", Vec::new(), Vec::new()).unwrap();
        let Value::Ext(c) = &copy else { panic!("esperava um objeto Ext") };
        c.call_method(&mut vm, "update", vec![Value::bytes(b"d".to_vec())], Vec::new()).unwrap();
        let d1 = obj.call_method(&mut vm, "digest", Vec::new(), Vec::new()).unwrap();
        let d2 = c.call_method(&mut vm, "digest", Vec::new(), Vec::new()).unwrap();
        assert_ne!(repr(&d1), repr(&d2));
        let size = obj.getattr(&mut vm, "digest_size").unwrap().unwrap();
        assert_eq!(repr(&size), "16");
        let name = obj.getattr(&mut vm, "name").unwrap().unwrap();
        assert_eq!(repr(&name), "'md5'");
        let e = new(&mut vm, vec![Value::str("nope")], Vec::new()).unwrap_err();
        assert_eq!(e.msg, "unsupported hash type nope");
        let e = obj.call_method(&mut vm, "update", vec![Value::str("x")], Vec::new()).unwrap_err();
        assert_eq!(e.msg, "Strings must be encoded before hashing");
    }

    #[test]
    fn xof_needs_a_length() {
        let mut vm = Vm::new();
        let h = new(&mut vm, vec![Value::str("shake_128")], Vec::new()).unwrap();
        let Value::Ext(obj) = &h else { panic!("esperava um objeto Ext") };
        assert_eq!(obj.type_name(), "HASHXOF");
        let e = obj.call_method(&mut vm, "digest", Vec::new(), Vec::new()).unwrap_err();
        assert_eq!(e.msg, "digest() missing required argument 'length' (pos 1)");
        let d = obj.call_method(&mut vm, "hexdigest", vec![Value::Int(4)], Vec::new()).unwrap();
        assert_eq!(repr(&d), "'7f9c2ba4'");
    }
}
