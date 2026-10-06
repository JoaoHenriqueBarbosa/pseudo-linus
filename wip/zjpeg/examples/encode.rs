//! `cargo run --example encode -- entrada.raw L|RGB|CMYK largura altura saida.jpg [chave=valor...]`:
//! codifica amostras cruas, para comparar byte a byte com o `Image.save` do Pillow no oráculo.
//! Chaves: `quality`, `subsampling`, `progressive`, `optimize`, `keep_rgb`, `restart`, `restart_rows`, `dpi`.

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pixels = std::fs::read(&a[1]).expect("ler entrada");
    let input = match a[2].as_str() {
        "L" => zjpeg::InputSpace::Grayscale,
        "CMYK" => zjpeg::InputSpace::Cmyk,
        "YCbCr" => zjpeg::InputSpace::YCbCr,
        _ => zjpeg::InputSpace::Rgb,
    };
    let mut o = zjpeg::EncodeOptions::new(a[3].parse().unwrap(), a[4].parse().unwrap(), input);
    for kv in &a[6..] {
        let (k, v) = kv.split_once('=').expect("chave=valor");
        match k {
            "quality" => o.quality = Some(v.parse().unwrap()),
            "subsampling" => o.subsampling = v.parse().unwrap(),
            "progressive" => o.progressive = v == "1",
            "optimize" => o.optimize = v == "1",
            "keep_rgb" => o.keep_rgb = v == "1",
            "restart" => o.restart_interval = v.parse().unwrap(),
            "restart_rows" => o.restart_in_rows = v.parse().unwrap(),
            "dpi" => {
                let d: u16 = v.parse().unwrap();
                o.dpi = Some((d, d));
            }
            _ => panic!("chave desconhecida {k}"),
        }
    }
    match zjpeg::encode(&o, &pixels) {
        Ok(b) => std::fs::write(&a[5], b).expect("gravar"),
        Err(e) => {
            eprintln!("erro: {e:?}");
            std::process::exit(1);
        }
    }
}
