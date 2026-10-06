//! Métodos de `str` (`Objects/unicodeobject.c`).
//!
//! Índices e comprimentos contam código-pontos, como no CPython. As classificações Unicode
//! (`isalpha`, `isdecimal`...) usam as tabelas da `std` do Rust, com aproximações onde a `std` não
//! expõe a categoria geral (ver os comentários de cada predicado).

use crate::format::str_format;
use crate::native_util::{bind, exactly, no_kwargs, value_error, want_int};
use crate::object::{is_printable, Kw, NativeFnPtr, PyStr, Value};
use crate::vm::{exc, iterate, type_error, PyResult, Vm};

// ---------------------------------------------------------------------------------------------
// Utilidades
// ---------------------------------------------------------------------------------------------

/// O receptor (`args[0]`) como `str`.
fn me(args: &[Value]) -> PyResult<&PyStr> {
    match args.first() {
        Some(Value::Str(s)) => Ok(&**s),
        _ => Err(type_error("descriptor requires a 'str' object")),
    }
}

/// Argumento obrigatório já ligado por `bind`.
fn req(b: &[Option<Value>], i: usize) -> PyResult<&Value> {
    match b.get(i) {
        Some(Some(v)) => Ok(v),
        _ => Err(type_error("missing required argument")),
    }
}

/// `sub` de `find`/`count`...: tem de ser `str`.
fn sub_str(v: &Option<Value>) -> PyResult<&str> {
    match v {
        Some(Value::Str(s)) => Ok(s.as_str()),
        Some(o) => Err(type_error(format!("must be str, not {}", o.type_name()))),
        None => Err(type_error("missing required argument")),
    }
}

fn opt_index(v: &Option<Value>) -> PyResult<Option<i64>> {
    match v {
        None | Some(Value::None) => Ok(None),
        Some(Value::Int(i)) => Ok(Some(*i)),
        Some(Value::Bool(b)) => Ok(Some(i64::from(*b))),
        Some(_) => Err(type_error("slice indices must be integers or None or have an __index__ method")),
    }
}

/// `ADJUST_INDICES` do CPython: normaliza `start`/`end` (negativos contam do fim, `end` recorta ao
/// tamanho). `start` pode ficar além de `end`.
fn adjust(len: usize, start: Option<i64>, end: Option<i64>) -> (usize, usize) {
    let len_i = len as i64;
    let mut e = end.unwrap_or(len_i);
    if e > len_i {
        e = len_i;
    } else if e < 0 {
        e += len_i;
        if e < 0 {
            e = 0;
        }
    }
    let mut s = start.unwrap_or(0);
    if s < 0 {
        s += len_i;
        if s < 0 {
            s = 0;
        }
    }
    (s as usize, e as usize)
}

fn no_args(name: &str, args: &[Value], kw: &Kw) -> PyResult<()> {
    no_kwargs(name, kw)?;
    exactly(name, &args[1..], 0)
}

/// Espaço no sentido de `str.isspace` (`Py_UNICODE_ISSPACE`).
fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

fn title_char(c: char) -> String {
    if c == 'ß' {
        "Ss".to_string()
    } else {
        c.to_uppercase().collect()
    }
}

fn fill_char(name: &str, v: &Option<Value>) -> PyResult<char> {
    match v {
        None => Ok(' '),
        Some(Value::Str(s)) if s.len() == 1 => Ok(s.as_str().chars().next().unwrap_or(' ')),
        Some(Value::Str(_)) => Err(type_error("The fill character must be exactly one character long")),
        Some(o) => Err(type_error(format!("{name}() argument 2 must be str, not {}", o.type_name()))),
    }
}

// ---------------------------------------------------------------------------------------------
// Caixa
// ---------------------------------------------------------------------------------------------

fn upper(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_args("upper", &args, &kw)?;
    Ok(Value::str(me(&args)?.as_str().to_uppercase()))
}

fn lower(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_args("lower", &args, &kw)?;
    Ok(Value::str(me(&args)?.as_str().to_lowercase()))
}

fn capitalize(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_args("capitalize", &args, &kw)?;
    let t = me(&args)?.as_str();
    let mut chars = t.chars();
    let mut out = String::with_capacity(t.len());
    if let Some(first) = chars.next() {
        out.push_str(&title_char(first));
        out.push_str(&chars.as_str().to_lowercase());
    }
    Ok(Value::str(out))
}

fn casefold(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_args("casefold", &args, &kw)?;
    let mut out = String::new();
    for c in me(&args)?.as_str().chars() {
        match c {
            'ß' => out.push_str("ss"),
            'ς' => out.push('σ'),
            _ => out.extend(c.to_lowercase()),
        }
    }
    Ok(Value::str(out))
}

