//! Comprime o stdin com `compress2` no nível dado (padrão 6) e escreve no stdout. Serve para comparar
//! byte a byte com o zlib de verdade.

use std::io::{Read, Write};

fn main() {
    let level: i32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(6);
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input).expect("leitura do stdin");
    let out = zdeflate::compress(&input, level).expect("compress");
    std::io::stdout().write_all(&out).expect("escrita no stdout");
}
