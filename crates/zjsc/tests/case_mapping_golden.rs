//! Mudança de caixa contra o bun 1.4.2 (`scripts/gen-case-golden.js`).
use zjsc::wtf::unicode::case_mapping::{str_to_lower, str_to_upper};

fn parse(f: &str) -> Vec<u16> {
    f.split(' ').filter(|x| !x.is_empty()).map(|x| u16::from_str_radix(x, 16).unwrap()).collect()
}

#[test]
fn matches_bun() {
    let mut bad = Vec::new();
    for line in include_str!("golden/case_mapping.tsv").lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (s, lo, up) = (parse(f[0]), parse(f[1]), parse(f[2]));
        if str_to_lower(&s) != lo {
            bad.push(format!("lower {}: esperado {} obtido {:x?}", f[0], f[1], str_to_lower(&s)));
        }
        if str_to_upper(&s) != up {
            bad.push(format!("upper {}: esperado {} obtido {:x?}", f[0], f[2], str_to_upper(&s)));
        }
    }
    assert!(bad.is_empty(), "{} divergências:\n{}", bad.len(), bad.iter().take(20).cloned().collect::<Vec<_>>().join("\n"));
}
