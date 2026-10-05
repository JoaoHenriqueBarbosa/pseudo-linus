//! Funções embutidas do gawk 5.2.1.

use std::rc::Rc;

use sysabi::{Clock, Fd, FdAction};

use crate::array::Subscript;
use crate::ast::{Builtin, Expr, LValue, Var};
use crate::interp::{ArrRef, Cell, Interp, R, SplitMode, char_len};
use crate::parser::sv;
use crate::regex::Regex;
use crate::value::*;

// ---------------------------------------------------------------------- rand

/// O gerador do `rand()` do gawk 5.2.1, reconstruído a partir da sequência que ele produz:
///
/// - um `random()` aditivo de grau 63 e separação 1 (o tipo 4 do BSD), semeado com o gerador de
///   Park e Miller (`16807 * x mod (2^31 - 1)`, com 0 trocado por 123459876 dentro do passo);
/// - por cima, uma tabela de embaralhamento de 512 posições (Bays e Durham): o índice é o valor anterior
///   `& 511`, a tabela é preenchida com os primeiros 512 valores logo depois da semente, e os 630
///   descartes da semeadura já passam pela tabela;
/// - cada `rand()` usa dois sorteios: `0.5 + ((d1/2^31 + d2)/2^31) - 0.5`.
///
/// Conferido contra o gawk do host em várias sementes (0, 1, 2, 3, -1, 123456789) e 80 sorteios seguidos.
#[derive(Clone)]
pub struct Random {
    state: [u32; 63],
    f: usize,
    r: usize,
    table: Vec<u32>,
    last: u32,
}

impl Default for Random {
    fn default() -> Self {
        Self::new()
    }
}

impl Random {
    pub fn new() -> Random {
        let mut r = Random { state: [0; 63], f: 1, r: 0, table: vec![0; 512], last: 0 };
        r.seed(1);
        r
    }

    fn good_rand(x: i32) -> i32 {
        let mut x = if x == 0 { 123_459_876 } else { x };
        let hi = x / 127_773;
        let lo = x % 127_773;
        x = 16_807i32.wrapping_mul(lo).wrapping_sub(2_836i32.wrapping_mul(hi));
        if x < 0 {
            x = x.wrapping_add(0x7fff_ffff);
        }
        x
    }

    pub fn seed(&mut self, seed: u32) {
        self.state[0] = seed;
        for i in 1..63 {
            self.state[i] = Self::good_rand(self.state[i - 1] as i32) as u32;
        }
        self.f = 1;
        self.r = 0;
        for i in 0..512 {
            self.table[i] = self.raw();
        }
        self.last = self.raw();
        for _ in 0..630 {
            self.draw();
        }
    }

    /// O `random()` aditivo, sem a tabela.
    fn raw(&mut self) -> u32 {
        self.state[self.f] = self.state[self.f].wrapping_add(self.state[self.r]);
        let i = (self.state[self.f] >> 1) & 0x7fff_ffff;
        self.f = (self.f + 1) % 63;
        self.r = (self.r + 1) % 63;
        i
    }

    /// Um sorteio, passando pela tabela de embaralhamento.
    pub fn draw(&mut self) -> u32 {
        let j = (self.last & 511) as usize;
        self.last = self.table[j];
        self.table[j] = self.raw();
        self.last
    }

    pub fn rand(&mut self) -> f64 {
        const DIV: f64 = 2147483648.0;
        loop {
            let d1 = self.draw() as f64;
            let d2 = self.draw() as f64;
            let mut t = 0.5 + ((d1 / DIV + d2) / DIV);
            t -= 0.5;
            if t != 1.0 {
                return t;
            }
        }
    }
}

// ---------------------------------------------------------------------- texto UTF-8

/// Deslocamentos em bytes do início de cada caractere (byte inválido conta como um caractere).
pub fn char_starts(s: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        out.push(i);
        i += utf8_char_len(&s[i..]);
    }
    out
}

/// Comprimento de um caractere UTF-8 válido no início de `s`; 1 para byte inválido.
pub fn utf8_char_len(s: &[u8]) -> usize {
    let n = char_len(s);
    if n <= 1 {
        return 1;
    }
    if std::str::from_utf8(&s[..n]).is_ok() { n } else { 1 }
}

pub fn char_count(s: &[u8]) -> usize {
    if s.is_ascii() {
        return s.len();
    }
    let mut i = 0;
    let mut n = 0;
    while i < s.len() {
        i += utf8_char_len(&s[i..]);
        n += 1;
    }
    n
}

/// Número de caracteres antes da posição em bytes `pos`.
pub fn chars_before(s: &[u8], pos: usize) -> usize {
    char_count(&s[..pos.min(s.len())])
}

