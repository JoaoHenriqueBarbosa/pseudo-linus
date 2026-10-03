//! Lado de dentro do `--exec`: chama a ferramenta candidata como o binário dela faria, com o
//! `argv` do caso. Roda num processo próprio, com `cwd` na cópia da fixture (ver [`crate::exec`]).

use std::ffi::OsString;
use std::io::Write;

fn os_args(prog: &str, args: &[String]) -> Vec<OsString> {
    std::iter::once(prog.to_string()).chain(args.iter().cloned()).map(OsString::from).collect()
}

/// uutils/sed: o `main` do binário dele chama `sed::sed::uumain`.
pub fn uutils_sed(args: &[String]) -> i32 {
    uucore::panic::mute_sigpipe_panic();
    let code = sed::sed::uumain(os_args("sed", args).into_iter());
    let _ = std::io::stdout().flush();
    code
}

/// uutils/grep: o binário dele é `uucore::bin!(uu_grep)`.
pub fn uu_grep(args: &[String]) -> i32 {
    uucore::panic::mute_sigpipe_panic();
    if let Err(e) = uucore::locale::setup_localization("grep") {
        eprintln!("localização: {e}");
    }
    let code = uu_grep::uumain(os_args("grep", args).into_iter());
    let _ = std::io::stdout().flush();
    code
}

/// sed-rs: o mesmo `try_main` do `src/main.rs` dele.
pub fn sed_rs(args: &[String]) -> i32 {
    use clap::Parser;
    use sed_rs::{cli, command, engine};
    let run = || -> sed_rs::Result<()> {
        let argv = cli::preprocess_args(std::iter::once("sed".to_string()).chain(args.iter().cloned()));
        let options = cli::Options::parse_from(argv);
        let (script, files) = options.script_and_files()?;
        if script.is_empty() {
            return Err(sed_rs::Error::Parse("empty script".into()));
        }
        let commands = command::parse(&script)?;
        let engine = engine::Engine::new(commands, &options)?;
        if let Some(ref suffix) = options.in_place {
            if files.is_empty() {
                return Err(sed_rs::Error::Parse("-i/--in-place requires at least one file argument".into()));
            }
            engine.run_in_place(&files, suffix)?;
        } else {
            engine.run(&files)?;
        }
        Ok(())
    };
    let code = match run() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("sed: {e}");
            2
        }
    };
    let _ = std::io::stdout().flush();
    code
}

/// red: o `cli.rs` do red é do binário (não está na lib); aqui é o mesmo parser (lexopt), portado
/// pra receber o argv do caso, montando o `red::RunConfig` público.
pub fn red(args: &[String]) -> i32 {
    use lexopt::prelude::*;
    use red::errors::ScriptSource;
    red::mbcs::initialize();
    let mut scripts: Vec<(String, Vec<u8>, ScriptSource)> = Vec::new();
    let mut files: Vec<String> = Vec::new();
    let mut quiet = false;
    let mut in_place: Option<String> = None;
    let mut extended = false;
    let mut separate = false;
    let mut line_length = red::constants::DEFAULT_LINE_LENGTH;
    let mut unbuffered = false;
    let mut posix = false;
    let mut follow_symlinks = false;
    let mut sandbox = false;
    let mut null_data = false;
    let mut binary = false;
    let mut expr_index = 0;
    let mut parser = lexopt::Parser::from_args(args.iter().cloned());
    let parsed: Result<(), String> = (|| {
        while let Some(arg) = parser.next().map_err(|e| e.to_string())? {
            match arg {
                Short('e') | Long("expression") => {
                    let v = parser.value().map_err(|e| e.to_string())?.to_string_lossy().into_owned();
                    scripts.push((v.clone(), v.into_bytes(), ScriptSource::Expression(expr_index)));
                    expr_index += 1;
                }
                Short('f') | Long("file") => {
                    let path = parser.value().map_err(|e| e.to_string())?.to_string_lossy().into_owned();
                    let raw = if path == "-" {
                        let mut b = Vec::new();
                        std::io::Read::read_to_end(&mut std::io::stdin(), &mut b).map_err(|e| e.to_string())?;
                        b
                    } else {
                        std::fs::read(&path).map_err(|e| format!("couldn't open file {path}: {e}"))?
                    };
                    let mut text = String::from_utf8_lossy(&raw).into_owned();
                    if text.ends_with('\n') {
                        text.pop();
                    }
                    scripts.push((text, raw, ScriptSource::File(path)));
                }
                Short('n') | Long("quiet") | Long("silent") => quiet = true,
                Short('i') | Long("in-place") => {
                    let suffix = match parser.optional_value() {
                        Some(v) => {
                            let s = v.to_string_lossy().into_owned();
                            if s.starts_with('=') { format!("={s}") } else { s }
                        }
                        None => String::new(),
                    };
                    in_place = Some(suffix);
                }
                Short('r') | Short('E') | Long("regexp-extended") => extended = true,
                Short('s') | Long("separate") => separate = true,
                Short('l') | Long("line-length") => {
                    let v = parser.value().map_err(|e| e.to_string())?.to_string_lossy().into_owned();
                    line_length = v.parse().map_err(|_| format!("invalid line length: {v}"))?;
                }
                Short('u') | Long("unbuffered") => unbuffered = true,
                Long("posix") => posix = true,
                Long("follow-symlinks") => follow_symlinks = true,
                Long("sandbox") => sandbox = true,
                Short('z') | Long("null-data") => null_data = true,
                Short('b') | Long("binary") => binary = true,
                Long("debug") => {}
                Value(v) => files.push(v.to_string_lossy().into_owned()),
                Short(c) => return Err(format!("invalid option -- '{c}'")),
                Long(o) => return Err(format!("invalid option -- --{o}")),
            }
        }
        Ok(())
    })();
    if let Err(msg) = parsed {
        eprintln!("sed: {msg}");
        return 1;
    }
    if scripts.is_empty() {
        if files.is_empty() {
            eprintln!("Usage: sed [OPTION]... {{script-only-if-no-other-script}} [input-file]...");
            return 4;
        }
        let s = files.remove(0);
        scripts.push((s.clone(), s.into_bytes(), ScriptSource::Expression(0)));
    }
    let cfg = red::RunConfig {
        scripts_with_sources: scripts,
        input_files: files,
        quiet,
        in_place,
        extended_regex: extended,
        separate_files: separate,
        line_length,
        unbuffered,
        posix,
        strict_posix: posix,
        follow_symlinks,
        sandbox,
        null_data,
        binary,
    };
    let code = match red::run(cfg) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("sed: {e}");
            e.exit_code()
        }
    };
    let _ = std::io::stdout().flush();
    code
}

/// Despacha o `--exec IMPL ARGS...`.
pub fn dispatch(implementation: &str, args: &[String]) -> i32 {
    match implementation {
        "uutils-sed" => uutils_sed(args),
        "uu-grep" => uu_grep(args),
        "sed-rs" => sed_rs(args),
        "red" => red(args),
        other => {
            eprintln!("implementação desconhecida: {other}");
            125
        }
    }
}
