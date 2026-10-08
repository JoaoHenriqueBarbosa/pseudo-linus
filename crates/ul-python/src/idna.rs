//! Os codecs `idna` e `punycode`, porte linha a linha de `Lib/encodings/idna.py`,
//! `Lib/encodings/punycode.py` e das tabelas de `Lib/stringprep.py` do CPython 3.13.
//!
//! O nameprep trabalha por código-ponto sobre o `unicodedata.ucd_3_2_0` (categoria, bidi e
//! normalização NFKC do 3.2.0, como o `stringprep.py` importa). O mapeamento B.3 sai do próprio
//! `stringprep.py` da imagem (`b3_exceptions`) e, fora dele, de `str.lower()`. Os inteiros do
//! punycode são de precisão arbitrária, como no Python, para os erros e as mensagens coincidirem
//! mesmo com entrada absurda.

use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use num_bigint::BigUint;
use num_traits::{One, ToPrimitive, Zero};

use crate::modules::ucd::{self, Props};
use crate::object::{bytes_repr, code_points, cp_to_str, lower_str, push_cp, str_repr, Value};
use crate::textcodec::unicode_error;
use crate::vm::{exc, PyException, PyResult};

const STRINGPREP: &str = include_str!("../../kernel/image/usr/lib/python3.13/stringprep.py");

const ACE_PREFIX: &[u8] = b"xn--";
const DIGITS: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// O que sobe de um passo do codec: um `UnicodeError` com `args` do CPython, ou o `OverflowError`
/// de um inteiro que não cabe em `Py_ssize_t` ao montar o erro.
enum Fault {
    Unicode { encoding: &'static str, start: usize, end: usize, reason: String },
    Overflow,
}

type Step<T> = Result<T, Fault>;

fn fail(encoding: &'static str, start: usize, end: usize, reason: impl Into<String>) -> Fault {
    Fault::Unicode { encoding, start, end, reason: reason.into() }
}

impl Fault {
    /// O mesmo erro com as posições deslocadas de `by` (o `offset + exc.start` do CPython).
    fn shifted(self, encoding: &'static str, by: usize) -> Fault {
        match self {
            Fault::Unicode { start, end, reason, .. } => Fault::Unicode { encoding, start: by + start, end: by + end, reason },
            Fault::Overflow => Fault::Overflow,
        }
    }

    fn raise(self, kind: &'static str, object: Value) -> PyException {
        match self {
            Fault::Unicode { encoding, start, end, reason } => unicode_error(kind, encoding, object, start, end, &reason),
            Fault::Overflow => exc("OverflowError", "Python int too large to convert to C ssize_t"),
        }
    }
}

fn unsupported_errors(errors: &str) -> PyException {
    exc("UnicodeError", format!("Unsupported error handling: {errors}"))
}

fn cps_to_string(cps: &[u32]) -> String {
    let mut out = String::with_capacity(cps.len());
    cps.iter().for_each(|&cp| push_cp(&mut out, cp));
    out
}

// ---------------------------------------------------------------------------------------------
// stringprep (RFC 3454), as tabelas que o nameprep consulta.

fn in_table_b1(c: u32) -> bool {
    matches!(c, 173 | 847 | 6150 | 6155..=6157 | 8203..=8205 | 8288 | 65279 | 65024..=65039)
}

/// `b3_exceptions` do `stringprep.py`, lido do próprio arquivo da imagem.
fn b3_exceptions() -> &'static HashMap<u32, Vec<u32>> {
    static TABLE: OnceLock<HashMap<u32, Vec<u32>>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let body = STRINGPREP.split_once("b3_exceptions = {").and_then(|(_, rest)| rest.split_once('}')).map_or("", |(body, _)| body);
        body.split("0x")
            .skip(1)
            .filter_map(|entry| {
                let (key, rest) = entry.split_once(':')?;
                let quoted = rest.trim_start().strip_prefix('\'')?;
                let value = &quoted[..quoted.rfind('\'')?];
                Some((u32::from_str_radix(key, 16).ok()?, unescape(value)))
            })
            .collect()
    })
}