fn map_case(s: &[u8], upper: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c < 0x80 {
            out.push(if upper { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() });
            i += 1;
            continue;
        }
        let n = utf8_char_len(&s[i..]);
        match std::str::from_utf8(&s[i..i + n]).ok().and_then(|t| t.chars().next()) {
            Some(ch) if n > 1 => {
                let mut mapped: Vec<char> = if upper { ch.to_uppercase().collect() } else { ch.to_lowercase().collect() };
                if mapped.len() != 1 {
                    mapped = vec![ch];
                }
                let mut buf = [0u8; 4];
                out.extend_from_slice(mapped[0].encode_utf8(&mut buf).as_bytes());
            }
            _ => out.extend_from_slice(&s[i..i + n]),
        }
        i += n;
    }
    out
}

pub fn lower_bytes(s: &[u8]) -> Vec<u8> {
    map_case(s, false)
}

pub fn upper_bytes(s: &[u8]) -> Vec<u8> {
    map_case(s, true)
}

fn fmt_g(x: f64) -> String {
    String::from_utf8_lossy(&crate::format::num_to_str(x, b"%g")).into_owned()
}

// ---------------------------------------------------------------------- chamada

impl<'p> Interp<'p> {
    pub(crate) fn call_builtin(&mut self, b: Builtin, args: &[Expr]) -> R<Value> {
        use Builtin::*;
        match b {
            Length => self.bi_length(args),
            Substr => {
                let s = self.eval_str(&args[0])?;
                let m = self.eval_num(&args[1])?;
                let n = match args.get(2) {
                    Some(e) => Some(self.eval_num(e)?),
                    None => None,
                };
                Ok(Value::Str(Rc::from(substr(&s, m, n))))
            }
            Index => {
                let s = self.eval_str(&args[0])?;
                let t = match &args[1] {
                    Expr::Regex(_) => return Err(self.fatal("index: regexp constant as second argument is not allowed")),
                    e => self.eval_str(e)?,
                };
                Ok(Value::Num(self.index_of(&s, &t) as f64))
            }
            Split => self.bi_split(args, false),
            Patsplit => self.bi_split(args, true),
            Sub | Gsub => self.bi_sub(args, b == Gsub),
            Gensub => self.bi_gensub(args),
            Match => self.bi_match(args),
            Sprintf => {
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(self.eval(a)?);
                }
                Ok(Value::Str(Rc::from(self.sprintf(&vals)?)))
            }
            Sin => Ok(Value::Num(self.eval_num(&args[0])?.sin())),
            Cos => Ok(Value::Num(self.eval_num(&args[0])?.cos())),
            Atan2 => {
                let y = self.eval_num(&args[0])?;
                let x = self.eval_num(&args[1])?;
                Ok(Value::Num(y.atan2(x)))
            }
            Exp => {
                let x = self.eval_num(&args[0])?;
                let r = x.exp();
                if r.is_infinite() && x.is_finite() {
                    self.warning(format!("exp: argument {} is out of range", fmt_g(x)));
                }
                Ok(Value::Num(r))
            }
            Log => {
                let x = self.eval_num(&args[0])?;
                if x < 0.0 {
                    self.warning(format!("log: received negative argument {}", fmt_g(x)));
                    return Ok(Value::Num(nan(true)));
                }
                Ok(Value::Num(x.ln()))
            }
            Sqrt => {
                let x = self.eval_num(&args[0])?;
                if x < 0.0 {
                    self.warning(format!("sqrt: received negative argument {}", fmt_g(x)));
                    return Ok(Value::Num(nan(true)));
                }
                Ok(Value::Num(x.sqrt()))
            }
            Int => Ok(Value::Num(self.eval_num(&args[0])?.trunc())),
            Rand => Ok(Value::Num(self.rand.rand())),
            Srand => {
                let prev = self.seed;
                let s = match args.first() {
                    Some(e) => self.eval_num(e)?,
                    None => self.sys.clock_gettime(Clock::Realtime).map(|t| t.sec as f64).unwrap_or(0.0),
                };
                // O gawk guarda a semente truncada (`long`) e semeia com ela como `unsigned`.
                let trunc = if s.is_finite() { s as i64 } else { 0 };
                self.seed = trunc as f64;
                self.rand.seed(trunc as u32);
                Ok(Value::Num(prev))
            }
            Tolower => {
                let s = self.eval_str(&args[0])?;
                Ok(Value::Str(Rc::from(lower_bytes(&s))))
            }
            Toupper => {
                let s = self.eval_str(&args[0])?;
                Ok(Value::Str(Rc::from(upper_bytes(&s))))
            }
            System => {
                let cmd = self.eval_str(&args[0])?;
                if self.cfg.sandbox {
                    return Err(self.fatal("'system' function not allowed in sandbox mode"));
                }
                self.flush_all()?;
                let st = match crate::io::spawn_shell(&self.sys, &cmd, Vec::<FdAction>::new()) {
                    Ok(pid) => crate::io::wait_pid(&self.sys, pid).map(crate::io::status_value).unwrap_or(-1.0),
                    Err(_) => 127.0,
                };
                Ok(Value::Num(st))
            }
            Close => {
                let name = self.eval_str(&args[0])?;
                let how = match args.get(1) {
                    Some(e) => Some(self.eval_str(e)?.to_vec()),
                    None => None,
                };
                if let Some(h) = &how {
                    let hl = lower_bytes(h);
                    if hl != b"to" && hl != b"from" {
                        return Err(self.fatal("close: second argument must be `to' or `from'"));
                    }
                }
                let r = self.close_named(&name, how.as_deref())?;
                Ok(Value::Num(r))
            }
            Fflush => {
                match args.first() {
                    None => {
                        self.flush_all()?;
                        Ok(Value::Num(0.0))
                    }
                    Some(e) => {
                        let name = self.eval_str(e)?;
                        if name.is_empty() {
                            self.flush_all()?;
                            return Ok(Value::Num(0.0));
                        }
                        if &*name == b"/dev/stdout" {
                            self.flush_stdout()?;
                            return Ok(Value::Num(0.0));
                        }
                        if &*name == b"/dev/stderr" {
                            return Ok(Value::Num(0.0));
                        }
                        match self.outputs.iter().position(|o| o.name == *name) {
                            Some(i) => {
                                self.flush_stream(i)?;
                                Ok(Value::Num(0.0))
                            }
                            None => {
                                let n = String::from_utf8_lossy(&name).into_owned();
                                self.warning(format!("fflush: `{n}' is not an open file, pipe or co-process"));
                                Ok(Value::Num(-1.0))
                            }
                        }
                    }
                }
            }
            Systime => Ok(Value::Num(self.now() as f64)),
            Strftime => self.bi_strftime(args),
            Mktime => {
                let spec = self.eval_str(&args[0])?;
                let utc = match args.get(1) {
                    Some(e) => self.eval(e)?.truthy(),
                    None => false,
                };
                let tz = if utc { crate::time::TimeZone::utc() } else { self.local_tz() };
                Ok(Value::Num(crate::time::mktime(&spec, &tz) as f64))
            }
            Asort | Asorti => crate::sort::asort(self, args, b == Asorti),
            Isarray => {
                let r = match &args[0] {
                    Expr::Var(v) => matches!(self.var_cell(*v), Cell::Arr(_)),
                    Expr::Index(v, groups) => matches!(self.elem_cell(*v, groups)?, Some(Cell::Arr(_))),
                    e => {
                        self.eval(e)?;
                        false
                    }
                };
                Ok(Value::Num(r as i32 as f64))
            }
            Typeof => self.bi_typeof(args),
            Strtonum => {
                let v = self.eval(&args[0])?;
                Ok(Value::Num(match v {
                    Value::Num(n) => n,
                    other => strtonum(&self.to_str(&other)),
                }))
            }
            And | Or | Xor => {
                let mut acc: Option<u64> = None;
                for (i, a) in args.iter().enumerate() {
                    let x = self.eval_num(a)?;
                    if x < 0.0 {
                        let name = b.name();
                        return Err(self.fatal(format!("{name}: argument {} negative value {} is not allowed", i + 1, fmt_g(x))));
                    }
                    let u = to_u64(x);
                    acc = Some(match (acc, b) {
                        (None, _) => u,
                        (Some(a), And) => a & u,
                        (Some(a), Or) => a | u,
                        (Some(a), _) => a ^ u,
                    });
                }
                Ok(Value::Num(acc.unwrap_or(0) as f64))
            }
            Lshift | Rshift => {
                let x = self.eval_num(&args[0])?;
                let n = self.eval_num(&args[1])?;
                if x < 0.0 || n < 0.0 {
                    let name = b.name();
                    return Err(self.fatal(format!("{name}({x:.6}, {n:.6}): negative values are not allowed")));
                }
                let u = to_u64(x);
                let n = to_u64(n);
                let r = if b == Lshift { if n >= 64 { 0 } else { u << n } } else if n >= 64 { 0 } else { u >> n };
                Ok(Value::Num(r as f64))
            }
            Compl => {
                let x = self.eval_num(&args[0])?;
                if x < 0.0 {
                    return Err(self.fatal(format!("compl({x:.6}): negative value is not allowed")));
                }
                let u = to_u64(x);
                Ok(Value::Num((!u & ((1u64 << 53) - 1)) as f64))
            }
            Bindtextdomain => {
                let s = self.eval_str(&args[0])?;
                if let Some(e) = args.get(1) {
                    self.eval(e)?;
                }
                Ok(Value::Str(s))
            }
            Dcgettext => {
                let s = self.eval_str(&args[0])?;
                for e in &args[1..] {
                    self.eval(e)?;
                }
                Ok(Value::Str(s))
            }
            Dcngettext => {
                let s1 = self.eval_str(&args[0])?;
                let s2 = self.eval_str(&args[1])?;
                let n = self.eval_num(&args[2])?;
                for e in &args[3..] {
                    self.eval(e)?;
                }
                Ok(Value::Str(if n == 1.0 { s1 } else { s2 }))
            }
            Mkbool => {
                let v = self.eval(&args[0])?;
                Ok(Value::Bool(v.truthy()))
            }
        }
    }

    pub(crate) fn eval_str(&mut self, e: &Expr) -> R<Str> {
        let v = self.eval(e)?;
        Ok(self.to_str(&v))
    }

    pub(crate) fn eval_num(&mut self, e: &Expr) -> R<f64> {
        let v = self.eval(e)?;
        Ok(self.to_num(&v))
    }

    pub(crate) fn now(&self) -> i64 {
        self.sys.clock_gettime(Clock::Realtime).map(|t| t.sec).unwrap_or(0)
    }

    pub(crate) fn local_tz(&self) -> crate::time::TimeZone {
        let tz = self.sys.getenv(b"TZ");
        let sys = self.sys.clone();
        let mut read = |path: &[u8]| -> Option<Vec<u8>> {
            let fd = sys.openat(Fd::CWD, path, sysabi::OFlags::RDONLY | sysabi::OFlags::CLOEXEC, 0).ok()?;
            let mut out = Vec::new();
            let mut buf = vec![0u8; 65536];
            loop {
                match sys.read(fd, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => out.extend_from_slice(&buf[..n]),
                    Err(_) => {
                        let _ = sys.close(fd);
                        return None;
                    }
                }
            }
            let _ = sys.close(fd);
            Some(out)
        };
        crate::time::TimeZone::resolve(tz.as_deref(), &mut read)
    }

    fn bi_strftime(&mut self, args: &[Expr]) -> R<Value> {
        let fmt = match args.first() {
            Some(e) => self.eval_str(e)?.to_vec(),
            None => match &self.globals[sv::PROCINFO as usize] {
                Cell::Arr(a) => match a.borrow().get(&Subscript::from_bytes(Rc::from(&b"strftime"[..]))) {
                    Some(Cell::Val(v)) => self.to_str(v).to_vec(),
                    _ => b"%a %b %e %H:%M:%S %Z %Y".to_vec(),
                },
                _ => b"%a %b %e %H:%M:%S %Z %Y".to_vec(),
            },
        };
        let t = match args.get(1) {
            Some(e) => {
                let x = self.eval_num(e)?;
                if (x < 0.0 || !x.is_finite())
                    && x < 0.0 {
                        self.warning("strftime: second argument less than 0 or too big for time_t");
                        return Ok(Value::Str(empty_str()));
                    }
                x as i64
            }
            None => self.now(),
        };
        let utc = match args.get(2) {
            Some(e) => self.eval(e)?.truthy(),
            None => false,
        };
        let tz = if utc { crate::time::TimeZone::utc() } else { self.local_tz() };
        Ok(Value::Str(Rc::from(crate::time::strftime(&fmt, t, &tz))))
    }

    fn var_cell(&self, v: Var) -> Cell {
        let s = match v {
            Var::Global(i) => crate::interp::Slot::Global(i),
            Var::Local(i) => crate::interp::Slot::Local(self.frame_base() + i as usize),
        };
        let mut c = self.cell(s).clone();
        let mut guard = 0;
        while let Cell::Ref(t) = c {
            c = self.target_cell(&t);
            guard += 1;
            if guard > 10_000 {
                return Cell::Uninit;
            }
        }
        c
    }

    /// Célula de um elemento sem criá-lo.
    fn elem_cell(&mut self, v: Var, groups: &[Vec<Expr>]) -> R<Option<Cell>> {
        let (last, path) = groups.split_last().expect("grupos");
        let a = self.array_at(v, path)?;
        let k = self.subscript(last)?;
        let c = a.borrow().get(&k).cloned();
        Ok(c)
    }

    fn bi_length(&mut self, args: &[Expr]) -> R<Value> {
        let Some(arg) = args.first() else {
            let rec = self.get_record_value();
            return Ok(Value::Num(char_count(&rec) as f64));
        };
        match arg {
            Expr::Var(v) => {
                if let Var::Global(i) = v
                    && *i < sv::COUNT && !matches!(*i, sv::ENVIRON | sv::ARGV | sv::PROCINFO | sv::SYMTAB | sv::FUNCTAB) {
                        let x = self.read_var(*v)?;
                        let s = self.to_str(&x);
                        return Ok(Value::Num(char_count(&s) as f64));
                    }
                match self.var_cell(*v) {
                    Cell::Arr(a) => Ok(Value::Num(a.borrow().len() as f64)),
                    Cell::Uninit => {
                        // O gawk trata a variável não tipada como escalar daqui em diante.
                        self.write_var(*v, Value::Uninit)?;
                        Ok(Value::Num(0.0))
                    }
                    Cell::Val(x) => {
                        let s = self.to_str(&x);
                        Ok(Value::Num(char_count(&s) as f64))
                    }
                    Cell::Ref(_) => Ok(Value::Num(0.0)),
                }
            }
            Expr::Index(v, groups) => {
                if let Some(Cell::Arr(a)) = self.elem_cell(*v, groups)? {
                    return Ok(Value::Num(a.borrow().len() as f64));
                }
                let x = self.eval(arg)?;
                let s = self.to_str(&x);
                Ok(Value::Num(char_count(&s) as f64))
            }
            e => {
                let x = self.eval(e)?;
                let s = self.to_str(&x);
                Ok(Value::Num(char_count(&s) as f64))
            }
        }
    }

    /// `index` com IGNORECASE, em caracteres.
    fn index_of(&self, s: &[u8], t: &[u8]) -> usize {
        // O gawk 5.2.1 devolve 1 pra busca de texto vazio (mesmo em texto vazio).
        if t.is_empty() {
            return 1;
        }
        let (hs, ts);
        let (h, n): (&[u8], &[u8]) = if self.ignorecase {
            hs = lower_bytes(s);
            ts = lower_bytes(t);
            (&hs, &ts)
        } else {
            (s, t)
        };
        if n.len() > h.len() {
            return 0;
        }
        let starts = char_starts(h);
        for (ci, &bi) in starts.iter().enumerate() {
            if h[bi..].starts_with(n) {
                return ci + 1;
            }
        }
        0
    }

    /// Array alvo de um argumento de builtin (`split(s, a)`), limpando-o se `clear`.
    fn array_arg(&mut self, e: &Expr, fname: &str, argno: usize) -> R<ArrRef> {
        match e {
            Expr::Var(v) => {
                if let Var::Global(i) = v
                    && *i < sv::COUNT && !matches!(*i, sv::ENVIRON | sv::ARGV | sv::PROCINFO | sv::SYMTAB | sv::FUNCTAB) {
                        let _ = argno;
                        let n = crate::parser::SPECIALS[*i as usize];
                        return Err(self.fatal(format!("{fname}: cannot use special variable `{n}' as {}", ordinal_arg(argno))));
                    }
                self.get_array(*v)
            }
            Expr::Index(v, groups) => self.array_at(*v, groups),
            _ => Err(self.fatal(format!("{fname}: {} argument is not an array", ordinal(argno)))),
        }
    }

    fn bi_split(&mut self, args: &[Expr], pat: bool) -> R<Value> {
        let fname = if pat { "patsplit" } else { "split" };
        let s = self.eval_str(&args[0])?;
        let arr = self.array_arg(&args[1], fname, 2)?;
        let seps_arr = match args.get(3) {
            Some(e) => {
                let a = self.array_arg(e, fname, 4)?;
                if Rc::ptr_eq(&a, &arr) {
                    return Err(self.fatal(format!("{fname}: cannot use the same array for second and fourth args")));
                }
                Some(a)
            }
            None => None,
        };
        let mode = if pat {
            match args.get(2) {
                Some(e) => SplitMode::Fpat(self.regex_of(e)?),
                None => {
                    let fp = self.global_bytes(sv::FPAT);
                    SplitMode::Fpat(self.dyn_regex(&fp)?)
                }
            }
        } else {
            match args.get(2) {
                Some(Expr::Regex(id)) | Some(Expr::TypedRegex(id)) => SplitMode::Regex(self.const_regex(*id)?),
                Some(e) => {
                    let v = self.eval(e)?;
                    if let Value::Regex(id, _) = v {
                        SplitMode::Regex(self.const_regex(id)?)
                    } else {
                        let fs = self.to_str(&v).to_vec();
                        let ic = self.ignorecase;
                        self.split_mode_for(&fs, ic)?
                    }
                }
                None => {
                    let fs = self.global_bytes(sv::FS);
                    let ic = self.ignorecase;
                    self.split_mode_for(&fs, ic)?
                }
            }
        };
        let mut fields = Vec::new();
        let mut seps = Vec::new();
        crate::fields::split_record(self, &s, &mode, false, &mut fields, Some(&mut seps))?;
        {
            let mut a = arr.borrow_mut();
            a.clear();
            for (i, f) in fields.iter().enumerate() {
                a.insert(Subscript::from_int(i as i64 + 1), Cell::Val(Value::strnum(f)));
            }
        }
        if let Some(sa) = seps_arr {
            let mut a = sa.borrow_mut();
            a.clear();
            for (i, sep) in seps {
                let idx = i as i64;
                a.insert(Subscript::from_int(idx), Cell::Val(Value::strnum(&sep)));
            }
        }
        Ok(Value::Num(fields.len() as f64))
    }

    fn bi_sub(&mut self, args: &[Expr], global: bool) -> R<Value> {
        let re = self.regex_of(&args[0])?;
        let repl = self.eval_str(&args[1])?;
        let target: LValue = match args.get(2) {
            Some(Expr::Var(v)) => LValue::Var(*v),
            Some(Expr::Field(e)) => LValue::Field(e.clone()),
            Some(Expr::Index(v, g)) => LValue::Index(*v, g.clone()),
            Some(Expr::Group(inner)) if inner.is_lvalue() => match (**inner).clone().into_lvalue() {
                Ok(lv) => lv,
                Err(_) => unreachable!(),
            },
            Some(e) => {
                // Alvo que não é variável: o gawk avisa e calcula sem guardar.
                let s = self.eval_str(e)?;
                let name = if global { "gsub" } else { "sub" };
                self.warning(format!("{name}: third argument is not a changeable object"));
                let (_, n) = substitute(&re, &s, &repl, global);
                return Ok(Value::Num(n as f64));
            }
            None => LValue::Field(Box::new(Expr::Num(0.0))),
        };
        let place = self.place(&target)?;
        let cur = self.read_place(&place)?;
        let s = self.to_str(&cur);
        let (out, n) = substitute(&re, &s, &repl, global);
        if n > 0 {
            self.write_place(&place, Value::Str(Rc::from(out)))?;
        }
        Ok(Value::Num(n as f64))
    }

    fn bi_gensub(&mut self, args: &[Expr]) -> R<Value> {
        let re = self.regex_of(&args[0])?;
        let repl = self.eval_str(&args[1])?;
        let how_v = self.eval(&args[2])?;
        let target = match args.get(3) {
            Some(e) => self.eval_str(e)?,
            None => self.get_record_value(),
        };
        let how_s = self.to_str(&how_v);
        let which: Option<usize> = if how_s.first().is_some_and(|c| *c == b'g' || *c == b'G') {
            None
        } else {
            let n = how_v.to_num();
            if n < 1.0 || (!matches!(how_v, Value::Num(_)) && !looks_numeric(&how_s)) {
                let shown = String::from_utf8_lossy(&how_s).into_owned();
                self.warning(format!("gensub: third argument `{shown}' treated as 1"));
                Some(1)
            } else {
                Some(n as usize)
            }
        };
        Ok(Value::Str(Rc::from(gensub(&re, &target, &repl, which))))
    }

    fn bi_match(&mut self, args: &[Expr]) -> R<Value> {
        let s = self.eval_str(&args[0])?;
        let re = self.regex_of(&args[1])?;
        let arr = match args.get(2) {
            Some(e) => Some(self.array_arg(e, "match", 3)?),
            None => None,
        };
        let caps = if arr.is_some() { re.captures_at(&s, 0, false) } else { re.find_at(&s, 0, false).map(|m| vec![Some(m)]) };
        if let Some(a) = &arr {
            a.borrow_mut().clear();
        }
        match caps {
            Some(caps) => {
                let (ms, me) = caps[0].expect("casada");
                let rstart = chars_before(&s, ms) + 1;
                let rlength = char_count(&s[ms..me]);
                self.set_global(sv::RSTART, Value::Num(rstart as f64))?;
                self.set_global(sv::RLENGTH, Value::Num(rlength as f64))?;
                if let Some(a) = &arr {
                    let subsep = self.subsep.clone();
                    let mut a = a.borrow_mut();
                    for (i, c) in caps.iter().enumerate() {
                        let Some((gs, ge)) = c else { continue };
                        a.insert(Subscript::from_int(i as i64), Cell::Val(Value::strnum(&s[*gs..*ge])));
                        let mut k = i.to_string().into_bytes();
                        k.extend_from_slice(&subsep);
                        let mut ks = k.clone();
                        ks.extend_from_slice(b"start");
                        a.insert(Subscript::from_bytes(Rc::from(ks)), Cell::Val(Value::Num((chars_before(&s, *gs) + 1) as f64)));
                        let mut kl = k;
                        kl.extend_from_slice(b"length");
                        a.insert(Subscript::from_bytes(Rc::from(kl)), Cell::Val(Value::Num(char_count(&s[*gs..*ge]) as f64)));
                    }
                }
                Ok(Value::Num(rstart as f64))
            }
            None => {
                self.set_global(sv::RSTART, Value::Num(0.0))?;
                self.set_global(sv::RLENGTH, Value::Num(-1.0))?;
                Ok(Value::Num(0.0))
            }
        }
    }

    fn bi_typeof(&mut self, args: &[Expr]) -> R<Value> {
        // Segundo argumento (não documentado no gawk): recebe `array_type` com o sabor do array.
        if let Some(info) = args.get(1) {
            let arr = match &args[0] {
                Expr::Var(v) => match self.var_cell(*v) {
                    Cell::Arr(a) => Some(a),
                    _ => None,
                },
                Expr::Index(v, groups) => match self.elem_cell(*v, groups)? {
                    Some(Cell::Arr(a)) => Some(a),
                    _ => None,
                },
                _ => None,
            };
            let dst = self.array_arg(info, "typeof", 2)?;
            dst.borrow_mut().clear();
            if let Some(a) = arr {
                let flavor = a.borrow().flavor();
                dst.borrow_mut().insert(Subscript::from_bytes(Rc::from(&b"array_type"[..])), Cell::Val(Value::from_bytes(flavor.as_bytes())));
            }
        }
        let t: &str = match &args[0] {
            Expr::Var(v) => {
                if let Var::Global(i) = v {
                    if matches!(*i, sv::NF | sv::NR | sv::FNR) {
                        "number"
                    } else {
                        self.type_of_cell(self.var_cell(*v))
                    }
                } else {
                    self.type_of_cell(self.var_cell(*v))
                }
            }
            Expr::Index(v, groups) => match self.elem_cell(*v, groups)? {
                Some(c) => self.type_of_cell(c),
                None => "untyped",
            },
            Expr::Field(e) => {
                let i = self.field_index(e)?;
                let v = self.get_field(i)?;
                type_of_value(&v)
            }
            e => {
                let v = self.eval(e)?;
                type_of_value(&v)
            }
        };
        Ok(Value::from_bytes(t.as_bytes()))
    }

    fn type_of_cell(&self, c: Cell) -> &'static str {
        match c {
            Cell::Uninit | Cell::Ref(_) => "untyped",
            Cell::Arr(_) => "array",
            Cell::Val(v) => type_of_value(&v),
        }
    }
}