fn swapcase(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_args("swapcase", &args, &kw)?;
    let mut out = String::new();
    for c in me(&args)?.as_str().chars() {
        if c.is_uppercase() {
            out.extend(c.to_lowercase());
        } else if c.is_lowercase() {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
    }
    Ok(Value::str(out))
}

fn title(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_args("title", &args, &kw)?;
    let mut out = String::new();
    let mut prev_cased = false;
    for c in me(&args)?.as_str().chars() {
        if prev_cased {
            out.extend(c.to_lowercase());
        } else {
            out.push_str(&title_char(c));
        }
        prev_cased = c.is_lowercase() || c.is_uppercase();
    }
    Ok(Value::str(out))
}

// ---------------------------------------------------------------------------------------------
// Busca
// ---------------------------------------------------------------------------------------------

fn find_impl(args: Vec<Value>, kw: Kw, name: &str, reverse: bool, raise: bool) -> PyResult<Value> {
    let s = me(&args)?;
    let b = bind(name, args[1..].to_vec(), kw, &["sub", "start", "end"], 1)?;
    let sub = sub_str(&b[0])?;
    let (st, en) = adjust(s.len(), opt_index(&b[1])?, opt_index(&b[2])?);
    let found = if st > en {
        None
    } else {
        let sl = s.slice(st, en);
        let off = if reverse { sl.rfind(sub) } else { sl.find(sub) };
        off.map(|o| st + sl[..o].chars().count())
    };
    match found {
        Some(i) => Ok(Value::Int(i as i64)),
        None if raise => Err(value_error("substring not found")),
        None => Ok(Value::Int(-1)),
    }
}

fn find(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    find_impl(args, kw, "find", false, false)
}

fn rfind(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    find_impl(args, kw, "rfind", true, false)
}

fn index(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    find_impl(args, kw, "index", false, true)
}

fn rindex(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    find_impl(args, kw, "rindex", true, true)
}

fn count(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = me(&args)?;
    let b = bind("count", args[1..].to_vec(), kw, &["sub", "start", "end"], 1)?;
    let sub = sub_str(&b[0])?;
    let (st, en) = adjust(s.len(), opt_index(&b[1])?, opt_index(&b[2])?);
    let n = if st > en {
        0
    } else if sub.is_empty() {
        en - st + 1
    } else {
        s.slice(st, en).matches(sub).count()
    };
    Ok(Value::Int(n as i64))
}

fn affix(args: Vec<Value>, kw: Kw, name: &str, ends: bool) -> PyResult<Value> {
    let s = me(&args)?;
    let b = bind(name, args[1..].to_vec(), kw, &["prefix", "start", "end"], 1)?;
    let (st, en) = adjust(s.len(), opt_index(&b[1])?, opt_index(&b[2])?);
    if st > en {
        return Ok(Value::Bool(false));
    }
    let sl = s.slice(st, en);
    let test = |p: &str| if ends { sl.ends_with(p) } else { sl.starts_with(p) };
    match req(&b, 0)? {
        Value::Str(p) => Ok(Value::Bool(test(p.as_str()))),
        Value::Tuple(t) => {
            for item in t.iter() {
                match item {
                    Value::Str(p) => {
                        if test(p.as_str()) {
                            return Ok(Value::Bool(true));
                        }
                    }
                    other => {
                        return Err(type_error(format!(
                            "tuple for {name} must only contain str, not {}",
                            other.type_name()
                        )))
                    }
                }
            }
            Ok(Value::Bool(false))
        }
        other => Err(type_error(format!(
            "{name} first arg must be str or a tuple of str, not {}",
            other.type_name()
        ))),
    }
}

fn startswith(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    affix(args, kw, "startswith", false)
}

fn endswith(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    affix(args, kw, "endswith", true)
}

// ---------------------------------------------------------------------------------------------
// Alinhamento e preenchimento
// ---------------------------------------------------------------------------------------------

fn justify(args: Vec<Value>, kw: Kw, name: &str, mode: char) -> PyResult<Value> {
    let s = me(&args)?;
    let b = bind(name, args[1..].to_vec(), kw, &["width", "fillchar"], 1)?;
    let width = want_int(req(&b, 0)?)?;
    let fill = fill_char(name, &b[1])?;
    let len = s.len() as i64;
    if width <= len {
        return Ok(args[0].clone());
    }
    let marg = (width - len) as usize;
    let (l, r) = match mode {
        'l' => (0, marg),
        'r' => (marg, 0),
        _ => {
            let left = marg / 2 + (marg & (width as usize) & 1);
            (left, marg - left)
        }
    };
    let mut out = String::new();
    out.extend(std::iter::repeat_n(fill, l));
    out.push_str(s.as_str());
    out.extend(std::iter::repeat_n(fill, r));
    Ok(Value::str(out))
}

fn center(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    justify(args, kw, "center", 'c')
}

fn ljust(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    justify(args, kw, "ljust", 'l')
}

fn rjust(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    justify(args, kw, "rjust", 'r')
}

fn zfill(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = me(&args)?;
    let b = bind("zfill", args[1..].to_vec(), kw, &["width"], 1)?;
    let width = want_int(req(&b, 0)?)?;
    let len = s.len() as i64;
    if width <= len {
        return Ok(args[0].clone());
    }
    let t = s.as_str();
    let (sign, rest) = if t.starts_with('+') || t.starts_with('-') { t.split_at(1) } else { ("", t) };
    Ok(Value::str(format!("{sign}{}{rest}", "0".repeat((width - len) as usize))))
}

fn expandtabs(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = me(&args)?;
    let b = bind("expandtabs", args[1..].to_vec(), kw, &["tabsize"], 0)?;
    let tab = match &b[0] {
        None => 8,
        Some(v) => want_int(v)?,
    };
    let mut out = String::new();
    let mut col: i64 = 0;
    for c in s.as_str().chars() {
        match c {
            '\t' => {
                if tab > 0 {
                    let n = tab - col % tab;
                    for _ in 0..n {
                        out.push(' ');
                    }
                    col += n;
                }
            }
            '\n' | '\r' => {
                out.push(c);
                col = 0;
            }
            _ => {
                out.push(c);
                col += 1;
            }
        }
    }
    Ok(Value::str(out))
}

// ---------------------------------------------------------------------------------------------
// Predicados
// ---------------------------------------------------------------------------------------------

/// Letra no sentido de `str.isalpha` (categorias L*): a `std` também inclui os numerais romanos
/// (Nl), que ficam de fora aqui.
fn is_alpha(c: char) -> bool {
    c.is_alphabetic() && !c.is_numeric()
}

fn is_alnum(c: char) -> bool {
    c.is_alphabetic() || c.is_numeric()
}

/// Início de cada bloco de dez dígitos decimais (categoria Nd) que a `std` não separa de Nl/No.
const DECIMAL_STARTS: &[u32] = &[
    0x30, 0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66, 0xde6, 0xe50, 0xed0,
    0xf20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90, 0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620,
    0xa8d0, 0xa900, 0xa9d0, 0xa9f0, 0xaa50, 0xabf0, 0xff10,
];

fn is_decimal(c: char) -> bool {
    let v = c as u32;
    DECIMAL_STARTS.iter().any(|&s| v >= s && v < s + 10)
}

fn is_digit(c: char) -> bool {
    let v = c as u32;
    is_decimal(c)
        || matches!(v, 0xb2 | 0xb3 | 0xb9 | 0x2070 | 0x2074..=0x2079 | 0x2080..=0x2089)
        || matches!(v, 0x2460..=0x2468 | 0x2474..=0x247c | 0x2488..=0x2490 | 0x24ea | 0x2776..=0x277e)
}

fn is_ident_start(c: char) -> bool {
    c == '_' || is_alpha(c)
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || is_alnum(c)
}

fn all_nonempty(name: &str, args: &[Value], kw: &Kw, pred: fn(char) -> bool) -> PyResult<Value> {
    no_args(name, args, kw)?;
    let t = me(args)?.as_str();
    Ok(Value::Bool(!t.is_empty() && t.chars().all(pred)))
}

fn isalnum(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    all_nonempty("isalnum", &args, &kw, is_alnum)
}

fn isalpha(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    all_nonempty("isalpha", &args, &kw, is_alpha)
}

fn isdecimal(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    all_nonempty("isdecimal", &args, &kw, is_decimal)
}

fn isdigit(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    all_nonempty("isdigit", &args, &kw, is_digit)
}

fn isnumeric(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    all_nonempty("isnumeric", &args, &kw, char::is_numeric)
}

fn isspace(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    all_nonempty("isspace", &args, &kw, is_py_space)
}

fn isascii(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_args("isascii", &args, &kw)?;
    Ok(Value::Bool(me(&args)?.as_str().is_ascii()))
}

fn isprintable(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_args("isprintable", &args, &kw)?;
    Ok(Value::Bool(me(&args)?.as_str().chars().all(is_printable)))
}

fn isidentifier(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_args("isidentifier", &args, &kw)?;
    let mut chars = me(&args)?.as_str().chars();
    let ok = match chars.next() {
        Some(first) => is_ident_start(first) && chars.all(is_ident_continue),
        None => false,
    };
    Ok(Value::Bool(ok))
}

fn islower(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_args("islower", &args, &kw)?;
    let mut cased = false;
    for c in me(&args)?.as_str().chars() {
        if c.is_uppercase() {
            return Ok(Value::Bool(false));
        }
        if c.is_lowercase() {
            cased = true;
        }
    }
    Ok(Value::Bool(cased))
}

fn isupper(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_args("isupper", &args, &kw)?;
    let mut cased = false;
    for c in me(&args)?.as_str().chars() {
        if c.is_lowercase() {
            return Ok(Value::Bool(false));
        }
        if c.is_uppercase() {
            cased = true;
        }
    }
    Ok(Value::Bool(cased))
}

fn istitle(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_args("istitle", &args, &kw)?;
    let mut prev_cased = false;
    let mut any = false;
    for c in me(&args)?.as_str().chars() {
        if c.is_uppercase() {
            if prev_cased {
                return Ok(Value::Bool(false));
            }
            prev_cased = true;
            any = true;
        } else if c.is_lowercase() {
            if !prev_cased {
                return Ok(Value::Bool(false));
            }
            prev_cased = true;
            any = true;
        } else {
            prev_cased = false;
        }
    }
    Ok(Value::Bool(any))
}

// ---------------------------------------------------------------------------------------------
// join, strip, partition, replace
// ---------------------------------------------------------------------------------------------

fn join(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("join", &kw)?;
    exactly("join", &args[1..], 1)?;
    let sep = me(&args)?.as_str();
    let items = match &args[1] {
        Value::Int(_) | Value::Float(_) | Value::Bool(_) | Value::None => {
            return Err(type_error("can only join an iterable"))
        }
        v => iterate(v)?,
    };
    let mut out = String::new();
    for (i, item) in items.iter().enumerate() {
        match item {
            Value::Str(x) => {
                if i > 0 {
                    out.push_str(sep);
                }
                out.push_str(x.as_str());
            }
            other => {
                return Err(type_error(format!(
                    "sequence item {i}: expected str instance, {} found",
                    other.type_name()
                )))
            }
        }
    }
    Ok(Value::str(out))
}

fn strip_impl(args: Vec<Value>, kw: Kw, name: &str, left: bool, right: bool) -> PyResult<Value> {
    let s = me(&args)?;
    let b = bind(name, args[1..].to_vec(), kw, &["chars"], 0)?;
    let mut t = s.as_str();
    match &b[0] {
        None | Some(Value::None) => {
            if left {
                t = t.trim_start_matches(is_py_space);
            }
            if right {
                t = t.trim_end_matches(is_py_space);
            }
        }
        Some(Value::Str(cs)) => {
            let set: Vec<char> = cs.as_str().chars().collect();
            if left {
                t = t.trim_start_matches(|c: char| set.contains(&c));
            }
            if right {
                t = t.trim_end_matches(|c: char| set.contains(&c));
            }
        }
        Some(_) => return Err(type_error(format!("{name} arg must be None or str"))),
    }
    Ok(Value::str(t))
}

fn strip(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    strip_impl(args, kw, "strip", true, true)
}

fn lstrip(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    strip_impl(args, kw, "lstrip", true, false)
}

fn rstrip(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    strip_impl(args, kw, "rstrip", false, true)
}

fn partition_impl(args: Vec<Value>, kw: Kw, name: &str, reverse: bool) -> PyResult<Value> {
    no_kwargs(name, &kw)?;
    exactly(name, &args[1..], 1)?;
    let t = me(&args)?.as_str();
    let sep = match &args[1] {
        Value::Str(x) => x.as_str(),
        other => return Err(type_error(format!("must be str, not {}", other.type_name()))),
    };
    if sep.is_empty() {
        return Err(value_error("empty separator"));
    }
    let found = if reverse { t.rfind(sep) } else { t.find(sep) };
    let parts = match found {
        Some(i) => vec![Value::str(&t[..i]), Value::str(sep), Value::str(&t[i + sep.len()..])],
        None if reverse => vec![Value::str(""), Value::str(""), Value::str(t)],
        None => vec![Value::str(t), Value::str(""), Value::str("")],
    };
    Ok(Value::tuple(parts))
}

fn partition(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    partition_impl(args, kw, "partition", false)
}

fn rpartition(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    partition_impl(args, kw, "rpartition", true)
}

fn removeprefix(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("removeprefix", &kw)?;
    exactly("removeprefix", &args[1..], 1)?;
    let t = me(&args)?.as_str();
    match &args[1] {
        Value::Str(p) => Ok(Value::str(t.strip_prefix(p.as_str()).unwrap_or(t))),
        other => Err(type_error(format!("removeprefix() argument must be str, not {}", other.type_name()))),
    }
}

fn removesuffix(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("removesuffix", &kw)?;
    exactly("removesuffix", &args[1..], 1)?;
    let t = me(&args)?.as_str();
    match &args[1] {
        Value::Str(p) => Ok(Value::str(t.strip_suffix(p.as_str()).unwrap_or(t))),
        other => Err(type_error(format!("removesuffix() argument must be str, not {}", other.type_name()))),
    }
}

fn replace(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = me(&args)?;
    let b = bind("replace", args[1..].to_vec(), kw, &["old", "new", "count"], 2)?;
    let old = match req(&b, 0)? {
        Value::Str(x) => x.as_str(),
        o => return Err(type_error(format!("replace() argument 1 must be str, not {}", o.type_name()))),
    };
    let new = match req(&b, 1)? {
        Value::Str(x) => x.as_str(),
        o => return Err(type_error(format!("replace() argument 2 must be str, not {}", o.type_name()))),
    };
    let count = match &b[2] {
        None => -1,
        Some(v) => want_int(v)?,
    };
    let t = s.as_str();
    let r = if count < 0 { t.replace(old, new) } else { t.replacen(old, new, count as usize) };
    Ok(Value::str(r))
}

// ---------------------------------------------------------------------------------------------
// split
// ---------------------------------------------------------------------------------------------

fn split_ws(s: &str, max: i64) -> Vec<String> {
    let b: Vec<(usize, char)> = s.char_indices().collect();
    let n = b.len();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut splits = 0i64;
    loop {
        while i < n && is_py_space(b[i].1) {
            i += 1;
        }
        if i >= n {
            break;
        }
        if max >= 0 && splits >= max {
            out.push(s[b[i].0..].to_string());
            break;
        }
        let st = i;
        while i < n && !is_py_space(b[i].1) {
            i += 1;
        }
        let end = if i < n { b[i].0 } else { s.len() };
        out.push(s[b[st].0..end].to_string());
        splits += 1;
    }
    out
}

fn rsplit_ws(s: &str, max: i64) -> Vec<String> {
    let b: Vec<(usize, char)> = s.char_indices().collect();
    let mut i = b.len();
    let mut out = Vec::new();
    let mut splits = 0i64;
    loop {
        while i > 0 && is_py_space(b[i - 1].1) {
            i -= 1;
        }
        if i == 0 {
            break;
        }
        if max >= 0 && splits >= max {
            let end = b[i - 1].0 + b[i - 1].1.len_utf8();
            out.push(s[..end].to_string());
            break;
        }
        let end = b[i - 1].0 + b[i - 1].1.len_utf8();
        while i > 0 && !is_py_space(b[i - 1].1) {
            i -= 1;
        }
        let st = if i < b.len() { b[i].0 } else { s.len() };
        out.push(s[st..end].to_string());
        splits += 1;
    }
    out.reverse();
    out
}

fn split_impl(args: Vec<Value>, kw: Kw, name: &str, reverse: bool) -> PyResult<Value> {
    let s = me(&args)?;
    let b = bind(name, args[1..].to_vec(), kw, &["sep", "maxsplit"], 0)?;
    let max = match &b[1] {
        None => -1,
        Some(v) => want_int(v)?,
    };
    let t = s.as_str();
    let parts: Vec<String> = match &b[0] {
        None | Some(Value::None) => {
            if reverse {
                rsplit_ws(t, max)
            } else {
                split_ws(t, max)
            }
        }
        Some(Value::Str(sep)) => {
            let sep = sep.as_str();
            if sep.is_empty() {
                return Err(value_error("empty separator"));
            }
            if max < 0 {
                t.split(sep).map(String::from).collect()
            } else {
                let n = (max as usize).saturating_add(1);
                if reverse {
                    let mut v: Vec<String> = t.rsplitn(n, sep).map(String::from).collect();
                    v.reverse();
                    v
                } else {
                    t.splitn(n, sep).map(String::from).collect()
                }
            }
        }
        Some(o) => return Err(type_error(format!("must be str or None, not {}", o.type_name()))),
    };
    Ok(Value::list(parts.into_iter().map(Value::str).collect()))
}

fn split(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    split_impl(args, kw, "split", false)
}

fn rsplit(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    split_impl(args, kw, "rsplit", true)
}

fn is_line_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

fn splitlines(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = me(&args)?;
    let b = bind("splitlines", args[1..].to_vec(), kw, &["keepends"], 0)?;
    let keep = b[0].as_ref().is_some_and(|v| v.is_true());
    let t = s.as_str();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut it = t.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if !is_line_break(c) {
            continue;
        }
        let mut end = i + c.len_utf8();
        if c == '\r' {
            if let Some(&(_, '\n')) = it.peek() {
                it.next();
                end += 1;
            }
        }
        let content_end = if keep { end } else { i };
        out.push(Value::str(&t[start..content_end]));
        start = end;
    }
    if start < t.len() {
        out.push(Value::str(&t[start..]));
    }
    Ok(Value::list(out))
}

