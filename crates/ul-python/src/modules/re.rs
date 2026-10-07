//! Módulo `re` do CPython 3.13 sobre o motor de [`re_engine`](super::re_engine).
//!
//! `re.Pattern` e `re.Match` são `ExtObject`. Os índices são em pontos de código. Fora desta
//! versão: padrões e textos em `bytes` (levantam `NotImplementedError`/`TypeError`), `Scanner`,
//! `Pattern.__eq__`/`__hash__`, cópia e `pickle`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use super::re_engine::{self as eng, Captures, IterState, Mode, ReError, Regex};
use super::ModuleBuilder;
use crate::native_util::{bind, exactly, no_kwargs, want_int};
use crate::object::{repr, Dict, ExcObj, ExtObject, Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyException, PyResult, Vm};

thread_local! {
    /// Endereço do objeto para o `Pattern` (o `dyn ExtObject` não permite recuperar o tipo).
    static REGISTRY: RefCell<HashMap<usize, Weak<PatternObj>>> = RefCell::new(HashMap::new());
    /// Cache de `re.compile` por (padrão, flags), como o do CPython.
    static CACHE: RefCell<HashMap<(String, u32, bool), Rc<PatternObj>>> = RefCell::new(HashMap::new());
}

const CACHE_MAX: usize = 512;

fn to_py(e: &ReError, pattern: &[char]) -> PyException {
    if e.value_error {
        exc("ValueError", e.msg.clone())
    } else {
        re_error(e, pattern)
    }
}

/// `re.error` com `msg`, `pattern` e `pos` guardados nos argumentos (o `str` mostra só o primeiro).
fn re_error(e: &ReError, pattern: &[char]) -> PyException {
    let formatted = e.format(pattern);
    let pos = e.pos.map_or(Value::None, |p| Value::Int(p as i64));
    let args = vec![
        Value::str(formatted.clone()),
        Value::str(e.msg.clone()),
        Value::str(pattern.iter().collect::<String>()),
        pos,
    ];
    let value = Value::Exception(Rc::new(ExcObj::new("re.PatternError", args)));
    PyException { kind: "re.PatternError", msg: formatted, value: Some(value), tb: Vec::new() }
}

fn index_error() -> PyException {
    exc("IndexError", "no such group")
}

fn arg(a: &[Option<Value>], i: usize) -> Value {
    a.get(i).cloned().flatten().unwrap_or(Value::None)
}

fn flags_of(v: &Option<Value>) -> PyResult<u32> {
    match v {
        None => Ok(0),
        Some(x) => Ok((want_int(x)? & 0xFFFF_FFFF) as u32),
    }
}

/// Textos longos já convertidos: um scanner chama `match(s, pos)` centenas de vezes sobre a mesma `str`, e
/// converter a string inteira a cada chamada tornaria o laço quadrático. Guarda a última conversão (por
/// identidade do objeto, mantido vivo pela própria entrada).
const TEXT_CACHE_MIN: usize = 512;

thread_local! {
    static TEXT_CACHE: std::cell::RefCell<Option<(Value, Rc<Vec<char>>)>> = const { std::cell::RefCell::new(None) };
}

fn cached_chars(v: &Value, convert: impl FnOnce() -> Vec<char>, len: usize) -> Rc<Vec<char>> {
    if len < TEXT_CACHE_MIN {
        return Rc::new(convert());
    }
    let same = |a: &Value, b: &Value| match (a, b) {
        (Value::Str(x), Value::Str(y)) => Rc::ptr_eq(x, y),
        (Value::Bytes(x), Value::Bytes(y)) => Rc::ptr_eq(x, y),
        _ => false,
    };
    TEXT_CACHE.with(|c| {
        if let Some((held, chars)) = c.borrow().as_ref() {
            if same(held, v) {
                return chars.clone();
            }
        }
        let chars = Rc::new(convert());
        *c.borrow_mut() = Some((v.clone(), chars.clone()));
        chars
    })
}

/// O texto a casar: `str` (em pontos de código), ou `bytes` (um ponto de código por byte, latin-1).
fn want_text_for(v: &Value, bytes_pattern: bool) -> PyResult<(Value, Rc<Vec<char>>)> {
    // Subclasse de `str`/`bytes` (como `configparser._Line`): casa o valor embutido.
    let unwrapped = match crate::vm::unwrap_payload(v) {
        // `bytearray` e `memoryview` casam como `bytes` (os grupos saem `bytes`).
        b @ (Value::ByteArray(_) | Value::Instance(_)) => b.bytes_like().map_or(b, Value::Bytes),
        other => other,
    };
    let v = &unwrapped;
    match v {
        Value::Str(_) if bytes_pattern => Err(type_error("cannot use a bytes pattern on a string-like object")),
        Value::Str(s) => Ok((v.clone(), cached_chars(v, || s.as_str().chars().collect(), s.len()))),
        Value::Bytes(_) if !bytes_pattern => Err(type_error("cannot use a string pattern on a bytes-like object")),
        Value::Bytes(b) => Ok((v.clone(), cached_chars(v, || b.iter().map(|&c| c as char).collect(), b.len()))),
        other => Err(type_error(format!("expected string or bytes-like object, got '{}'", other.type_name()))),
    }
}

#[cfg(test)]
fn want_text(v: &Value) -> PyResult<(Value, Rc<Vec<char>>)> {
    want_text_for(v, false)
}

/// Texto `str` (pontos de código latin-1) devolvido como `bytes`; `Tuple` recursivamente.
fn rebyte(v: Value) -> Value {
    match v {
        Value::Str(s) => Value::bytes(s.as_str().chars().map(|c| c as u32 as u8).collect::<Vec<u8>>()),
        Value::Tuple(t) => Value::tuple(t.iter().cloned().map(rebyte).collect()),
        other => other,
    }
}

/// Um `str` ou `bytes` saído do motor, no tipo do padrão.
fn out_text(bytes: bool, s: String) -> Value {
    if bytes {
        rebyte(Value::str(s))
    } else {
        Value::str(s)
    }
}

/// `pos`/`endpos` como o CPython: limitados a `[0, len]`.
fn clamp_arg(v: Option<&Value>, default: i64, len: usize) -> PyResult<usize> {
    let n = match v {
        None | Some(Value::None) => default,
        Some(x) => want_int(x)?,
    };
    Ok(n.clamp(0, len as i64) as usize)
}