fn type_of_value(v: &Value) -> &'static str {
    match v {
        Value::Uninit => "unassigned",
        Value::Num(_) => "number",
        Value::Str(_) => "string",
        Value::StrNum(s) => {
            if looks_numeric(s) {
                "strnum"
            } else {
                "string"
            }
        }
        Value::Regex(..) => "regexp",
        Value::Bool(_) => "number|bool",
    }
}

fn ordinal(n: usize) -> &'static str {
    match n {
        1 => "first",
        2 => "second",
        3 => "third",
        4 => "fourth",
        _ => "an",
    }
}

fn ordinal_arg(n: usize) -> String {
    format!("{} arg", ordinal(n))
}

fn to_u64(x: f64) -> u64 {
    if x >= 18446744073709551615.0 { u64::MAX } else { x.trunc() as u64 }
}

/// `strtonum`: hexadecimal `0x`, octal com zero à esquerda, senão decimal.
pub fn strtonum(s: &[u8]) -> f64 {
    let start = s.iter().position(|c| !matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)).unwrap_or(s.len());
    let t = &s[start..];
    if t.len() >= 2 && t[0] == b'0' && (t[1] == b'x' || t[1] == b'X') {
        let mut v = 0f64;
        for c in &t[2..] {
            match (*c as char).to_digit(16) {
                Some(d) => v = v * 16.0 + d as f64,
                None => break,
            }
        }
        return v;
    }
    if t.len() >= 2 && t[0] == b'0' && t[1].is_ascii_digit() {
        // Octal só se todos os dígitos forem 0-7; senão decimal.
        let digits: Vec<u8> = t.iter().take_while(|c| c.is_ascii_digit()).copied().collect();
        if digits.iter().all(|c| *c <= b'7') {
            let mut v = 0f64;
            for c in &digits {
                v = v * 8.0 + (c - b'0') as f64;
            }
            return v;
        }
    }
    str_to_num(t)
}

