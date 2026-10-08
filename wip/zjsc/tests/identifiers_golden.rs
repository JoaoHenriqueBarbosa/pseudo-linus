//! Tabelas `ID_Start`/`ID_Continue` (com os acréscimos do `Lexer.cpp`: `$`, `_`, ZWNJ e ZWJ na
//! continuação) contra o bun 1.4.2 (`scripts/gen-ident-golden.js`).
use zjsc::wtf::unicode::{is_id_continue, is_id_start};

#[test]
fn matches_bun() {
    let runs: Vec<(u32, bool, bool)> = include_str!("golden/identifiers.txt")
        .lines()
        .map(|l| {
            let (a, v) = l.split_once(' ').unwrap();
            let b = v.as_bytes();
            (u32::from_str_radix(a, 16).unwrap(), b[0] == b'1', b[1] == b'1')
        })
        .collect();
    let mut bad = Vec::new();
    for (i, &(start, s, c)) in runs.iter().enumerate() {
        let end = runs.get(i + 1).map_or(0x110000, |r| r.0);
        for cp in start..end {
            if (0xD800..0xE000).contains(&cp) {
                continue;
            }
            let our_s = cp == '$' as u32 || cp == '_' as u32 || is_id_start(cp);
            let our_c = our_s || is_id_continue(cp) || cp == 0x200C || cp == 0x200D;
            if (our_s, our_c) != (s, c) && bad.len() < 20 {
                bad.push(format!("U+{cp:04X}: esperado {s}/{c} obtido {our_s}/{our_c}"));
            }
        }
    }
    assert!(bad.is_empty(), "divergências:\n{}", bad.join("\n"));
}