fn valid_name(s: &str) -> bool {
    let mut it = s.chars();
    match it.next() {
        Some(c) if c == '_' || c.is_alphabetic() => {}
        _ => return false,
    }
    it.all(|c| c == '_' || c.is_alphanumeric())
}

// ---------------------------------------------------------------------------
// Núcleo sem VM (também usado pelos testes)
// ---------------------------------------------------------------------------

fn slice_string(chars: &[char], a: usize, b: usize) -> String {
    chars[a..b].iter().collect()
}

/// `findall`: strings do casamento, do grupo único ou tuplas de grupos (não casados viram `''`).
pub fn findall_core(re: &Regex, chars: &[char], pos: usize, endpos: usize) -> Vec<Value> {
    let mut out = Vec::new();
    let mut st = IterState::new(pos, endpos);
    while let Some(c) = st.next(re, chars) {
        let g = |k: usize| -> String {
            match c.spans[k] {
                Some((a, b)) => slice_string(chars, a, b),
                None => String::new(),
            }
        };
        let v = match re.ngroups {
            0 => Value::str(g(0)),
            1 => Value::str(g(1)),
            n => Value::tuple((1..=n).map(|k| Value::str(g(k))).collect()),
        };
        out.push(v);
    }
    out
}

/// `split`: pedaços e, entre eles, os grupos de captura (`None` se o grupo não casou).
/// `maxsplit == 0` não limita.
pub fn split_core(re: &Regex, chars: &[char], maxsplit: usize) -> Vec<Option<String>> {
    let mut out: Vec<Option<String>> = Vec::new();
    let mut last = 0usize;
    let mut n = 0usize;
    let mut st = IterState::new(0, chars.len());
    while maxsplit == 0 || n < maxsplit {
        let c = match st.next(re, chars) {
            Some(c) => c,
            None => break,
        };
        let (s, e) = c.spans[0].unwrap_or((last, last));
        out.push(Some(slice_string(chars, last, s)));
        for k in 1..=re.ngroups {
            out.push(c.spans[k].map(|(a, b)| slice_string(chars, a, b)));
        }
        last = e;
        n += 1;
    }
    out.push(Some(slice_string(chars, last, chars.len())));
    out
}

/// `sub`/`subn`: troca cada casamento (até `count`, 0 = todos) pelo que `f` devolver.
pub fn sub_core<F>(re: &Regex, chars: &[char], count: usize, mut f: F) -> PyResult<(String, usize)>
where
    F: FnMut(&Captures) -> PyResult<String>,
{
    let mut out = String::new();
    let mut last = 0usize;
    let mut n = 0usize;
    let mut st = IterState::new(0, chars.len());
    while count == 0 || n < count {
        let caps = match st.next(re, chars) {
            Some(c) => c,
            None => break,
        };
        let (s, e) = caps.spans[0].unwrap_or((last, last));
        out.extend(chars[last..s].iter());
        out.push_str(&f(&caps)?);
        last = e;
        n += 1;
    }
    out.extend(chars[last..].iter());
    Ok((out, n))
}

/// `re.escape` para `str`.
pub fn escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '(' | ')' | '[' | ']' | '{' | '}' | '?' | '*' | '+' | '-' | '|' | '^' | '$' | '\\' | '.' | '&' | '~' | '#' | ' '
                | '\t' | '\n' | '\r' | '\u{b}' | '\u{c}'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------------------
// Template de substituição
// ---------------------------------------------------------------------------

/// Pedaço de um template: texto literal ou referência a grupo.
#[derive(Debug, Clone)]
pub enum Tpl {
    Lit(String),
    Group(usize),
}

fn terr(msg: impl Into<String>, pos: usize, tpl: &[char]) -> PyException {
    re_error(&ReError::at(msg, pos), tpl)
}

/// Analisa `\1`, `\g<1>`, `\g<nome>`, `\n`... de um template de `sub`/`expand`.
pub fn parse_template(tpl: &[char], re: &Regex) -> PyResult<Vec<Tpl>> {
    let mut out: Vec<Tpl> = Vec::new();
    let mut lit = String::new();
    let mut i = 0usize;
    while i < tpl.len() {
        let c = tpl[i];
        if c != '\\' {
            lit.push(c);
            i += 1;
            continue;
        }
        let bs = i;
        i += 1;
        if i >= tpl.len() {
            return Err(terr("bad escape (end of pattern)", bs, tpl));
        }
        let d = tpl[i];
        i += 1;
        match d {
            'g' => {
                if tpl.get(i) != Some(&'<') {
                    return Err(terr("missing <", i, tpl));
                }
                i += 1;
                let ns = i;
                let mut name = String::new();
                let mut closed = false;
                while i < tpl.len() {
                    let ch = tpl[i];
                    i += 1;
                    if ch == '>' {
                        closed = true;
                        break;
                    }
                    name.push(ch);
                }
                if !closed {
                    let m = if name.is_empty() { "missing group name" } else { "missing >, unterminated name" };
                    return Err(terr(m, ns, tpl));
                }
                if name.is_empty() {
                    return Err(terr("missing group name", ns, tpl));
                }
                let g = if name.chars().all(|c| c.is_ascii_digit()) {
                    let n = name.parse::<usize>().unwrap_or(usize::MAX);
                    if n > re.ngroups {
                        return Err(terr(format!("invalid group reference {n}"), ns, tpl));
                    }
                    n
                } else {
                    if !valid_name(&name) {
                        return Err(terr(format!("bad character in group name '{name}'"), ns, tpl));
                    }
                    match re.group_index(&name) {
                        Some(k) => k,
                        None => return Err(exc("IndexError", format!("unknown group name '{name}'"))),
                    }
                };
                if !lit.is_empty() {
                    out.push(Tpl::Lit(std::mem::take(&mut lit)));
                }
                out.push(Tpl::Group(g));
            }
            '0' => {
                let mut v = 0u32;
                let mut n = 0;
                while n < 2 {
                    match tpl.get(i).and_then(|x| x.to_digit(8)) {
                        Some(x) => {
                            v = v * 8 + x;
                            i += 1;
                            n += 1;
                        }
                        None => break,
                    }
                }
                lit.push(char::from_u32(v).unwrap_or('\0'));
            }
            '1'..='9' => {
                let mut n = d.to_digit(10).unwrap_or(0) as usize;
                if let Some(&d2) = tpl.get(i) {
                    if d2.is_ascii_digit() {
                        if ('0'..='3').contains(&d) && ('0'..='7').contains(&d2) {
                            if let Some(&d3) = tpl.get(i + 1) {
                                if ('0'..='7').contains(&d3) {
                                    let v = d.to_digit(8).unwrap_or(0) * 64
                                        + d2.to_digit(8).unwrap_or(0) * 8
                                        + d3.to_digit(8).unwrap_or(0);
                                    lit.push(char::from_u32(v).unwrap_or('\0'));
                                    i += 2;
                                    continue;
                                }
                            }
                        }
                        n = n * 10 + d2.to_digit(10).unwrap_or(0) as usize;
                        i += 1;
                    }
                }
                if n > re.ngroups {
                    return Err(terr(format!("invalid group reference {n}"), bs + 1, tpl));
                }
                if !lit.is_empty() {
                    out.push(Tpl::Lit(std::mem::take(&mut lit)));
                }
                out.push(Tpl::Group(n));
            }
            'a' => lit.push('\u{7}'),
            'b' => lit.push('\u{8}'),
            'f' => lit.push('\u{c}'),
            'n' => lit.push('\n'),
            'r' => lit.push('\r'),
            't' => lit.push('\t'),
            'v' => lit.push('\u{b}'),
            '\\' => lit.push('\\'),
            c if c.is_ascii_alphabetic() => return Err(terr(format!("bad escape \\{c}"), bs, tpl)),
            c => {
                lit.push('\\');
                lit.push(c);
            }
        }
    }
    if !lit.is_empty() {
        out.push(Tpl::Lit(lit));
    }
    Ok(out)
}

