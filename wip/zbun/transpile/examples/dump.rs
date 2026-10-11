//! `cargo run -p bun_transpile --example dump -- ARQUIVO [--map]`: imprime o texto que o runtime entregaria ao JSC e,
//! com `--map`, os segmentos do mapa (`linha:coluna -> linha:coluna`, base 1 nos dois lados).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().expect("uso: dump ARQUIVO [--map]");
    let source = std::fs::read(path).expect("ler o arquivo");
    let done = match bun_transpile::transpile(path, &source) {
        Ok(done) => done,
        Err(e) => {
            eprintln!("erro: {e}");
            std::process::exit(1);
        }
    };
    if args.iter().any(|a| a == "--map") {
        for s in &done.segments {
            println!(
                "{}:{} -> {}:{}",
                s.generated_line + 1,
                s.generated_column + 1,
                s.original_line + 1,
                s.original_column + 1
            );
        }
    } else {
        print!("{}", String::from_utf8_lossy(&done.text));
        eprintln!("commonjs: {}", done.is_commonjs);
    }
}
