//! `dc` do GNU bc 1.07.1 (pacote dc do Debian 13, dc 1.4.1), porte de `dc/dc.c`, `eval.c`,
//! `stack.c`, `string.c` e `numeric.c`.
//!
//! A aritmética é a mesma `number.c` do bc, então reaproveita [`crate::bc::number::Num`]: soma e
//! subtração na maior escala, multiplicação, divisão, resto e potência na escala `k`, raiz quadrada em
//! `max(k, escala)`. Os números da entrada aceitam dígitos 0-9 e A-F em qualquer `ibase` (sem o teto
//! do bc), `_` marca o negativo e a fração tem tantos dígitos decimais quantos foram escritos.
//!
//! Opções: `-e`/`--expression` e `-f`/`--file` são avaliados na ordem em que aparecem, depois os
//! arquivos operandos (`-` é o stdin); sem nenhum deles, o stdin. `-h` e `-V`. Cada número impresso
//! quebra na largura de `DC_LINE_LENGTH` (70 por padrão) com `\` no fim da linha.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::rc::Rc;

use num_bigint::BigUint;
use num_traits::{ToPrimitive, Zero};
use sysabi::{Ctx, Errno, ProcAttrs, SpawnSpec, WaitOptions, WaitTarget, sys};

use crate::bc::number::{Num, NumResult, RaiseError, pow10};
use crate::bc::output::line_length_from_env;
use crate::util::getopt::{Getopt, HasArg, LongOpt};
use crate::util::io;