/// Aplica o template já analisado a um casamento (grupo não casado vira texto vazio).
pub fn expand_parts(parts: &[Tpl], chars: &[char], caps: &Captures) -> String {
    let mut out = String::new();
    for p in parts {
        match p {
            Tpl::Lit(s) => out.push_str(s),
            Tpl::Group(g) => {
                if let Some(Some((a, b))) = caps.spans.get(*g) {
                    out.extend(chars[*a..*b].iter());
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Pattern
// ---------------------------------------------------------------------------

pub struct PatternObj {
    me: Weak<PatternObj>,
    source: Value,
    text: String,
    /// Padrão em `bytes`: casa `bytes`, com classes ASCII, e devolve `bytes`.
    bytes: bool,
    pub regex: Rc<Regex>,
}

/// Compila `pattern` (sem cache) e registra o objeto para `as_pattern`.
pub fn compile_pattern(pattern: &str, flags: u32) -> PyResult<Rc<PatternObj>> {
    compile_pattern_kind(pattern, flags, false)
}

/// Como [`compile_pattern`]; `bytes` indica um padrão `bytes` (texto em latin-1).
fn compile_pattern_kind(pattern: &str, flags: u32, bytes: bool) -> PyResult<Rc<PatternObj>> {
    let chars: Vec<char> = pattern.chars().collect();
    let flags = if bytes { flags | eng::A } else { flags };
    let regex = eng::compile(&chars, flags).map_err(|e| to_py(&e, &chars))?;
    let rc = Rc::new_cyclic(|w| PatternObj {
        me: w.clone(),
        source: if bytes { rebyte(Value::str(pattern)) } else { Value::str(pattern) },
        text: pattern.to_string(),
        bytes,
        regex: Rc::new(regex),
    });
    let addr = Rc::as_ptr(&rc) as *const () as usize;
    REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        if r.len() > 2048 {
            r.retain(|_, w| w.strong_count() > 0);
        }
        r.insert(addr, Rc::downgrade(&rc));
    });
    Ok(rc)
}

fn as_pattern(v: &Value) -> Option<Rc<PatternObj>> {
    match v {
        Value::Ext(e) => {
            let addr = Rc::as_ptr(e) as *const () as usize;
            REGISTRY.with(|r| r.borrow().get(&addr).and_then(|w| w.upgrade()))
        }
        _ => None,
    }
}

/// Padrão a partir de `str` (com cache) ou de um `Pattern` já compilado.
fn get_pattern(v: &Value, flags: u32) -> PyResult<Rc<PatternObj>> {
    let unwrapped = match crate::vm::unwrap_payload(v) {
        // `bytearray` e `memoryview` casam como `bytes` (os grupos saem `bytes`).
        b @ (Value::ByteArray(_) | Value::Instance(_)) => b.bytes_like().map_or(b, Value::Bytes),
        other => other,
    };
    let v = &unwrapped;
    if let Some(p) = as_pattern(v) {
        if flags != 0 {
            return Err(exc("ValueError", "cannot process flags argument with a compiled pattern"));
        }
        return Ok(p);
    }
    match v {
        Value::Str(_) | Value::Bytes(_) => {
            let (text, bytes) = match v {
                Value::Str(s) => (s.as_str().to_string(), false),
                Value::Bytes(b) => (b.iter().map(|&c| c as char).collect::<String>(), true),
                _ => unreachable!(),
            };
            let key = (text, flags, bytes);
            if let Some(p) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
                return Ok(p);
            }
            let p = compile_pattern_kind(&key.0, flags, bytes)?;
            CACHE.with(|c| {
                let mut c = c.borrow_mut();
                if c.len() >= CACHE_MAX {
                    c.clear();
                }
                c.insert(key, p.clone());
            });
            Ok(p)
        }
        _ => Err(type_error("first argument must be string or compiled pattern")),
    }
}

impl PatternObj {
    fn this(&self) -> Rc<PatternObj> {
        self.me.upgrade().expect("o Pattern vive enquanto é chamado")
    }

    fn new_match(&self, sv: &Value, chars: &Rc<Vec<char>>, caps: Captures, pos: usize, endpos: usize) -> Value {
        Value::Ext(Rc::new(MatchObj {
            pattern: self.this(),
            string: sv.clone(),
            chars: chars.clone(),
            caps,
            pos,
            endpos,
        }))
    }

    /// `match`/`search`/`fullmatch`.
    pub fn exec_value(
        &self,
        string: &Value,
        pos: Option<&Value>,
        endpos: Option<&Value>,
        mode: Mode,
    ) -> PyResult<Value> {
        let (sv, chars) = want_text_for(string, self.bytes)?;
        let p = clamp_arg(pos, 0, chars.len())?;
        let e = clamp_arg(endpos, chars.len() as i64, chars.len())?;
        match self.regex.exec(&chars, p, e, mode, false) {
            Some(c) => Ok(self.new_match(&sv, &chars, c, p, e)),
            None => Ok(Value::None),
        }
    }

    pub fn findall_value(&self, string: &Value, pos: Option<&Value>, endpos: Option<&Value>) -> PyResult<Value> {
        let (_, chars) = want_text_for(string, self.bytes)?;
        let p = clamp_arg(pos, 0, chars.len())?;
        let e = clamp_arg(endpos, chars.len() as i64, chars.len())?;
        let items = findall_core(&self.regex, &chars, p, e);
        Ok(Value::list(if self.bytes { items.into_iter().map(rebyte).collect() } else { items }))
    }

    pub fn finditer_value(&self, string: &Value, pos: Option<&Value>, endpos: Option<&Value>) -> PyResult<Value> {
        let (sv, chars) = want_text_for(string, self.bytes)?;
        let p = clamp_arg(pos, 0, chars.len())?;
        let e = clamp_arg(endpos, chars.len() as i64, chars.len())?;
        Ok(Value::Ext(Rc::new(FinditerObj {
            pattern: self.this(),
            string: sv,
            chars,
            state: RefCell::new(IterState::new(p, e)),
            pos: p,
            endpos: e,
        })))
    }

    /// `sub` (`want_n = false`) e `subn` (`want_n = true`).
    pub fn sub_value(
        &self,
        vm: &mut Vm,
        repl: &Value,
        string: &Value,
        count: Option<&Value>,
        want_n: bool,
    ) -> PyResult<Value> {
        let (sv, chars) = want_text_for(string, self.bytes)?;
        let count = match count {
            None | Some(Value::None) => 0,
            Some(x) => want_int(x)?,
        };
        let finish = |s: Value, n: usize| -> Value {
            if want_n {
                Value::tuple(vec![s, Value::Int(n as i64)])
            } else {
                s
            }
        };
        let callable = matches!(repl, Value::Function(_) | Value::Builtin(_) | Value::NativeFn(_) | Value::Bound(_));
        let parts: Option<Vec<Tpl>> = if callable {
            None
        } else {
            match repl {
                Value::Str(_) if self.bytes => {
                    return Err(type_error("expected a bytes-like object, str found"));
                }
                Value::Str(s) => {
                    let t: Vec<char> = s.as_str().chars().collect();
                    Some(parse_template(&t, &self.regex)?)
                }
                Value::Bytes(b) if self.bytes => {
                    let t: Vec<char> = b.iter().map(|&c| c as char).collect();
                    Some(parse_template(&t, &self.regex)?)
                }
                other => {
                    return Err(type_error(format!(
                        "expected string or bytes-like object, got '{}'",
                        other.type_name()
                    )));
                }
            }
        };
        if count < 0 {
            return Ok(finish(sv, 0));
        }
        let pat = self.this();
        let (out, n) = sub_core(&self.regex, &chars, count as usize, |caps| match &parts {
            Some(p) => Ok(expand_parts(p, chars.as_slice(), caps)),
            None => {
                let m = pat.new_match(&sv, &chars, caps.clone(), 0, chars.len());
                match vm.call_value(repl, vec![m], Vec::new())? {
                    Value::Str(s) if !pat.bytes => Ok(s.as_str().to_string()),
                    Value::Bytes(b) if pat.bytes => Ok(b.iter().map(|&c| c as char).collect()),
                    other => Err(type_error(format!("expected str instance, {} found", other.type_name()))),
                }
            }
        })?;
        Ok(finish(out_text(self.bytes, out), n))
    }

    pub fn split_value(&self, string: &Value, maxsplit: Option<&Value>) -> PyResult<Value> {
        let (_, chars) = want_text_for(string, self.bytes)?;
        let max = match maxsplit {
            None | Some(Value::None) => 0,
            Some(x) => want_int(x)?,
        };
        if max < 0 {
            return Ok(Value::list(vec![out_text(self.bytes, slice_string(&chars, 0, chars.len()))]));
        }
        let items = split_core(&self.regex, &chars, max as usize);
        Ok(Value::list(
            items
                .into_iter()
                .map(|o| match o {
                    Some(s) => out_text(self.bytes, s),
                    None => Value::None,
                })
                .collect(),
        ))
    }
}

const PATTERN_FLAG_NAMES: &[(u32, &str)] = &[
    (eng::I, "re.IGNORECASE"),
    (eng::L, "re.LOCALE"),
    (eng::M, "re.MULTILINE"),
    (eng::S, "re.DOTALL"),
    (eng::U, "re.UNICODE"),
    (eng::X, "re.VERBOSE"),
    (128, "re.DEBUG"),
    (eng::A, "re.ASCII"),
];

impl ExtObject for PatternObj {
    fn type_name(&self) -> &'static str {
        "Pattern"
    }

    fn repr(&self) -> String {
        let shown: String = if self.text.chars().count() > 200 { self.text.chars().take(200).collect() } else { self.text.clone() };
        let mut rest = self.regex.flags & !eng::U & !(if self.bytes { eng::A } else { 0 });
        let mut parts: Vec<String> = Vec::new();
        for (bit, name) in PATTERN_FLAG_NAMES {
            if rest & *bit != 0 {
                parts.push((*name).to_string());
                rest &= !*bit;
            }
        }
        if rest != 0 {
            parts.push(format!("0x{rest:x}"));
        }
        let r = repr(&if self.bytes { rebyte(Value::str(shown)) } else { Value::str(shown) });
        if parts.is_empty() {
            format!("re.compile({r})")
        } else {
            format!("re.compile({r}, {})", parts.join("|"))
        }
    }

    fn methods(&self) -> &'static [&'static str] {
        &["match", "search", "fullmatch", "findall", "finditer", "sub", "subn", "split"]
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "pattern" => Some(Ok(self.source.clone())),
            "flags" => Some(Ok(Value::Int(i64::from(if self.bytes { self.regex.flags & !eng::A } else { self.regex.flags })))),
            "groups" => Some(Ok(Value::Int(self.regex.ngroups as i64))),
            "groupindex" => {
                let mut d = Dict::new();
                for (n, k) in &self.regex.group_names {
                    if let Err(e) = d.set(Value::str(n.clone()), Value::Int(*k as i64)) {
                        return Some(Err(e.into()));
                    }
                }
                Some(Ok(Value::dict(d)))
            }
            _ => None,
        }
    }

    fn call_method(&self, vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        match name {
            "match" | "search" | "fullmatch" | "findall" | "finditer" => {
                let a = bind(name, args, kw, &["string", "pos", "endpos"], 1)?;
                let (s, pos, endpos) = (&arg(&a, 0), a[1].as_ref(), a[2].as_ref());
                match name {
                    "findall" => self.findall_value(s, pos, endpos),
                    "finditer" => self.finditer_value(s, pos, endpos),
                    "match" => self.exec_value(s, pos, endpos, Mode::Match),
                    "search" => self.exec_value(s, pos, endpos, Mode::Search),
                    _ => self.exec_value(s, pos, endpos, Mode::Fullmatch),
                }
            }
            "sub" | "subn" => {
                let a = bind(name, args, kw, &["repl", "string", "count"], 2)?;
                self.sub_value(vm, &arg(&a, 0), &arg(&a, 1), a[2].as_ref(), name == "subn")
            }
            "split" => {
                let a = bind(name, args, kw, &["string", "maxsplit"], 1)?;
                self.split_value(&arg(&a, 0), a[1].as_ref())
            }
            _ => Err(crate::object::no_attribute("re.Pattern", name)),
        }
    }
}