// ---------------------------------------------------------------------------------------------
// translate, encode, format
// ---------------------------------------------------------------------------------------------

fn table_lookup(table: &Value, ord: u32) -> PyResult<Option<Value>> {
    match table {
        Value::Dict(d) => Ok(d.borrow().get(&Value::Int(i64::from(ord)))?),
        Value::Str(s) => Ok(s.char_at(ord as usize).map(|c| Value::str(c.to_string()))),
        Value::List(l) => Ok(l.borrow().get(ord as usize).cloned()),
        Value::Tuple(t) => Ok(t.get(ord as usize).cloned()),
        other => Err(type_error(format!("'{}' object is not subscriptable", other.type_name()))),
    }
}

fn translate(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("translate", &kw)?;
    exactly("translate", &args[1..], 1)?;
    let t = me(&args)?.as_str();
    let table = &args[1];
    let mut out = String::with_capacity(t.len());
    for c in t.chars() {
        match table_lookup(table, c as u32)? {
            None => out.push(c),
            Some(Value::None) => {}
            Some(Value::Int(n)) => {
                let ch = u32::try_from(n)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(|| value_error("character mapping must be in range(0x110000)"))?;
                out.push(ch);
            }
            Some(Value::Str(x)) => out.push_str(x.as_str()),
            Some(_) => return Err(type_error("character mapping must return integer, None or str")),
        }
    }
    Ok(Value::str(out))
}

