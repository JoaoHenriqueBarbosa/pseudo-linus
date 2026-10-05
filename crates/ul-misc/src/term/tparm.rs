//! O interpretador de `tparm` (`lib_tparm.c`) e o `tputs` com o preenchimento (`lib_tputs.c`).

use std::time::Duration;

use sysabi::sys;

use super::terminfo::TermType;

const NUM_PARM: usize = 9;
const NUM_VARS: usize = 26;
const STACKSIZE: usize = 20;

/// Um parâmetro de `tparm`: número (um `long`) ou cadeia (`NULL` vira a cadeia vazia).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arg {
    Num(i64),
    Str(Option<Vec<u8>>),
}

/// O estado que sobrevive entre chamadas: as variáveis estáticas `A` a `Z`.
#[derive(Clone, Debug, Default)]
pub struct ParmState {
    pub static_vars: [i32; NUM_VARS],
}

impl ParmState {
    /// `_nc_reset_tparm`.
    pub fn reset(&mut self) {
        self.static_vars = [0; NUM_VARS];
    }
}

fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// `parse_format`: lê os modificadores depois do `%` e devolve (posição do caractere de conversão,
/// o formato reconstruído para o printf, e o tamanho máximo da saída).
fn parse_format(s: &[u8], mut pos: usize) -> (usize, Vec<u8>, i32) {
    let mut format: Vec<u8> = vec![b'%'];
    let mut done = false;
    let mut allowminus = false;
    let mut dot = false;
    let mut err = false;
    let mut my_width = 0i32;
    let mut my_prec = 0i32;
    let mut value = 0i32;
    while at(s, pos) != 0 && !done {
        let c = at(s, pos);
        match c {
            b'c' | b'd' | b'o' | b'x' | b'X' | b's' => {
                format.push(c);
                done = true;
            }
            b'.' => {
                format.push(c);
                pos += 1;
                if dot {
                    err = true;
                } else {
                    dot = true;
                    my_width = value;
                }
                value = 0;
            }
            b'#' | b' ' => {
                format.push(c);
                pos += 1;
            }
            b':' => {
                pos += 1;
                allowminus = true;
            }
            b'-' => {
                if allowminus {
                    format.push(c);
                    pos += 1;
                } else {
                    done = true;
                }
            }
            _ => {
                if c.is_ascii_digit() {
                    value = value.wrapping_mul(10).wrapping_add(i32::from(c - b'0'));
                    if value > 10000 {
                        err = true;
                    }
                    format.push(c);
                    pos += 1;
                } else {
                    done = true;
                }
            }
        }
    }
    if err {
        my_width = 0;
        my_prec = 0;
        value = 0;
        format = vec![b'%', at(s, pos)];
    }
    if dot {
        my_prec = value;
    } else {
        my_width = value;
    }
    (pos, format, my_width.max(my_prec))
}

/// O resultado de `_nc_tparm_analyze`.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    /// O que a função devolve (`number`).
    pub number: i32,
    /// O maior parâmetro referenciado (`popcount`).
    pub popcount: i32,
    pub p_is_s: [bool; NUM_PARM],
}

