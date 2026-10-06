//! Módulo `unicodedata`: normalização (NFC/NFD/NFKC/NFKD), categoria geral, classe de combinação,
//! nome e dígitos de um caractere.

use std::rc::Rc;

use icu_normalizer::{ComposingNormalizer, DecomposingNormalizer};
use icu_properties::props::{CanonicalCombiningClass, GeneralCategory};
use icu_properties::CodePointMapData;

use crate::modules::ModuleBuilder;
use crate::native_util::bind;
use crate::object::{Kw, ModuleObj, Value};
use crate::vm::{exc, type_error, PyResult, Vm};

fn one_char(fname: &str, v: Option<&Value>) -> PyResult<char> {
    match v {
        Some(Value::Str(s)) => {
            let mut it = s.as_str().chars();
            match (it.next(), it.next()) {
                (Some(c), None) => Ok(c),
                _ => Err(type_error(format!("{fname}() argument must be a unicode character, not str"))),
            }
        }
        Some(other) => Err(type_error(format!("{fname}() argument must be a unicode character, not {}", other.type_name()))),
        None => Err(type_error(format!("{fname}() takes at least 1 argument (0 given)"))),
    }
}

pub(crate) fn category_code(c: char) -> &'static str {
    use GeneralCategory as G;
    match CodePointMapData::<GeneralCategory>::new().get(c) {
        G::Control => "Cc",
        G::Format => "Cf",
        G::Unassigned => "Cn",
        G::PrivateUse => "Co",
        G::Surrogate => "Cs",
        G::LowercaseLetter => "Ll",
        G::ModifierLetter => "Lm",
        G::OtherLetter => "Lo",
        G::TitlecaseLetter => "Lt",
        G::UppercaseLetter => "Lu",
        G::SpacingMark => "Mc",
        G::EnclosingMark => "Me",
        G::NonspacingMark => "Mn",
        G::DecimalNumber => "Nd",
        G::LetterNumber => "Nl",
        G::OtherNumber => "No",
        G::ConnectorPunctuation => "Pc",
        G::DashPunctuation => "Pd",
        G::ClosePunctuation => "Pe",
        G::FinalPunctuation => "Pf",
        G::InitialPunctuation => "Pi",
        G::OtherPunctuation => "Po",
        G::OpenPunctuation => "Ps",
        G::CurrencySymbol => "Sc",
        G::ModifierSymbol => "Sk",
        G::MathSymbol => "Sm",
        G::OtherSymbol => "So",
        G::LineSeparator => "Zl",
        G::ParagraphSeparator => "Zp",
        G::SpaceSeparator => "Zs",
    }
}

fn category(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("category", args, kw, &["chr"], 1)?;
    Ok(Value::str(category_code(one_char("category", a[0].as_ref())?).to_string()))
}

fn east_asian_width(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    use icu_properties::props::EastAsianWidth as W;
    let a = bind("east_asian_width", args, kw, &["chr"], 1)?;
    let c = one_char("east_asian_width", a[0].as_ref())?;
    let code = match CodePointMapData::<W>::new().get(c) {
        W::Ambiguous => "A",
        W::Fullwidth => "F",
        W::Halfwidth => "H",
        W::Narrow => "Na",
        W::Wide => "W",
        _ => "N",
    };
    Ok(Value::str(code.to_string()))
}

fn bidirectional(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    use icu_properties::props::BidiClass as B;
    let a = bind("bidirectional", args, kw, &["chr"], 1)?;
    let c = one_char("bidirectional", a[0].as_ref())?;
    if category_code(c) == "Cn" {
        return Ok(Value::str(String::new()));
    }
    let code = match CodePointMapData::<B>::new().get(c) {
        B::LeftToRight => "L",
        B::RightToLeft => "R",
        B::ArabicLetter => "AL",
        B::EuropeanNumber => "EN",
        B::EuropeanSeparator => "ES",
        B::EuropeanTerminator => "ET",
        B::ArabicNumber => "AN",
        B::CommonSeparator => "CS",
        B::NonspacingMark => "NSM",
        B::BoundaryNeutral => "BN",
        B::ParagraphSeparator => "B",
        B::SegmentSeparator => "S",
        B::WhiteSpace => "WS",
        B::LeftToRightEmbedding => "LRE",
        B::LeftToRightOverride => "LRO",
        B::RightToLeftEmbedding => "RLE",
        B::RightToLeftOverride => "RLO",
        B::PopDirectionalFormat => "PDF",
        B::LeftToRightIsolate => "LRI",
        B::RightToLeftIsolate => "RLI",
        B::FirstStrongIsolate => "FSI",
        B::PopDirectionalIsolate => "PDI",
        _ => "ON",
    };
    Ok(Value::str(code.to_string()))
}

