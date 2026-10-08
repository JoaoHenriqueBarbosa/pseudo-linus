//! Conformidade do `printf` e da conversão de REAL contra o sqlite3 3.46.1 do Debian 13.
//!
//! `tests/data/printf_cases.tsv` lista (formato, tipo do argumento, argumento); o arquivo
//! `printf_golden.txt` tem a saída de `select printf(fmt, arg)` no oráculo, uma linha por caso.
//! Para regerar: veja o comando no README do testbench (docker run da imagem do oráculo).

use zsqlite::printf::{PrintfArguments, PrintfArgs, PrintfValue, StrAccum};

enum Val {
    Int(i64),
    Real(f64),
    Text(Vec<u8>),
    Null,
}

impl PrintfValue for Val {
    fn value_int64(&mut self) -> i64 {
        match self {
            Val::Int(i) => *i,
            // sqlite3_value_int64 de REAL: truncamento com saturação (doubleToInt64).
            Val::Real(r) => {
                if r.is_nan() {
                    0
                } else if *r >= 9223372036854775807.0 {
                    i64::MAX
                } else if *r <= -9223372036854775808.0 {
                    i64::MIN
                } else {
                    *r as i64
                }
            }
            Val::Text(t) => {
                let mut n = 0i64;
                zsqlite::util::atoi64(t, &mut n, t.len() as i32, 1);
                n
            }
            Val::Null => 0,
        }
    }
    fn value_double(&mut self) -> f64 {
        match self {
            Val::Int(i) => *i as f64,
            Val::Real(r) => *r,
            Val::Text(t) => zsqlite::util::atof(t, t.len() as i32, 1, true).1,
            Val::Null => 0.0,
        }
    }
    fn value_text(&mut self) -> Option<Vec<u8>> {
        match self {
            Val::Int(i) => Some(i.to_string().into_bytes()),
            Val::Real(r) => zsqlite::printf::mprintf(b"%!.15g", &[zsqlite::printf::PrintfArg::Double(*r)]),
            Val::Text(t) => Some(t.clone()),
            Val::Null => None,
        }
    }
}

#[test]
fn printf_matches_oracle() {
    let cases = include_str!("data/printf_cases.tsv");
    let golden = include_str!("data/printf_golden.txt");
    let mut failures = Vec::new();
    for (n, (case, want)) in cases.lines().zip(golden.lines()).enumerate() {
        let mut it = case.split('\t');
        let fmt = it.next().unwrap();
        let kind = it.next().unwrap();
        let arg = it.next().unwrap_or("");
        let mut v = match kind {
            "i" => Val::Int(arg.parse().unwrap()),
            "d" => Val::Real(arg.parse().unwrap()),
            "s" => Val::Text(arg.as_bytes().to_vec()),
            _ => Val::Null,
        };
        let mut args = PrintfArguments { n_used: 0, ap_arg: vec![&mut v] };
        let mut acc = StrAccum::new(1_000_000_000);
        acc.printf_flags |= zsqlite::consts::SQLITE_PRINTF_SQLFUNC;
        acc.str_vappendf(fmt.as_bytes(), PrintfArgs::SqlFunc(&mut args));
        let got = String::from_utf8_lossy(&acc.finish().unwrap_or_default()).into_owned();
        if got != want {
            failures.push(format!("caso {} fmt={:?} kind={} arg={:?}: obtido {:?}, oráculo {:?}", n + 1, fmt, kind, arg, got, want));
        }
    }
    assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
}