/// `_nc_tparm_analyze`: quantos parâmetros a cadeia usa e quais são cadeias.
pub fn analyze(string: &[u8]) -> Analysis {
    let mut a = Analysis::default();
    let len2 = string.len();
    let mut lastpop: i32 = -1;
    let mut number = 0i32;
    let mut level = -1i32;
    let mut cp = 0usize;
    let bump = |level: i32, number: &mut i32| {
        if level < 0 && *number < 2 {
            *number += 1;
        }
    };
    while cp < len2 {
        if at(string, cp) == b'%' {
            cp += 1;
            let (np, _fmt, _len) = parse_format(string, cp);
            cp = np;
            match at(string, cp) {
                b'd' | b'o' | b'x' | b'X' | b'c' => {
                    if lastpop <= 0 {
                        bump(level, &mut number);
                    }
                    level -= 1;
                    lastpop = -1;
                }
                b'l' | b's' => {
                    if lastpop > 0 {
                        level -= 1;
                        a.p_is_s[(lastpop - 1) as usize] = true;
                    }
                    bump(level, &mut number);
                }
                b'p' => {
                    cp += 1;
                    let i = i32::from(at(string, cp)) - i32::from(b'0');
                    if (0..=NUM_PARM as i32).contains(&i) {
                        level += 1;
                        lastpop = i;
                        if lastpop > a.popcount {
                            a.popcount = lastpop;
                        }
                    }
                }
                b'P' => cp += 1,
                b'g' => {
                    level += 1;
                    cp += 1;
                }
                b'\'' => {
                    level += 1;
                    cp += 2;
                    lastpop = -1;
                }
                b'{' => {
                    level += 1;
                    cp += 1;
                    while at(string, cp).is_ascii_digit() {
                        cp += 1;
                    }
                }
                b'+' | b'-' | b'*' | b'/' | b'm' | b'A' | b'O' | b'&' | b'|' | b'^' | b'=' | b'<' | b'>' => {
                    bump(level, &mut number);
                    level -= 1;
                    lastpop = -1;
                }
                b'!' | b'~' => {
                    bump(level, &mut number);
                    lastpop = -1;
                }
                _ => {}
            }
        }
        if at(string, cp) != 0 {
            cp += 1;
        }
    }
    a.number = number.min(NUM_PARM as i32);
    a
}

/// `tparm_setup`: a análise da cadeia.
#[derive(Clone, Debug, Default)]
pub struct Setup {
    pub tparm_type: u32,
    pub num_actual: usize,
    pub num_parsed: usize,
    pub num_popped: usize,
    pub p_is_s: [bool; NUM_PARM],
}

pub fn setup(string: &[u8]) -> Setup {
    let a = analyze(string);
    let num_parsed = (a.number.max(0) as usize).min(NUM_PARM);
    let num_popped = (a.popcount.max(0) as usize).min(NUM_PARM);
    let num_actual = num_parsed.max(num_popped);
    let mut tparm_type = 0u32;
    for n in 0..num_actual {
        if a.p_is_s[n] {
            tparm_type |= 1 << n;
        }
    }
    Setup { tparm_type, num_actual, num_parsed, num_popped, p_is_s: a.p_is_s }
}

#[derive(Clone, Debug)]
enum Item {
    Num(i32),
    Str(Option<Vec<u8>>),
}

struct Stack {
    items: Vec<Item>,
    ptr: isize,
}

impl Stack {
    fn npush(&mut self, x: i32) {
        if (self.ptr as usize) < STACKSIZE && self.ptr >= 0 {
            self.set(Item::Num(x));
        }
    }

    fn spush(&mut self, x: Option<Vec<u8>>) {
        if (self.ptr as usize) < STACKSIZE && self.ptr >= 0 {
            self.set(Item::Str(x));
        }
    }

    fn set(&mut self, it: Item) {
        let p = self.ptr as usize;
        if p < self.items.len() {
            self.items[p] = it;
        } else {
            self.items.push(it);
        }
        self.ptr += 1;
    }

    fn npop(&mut self) -> i32 {
        let was = self.ptr;
        self.ptr -= 1;
        if was > 0 {
            match &self.items[self.ptr as usize] {
                Item::Num(n) => *n,
                Item::Str(_) => 0,
            }
        } else {
            self.ptr = 0;
            0
        }
    }

    fn spop(&mut self) -> Vec<u8> {
        let was = self.ptr;
        self.ptr -= 1;
        if was > 0 {
            match &self.items[self.ptr as usize] {
                Item::Str(Some(s)) => s.clone(),
                _ => Vec::new(),
            }
        } else {
            self.ptr = 0;
            Vec::new()
        }
    }
}

fn pad_to(mut body: Vec<u8>, width: usize, left: bool, zero: bool, sign_len: usize) -> Vec<u8> {
    if body.len() >= width {
        return body;
    }
    let fill = width - body.len();
    if left {
        body.extend(std::iter::repeat_n(b' ', fill));
        body
    } else if zero {
        let mut out = body[..sign_len].to_vec();
        out.extend(std::iter::repeat_n(b'0', fill));
        out.extend_from_slice(&body[sign_len..]);
        out
    } else {
        let mut out: Vec<u8> = std::iter::repeat_n(b' ', fill).collect();
        out.append(&mut body);
        out
    }
}