/// O texto de um literal `'...'` do `stringprep.py`: só aparecem `\uXXXX` e `\xHH`.
fn unescape(literal: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let mut chars = literal.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c as u32);
            continue;
        }
        let width = match chars.next() {
            Some('u') => 4,
            _ => 2,
        };
        let digits: String = chars.by_ref().take(width).collect();
        out.extend(u32::from_str_radix(&digits, 16).ok());
    }
    out
}

/// `map_table_b3`: o mapeamento de caixa do `stringprep.py`.
fn map_table_b3(cp: u32) -> Vec<u32> {
    b3_exceptions().get(&cp).cloned().unwrap_or_else(|| code_points(&lower_str(&cp_to_str(cp))).collect())
}

/// NFKC do `unicodedata.ucd_3_2_0`.
fn nfkc(cps: &[u32]) -> Vec<u32> {
    let text = cps_to_string(cps);
    ucd::v3_2().normalize("NFKC", &text).map_or_else(|| cps.to_vec(), |out| code_points(&out).collect())
}

/// `map_table_b2`: a caixa do B.3 conferida contra a normalização NFKC.
fn map_table_b2(cp: u32) -> Vec<u32> {
    let al = map_table_b3(cp);
    let b = nfkc(&al);
    let bl: Vec<u32> = b.iter().flat_map(|&c| map_table_b3(c)).collect();
    let c = nfkc(&bl);
    if b != c { c } else { al }
}

fn category_is(cp: u32, category: &str) -> bool {
    ucd::v3_2().category(cp) == category
}

/// Os códigos proibidos do `nameprep`: C.1.2, C.2.2, C.3, C.4, C.5, C.6, C.7, C.8 e C.9.
fn is_prohibited(c: u32) -> bool {
    let c12 = category_is(c, "Zs") && c != 0x20;
    let c22 = c >= 128
        && (category_is(c, "Cc")
            || matches!(c, 1757 | 1807 | 6158 | 8204 | 8205 | 8232 | 8233 | 65279 | 8288..=8291 | 8298..=8303 | 65529..=65532 | 119155..=119162));
    let c4 = c >= 0xFDD0 && (c < 0xFDF0 || matches!(c & 0xFFFF, 0xFFFE | 0xFFFF));
    let c6 = (65529..=65533).contains(&c);
    let c7 = (12272..=12283).contains(&c);
    let c8 = matches!(c, 832 | 833 | 8206 | 8207 | 8234..=8238 | 8298..=8303);
    let c9 = c == 917505 || (917536..=917631).contains(&c);
    c12 || c22 || category_is(c, "Co") || c4 || category_is(c, "Cs") || c6 || c7 || c8 || c9
}

fn in_table_d1(c: u32) -> bool {
    matches!(ucd::v3_2().bidirectional(c), "R" | "AL")
}

fn in_table_d2(c: u32) -> bool {
    ucd::v3_2().bidirectional(c) == "L"
}

// ---------------------------------------------------------------------------------------------
// punycode.py

/// Parâmetros do punycode: tmin = 1, tmax = 26, base = 36.
fn punycode_t(j: usize, bias: i64) -> i64 {
    (36 * (j as i64 + 1) - bias).clamp(1, 26)
}

fn punycode_adapt(delta: BigUint, first: bool, numchars: usize) -> i64 {
    let mut delta = if first { delta / 700u32 } else { delta / 2u32 };
    let extra = &delta / numchars as u64;
    delta += extra;
    let mut divisions = 0;
    while delta > BigUint::from(455u32) {
        delta /= 35u32;
        divisions += 36;
    }
    let delta = delta.to_i64().unwrap_or(0);
    divisions + (36 * delta / (delta + 38))
}

fn generalized_integer(mut n: u64, bias: i64) -> Vec<u8> {
    let mut out = Vec::new();
    for j in 0.. {
        let t = punycode_t(j, bias) as u64;
        if n < t {
            out.push(DIGITS[n as usize]);
            break;
        }
        out.push(DIGITS[(t + (n - t) % (36 - t)) as usize]);
        n = (n - t) / (36 - t);
    }
    out
}

