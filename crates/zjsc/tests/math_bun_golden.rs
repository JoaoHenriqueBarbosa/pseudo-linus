//! `Math` contra o JavaScriptCore real (bun 1.4.2, glibc): `tests/golden/math_bun.tsv` sai de
//! `scripts/gen-math-golden.js`. Colunas: função, argumentos em hexadecimal (bits do double,
//! separados por vírgula), bits do resultado (NaN canônico, `7ff8000000000000`).
//!
//! Cada função abaixo repete o corpo do `mathProtoFuncX` de `upstream/JavaScriptCore/runtime/MathObject.cpp`
//! sobre `double`, chamando as mesmas operações que `src/runtime/math_object.rs` usa. As funções
//! transcendentes dependem da libm do sistema; divergências de último bit estão em
//! `wip-notes/math-audit.md`.
use zjsc::runtime::glibc_hyper;
use zjsc::runtime::math_common::{self, math};
use zjsc::runtime::math_object::operations;
use zjsc::wtf::precise_sum::PreciseSum;

const CANONICAL_NAN: u64 = 0x7ff8_0000_0000_0000;

fn bits(x: f64) -> u64 {
    if x.is_nan() { CANONICAL_NAN } else { x.to_bits() }
}

fn unary(name: &str, x: f64) -> Option<f64> {
    Some(match name {
        "abs" => x.abs(),
        "acos" => x.acos(),
        "acosh" => glibc_hyper::acosh(x),
        "asin" => x.asin(),
        "asinh" => glibc_hyper::asinh(x),
        "atan" => x.atan(),
        "atanh" => glibc_hyper::atanh(x),
        "cbrt" => glibc_hyper::cbrt(x),
        "ceil" => x.ceil(),
        "clz32" => math_common::to_uint32(x).leading_zeros() as f64,
        "cos" => x.cos(),
        "cosh" => glibc_hyper::cosh(x),
        "exp" => x.exp(),
        "expm1" => glibc_hyper::expm1(x),
        "floor" => x.floor(),
        "fround" => x as f32 as f64,
        "log" => x.ln(),
        "log10" => glibc_hyper::log10(x),
        "log1p" => glibc_hyper::log1p(x),
        "log2" => x.log2(),
        "round" => math_common::js_round(x),
        "sign" => operations::sign(x),
        "sin" => x.sin(),
        "sinh" => glibc_hyper::sinh(x),
        "sqrt" => x.sqrt(),
        "tan" => x.tan(),
        "tanh" => glibc_hyper::tanh(x),
        "trunc" => x.trunc(),
        "f16round" => operations::f16_round(x),
        _ => return None,
    })
}

fn binary(name: &str, a: f64, b: f64) -> Option<f64> {
    Some(match name {
        "atan2" => a.atan2(b),
        "pow" => math_common::operation_math_pow(a, b),
        "imul" => math_common::to_int32(a).wrapping_mul(math_common::to_int32(b)) as f64,
        "max" => math::js_max_double(a, b),
        "min" => math::js_min_double(a, b),
        _ => return None,
    })
}

fn compute(name: &str, args: &[f64]) -> f64 {
    if name == "hypot" {
        return operations::hypot(args);
    }
    if name == "sumPrecise" {
        let mut sum = PreciseSum::new();
        for &value in args {
            sum.add(value);
        }
        return sum.compute();
    }
    match args {
        [x] => unary(name, *x),
        [a, b] => binary(name, *a, *b),
        _ => None,
    }
    .unwrap_or_else(|| panic!("função desconhecida no golden: {name}/{}", args.len()))
}

#[test]
fn math_matches_javascriptcore() {
    let golden = include_str!("golden/math_bun.tsv");
    let mut checked = 0usize;
    let mut divergences: Vec<String> = Vec::new();
    let mut by_function: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();

    for (index, line) in golden.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let line_number = index + 1;
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 3, "linha {line_number}: esperava 3 colunas, achou {}", f.len());

        let args: Vec<f64> = f[1]
            .split(',')
            .filter(|hex| !hex.is_empty())
            .map(|hex| f64::from_bits(u64::from_str_radix(hex, 16).expect("bits em hexadecimal")))
            .collect();
        let want = u64::from_str_radix(f[2], 16).expect("bits em hexadecimal");
        let got = bits(compute(f[0], &args));
        checked += 1;
        if want != got {
            *by_function.entry(f[0].to_string()).or_default() += 1;
            divergences.push(format!(
                "linha {line_number} {}({}): esperado {want:016x}, obtido {got:016x}",
                f[0], f[1]
            ));
        }
    }

    assert!(checked > 0, "o golden está vazio");
    assert!(
        divergences.is_empty(),
        "{} divergências em {checked} comparações; por função: {by_function:?}; as 20 primeiras:\n{}",
        divergences.len(),
        divergences.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}
