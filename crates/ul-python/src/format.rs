//! Formatação de texto do Python: a mini-linguagem de `format()` e das f-strings
//! (`format_value`), o operador `%` de `str` (`percent_format`) e `str.format` (`str_format`).
//!
//! Segue `Python/formatter_unicode.c`, `Objects/unicodeobject.c` (`PyUnicode_Format`) e
//! `Objects/stringlib/unicode_format.h` do CPython 3.13, com as mesmas mensagens de erro.

use std::rc::Rc;

use crate::native_util::value_error;
use crate::object::{float_repr, repr, to_str, ExcObj, Kw, Value};
use crate::vm::{exc, type_error, PyException, PyResult, Vm};

/// `KeyError(chave)`: a mensagem é o `repr` da chave, como no CPython.
fn key_error(key: &Value) -> PyException {
    PyException {
        kind: "KeyError",
        msg: repr(key),
        value: Some(Value::Exception(Rc::new(ExcObj::new("KeyError", vec![key.clone()])))),
        tb: Vec::new(),
    }
}

/// `ascii(v)`: o `repr` com tudo fora do ASCII escapado.
pub fn ascii_repr(v: &Value) -> String {
    let r = repr(v);
    let mut out = String::with_capacity(r.len());
    for c in r.chars() {
        let n = c as u32;
        if n < 128 {
            out.push(c);
        } else if n <= 0xff {
            out.push_str(&format!("\\x{n:02x}"));
        } else if n <= 0xffff {
            out.push_str(&format!("\\u{n:04x}"));
        } else {
            out.push_str(&format!("\\U{n:08x}"));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Especificação de formato
// ---------------------------------------------------------------------------------------------

#[derive(Clone)]
struct Spec {
    fill: char,
    fill_given: bool,
    align: Option<char>,
    sign: Option<char>,
    zero_neg: bool,
    alt: bool,
    zero: bool,
    width: Option<usize>,
    grouping: Option<char>,
    precision: Option<usize>,
    ty: Option<char>,
}

fn parse_digits(c: &[char], i: &mut usize) -> PyResult<Option<usize>> {
    let st = *i;
    while *i < c.len() && c[*i].is_ascii_digit() {
        *i += 1;
    }
    if *i == st {
        return Ok(None);
    }
    if *i - st > 18 {
        return Err(value_error("Too many decimal digits in format string"));
    }
    let t: String = c[st..*i].iter().collect();
    Ok(Some(t.parse::<usize>().unwrap_or(0)))
}

fn parse_spec(spec: &str, tname: &str) -> PyResult<Spec> {
    let c: Vec<char> = spec.chars().collect();
    let mut i = 0usize;
    let mut sp = Spec {
        fill: ' ',
        fill_given: false,
        align: None,
        sign: None,
        zero_neg: false,
        alt: false,
        zero: false,
        width: None,
        grouping: None,
        precision: None,
        ty: None,
    };
    let is_align = |ch: char| matches!(ch, '<' | '>' | '^' | '=');
    if c.len() >= 2 && is_align(c[1]) {
        sp.fill = c[0];
        sp.fill_given = true;
        sp.align = Some(c[1]);
        i = 2;
    } else if !c.is_empty() && is_align(c[0]) {
        sp.align = Some(c[0]);
        i = 1;
    }
    if i < c.len() && matches!(c[i], '+' | '-' | ' ') {
        sp.sign = Some(c[i]);
        i += 1;
    }
    if i < c.len() && c[i] == 'z' {
        sp.zero_neg = true;
        i += 1;
    }
    if i < c.len() && c[i] == '#' {
        sp.alt = true;
        i += 1;
    }
    if !sp.fill_given && i < c.len() && c[i] == '0' {
        sp.fill = '0';
        sp.zero = true;
        i += 1;
    }
    sp.width = parse_digits(&c, &mut i)?;
    if i < c.len() && (c[i] == ',' || c[i] == '_') {
        sp.grouping = Some(c[i]);
        i += 1;
        if i < c.len() && (c[i] == ',' || c[i] == '_') {
            if Some(c[i]) != sp.grouping {
                return Err(value_error("Cannot specify both ',' and '_'."));
            }
            return Err(value_error(format!("Invalid format specifier '{spec}' for object of type '{tname}'")));
        }
    }
    if i < c.len() && c[i] == '.' {
        i += 1;
        match parse_digits(&c, &mut i)? {
            Some(p) => sp.precision = Some(p),
            None => return Err(value_error("Format specifier missing precision")),
        }
    }
    let rem = c.len() - i;
    if rem == 1 {
        sp.ty = Some(c[i]);
    } else if rem > 1 {
        return Err(value_error(format!("Invalid format specifier '{spec}' for object of type '{tname}'")));
    }
    if let Some(g) = sp.grouping {
        let ok = match (g, sp.ty) {
            (_, None) => true,
            (_, Some('d' | 'e' | 'f' | 'g' | 'E' | 'G' | '%' | 'F')) => true,
            ('_', Some('b' | 'o' | 'x' | 'X')) => true,
            _ => false,
        };
        if !ok {
            return Err(value_error(format!("Cannot specify '{g}' with '{}'.", sp.ty.unwrap_or('?'))));
        }
    }
    Ok(sp)
}

fn unknown_code(t: char, tname: &str) -> PyException {
    value_error(format!("Unknown format code '{t}' for object of type '{tname}'"))
}

/// Preenche `body` até a largura com `fill`, alinhando por `align` (`<`, `>` ou `^`).
fn pad(body: &str, width: Option<usize>, fill: char, align: char) -> String {
    let n = body.chars().count();
    let w = width.unwrap_or(0);
    if w <= n {
        return body.to_string();
    }
    let total = w - n;
    let (l, r) = match align {
        '<' => (0, total),
        '^' => (total / 2, total - total / 2),
        _ => (total, 0),
    };
    let mut out = String::with_capacity(body.len() + total);
    out.extend(std::iter::repeat_n(fill, l));
    out.push_str(body);
    out.extend(std::iter::repeat_n(fill, r));
    out
}

fn group_digits(d: &str, sep: char, size: usize) -> String {
    let n = d.chars().count();
    let mut out = String::new();
    for (i, ch) in d.chars().enumerate() {
        if i > 0 && (n - i) % size == 0 {
            out.push(sep);
        }
        out.push(ch);
    }
    out
}

/// Monta um número: `lead` (sinal e prefixo), parte inteira (agrupável) e o resto (`.5e+03`).
fn assemble(lead: &str, int_part: &str, rest: &str, sp: &Spec, size: usize) -> String {
    let align = sp.align.unwrap_or(if sp.zero { '=' } else { '>' });
    let group = |d: &str| match sp.grouping {
        Some(g) => group_digits(d, g, size),
        None => d.to_string(),
    };
    let mut int_g = group(int_part);
    let w = sp.width.unwrap_or(0);
    let fixed = lead.chars().count() + rest.chars().count();
    if align == '=' && sp.fill == '0' && sp.grouping.is_some() && !int_part.is_empty() {
        // O CPython estende o agrupamento para dentro do preenchimento de zeros.
        let mut digits = int_part.to_string();
        while fixed + int_g.chars().count() < w {
            digits.insert(0, '0');
            int_g = group(&digits);
        }
    }
    let len = fixed + int_g.chars().count();
    if len >= w {
        return format!("{lead}{int_g}{rest}");
    }
    let total = w - len;
    let fill = |n: usize| -> String { std::iter::repeat_n(sp.fill, n).collect() };
    match align {
        '<' => format!("{lead}{int_g}{rest}{}", fill(total)),
        '^' => format!("{}{lead}{int_g}{rest}{}", fill(total / 2), fill(total - total / 2)),
        '=' => format!("{lead}{}{int_g}{rest}", fill(total)),
        _ => format!("{}{lead}{int_g}{rest}", fill(total)),
    }
}

// ---------------------------------------------------------------------------------------------
// Corpos de número
// ---------------------------------------------------------------------------------------------

fn fmt_fixed(a: f64, prec: usize, alt: bool) -> String {
    let mut s = format!("{a:.prec$}");
    if prec == 0 && alt {
        s.push('.');
    }
    s
}

/// Expoente no estilo do C: sinal e ao menos dois dígitos.
fn exp_suffix(exp: i32, upper: bool) -> String {
    format!("{}{}{:02}", if upper { 'E' } else { 'e' }, if exp < 0 { '-' } else { '+' }, exp.abs())
}

fn split_sci(a: f64, prec: usize) -> (String, i32) {
    let sci = format!("{a:.prec$e}");
    match sci.split_once('e') {
        Some((m, e)) => (m.to_string(), e.parse::<i32>().unwrap_or(0)),
        None => (sci, 0),
    }
}

fn fmt_exp(a: f64, prec: usize, alt: bool, upper: bool) -> String {
    let (mut m, e) = split_sci(a, prec);
    if prec == 0 && alt {
        m.push('.');
    }
    format!("{m}{}", exp_suffix(e, upper))
}

fn strip_zeros(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

fn fmt_general(a: f64, prec: usize, alt: bool, upper: bool) -> String {
    let p = if prec == 0 { 1 } else { prec };
    let (m, e) = split_sci(a, p - 1);
    if e >= -4 && i64::from(e) < p as i64 {
        let decimals = (p as i64 - 1 - i64::from(e)) as usize;
        let mut s = format!("{a:.decimals$}");
        if alt {
            if !s.contains('.') {
                s.push('.');
            }
        } else {
            s = strip_zeros(&s);
        }
        s
    } else {
        let mut m = if alt { m } else { strip_zeros(&m) };
        if alt && !m.contains('.') {
            m.push('.');
        }
        format!("{m}{}", exp_suffix(e, upper))
    }
}

fn nonfinite(a: f64, upper: bool) -> String {
    let s = if a.is_nan() { "nan" } else { "inf" };
    if upper {
        s.to_uppercase()
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------------------------
// format(valor, spec)
// ---------------------------------------------------------------------------------------------

/// `format(v, spec)` para `int`, `float`, `bool` e `str`. Outros tipos só aceitam o `spec` vazio.
pub fn format_value(v: &Value, spec: &str) -> PyResult<String> {
    match v {
        Value::Str(s) => format_str(s.as_str(), spec),
        Value::Bool(b) => {
            if spec.is_empty() {
                Ok(if *b { "True" } else { "False" }.to_string())
            } else {
                format_int(i64::from(*b), spec, "bool")
            }
        }
        Value::Int(n) => {
            if spec.is_empty() {
                Ok(n.to_string())
            } else {
                format_int(*n, spec, "int")
            }
        }
        Value::Float(x) => {
            if spec.is_empty() {
                Ok(float_repr(*x))
            } else {
                let sp = parse_spec(spec, "float")?;
                format_float_sp(*x, &sp)
            }
        }
        other => {
            if spec.is_empty() {
                Ok(to_str(other))
            } else {
                Err(type_error(format!("unsupported format string passed to {}.__format__", other.type_name())))
            }
        }
    }
}

fn format_str(s: &str, spec: &str) -> PyResult<String> {
    if spec.is_empty() {
        return Ok(s.to_string());
    }
    let sp = parse_spec(spec, "str")?;
    match sp.ty {
        None | Some('s') => {}
        Some(t) => return Err(unknown_code(t, "str")),
    }
    if let Some(g) = sp.grouping {
        return Err(value_error(format!("Cannot specify '{g}' with 's'.")));
    }
    if sp.sign.is_some() {
        return Err(value_error("Sign not allowed in string format specifier"));
    }
    if sp.alt {
        return Err(value_error("Alternate form (#) not allowed in string format specifier"));
    }
    if sp.align == Some('=') {
        return Err(value_error("'=' alignment not allowed in string format specifier"));
    }
    let text: String = match sp.precision {
        Some(p) => s.chars().take(p).collect(),
        None => s.to_string(),
    };
    Ok(pad(&text, sp.width, sp.fill, sp.align.unwrap_or('<')))
}

fn sign_str(neg: bool, sign: Option<char>) -> &'static str {
    if neg {
        "-"
    } else {
        match sign {
            Some('+') => "+",
            Some(' ') => " ",
            _ => "",
        }
    }
}

fn format_int(n: i64, spec: &str, tname: &str) -> PyResult<String> {
    let sp = parse_spec(spec, tname)?;
    let ty = sp.ty.unwrap_or('d');
    match ty {
        'e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%' => return format_float_sp(n as f64, &sp),
        'b' | 'o' | 'x' | 'X' | 'd' | 'n' | 'c' => {}
        t => return Err(unknown_code(t, tname)),
    }
    if sp.precision.is_some() {
        return Err(value_error("Precision not allowed in integer format specifier"));
    }
    if ty == 'c' {
        if sp.sign.is_some() {
            return Err(value_error("Sign not allowed with integer format specifier 'c'"));
        }
        if sp.alt {
            return Err(value_error("Alternate form (#) not allowed with integer format specifier 'c'"));
        }
        let ch = u32::try_from(n)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| exc("OverflowError", "%c arg not in range(0x110000)"))?;
        return Ok(pad(&ch.to_string(), sp.width, sp.fill, sp.align.unwrap_or('>')));
    }
    let a = n.unsigned_abs();
    let (digits, prefix, size) = match ty {
        'b' => (format!("{a:b}"), "0b", 4),
        'o' => (format!("{a:o}"), "0o", 4),
        'x' => (format!("{a:x}"), "0x", 4),
        'X' => (format!("{a:X}"), "0X", 4),
        _ => (a.to_string(), "", 3),
    };
    let mut lead = String::from(sign_str(n < 0, sp.sign));
    if sp.alt {
        lead.push_str(prefix);
    }
    Ok(assemble(&lead, &digits, "", &sp, size))
}

fn format_float_sp(x: f64, sp: &Spec) -> PyResult<String> {
    match sp.ty {
        None | Some('e' | 'E' | 'f' | 'F' | 'g' | 'G' | '%' | 'n') => {}
        Some(t) => return Err(unknown_code(t, "float")),
    }
    let mut neg = x.is_sign_negative() && !x.is_nan();
    let a = x.abs();
    let upper = matches!(sp.ty, Some('E' | 'F' | 'G'));
    let body = if !a.is_finite() {
        let mut s = nonfinite(a, upper);
        if sp.ty == Some('%') {
            s.push('%');
        }
        s
    } else {
        match sp.ty {
            Some('f' | 'F') => fmt_fixed(a, sp.precision.unwrap_or(6), sp.alt),
            Some('e' | 'E') => fmt_exp(a, sp.precision.unwrap_or(6), sp.alt, upper),
            Some('g' | 'G' | 'n') => fmt_general(a, sp.precision.unwrap_or(6), sp.alt, upper),
            Some('%') => {
                let mut s = fmt_fixed(a * 100.0, sp.precision.unwrap_or(6), sp.alt);
                s.push('%');
                s
            }
            _ => match sp.precision {
                None => float_repr(a),
                Some(p) => {
                    let mut s = fmt_general(a, p, sp.alt, false);
                    if !s.contains('.') && !s.contains('e') {
                        s.push_str(".0");
                    }
                    s
                }
            },
        }
    };
    if sp.zero_neg && neg {
        let mantissa = body.split(['e', 'E']).next().unwrap_or("");
        if mantissa.chars().all(|c| matches!(c, '0' | '.' | '%')) {
            neg = false;
        }
    }
    let lead = sign_str(neg, sp.sign);
    if a.is_finite() {
        let split = body.find(|c: char| !c.is_ascii_digit()).unwrap_or(body.len());
        Ok(assemble(lead, &body[..split], &body[split..], sp, 3))
    } else {
        Ok(assemble(lead, "", &body, sp, 3))
    }
}

// ---------------------------------------------------------------------------------------------
// Operador % de str
// ---------------------------------------------------------------------------------------------

fn next_arg(items: &[Value], next: &mut usize) -> PyResult<Value> {
    match items.get(*next) {
        Some(v) => {
            *next += 1;
            Ok(v.clone())
        }
        None => Err(type_error("not enough arguments for format string")),
    }
}

/// Sinal e prefixo, preenchimento de zeros entre eles e os dígitos, ou espaços.
fn layout(lead: &str, body: &str, width: Option<usize>, left: bool, zero: bool) -> String {
    let n = lead.chars().count() + body.chars().count();
    let w = width.unwrap_or(0);
    if w <= n {
        return format!("{lead}{body}");
    }
    let pad = w - n;
    if left {
        format!("{lead}{body}{}", " ".repeat(pad))
    } else if zero {
        format!("{lead}{}{body}", "0".repeat(pad))
    } else {
        format!("{}{lead}{body}", " ".repeat(pad))
    }
}

fn float_to_i64(x: f64) -> PyResult<i64> {
    if x.is_nan() {
        return Err(value_error("cannot convert float NaN to integer"));
    }
    if x.is_infinite() {
        return Err(exc("OverflowError", "cannot convert float infinity to integer"));
    }
    let t = x.trunc();
    if t >= 9_223_372_036_854_775_808.0 || t < -9_223_372_036_854_775_808.0 {
        return Err(exc("OverflowError", "integer result outside the 64-bit range (arbitrary int is pending)"));
    }
    Ok(t as i64)
}

/// `fmt % args` com `fmt` em `bytes`: `%b` vale como `%s`, e argumentos `bytes` entram como estão.
pub fn bytes_percent_format(fmt: &[u8], args: &Value) -> PyResult<Vec<u8>> {
    let latin = |b: &[u8]| -> String { b.iter().map(|&c| c as char).collect() };
    let mut text: Vec<char> = latin(fmt).chars().collect();
    let mut i = 0usize;
    while i < text.len() {
        if text[i] != '%' {
            i += 1;
            continue;
        }
        i += 1;
        if i < text.len() && text[i] == '(' {
            while i < text.len() && text[i] != ')' {
                i += 1;
            }
            i += 1;
        }
        while i < text.len() && "#0- +.*123456789".contains(text[i]) {
            i += 1;
        }
        if i < text.len() && text[i] == 'b' {
            text[i] = 's';
        }
        i += 1;
    }
    let conv = |v: &Value| match v {
        Value::Bytes(b) => Value::str(&latin(b)),
        other => other.clone(),
    };
    let args = match args {
        Value::Tuple(t) => Value::tuple(t.iter().map(conv).collect()),
        other => conv(other),
    };
    let text: String = text.into_iter().collect();
    let out = percent_format(&text, &args)?;
    out.chars()
        .map(|c| u8::try_from(c as u32).map_err(|_| exc("ValueError", "bytes formatting: character out of latin-1 range")))
        .collect()
}

/// `fmt % args`.
pub fn percent_format(fmt: &str, args: &Value) -> PyResult<String> {
    let c: Vec<char> = fmt.chars().collect();
    let items: Vec<Value> = match args {
        Value::Tuple(t) => t.to_vec(),
        other => vec![other.clone()],
    };
    let is_dict = matches!(args, Value::Dict(_));
    let mut next = 0usize;
    let mut out = String::new();
    let mut i = 0usize;
    while i < c.len() {
        if c[i] != '%' {
            out.push(c[i]);
            i += 1;
            continue;
        }
        i += 1;
        let mut key: Option<String> = None;
        if i < c.len() && c[i] == '(' {
            let mut depth = 1;
            let st = i + 1;
            i += 1;
            while i < c.len() && depth > 0 {
                match c[i] {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            if depth > 0 {
                return Err(value_error("incomplete format key"));
            }
            key = Some(c[st..i - 1].iter().collect());
        }
        let (mut left, mut plus, mut space, mut alt, mut zero) = (false, false, false, false, false);
        while i < c.len() {
            match c[i] {
                '-' => left = true,
                '+' => plus = true,
                ' ' => space = true,
                '#' => alt = true,
                '0' => zero = true,
                _ => break,
            }
            i += 1;
        }
        let mut width: Option<usize> = None;
        if i < c.len() && c[i] == '*' {
            let v = next_arg(&items, &mut next)?;
            let n = match v {
                Value::Int(n) => n,
                Value::Bool(b) => i64::from(b),
                _ => return Err(type_error("* wants int")),
            };
            if n < 0 {
                left = true;
            }
            width = Some(n.unsigned_abs() as usize);
            i += 1;
        } else {
            width = parse_digits(&c, &mut i)?.or(width);
        }
        let mut prec: Option<usize> = None;
        if i < c.len() && c[i] == '.' {
            i += 1;
            if i < c.len() && c[i] == '*' {
                let v = next_arg(&items, &mut next)?;
                let n = match v {
                    Value::Int(n) => n,
                    Value::Bool(b) => i64::from(b),
                    _ => return Err(type_error("* wants int")),
                };
                prec = Some(n.max(0) as usize);
                i += 1;
            } else {
                prec = Some(parse_digits(&c, &mut i)?.unwrap_or(0));
            }
        }
        while i < c.len() && matches!(c[i], 'h' | 'l' | 'L') {
            i += 1;
        }
        if i >= c.len() {
            return Err(value_error("incomplete format"));
        }
        let conv = c[i];
        i += 1;
        if conv == '%' {
            out.push('%');
            continue;
        }
        let arg = match &key {
            Some(k) => {
                let kv = Value::str(k.clone());
                match args {
                    Value::Dict(d) => match d.borrow().get(&kv)? {
                        Some(v) => v,
                        None => return Err(key_error(&kv)),
                    },
                    _ => return Err(type_error("format requires a mapping")),
                }
            }
            None => next_arg(&items, &mut next)?,
        };
        let sign_for = |neg: bool| -> &'static str {
            if neg {
                "-"
            } else if plus {
                "+"
            } else if space {
                " "
            } else {
                ""
            }
        };
        match conv {
            's' | 'r' | 'a' => {
                let mut t = match conv {
                    's' => to_str(&arg),
                    'r' => repr(&arg),
                    _ => ascii_repr(&arg),
                };
                if let Some(p) = prec {
                    t = t.chars().take(p).collect();
                }
                out.push_str(&layout("", &t, width, left, false));
            }
            'c' => {
                let from_int = |n: i64| -> PyResult<char> {
                    u32::try_from(n)
                        .ok()
                        .and_then(char::from_u32)
                        .ok_or_else(|| exc("OverflowError", "%c arg not in range(0x110000)"))
                };
                let ch = match &arg {
                    Value::Int(n) => from_int(*n)?,
                    Value::Bool(b) => from_int(i64::from(*b))?,
                    Value::Str(s) if s.len() == 1 => s.as_str().chars().next().unwrap_or(' '),
                    Value::Str(s) => {
                        return Err(type_error(format!(
                            "%c requires an int or a unicode character, not a string of length {}",
                            s.len()
                        )))
                    }
                    other => {
                        return Err(type_error(format!(
                            "%c requires an int or a unicode character, not {}",
                            other.type_name()
                        )))
                    }
                };
                out.push_str(&layout("", &ch.to_string(), width, left, false));
            }
            'd' | 'i' | 'u' => {
                let n: i64 = match &arg {
                    Value::Int(n) => *n,
                    Value::Bool(b) => i64::from(*b),
                    Value::Float(x) => float_to_i64(*x)?,
                    other => {
                        return Err(type_error(format!(
                            "%{conv} format: a real number is required, not {}",
                            other.type_name()
                        )))
                    }
                };
                let mut digits = n.unsigned_abs().to_string();
                if let Some(p) = prec {
                    while digits.len() < p {
                        digits.insert(0, '0');
                    }
                }
                out.push_str(&layout(sign_for(n < 0), &digits, width, left, zero));
            }
            'o' | 'x' | 'X' => {
                let n: i64 = match &arg {
                    Value::Int(n) => *n,
                    Value::Bool(b) => i64::from(*b),
                    other => {
                        return Err(type_error(format!(
                            "%{conv} format: an integer is required, not {}",
                            other.type_name()
                        )))
                    }
                };
                let a = n.unsigned_abs();
                let (mut digits, prefix) = match conv {
                    'o' => (format!("{a:o}"), "0o"),
                    'x' => (format!("{a:x}"), "0x"),
                    _ => (format!("{a:X}"), "0X"),
                };
                if let Some(p) = prec {
                    while digits.len() < p {
                        digits.insert(0, '0');
                    }
                }
                let mut lead = String::from(sign_for(n < 0));
                if alt {
                    lead.push_str(prefix);
                }
                out.push_str(&layout(&lead, &digits, width, left, zero));
            }
            'e' | 'E' | 'f' | 'F' | 'g' | 'G' => {
                let x: f64 = match &arg {
                    Value::Float(x) => *x,
                    Value::Int(n) => *n as f64,
                    Value::Bool(b) => f64::from(u8::from(*b)),
                    other => return Err(type_error(format!("must be real number, not {}", other.type_name()))),
                };
                let neg = x.is_sign_negative() && !x.is_nan();
                let a = x.abs();
                let upper = matches!(conv, 'E' | 'F' | 'G');
                let body = if !a.is_finite() {
                    nonfinite(a, upper)
                } else {
                    match conv {
                        'e' | 'E' => fmt_exp(a, prec.unwrap_or(6), alt, upper),
                        'f' | 'F' => fmt_fixed(a, prec.unwrap_or(6), alt),
                        _ => fmt_general(a, prec.unwrap_or(6), alt, upper),
                    }
                };
                out.push_str(&layout(sign_for(neg), &body, width, left, zero && a.is_finite()));
            }
            other => {
                return Err(value_error(format!(
                    "unsupported format character '{other}' (0x{:x})",
                    other as u32
                )))
            }
        }
    }
    if !is_dict && next < items.len() {
        return Err(type_error("not all arguments converted during string formatting"));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// str.format
// ---------------------------------------------------------------------------------------------

struct Ctx<'a> {
    args: &'a [Value],
    kw: &'a Kw,
    /// `None` = ainda indefinido, `Some(true)` = numeração automática, `Some(false)` = manual.
    auto: Option<bool>,
    next: usize,
}

/// `template.format(*args, **kw)`.
pub fn str_format(vm: &mut Vm, template: &str, args: &[Value], kw: &Kw) -> PyResult<String> {
    let mut ctx = Ctx { args, kw, auto: None, next: 0 };
    render(vm, &mut ctx, template, 2)
}

fn render(vm: &mut Vm, ctx: &mut Ctx, tpl: &str, depth: u32) -> PyResult<String> {
    let c: Vec<char> = tpl.chars().collect();
    let mut out = String::new();
    let mut i = 0usize;
    while i < c.len() {
        let ch = c[i];
        if ch == '{' {
            if i + 1 < c.len() && c[i + 1] == '{' {
                out.push('{');
                i += 2;
                continue;
            }
            if i + 1 >= c.len() {
                return Err(value_error("Single '{' encountered in format string"));
            }
            let start = i + 1;
            let mut nest = 1;
            let mut j = start;
            while j < c.len() {
                match c[j] {
                    '{' => nest += 1,
                    '}' => {
                        nest -= 1;
                        if nest == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            if j >= c.len() {
                return Err(value_error("expected '}' before end of string"));
            }
            let field: String = c[start..j].iter().collect();
            i = j + 1;
            let piece = render_field(vm, ctx, &field, depth)?;
            out.push_str(&piece);
        } else if ch == '}' {
            if i + 1 < c.len() && c[i + 1] == '}' {
                out.push('}');
                i += 2;
            } else {
                return Err(value_error("Single '}' encountered in format string"));
            }
        } else {
            out.push(ch);
            i += 1;
        }
    }
    Ok(out)
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn parse_index(s: &str) -> PyResult<usize> {
    if s.len() > 18 {
        return Err(value_error("Too many decimal digits in format string"));
    }
    Ok(s.parse::<usize>().unwrap_or(0))
}

fn render_field(vm: &mut Vm, ctx: &mut Ctx, field: &str, depth: u32) -> PyResult<String> {
    let f: Vec<char> = field.chars().collect();
    let mut k = 0usize;
    let mut in_br = false;
    while k < f.len() {
        let ch = f[k];
        if ch == '[' {
            in_br = true;
        } else if ch == ']' {
            in_br = false;
        } else if !in_br && (ch == '!' || ch == ':') {
            break;
        }
        k += 1;
    }
    let name: Vec<char> = f[..k].to_vec();
    let mut conv: Option<char> = None;
    let mut spec = String::new();
    if k < f.len() && f[k] == '!' {
        if k + 1 >= f.len() {
            return Err(value_error("end of string while looking for conversion specifier"));
        }
        conv = Some(f[k + 1]);
        k += 2;
        if k < f.len() && f[k] != ':' {
            return Err(value_error("expected ':' after conversion specifier"));
        }
    }
    if k < f.len() && f[k] == ':' {
        spec = f[k + 1..].iter().collect();
    }

    // Primeira parte do nome.
    let mut p = 0usize;
    while p < name.len() && name[p] != '.' && name[p] != '[' {
        p += 1;
    }
    let first: String = name[..p].iter().collect();
    let mut obj = if first.is_empty() {
        if ctx.auto == Some(false) {
            return Err(value_error("cannot switch from manual field specification to automatic field numbering"));
        }
        ctx.auto = Some(true);
        let idx = ctx.next;
        ctx.next += 1;
        positional(ctx, idx)?
    } else if all_digits(&first) {
        if ctx.auto == Some(true) {
            return Err(value_error("cannot switch from automatic field numbering to manual field specification"));
        }
        ctx.auto = Some(false);
        let idx = parse_index(&first)?;
        positional(ctx, idx)?
    } else {
        match ctx.kw.iter().find(|(kname, _)| kname.as_str() == first) {
            Some((_, v)) => v.clone(),
            None => return Err(key_error(&Value::str(first.clone()))),
        }
    };

    while p < name.len() {
        if name[p] == '.' {
            p += 1;
            let st = p;
            while p < name.len() && name[p] != '.' && name[p] != '[' {
                p += 1;
            }
            let attr: String = name[st..p].iter().collect();
            if attr.is_empty() {
                return Err(value_error("Empty attribute in format string"));
            }
            obj = vm.getattr(&obj, &attr)?;
        } else if name[p] == '[' {
            p += 1;
            let st = p;
            while p < name.len() && name[p] != ']' {
                p += 1;
            }
            if p >= name.len() {
                return Err(value_error("Missing ']' in format string"));
            }
            let idx_s: String = name[st..p].iter().collect();
            p += 1;
            if idx_s.is_empty() {
                return Err(value_error("Empty attribute in format string"));
            }
            let key = if all_digits(&idx_s) { Value::Int(parse_index(&idx_s)? as i64) } else { Value::str(idx_s) };
            obj = get_item(&obj, &key)?;
        } else {
            return Err(value_error("Only '.' or '[' may follow ']' in format field specifier"));
        }
    }

    let obj = match conv {
        None => obj,
        Some('r') => Value::str(repr(&obj)),
        Some('s') => Value::str(to_str(&obj)),
        Some('a') => Value::str(ascii_repr(&obj)),
        Some(other) => return Err(value_error(format!("Unknown conversion specifier {other}"))),
    };
    let spec = if spec.contains('{') {
        if depth <= 1 {
            return Err(value_error("Max string recursion exceeded"));
        }
        render(vm, ctx, &spec, depth - 1)?
    } else {
        spec
    };
    format_value(&obj, &spec)
}

fn positional(ctx: &Ctx, idx: usize) -> PyResult<Value> {
    ctx.args.get(idx).cloned().ok_or_else(|| {
        exc("IndexError", format!("Replacement index {idx} out of range for positional args tuple"))
    })
}

fn norm_index(key: &Value, len: usize) -> Option<Option<usize>> {
    let i = match key {
        Value::Int(i) => *i,
        Value::Bool(b) => i64::from(*b),
        _ => return None,
    };
    let j = if i < 0 { i + len as i64 } else { i };
    Some(if j >= 0 && (j as usize) < len { Some(j as usize) } else { None })
}

/// `obj[key]` para os campos `{a[1]}` e `{a[chave]}`.
fn get_item(obj: &Value, key: &Value) -> PyResult<Value> {
    match obj {
        Value::List(l) => {
            let items = l.borrow();
            match norm_index(key, items.len()) {
                Some(Some(i)) => Ok(items[i].clone()),
                Some(None) => Err(exc("IndexError", "list index out of range")),
                None => Err(type_error(format!(
                    "list indices must be integers or slices, not {}",
                    key.type_name()
                ))),
            }
        }
        Value::Tuple(t) => match norm_index(key, t.len()) {
            Some(Some(i)) => Ok(t[i].clone()),
            Some(None) => Err(exc("IndexError", "tuple index out of range")),
            None => Err(type_error(format!("tuple indices must be integers or slices, not {}", key.type_name()))),
        },
        Value::Str(s) => match norm_index(key, s.len()) {
            Some(Some(i)) => Ok(Value::str(s.char_at(i).map(String::from).unwrap_or_default())),
            Some(None) => Err(exc("IndexError", "string index out of range")),
            None => Err(type_error(format!("string indices must be integers, not '{}'", key.type_name()))),
        },
        Value::Dict(d) => match d.borrow().get(key)? {
            Some(v) => Ok(v),
            None => Err(key_error(key)),
        },
        Value::Ext(e) => match e.getitem(key) {
            Some(r) => r,
            None => Err(type_error(format!("'{}' object is not subscriptable", obj.type_name()))),
        },
        _ => Err(type_error(format!("'{}' object is not subscriptable", obj.type_name()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::Dict;

    fn fv(v: Value, spec: &str) -> String {
        format_value(&v, spec).unwrap()
    }

    fn fe(v: Value, spec: &str) -> String {
        let e = format_value(&v, spec).unwrap_err();
        format!("{}: {}", e.kind, e.msg)
    }

    fn s(t: &str) -> Value {
        Value::str(t)
    }

    #[test]
    fn int_formats() {
        assert_eq!(fv(Value::Int(255), "x"), "ff");
        assert_eq!(fv(Value::Int(255), "X"), "FF");
        assert_eq!(fv(Value::Int(255), "#x"), "0xff");
        assert_eq!(fv(Value::Int(8), "o"), "10");
        assert_eq!(fv(Value::Int(8), "#o"), "0o10");
        assert_eq!(fv(Value::Int(5), "08b"), "00000101");
        assert_eq!(fv(Value::Int(5), "#010b"), "0b00000101");
        assert_eq!(fv(Value::Int(1234567), ","), "1,234,567");
        assert_eq!(fv(Value::Int(1234567), "_d"), "1_234_567");
        assert_eq!(fv(Value::Int(255), "_b"), "1111_1111");
        assert_eq!(fv(Value::Int(5), "+d"), "+5");
        assert_eq!(fv(Value::Int(5), " d"), " 5");
        assert_eq!(fv(Value::Int(-5), "+d"), "-5");
        assert_eq!(fv(Value::Int(42), ">5"), "   42");
        assert_eq!(fv(Value::Int(42), "<5"), "42   ");
        assert_eq!(fv(Value::Int(42), "^6"), "  42  ");
        assert_eq!(fv(Value::Int(42), "*^7"), "**42***");
        assert_eq!(fv(Value::Int(42), "=+6"), "+   42");
        assert_eq!(fv(Value::Int(-42), "06"), "-00042");
        assert_eq!(fv(Value::Int(65), "c"), "A");
        assert_eq!(fv(Value::Int(42), "n"), "42");
        assert_eq!(fv(Value::Int(3), "05.1f"), "003.0");
        assert_eq!(fv(Value::Int(42), ""), "42");
    }

    #[test]
    fn int_errors() {
        assert_eq!(fe(Value::Int(5), ".2d"), "ValueError: Precision not allowed in integer format specifier");
        assert_eq!(fe(Value::Int(5), "s"), "ValueError: Unknown format code 's' for object of type 'int'");
        assert_eq!(fe(Value::Int(5), "xx"), "ValueError: Invalid format specifier 'xx' for object of type 'int'");
        assert_eq!(fe(Value::Int(5), ",x"), "ValueError: Cannot specify ',' with 'x'.");
        assert_eq!(fe(Value::Int(5), "."), "ValueError: Format specifier missing precision");
    }

    #[test]
    fn bool_formats() {
        assert_eq!(fv(Value::Bool(true), ""), "True");
        assert_eq!(fv(Value::Bool(true), "d"), "1");
        assert_eq!(fv(Value::Bool(false), "3"), "  0");
        assert_eq!(fe(Value::Bool(true), "s"), "ValueError: Unknown format code 's' for object of type 'bool'");
    }

    #[test]
    fn float_formats() {
        assert_eq!(fv(Value::Float(1234.5), "e"), "1.234500e+03");
        assert_eq!(fv(Value::Float(1234.5), ".2e"), "1.23e+03");
        assert_eq!(fv(Value::Float(0.000123), "E"), "1.230000E-04");
        assert_eq!(fv(Value::Float(3.14159), ".2f"), "3.14");
        assert_eq!(fv(Value::Float(3.14159), "10.3f"), "     3.142");
        assert_eq!(fv(Value::Float(-3.14159), "08.2f"), "-0003.14");
        assert_eq!(fv(Value::Float(3.5), "f"), "3.500000");
        assert_eq!(fv(Value::Float(1234567.0), "g"), "1.23457e+06");
        assert_eq!(fv(Value::Float(0.0001), "g"), "0.0001");
        assert_eq!(fv(Value::Float(100000.0), "g"), "100000");
        assert_eq!(fv(Value::Float(1e-5), "g"), "1e-05");
        assert_eq!(fv(Value::Float(0.25), "%"), "25.000000%");
        assert_eq!(fv(Value::Float(0.256), ".1%"), "25.6%");
        assert_eq!(fv(Value::Float(1234567.891), ",.2f"), "1,234,567.89");
        assert_eq!(fv(Value::Float(1.5), ""), "1.5");
        assert_eq!(fv(Value::Float(1.5), "6"), "   1.5");
        assert_eq!(fv(Value::Float(3.14159), ".3"), "3.14");
        assert_eq!(fv(Value::Float(2.0), ".3"), "2.0");
        assert_eq!(fv(Value::Float(f64::INFINITY), "f"), "inf");
        assert_eq!(fv(Value::Float(f64::NAN), "F"), "NAN");
        assert_eq!(fv(Value::Float(-2.5), "+.1f"), "-2.5");
        assert_eq!(fv(Value::Float(2.5), "+.1f"), "+2.5");
        assert_eq!(fe(Value::Float(2.5), "d"), "ValueError: Unknown format code 'd' for object of type 'float'");
    }

    #[test]
    fn str_formats() {
        assert_eq!(fv(s("abc"), ">5"), "  abc");
        assert_eq!(fv(s("abc"), "10"), "abc       ");
        assert_eq!(fv(s("abc"), ".2"), "ab");
        assert_eq!(fv(s("abc"), "*^7"), "**abc**");
        assert_eq!(fv(s("abc"), "s"), "abc");
        assert_eq!(fv(s("é"), ">3"), "  é");
        assert_eq!(fe(s("abc"), "d"), "ValueError: Unknown format code 'd' for object of type 'str'");
        assert_eq!(fe(s("abc"), "+"), "ValueError: Sign not allowed in string format specifier");
        assert_eq!(fe(s("abc"), "#"), "ValueError: Alternate form (#) not allowed in string format specifier");
        assert_eq!(fe(s("abc"), "=5"), "ValueError: '=' alignment not allowed in string format specifier");
    }

    #[test]
    fn other_types() {
        assert_eq!(fv(Value::None, ""), "None");
        assert_eq!(fe(Value::None, "5"), "TypeError: unsupported format string passed to NoneType.__format__");
        assert_eq!(fe(Value::list(vec![]), "5"), "TypeError: unsupported format string passed to list.__format__");
    }

    fn pf(fmt: &str, args: Value) -> String {
        percent_format(fmt, &args).unwrap()
    }

    fn pe(fmt: &str, args: Value) -> String {
        let e = percent_format(fmt, &args).unwrap_err();
        format!("{}: {}", e.kind, e.msg)
    }

    #[test]
    fn percent_basic() {
        assert_eq!(pf("%s and %s", Value::tuple(vec![s("a"), s("b")])), "a and b");
        assert_eq!(pf("%5d", Value::Int(42)), "   42");
        assert_eq!(pf("%-5d|", Value::Int(42)), "42   |");
        assert_eq!(pf("%05d", Value::Int(-42)), "-0042");
        assert_eq!(pf("%+d", Value::Int(5)), "+5");
        assert_eq!(pf("% d", Value::Int(5)), " 5");
        assert_eq!(pf("%x", Value::Int(255)), "ff");
        assert_eq!(pf("%X", Value::Int(255)), "FF");
        assert_eq!(pf("%#x", Value::Int(255)), "0xff");
        assert_eq!(pf("%#o", Value::Int(8)), "0o10");
        assert_eq!(pf("%.2f", Value::Float(3.14159)), "3.14");
        assert_eq!(pf("%e", Value::Float(12345.678)), "1.234568e+04");
        assert_eq!(pf("%g", Value::Float(0.00001234)), "1.234e-05");
        assert_eq!(pf("%c", Value::Int(65)), "A");
        assert_eq!(pf("%c", s("x")), "x");
        assert_eq!(pf("100%%", Value::tuple(vec![])), "100%");
        assert_eq!(pf("%r", s("a")), "'a'");
        assert_eq!(pf("%a", s("é")), "'\\xe9'");
        assert_eq!(pf("%*d", Value::tuple(vec![Value::Int(5), Value::Int(42)])), "   42");
        assert_eq!(pf("%.3s", s("abcdef")), "abc");
        assert_eq!(pf("%5s", s("ab")), "   ab");
        assert_eq!(pf("%-5s|", s("ab")), "ab   |");
        assert_eq!(pf("%s", Value::None), "None");
        assert_eq!(pf("%d", Value::Float(3.9)), "3");
        assert_eq!(pf("%.3d", Value::Int(5)), "005");
        assert_eq!(pf("%s", Value::list(vec![Value::Int(1), Value::Int(2)])), "[1, 2]");
        assert_eq!(pf("%i", Value::Int(7)), "7");
    }

    #[test]
    fn percent_dict() {
        let mut d = Dict::new();
        d.set(s("a"), s("x")).unwrap();
        d.set(s("b"), Value::Int(3)).unwrap();
        assert_eq!(pf("%(a)s-%(b)d", Value::dict(d.clone())), "x-3");
        assert_eq!(pf("sem campos", Value::dict(d.clone())), "sem campos");
        assert_eq!(pe("%(z)s", Value::dict(d)), "KeyError: 'z'");
        assert_eq!(pe("%(a)s", Value::Int(1)), "TypeError: format requires a mapping");
    }

    #[test]
    fn percent_errors() {
        assert_eq!(pe("%s %s", Value::tuple(vec![s("a")])), "TypeError: not enough arguments for format string");
        assert_eq!(
            pe("%s", Value::tuple(vec![s("a"), s("b")])),
            "TypeError: not all arguments converted during string formatting"
        );
        assert_eq!(pe("sem", Value::Int(1)), "TypeError: not all arguments converted during string formatting");
        assert_eq!(pe("%d", s("x")), "TypeError: %d format: a real number is required, not str");
        assert_eq!(pe("%x", Value::Float(1.5)), "TypeError: %x format: an integer is required, not float");
        assert_eq!(pe("%y", Value::Int(1)), "ValueError: unsupported format character 'y' (0x79)");
        assert_eq!(pe("%", Value::tuple(vec![])), "ValueError: incomplete format");
        assert_eq!(pe("%f", s("x")), "TypeError: must be real number, not str");
    }

    fn sf(tpl: &str, args: &[Value], kw: &[(&str, Value)]) -> String {
        let mut vm = Vm::new();
        let kw: Kw = kw.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        str_format(&mut vm, tpl, args, &kw).unwrap()
    }

    fn se(tpl: &str, args: &[Value], kw: &[(&str, Value)]) -> String {
        let mut vm = Vm::new();
        let kw: Kw = kw.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let e = str_format(&mut vm, tpl, args, &kw).unwrap_err();
        format!("{}: {}", e.kind, e.msg)
    }

    #[test]
    fn str_format_fields() {
        assert_eq!(sf("{} {}", &[s("a"), Value::Int(1)], &[]), "a 1");
        assert_eq!(sf("{1} {0}", &[s("a"), s("b")], &[]), "b a");
        assert_eq!(sf("{name}", &[], &[("name", s("x"))]), "x");
        assert_eq!(sf("{0[1]}", &[Value::list(vec![Value::Int(1), Value::Int(2)])], &[]), "2");
        let mut d = Dict::new();
        d.set(s("k"), s("v")).unwrap();
        assert_eq!(sf("{a[k]}", &[], &[("a", Value::dict(d))]), "v");
        assert_eq!(sf("{!r}", &[s("a")], &[]), "'a'");
        assert_eq!(sf("{!s}", &[s("a")], &[]), "a");
        assert_eq!(sf("{!a}", &[s("é")], &[]), "'\\xe9'");
        assert_eq!(sf("{:>5}", &[s("ab")], &[]), "   ab");
        assert_eq!(sf("{:{w}}|", &[s("ab")], &[("w", Value::Int(4))]), "ab  |");
        assert_eq!(sf("{:.{p}f}", &[Value::Float(3.14159)], &[("p", Value::Int(2))]), "3.14");
        assert_eq!(sf("{{}}", &[], &[]), "{}");
        assert_eq!(sf("{:,}", &[Value::Int(1234567)], &[]), "1,234,567");
        assert_eq!(sf("{!r:>6}", &[s("a")], &[]), "   'a'");
    }

    #[test]
    fn str_format_errors() {
        assert_eq!(
            se("{} {}", &[Value::Int(1)], &[]),
            "IndexError: Replacement index 1 out of range for positional args tuple"
        );
        assert_eq!(se("{x}", &[], &[]), "KeyError: 'x'");
        assert_eq!(
            se("{0} {}", &[s("a"), s("b")], &[]),
            "ValueError: cannot switch from manual field specification to automatic field numbering"
        );
        assert_eq!(
            se("{} {0}", &[s("a"), s("b")], &[]),
            "ValueError: cannot switch from automatic field numbering to manual field specification"
        );
        assert_eq!(se("}", &[], &[]), "ValueError: Single '}' encountered in format string");
        assert_eq!(se("{", &[], &[]), "ValueError: Single '{' encountered in format string");
        assert_eq!(se("{!x}", &[s("a")], &[]), "ValueError: Unknown conversion specifier x");
    }
}
