//! Bancada: imprime o shaping como o `raqmref` (índice, avanços, offsets e cluster por glifo).
//! uso: shape fonte.ttf tamanho dir texto [features...]

use std::cell::RefCell;

use zhb::buffer::tag;
use zhb::font::{Font, Tables};
use zhb::raqm::{layout, ParDirection};
use zhb::shape::Feature;

fn parse_feature(s: &str) -> Option<Feature> {
    let (name, value) = match s.split_once('=') {
        Some((n, v)) => (n, v.parse().ok()?),
        None => match s.strip_prefix('-') {
            Some(n) => (n, 0),
            None => (s.strip_prefix('+').unwrap_or(s), 1),
        },
    };
    let mut t = [b' '; 4];
    for (d, b) in t.iter_mut().zip(name.bytes()) {
        *d = b;
    }
    Some(Feature { tag: tag(&t), value, start: Feature::GLOBAL_START, end: Feature::GLOBAL_END })
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).expect("fonte");
    let size: f64 = args[2].parse().expect("tamanho");
    let mut face = zft::Face::new(data, 0).expect("face");
    face.request_size(0, (size * 64.0) as i64).expect("tamanho");
    let tables = Tables::load(&face);
    let face = RefCell::new(face);
    let font = Font::new(&face, &tables);
    let text: Vec<u32> = args[4].chars().map(u32::from).collect();
    let features: Vec<Feature> = args[5..].iter().filter_map(|s| parse_feature(s)).collect();

    let dir = match args[3].as_str() {
        "rtl" => ParDirection::Rtl,
        "ltr" => ParDirection::Ltr,
        "ttb" => ParDirection::Ttb,
        _ => ParDirection::Default,
    };
    let out = layout(&font, &text, dir, None, &features);
    // O raqm com texto UTF-8 devolve o cluster como offset em bytes.
    let byte_at: Vec<usize> = args[4].char_indices().map(|(b, _)| b).collect();
    for g in &out.glyphs {
        let cluster = byte_at.get(g.cluster as usize).copied().unwrap_or(args[4].len());
        println!("{} {} {} {} {} {}", g.index, g.x_advance, g.y_advance, g.x_offset, g.y_offset, cluster);
    }
}
