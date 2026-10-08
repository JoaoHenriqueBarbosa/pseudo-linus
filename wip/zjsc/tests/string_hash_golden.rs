//! Hash de string da WTF contra o C++ real: `tests/golden/string_hash.txt` sai de
//! `scripts/oracle/string_hash.cpp`, compilado com os flags do build do WebKit do Bun.
use zjsc::wtf::text::string_hasher::compute_hash_and_mask_top8_bits;

#[test]
fn matches_cpp_oracle() {
    let latin1: Vec<u8> = (0..256u32).map(|i| (i.wrapping_mul(37).wrapping_add(11)) as u8).collect();
    let utf16: Vec<u16> = (0..64u32).map(|i| (0x100 + i * 977) as u16).collect();
    let mut bad = Vec::new();
    for line in include_str!("golden/string_hash.txt").lines() {
        let f: Vec<&str> = line.split(' ').collect();
        let n: usize = f[1].parse().unwrap();
        let want: u32 = f[2].parse().unwrap();
        let got = if f[0] == "L" {
            compute_hash_and_mask_top8_bits(&latin1[..n])
        } else {
            compute_hash_and_mask_top8_bits(&utf16[..n])
        };
        if got != want {
            bad.push(format!("{} {}: esperado {} obtido {}", f[0], n, want, got));
        }
    }
    assert!(bad.is_empty(), "{} divergências:\n{}", bad.len(), bad.join("\n"));
}