const LONGS: &[LongOpt] = &[
    LongOpt::new("expression", HasArg::Required, b'e' as i32),
    LongOpt::new("file", HasArg::Required, b'f' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const VERSION: &str = "dc (GNU bc 1.07.1) 1.4.1

Copyright 1994, 1997, 1998, 2000, 2001, 2003-2006, 2008, 2010, 2012-2017 Free Software Foundation, Inc.
This is free software; see the source for copying conditions.  There is NO
warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE,
to the extent permitted by law.
";

fn usage(progname: &str) -> String {
    format!(
        "Usage: {progname} [OPTION] [file ...]
  -e, --expression=EXPR    evaluate expression
  -f, --file=FILE          evaluate contents of file
  -h, --help               display this help and exit
  -V, --version            output version information and exit

Email bug reports to:  bug-dc@gnu.org .
"
    )
}

/// Maior `ibase` aceito pelo `i`.
const IBASE_MAX: i64 = 16;

/// Um valor da pilha: número ou string.
#[derive(Clone)]
enum Value {
    Num(Num),
    Str(Rc<[u8]>),
}

/// Uma entrada da pilha de um registrador: o valor (ausente quando só o array foi usado) e o array.
#[derive(Default)]
struct RegEntry {
    value: Option<Value>,
    array: BTreeMap<i64, Value>,
}

/// O que a execução de um trecho pede a quem o chamou.
enum Flow {
    Next,
    /// Encerrar este nível e mais `n - 1` macros acima (o `q` e o `Q`).
    Unwind(u64),
    /// Sair do dc.
    Exit,
}

/// De onde vem o código: bytes (macro, `-e`, arquivo) ou o stdin, que o `?` também consome.
enum Src {
    Bytes { code: Rc<[u8]>, pos: usize },
    Stdin,
}

struct Dc {
    progname: String,
    stack: Vec<Value>,
    regs: Vec<Vec<RegEntry>>,
    scale: u32,
    ibase: u32,
    obase: u64,
    line_len: i64,
    stdin: Option<Vec<u8>>,
    stdin_pos: usize,
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let progname = match argv0.rfind('/') {
        Some(p) => argv0[p + 1..].to_string(),
        None => argv0.clone(),
    };
    let rest = if argv.is_empty() { &argv[..] } else { &argv[1..] };
    let mut dc = Dc::new(progname.clone());
    let mut g = Getopt::from_env(rest, "hVe:f:", LONGS);
    let mut did_eval = false;
    while let Some(opt) = g.next_opt() {
        match opt {
            Ok(o) => match o.short() {
                Some('e') => {
                    did_eval = true;
                    let code: Rc<[u8]> = Rc::from(o.arg.unwrap_or_default());
                    if let Flow::Exit = dc.exec(&mut Src::Bytes { code, pos: 0 }, 0) {
                        return 0;
                    }
                }
                Some('f') => {
                    did_eval = true;
                    if let Some(code) = dc.try_file(&o.arg.unwrap_or_default()) {
                        return code;
                    }
                }
                Some('h') => {
                    let _ = io::stdout().write_all(usage(&progname).as_bytes());
                    return 0;
                }
                Some('V') => {
                    let _ = io::stdout().write_all(VERSION.as_bytes());
                    return 0;
                }
                _ => {}
            },
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                io::eprint(usage(&progname));
                return 1;
            }
        }
    }
    for file in g.operands() {
        did_eval = true;
        if let Some(code) = dc.try_file(&file) {
            return code;
        }
    }
    if !did_eval {
        dc.exec(&mut Src::Stdin, 0);
    }
    0
}

/// `%#o` do C: `0` sozinho pro zero, senão o octal com um `0` na frente.
fn octal(c: u8) -> String {
    if c == 0 { "0".to_string() } else { format!("0{c:o}") }
}

/// `dc_show_id`: `'a' (0141)` pra caractere gráfico, só o octal pro resto.
fn show_id(c: u8) -> String {
    if c.is_ascii_graphic() { format!("'{}' ({})", char::from(c), octal(c)) } else { octal(c) }
}

/// Dígitos decimais da parte inteira mais a escala, sem contar o zero da parte inteira vazia
/// (`n_len + n_scale - (primeiro dígito == 0)`).
fn num_len(n: &Num) -> u64 {
    let int = n.int_mag();
    let int_digits = if int.is_zero() { 0 } else { int.to_str_radix(10).len() as u64 };
    int_digits + u64::from(n.scale())
}

impl Dc {
    fn new(progname: String) -> Dc {
        let line_len = line_length_from_env(sys::getenv("DC_LINE_LENGTH").as_deref());
        Dc {
            progname,
            stack: Vec::new(),
            regs: (0..256).map(|_| Vec::new()).collect(),
            scale: 0,
            ibase: 10,
            obase: 10,
            line_len,
            stdin: None,
            stdin_pos: 0,
        }
    }

    fn err(&self, msg: impl AsRef<str>) {
        io::eprint(format!("{}: {}\n", self.progname, msg.as_ref()));
    }

    fn stack_empty(&self) {
        self.err("stack empty");
    }

    /// Conta que estourou o teto de memória: o `dc_memfail` do original.
    fn mem<T>(&self, r: NumResult<T>) -> T {
        match r {
            Ok(v) => v,
            Err(_) => {
                self.err("out of memory");
                let _ = io::flush_stdout();
                sys::exit(1);
            }
        }
    }

    /// `try_file`: avalia um arquivo (`-` é o stdin). `Some(código)` quando o dc tem que sair.
    fn try_file(&mut self, path: &[u8]) -> Option<i32> {
        if path == b"-" {
            return match self.exec(&mut Src::Stdin, 0) {
                Flow::Exit => Some(0),
                _ => None,
            };
        }
        let name = io::lossy(path);
        let mut file = match io::File::open(path) {
            Ok(f) => f,
            Err(_) => {
                self.err(format!("Could not open file {name}"));
                return Some(1);
            }
        };
        let data = match file.read_to_end_sys() {
            Ok(d) => d,
            Err(Errno::EISDIR) => {
                self.err(format!("Will not attempt to process directory {name}"));
                return Some(1);
            }
            Err(_) => {
                self.err(format!("Could not open file {name}"));
                return Some(1);
            }
        };
        drop(file);
        match self.exec(&mut Src::Bytes { code: Rc::from(data), pos: 0 }, 0) {
            Flow::Exit => Some(0),
            _ => None,
        }
    }

    fn load_stdin(&mut self) {
        if self.stdin.is_none() {
            self.stdin = Some(io::read_stdin().unwrap_or_default());
        }
    }

    fn getc(&mut self, src: &mut Src) -> Option<u8> {
        match src {
            Src::Bytes { code, pos } => {
                let c = code.get(*pos).copied();
                if c.is_some() {
                    *pos += 1;
                }
                c
            }
            Src::Stdin => {
                self.load_stdin();
                let c = self.stdin.as_ref().and_then(|s| s.get(self.stdin_pos).copied());
                if c.is_some() {
                    self.stdin_pos += 1;
                }
                c
            }
        }
    }

    fn ungetc(&mut self, src: &mut Src) {
        match src {
            Src::Bytes { pos, .. } => *pos -= 1,
            Src::Stdin => self.stdin_pos -= 1,
        }
    }

    /// Uma linha do stdin, com o `\n` (o `?`).
    fn read_stdin_line(&mut self) -> Vec<u8> {
        self.load_stdin();
        let data = self.stdin.as_deref().unwrap_or_default();
        let start = self.stdin_pos.min(data.len());
        let end = match data[start..].iter().position(|&c| c == b'\n') {
            Some(p) => start + p + 1,
            None => data.len(),
        };
        let line = data[start..end].to_vec();
        self.stdin_pos = end;
        line
    }

    fn push(&mut self, v: Value) {
        self.stack.push(v);
    }

    fn push_num(&mut self, n: Num) {
        self.stack.push(Value::Num(n));
    }

    fn pop(&mut self) -> Option<Value> {
        let v = self.stack.pop();
        if v.is_none() {
            self.stack_empty();
        }
        v
    }

    /// `dc_num2int`: a parte inteira como `long`, com o aviso do original quando não cabe.
    fn num2int(&self, n: &Num) -> i64 {
        let r = n.num2long();
        if r == 0 && !n.is_zero() {
            self.err("value overflows simple integer; punting...");
            return -1;
        }
        r
    }

    /// `dc_getnum`: lê um número na base de entrada corrente.
    fn read_number(&mut self, src: &mut Src) -> Num {
        let base = self.ibase;
        let digit = |c: Option<u8>| match c {
            Some(d @ b'0'..=b'9') => Some(u32::from(d - b'0')),
            Some(d @ b'A'..=b'F') => Some(u32::from(d - b'A') + 10),
            _ => None,
        };
        let mut c = self.getc(src);
        let mut neg = false;
        if c == Some(b'_') {
            neg = true;
            c = self.getc(src);
        }
        let mut int = BigUint::zero();
        while let Some(d) = digit(c) {
            int = int * base + d;
            c = self.getc(src);
        }
        let mut frac = BigUint::zero();
        let mut decimals: u32 = 0;
        if c == Some(b'.') {
            c = self.getc(src);
            while let Some(d) = digit(c) {
                frac = frac * base + d;
                decimals += 1;
                c = self.getc(src);
            }
        }
        if c.is_some() {
            self.ungetc(src);
        }
        let num = if decimals == 0 {
            Num::from_parts(false, int, 0)
        } else {
            let ten = self.mem(pow10(u64::from(decimals)));
            let f = frac * &ten / BigUint::from(base).pow(decimals);
            Num::from_parts(false, int * ten + f, decimals)
        };
        if neg { num.negate() } else { num }
    }

    /// Uma string entre colchetes, com aninhamento; o `[` já foi lido.
    fn read_string(&mut self, src: &mut Src) -> Rc<[u8]> {
        let mut depth = 1usize;
        let mut s = Vec::new();
        while let Some(c) = self.getc(src) {
            if c == b'[' {
                depth += 1;
            } else if c == b']' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            s.push(c);
        }
        Rc::from(s)
    }

    /// `dc_out_num`: o número na base de saída, quebrando a linha com `\`.
    fn print_num(&self, n: &Num) {
        let mut buf = Vec::new();
        let mut col: i64 = 0;
        let ll = self.line_len;
        n.write(self.obase, &mut |ch| {
            col += 1;
            if ll != 0 && col == ll - 1 {
                buf.extend_from_slice(b"\\\n");
                col = 1;
            }
            buf.push(ch);
        });
        let _ = io::stdout().write_all(&buf);
    }

    fn print_value(&self, v: &Value, newline: bool) {
        match v {
            Value::Num(n) => self.print_num(n),
            Value::Str(s) => {
                let _ = io::stdout().write_all(s);
            }
        }
        if newline {
            let _ = io::stdout().write_all(b"\n");
        }
    }

    /// `dc_dump_num`: a parte inteira de |x| como bytes em base 256.
    fn dump_num(&self, n: &Num) {
        let mut v = n.int_mag();
        let base = BigUint::from(256u32);
        let mut bytes = Vec::new();
        while !v.is_zero() {
            bytes.push((&v % &base).to_u8().unwrap_or(0));
            v /= &base;
        }
        bytes.reverse();
        let _ = io::stdout().write_all(&bytes);
    }

    /// Os dois números do topo (segundo, topo), sem tirar da pilha.
    fn top_two(&self) -> Option<(Num, Num)> {
        let n = self.stack.len();
        if n < 2 {
            self.stack_empty();
            return None;
        }
        match (&self.stack[n - 2], &self.stack[n - 1]) {
            (Value::Num(a), Value::Num(b)) => Some((a.clone(), b.clone())),
            _ => {
                self.err("non-numeric value");
                None
            }
        }
    }

    /// `dc_binop`: se a operação falha a pilha fica como estava.
    fn binop(&mut self, op: u8) {
        let Some((a, b)) = self.top_two() else { return };
        let k = self.scale;
        let r = match op {
            b'+' => Some(self.mem(a.add(&b, 0))),
            b'-' => Some(self.mem(a.sub(&b, 0))),
            b'*' => Some(self.mem(a.mul(&b, k))),
            b'/' => {
                let r = self.mem(a.div(&b, k));
                if r.is_none() {
                    self.err("divide by zero");
                }
                r
            }
            b'%' => {
                let r = self.mem(a.modulo(&b, k));
                if r.is_none() {
                    self.err("remainder by zero");
                }
                r
            }
            _ => match a.raise(&b, k) {
                Ok((r, warn)) => {
                    if warn {
                        self.err("non-zero scale in exponent");
                    }
                    Some(r)
                }
                Err(RaiseError::TooLarge) => {
                    self.err("exponent too large in raise");
                    None
                }
                Err(RaiseError::DivideByZero) => {
                    self.err("divide by zero");
                    None
                }
                Err(RaiseError::OutOfMemory) => {
                    self.mem::<()>(Err(crate::bc::number::OutOfMemory));
                    None
                }
            },
        };
        if let Some(r) = r {
            self.stack.truncate(self.stack.len() - 2);
            self.push_num(r);
        }
    }

    /// `~`: quociente e resto.
    fn divrem(&mut self) {
        let Some((a, b)) = self.top_two() else { return };
        let k = self.scale;
        let Some(q) = self.mem(a.div(&b, k)) else {
            self.err("divide by zero");
            return;
        };
        let r = self.mem(a.modulo(&b, k)).unwrap_or_default();
        self.stack.truncate(self.stack.len() - 2);
        self.push_num(q);
        self.push_num(r);
    }

    /// `|`: `base^expo mod m` como o `bc_raisemod`.
    fn modexp(&mut self) {
        let n = self.stack.len();
        if n < 3 {
            self.stack_empty();
            return;
        }
        let (base, expo, m) = match (&self.stack[n - 3], &self.stack[n - 2], &self.stack[n - 1]) {
            (Value::Num(a), Value::Num(b), Value::Num(c)) => (a.clone(), b.clone(), c.clone()),
            _ => {
                self.err("non-numeric value");
                return;
            }
        };
        if m.is_zero() {
            self.err("remainder by zero");
            return;
        }
        if expo.is_neg() && !expo.is_zero() {
            self.err("negative exponent");
            return;
        }
        if base.scale() != 0 {
            self.err("non-zero scale in base");
        }
        if expo.scale() != 0 {
            self.err("non-zero scale in exponent");
        }
        if m.scale() != 0 {
            self.err("non-zero scale in modulus");
        }
        let k = self.scale;
        let rscale = k.max(base.scale());
        let mut e = expo.int_mag();
        let mut power = base;
        let mut temp = Num::one();
        let two = BigUint::from(2u32);
        while !e.is_zero() {
            let odd = !(&e % &two).is_zero();
            e /= &two;
            if odd {
                temp = self.mem(temp.mul(&power, rscale));
                temp = self.mem(temp.modulo(&m, k)).unwrap_or_default();
            }
            power = self.mem(power.mul(&power, rscale));
            power = self.mem(power.modulo(&m, k)).unwrap_or_default();
            sys::checkpoint();
        }
        self.stack.truncate(n - 3);
        self.push_num(temp);
    }

    fn sqrt(&mut self) {
        let Some(top) = self.stack.last() else {
            self.stack_empty();
            return;
        };
        let Value::Num(x) = top else {
            self.err("square root of nonnumeric attempted");
            return;
        };
        let x = x.clone();
        match self.mem(x.sqrt(self.scale)) {
            Some(r) => {
                self.stack.pop();
                self.push_num(r);
            }
            None => self.err("square root of negative number"),
        }
    }

    /// `R`: gira os `n` do topo.
    fn rotate(&mut self, n: i64) {
        let count = n.unsigned_abs() as usize;
        if count < 2 || count > self.stack.len() {
            return;
        }
        let len = self.stack.len();
        let seg = &mut self.stack[len - count..];
        if n > 0 {
            seg.rotate_left(1);
        } else {
            seg.rotate_right(1);
        }
    }

    /// Lê o nome do registrador depois de um comando.
    fn reg_name(&mut self, src: &mut Src) -> Option<usize> {
        let r = self.getc(src);
        if r.is_none() {
            self.err("unexpected EOF");
        }
        r.map(usize::from)
    }

    fn reg_top(&mut self, r: usize) -> &mut RegEntry {
        if self.regs[r].is_empty() {
            self.regs[r].push(RegEntry::default());
        }
        let st = &mut self.regs[r];
        let last = st.len() - 1;
        &mut st[last]
    }

    /// Executa uma string como macro, um nível abaixo de `depth`.
    fn call_macro(&mut self, code: Rc<[u8]>, depth: u64) -> Flow {
        sys::checkpoint();
        match self.exec(&mut Src::Bytes { code, pos: 0 }, depth + 1) {
            Flow::Next => Flow::Next,
            Flow::Exit => Flow::Exit,
            Flow::Unwind(k) => {
                if k > 1 && depth > 0 {
                    Flow::Unwind(k - 1)
                } else {
                    Flow::Next
                }
            }
        }
    }

    /// O conteúdo do registrador: string vira macro, número vai pra pilha.
    fn exec_register(&mut self, r: usize, depth: u64) -> Flow {
        let v = self.regs[r].last().and_then(|e| e.value.clone());
        match v {
            None => {
                self.err(format!("register {} is empty", show_id(r as u8)));
                Flow::Next
            }
            Some(Value::Str(s)) => self.call_macro(s, depth),
            Some(v) => {
                self.push(v);
                Flow::Next
            }
        }
    }

    /// `<r`, `>r`, `=r` e as negações: tira os dois do topo e executa `r` se a condição vale entre o
    /// topo original e o segundo.
    fn compare(&mut self, src: &mut Src, depth: u64, cond: fn(Ordering) -> bool) -> Flow {
        let Some(r) = self.reg_name(src) else { return Flow::Next };
        if self.stack.len() < 2 {
            self.stack_empty();
            return Flow::Next;
        }
        let top = self.stack.pop();
        let second = self.stack.pop();
        let (Some(Value::Num(a)), Some(Value::Num(b))) = (top, second) else {
            self.err("non-numeric value");
            return Flow::Next;
        };
        if cond(a.compare(&b)) { self.exec_register(r, depth) } else { Flow::Next }
    }

    /// `!comando`: o resto da linha vai pro `sh -c`.
    fn shell(&mut self, src: &mut Src) {
        let mut cmd = Vec::new();
        while let Some(c) = self.getc(src) {
            if c == b'\n' {
                break;
            }
            cmd.push(c);
        }
        let _ = io::flush_stdout();
        let s = sys::current();
        let spec = SpawnSpec {
            path: b"/bin/sh".to_vec(),
            argv: vec![b"sh".to_vec(), b"-c".to_vec(), cmd],
            attrs: ProcAttrs::default(),
        };
        if let Ok(pid) = s.spawn(spec) {
            let _ = s.wait4(WaitTarget::Pid(pid), WaitOptions::empty());
        }
    }

    /// O laço do `dc_evalstr`/`dc_evalfile`.
    fn exec(&mut self, src: &mut Src, depth: u64) -> Flow {
        while let Some(c) = self.getc(src) {
            match c {
                b' ' | b'\t' | b'\n' => {}
                b'_' | b'0'..=b'9' | b'A'..=b'F' | b'.' => {
                    self.ungetc(src);
                    let n = self.read_number(src);
                    self.push_num(n);
                }
                b'[' => {
                    let s = self.read_string(src);
                    self.push(Value::Str(s));
                }
                b'#' => {
                    while let Some(c) = self.getc(src) {
                        if c == b'\n' {
                            break;
                        }
                    }
                }
                b'p' => match self.stack.last().cloned() {
                    Some(v) => self.print_value(&v, true),
                    None => self.stack_empty(),
                },
                b'n' => {
                    if let Some(v) = self.pop() {
                        self.print_value(&v, false);
                    }
                }
                b'P' => match self.pop() {
                    Some(Value::Str(s)) => {
                        let _ = io::stdout().write_all(&s);
                    }
                    Some(Value::Num(n)) => self.dump_num(&n),
                    None => {}
                },
                b'f' => {
                    let all: Vec<Value> = self.stack.iter().rev().cloned().collect();
                    for v in &all {
                        self.print_value(v, true);
                    }
                }
                b'+' | b'-' | b'*' | b'/' | b'%' | b'^' => self.binop(c),
                b'~' => self.divrem(),
                b'|' => self.modexp(),
                b'v' => self.sqrt(),
                b'c' => self.stack.clear(),
                b'd' => match self.stack.last().cloned() {
                    Some(v) => self.push(v),
                    None => self.stack_empty(),
                },
                b'r' => {
                    let n = self.stack.len();
                    if n < 2 {
                        self.stack_empty();
                    } else {
                        self.stack.swap(n - 1, n - 2);
                    }
                }
                b'R' => match self.pop() {
                    Some(Value::Num(n)) => {
                        let k = self.num2int(&n);
                        self.rotate(k);
                    }
                    Some(_) => self.err("non-numeric value"),
                    None => {}
                },
                b'z' => {
                    let n = self.stack.len() as u64;
                    self.push_num(Num::from_u64(n));
                }
                b'Z' => match self.pop() {
                    Some(Value::Num(n)) => self.push_num(Num::from_u64(num_len(&n))),
                    Some(Value::Str(s)) => self.push_num(Num::from_u64(s.len() as u64)),
                    None => {}
                },
                b'X' => match self.pop() {
                    Some(Value::Num(n)) => self.push_num(Num::from_u64(u64::from(n.scale()))),
                    Some(Value::Str(_)) => self.push_num(Num::zero()),
                    None => {}
                },
                b'k' => {
                    if let Some(v) = self.pop() {
                        if let Value::Num(n) = &v {
                            let t = self.num2int(n);
                            if t >= 0 {
                                self.scale = t.min(i64::from(u32::MAX)) as u32;
                                continue;
                            }
                        }
                        self.err("scale must be a nonnegative number");
                    }
                }
                b'i' => {
                    if let Some(v) = self.pop() {
                        if let Value::Num(n) = &v {
                            let t = self.num2int(n);
                            if (2..=IBASE_MAX).contains(&t) {
                                self.ibase = t as u32;
                                continue;
                            }
                        }
                        self.err(format!("input base must be a number between 2 and {IBASE_MAX} (inclusive)"));
                    }
                }
                b'o' => {
                    if let Some(v) = self.pop() {
                        if let Value::Num(n) = &v {
                            let t = self.num2int(n);
                            if t > 1 {
                                self.obase = t as u64;
                                continue;
                            }
                        }
                        self.err("output base must be a number greater than 1");
                    }
                }
                b'K' => self.push_num(Num::from_u64(u64::from(self.scale))),
                b'I' => self.push_num(Num::from_u64(u64::from(self.ibase))),
                b'O' => self.push_num(Num::from_u64(self.obase)),
                b's' => {
                    let Some(r) = self.reg_name(src) else { return Flow::Next };
                    if let Some(v) = self.pop() {
                        self.reg_top(r).value = Some(v);
                    }
                }
                b'l' => {
                    let Some(r) = self.reg_name(src) else { return Flow::Next };
                    match self.regs[r].last().and_then(|e| e.value.clone()) {
                        Some(v) => self.push(v),
                        None => self.err(format!("register {} is empty", show_id(r as u8))),
                    }
                }
                b'S' => {
                    let Some(r) = self.reg_name(src) else { return Flow::Next };
                    if let Some(v) = self.pop() {
                        self.regs[r].push(RegEntry { value: Some(v), array: BTreeMap::new() });
                    }
                }
                b'L' => {
                    let Some(r) = self.reg_name(src) else { return Flow::Next };
                    match self.regs[r].pop() {
                        Some(RegEntry { value: Some(v), .. }) => self.push(v),
                        _ => self.err(format!("stack register {} is empty", show_id(r as u8))),
                    }
                }
                b':' => {
                    let Some(r) = self.reg_name(src) else { return Flow::Next };
                    let Some(idx) = self.pop() else { continue };
                    let idx = match idx {
                        Value::Num(n) => self.num2int(&n),
                        Value::Str(_) => -1,
                    };
                    let Some(v) = self.pop() else { continue };
                    if idx < 0 {
                        self.err("array index must be a nonnegative integer");
                    } else {
                        self.reg_top(r).array.insert(idx, v);
                    }
                }
                b';' => {
                    let Some(r) = self.reg_name(src) else { return Flow::Next };
                    let Some(idx) = self.pop() else { continue };
                    let idx = match idx {
                        Value::Num(n) => self.num2int(&n),
                        Value::Str(_) => -1,
                    };
                    if idx < 0 {
                        self.err("array index must be a nonnegative integer");
                    } else {
                        let v = self.regs[r].last().and_then(|e| e.array.get(&idx).cloned());
                        self.push(v.unwrap_or(Value::Num(Num::zero())));
                    }
                }
                b'x' => match self.pop() {
                    Some(Value::Str(s)) => match self.call_macro(s, depth) {
                        Flow::Next => {}
                        f => return f,
                    },
                    Some(v) => self.push(v),
                    None => {}
                },
                b'<' => match self.compare(src, depth, |o| o == Ordering::Less) {
                    Flow::Next => {}
                    f => return f,
                },
                b'>' => match self.compare(src, depth, |o| o == Ordering::Greater) {
                    Flow::Next => {}
                    f => return f,
                },
                b'=' => match self.compare(src, depth, |o| o == Ordering::Equal) {
                    Flow::Next => {}
                    f => return f,
                },
                b'!' => {
                    let next = self.getc(src);
                    let flow = match next {
                        Some(b'<') => self.compare(src, depth, |o| o != Ordering::Less),
                        Some(b'>') => self.compare(src, depth, |o| o != Ordering::Greater),
                        Some(b'=') => self.compare(src, depth, |o| o != Ordering::Equal),
                        Some(_) => {
                            self.ungetc(src);
                            self.shell(src);
                            Flow::Next
                        }
                        None => {
                            self.shell(src);
                            Flow::Next
                        }
                    };
                    if !matches!(flow, Flow::Next) {
                        return flow;
                    }
                }
                b'?' => {
                    let line = self.read_stdin_line();
                    match self.exec(&mut Src::Bytes { code: Rc::from(line), pos: 0 }, depth) {
                        Flow::Next => {}
                        f => return f,
                    }
                }
                b'a' => match self.pop() {
                    Some(Value::Num(n)) => {
                        let t = self.num2int(&n);
                        self.push(Value::Str(Rc::from(vec![(t & 0xff) as u8])));
                    }
                    Some(Value::Str(s)) => {
                        let first: Vec<u8> = s.iter().take(1).copied().collect();
                        self.push(Value::Str(Rc::from(first)));
                    }
                    None => {}
                },
                b'q' => {
                    return if depth <= 1 { Flow::Exit } else { Flow::Unwind(2) };
                }
                b'Q' => {
                    if let Some(v) = self.pop() {
                        if let Value::Num(n) = &v {
                            let t = self.num2int(n);
                            if t > 0 {
                                if depth == 0 {
                                    continue;
                                }
                                return Flow::Unwind(t as u64);
                            }
                        }
                        self.err("Q command requires a number >= 1");
                    }
                }
                _ => self.err(format!("{} unimplemented", show_id(c))),
            }
        }
        Flow::Next
    }
}
