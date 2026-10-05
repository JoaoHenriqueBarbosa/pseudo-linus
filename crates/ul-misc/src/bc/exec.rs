//! Código e máquina do bc, com o comportamento observável do GNU bc 1.07.1 medido em caixa preta.
//!
//! O compilador gera uma lista de instruções nossas, mas cada uma carrega o tamanho em bytes que a
//! construção equivalente ocupa no bytecode do GNU, porque o endereço aparece nas mensagens
//! (`Runtime error (func=f, adr=7): ...`). Os tamanhos foram medidos variando expressões no oráculo
//! e lendo o `adr` dos erros: constante `0` e `1` ocupam 1 byte e as outras `2 + dígitos`; carga e
//! gravação de variável `1 + nome` (o nome ocupa 1 byte até o índice 127 e 2 acima); operador 1;
//! salto 3; `sqrt`/`length`/`scale`/`read`/`random` 2; chamada `1 + nome + argumentos + 1`; o fim
//! de cada comando de expressão 1. O erro sai com o endereço do byte seguinte à instrução que
//! falhou (o `pc` já avançado).
//!
//! Semântica (também medida):
//!
//! - variáveis com escopo dinâmico: parâmetros e `auto` empilham sobre a variável global de mesmo
//!   nome e uma função chamada enxerga os da chamadora;
//! - arrays por valor (cópia) ou por referência (`*a[]`), índice truncado, de 0 a 16777215;
//! - `ibase`, `obase` e `scale` validados na gravação, com aviso e valor corrigido; o valor que
//!   fica na pilha é o gravado pelo programa, não o corrigido;
//! - erro de execução interrompe o item de entrada inteiro (o resto da linha não roda) e desfaz as
//!   pilhas de variáveis das funções em curso.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use num_bigint::BigUint;

use super::lexer::StdinShare;
use super::number::{self, Num, RaiseError};
use super::output::Output;
use crate::util::io;

pub type Label = u32;

/// Índices fixos das variáveis especiais (o GNU numera as do usuário a partir de 5).
pub const VAR_IBASE: u32 = 0;
pub const VAR_OBASE: u32 = 1;
pub const VAR_SCALE: u32 = 2;
pub const VAR_HISTORY: u32 = 3;
pub const VAR_LAST: u32 = 4;
pub const FIRST_USER_VAR: u32 = 5;

/// Bytes que um índice de nome ocupa no bytecode do GNU.
pub fn name_bytes(idx: u32) -> u32 {
    if idx > 127 { 2 } else { 1 }
}