/// Como o CPython escreve um código-ponto nas mensagens e no `backslashreplace`.
fn escape_cp(c: char) -> String {
    let v = c as u32;
    if v <= 0xff {
        format!("\\x{v:02x}")
    } else if v <= 0xffff {
        format!("\\u{v:04x}")
    } else {
        format!("\\U{v:08x}")
    }
}

fn encode(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let s = me(&args)?;
    let b = bind("encode", args[1..].to_vec(), kw, &["encoding", "errors"], 0)?;
    let enc = match &b[0] {
        None => "utf-8".to_string(),
        Some(Value::Str(e)) => e.as_str().to_string(),
        Some(o) => {
            return Err(type_error(format!("encode() argument 'encoding' must be str, not {}", o.type_name())))
        }
    };
    let errors = match &b[1] {
        None => "strict".to_string(),
        Some(Value::Str(e)) => e.as_str().to_string(),
        Some(o) => return Err(type_error(format!("encode() argument 'errors' must be str, not {}", o.type_name()))),
    };
    let norm = enc.to_lowercase().replace(['-', ' '], "_");
    let (cname, limit): (&str, u32) = match norm.as_str() {
        "utf_8" | "utf8" | "u8" | "utf" | "cp65001" => {
            return Ok(Value::bytes(s.as_str().as_bytes().to_vec()));
        }
        "ascii" | "us_ascii" | "646" | "ansi_x3.4_1968" => ("ascii", 128),
        "latin_1" | "latin1" | "iso_8859_1" | "iso8859_1" | "l1" | "latin" | "8859" | "cp819" | "iso_ir_100" => {
            ("latin-1", 256)
        }
        _ if matches!(norm.as_str(), "cp437" | "437" | "ibm437") => {
            let mut out = Vec::with_capacity(s.as_str().len());
            for (i, c) in s.as_str().chars().enumerate() {
                match crate::cp437::encode_char(c) {
                    Some(b) => out.push(b),
                    None => {
                        return Err(exc(
                            "UnicodeEncodeError",
                            format!(
                                "'charmap' codec can't encode character '{}' in position {}: character maps to <undefined>",
                                escape_cp(c),
                                i
                            ),
                        ))
                    }
                }
            }
            return Ok(Value::bytes(out));
        }
        _ => return Err(exc("LookupError", format!("unknown encoding: {enc}"))),
    };
    let chars: Vec<char> = s.as_str().chars().collect();
    let mut out: Vec<u8> = Vec::with_capacity(chars.len());
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if (c as u32) < limit {
            out.push(c as u32 as u8);
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && (chars[j] as u32) >= limit {
            j += 1;
        }
        match errors.as_str() {
            "strict" => {
                let what = if j - i == 1 {
                    format!("character '{}' in position {}", escape_cp(chars[i]), i)
                } else {
                    format!("characters in position {}-{}", i, j - 1)
                };
                return Err(exc(
                    "UnicodeEncodeError",
                    format!("'{cname}' codec can't encode {what}: ordinal not in range({limit})"),
                ));
            }
            "ignore" => {}
            "replace" => out.extend(std::iter::repeat_n(b'?', j - i)),
            "backslashreplace" => {
                for &ch in &chars[i..j] {
                    out.extend(escape_cp(ch).bytes());
                }
            }
            "xmlcharrefreplace" => {
                for &ch in &chars[i..j] {
                    out.extend(format!("&#{};", ch as u32).bytes());
                }
            }
            other => return Err(exc("LookupError", format!("unknown error handler name '{other}'"))),
        }
        i = j;
    }
    Ok(Value::bytes(out))
}