// ---------------------------------------------------------------------------
// finditer
// ---------------------------------------------------------------------------

struct FinditerObj {
    pattern: Rc<PatternObj>,
    string: Value,
    chars: Rc<Vec<char>>,
    state: RefCell<IterState>,
    pos: usize,
    endpos: usize,
}

impl ExtObject for FinditerObj {
    fn type_name(&self) -> &'static str {
        "callable_iterator"
    }

    fn is_iterable(&self) -> bool {
        true
    }

    fn iter_next(&self) -> PyResult<Option<Value>> {
        let mut st = self.state.borrow_mut();
        match st.next(&self.pattern.regex, &self.chars) {
            None => Ok(None),
            Some(c) => Ok(Some(self.pattern.new_match(&self.string, &self.chars, c, self.pos, self.endpos))),
        }
    }
}

// ---------------------------------------------------------------------------
// Match
// ---------------------------------------------------------------------------

struct MatchObj {
    pattern: Rc<PatternObj>,
    string: Value,
    chars: Rc<Vec<char>>,
    caps: Captures,
    pos: usize,
    endpos: usize,
}

impl MatchObj {
    /// O texto do grupo `k`, ou `default` quando ele não participou.
    fn group_or(&self, k: usize, default: &Value) -> Value {
        match self.caps.spans[k] {
            Some(_) => self.group_value(k),
            None => default.clone(),
        }
    }