/// `substr` com o arredondamento observado no gawk 5.2.1 (truncamento, começo mínimo 1).
pub fn substr(s: &[u8], m: f64, n: Option<f64>) -> Vec<u8> {
    let start = if m.is_nan() { 1.0 } else { m };
    if start == f64::INFINITY {
        return Vec::new();
    }
    let indx = if start < 1.0 { 1usize } else { start.trunc().min(4.0e18) as usize };
    let len = match n {
        Some(l) => {
            if l.is_nan() {
                return Vec::new();
            }
            let l = l.trunc();
            if l <= 0.0 {
                return Vec::new();
            }
            if l > 4.0e18 { usize::MAX } else { l as usize }
        }
        None => usize::MAX,
    };
    if s.is_ascii() {
        if indx > s.len() {
            return Vec::new();
        }
        let end = (indx - 1).saturating_add(len).min(s.len());
        return s[indx - 1..end].to_vec();
    }
    let starts = char_starts(s);
    let nchars = starts.len();
    if indx > nchars {
        return Vec::new();
    }
    let from = starts[indx - 1];
    let end_char = (indx - 1).saturating_add(len);
    let to = if end_char >= nchars { s.len() } else { starts[end_char] };
    s[from..to].to_vec()
}

/// Substituição do `sub`/`gsub`: devolve o texto novo e quantas trocas.
pub fn substitute(re: &Regex, s: &[u8], repl: &[u8], global: bool) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut pos = 0;
    let mut count = 0;
    let mut last_match_end: Option<usize> = None;
    loop {
        if pos > s.len() {
            break;
        }
        let Some((ms, me)) = re.find_at(s, pos, false) else { break };
        if ms == me && last_match_end == Some(ms) {
            // Casada vazia colada na casada anterior não conta.
            if ms >= s.len() {
                break;
            }
            let n = utf8_char_len(&s[ms..]);
            out.extend_from_slice(&s[pos..ms + n]);
            pos = ms + n;
            continue;
        }
        out.extend_from_slice(&s[pos..ms]);
        expand_sub_repl(repl, &s[ms..me], &mut out);
        count += 1;
        last_match_end = Some(me);
        if ms == me {
            if ms >= s.len() {
                pos = ms;
                break;
            }
            let n = utf8_char_len(&s[ms..]);
            out.extend_from_slice(&s[ms..ms + n]);
            pos = ms + n;
        } else {
            pos = me;
        }
        if !global {
            break;
        }
    }
    if pos < s.len() {
        out.extend_from_slice(&s[pos..]);
    }
    (out, count)
}