fn format(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let t = me(&args)?.as_str();
    Ok(Value::str(str_format(vm, t, &args[1..], &kw)?))
}

fn format_map(vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    no_kwargs("format_map", &kw)?;
    exactly("format_map", &args[1..], 1)?;
    let t = me(&args)?.as_str();
    let mut mapping: Kw = Vec::new();
    match &args[1] {
        Value::Dict(d) => {
            for (k, v) in d.borrow().iter() {
                if let Value::Str(name) = k {
                    mapping.push((name.as_str().to_string(), v.clone()));
                }
            }
        }
        other => return Err(type_error(format!("'{}' object is not a mapping", other.type_name()))),
    }
    Ok(Value::str(str_format(vm, t, &[], &mapping)?))
}

pub const TABLE: &[(&str, NativeFnPtr)] = &[
    ("capitalize", capitalize),
    ("casefold", casefold),
    ("center", center),
    ("count", count),
    ("encode", encode),
    ("endswith", endswith),
    ("expandtabs", expandtabs),
    ("find", find),
    ("format", format),
    ("format_map", format_map),
    ("index", index),
    ("isalnum", isalnum),
    ("isalpha", isalpha),
    ("isascii", isascii),
    ("isdecimal", isdecimal),
    ("isdigit", isdigit),
    ("isidentifier", isidentifier),
    ("islower", islower),
    ("isnumeric", isnumeric),
    ("isprintable", isprintable),
    ("isspace", isspace),
    ("istitle", istitle),
    ("isupper", isupper),
    ("join", join),
    ("ljust", ljust),
    ("lower", lower),
    ("lstrip", lstrip),
    ("partition", partition),
    ("removeprefix", removeprefix),
    ("removesuffix", removesuffix),
    ("replace", replace),
    ("rfind", rfind),
    ("rindex", rindex),
    ("rjust", rjust),
    ("rpartition", rpartition),
    ("rsplit", rsplit),
    ("rstrip", rstrip),
    ("split", split),
    ("splitlines", splitlines),
    ("startswith", startswith),
    ("strip", strip),
    ("swapcase", swapcase),
    ("title", title),
    ("translate", translate),
    ("upper", upper),
    ("zfill", zfill),
];