/// `printf` de um formato `%[#0- ][largura][.precisão](d|o|x|X)` com um `int`.
fn format_int(fmt: &[u8], x: i32) -> Vec<u8> {
    let conv = *fmt.last().unwrap_or(&b'd');
    let mut i = 1;
    let (mut alt, mut zero, mut left, mut space) = (false, false, false, false);
    while i + 1 < fmt.len() {
        match fmt[i] {
            b'#' => alt = true,
            b'0' => zero = true,
            b'-' => left = true,
            b' ' => space = true,
            _ => break,
        }
        i += 1;
    }
    let mut width = 0usize;
    while i + 1 < fmt.len() && fmt[i].is_ascii_digit() {
        width = width.saturating_mul(10).saturating_add(usize::from(fmt[i] - b'0'));
        i += 1;
    }
    let mut prec: Option<usize> = None;
    if i + 1 < fmt.len() && fmt[i] == b'.' {
        i += 1;
        let mut p = 0usize;
        while i + 1 < fmt.len() && fmt[i].is_ascii_digit() {
            p = p.saturating_mul(10).saturating_add(usize::from(fmt[i] - b'0'));
            i += 1;
        }
        prec = Some(p);
    }
    let mut sign: Vec<u8> = Vec::new();
    let mut digits: Vec<u8> = match conv {
        b'o' => format!("{:o}", x as u32).into_bytes(),
        b'x' => format!("{:x}", x as u32).into_bytes(),
        b'X' => format!("{:X}", x as u32).into_bytes(),
        _ => {
            if x < 0 {
                sign.push(b'-');
            } else if space {
                sign.push(b' ');
            }
            i64::from(x).unsigned_abs().to_string().into_bytes()
        }
    };
    if prec == Some(0) && x == 0 {
        digits.clear();
    }
    if let Some(p) = prec {
        while digits.len() < p {
            digits.insert(0, b'0');
        }
    }
    match conv {
        b'o' if alt && digits.first() != Some(&b'0') => digits.insert(0, b'0'),
        b'x' if alt && x != 0 => sign.extend_from_slice(b"0x"),
        b'X' if alt && x != 0 => sign.extend_from_slice(b"0X"),
        _ => {}
    }
    let sign_len = sign.len();
    let mut body = sign;
    body.extend_from_slice(&digits);
    pad_to(body, width, left, zero && prec.is_none(), sign_len)
}

/// `printf` de `%[-][largura][.precisão]s`.
fn format_str(fmt: &[u8], s: &[u8]) -> Vec<u8> {
    let mut i = 1;
    let mut left = false;
    while i + 1 < fmt.len() {
        match fmt[i] {
            b'-' => left = true,
            b'#' | b' ' | b'0' => {}
            _ => break,
        }
        i += 1;
    }
    let mut width = 0usize;
    while i + 1 < fmt.len() && fmt[i].is_ascii_digit() {
        width = width.saturating_mul(10).saturating_add(usize::from(fmt[i] - b'0'));
        i += 1;
    }
    let mut body = s.to_vec();
    if i + 1 < fmt.len() && fmt[i] == b'.' {
        i += 1;
        let mut p = 0usize;
        while i + 1 < fmt.len() && fmt[i].is_ascii_digit() {
            p = p.saturating_mul(10).saturating_add(usize::from(fmt[i] - b'0'));
            i += 1;
        }
        body.truncate(p);
    }
    pad_to(body, width, left, false, 0)
}