/// `selective_find`: o próximo `c` depois de `pos`, com o índice contado só entre os menores.
fn selective_find(cps: &[u32], c: u32, mut index: i64, mut pos: i64) -> Option<(i64, i64)> {
    loop {
        pos += 1;
        let x = *cps.get(pos as usize)?;
        if x == c {
            return Some((index + 1, pos));
        }
        if x < c {
            index += 1;
        }
    }
}

/// `insertion_unsort`.
fn insertion_unsort(cps: &[u32], extended: &BTreeSet<u32>) -> Vec<u64> {
    let mut oldchar = 0x80i64;
    let mut oldindex = -1i64;
    let mut result = Vec::new();
    for &c in extended {
        let curlen = cps.iter().filter(|&&x| x < c).count() as i64;
        let mut delta = (curlen + 1) * (i64::from(c) - oldchar);
        let (mut index, mut pos) = (-1i64, -1i64);
        while let Some((i, p)) = selective_find(cps, c, index, pos) {
            (index, pos) = (i, p);
            delta += index - oldindex;
            result.push((delta - 1) as u64);
            oldindex = index;
            delta = 0;
        }
        oldchar = i64::from(c);
    }
    result
}

/// `punycode_encode`: o texto sem o prefixo `xn--`.
pub fn punycode_encode(cps: &[u32]) -> Vec<u8> {
    let base: Vec<u8> = cps.iter().filter(|&&c| c < 128).map(|&c| c as u8).collect();
    let extended: BTreeSet<u32> = cps.iter().copied().filter(|&c| c >= 128).collect();
    let mut bias = 72;
    let mut tail = Vec::new();
    for (points, delta) in insertion_unsort(cps, &extended).into_iter().enumerate() {
        tail.extend(generalized_integer(delta, bias));
        bias = punycode_adapt(BigUint::from(delta), points == 0, base.len() + points + 1);
    }
    if base.is_empty() {
        return tail;
    }
    let mut out = base;
    out.push(b'-');
    out.extend(tail);
    out
}

/// `decode_generalized_number`: o inteiro de `extended` a partir de `extpos` e a posição seguinte;
/// o inteiro é `None` quando o erro é tolerado (`replace`, `ignore`).
fn decode_generalized_number(extended: &[u8], mut extpos: usize, bias: i64, strict: bool) -> Step<(usize, Option<BigUint>)> {
    let mut result = BigUint::zero();
    let mut w = BigUint::one();
    for j in 0.. {
        let Some(&ch) = extended.get(extpos) else {
            return if strict { Err(fail("punycode", extpos, extpos + 1, "incomplete punycode string")) } else { Ok((extpos + 1, None)) };
        };
        extpos += 1;
        let digit = match ch {
            0x41..=0x5A => ch - 0x41,
            0x30..=0x39 => ch - 22,
            _ if strict => return Err(fail("punycode", extpos - 1, extpos, format!("Invalid extended code point '{ch}'"))),
            _ => return Ok((extpos, None)),
        };
        let t = punycode_t(j, bias) as u32;
        result += &w * u32::from(digit);
        if u32::from(digit) < t {
            return Ok((extpos, Some(result)));
        }
        w *= 36 - t;
    }
    unreachable!("o laço só sai por return")
}

/// `insertion_sort`: as posições dos erros são relativas a `extended`.
fn insertion_sort(mut base: Vec<u32>, extended: &[u8], strict: bool) -> Step<Vec<u32>> {
    let mut ch = BigUint::from(0x80u32);
    // `None` é o -1 inicial do `pos`.
    let mut pos: Option<BigUint> = None;
    let mut bias = 72;
    let mut extpos = 0;
    while extpos < extended.len() {
        let (newpos, delta) = decode_generalized_number(extended, extpos, bias, strict)?;
        // Erro tolerado: a sincronia se perdeu e o que já foi montado é o resultado.
        let Some(delta) = delta else { return Ok(base) };
        let slots = BigUint::from(base.len() + 1);
        let at = match pos {
            None => delta.clone(),
            Some(p) => p + &delta + 1u32,
        };
        ch += &at / &slots;
        if ch > BigUint::from(0x10FFFFu32) {
            if strict {
                let end = at.to_i64().ok_or(Fault::Overflow)? as usize;
                return Err(fail("punycode", end.saturating_sub(1), end, format!("Invalid character U+{}", ch.to_str_radix(16))));
            }
            ch = BigUint::from(b'?');
        }
        let at = at % &slots;
        base.insert(at.to_usize().unwrap_or(0), ch.to_u32().unwrap_or(0));
        bias = punycode_adapt(delta, extpos == 0, base.len());
        extpos = newpos;
        pos = Some(at);
    }
    Ok(base)
}