/// Bytes de uma constante no bytecode do GNU (`0` e `1` têm instrução própria).
pub fn const_bytes(text: &[u8]) -> u32 {
    if text == b"0" || text == b"1" {
        1
    } else {
        2 + text.len() as u32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rel {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Clone, Debug)]
pub enum Ins {
    Num(Rc<[u8]>),
    Load(u32),
    Store(u32),
    LoadArr(u32),
    StoreArr(u32),
    /// Índice no topo: carrega o elemento e mantém o índice (atribuição composta em array).
    DupLoadArr(u32),
    /// `++x`/`--x` (verdadeiro = incremento).
    PreInc(u32, bool),
    PostInc(u32, bool),
    PreIncArr(u32, bool),
    PostIncArr(u32, bool),
    Bin(u8),
    Neg,
    Not,
    Rel(Rel),
    AndCheck(Label),
    AndEnd,
    OrCheck(Label),
    OrEnd,
    Jump(Label),
    JumpZero(Label),
    Pop,
    PrintPop,
    PrintNoNl,
    PrintStr(Rc<[u8]>),
    StrStmt(Rc<[u8]>),
    PushArray(u32),
    /// Função e o tipo de cada argumento (verdadeiro = array), na ordem do código.
    Call(u32, Rc<[bool]>),
    Ret,
    Sqrt,
    Length,
    ScaleOf,
    Read,
    Random,
    Halt,
}

/// Uma sequência de instruções com os endereços do GNU e os rótulos resolvidos.
#[derive(Clone, Debug, Default)]
pub struct Code {
    pub ins: Vec<Ins>,
    /// Endereço (em bytes do GNU) do começo de cada instrução.
    pub addr: Vec<u32>,
    pub size: u32,
    labels: Vec<Option<u32>>,
}

impl Code {
    pub fn emit(&mut self, ins: Ins, size: u32) {
        self.addr.push(self.size);
        self.ins.push(ins);
        self.size += size;
    }

    pub fn new_label(&mut self) -> Label {
        self.labels.push(None);
        (self.labels.len() - 1) as Label
    }

    pub fn define(&mut self, l: Label) {
        self.labels[l as usize] = Some(self.ins.len() as u32);
    }

    fn target(&self, l: Label) -> usize {
        self.labels[l as usize].unwrap_or(self.ins.len() as u32) as usize
    }

    /// Endereço do byte seguinte à instrução `pc`.
    fn end_of(&self, pc: usize) -> u32 {
        self.addr.get(pc + 1).copied().unwrap_or(self.size)
    }

    pub fn is_empty(&self) -> bool {
        self.ins.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Var,
    Array,
    /// `*a[]` (ou `&a[]`): array por referência.
    RefArray,
}

impl Kind {
    pub fn is_array(self) -> bool {
        self != Kind::Var
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Param {
    pub idx: u32,
    pub kind: Kind,
}

/// Funções da biblioteca matemática (`-l`), implementadas aqui com os mesmos passos e escalas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lib {
    E,
    L,
    S,
    A,
    C,
    J,
}

#[derive(Clone, Debug, Default)]
pub struct Func {
    pub name: Vec<u8>,
    pub defined: bool,
    pub void: bool,
    pub params: Vec<Param>,
    pub autos: Vec<Param>,
    pub code: Rc<Code>,
    pub native: Option<Lib>,
}

type Array = Rc<RefCell<BTreeMap<u32, Num>>>;

/// Por que a execução parou antes do fim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// Erro de execução: o item corrente é abandonado.
    Error,
    /// O processo termina com esse código (`halt`, falta de memória, falha imitada).
    Exit(i32),
}

type R<T> = Result<T, Stop>;

struct Frame {
    code: Rc<Code>,
    pc: usize,
    func: u32,
    pushed_vars: Vec<u32>,
    pushed_arrays: Vec<u32>,
}

/// O estado do programa: variáveis, arrays, funções e a saída.
pub struct Vm {
    pub out: Output,
    pub ibase: u32,
    pub obase: u64,
    pub scale: u32,
    history: Num,
    last: Num,
    vars: Vec<Vec<Num>>,
    arrays: Vec<Vec<Array>>,
    pub funcs: Vec<Func>,
    var_names: HashMap<Vec<u8>, u32>,
    arr_names: HashMap<Vec<u8>, u32>,
    arr_name_of: Vec<Vec<u8>>,
    func_names: HashMap<Vec<u8>, u32>,
    stdin: StdinShare,
    steps: u32,
    /// Nome da função em execução (pras mensagens da biblioteca).
    cur_func: u32,
}

fn digits_of(n: u32) -> u32 {
    n.to_string().len() as u32
}

/// Bytes da instrução que empilha um array como argumento (`K<n>:`).
pub fn push_array_bytes(idx: u32) -> u32 {
    2 + digits_of(idx)
}

fn k(s: &str) -> Num {
    Num::parse_constant(s.as_bytes(), 10).unwrap_or_default()
}

fn int(v: u64) -> Num {
    Num::from_u64(v)
}

impl Vm {
    pub fn new(line_size: i64) -> Vm {
        let main = Func {
            name: b"(main)".to_vec(),
            defined: true,
            ..Func::default()
        };
        Vm {
            out: Output::new(line_size),
            ibase: 10,
            obase: 10,
            scale: 0,
            history: Num::from_i64(-1),
            last: Num::zero(),
            vars: Vec::new(),
            arrays: Vec::new(),
            funcs: vec![main],
            var_names: HashMap::new(),
            arr_names: HashMap::new(),
            arr_name_of: vec![Vec::new()],
            func_names: HashMap::new(),
            stdin: StdinShare::new(),
            steps: 0,
            cur_func: 0,
        }
    }

    pub fn var_index(&mut self, name: &[u8]) -> u32 {
        let next = FIRST_USER_VAR + self.var_names.len() as u32;
        *self.var_names.entry(name.to_vec()).or_insert(next)
    }

    pub fn array_index(&mut self, name: &[u8]) -> u32 {
        if let Some(&i) = self.arr_names.get(name) {
            return i;
        }
        let i = self.arr_name_of.len() as u32;
        self.arr_names.insert(name.to_vec(), i);
        self.arr_name_of.push(name.to_vec());
        i
    }

    pub fn func_index(&mut self, name: &[u8]) -> u32 {
        if let Some(&i) = self.func_names.get(name) {
            return i;
        }
        let i = self.funcs.len() as u32;
        self.func_names.insert(name.to_vec(), i);
        self.funcs.push(Func {
            name: name.to_vec(),
            ..Func::default()
        });
        i
    }

    /// Carrega a biblioteca matemática: as seis funções e `scale=20`.
    pub fn install_mathlib(&mut self) {
        for (name, lib, nparams) in [
            (b"e", Lib::E, 1),
            (b"l", Lib::L, 1),
            (b"s", Lib::S, 1),
            (b"a", Lib::A, 1),
            (b"c", Lib::C, 1),
            (b"j", Lib::J, 2),
        ] {
            let fi = self.func_index(name) as usize;
            let params = (0..nparams)
                .map(|i| Param {
                    idx: i,
                    kind: Kind::Var,
                })
                .collect();
            self.funcs[fi] = Func {
                name: name.to_vec(),
                defined: true,
                params,
                native: Some(lib),
                ..Func::default()
            };
        }
        self.scale = 20;
    }

    // ---- mensagens ----

    fn func_name(&self, f: u32) -> String {
        io::lossy(&self.funcs[f as usize].name)
    }

    fn rt_error(&mut self, func: u32, adr: u32, msg: &str) -> Stop {
        self.out.flush();
        io::eprint(format!(
            "Runtime error (func={}, adr={adr}): {msg}\n",
            self.func_name(func)
        ));
        Stop::Error
    }

    fn rt_warn(&mut self, func: u32, adr: u32, msg: &str) {
        self.out.flush();
        io::eprint(format!(
            "Runtime warning (func={}, adr={adr}): {msg}\n",
            self.func_name(func)
        ));
    }

    fn out_of_memory(&mut self) -> Stop {
        self.out.flush();
        io::eprint("Fatal error: Out of memory for malloc.\n");
        Stop::Exit(1)
    }

    // ---- variáveis ----

    fn var_stack(&mut self, i: u32) -> &mut Vec<Num> {
        let i = i as usize;
        if self.vars.len() <= i {
            self.vars.resize_with(i + 1, Vec::new);
        }
        let s = &mut self.vars[i];
        if s.is_empty() {
            s.push(Num::zero());
        }
        s
    }

    fn array_stack(&mut self, i: u32) -> &mut Vec<Array> {
        let i = i as usize;
        if self.arrays.len() <= i {
            self.arrays.resize_with(i + 1, Vec::new);
        }
        let s = &mut self.arrays[i];
        if s.is_empty() {
            s.push(Rc::new(RefCell::new(BTreeMap::new())));
        }
        s
    }

    fn array(&mut self, i: u32) -> Array {
        self.array_stack(i).last().cloned().unwrap_or_default()
    }

    fn load(&mut self, i: u32) -> Num {
        match i {
            VAR_IBASE => int(u64::from(self.ibase)),
            VAR_OBASE => int(self.obase),
            VAR_SCALE => int(u64::from(self.scale)),
            VAR_HISTORY => self.history.clone(),
            VAR_LAST => self.last.clone(),
            _ => self.var_stack(i).last().cloned().unwrap_or_default(),
        }
    }

    /// Grava; `adr` é onde sai o aviso de valor corrigido das especiais.
    fn store(&mut self, i: u32, v: &Num, func: u32, adr: u32) {
        match i {
            VAR_IBASE => {
                let (neg, m) = int_parts(v);
                self.ibase = if neg {
                    self.rt_warn(func, adr, "negative ibase, set to 2");
                    2
                } else if m < BigUint::from(2u32) {
                    self.rt_warn(func, adr, "ibase too small, set to 2");
                    2
                } else if m > BigUint::from(number::IBASE_MAX) {
                    self.rt_warn(func, adr, "ibase too large, set to 36");
                    36
                } else {
                    big_u64(&m) as u32
                };
            }
            VAR_OBASE => {
                let (neg, m) = int_parts(v);
                self.obase = if neg {
                    self.rt_warn(func, adr, "negative obase, set to 2");
                    2
                } else if m < BigUint::from(2u32) {
                    self.rt_warn(func, adr, "obase too small, set to 2");
                    2
                } else if m > BigUint::from(number::BASE_MAX) {
                    self.rt_warn(
                        func,
                        adr,
                        &format!("obase too large, set to {}", number::BASE_MAX),
                    );
                    number::BASE_MAX
                } else {
                    big_u64(&m)
                };
            }
            VAR_SCALE => {
                let (neg, m) = int_parts(v);
                self.scale = if neg {
                    self.rt_warn(func, adr, "negative scale, set to 0");
                    0
                } else if m > BigUint::from(number::SCALE_MAX) {
                    self.rt_warn(
                        func,
                        adr,
                        &format!("scale too large, set to {}", number::SCALE_MAX),
                    );
                    number::SCALE_MAX as u32
                } else {
                    big_u64(&m) as u32
                };
            }
            VAR_HISTORY => self.history = v.clone(),
            VAR_LAST => self.last = v.clone(),
            _ => {
                if let Some(top) = self.var_stack(i).last_mut() {
                    *top = v.clone();
                }
            }
        }
    }

    /// Valida o índice de array como o GNU: parte inteira entre 0 e `BC_DIM_MAX`.
    fn index(&mut self, arr: u32, v: &Num, func: u32, adr: u32) -> R<u32> {
        let (neg, m) = int_parts(v);
        if !neg && m <= BigUint::from(number::DIM_MAX) {
            return Ok(big_u64(&m) as u32);
        }
        let name = io::lossy(&self.arr_name_of[arr as usize]);
        Err(self.rt_error(func, adr, &format!("Array {name} subscript out of bounds.")))
    }

    fn arr_get(&mut self, arr: u32, idx: u32) -> Num {
        self.array(arr)
            .borrow()
            .get(&idx)
            .cloned()
            .unwrap_or_default()
    }

    fn arr_set(&mut self, arr: u32, idx: u32, v: Num) {
        self.array(arr).borrow_mut().insert(idx, v);
    }

    // ---- saída ----

    fn print_num(&mut self, v: &Num) {
        let obase = self.obase;
        let out = &mut self.out;
        v.write(obase, &mut |c| out.put(c));
    }

    fn print_escaped(&mut self, s: &[u8]) {
        let mut i = 0;
        while i < s.len() {
            let c = s[i];
            i += 1;
            if c != b'\\' {
                self.out.put(c);
                continue;
            }
            let Some(&e) = s.get(i) else { break };
            i += 1;
            let r = match e {
                b'a' => Some(7),
                b'b' => Some(8),
                b'f' => Some(12),
                b'n' => Some(b'\n'),
                b'q' => Some(b'"'),
                b'r' => Some(b'\r'),
                b't' => Some(b'\t'),
                b'\\' => Some(b'\\'),
                _ => None,
            };
            if let Some(r) = r {
                self.out.put(r);
            }
        }
    }

    // ---- execução ----

    /// Executa o código principal de um item de entrada. Devolve `Some(código)` se o processo tem
    /// de terminar.
    pub fn run(&mut self, code: Rc<Code>) -> Option<i32> {
        let mut frames = vec![Frame {
            code,
            pc: 0,
            func: 0,
            pushed_vars: Vec::new(),
            pushed_arrays: Vec::new(),
        }];
        let mut stack: Vec<Num> = Vec::new();
        let r = self.exec(&mut frames, &mut stack);
        // Erro: desfaz o que as funções em curso empilharam.
        while let Some(f) = frames.pop() {
            self.unwind(&f);
        }
        self.cur_func = 0;
        match r {
            Ok(()) | Err(Stop::Error) => None,
            Err(Stop::Exit(c)) => Some(c),
        }
    }

    fn unwind(&mut self, f: &Frame) {
        for &v in f.pushed_vars.iter().rev() {
            if let Some(s) = self.vars.get_mut(v as usize) {
                s.pop();
            }
        }
        for &a in f.pushed_arrays.iter().rev() {
            if let Some(s) = self.arrays.get_mut(a as usize) {
                s.pop();
            }
        }
    }

    fn exec(&mut self, frames: &mut Vec<Frame>, stack: &mut Vec<Num>) -> R<()> {
        loop {
            let (code, pc, func) = {
                let f = frames.last().expect("quadro");
                (f.code.clone(), f.pc, f.func)
            };
            self.cur_func = func;
            if pc >= code.ins.len() {
                // Só o principal chega ao fim; funções sempre terminam num retorno.
                if frames.len() == 1 {
                    return Ok(());
                }
                stack.push(Num::zero());
                self.do_return(frames, stack);
                continue;
            }
            self.steps = self.steps.wrapping_add(1);
            if self.steps.is_multiple_of(1024) {
                sysabi::sys::checkpoint();
            }
            let adr = code.end_of(pc);
            let start = code.addr[pc];
            let mut next = pc + 1;
            match &code.ins[pc] {
                Ins::Num(t) => match Num::parse_constant(t, self.ibase) {
                    Ok(n) => stack.push(n),
                    Err(_) => return Err(self.out_of_memory()),
                },
                Ins::Load(i) => {
                    let v = self.load(*i);
                    stack.push(v);
                }
                Ins::Store(i) => {
                    let v = stack.last().cloned().unwrap_or_default();
                    self.store(*i, &v, func, adr);
                }
                Ins::LoadArr(a) => {
                    let iv = stack.pop().unwrap_or_default();
                    let idx = self.index(*a, &iv, func, adr)?;
                    let v = self.arr_get(*a, idx);
                    stack.push(v);
                }
                Ins::StoreArr(a) => {
                    let v = stack.pop().unwrap_or_default();
                    let iv = stack.pop().unwrap_or_default();
                    let idx = self.index(*a, &iv, func, adr)?;
                    self.arr_set(*a, idx, v.clone());
                    stack.push(v);
                }
                Ins::DupLoadArr(a) => {
                    let iv = stack.last().cloned().unwrap_or_default();
                    let idx = self.index(*a, &iv, func, adr)?;
                    let v = self.arr_get(*a, idx);
                    stack.push(v);
                }
                Ins::PreInc(i, up) => {
                    let old = self.load(*i);
                    let v = self.step(&old, *up)?;
                    self.store(*i, &v, func, start + 1 + name_bytes(*i));
                    stack.push(self.load(*i));
                }
                Ins::PostInc(i, up) => {
                    let old = self.load(*i);
                    let v = self.step(&old, *up)?;
                    self.store(*i, &v, func, adr);
                    stack.push(old);
                }
                Ins::PreIncArr(a, up) | Ins::PostIncArr(a, up) => {
                    let iv = stack.pop().unwrap_or_default();
                    let idx = self.index(*a, &iv, func, start + 2 + name_bytes(*a))?;
                    let old = self.arr_get(*a, idx);
                    let v = self.step(&old, *up)?;
                    self.arr_set(*a, idx, v.clone());
                    stack.push(if matches!(code.ins[pc], Ins::PreIncArr(..)) {
                        v
                    } else {
                        old
                    });
                }
                Ins::Bin(op) => {
                    let b = stack.pop().unwrap_or_default();
                    let a = stack.pop().unwrap_or_default();
                    let v = self.binary(*op, &a, &b, func, adr)?;
                    stack.push(v);
                }
                Ins::Neg => {
                    let a = stack.pop().unwrap_or_default();
                    stack.push(a.negate());
                }
                Ins::Not => {
                    let a = stack.pop().unwrap_or_default();
                    stack.push(int(u64::from(a.is_zero())));
                }
                Ins::Rel(r) => {
                    let b = stack.pop().unwrap_or_default();
                    let a = stack.pop().unwrap_or_default();
                    let o = a.compare(&b);
                    let t = match r {
                        Rel::Eq => o == Ordering::Equal,
                        Rel::Ne => o != Ordering::Equal,
                        Rel::Lt => o == Ordering::Less,
                        Rel::Le => o != Ordering::Greater,
                        Rel::Gt => o == Ordering::Greater,
                        Rel::Ge => o != Ordering::Less,
                    };
                    stack.push(int(u64::from(t)));
                }
                Ins::AndCheck(l) => {
                    let a = stack.pop().unwrap_or_default();
                    if a.is_zero() {
                        stack.push(Num::zero());
                        next = code.target(*l);
                    }
                }
                Ins::OrCheck(l) => {
                    let a = stack.pop().unwrap_or_default();
                    if !a.is_zero() {
                        stack.push(Num::one());
                        next = code.target(*l);
                    }
                }
                Ins::AndEnd | Ins::OrEnd => {
                    let a = stack.pop().unwrap_or_default();
                    stack.push(int(u64::from(!a.is_zero())));
                }
                Ins::Jump(l) => next = code.target(*l),
                Ins::JumpZero(l) => {
                    let a = stack.pop().unwrap_or_default();
                    if a.is_zero() {
                        next = code.target(*l);
                    }
                }
                Ins::Pop => {
                    stack.pop();
                }
                Ins::PrintPop => {
                    let v = stack.pop().unwrap_or_default();
                    self.print_num(&v);
                    self.out.put(b'\n');
                    self.last = v;
                }
                Ins::PrintNoNl => {
                    let v = stack.pop().unwrap_or_default();
                    self.print_num(&v);
                    self.last = v;
                }
                Ins::PrintStr(s) => {
                    let s = s.clone();
                    self.print_escaped(&s);
                }
                Ins::StrStmt(s) => {
                    let s = s.clone();
                    self.out.put_bytes(&s);
                }
                Ins::PushArray(a) => stack.push(int(u64::from(*a))),
                Ins::Call(fi, kinds) => {
                    frames.last_mut().expect("quadro").pc = next;
                    self.call(frames, stack, *fi, kinds, func, start)?;
                    continue;
                }
                Ins::Ret => {
                    self.do_return(frames, stack);
                    continue;
                }
                Ins::Sqrt => {
                    let a = stack.pop().unwrap_or_default();
                    match a.sqrt(self.scale) {
                        Ok(Some(v)) => stack.push(v),
                        Ok(None) => {
                            return Err(self.rt_error(
                                func,
                                adr,
                                "Square root of a negative number",
                            ));
                        }
                        Err(_) => return Err(self.out_of_memory()),
                    }
                }
                Ins::Length => {
                    let a = stack.pop().unwrap_or_default();
                    stack.push(int(a.length()));
                }
                Ins::ScaleOf => {
                    let a = stack.pop().unwrap_or_default();
                    stack.push(int(u64::from(a.scale())));
                }
                Ins::Read => {
                    let v = self.read_number();
                    stack.push(v);
                }
                Ins::Random => {
                    let mut b = [0u8; 4];
                    if let Some(s) = sysabi::sys::try_current() {
                        let _ = s.getrandom(&mut b);
                    }
                    stack.push(int(u64::from(u32::from_le_bytes(b) & 0x7fff_ffff)));
                }
                Ins::Halt => return Err(Stop::Exit(0)),
            }
            frames.last_mut().expect("quadro").pc = next;
        }
    }

    fn step(&mut self, v: &Num, up: bool) -> R<Num> {
        let one = Num::one();
        let r = if up { v.add(&one, 0) } else { v.sub(&one, 0) };
        r.map_err(|_| self.out_of_memory())
    }

    fn binary(&mut self, op: u8, a: &Num, b: &Num, func: u32, adr: u32) -> R<Num> {
        let s = self.scale;
        let r = match op {
            b'+' => a.add(b, 0).map(Some),
            b'-' => a.sub(b, 0).map(Some),
            b'*' => a.mul(b, s).map(Some),
            b'/' => match a.div(b, s) {
                Ok(None) => return Err(self.rt_error(func, adr, "Divide by zero")),
                r => r,
            },
            b'%' => match a.modulo(b, s) {
                Ok(None) => return Err(self.rt_error(func, adr, "Modulo by zero")),
                r => r,
            },
            _ => {
                return match a.raise(b, s) {
                    Ok((v, warn)) => {
                        if warn {
                            self.rt_warn(func, adr, "non-zero scale in exponent");
                        }
                        Ok(v)
                    }
                    Err(RaiseError::TooLarge) => {
                        Err(self.rt_error(func, adr, "exponent too large in raise"))
                    }
                    Err(RaiseError::DivideByZero) => {
                        Err(self.rt_error(func, adr, "divide by zero"))
                    }
                    Err(RaiseError::OutOfMemory) => Err(self.out_of_memory()),
                };
            }
        };
        match r {
            Ok(Some(v)) => Ok(v),
            Ok(None) => Ok(Num::zero()),
            Err(_) => Err(self.out_of_memory()),
        }
    }

    /// `read()`: um número do stdin no `ibase` corrente.
    fn read_number(&mut self) -> Num {
        let mut c = self.stdin.getchar();
        while matches!(c, Some(b' ' | b'\t' | b'\n')) {
            c = self.stdin.getchar();
        }
        let neg = c == Some(b'-');
        if neg {
            c = self.stdin.getchar();
        }
        let mut text = Vec::new();
        let mut dot = false;
        while let Some(d) = c {
            if d.is_ascii_digit() || d.is_ascii_uppercase() || (d == b'.' && !dot) {
                dot |= d == b'.';
                text.push(d);
                c = self.stdin.getchar();
            } else {
                break;
            }
        }
        let text = super::lexer::normalize_number(&text);
        let v = Num::parse_constant(&text, self.ibase).unwrap_or_default();
        if neg { v.negate() } else { v }
    }

    fn call(
        &mut self,
        frames: &mut Vec<Frame>,
        stack: &mut Vec<Num>,
        fi: u32,
        kinds: &[bool],
        caller: u32,
        start: u32,
    ) -> R<()> {
        let nb = name_bytes(fi);
        let total = start + 1 + nb + kinds.len() as u32 + 1;
        if !self.funcs[fi as usize].defined {
            let name = self.func_name(fi);
            return Err(self.rt_error(
                caller,
                start + 1 + nb,
                &format!("Function {name} not defined."),
            ));
        }
        let params = self.funcs[fi as usize].params.clone();
        let autos = self.funcs[fi as usize].autos.clone();
        let native = self.funcs[fi as usize].native;
        let mut frame = Frame {
            code: self.funcs[fi as usize].code.clone(),
            pc: 0,
            func: fi,
            pushed_vars: Vec::new(),
            pushed_arrays: Vec::new(),
        };
        let mut native_args: Vec<Num> = Vec::new();
        // O GNU percorre os argumentos do último pro primeiro, junto com os parâmetros.
        let n = kinds.len();
        let mut corrupt = false;
        for k in 1..=n {
            let adr = start + 1 + nb + k as u32;
            let is_array = kinds[n - k];
            let value = stack.pop().unwrap_or_default();
            if corrupt {
                // O GNU segue com a lista de parâmetros estragada e cai.
                self.unwind(&frame);
                self.out = Output::new(self.out.line_size());
                return Err(Stop::Exit(139));
            }
            let Some(p) = params.len().checked_sub(k).map(|j| params[j]) else {
                self.unwind(&frame);
                return Err(self.rt_error(caller, adr, "Parameter number mismatch"));
            };
            match (is_array, p.kind) {
                (false, Kind::Var) => {
                    if native.is_some() {
                        native_args.push(value);
                    } else {
                        self.var_stack(p.idx).push(value);
                        frame.pushed_vars.push(p.idx);
                    }
                }
                (true, Kind::Array | Kind::RefArray) => {
                    let src_idx = value.int_u64().unwrap_or(0) as u32;
                    let src = self.array(src_idx);
                    let dest = if p.kind == Kind::RefArray {
                        src
                    } else {
                        Rc::new(RefCell::new(src.borrow().clone()))
                    };
                    self.array_stack(p.idx).push(dest);
                    frame.pushed_arrays.push(p.idx);
                }
                (_, kind) => {
                    let msg = if kind.is_array() {
                        format!(
                            "Parameter type mismatch parameter {}.",
                            io::lossy(&self.arr_name_of[p.idx as usize])
                        )
                    } else {
                        "Parameter type mismatch, parameter (null).".to_string()
                    };
                    self.rt_error(caller, adr, &msg);
                    corrupt = true;
                }
            }
        }
        if corrupt || n < params.len() {
            self.unwind(&frame);
            return Err(self.rt_error(caller, total, "Parameter number mismatch"));
        }
        if let Some(lib) = native {
            native_args.reverse();
            let saved = self.cur_func;
            self.cur_func = fi;
            let v = self.lib_call(lib, &native_args);
            self.cur_func = saved;
            stack.push(v?);
            return Ok(());
        }
        for a in &autos {
            if a.kind.is_array() {
                self.array_stack(a.idx)
                    .push(Rc::new(RefCell::new(BTreeMap::new())));
                frame.pushed_arrays.push(a.idx);
            } else {
                self.var_stack(a.idx).push(Num::zero());
                frame.pushed_vars.push(a.idx);
            }
        }
        frames.push(frame);
        Ok(())
    }

    fn do_return(&mut self, frames: &mut Vec<Frame>, stack: &mut Vec<Num>) {
        let v = stack.pop().unwrap_or_default();
        if let Some(f) = frames.pop() {
            self.unwind(&f);
        }
        stack.push(v);
    }

    // ---- biblioteca matemática ----
    //
    // Os passos (e as escalas em que cada conta é feita) são os da biblioteca do GNU bc 1.07.1, que
    // é ela mesma um programa bc; cada linha abaixo é uma conta do bc com a escala corrente, então o
    // último dígito sai igual.

    fn lib_call(&mut self, lib: Lib, args: &[Num]) -> R<Num> {
        let x = args.first().cloned().unwrap_or_default();
        let r = match lib {
            Lib::E => self.lib_e(x),
            Lib::L => self.lib_l(x),
            Lib::S => self.lib_s(x),
            Lib::A => self.lib_a(x),
            Lib::C => self.lib_c(x),
            Lib::J => self.lib_j(x, args.get(1).cloned().unwrap_or_default()),
        };
        r.map_err(|_| self.out_of_memory())
    }

    fn set_scale(&mut self, v: &Num) {
        let f = self.cur_func;
        self.store(VAR_SCALE, v, f, 0);
    }

    fn sc(&self) -> Num {
        int(u64::from(self.scale))
    }

    fn lib_e(&mut self, x: Num) -> Result<Num, number::OutOfMemory> {
        let zero = Num::zero();
        let one = Num::one();
        let mut x = x;
        let mut m = false;
        if x.compare(&zero) == Ordering::Less {
            m = true;
            x = x.negate();
        }
        let z = self.scale;
        let zn = self.sc();
        let n = int(6).add(&zn, 0)?.add(&k(".44").mul(&x, self.scale)?, 0)?;
        self.set_scale(&int(u64::from(x.scale())).add(&one, 0)?);
        let mut f = Num::zero();
        while x.compare(&one) == Ordering::Greater {
            f = f.add(&one, 0)?;
            x = x.div(&int(2), self.scale)?.unwrap_or_default();
            let s = self.sc().add(&one, 0)?;
            self.set_scale(&s);
        }
        self.set_scale(&n);
        let mut v = one.add(&x, 0)?;
        let mut a = x.clone();
        let mut d = Num::one();
        let mut i = 2u64;
        loop {
            sysabi::sys::checkpoint();
            a = a.mul(&x, self.scale)?;
            d = d.mul(&int(i), self.scale)?;
            let e = a.div(&d, self.scale)?.unwrap_or_default();
            if e.compare(&zero) == Ordering::Equal {
                if f.compare(&zero) == Ordering::Greater {
                    loop {
                        let old = f.clone();
                        f = f.sub(&one, 0)?;
                        if old.is_zero() {
                            break;
                        }
                        v = v.mul(&v, self.scale)?;
                    }
                }
                self.scale = z;
                return if m {
                    Ok(one.div(&v, z)?.unwrap_or_default())
                } else {
                    Ok(v.div(&one, z)?.unwrap_or_default())
                };
            }
            v = v.add(&e, 0)?;
            i += 1;
        }
    }

    fn lib_l(&mut self, x: Num) -> Result<Num, number::OutOfMemory> {
        let zero = Num::zero();
        let one = Num::one();
        let mut x = x;
        if x.compare(&zero) != Ordering::Greater {
            let p = int(10)
                .raise(&self.sc(), self.scale)
                .map(|r| r.0)
                .unwrap_or_default();
            return Ok(one.sub(&p, 0)?.div(&one, self.scale)?.unwrap_or_default());
        }
        let z = self.scale;
        let s = int(6).add(&self.sc(), 0)?;
        self.set_scale(&s);
        let mut f = int(2);
        let two = int(2);
        let half = k(".5");
        while x.compare(&two) != Ordering::Less {
            f = f.mul(&two, self.scale)?;
            x = x.sqrt(self.scale)?.unwrap_or_default();
        }
        while x.compare(&half) != Ordering::Greater {
            f = f.mul(&two, self.scale)?;
            x = x.sqrt(self.scale)?.unwrap_or_default();
        }
        let mut n = x
            .sub(&one, 0)?
            .div(&x.add(&one, 0)?, self.scale)?
            .unwrap_or_default();
        let mut v = n.clone();
        let m = n.mul(&n, self.scale)?;
        let mut i = 3u64;
        loop {
            sysabi::sys::checkpoint();
            n = n.mul(&m, self.scale)?;
            let e = n.div(&int(i), self.scale)?.unwrap_or_default();
            if e.compare(&zero) == Ordering::Equal {
                v = f.mul(&v, self.scale)?;
                self.scale = z;
                return Ok(v.div(&one, z)?.unwrap_or_default());
            }
            v = v.add(&e, 0)?;
            i += 2;
        }
    }

    fn lib_s(&mut self, x: Num) -> Result<Num, number::OutOfMemory> {
        let zero = Num::zero();
        let one = Num::one();
        let mut x = x;
        let z = self.scale;
        let zn = self.sc();
        let s = k("1.1").mul(&zn, self.scale)?.add(&int(2), 0)?;
        self.set_scale(&s);
        let mut v = self.lib_a(one.clone())?;
        let mut m = false;
        if x.compare(&zero) == Ordering::Less {
            m = true;
            x = x.negate();
        }
        self.scale = 0;
        let n = x
            .div(&v, 0)?
            .unwrap_or_default()
            .add(&int(2), 0)?
            .div(&int(4), 0)?
            .unwrap_or_default();
        x = x.sub(&int(4).mul(&n, 0)?.mul(&v, 0)?, 0)?;
        if !n.modulo(&int(2), 0)?.unwrap_or_default().is_zero() {
            x = x.negate();
        }
        self.scale = z + 2;
        v = x.clone();
        let mut e = x.clone();
        let s = x.negate().mul(&x, self.scale)?;
        let mut i = 3u64;
        loop {
            sysabi::sys::checkpoint();
            let den = int(i).mul(&int(i - 1), self.scale)?;
            e = e.mul(&s.div(&den, self.scale)?.unwrap_or_default(), self.scale)?;
            if e.compare(&zero) == Ordering::Equal {
                self.scale = z;
                return if m {
                    Ok(v.negate().div(&one, z)?.unwrap_or_default())
                } else {
                    Ok(v.div(&one, z)?.unwrap_or_default())
                };
            }
            v = v.add(&e, 0)?;
            i += 2;
        }
    }

    fn lib_c(&mut self, x: Num) -> Result<Num, number::OutOfMemory> {
        let one = Num::one();
        let z = self.scale;
        let s = self.sc().mul(&k("1.2"), self.scale)?;
        self.set_scale(&s);
        let a1 = self.lib_a(one.clone())?;
        let arg = x.add(&a1.mul(&int(2), self.scale)?, 0)?;
        let v = self.lib_s(arg)?;
        self.scale = z;
        Ok(v.div(&one, z)?.unwrap_or_default())
    }

    fn lib_a(&mut self, x: Num) -> Result<Num, number::OutOfMemory> {
        let zero = Num::zero();
        let one = Num::one();
        let fifth = k(".2");
        let mut x = x;
        let mut m = one.clone();
        if x.compare(&zero) == Ordering::Less {
            m = Num::from_i64(-1);
            x = x.negate();
        }
        let table: [(&Num, [&str; 3]); 2] = [
            (
                &one,
                [
                    ".7853981633974483096156608",
                    ".7853981633974483096156608458198757210492",
                    ".785398163397448309615660845819875721049292349843776455243736",
                ],
            ),
            (
                &fifth,
                [
                    ".1973955598498807583700497",
                    ".1973955598498807583700497651947902934475",
                    ".197395559849880758370049765194790293447585103787852101517688",
                ],
            ),
        ];
        for (val, consts) in table {
            if x.compare(val) == Ordering::Equal {
                for (limit, c) in [25, 40, 60].into_iter().zip(consts) {
                    if self.scale <= limit {
                        return Ok(k(c).div(&m, self.scale)?.unwrap_or_default());
                    }
                }
            }
        }
        let z = self.scale;
        let mut a = Num::zero();
        if x.compare(&fifth) == Ordering::Greater {
            self.scale = z + 5;
            a = self.lib_a(fifth.clone())?;
        }
        self.scale = z + 3;
        let mut f = Num::zero();
        while x.compare(&fifth) == Ordering::Greater {
            f = f.add(&one, 0)?;
            let num = x.sub(&fifth, 0)?;
            let den = one.add(&x.mul(&fifth, self.scale)?, 0)?;
            x = num.div(&den, self.scale)?.unwrap_or_default();
        }
        let mut v = x.clone();
        let mut n = x.clone();
        let s = x.negate().mul(&x, self.scale)?;
        let mut i = 3u64;
        loop {
            sysabi::sys::checkpoint();
            n = n.mul(&s, self.scale)?;
            let e = n.div(&int(i), self.scale)?.unwrap_or_default();
            if e.compare(&zero) == Ordering::Equal {
                self.scale = z;
                let t = f.mul(&a, z)?.add(&v, 0)?;
                return Ok(t.div(&m, z)?.unwrap_or_default());
            }
            v = v.add(&e, 0)?;
            i += 2;
        }
    }

    fn lib_j(&mut self, n: Num, x: Num) -> Result<Num, number::OutOfMemory> {
        let zero = Num::zero();
        let one = Num::one();
        let z = self.scale;
        let zn = self.sc();
        self.scale = 0;
        let mut n = n.div(&one, 0)?.unwrap_or_default();
        let mut m = false;
        if n.compare(&zero) == Ordering::Less {
            n = n.negate();
            if n.modulo(&int(2), 0)?.unwrap_or_default().compare(&one) == Ordering::Equal {
                m = true;
            }
        }
        let s15 = k("1.5").mul(&zn, self.scale)?;
        self.set_scale(&s15);
        let mut f = Num::one();
        let mut i = int(2);
        while i.compare(&n) != Ordering::Greater {
            f = f.mul(&i, self.scale)?;
            i = i.add(&one, 0)?;
        }
        let s15 = k("1.5").mul(&zn, self.scale)?;
        self.set_scale(&s15);
        let xn = x.raise(&n, self.scale).map(|r| r.0).unwrap_or_default();
        let tn = int(2)
            .raise(&n, self.scale)
            .map(|r| r.0)
            .unwrap_or_default();
        f = xn
            .div(&tn, self.scale)?
            .unwrap_or_default()
            .div(&f, self.scale)?
            .unwrap_or_default();
        let mut v = Num::one();
        let mut e = Num::one();
        let s = x
            .negate()
            .mul(&x, self.scale)?
            .div(&int(4), self.scale)?
            .unwrap_or_default();
        let sc = k("1.5")
            .mul(&zn, self.scale)?
            .add(&int(f.length()), 0)?
            .sub(&int(u64::from(f.scale())), 0)?;
        self.set_scale(&sc);
        let mut i = Num::one();
        loop {
            sysabi::sys::checkpoint();
            e = e
                .mul(&s, self.scale)?
                .div(&i, self.scale)?
                .unwrap_or_default()
                .div(&n.add(&i, 0)?, self.scale)?
                .unwrap_or_default();
            if e.compare(&zero) == Ordering::Equal {
                self.scale = z;
                let fv = if m { f.negate() } else { f.clone() };
                return Ok(fv.mul(&v, z)?.div(&one, z)?.unwrap_or_default());
            }
            v = v.add(&e, 0)?;
            i = i.add(&one, 0)?;
        }
    }
}

/// Sinal (só se a parte inteira não for zero) e magnitude da parte inteira.
fn int_parts(v: &Num) -> (bool, BigUint) {
    let m = v.int_mag();
    (v.is_neg() && m != BigUint::default(), m)
}

fn big_u64(m: &BigUint) -> u64 {
    m.to_u64_digits().first().copied().unwrap_or(0)
}
