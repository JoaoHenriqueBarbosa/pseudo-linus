//! `pzstd` 1.5.7 do Debian 13 (`contrib/pzstd`): zstd paralelo.
//!
//! O paralelismo não muda o resultado, então o programa só interpreta as opções do `pzstd`
//! (`Options.cpp`) e delega o trabalho ao porte do `zstd` ([`super::zstd`]), que dá a mesma saída pros
//! mesmos níveis. O `-p` (número de threads) é validado e descartado, `-v` e `-q` só mexem no nível de
//! log, e `-h`/`-V` imprimem os textos do `pzstd` no stderr.

use super::zstd;

const DEFAULT_LEVEL: i32 = 3;
const MAX_LEVEL: i32 = 19;
const DEFAULT_LOG_LEVEL: i32 = 2;

fn eprint(text: &str) {
    let _ = sysabi::sys::write_all(sysabi::Fd::STDERR, text.as_bytes());
}

fn help() -> String {
    let mut s = String::new();
    s.push_str("Usage:\n");
    s.push_str("  pzstd [args] [FILE(s)]\n");
    s.push_str("Parallel ZSTD options:\n");
    s.push_str("  -p, --processes   #    : number of threads to use for (de)compression (default:<numcores>)\n");
    s.push_str("ZSTD options:\n");
    s.push_str(&format!("  -#                     : # compression level (1-{MAX_LEVEL}, default:{DEFAULT_LEVEL})\n"));
    s.push_str("  -d, --decompress       : decompression\n");
    s.push_str("  -o                     : result stored into `file` (only if 1 input file)\n");
    s.push_str("  -f, --force            : overwrite output without prompting, (de)compress links\n");
    s.push_str("      --rm               : remove source file(s) after successful (de)compression\n");
    s.push_str("  -k, --keep             : preserve source file(s) (default)\n");
    s.push_str("  -h, --help             : display help and exit\n");
    s.push_str("  -V, --version          : display version number and exit\n");
    s.push_str(&format!(
        "  -v, --verbose          : verbose mode; specify multiple times to increase log level (default:{DEFAULT_LOG_LEVEL})\n"
    ));
    s.push_str("  -q, --quiet            : suppress warnings; specify twice to suppress errors too\n");
    s.push_str("  -c, --stdout           : write to standard output (even if it is the console)\n");
    s.push_str("  -r                     : operate recursively on directories\n");
    s.push_str(&format!(
        "      --ultra            : enable levels beyond {MAX_LEVEL}, up to 22 (requires more memory)\n"
    ));
    s.push_str("  -C, --check            : integrity check (default)\n");
    s.push_str("      --no-check         : no integrity check\n");
    s.push_str("  -t, --test             : test compressed file integrity\n");
    s.push_str("  --                     : all arguments after \"--\" are treated as files\n");
    s
}

fn invalid(arg: &[u8]) -> i32 {
    eprint(&format!("Invalid argument: {}\n", String::from_utf8_lossy(arg)));
    eprint(&help());
    1
}

/// `-p` e `--processes`: precisa ser um inteiro positivo.
fn check_threads(value: &[u8]) -> bool {
    std::str::from_utf8(value).ok().and_then(|s| s.parse::<u32>().ok()).is_some_and(|n| n > 0)
}

pub fn main(argv: &[Vec<u8>]) -> i32 {
    let mut out: Vec<Vec<u8>> = vec![b"zstd".to_vec()];
    let mut files: Vec<Vec<u8>> = Vec::new();
    let mut only_files = false;
    let mut i = 1;
    while i < argv.len() {
        let arg = &argv[i];
        i += 1;
        if only_files || arg == b"-" || !arg.starts_with(b"-") {
            files.push(arg.clone());
            continue;
        }
        if arg == b"--" {
            only_files = true;
            continue;
        }
        if let Some(long) = arg.strip_prefix(b"--") {
            let (name, value) = match long.iter().position(|b| *b == b'=') {
                Some(p) => (&long[..p], Some(long[p + 1..].to_vec())),
                None => (long, None),
            };
            match name {
                b"processes" => {
                    let v = match value {
                        Some(v) => v,
                        None => {
                            let Some(v) = argv.get(i).cloned() else { return invalid(arg) };
                            i += 1;
                            v
                        }
                    };
                    if !check_threads(&v) {
                        return invalid(arg);
                    }
                }
                b"decompress" => out.push(b"-d".to_vec()),
                b"force" => out.push(b"-f".to_vec()),
                b"rm" => out.push(b"--rm".to_vec()),
                b"keep" => out.push(b"-k".to_vec()),
                b"stdout" => out.push(b"-c".to_vec()),
                b"test" => out.push(b"-t".to_vec()),
                b"ultra" => out.push(b"--ultra".to_vec()),
                b"check" => {}
                b"no-check" => out.push(b"--no-check".to_vec()),
                b"verbose" => {}
                b"quiet" => out.push(b"-q".to_vec()),
                b"help" => {
                    eprint(&help());
                    return 0;
                }
                b"version" => {
                    eprint("PZSTD version: 1.5.7.\n");
                    return 0;
                }
                _ => return invalid(arg),
            }
            continue;
        }
        // Opções curtas agrupadas.
        let shorts = &arg[1..];
        let mut j = 0;
        while j < shorts.len() {
            let c = shorts[j];
            j += 1;
            match c {
                b'0'..=b'9' => {
                    let mut level = vec![c];
                    while j < shorts.len() && shorts[j].is_ascii_digit() {
                        level.push(shorts[j]);
                        j += 1;
                    }
                    let mut a = b"-".to_vec();
                    a.extend(level);
                    out.push(a);
                }
                b'd' => out.push(b"-d".to_vec()),
                b'f' => out.push(b"-f".to_vec()),
                b'k' => out.push(b"-k".to_vec()),
                b'c' => out.push(b"-c".to_vec()),
                b't' => out.push(b"-t".to_vec()),
                b'r' => out.push(b"-r".to_vec()),
                b'C' | b'v' => {}
                b'q' => out.push(b"-q".to_vec()),
                b'h' => {
                    eprint(&help());
                    return 0;
                }
                b'V' => {
                    eprint("PZSTD version: 1.5.7.\n");
                    return 0;
                }
                b'p' | b'o' => {
                    // O valor é o resto do grupo ou o próximo argumento.
                    let value = if j < shorts.len() {
                        let v = shorts[j..].to_vec();
                        j = shorts.len();
                        v
                    } else {
                        let Some(v) = argv.get(i).cloned() else { return invalid(arg) };
                        i += 1;
                        v
                    };
                    if c == b'p' {
                        if !check_threads(&value) {
                            return invalid(arg);
                        }
                    } else {
                        out.push(b"-o".to_vec());
                        out.push(value);
                    }
                }
                _ => return invalid(arg),
            }
        }
    }
    if !files.is_empty() {
        out.push(b"--".to_vec());
        out.extend(files);
    }
    zstd::main(&out)
}