/// `str(text[:pos], "ascii", errors)` com os handlers que o punycode aceita.
fn ascii_base(text: &[u8], errors: &str) -> Step<Vec<u32>> {
    let mut out = Vec::with_capacity(text.len());
    for (i, &b) in text.iter().enumerate() {
        match (b < 128, errors) {
            (true, _) => out.push(u32::from(b)),
            (false, "replace") => out.push(0xFFFD),
            (false, "ignore") => {}
            (false, _) => return Err(fail("ascii", i, i + 1, "ordinal not in range(128)")),
        }
    }
    Ok(out)
}

/// `punycode_decode`; as posições do erro são as do texto inteiro.
fn punycode_decode_cps(text: &[u8], errors: &str) -> Step<Vec<u32>> {
    let (base, offset, extended) = match text.iter().rposition(|&b| b == b'-') {
        None => (Vec::new(), 0, text.to_ascii_uppercase()),
        Some(pos) => (ascii_base(&text[..pos], errors)?, pos + 1, text[pos + 1..].to_ascii_uppercase()),
    };
    insertion_sort(base, &extended, errors == "strict").map_err(|f| f.shifted("punycode", offset))
}

/// `bytes.decode('punycode', errors)`.
pub fn punycode_decode(data: &[u8], errors: &str) -> PyResult<String> {
    if !matches!(errors, "strict" | "replace" | "ignore") {
        return Err(unsupported_errors(errors));
    }
    punycode_decode_cps(data, errors).map(|cps| cps_to_string(&cps)).map_err(|f| f.raise("UnicodeDecodeError", Value::bytes(data)))
}

// ---------------------------------------------------------------------------------------------
// idna.py

fn is_dot(c: &u32) -> bool {
    matches!(*c, 0x2E | 0x3002 | 0xFF0E | 0xFF61)
}

/// `nameprep`: mapeia, normaliza, proíbe e confere o bidi (AllowUnassigned é verdadeiro).
fn nameprep(label: &[u32]) -> Step<Vec<u32>> {
    let mapped: Vec<u32> = label.iter().filter(|&&c| !in_table_b1(c)).flat_map(|&c| map_table_b2(c)).collect();
    let label = nfkc(&mapped);
    if let Some(i) = label.iter().position(|&c| is_prohibited(c)) {
        return Err(fail("idna", i, i + 1, format!("Invalid character {}", str_repr(&cp_to_str(label[i])))));
    }
    let rand_al: Vec<bool> = label.iter().map(|&c| in_table_d1(c)).collect();
    if rand_al.iter().any(|&r| r) {
        if let Some(i) = label.iter().position(|&c| in_table_d2(c)) {
            return Err(fail("idna", i, i + 1, "Violation of BIDI requirement 2"));
        }
        if !rand_al[0] {
            return Err(fail("idna", 0, 1, "Violation of BIDI requirement 3"));
        }
        if !rand_al[rand_al.len() - 1] {
            return Err(fail("idna", label.len() - 1, label.len(), "Violation of BIDI requirement 3"));
        }
    }
    Ok(label)
}

/// O fim do `ToASCII` para um rótulo já ASCII: de 1 a 63 caracteres, senão vazio ou longo demais.
fn ascii_label(label: &[u32]) -> Step<Vec<u8>> {
    if (1..64).contains(&label.len()) {
        return Ok(label.iter().map(|&c| c as u8).collect());
    }
    if label.is_empty() {
        return Err(fail("idna", 0, 1, "label empty"));
    }
    Err(fail("idna", 0, label.len(), "label too long"))
}

fn is_ascii_cps(cps: &[u32]) -> bool {
    cps.iter().all(|&c| c < 128)
}

