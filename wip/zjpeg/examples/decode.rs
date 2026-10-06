//! `cargo run --example decode -- arquivo.jpg saida.raw [L|RGB|CMYK]`: decodifica e grava as
//! amostras cruas, para comparar com o `Image.tobytes()` do Pillow no oráculo.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).expect("ler entrada");
    let mut opts = zjpeg::Options::default();
    if let Some(m) = args.get(3) {
        opts.out_color_space = Some(match m.as_str() {
            "L" => zjpeg::ColorSpace::Grayscale,
            "CMYK" => zjpeg::ColorSpace::Cmyk,
            _ => zjpeg::ColorSpace::Rgb,
        });
    }
    match zjpeg::decode(&data, &opts) {
        Ok(d) => {
            std::fs::write(&args[2], &d.data).expect("gravar saída");
            eprintln!("{}x{}x{} {:?}", d.width, d.height, d.components, d.warnings);
        }
        Err(e) => {
            eprintln!("erro: {e}");
            std::process::exit(1);
        }
    }
}
