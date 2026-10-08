//! Formatação de número da WTF contra o JavaScriptCore real: `tests/golden/number_to_string.tsv`
//! sai de `scripts/gen-number-golden.js`, rodado no bun 1.4.2. Colunas: bits do double em
//! hexadecimal, `String(x)`, `x.toFixed(k)`, `k`, `x.toPrecision(p)`, `p`, `x.toExponential(k)`;
//! `!RangeError` quando o JavaScript lança.
//!
//! As quatro funções abaixo reproduzem `numberProtoFuncToString` (base 10), `numberProtoFuncToFixed`,
//! `numberProtoFuncToPrecision` e `numberProtoFuncToExponential` de
//! `upstream/JavaScriptCore/runtime/NumberPrototype.cpp`, chamando só a WTF portada. A
//! `NumberPrototype` de verdade (com `JSValue`, `ThrowScope` e `toIntegerOrInfinity`) vem numa camada
//! futura; aqui o argumento já chega inteiro, que é o que o gerador do golden passa.
use zjsc::wtf::dtoa::double_conversion::DoubleToStringConverter;
use zjsc::wtf::dtoa::utils::StringBuilder;
use zjsc::wtf::dtoa::{
    number_to_fixed_precision_string, number_to_fixed_width_string, number_to_string_and_size,
    NumberToStringBuffer,
};

/// O que a função nativa devolve: o texto, ou o `RangeError` lançado.
type JsResult = Result<String, &'static str>;

fn ascii(span: &[u8]) -> String {
    String::from_utf8(span.to_vec()).expect("a WTF produz só ASCII")
}

/// `String::number(double)`: `numberToStringAndSize` num buffer local.
fn string_number(x: f64) -> String {
    let mut buffer: NumberToStringBuffer = [0; 124];
    ascii(number_to_string_and_size(x, &mut buffer))
}

/// `truncateDoubleToInt32` no x86_64 (`cvttsd2si`): NaN e fora da faixa dão `INT32_MIN`.
fn truncate_double_to_int32(number: f64) -> i32 {
    if number.is_nan() || number <= -2147483649.0 || number >= 2147483648.0 {
        return i32::MIN;
    }
    number as i32
}

/// `numberToStringInternal(vm, doubleValue, 10)`. O caminho `int32ToStringInternal` imprime o inteiro
/// com `String::number(int)`, que ainda não está portado (`wtf/text`); para um `i32` o texto é o
/// mesmo do caminho de `double`, então o teste passa o inteiro por `String::number(double)`.
fn number_proto_func_to_string(double_value: f64) -> JsResult {
    let integer_value = truncate_double_to_int32(double_value);
    if integer_value as f64 == double_value {
        return Ok(string_number(integer_value as f64));
    }

    // radix == 10
    Ok(string_number(double_value))
}

fn number_proto_func_to_fixed(x: f64, decimal_places_double: f64) -> JsResult {
    if decimal_places_double < 0.0 || decimal_places_double > 100.0 {
        return Err("RangeError");
    }
    let decimal_places = decimal_places_double as i32;

    // 15.7.4.5.7 states "If x >= 10^21, then let m = ToString(x)"
    // This also covers Ininity, and structure the check so that NaN
    // values are also handled by numberToString
    if !(x.abs() < 1e+21) {
        return Ok(string_number(x));
    }

    // The check above will return false for NaN or Infinity, these will be
    // handled by numberToString.
    assert!(x.is_finite());

    // String::numberToStringFixedWidth(x, decimalPlaces)
    let mut buffer: NumberToStringBuffer = [0; 124];
    Ok(ascii(number_to_fixed_width_string(x, decimal_places as u32, &mut buffer)))
}

fn number_proto_func_to_precision(x: f64, significant_figures_double: f64) -> JsResult {
    // Handle NaN and Infinity.
    if !x.is_finite() {
        return Ok(string_number(x));
    }

    if significant_figures_double < 1.0 || significant_figures_double > 100.0 {
        return Err("RangeError");
    }
    let significant_figures = significant_figures_double as i32;

    // String::numberToStringFixedPrecision(x, significantFigures, TrailingZerosPolicy::Keep)
    let mut buffer: NumberToStringBuffer = [0; 124];
    Ok(ascii(number_to_fixed_precision_string(x, significant_figures as u32, &mut buffer, false)))
}

fn number_proto_func_to_exponential(x: f64, decimal_places_double: f64) -> JsResult {
    // Handle NaN and Infinity.
    if !x.is_finite() {
        return Ok(string_number(x));
    }

    if decimal_places_double < 0.0 || decimal_places_double > 100.0 {
        return Err("RangeError");
    }
    let decimal_places = decimal_places_double as i32;

    // Round if the argument is not undefined, always format as exponential.
    // (O gerador do golden sempre passa o argumento, então o ramo de `undefined`, que usa
    // `dragonbox::ToExponential`, não entra aqui.)
    let mut buffer: NumberToStringBuffer = [0; 124];
    let length = {
        let mut builder = StringBuilder::new(&mut buffer[..]);
        builder.reset();
        let converter = DoubleToStringConverter::ecma_script_converter();
        converter.to_exponential(x, decimal_places, &mut builder);
        builder.finalize().len()
    };
    Ok(ascii(&buffer[..length]))
}

/// A coluna do golden vira o resultado esperado: `!RangeError` é o erro lançado.
fn expected(column: &str) -> JsResult {
    match column.strip_prefix('!') {
        Some(error) if error == "RangeError" => Err("RangeError"),
        Some(error) => panic!("erro inesperado no golden: {error}"),
        None => Ok(column.to_string()),
    }
}

fn show(result: &JsResult) -> String {
    match result {
        Ok(text) => text.clone(),
        Err(error) => format!("!{error}"),
    }
}

#[test]
fn number_formatting_matches_javascriptcore() {
    let golden = include_str!("golden/number_to_string.tsv");
    let mut checked = 0usize;
    let mut divergences: Vec<String> = Vec::new();

    for (index, line) in golden.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let line_number = index + 1;
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 7, "linha {line_number}: esperava 7 colunas, achou {}", f.len());

        let bits = u64::from_str_radix(f[0], 16).expect("bits em hexadecimal");
        let x = f64::from_bits(bits);
        let k: f64 = f[3].parse().expect("k inteiro");
        let p: f64 = f[5].parse().expect("p inteiro");

        let cases: [(&str, JsResult, JsResult); 4] = [
            ("String(x)", expected(f[1]), number_proto_func_to_string(x)),
            ("toFixed(k)", expected(f[2]), number_proto_func_to_fixed(x, k)),
            ("toPrecision(p)", expected(f[4]), number_proto_func_to_precision(x, p)),
            ("toExponential(k)", expected(f[6]), number_proto_func_to_exponential(x, k)),
        ];

        for (name, want, got) in cases {
            checked += 1;
            if want != got {
                divergences.push(format!(
                    "linha {line_number} bits {} {name} (k={k}, p={p}): esperado {:?}, obtido {:?}",
                    f[0],
                    show(&want),
                    show(&got)
                ));
            }
        }
    }

    assert!(checked > 0, "o golden está vazio");
    assert!(
        divergences.is_empty(),
        "{} divergências em {checked} comparações; as 20 primeiras:\n{}",
        divergences.len(),
        divergences.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}