fn decomposition(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("decomposition", args, kw, &["chr"], 1)?;
    let c = one_char("decomposition", a[0].as_ref())?;
    let s = c.to_string();
    // Hangul e uso de decomposição completa: o CPython mostra só o mapeamento de um passo; aqui
    // a decomposição canônica ou de compatibilidade (NFD/NFKD) é a aproximação.
    let canon = DecomposingNormalizer::new_nfd().normalize(&s).into_owned();
    let (tag, text) = if canon != s {
        ("", canon)
    } else {
        let compat = DecomposingNormalizer::new_nfkd().normalize(&s).into_owned();
        if compat == s {
            return Ok(Value::str(String::new()));
        }
        ("<compat> ", compat)
    };
    let hex: Vec<String> = text.chars().map(|ch| format!("{:04X}", ch as u32)).collect();
    Ok(Value::str(format!("{tag}{}", hex.join(" "))))
}

fn mirrored(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    use icu_properties::props::BidiMirrored;
    use icu_properties::CodePointSetData;
    let a = bind("mirrored", args, kw, &["chr"], 1)?;
    let c = one_char("mirrored", a[0].as_ref())?;
    Ok(Value::Int(i64::from(CodePointSetData::new::<BidiMirrored>().contains(c))))
}

fn combining(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("combining", args, kw, &["chr"], 1)?;
    let c = one_char("combining", a[0].as_ref())?;
    Ok(Value::Int(i64::from(CodePointMapData::<CanonicalCombiningClass>::new().get(c).to_icu4c_value())))
}

fn name(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("name", args, kw, &["chr", "default"], 1)?;
    let c = one_char("name", a[0].as_ref())?;
    match unicode_names2::name(c) {
        Some(n) => Ok(Value::str(n.to_string())),
        None => match a[1].clone() {
            Some(d) => Ok(d),
            None => Err(exc("ValueError", "no such name")),
        },
    }
}

fn lookup(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("lookup", args, kw, &["name"], 1)?;
    let Some(Value::Str(s)) = a[0].as_ref() else {
        return Err(type_error("lookup() argument must be str"));
    };
    match unicode_names2::character(s.as_str()) {
        Some(c) => Ok(Value::str(c.to_string())),
        None => Err(exc("KeyError", format!("undefined character name '{}'", s.as_str()))),
    }
}

fn normalize(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("normalize", args, kw, &["form", "unistr"], 2)?;
    let (Some(Value::Str(form)), Some(Value::Str(s))) = (a[0].as_ref(), a[1].as_ref()) else {
        return Err(type_error("normalize() argument 2 must be str"));
    };
    let out = match form.as_str() {
        "NFC" => ComposingNormalizer::new_nfc().normalize(s.as_str()).into_owned(),
        "NFKC" => ComposingNormalizer::new_nfkc().normalize(s.as_str()).into_owned(),
        "NFD" => DecomposingNormalizer::new_nfd().normalize(s.as_str()).into_owned(),
        "NFKD" => DecomposingNormalizer::new_nfkd().normalize(s.as_str()).into_owned(),
        _ => return Err(exc("ValueError", "invalid normalization form")),
    };
    Ok(Value::str(out))
}

fn is_normalized(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("is_normalized", args, kw, &["form", "unistr"], 2)?;
    let (Some(Value::Str(form)), Some(Value::Str(s))) = (a[0].as_ref(), a[1].as_ref()) else {
        return Err(type_error("is_normalized() argument 2 must be str"));
    };
    let text = s.as_str();
    let ok = match form.as_str() {
        "NFC" => ComposingNormalizer::new_nfc().is_normalized(text),
        "NFKC" => ComposingNormalizer::new_nfkc().is_normalized(text),
        "NFD" => DecomposingNormalizer::new_nfd().is_normalized(text),
        "NFKD" => DecomposingNormalizer::new_nfkd().is_normalized(text),
        _ => return Err(exc("ValueError", "invalid normalization form")),
    };
    Ok(Value::Bool(ok))
}

/// Valor de um dígito decimal Unicode: os `Nd` vêm em corridas contíguas de dez, começando no zero.
fn decimal_value(c: char) -> Option<u32> {
    let nd = |ch: char| CodePointMapData::<GeneralCategory>::new().get(ch) == GeneralCategory::DecimalNumber;
    if !nd(c) {
        return None;
    }
    let mut start = c as u32;
    while start > 0 && char::from_u32(start - 1).is_some_and(nd) {
        start -= 1;
    }
    Some((c as u32 - start) % 10)
}

