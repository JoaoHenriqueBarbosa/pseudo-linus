//! Níveis de embutimento do fribidi para cada linha da entrada padrão, no formato do
//! `bidiref` da bancada: `direção-resolvida max níveis...`. O primeiro argumento é a direção
//! do parágrafo (`ltr`, `rtl` ou `on`).

use std::io::BufRead;

use zhb::bidi;

fn main() {
    let par = match std::env::args().nth(1).as_deref() {
        Some("ltr") => bidi::PAR_LTR,
        Some("rtl") => bidi::PAR_RTL,
        _ => bidi::PAR_ON,
    };
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let text: Vec<u32> = line.chars().map(u32::from).collect();
        let types = bidi::bidi_types(&text);
        let brackets = bidi::bracket_types(&text, &types);
        let mut dir = par;
        let mut levels = vec![0; text.len()];
        let max = bidi::par_embedding_levels(&types, &brackets, &mut dir, &mut levels);
        let lv: String = levels.iter().map(|l| format!(" {l}")).collect();
        println!("{dir:x} {max}{lv}");
    }
}