/// Expande `&` e as barras do texto de troca do `sub`/`gsub` (regras do gawk).
fn expand_sub_repl(repl: &[u8], matched: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < repl.len() {
        let c = repl[i];
        if c == b'\\' {
            let start = i;
            while i < repl.len() && repl[i] == b'\\' {
                i += 1;
            }
            let k = i - start;
            if i < repl.len() && repl[i] == b'&' {
                out.extend(std::iter::repeat_n(b'\\', k / 2));
                if k % 2 == 1 {
                    out.push(b'&');
                } else {
                    out.extend_from_slice(matched);
                }
                i += 1;
            } else {
                out.extend(std::iter::repeat_n(b'\\', k));
            }
        } else if c == b'&' {
            out.extend_from_slice(matched);
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
}

/// `gensub`: `which` `None` é global, `Some(n)` troca só a n-ésima casada.
pub fn gensub(re: &Regex, s: &[u8], repl: &[u8], which: Option<usize>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut pos = 0;
    let mut nth = 0;
    let mut last_match_end: Option<usize> = None;
    loop {
        if pos > s.len() {
            break;
        }
        let Some(caps) = re.captures_at(s, pos, false) else { break };
        let (ms, me) = caps[0].expect("casada");
        if ms == me && last_match_end == Some(ms) {
            if ms >= s.len() {
                break;
            }
            let n = utf8_char_len(&s[ms..]);
            out.extend_from_slice(&s[pos..ms + n]);
            pos = ms + n;
            continue;
        }
        nth += 1;
        out.extend_from_slice(&s[pos..ms]);
        let replace = match which {
            None => true,
            Some(n) => n == nth,
        };
        if replace {
            expand_gensub_repl(repl, s, &caps, &mut out);
        } else {
            out.extend_from_slice(&s[ms..me]);
        }
        last_match_end = Some(me);
        if ms == me {
            if ms >= s.len() {
                pos = ms;
                break;
            }
            let n = utf8_char_len(&s[ms..]);
            out.extend_from_slice(&s[ms..ms + n]);
            pos = ms + n;
        } else {
            pos = me;
        }
        if let Some(n) = which
            && nth >= n {
                break;
            }
    }
    if pos < s.len() {
        out.extend_from_slice(&s[pos..]);
    }
    out
}

fn expand_gensub_repl(repl: &[u8], s: &[u8], caps: &[Option<(usize, usize)>], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < repl.len() {
        let c = repl[i];
        if c == b'\\' && i + 1 < repl.len() {
            let d = repl[i + 1];
            if d.is_ascii_digit() {
                let g = (d - b'0') as usize;
                if let Some(Some((a, b))) = caps.get(g) {
                    out.extend_from_slice(&s[*a..*b]);
                }
            } else {
                out.push(d);
            }
            i += 2;
        } else if c == b'&' {
            if let Some((a, b)) = caps[0] {
                out.extend_from_slice(&s[a..b]);
            }
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rand_matches_gawk() {
        let mut r = Random::new();
        assert_eq!(format!("{:.17}", r.rand()), "0.92404581539912467");
        assert_eq!(format!("{:.17}", r.rand()), "0.59390861929587824");
        assert_eq!(format!("{:.17}", r.rand()), "0.30639443885198236");
        r.seed(0);
        assert_eq!(format!("{:.17}", r.rand()), "0.85556592365572337");
        r.seed(2);
        assert_eq!(format!("{:.17}", r.rand()), "0.89310426406734589");
        assert_eq!(format!("{:.17}", r.rand()), "0.22469458977598800");
        r.seed(u32::MAX);
        assert_eq!(format!("{:.17}", r.rand()), "0.79687345055059766");
        r.seed(123456789);
        assert_eq!(format!("{:.17}", r.rand()), "0.57770541665618058");
        assert_eq!(format!("{:.18}", r.rand()), "0.087389898375496267");
    }

    #[test]
    fn substr_rules() {
        assert_eq!(substr(b"hello", 0.0, Some(2.0)), b"he");
        assert_eq!(substr(b"abcdef", 2.7, Some(2.0)), b"bc");
        assert_eq!(substr(b"abcdef", 2.0, Some(f64::INFINITY)), b"bcdef");
        assert_eq!(substr(b"abcdef", 7.0, Some(2.0)), b"");
    }
}