#[cfg(test)]
mod tests {
    fn run(src: &str) -> String {
        let o = crate::run_source(src);
        assert!(o.stderr.is_empty(), "stderr: {}", o.stderr);
        String::from_utf8(o.stdout).unwrap()
    }

    #[test]
    fn case_methods() {
        assert_eq!(run("print('hello wORLD'.capitalize())"), "Hello world\n");
        assert_eq!(run("print('Hello'.upper(), 'Hello'.lower())"), "HELLO hello\n");
        assert_eq!(run("print('Hello World'.swapcase())"), "hELLO wORLD\n");
        assert_eq!(run("print('hello world'.title())"), "Hello World\n");
        assert_eq!(run("print(\"they're\".title())"), "They'Re\n");
        assert_eq!(run("print('HeLLo'.casefold())"), "hello\n");
        assert_eq!(run("print('straße'.upper())"), "STRASSE\n");
    }

    #[test]
    fn find_family() {
        assert_eq!(run("print('abcabc'.find('b'), 'abcabc'.rfind('b'), 'abc'.find('z'))"), "1 4 -1\n");
        assert_eq!(run("print('abcabc'.find('b', 2), 'abcabc'.find('b', 2, 4), 'abcabc'.find('', 6))"), "4 -1 6\n");
        assert_eq!(run("print('abc'.find('', 4), 'aéb'.find('b'), 'aébéb'.rfind('b'))"), "-1 2 4\n");
        assert_eq!(run("print('abcabc'.index('c'), 'abcabc'.rindex('a'))"), "2 3\n");
        assert_eq!(
            run("try:\n    'abc'.index('z')\nexcept ValueError as e:\n    print(e)\n"),
            "substring not found\n"
        );
        assert_eq!(
            run("try:\n    'abc'.rindex('z')\nexcept ValueError as e:\n    print(e)\n"),
            "substring not found\n"
        );
        assert_eq!(run("print('banana'.count('a'), 'banana'.count('na'), 'aaa'.count('aa'), 'abc'.count(''))"), "3 2 1 4\n");
        assert_eq!(run("print('banana'.count('a', 2), 'banana'.count('a', 1, 4), 'abc'.count('', 4))"), "2 2 0\n");
        assert_eq!(run("print('abcabc'.find('c', -3), 'abcabc'.find('a', 0, -3))"), "5 0\n");
    }