/// `tparam_internal`: expande a cadeia com os parâmetros.
fn expand(state: &mut ParmState, string: &[u8], setup: &Setup, mut param: [i32; NUM_PARM], pstr: &[Option<Vec<u8>>; NUM_PARM]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut stack = Stack { items: Vec::new(), ptr: 0 };
    let termcap_hack = setup.num_popped == 0;
    if termcap_hack {
        for i in (0..setup.num_parsed).rev() {
            if setup.p_is_s[i] {
                stack.spush(pstr[i].clone());
            } else {
                stack.npush(param[i]);
            }
        }
    }
    let mut incremented_two = false;
    let mut dynamic_vars = [0i32; NUM_VARS];
    let len2 = string.len();
    let mut cp = 0usize;
    while cp < len2 {
        if at(string, cp) != b'%' {
            out.push(at(string, cp));
        } else {
            cp += 1;
            let (np, fmt, len) = parse_format(string, cp);
            let _ = len;
            cp = np;
            match at(string, cp) {
                b'%' => out.push(b'%'),
                b'd' | b'o' | b'x' | b'X' => {
                    let x = stack.npop();
                    out.extend_from_slice(&format_int(&fmt, x));
                }
                b'c' => {
                    let x = stack.npop();
                    out.push(if x == 0 { 0o200 } else { x as u8 });
                }
                b'l' => {
                    let s = stack.spop();
                    stack.npush(s.len() as i32);
                }
                b's' => {
                    let s = stack.spop();
                    out.extend_from_slice(&format_str(&fmt, &s));
                }
                b'p' => {
                    cp += 1;
                    let i = i32::from(at(string, cp)) - i32::from(b'1');
                    if (0..NUM_PARM as i32).contains(&i) {
                        let i = i as usize;
                        if setup.p_is_s[i] {
                            stack.spush(pstr[i].clone());
                        } else {
                            stack.npush(param[i]);
                        }
                    }
                }
                b'P' => {
                    cp += 1;
                    let c = at(string, cp);
                    if c.is_ascii_uppercase() {
                        state.static_vars[usize::from(c - b'A')] = stack.npop();
                    } else if c.is_ascii_lowercase() {
                        dynamic_vars[usize::from(c - b'a')] = stack.npop();
                    }
                }
                b'g' => {
                    cp += 1;
                    let c = at(string, cp);
                    if c.is_ascii_uppercase() {
                        stack.npush(state.static_vars[usize::from(c - b'A')]);
                    } else if c.is_ascii_lowercase() {
                        stack.npush(dynamic_vars[usize::from(c - b'a')]);
                    }
                }
                b'\'' => {
                    cp += 1;
                    stack.npush(i32::from(at(string, cp)));
                    cp += 1;
                }
                b'{' => {
                    let mut number = 0i32;
                    cp += 1;
                    while at(string, cp).is_ascii_digit() {
                        number = number.wrapping_mul(10).wrapping_add(i32::from(at(string, cp) - b'0'));
                        cp += 1;
                    }
                    stack.npush(number);
                }
                b'+' => {
                    let y = stack.npop();
                    let x = stack.npop();
                    stack.npush(x.wrapping_add(y));
                }
                b'-' => {
                    let y = stack.npop();
                    let x = stack.npop();
                    stack.npush(x.wrapping_sub(y));
                }
                b'*' => {
                    let y = stack.npop();
                    let x = stack.npop();
                    stack.npush(x.wrapping_mul(y));
                }
                b'/' => {
                    let y = stack.npop();
                    let x = stack.npop();
                    stack.npush(if y != 0 { x.wrapping_div(y) } else { 0 });
                }
                b'm' => {
                    let y = stack.npop();
                    let x = stack.npop();
                    stack.npush(if y != 0 { x.wrapping_rem(y) } else { 0 });
                }
                b'A' => {
                    let y = stack.npop();
                    let x = stack.npop();
                    stack.npush(i32::from(y != 0 && x != 0));
                }
                b'O' => {
                    let y = stack.npop();
                    let x = stack.npop();
                    stack.npush(i32::from(y != 0 || x != 0));
                }
                b'&' => {
                    let y = stack.npop();
                    let x = stack.npop();
                    stack.npush(x & y);
                }
                b'|' => {
                    let y = stack.npop();
                    let x = stack.npop();
                    stack.npush(x | y);
                }
                b'^' => {
                    let y = stack.npop();
                    let x = stack.npop();
                    stack.npush(x ^ y);
                }
                b'=' => {
                    let y = stack.npop();
                    let x = stack.npop();
                    stack.npush(i32::from(x == y));
                }
                b'<' => {
                    let y = stack.npop();
                    let x = stack.npop();
                    stack.npush(i32::from(x < y));
                }
                b'>' => {
                    let y = stack.npop();
                    let x = stack.npop();
                    stack.npush(i32::from(x > y));
                }
                b'!' => {
                    let x = stack.npop();
                    stack.npush(i32::from(x == 0));
                }
                b'~' => {
                    let x = stack.npop();
                    stack.npush(!x);
                }
                b'i' => {
                    if !incremented_two {
                        incremented_two = true;
                        if !setup.p_is_s[0] {
                            param[0] = param[0].wrapping_add(1);
                            if termcap_hack {
                                if let Some(it) = stack.items.get_mut(0) {
                                    *it = Item::Num(param[0]);
                                }
                            }
                        }
                        if !setup.p_is_s[1] {
                            param[1] = param[1].wrapping_add(1);
                            if termcap_hack {
                                if let Some(it) = stack.items.get_mut(1) {
                                    *it = Item::Num(param[1]);
                                }
                            }
                        }
                    }
                }
                b't' => {
                    let x = stack.npop();
                    if x == 0 {
                        // procura %e ou %; no mesmo nível
                        cp += 1;
                        let mut level = 0;
                        while at(string, cp) != 0 {
                            if at(string, cp) == b'%' {
                                cp += 1;
                                let c = at(string, cp);
                                if c == b'?' {
                                    level += 1;
                                } else if c == b';' {
                                    if level > 0 {
                                        level -= 1;
                                    } else {
                                        break;
                                    }
                                } else if c == b'e' && level == 0 {
                                    break;
                                }
                            }
                            if at(string, cp) != 0 {
                                cp += 1;
                            }
                        }
                    }
                }
                b'e' => {
                    // procura o %; do mesmo nível
                    cp += 1;
                    let mut level = 0;
                    while at(string, cp) != 0 {
                        if at(string, cp) == b'%' {
                            cp += 1;
                            let c = at(string, cp);
                            if c == b'?' {
                                level += 1;
                            } else if c == b';' {
                                if level > 0 {
                                    level -= 1;
                                } else {
                                    break;
                                }
                            }
                        }
                        if at(string, cp) != 0 {
                            cp += 1;
                        }
                    }
                }
                _ => {}
            }
        }
        if at(string, cp) == 0 {
            break;
        }
        cp += 1;
    }
    out
}