    fn group_value(&self, k: usize) -> Value {
        match self.caps.spans.get(k).copied().flatten() {
            Some((a, b)) => out_text(self.pattern.bytes, slice_string(&self.chars, a, b)),
            None => Value::None,
        }
    }

    /// Índice de grupo a partir de `int` ou nome.
    fn resolve(&self, v: &Value) -> PyResult<usize> {
        let n = self.pattern.regex.ngroups;
        match v {
            Value::Int(i) => {
                if *i >= 0 && (*i as usize) <= n {
                    Ok(*i as usize)
                } else {
                    Err(index_error())
                }
            }
            Value::Bool(b) => {
                let i = usize::from(*b);
                if i <= n {
                    Ok(i)
                } else {
                    Err(index_error())
                }
            }
            Value::Str(s) => self.pattern.regex.group_index(s.as_str()).ok_or_else(index_error),
            _ => Err(index_error()),
        }
    }

    fn span_value(&self, k: usize) -> (i64, i64) {
        match self.caps.spans.get(k).copied().flatten() {
            Some((a, b)) => (a as i64, b as i64),
            None => (-1, -1),
        }
    }
}

impl ExtObject for MatchObj {
    fn type_name(&self) -> &'static str {
        "Match"
    }

    fn repr(&self) -> String {
        let (a, b) = self.caps.spans[0].unwrap_or((0, 0));
        let text = repr(&out_text(self.pattern.bytes, slice_string(&self.chars, a, b)));
        format!("<re.Match object; span=({a}, {b}), match={text}>")
    }

    fn methods(&self) -> &'static [&'static str] {
        &["group", "groups", "groupdict", "start", "end", "span", "expand"]
    }

    fn getattr(&self, _vm: &mut Vm, name: &str) -> Option<PyResult<Value>> {
        match name {
            "string" => Some(Ok(self.string.clone())),
            "re" => Some(Ok(Value::Ext(self.pattern.clone()))),
            "pos" => Some(Ok(Value::Int(self.pos as i64))),
            "endpos" => Some(Ok(Value::Int(self.endpos as i64))),
            "lastindex" => Some(Ok(match self.caps.lastindex {
                Some(k) => Value::Int(k as i64),
                None => Value::None,
            })),
            "lastgroup" => Some(Ok(match self.caps.lastindex {
                Some(k) => match self.pattern.regex.group_names.iter().find(|(_, g)| *g == k) {
                    Some((n, _)) => Value::str(n.clone()),
                    None => Value::None,
                },
                None => Value::None,
            })),
            "regs" => Some(Ok(Value::tuple(
                (0..=self.pattern.regex.ngroups)
                    .map(|k| {
                        let (a, b) = self.span_value(k);
                        Value::tuple(vec![Value::Int(a), Value::Int(b)])
                    })
                    .collect(),
            ))),
            _ => None,
        }
    }

    fn call_method(&self, _vm: &mut Vm, name: &str, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
        let ngroups = self.pattern.regex.ngroups;
        match name {
            "group" => {
                no_kwargs("group", &kw)?;
                if args.is_empty() {
                    return Ok(self.group_value(0));
                }
                if args.len() == 1 {
                    let k = self.resolve(&args[0])?;
                    return Ok(self.group_value(k));
                }
                let mut out = Vec::with_capacity(args.len());
                for a in &args {
                    let k = self.resolve(a)?;
                    out.push(self.group_value(k));
                }
                Ok(Value::tuple(out))
            }
            "groups" => {
                let a = bind("groups", args, kw, &["default"], 0)?;
                let default = arg(&a, 0);
                Ok(Value::tuple((1..=ngroups).map(|k| self.group_or(k, &default)).collect()))
            }
            "groupdict" => {
                let a = bind("groupdict", args, kw, &["default"], 0)?;
                let default = arg(&a, 0);
                let mut d = Dict::new();
                for (n, k) in &self.pattern.regex.group_names {
                    d.set(Value::str(n.clone()), self.group_or(*k, &default))?;
                }
                Ok(Value::dict(d))
            }
            "start" | "end" | "span" => {
                let a = bind(name, args, kw, &["group"], 0)?;
                let k = match &a[0] {
                    None => 0,
                    Some(v) => self.resolve(v)?,
                };
                let (s, e) = self.span_value(k);
                Ok(match name {
                    "start" => Value::Int(s),
                    "end" => Value::Int(e),
                    _ => Value::tuple(vec![Value::Int(s), Value::Int(e)]),
                })
            }
            "expand" => {
                no_kwargs("expand", &kw)?;
                exactly("expand", &args, 1)?;
                let t: Vec<char> = match &args[0] {
                    Value::Str(s) if !self.pattern.bytes => s.as_str().chars().collect(),
                    Value::Bytes(b) if self.pattern.bytes => b.iter().map(|&c| c as char).collect(),
                    other => {
                        return Err(type_error(format!(
                            "expected string or bytes-like object, got '{}'",
                            other.type_name()
                        )));
                    }
                };
                let parts = parse_template(&t, &self.pattern.regex)?;
                Ok(out_text(self.pattern.bytes, expand_parts(&parts, &self.chars, &self.caps)))
            }
            _ => Err(crate::object::no_attribute("re.Match", name)),
        }
    }

    fn getitem(&self, key: &Value) -> Option<PyResult<Value>> {
        Some(self.resolve(key).map(|k| self.group_value(k)))
    }
}