    #[test]
    fn startswith_endswith() {
        assert_eq!(run("print('hello'.startswith('he'), 'hello'.startswith('lo'), 'hello'.endswith('lo'))"), "True False True\n");
        assert_eq!(run("print('hello'.startswith(('x', 'h')), 'hello'.endswith(('x', 'y')))"), "True False\n");
        assert_eq!(run("print('hello'.startswith('ll', 2), 'hello'.endswith('ll', 0, 4), 'hello'.startswith('', 5))"), "True True True\n");
        assert_eq!(run("print('hello'.startswith('', 6))"), "False\n");
        assert_eq!(
            run("try:\n    'a'.startswith(1)\nexcept TypeError as e:\n    print(e)\n"),
            "startswith first arg must be str or a tuple of str, not int\n"
        );
    }

    #[test]
    fn justify_methods() {
        assert_eq!(run("print(repr('ab'.center(6)), repr('ab'.center(7, '*')), repr('abc'.center(2)))"), "'  ab  ' '***ab**' 'abc'\n");
        assert_eq!(run("print(repr('ab'.ljust(5)), repr('ab'.rjust(5, '-')), repr('ab'.ljust(1)))"), "'ab   ' '---ab' 'ab'\n");
        assert_eq!(run("print('42'.zfill(5), '-42'.zfill(5), '+7'.zfill(4), 'ab'.zfill(1))"), "00042 -0042 +007 ab\n");
        assert_eq!(
            run("try:\n    'a'.center(5, 'ab')\nexcept TypeError as e:\n    print(e)\n"),
            "The fill character must be exactly one character long\n"
        );
    }

    #[test]
    fn expandtabs_method() {
        assert_eq!(run("print(repr('a\\tb'.expandtabs()), repr('a\\tb'.expandtabs(4)), repr('ab\\tc'.expandtabs(2)))"), "'a       b' 'a   b' 'ab  c'\n");
        assert_eq!(run("print(repr('a\\tb\\nc\\td'.expandtabs(4)), repr('a\\tb'.expandtabs(0)))"), "'a   b\\nc   d' 'ab'\n");
    }

    #[test]
    fn predicates() {
        assert_eq!(run("print('abc1'.isalnum(), 'abc!'.isalnum(), ''.isalnum(), 'é'.isalpha(), 'ab1'.isalpha())"), "True False False True False\n");
        assert_eq!(run("print('abc'.isascii(), 'é'.isascii(), ''.isascii())"), "True False True\n");
        assert_eq!(run("print('123'.isdecimal(), '12a'.isdecimal(), ''.isdecimal(), '²'.isdecimal(), '²'.isdigit(), '²'.isnumeric(), '½'.isnumeric(), '½'.isdigit())"), "True False False False True True True False\n");
        assert_eq!(run("print('abc'.isidentifier(), '_a1'.isidentifier(), '1a'.isidentifier(), ''.isidentifier(), 'a b'.isidentifier())"), "True True False False False\n");
        assert_eq!(run("print('abc'.islower(), 'aBc'.islower(), '1'.islower(), 'ABC'.isupper(), 'AbC'.isupper(), ''.isupper())"), "True False False True False False\n");
        assert_eq!(run("print('a b'.isprintable(), 'a\\nb'.isprintable(), ''.isprintable())"), "True False True\n");
        assert_eq!(run("print(' \\t\\n'.isspace(), ' a'.isspace(), ''.isspace())"), "True False False\n");
        assert_eq!(run("print('Hello World'.istitle(), 'Hello world'.istitle(), 'HELLO'.istitle(), ''.istitle())"), "True False False False\n");
    }

    #[test]
    fn join_method() {
        assert_eq!(run("print(','.join(['a', 'b', 'c']), ''.join(('x', 'y')), '-'.join([]))"), "a,b,c xy \n");
        assert_eq!(
            run("try:\n    ','.join(['a', 1])\nexcept TypeError as e:\n    print(e)\n"),
            "sequence item 1: expected str instance, int found\n"
        );
        assert_eq!(
            run("try:\n    ','.join(5)\nexcept TypeError as e:\n    print(e)\n"),
            "can only join an iterable\n"
        );
    }

    #[test]
    fn strip_methods() {
        assert_eq!(run("print(repr('  ab  '.strip()), repr('  ab  '.lstrip()), repr('  ab  '.rstrip()))"), "'ab' 'ab  ' '  ab'\n");
        assert_eq!(run("print('xxabxx'.strip('x'), 'xxabxx'.lstrip('x'), 'xxabxx'.rstrip('x'), 'abcba'.strip('ab'))"), "ab abxx xxab c\n");
        assert_eq!(run("print(repr(' \\t\\nab\\n'.strip(None)))"), "'ab'\n");
        assert_eq!(
            run("try:\n    'a'.strip(1)\nexcept TypeError as e:\n    print(e)\n"),
            "strip arg must be None or str\n"
        );
    }

