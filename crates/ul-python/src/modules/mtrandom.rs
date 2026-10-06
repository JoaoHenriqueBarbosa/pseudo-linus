//! `_mt`: o Mersenne Twister (MT19937) do `random`, em Rust. A sequência é a do CPython para a mesma
//! semente; o `random.py` mantém a API (sementes, `state`, distribuições) e delega a este objeto o que é
//! quente: gerar palavras de 32 bits, `random()` e `getrandbits(k)` até 62 bits.

use std::cell::RefCell;
use std::rc::Rc;

use crate::modules::ModuleBuilder;
use crate::native_util::{exactly, no_kwargs, want_int};
use crate::object::{ExtObject, Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyException, PyResult, Vm};

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER: u32 = 0x8000_0000;
const LOWER: u32 = 0x7fff_ffff;

struct State {
    mt: Vec<u32>,
    index: usize,
    seeded: bool,
}

impl State {
    fn init_genrand(&mut self, s: u32) {
        self.mt[0] = s;
        for i in 1..N {
            let prev = self.mt[i - 1];
            self.mt[i] = 1_812_433_253u32.wrapping_mul(prev ^ (prev >> 30)).wrapping_add(i as u32);
        }
        self.index = N;
    }

    fn init_by_array(&mut self, key: &[u32]) {
        self.init_genrand(19_650_218);
        let mut i = 1usize;
        let mut j = 0usize;
        let mut k = N.max(key.len());
        while k > 0 {
            let prev = self.mt[i - 1];
            self.mt[i] = (self.mt[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
            k -= 1;
        }
        k = N - 1;
        while k > 0 {
            let prev = self.mt[i - 1];
            self.mt[i] = (self.mt[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1_566_083_941)).wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            k -= 1;
        }
        self.mt[0] = 0x8000_0000;
        self.index = N;
        self.seeded = true;
    }

    fn refill(&mut self) {
        let mag = |y: u32| if y & 1 == 1 { MATRIX_A } else { 0 };
        for kk in 0..N - M {
            let y = (self.mt[kk] & UPPER) | (self.mt[kk + 1] & LOWER);
            self.mt[kk] = self.mt[kk + M] ^ (y >> 1) ^ mag(y);
        }
        for kk in N - M..N - 1 {
            let y = (self.mt[kk] & UPPER) | (self.mt[kk + 1] & LOWER);
            self.mt[kk] = self.mt[kk + M - N] ^ (y >> 1) ^ mag(y);
        }
        let y = (self.mt[N - 1] & UPPER) | (self.mt[0] & LOWER);
        self.mt[N - 1] = self.mt[M - 1] ^ (y >> 1) ^ mag(y);
        self.index = 0;
    }

    fn genrand(&mut self) -> u32 {
        if self.index >= N {
            self.refill();
        }
        let mut y = self.mt[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }
}

struct Mt {
    state: RefCell<State>,
}

fn unseeded() -> PyException {
    exc("RuntimeError", "_mt: o gerador ainda não recebeu semente")
}

impl ExtObject for Mt {
    fn type_name(&self) -> &'static str {
        "MersenneTwister"
    }

    fn methods(&self) -> &'static [&'static str] {
        &["init_by_array", "genrand32", "random", "getrandbits", "getstate", "setstate", "seeded"]
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        no_kwargs(name, &kw)?;
        let mut st = self.state.borrow_mut();
        match name {
            "init_by_array" => {
                exactly(name, &args, 1)?;
                let Value::List(l) = &args[0] else { return Err(type_error("init_by_array() espera uma lista")) };
                let key = l.borrow().iter().map(|v| want_int(v).map(|n| n as u32)).collect::<PyResult<Vec<u32>>>()?;
                if key.is_empty() {
                    return Err(type_error("init_by_array() espera uma chave não vazia"));
                }
                st.init_by_array(&key);
                Ok(Value::None)
            }
            "seeded" => Ok(Value::Bool(st.seeded)),
            "genrand32" => {
                if !st.seeded {
                    return Err(unseeded());
                }
                Ok(Value::Int(i64::from(st.genrand())))
            }
            "random" => {
                if !st.seeded {
                    return Err(unseeded());
                }
                let a = st.genrand() >> 5;
                let b = st.genrand() >> 6;
                Ok(Value::Float((f64::from(a) * 67_108_864.0 + f64::from(b)) * (1.0 / 9_007_199_254_740_992.0)))
            }
            "getrandbits" => {
                exactly(name, &args, 1)?;
                let k = want_int(&args[0])?;
                if !st.seeded {
                    return Err(unseeded());
                }
                if k < 0 {
                    return Err(exc("ValueError", "number of bits must be non-negative"));
                }
                if k == 0 {
                    return Ok(Value::Int(0));
                }
                if k > 62 {
                    return Err(exc("OverflowError", "_mt: getrandbits() nativo vai até 62 bits"));
                }
                if k <= 32 {
                    return Ok(Value::Int(i64::from(st.genrand() >> (32 - k))));
                }
                let mut k = k;
                let mut result: u64 = 0;
                let mut shift = 0u32;
                for _ in 0..((k - 1) / 32 + 1) {
                    let mut r = st.genrand();
                    if k < 32 {
                        r >>= 32 - k;
                    }
                    result |= u64::from(r) << shift;
                    shift += 32;
                    k -= 32;
                }
                Ok(Value::Int(result as i64))
            }
            "getstate" => {
                let mut out: Vec<Value> = st.mt.iter().map(|&w| Value::Int(i64::from(w))).collect();
                out.push(Value::Int(st.index as i64));
                Ok(Value::tuple(out))
            }
            "setstate" => {
                exactly(name, &args, 1)?;
                let Value::Tuple(t) = &args[0] else { return Err(type_error("setstate() espera uma tupla")) };
                if t.len() != N + 1 {
                    return Err(exc("ValueError", "state vector is the wrong size"));
                }
                let mut words = Vec::with_capacity(N);
                for v in &t[..N] {
                    let w = want_int(v)?;
                    if !(0..=0xffff_ffff).contains(&w) {
                        return Err(exc("ValueError", "state vector invalid"));
                    }
                    words.push(w as u32);
                }
                let index = want_int(&t[N])?;
                if !(0..=N as i64).contains(&index) {
                    return Err(exc("ValueError", "invalid state"));
                }
                st.mt = words;
                st.index = index as usize;
                st.seeded = true;
                Ok(Value::None)
            }
            other => Err(exc("AttributeError", format!("'MersenneTwister' object has no attribute '{other}'"))),
        }
    }
}

fn new_state(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("new", &kw)?;
    exactly("new", &args, 0)?;
    Ok(Value::Ext(Rc::new(Mt { state: RefCell::new(State { mt: vec![0; N], index: N, seeded: false }) })))
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_mt").func("new", new_state).build()
}