// ---------------------------------------------------------------------------
// Funções do módulo
// ---------------------------------------------------------------------------

fn f_compile(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("compile", args, kw, &["pattern", "flags"], 1)?;
    let p = get_pattern(&arg(&a, 0), flags_of(&a[1])?)?;
    Ok(Value::Ext(p))
}

fn mod_exec(args: Vec<Value>, kw: Kw, fname: &str, mode: Mode) -> PyResult<Value> {
    let a = bind(fname, args, kw, &["pattern", "string", "flags"], 2)?;
    let p = get_pattern(&arg(&a, 0), flags_of(&a[2])?)?;
    p.exec_value(&arg(&a, 1), None, None, mode)
}

fn f_match(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    mod_exec(args, kw, "match", Mode::Match)
}

fn f_search(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    mod_exec(args, kw, "search", Mode::Search)
}

fn f_fullmatch(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    mod_exec(args, kw, "fullmatch", Mode::Fullmatch)
}

fn f_findall(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("findall", args, kw, &["pattern", "string", "flags"], 2)?;
    let p = get_pattern(&arg(&a, 0), flags_of(&a[2])?)?;
    p.findall_value(&arg(&a, 1), None, None)
}

fn f_finditer(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("finditer", args, kw, &["pattern", "string", "flags"], 2)?;
    let p = get_pattern(&arg(&a, 0), flags_of(&a[2])?)?;
    p.finditer_value(&arg(&a, 1), None, None)
}

fn mod_sub(vm: &mut Vm, args: Vec<Value>, kw: Kw, fname: &str, want_n: bool) -> PyResult<Value> {
    let a = bind(fname, args, kw, &["pattern", "repl", "string", "count", "flags"], 3)?;
    let p = get_pattern(&arg(&a, 0), flags_of(&a[4])?)?;
    p.sub_value(vm, &arg(&a, 1), &arg(&a, 2), a[3].as_ref(), want_n)
}

fn f_sub(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    mod_sub(vm, args, kw, "sub", false)
}

fn f_subn(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    mod_sub(vm, args, kw, "subn", true)
}

fn f_split(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("split", args, kw, &["pattern", "string", "maxsplit", "flags"], 2)?;
    let p = get_pattern(&arg(&a, 0), flags_of(&a[3])?)?;
    p.split_value(&arg(&a, 1), a[2].as_ref())
}

fn f_escape(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("escape", args, kw, &["pattern"], 1)?;
    match arg(&a, 0) {
        Value::Str(s) => Ok(Value::str(escape_str(s.as_str()))),
        Value::Bytes(b) => {
            let s: String = b.iter().map(|&c| c as char).collect();
            Ok(rebyte(Value::str(escape_str(&s))))
        }
        other => Err(type_error(format!("expected str instance, {} found", other.type_name()))),
    }
}

fn f_purge(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("purge", &kw)?;
    exactly("purge", &args, 0)?;
    CACHE.with(|c| c.borrow_mut().clear());
    Ok(Value::None)
}

/// Constrói o módulo `re`.
pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("_re")
        .func("compile", f_compile)
        .func("match", f_match)
        .func("search", f_search)
        .func("fullmatch", f_fullmatch)
        .func("findall", f_findall)
        .func("finditer", f_finditer)
        .func("sub", f_sub)
        .func("subn", f_subn)
        .func("split", f_split)
        .func("escape", f_escape)
        .func("purge", f_purge)
        .value("error", Value::Builtin("re.PatternError"))
        .value("NOFLAG", Value::Int(0))
        .value("I", Value::Int(i64::from(eng::I)))
        .value("IGNORECASE", Value::Int(i64::from(eng::I)))
        .value("L", Value::Int(i64::from(eng::L)))
        .value("LOCALE", Value::Int(i64::from(eng::L)))
        .value("M", Value::Int(i64::from(eng::M)))
        .value("MULTILINE", Value::Int(i64::from(eng::M)))
        .value("S", Value::Int(i64::from(eng::S)))
        .value("DOTALL", Value::Int(i64::from(eng::S)))
        .value("U", Value::Int(i64::from(eng::U)))
        .value("UNICODE", Value::Int(i64::from(eng::U)))
        .value("X", Value::Int(i64::from(eng::X)))
        .value("VERBOSE", Value::Int(i64::from(eng::X)))
        .value("A", Value::Int(i64::from(eng::A)))
        .value("ASCII", Value::Int(i64::from(eng::A)))
        .value("DEBUG", Value::Int(128))
        .build()
}