/// Troca os dígitos decimais Unicode (`Nd`) de um texto não ASCII pelos ASCII (`int('٣')` é 3).
/// `None` se o texto já é ASCII ou não tem nada a trocar.
pub fn fold_decimal_digits(s: &str) -> Option<String> {
    if s.is_ascii() {
        return None;
    }
    let mut changed = false;
    let out: String = s
        .chars()
        .map(|c| match (c.is_ascii(), decimal_value(c)) {
            (false, Some(d)) => {
                changed = true;
                char::from_digit(d, 10).unwrap_or(c)
            }
            _ => c,
        })
        .collect();
    changed.then_some(out)
}

fn fraction(c: char) -> Option<f64> {
    Some(match c {
        '\u{bc}' => 0.25,
        '\u{bd}' => 0.5,
        '\u{be}' => 0.75,
        '\u{2150}' => 1.0 / 7.0,
        '\u{2151}' => 1.0 / 9.0,
        '\u{2152}' => 0.1,
        '\u{2153}' => 1.0 / 3.0,
        '\u{2154}' => 2.0 / 3.0,
        '\u{2155}' => 0.2,
        '\u{2156}' => 0.4,
        '\u{2157}' => 0.6,
        '\u{2158}' => 0.8,
        '\u{2159}' => 1.0 / 6.0,
        '\u{215a}' => 5.0 / 6.0,
        '\u{215b}' => 0.125,
        '\u{215c}' => 0.375,
        '\u{215d}' => 0.625,
        '\u{215e}' => 0.875,
        _ => return None,
    })
}

/// Valor numérico de `c`: dígito decimal, números romanos, circulados e frações comuns.
fn numeric_value(c: char) -> Option<f64> {
    if let Some(d) = decimal_value(c) {
        return Some(f64::from(d));
    }
    if let Some(f) = fraction(c) {
        return Some(f);
    }
    let u = c as u32;
    Some(match u {
        0xb2 => 2.0,
        0xb3 => 3.0,
        0xb9 => 1.0,
        0x2070 => 0.0,
        0x2074..=0x2079 => f64::from(u - 0x2070),
        0x2080..=0x2089 => f64::from(u - 0x2080),
        0x2460..=0x2473 => f64::from(u - 0x2460 + 1),
        0x2160..=0x216b => f64::from(u - 0x2160 + 1),
        0x2170..=0x217b => f64::from(u - 0x2170 + 1),
        0x216c | 0x217c => 50.0,
        0x216d | 0x217d => 100.0,
        0x216e | 0x217e => 500.0,
        0x216f | 0x217f => 1000.0,
        _ => return None,
    })
}

fn digit_like(fname: &'static str, only_decimal: bool, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind(fname, args, kw, &["chr", "default"], 1)?;
    let c = one_char(fname, a[0].as_ref())?;
    let v = if only_decimal {
        decimal_value(c).map(f64::from)
    } else {
        numeric_value(c).filter(|n| n.fract() == 0.0 && *n <= 9.0 && (decimal_value(c).is_some() || matches!(c as u32, 0xb2 | 0xb3 | 0xb9 | 0x2070..=0x2079 | 0x2080..=0x2089 | 0x2460..=0x2468)))
    };
    match v {
        Some(n) => Ok(Value::Int(n as i64)),
        None => match a[1].clone() {
            Some(d) => Ok(d),
            None => Err(exc("ValueError", if only_decimal { "not a decimal" } else { "not a digit" })),
        },
    }
}

fn decimal(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    digit_like("decimal", true, args, kw)
}

fn digit(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    digit_like("digit", false, args, kw)
}

fn numeric(_vm: &mut Vm, args: Vec<Value>, kw: Kw) -> PyResult<Value> {
    let a = bind("numeric", args, kw, &["chr", "default"], 1)?;
    let c = one_char("numeric", a[0].as_ref())?;
    match numeric_value(c) {
        Some(n) => Ok(Value::Float(n)),
        None => match a[1].clone() {
            Some(d) => Ok(d),
            None => Err(exc("ValueError", "not a numeric character")),
        },
    }
}

pub fn build(_vm: &mut Vm) -> Rc<ModuleObj> {
    ModuleBuilder::new("unicodedata")
        .func("category", category)
        .func("combining", combining)
        .func("east_asian_width", east_asian_width)
        .func("bidirectional", bidirectional)
        .func("mirrored", mirrored)
        .func("decomposition", decomposition)
        .func("name", name)
        .func("lookup", lookup)
        .func("normalize", normalize)
        .func("is_normalized", is_normalized)
        .func("decimal", decimal)
        .func("digit", digit)
        .func("numeric", numeric)
        .value("unidata_version", Value::str("15.1.0".to_string()))
        .build()
}