    #[test]
    fn partition_methods() {
        assert_eq!(run("print('a=b=c'.partition('='), 'a=b=c'.rpartition('='))"), "('a', '=', 'b=c') ('a=b', '=', 'c')\n");
        assert_eq!(run("print('abc'.partition('x'), 'abc'.rpartition('x'))"), "('abc', '', '') ('', '', 'abc')\n");
        assert_eq!(
            run("try:\n    'abc'.partition('')\nexcept ValueError as e:\n    print(e)\n"),
            "empty separator\n"
        );
    }

    #[test]
    fn prefix_suffix_methods() {
        assert_eq!(run("print('prefix-x'.removeprefix('prefix-'), 'x.py'.removesuffix('.py'), 'abc'.removeprefix('z'), 'abc'.removesuffix(''))"), "x x abc abc\n");
    }

    #[test]
    fn replace_method() {
        assert_eq!(run("print('aaa'.replace('a', 'b'), 'aaa'.replace('a', 'b', 2), 'aaa'.replace('a', 'b', 0), 'abc'.replace('', '-'))"), "bbb bba aaa -a-b-c-\n");
        assert_eq!(run("print('abc'.replace('b', ''), 'aaa'.replace('a', 'b', count=1))"), "ac baa\n");
    }

    #[test]
    fn split_methods() {
        assert_eq!(run("print('a b  c'.split(), ' a b '.split(), ''.split(), 'a,b,,c'.split(','))"), "['a', 'b', 'c'] ['a', 'b'] [] ['a', 'b', '', 'c']\n");
        assert_eq!(run("print('a b c d'.split(None, 2), '  a  b  c  '.split(None, 1), 'a,b,c'.split(',', 1))"), "['a', 'b', 'c d'] ['a', 'b  c  '] ['a', 'b,c']\n");
        assert_eq!(run("print('a b c d'.rsplit(None, 2), '  a  b  c  '.rsplit(None, 1), 'a,b,c'.rsplit(',', 1))"), "['a b', 'c', 'd'] ['  a  b', 'c'] ['a,b', 'c']\n");
        assert_eq!(run("print('a,b,c'.split(sep=',', maxsplit=1), 'a b c'.rsplit(maxsplit=1), ''.split(','))"), "['a', 'b,c'] ['a b', 'c'] ['']\n");
        assert_eq!(run("print('abc'.split('b'), 'a--b'.split('--'))"), "['a', 'c'] ['a', 'b']\n");
        assert_eq!(
            run("try:\n    'abc'.split('')\nexcept ValueError as e:\n    print(e)\n"),
            "empty separator\n"
        );
    }

    #[test]
    fn splitlines_method() {
        assert_eq!(run("print('a\\nb\\r\\nc\\rd'.splitlines(), 'a\\nb\\n'.splitlines(), ''.splitlines())"), "['a', 'b', 'c', 'd'] ['a', 'b'] []\n");
        assert_eq!(run("print('a\\nb\\r\\nc'.splitlines(True), 'a\\nb'.splitlines(keepends=True))"), "['a\\n', 'b\\r\\n', 'c'] ['a\\n', 'b']\n");
        assert_eq!(run("print('a\\n\\nb'.splitlines())"), "['a', '', 'b']\n");
    }

    #[test]
    fn translate_method() {
        assert_eq!(run("print('abc'.translate({97: 'X', 98: None, 99: 100}))"), "Xd\n");
        assert_eq!(run("print('abc'.translate({}))"), "abc\n");
    }

    #[test]
    fn encode_method() {
        assert_eq!(run("print('aé'.encode())"), "b'a\\xc3\\xa9'\n");
        assert_eq!(run("print('aé'.encode('utf-8'), 'aé'.encode('latin-1'), 'abc'.encode('ascii'))"), "b'a\\xc3\\xa9' b'a\\xe9' b'abc'\n");
        assert_eq!(run("print('aé'.encode('ascii', 'ignore'), 'aé'.encode('ascii', 'replace'))"), "b'a' b'a?'\n");
        assert_eq!(
            run("try:\n    'aé'.encode('ascii')\nexcept UnicodeEncodeError as e:\n    print(e)\n"),
            "'ascii' codec can't encode character '\\xe9' in position 1: ordinal not in range(128)\n"
        );
        assert_eq!(
            run("try:\n    'a€€'.encode('latin-1')\nexcept UnicodeEncodeError as e:\n    print(e)\n"),
            "'latin-1' codec can't encode characters in position 1-2: ordinal not in range(256)\n"
        );
        assert_eq!(
            run("try:\n    'a'.encode('nope')\nexcept LookupError as e:\n    print(e)\n"),
            "unknown encoding: nope\n"
        );
    }

    #[test]
    fn format_methods() {
        assert_eq!(run("print('{} {}'.format('a', 1), '{1}{0}'.format('a', 'b'), '{x}'.format(x=5))"), "a 1 ba 5\n");
        assert_eq!(run("print('{:>5}|{:<5}|{:^5}'.format('a', 'b', 'c'))"), "    a|b    |  c  \n");
        assert_eq!(run("print('{:.2f} {:05d} {:x}'.format(3.14159, 42, 255))"), "3.14 00042 ff\n");
        assert_eq!(run("print('{a}-{b}'.format_map({'a': 1, 'b': 'z'}))"), "1-z\n");
        assert_eq!(
            run("try:\n    '{} {}'.format(1)\nexcept IndexError as e:\n    print(e)\n"),
            "Replacement index 1 out of range for positional args tuple\n"
        );
        assert_eq!(
            run("try:\n    '{x}'.format_map({})\nexcept KeyError as e:\n    print(repr(e))\n"),
            "KeyError('x')\n"
        );
    }
}
