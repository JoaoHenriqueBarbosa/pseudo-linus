//! `WTF::parseDouble` (fast_float) contra o bun 1.4.2 (`scripts/gen-parse-double-golden.js`).
use zjsc::wtf::fast_float::parse_double;

#[test]
fn matches_bun() {
    let mut bad = Vec::new();
    for line in include_str!("golden/parse_double.tsv").lines() {
        let (s, bits) = line.split_once('\t').unwrap();
        let want = u64::from_str_radix(bits, 16).unwrap();
        let mut len = 0usize;
        let got = parse_double(s.as_bytes(), &mut len).to_bits();
        let got16 = parse_double(&s.encode_utf16().collect::<Vec<u16>>(), &mut len).to_bits();
        if (got, got16, len) != (want, want, s.len()) && bad.len() < 20 {
            bad.push(format!("{s}: esperado {want:016x} obtido {got:016x}/{got16:016x} len {len}"));
        }
    }
    assert!(bad.is_empty(), "divergências:\n{}", bad.join("\n"));
}