/// `ToASCII`.
fn to_ascii(label: &[u32]) -> Step<Vec<u8>> {
    if is_ascii_cps(label) {
        return ascii_label(label);
    }
    let label = nameprep(label)?;
    if is_ascii_cps(&label) {
        return ascii_label(&label);
    }
    let lowered: Vec<u32> = code_points(&lower_str(&cps_to_string(&label))).collect();
    if lowered.len() >= ACE_PREFIX.len() && lowered.iter().zip(ACE_PREFIX).all(|(&c, &p)| c == u32::from(p)) {
        return Err(fail("idna", 0, ACE_PREFIX.len(), "Label starts with ACE prefix"));
    }
    let mut out = ACE_PREFIX.to_vec();
    out.extend(punycode_encode(&label));
    if out.len() < 64 {
        return Ok(out);
    }
    Err(fail("idna", 0, label.len(), "label too long"))
}

/// `ToUnicode` para um rótulo em bytes (o único que o `Codec.decode` entrega).
fn to_unicode(label: &[u8]) -> Step<Vec<u32>> {
    if label.len() > 1024 {
        return Err(fail("idna", 0, label.len(), "label way too long"));
    }
    to_unicode_ascii(label)
}

/// `ToUnicode` para um rótulo `str` (o que o decodificador incremental entrega): o limite de 1024
/// vale na entrada (em bytes UTF-8 com `backslashreplace`), e um rótulo não ASCII passa antes pelo
/// nameprep e precisa sair ASCII.
fn to_unicode_str(label: &[u32]) -> Step<Vec<u32>> {
    if label.len() > 1024 {
        let size: usize = label
            .iter()
            .map(|&c| match c {
                0xD800..=0xDFFF => 6,
                0..=0x7F => 1,
                0x80..=0x7FF => 2,
                0x800..=0xFFFF => 3,
                _ => 4,
            })
            .sum();
        return Err(fail("idna", 0, size, "label way too long"));
    }
    if is_ascii_cps(label) {
        return to_unicode_ascii(&label.iter().map(|&c| c as u8).collect::<Vec<u8>>());
    }
    let label = nameprep(label)?;
    if let Some(start) = label.iter().position(|&c| c >= 128) {
        let end = start + label[start..].iter().take_while(|&&c| c >= 128).count();
        return Err(fail("idna", start, end, "Invalid character in IDN label"));
    }
    to_unicode_ascii(&label.iter().map(|&c| c as u8).collect::<Vec<u8>>())
}

/// O `ToUnicode` a partir do passo 3 (rótulo já em bytes ASCII).
fn to_unicode_ascii(label: &[u8]) -> Step<Vec<u32>> {
    let ascii = |bytes: &[u8]| match bytes.iter().position(|b| !b.is_ascii()) {
        Some(i) => Err(fail("idna", i, i + 1, "ordinal not in range(128)")),
        None => Ok(bytes.to_ascii_lowercase()),
    };
    if !label.to_ascii_lowercase().starts_with(ACE_PREFIX) {
        ascii(label)?;
        return Ok(label.iter().map(|&b| u32::from(b)).collect());
    }
    let offset = ACE_PREFIX.len();
    let result = punycode_decode_cps(&label[offset..], "strict").map_err(|f| f.shifted("idna", offset))?;
    let label2 = to_ascii(&result)?;
    if ascii(label)? != label2 {
        return Err(fail(
            "idna",
            0,
            label.len(),
            format!("IDNA does not round-trip, '{}' != '{}'", bytes_repr(label), bytes_repr(&label2)),
        ));
    }
    Ok(result)
}