fn is_cap(tt: &TermType, var: &str, string: &[u8]) -> bool {
    tt.sv(var).is_some_and(|c| c == string)
}

/// `check_string_caps`: só algumas capacidades aceitam parâmetros de cadeia.
fn check_string_caps(tt: &TermType, tparm_type: u32, string: &[u8]) -> bool {
    let mut want = 0u32;
    if is_cap(tt, "pkey_key", string) || is_cap(tt, "pkey_local", string) || is_cap(tt, "pkey_xmit", string) || is_cap(tt, "plab_norm", string) {
        want = 2;
    } else if is_cap(tt, "pkey_plab", string) {
        want = 6;
    } else {
        if let Some(cs) = tt_ext_str(tt, b"Cs") {
            if cs == string {
                want = 1;
            }
        }
        if let Some(ms) = tt_ext_str(tt, b"Ms") {
            if ms == string {
                want = 3;
            }
        }
    }
    want == tparm_type
}

/// `tigetstr` de uma capacidade estendida ou predefinida pelo nome terminfo.
pub fn tt_ext_str<'a>(tt: &'a TermType, name: &[u8]) -> Option<&'a [u8]> {
    if let Some(j) = super::find_type_entry(name, super::Kind::Str) {
        return tt.strs[j].val();
    }
    for i in (tt.strs.len() - tt.ext_strs)..tt.strs.len() {
        if tt.ext_str_name(i) == name {
            return tt.strs[i].val();
        }
    }
    None
}

fn to_params(args: &[Arg], setup: &Setup) -> ([i32; NUM_PARM], [Option<Vec<u8>>; NUM_PARM]) {
    let mut param = [0i32; NUM_PARM];
    let mut pstr: [Option<Vec<u8>>; NUM_PARM] = Default::default();
    for i in 0..setup.num_actual {
        match args.get(i) {
            Some(Arg::Num(n)) => {
                if setup.p_is_s[i] {
                    pstr[i] = Some(Vec::new());
                } else {
                    param[i] = *n as i32;
                }
            }
            Some(Arg::Str(s)) => {
                if setup.p_is_s[i] {
                    pstr[i] = Some(s.clone().unwrap_or_default());
                }
            }
            None => {
                if setup.p_is_s[i] {
                    pstr[i] = Some(Vec::new());
                }
            }
        }
    }
    (param, pstr)
}