// ---------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn cs(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    fn pat(p: &str, fl: u32) -> Rc<PatternObj> {
        match compile_pattern(p, fl) {
            Ok(r) => r,
            Err(e) => panic!("padrão {p:?} recusado: {}", e.msg),
        }
    }

    fn sub_tpl(p: &str, tpl: &str, text: &str, count: usize) -> (String, usize) {
        let pt = pat(p, 0);
        let parts = match parse_template(&cs(tpl), &pt.regex) {
            Ok(x) => x,
            Err(e) => panic!("template recusado: {}", e.msg),
        };
        let chars = cs(text);
        match sub_core(&pt.regex, &chars, count, |c| Ok(expand_parts(&parts, &chars, c))) {
            Ok(r) => r,
            Err(e) => panic!("sub falhou: {}", e.msg),
        }
    }

    fn split(p: &str, text: &str, max: usize) -> Vec<String> {
        let pt = pat(p, 0);
        split_core(&pt.regex, &cs(text), max)
            .into_iter()
            .map(|o| o.unwrap_or_else(|| "<None>".to_string()))
            .collect()
    }

    fn findall(p: &str, text: &str) -> String {
        let pt = pat(p, 0);
        let t = cs(text);
        repr(&Value::list(findall_core(&pt.regex, &t, 0, t.len())))
    }

    #[test]
    fn findall_regras_de_grupos() {
        assert_eq!(findall("\\bf[a-z]*", "which foot or hand fell fastest"), "['foot', 'fell', 'fastest']");
        assert_eq!(findall("(\\w)(\\d)", "a1 b2"), "[('a', '1'), ('b', '2')]");
        assert_eq!(findall("(a)|b", "ab"), "['a', '']");
        assert_eq!(findall("a*", "baac"), "['', 'aa', '', '']");
        assert_eq!(findall("x", "abc"), "[]");
    }

    #[test]
    fn sub_com_templates() {
        assert_eq!(sub_tpl("(\\w+) (\\w+)", "\\2 \\1", "hello world", 0), ("world hello".to_string(), 1));
        assert_eq!(sub_tpl("(?P<a>x)", "[\\g<a>]", "axb", 0).0, "a[x]b");
        assert_eq!(sub_tpl("x", "<\\g<0>>", "axb", 0).0, "a<x>b");
        assert_eq!(sub_tpl("(x)", "\\g<1>\\g<1>", "axb", 0).0, "axxb");
        assert_eq!(sub_tpl("a", "\\n", "a", 0).0, "\n");
        assert_eq!(sub_tpl("a", "\\.", "a", 0).0, "\\.");
        assert_eq!(sub_tpl("a", "\\\\", "a", 0).0, "\\");
        assert_eq!(sub_tpl("x", "-", "axbxc", 1).0, "a-bxc");
        assert_eq!(sub_tpl("x", "-", "axbxc", 0), ("a-b-c".to_string(), 2));
        assert_eq!(sub_tpl("(a)|(b)", "[\\2]", "a", 0).0, "[]");
        assert_eq!(sub_tpl("x*", "-", "abxd", 0).0, "-a-b--d-");
        assert_eq!(sub_tpl("a", "b", "xyz", 0), ("xyz".to_string(), 0));
    }

    #[test]
    fn sub_com_funcao() {
        let pt = pat("\\d+", 0);
        let chars = cs("a1b22");
        let r = sub_core(&pt.regex, &chars, 0, |c| {
            let (a, b) = c.spans[0].unwrap();
            let n: i64 = slice_string(&chars, a, b).parse().unwrap();
            Ok((n * 2).to_string())
        });
        match r {
            Ok((s, n)) => {
                assert_eq!(s, "a2b44");
                assert_eq!(n, 2);
            }
            Err(e) => panic!("{}", e.msg),
        }
    }

    #[test]
    fn erros_de_template() {
        let pt = pat("(a)", 0);
        let e = |t: &str| match parse_template(&cs(t), &pt.regex) {
            Ok(_) => panic!("template {t:?} deveria falhar"),
            Err(e) => (e.kind, e.msg),
        };
        assert_eq!(e("\\q"), ("re.PatternError", "bad escape \\q at position 0".to_string()));
        assert_eq!(e("\\3"), ("re.PatternError", "invalid group reference 3 at position 1".to_string()));
        assert_eq!(e("\\g<x>"), ("IndexError", "unknown group name 'x'".to_string()));
    }

    #[test]
    fn split_como_o_cpython() {
        assert_eq!(split("\\W+", "Words, words, words.", 0), vec!["Words", "words", "words", ""]);
        assert_eq!(split("(\\W+)", "Words, words, words.", 0), vec!["Words", ", ", "words", ", ", "words", ".", ""]);
        assert_eq!(split("\\W+", "Words, words, words.", 1), vec!["Words", "words, words."]);
        assert_eq!(split("(\\W+)", "...words, words...", 0), vec!["", "...", "words", ", ", "words", "...", ""]);
        assert_eq!(split("x*", "axbc", 0), vec!["", "a", "", "b", "c", ""]);
        assert_eq!(split("\\b", "Words, words, words.", 0), vec!["", "Words", ", ", "words", ", ", "words", "."]);
        assert_eq!(split("(a)|b", "xbyaz", 0), vec!["x", "<None>", "y", "a", "z"]);
        assert_eq!(split(",", "abc", 0), vec!["abc"]);
    }

    #[test]
    fn escape() {
        assert_eq!(escape_str("a.b-c"), "a\\.b\\-c");
        assert_eq!(escape_str("abc_123"), "abc_123");
        assert_eq!(escape_str("a b\n"), "a\\ b\\\n");
        assert_eq!(escape_str("(x)[y]{z}*+?|^$\\&~#"), "\\(x\\)\\[y\\]\\{z\\}\\*\\+\\?\\|\\^\\$\\\\\\&\\~\\#");
    }

    #[test]
    fn repr_do_pattern() {
        assert_eq!(pat("abc", 0).repr(), "re.compile('abc')");
        assert_eq!(pat("abc", eng::I).repr(), "re.compile('abc', re.IGNORECASE)");
        assert_eq!(pat("abc", eng::I | eng::M).repr(), "re.compile('abc', re.IGNORECASE|re.MULTILINE)");
        assert_eq!(pat("a", eng::A).repr(), "re.compile('a', re.ASCII)");
        assert_eq!(pat("a'b", 0).repr(), "re.compile(\"a'b\")");
    }

    #[test]
    fn match_repr_e_getitem() {
        let p = pat("(?P<a>\\d+)-(\\d+)(x)?", 0);
        let m = match p.exec_value(&Value::str("z12-34"), None, None, Mode::Search) {
            Ok(m) => m,
            Err(e) => panic!("{}", e.msg),
        };
        let e = match &m {
            Value::Ext(e) => e.clone(),
            _ => panic!("esperava um Match"),
        };
        assert_eq!(e.type_name(), "Match");
        assert_eq!(e.repr(), "<re.Match object; span=(1, 6), match='12-34'>");
        let get = |k: Value| match e.getitem(&k) {
            Some(Ok(v)) => repr(&v),
            Some(Err(x)) => format!("{}: {}", x.kind, x.msg),
            None => "sem getitem".to_string(),
        };
        assert_eq!(get(Value::Int(0)), "'12-34'");
        assert_eq!(get(Value::Int(1)), "'12'");
        assert_eq!(get(Value::str("a")), "'12'");
        assert_eq!(get(Value::Int(2)), "'34'");
        assert_eq!(get(Value::Int(3)), "None");
        assert_eq!(get(Value::Int(4)), "IndexError: no such group");
        assert_eq!(get(Value::str("nope")), "IndexError: no such group");
    }

    #[test]
    fn match_none_e_modos() {
        let p = pat("b+", 0);
        let r = |mode: Mode, s: &str| match p.exec_value(&Value::str(s), None, None, mode) {
            Ok(Value::None) => "None".to_string(),
            Ok(Value::Ext(e)) => e.repr(),
            Ok(_) => "?".to_string(),
            Err(e) => e.msg,
        };
        assert_eq!(r(Mode::Match, "abb"), "None");
        assert_eq!(r(Mode::Search, "abb"), "<re.Match object; span=(1, 3), match='bb'>");
        assert_eq!(r(Mode::Fullmatch, "bbb"), "<re.Match object; span=(0, 3), match='bbb'>");
        assert_eq!(r(Mode::Fullmatch, "bbbc"), "None");
        let pos = Value::Int(1);
        let m = p.exec_value(&Value::str("abb"), Some(&pos), None, Mode::Match);
        assert!(matches!(m, Ok(Value::Ext(_))));
    }

    #[test]
    fn tipos_invalidos() {
        match p_text(&Value::Int(1)) {
            Err(e) => assert_eq!(e.msg, "expected string or bytes-like object, got 'int'"),
            Ok(_) => panic!("deveria falhar"),
        }
        match get_pattern(&Value::Int(1), 0) {
            Err(e) => {
                assert_eq!(e.kind, "TypeError");
                assert_eq!(e.msg, "first argument must be string or compiled pattern");
            }
            Ok(_) => panic!("deveria falhar"),
        }
        match get_pattern(&Value::str("["), 0) {
            Err(e) => {
                assert_eq!(e.kind, "re.PatternError");
                assert_eq!(e.msg, "unterminated character set at position 0");
            }
            Ok(_) => panic!("deveria falhar"),
        }
    }

    fn p_text(v: &Value) -> PyResult<(Value, Rc<Vec<char>>)> {
        want_text(v)
    }

    #[test]
    fn pattern_compilado_e_cache() {
        let a = match get_pattern(&Value::str("ab+"), 0) {
            Ok(p) => p,
            Err(e) => panic!("{}", e.msg),
        };
        let b = match get_pattern(&Value::str("ab+"), 0) {
            Ok(p) => p,
            Err(e) => panic!("{}", e.msg),
        };
        assert!(Rc::ptr_eq(&a, &b));
        let as_value = Value::Ext(a.clone());
        assert!(as_pattern(&as_value).is_some());
        assert!(as_pattern(&Value::Int(1)).is_none());
        match get_pattern(&as_value, 2) {
            Err(e) => assert_eq!(e.msg, "cannot process flags argument with a compiled pattern"),
            Ok(_) => panic!("deveria falhar"),
        }
        assert!(get_pattern(&as_value, 0).is_ok());
    }

    #[test]
    fn finditer_itera() {
        let p = pat("\\d", 0);
        let it = match p.finditer_value(&Value::str("a1b2c3"), None, None) {
            Ok(Value::Ext(e)) => e,
            _ => panic!("esperava o iterador"),
        };
        assert!(it.is_iterable());
        let mut spans = Vec::new();
        loop {
            match it.iter_next() {
                Ok(Some(Value::Ext(m))) => spans.push(m.repr()),
                Ok(Some(_)) => panic!("item inesperado"),
                Ok(None) => break,
                Err(e) => panic!("{}", e.msg),
            }
        }
        assert_eq!(
            spans,
            vec![
                "<re.Match object; span=(1, 2), match='1'>",
                "<re.Match object; span=(3, 4), match='2'>",
                "<re.Match object; span=(5, 6), match='3'>",
            ]
        );
    }

    #[test]
    fn indices_em_pontos_de_codigo() {
        let p = pat("b", 0);
        let m = match p.exec_value(&Value::str("\u{e9}\u{1F600}b"), None, None, Mode::Search) {
            Ok(Value::Ext(e)) => e,
            _ => panic!("esperava Match"),
        };
        assert_eq!(m.repr(), "<re.Match object; span=(2, 3), match='b'>");
    }
}