/// `str.encode('idna', errors)`: `Codec.encode`.
pub fn idna_encode(input: &str, errors: &str) -> PyResult<Vec<u8>> {
    if errors != "strict" {
        return Err(unsupported_errors(errors));
    }
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let cps: Vec<u32> = code_points(input).collect();
    let raise = |f: Fault| f.raise("UnicodeEncodeError", Value::str(input));
    if is_ascii_cps(&cps) {
        // Nome ASCII: caminho rápido, só confere os tamanhos.
        let labels: Vec<&[u32]> = cps.split(|&c| c == 0x2E).collect();
        let mut offset = 0;
        for label in &labels[..labels.len() - 1] {
            if label.is_empty() {
                return Err(raise(fail("idna", offset, offset + 1, "label empty")));
            }
            offset += label.len() + 1;
        }
        offset = 0;
        for label in &labels {
            if label.len() >= 64 {
                return Err(raise(fail("idna", offset, offset + label.len(), "label too long")));
            }
            offset += label.len() + 1;
        }
        return Ok(cps.iter().map(|&c| c as u8).collect());
    }
    let mut labels: Vec<&[u32]> = cps.split(is_dot).collect();
    let trailing_dot = labels.last().is_some_and(|l| l.is_empty());
    if trailing_dot {
        labels.pop();
    }
    let mut result = Vec::new();
    let mut offset = 0;
    for label in labels {
        if !result.is_empty() {
            result.push(b'.');
        }
        result.extend(to_ascii(label).map_err(|f| raise(f.shifted("idna", offset)))?);
        offset += label.len() + 1;
    }
    if trailing_dot {
        result.push(b'.');
    }
    Ok(result)
}

/// `bytes.decode('idna', errors)`: `Codec.decode`.
pub fn idna_decode(data: &[u8], errors: &str) -> PyResult<String> {
    if errors != "strict" {
        return Err(unsupported_errors(errors));
    }
    if data.is_empty() {
        return Ok(String::new());
    }
    let has_ace = data.to_ascii_lowercase().windows(ACE_PREFIX.len()).any(|w| w == ACE_PREFIX);
    if !has_ace && data.is_ascii() {
        return Ok(data.iter().map(|&b| char::from(b)).collect());
    }
    let mut labels: Vec<&[u8]> = data.split(|&b| b == b'.').collect();
    let trailing_dot = labels.last().is_some_and(|l| l.is_empty());
    if trailing_dot {
        labels.pop();
    }
    let mut result: Vec<u32> = Vec::new();
    let mut offset = 0;
    for (i, label) in labels.iter().enumerate() {
        if i > 0 {
            result.push(0x2E);
        }
        let decoded = to_unicode(label).map_err(|f| f.shifted("idna", offset).raise("UnicodeDecodeError", Value::bytes(data)))?;
        result.extend(decoded);
        offset += label.len() + 1;
    }
    if trailing_dot {
        result.push(0x2E);
    }
    Ok(cps_to_string(&result))
}

/// O `label` (um `str`) de `ToASCII`/`ToUnicode`: o texto e seus pontos de código.
fn label_arg(fname: &str, args: Vec<Value>, kw: crate::object::Kw) -> PyResult<(String, Vec<u32>)> {
    let slots = crate::native_util::bind(fname, args, kw, &["label"], 1)?;
    match slots[0].as_ref() {
        Some(Value::Str(s)) => Ok((s.as_str().to_string(), code_points(s.as_str()).collect())),
        Some(other) => Err(crate::vm::type_error(format!("{fname}() argument 1 must be str, not {}", other.type_name()))),
        None => unreachable!("bind exige o argumento"),
    }
}

/// `ToASCII(label)` do `encodings/idna.py`, com o erro do próprio rótulo.
fn native_to_ascii(_vm: &mut crate::vm::Vm, args: Vec<Value>, kw: crate::object::Kw) -> PyResult<Value> {
    let (text, cps) = label_arg("ToASCII", args, kw)?;
    to_ascii(&cps).map(Value::bytes).map_err(|f| f.raise("UnicodeEncodeError", Value::str(text)))
}

/// `ToUnicode(label)` do `encodings/idna.py` para um rótulo `str`.
fn native_to_unicode(_vm: &mut crate::vm::Vm, args: Vec<Value>, kw: crate::object::Kw) -> PyResult<Value> {
    let (text, cps) = label_arg("ToUnicode", args, kw)?;
    to_unicode_str(&cps).map(|r| Value::str(cps_to_string(&r))).map_err(|f| f.raise("UnicodeEncodeError", Value::str(text)))
}

/// `_idna`: o `ToASCII` e o `ToUnicode` por rótulo, que os codecs incrementais de `codecs.py` chamam.
pub fn build(_vm: &mut crate::vm::Vm) -> std::rc::Rc<crate::object::ModuleObj> {
    crate::modules::ModuleBuilder::new("_idna").func("to_ascii", native_to_ascii).func("to_unicode", native_to_unicode).build()
}
