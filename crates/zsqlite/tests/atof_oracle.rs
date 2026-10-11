//! `atof` (conversão texto para REAL com `long double` de 80 bits) e `fp_decode` contra o oráculo.

use zsqlite::printf::{mprintf, PrintfArg};

#[test]
fn atof_matches_oracle() {
    let cases = include_str!("data/atof_cases.txt");
    let golden = include_str!("data/atof_golden.txt");
    let mut failures = Vec::new();
    for (n, (case, want)) in cases.lines().zip(golden.lines()).enumerate() {
        let (_rc, r) = zsqlite::util::atof(case.as_bytes(), case.len() as i32, 1, true);
        let got = mprintf(b"%!.25e|%!.15g", &[PrintfArg::Double(r), PrintfArg::Double(r)]).unwrap_or_default();
        let got = String::from_utf8_lossy(&got).into_owned();
        if got != want {
            failures.push(format!("caso {} {:?}: obtido {:?}, oráculo {:?}", n + 1, case, got, want));
        }
    }
    assert!(failures.is_empty(), "{} divergências:\n{}", failures.len(), failures.join("\n"));
}