/// `tparm` e `tiparm`: expande `string` com `args`. Devolve `None` quando as regras do ncurses
/// recusam a chamada (cadeia como parâmetro de uma capacidade que não aceita).
pub fn tparm(tt: &TermType, state: &mut ParmState, string: &[u8], args: &[Arg]) -> Option<Vec<u8>> {
    let su = setup(string);
    if !(su.tparm_type == 0 || check_string_caps(tt, su.tparm_type, string)) {
        return None;
    }
    let (param, pstr) = to_params(args, &su);
    Some(expand(state, string, &su, param, &pstr))
}

/// `_nc_tiparm(expected, string, ...)`: só aceita parâmetros numéricos e confere a contagem.
pub fn tiparm(tt: &TermType, state: &mut ParmState, expected: i32, string: &[u8], args: &[i64]) -> Option<Vec<u8>> {
    let su = setup(string);
    if su.tparm_type != 0 {
        return None;
    }
    let mut expected = expected;
    if su.num_actual as i32 != expected {
        let mut needed = expected;
        if is_cap(tt, "to_status_line", string)
            || is_cap(tt, "set_a_background", string)
            || is_cap(tt, "set_a_foreground", string)
            || is_cap(tt, "set_background", string)
            || is_cap(tt, "set_foreground", string)
        {
            needed = 0;
        } else {
            if tt_ext_str(tt, b"xm").is_some_and(|c| c == string) {
                needed = 3;
            }
            if tt_ext_str(tt, b"S0").is_some_and(|c| c == string) {
                needed = 0;
            }
        }
        if su.num_actual as i32 >= needed && su.num_actual as i32 <= expected {
            expected = su.num_actual as i32;
        }
    }
    if su.num_actual == 0 && expected != 0 {
        return None;
    }
    if su.num_actual as i32 > expected {
        return None;
    }
    if expected != 9 && su.num_actual as i32 != expected {
        return None;
    }
    let a: Vec<Arg> = args.iter().map(|n| Arg::Num(*n)).collect();
    let (param, pstr) = to_params(&a, &su);
    Some(expand(state, string, &su, param, &pstr))
}

/// Pra onde o `tputs` escreve: o `outc` do ncurses (um `putchar`) mais o descarregamento que o
/// `delay_output` faz antes de dormir.
pub trait Sink {
    fn put(&mut self, c: u8);
    fn flush(&mut self);
}

/// Acumula num vetor (os testes e quem formata antes de escrever).
#[derive(Debug, Default)]
pub struct VecSink(pub Vec<u8>);

impl Sink for VecSink {
    fn put(&mut self, c: u8) {
        self.0.push(c);
    }

    fn flush(&mut self) {}
}

/// `delay_output(ms)`: espera (terminais com `npc`) ou manda caracteres nulos conforme a velocidade da
/// linha, que sem terminal é 0.
fn delay_output(ms: i32, no_pad_char: bool, sink: &mut dyn Sink) {
    let ms = ms.min(30000);
    if no_pad_char {
        sink.flush();
        if ms > 0 {
            if let Some(s) = sys::try_current() {
                let _ = s.nanosleep(Duration::from_millis(ms as u64));
            }
        }
    }
}

