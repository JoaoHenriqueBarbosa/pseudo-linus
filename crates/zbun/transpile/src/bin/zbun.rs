//! A CLI do zbun: `zbun build --no-bundle [--minify-syntax] [--outfile ARQ] ARQUIVO`, o `bun build --no-bundle` do bun
//! 1.4.2. O texto sai no stdout (ou em `--outfile`), os erros do parser no stderr com saída 1; arquivo ausente é
//! `Error resolving "<caminho absoluto>": ModuleNotFound` no stderr com saída 0, como o bun.
use std::io::Write;
use std::path::Path;

fn exit(code: i32) -> ! {
    bun_core::run_exit_callbacks();
    std::process::exit(code)
}

fn build(args: &[String]) -> i32 {
    let mut file = None;
    let mut outfile = None;
    let mut minify_syntax = false;
    let mut target_bun = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--minify-syntax" => minify_syntax = true,
            "--target=bun" => target_bun = true,
            "--outfile" => outfile = iter.next().cloned(),
            other if other.starts_with("--outfile=") => outfile = Some(other["--outfile=".len()..].to_owned()),
            other if other.starts_with("--") => {}
            other => file = Some(other.to_owned()),
        }
    }
    let Some(file) = file else {
        eprintln!("error: Missing entrypoints. What would you like to bundle?");
        return 1;
    };
    let absolute = std::path::absolute(&file).unwrap_or_else(|_| Path::new(&file).to_path_buf());
    let Ok(source) = std::fs::read(&absolute) else {
        eprintln!("Error resolving \"{}\": ModuleNotFound", absolute.display());
        return 0;
    };
    let config = bun_transpile::Config { like_build: true, no_minify_syntax: !minify_syntax, no_inlining: !target_bun, no_tree_shaking: !target_bun, ..Default::default() };
    match bun_transpile::transpile_with(&absolute.to_string_lossy(), &source, config) {
        Ok(done) => {
            match outfile {
                Some(path) => std::fs::write(path, &done.text).expect("gravar o outfile"),
                None => std::io::stdout().write_all(&done.text).expect("gravar o stdout"),
            }
            0
        }
        Err(error) => {
            std::io::stderr().write_all(&error.report).expect("gravar o stderr");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("build") => build(&args[1..]),
        _ => {
            eprintln!("usage: zbun build --no-bundle [--minify-syntax] [--outfile FILE] FILE");
            1
        }
    };
    exit(code);
}