/// `tputs(string, affcnt, outc)` com o `BSD_TPUTS` ligado. `always_delay` vale quando a cadeia é a
/// própria `bell` ou `flash_screen`.
pub fn tputs(tt: Option<&TermType>, string: &[u8], affcnt: i32, always_delay: bool, sink: &mut dyn Sink) {
    let no_pad_char = tt.is_some_and(|t| t.b("no_pad_char"));
    // `normal_delay` depende da velocidade da linha (0 sem terminal) contra `padding_baud_rate`:
    // nunca vale aqui.
    let normal_delay = false;
    let mut i = 0usize;
    let mut trailpad: i32 = 0;
    if at(string, i).is_ascii_digit() {
        while at(string, i).is_ascii_digit() {
            trailpad = trailpad.wrapping_mul(10).wrapping_add(i32::from(at(string, i) - b'0'));
            i += 1;
        }
        trailpad = trailpad.wrapping_mul(10);
        if at(string, i) == b'.' {
            i += 1;
            if at(string, i).is_ascii_digit() {
                trailpad = trailpad.wrapping_add(i32::from(at(string, i) - b'0'));
                i += 1;
            }
            while at(string, i).is_ascii_digit() {
                i += 1;
            }
        }
        if at(string, i) == b'*' {
            trailpad = trailpad.wrapping_mul(affcnt);
            i += 1;
        }
    }
    while at(string, i) != 0 {
        if at(string, i) != b'$' {
            sink.put(at(string, i));
        } else {
            i += 1;
            if at(string, i) != b'<' {
                sink.put(b'$');
                if at(string, i) != 0 {
                    sink.put(at(string, i));
                }
            } else {
                i += 1;
                let rest = &string[i.min(string.len())..];
                if (!at(string, i).is_ascii_digit() && at(string, i) != b'.') || !rest.contains(&b'>') {
                    sink.put(b'$');
                    sink.put(b'<');
                    continue;
                }
                let mut number: i32 = 0;
                while at(string, i).is_ascii_digit() {
                    number = number.wrapping_mul(10).wrapping_add(i32::from(at(string, i) - b'0'));
                    i += 1;
                }
                number = number.wrapping_mul(10);
                if at(string, i) == b'.' {
                    i += 1;
                    if at(string, i).is_ascii_digit() {
                        number = number.wrapping_add(i32::from(at(string, i) - b'0'));
                        i += 1;
                    }
                    while at(string, i).is_ascii_digit() {
                        i += 1;
                    }
                }
                let mut mandatory = false;
                while at(string, i) == b'*' || at(string, i) == b'/' {
                    if at(string, i) == b'*' {
                        number = number.wrapping_mul(affcnt);
                    } else {
                        mandatory = true;
                    }
                    i += 1;
                }
                if number > 0 && (always_delay || normal_delay || mandatory) {
                    delay_output(number / 10, no_pad_char, sink);
                }
            }
        }
        if at(string, i) == 0 {
            break;
        }
        i += 1;
    }
    if trailpad > 0 && (always_delay || normal_delay) {
        delay_output(trailpad / 10, no_pad_char, sink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(s: &[u8], args: &[i64]) -> Vec<u8> {
        let tt = TermType::empty();
        let mut st = ParmState::default();
        let su = setup(s);
        let a: Vec<Arg> = args.iter().map(|n| Arg::Num(*n)).collect();
        let (p, ps) = to_params(&a, &su);
        let _ = &tt;
        expand(&mut st, s, &su, p, &ps)
    }

    #[test]
    fn cup() {
        assert_eq!(run(b"\x1b[%i%p1%d;%p2%dH", &[3, 4]), b"\x1b[4;5H");
    }

    #[test]
    fn conditionals() {
        assert_eq!(run(b"%?%p1%{8}%<%t3%p1%d%e%p1%{16}%<%t9%p1%{8}%-%d%e38;5;%p1%d%;m", &[1]), b"31m");
        assert_eq!(run(b"%?%p1%{8}%<%t3%p1%d%e%p1%{16}%<%t9%p1%{8}%-%d%e38;5;%p1%d%;m", &[9]), b"91m");
        assert_eq!(run(b"%?%p1%{8}%<%t3%p1%d%e%p1%{16}%<%t9%p1%{8}%-%d%e38;5;%p1%d%;m", &[100]), b"38;5;100m");
    }

    #[test]
    fn formats() {
        assert_eq!(run(b"%p1%03d|%p1%:-4d|%p1%x|%p1%#o", &[7]), b"007|7   |7|07");
        assert_eq!(run(b"%p1%c", &[0]), b"\x80");
    }

    #[test]
    fn padding_is_dropped() {
        let mut out = VecSink::default();
        tputs(None, b"a$<5>b$<2*>c", 1, false, &mut out);
        assert_eq!(out.0, b"abc");
    }
}
