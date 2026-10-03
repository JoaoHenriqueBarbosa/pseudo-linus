//! CLI do jq 1.7.1 sobre o jaq: porte do `main.c` (opções, variáveis, laço de entradas, saída e
//! códigos 0/1/2/3/4/5). Todo I/O passa por [`Host`], então a mesma função roda em processo sobre
//! a MemTree (categoria a) ou como binário multicall sobre `std::fs`.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use jaq_core::{Ctx, Vars};
use jaq_json::{Map, Rc, Val};

use super::engine::{self, Data, EngineOpts, Hook, JqKind, Runtime};
use super::errors::{self, TopError};
use super::input::{InputState, ReadFile};
use super::json::{self, DumpOpts};

pub struct Host {
    pub stdin: Vec<u8>,
    pub read_file: ReadFile,
    pub env: Vec<(String, String)>,
    pub now: Option<f64>,
}

#[derive(Default)]
pub struct RunOpts {
    pub engine: EngineOpts,
    pub interrupt: Option<Arc<AtomicBool>>,
    pub timing: bool,
    pub stats: Option<Arc<engine::HookStats>>,
}

#[derive(Debug, Default)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit: i32,
    /// Chamadas ao checkpoint durante a execução.
    pub checkpoints: u64,
    pub max_gap_ns: u64,
}

const DIE: &str = "Use jq --help for help with command-line options,\nor see the jq manpage, or online docs  at https://jqlang.github.io/jq\n";

// Opções (mesmos bits do main.c).
const SLURP: u32 = 1;
const RAW_INPUT: u32 = 2;
const PROVIDE_NULL: u32 = 4;
const RAW_OUTPUT: u32 = 8;
const RAW_OUTPUT0: u32 = 16;
const ASCII_OUTPUT: u32 = 32;
const SORTED_OUTPUT: u32 = 256;
const FROM_FILE: u32 = 512;
const RAW_NO_LF: u32 = 1024;
const EXIT_STATUS: u32 = 4096;
const SEQ: u32 = 16384;

const JQ_OK: i32 = 0;
const JQ_OK_NULL_KIND: i32 = -1;
const JQ_ERROR_SYSTEM: i32 = 2;
const JQ_ERROR_COMPILE: i32 = 3;
const JQ_OK_NO_OUTPUT: i32 = -4;
const JQ_ERROR_UNKNOWN: i32 = 5;

fn isoptish(t: &str) -> bool {
    let b = t.as_bytes();
    b.first() == Some(&b'-') && b.get(1).is_some_and(|c| *c == b'-' || c.is_ascii_alphabetic())
}

fn isoption(t: &str, short: Option<u8>, long: &str, short_opts: &mut usize) -> bool {
    let b = t.as_bytes();
    if b.first() != Some(&b'-') || b.get(1) == Some(&b'-') {
        *short_opts = 0;
    }
    if b.first() != Some(&b'-') {
        return false;
    }
    if b.get(1) == Some(&b'-') {
        return &t[2..] == long;
    }
    match short {
        Some(c) if b.contains(&c) => {
            *short_opts += 1;
            true
        }
        _ => false,
    }
}

struct Parsed {
    options: u32,
    indent: i32,
    tab: bool,
    stream: bool,
    program: Option<String>,
    files: Vec<String>,
    named: Vec<(String, Val)>,
    positional: Vec<Val>,
}

enum Early {
    Exit(i32),
}

fn parse_args(args: &[String], host: &Host, out: &mut Output) -> Result<Parsed, Early> {
    let mut p = Parsed {
        options: 0,
        indent: 2,
        tab: false,
        stream: false,
        program: None,
        files: Vec::new(),
        named: Vec::new(),
        positional: Vec::new(),
    };
    let mut further_strings = false;
    let mut further_json = false;
    let mut args_done = false;
    let die = |out: &mut Output, msg: &str| {
        out.stderr.extend_from_slice(msg.as_bytes());
        out.stderr.extend_from_slice(DIE.as_bytes());
        Early::Exit(2)
    };
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let mut so = 0usize;
        if args_done || !isoptish(a) {
            if p.program.is_none() {
                p.program = Some(a.to_string());
            } else if further_strings {
                p.positional.push(Val::utf8_str(a.to_string()));
            } else if further_json {
                match json::parse_single(a.as_bytes()) {
                    Ok(v) => p.positional.push(v),
                    Err(_) => return Err(die(out, "jq: invalid JSON text passed to --jsonargs\n")),
                }
            } else {
                p.files.push(a.to_string());
            }
            i += 1;
            continue;
        }
        if a == "--" {
            args_done = true;
            i += 1;
            continue;
        }
        if a.as_bytes().get(1) == Some(&b'L') {
            if a.len() == 2 {
                if i >= args.len() - 1 {
                    return Err(die(out, "-L takes a parameter: (e.g. -L /search/path or -L/search/path)\n"));
                }
                i += 1;
            }
            i += 1;
            continue;
        }
        macro_rules! flag {
            ($short:expr, $long:expr, $body:block) => {
                if isoption(a, $short, $long, &mut so) {
                    $body
                    if so == 0 {
                        i += 1;
                        continue;
                    }
                }
            };
        }
        flag!(Some(b's'), "slurp", { p.options |= SLURP; });
        flag!(Some(b'r'), "raw-output", { p.options |= RAW_OUTPUT; });
        flag!(None, "raw-output0", { p.options |= RAW_OUTPUT | RAW_NO_LF | RAW_OUTPUT0; });
        flag!(Some(b'j'), "join-output", { p.options |= RAW_OUTPUT | RAW_NO_LF; });
        flag!(Some(b'c'), "compact-output", {
            p.indent = 0;
            p.tab = false;
        });
        flag!(Some(b'C'), "color-output", {});
        flag!(Some(b'M'), "monochrome-output", {});
        flag!(Some(b'a'), "ascii-output", { p.options |= ASCII_OUTPUT; });
        if isoption(a, None, "unbuffered", &mut so) {
            i += 1;
            continue;
        }
        flag!(Some(b'S'), "sort-keys", { p.options |= SORTED_OUTPUT; });
        flag!(Some(b'R'), "raw-input", { p.options |= RAW_INPUT; });
        flag!(Some(b'n'), "null-input", { p.options |= PROVIDE_NULL; });
        flag!(Some(b'f'), "from-file", { p.options |= FROM_FILE; });
        if isoption(a, None, "tab", &mut so) {
            p.tab = true;
            i += 1;
            continue;
        }
        if isoption(a, None, "indent", &mut so) {
            if i >= args.len() - 1 {
                return Err(die(out, "jq: --indent takes one parameter\n"));
            }
            let n = atoi(&args[i + 1]);
            if !(-1..=7).contains(&n) {
                return Err(die(out, "jq: --indent takes a number between -1 and 7\n"));
            }
            p.tab = false;
            p.indent = n;
            i += 2;
            continue;
        }
        if isoption(a, None, "seq", &mut so) {
            p.options |= SEQ;
            i += 1;
            continue;
        }
        if isoption(a, None, "stream", &mut so) || isoption(a, None, "stream-errors", &mut so) {
            p.stream = true;
            i += 1;
            continue;
        }
        flag!(Some(b'e'), "exit-status", { p.options |= EXIT_STATUS; });
        if isoption(a, None, "args", &mut so) {
            further_strings = true;
            further_json = false;
            i += 1;
            continue;
        }
        if isoption(a, None, "jsonargs", &mut so) {
            further_strings = false;
            further_json = true;
            i += 1;
            continue;
        }
        if isoption(a, None, "arg", &mut so) {
            if i + 2 >= args.len() {
                return Err(die(out, "jq: --arg takes two parameters (e.g. --arg varname value)\n"));
            }
            let name = args[i + 1].clone();
            if !p.named.iter().any(|(k, _)| *k == name) {
                p.named.push((name, Val::utf8_str(args[i + 2].clone())));
            }
            i += 3;
            continue;
        }
        if isoption(a, None, "argjson", &mut so) {
            if i + 2 >= args.len() {
                return Err(die(out, "jq: --argjson takes two parameters (e.g. --argjson varname text)\n"));
            }
            let name = args[i + 1].clone();
            if !p.named.iter().any(|(k, _)| *k == name) {
                match json::parse_single(args[i + 2].as_bytes()) {
                    Ok(v) => p.named.push((name, v)),
                    Err(_) => return Err(die(out, "jq: invalid JSON text passed to --argjson\n")),
                }
            }
            i += 3;
            continue;
        }
        let raw = isoption(a, None, "rawfile", &mut so);
        if raw || isoption(a, None, "slurpfile", &mut so) {
            let which = if raw { "rawfile" } else { "slurpfile" };
            if i + 2 >= args.len() {
                return Err(die(out, &format!("jq: --{which} takes two parameters (e.g. --{which} varname filename)\n")));
            }
            let name = args[i + 1].clone();
            let file = args[i + 2].clone();
            if !p.named.iter().any(|(k, _)| *k == name) {
                let data = (host.read_file)(&file).map_err(|e| format!("Could not open {file}: {e}"));
                let v = data.and_then(|bytes| {
                    if raw {
                        Ok(Val::utf8_str(String::from_utf8_lossy(&bytes).into_owned()))
                    } else {
                        json::parse_all(&bytes).map(|vs| Val::Arr(Rc::new(vs)))
                    }
                });
                match v {
                    Ok(v) => p.named.push((name, v)),
                    Err(msg) => {
                        out.stderr
                            .extend_from_slice(format!("jq: Bad JSON in --{which} {name} {file}: {msg}\n").as_bytes());
                        return Err(Early::Exit(JQ_ERROR_SYSTEM));
                    }
                }
            }
            i += 3;
            continue;
        }
        if isoption(a, None, "debug-dump-disasm", &mut so) || isoption(a, None, "debug-trace", &mut so) {
            i += 1;
            continue;
        }
        if isoption(a, Some(b'h'), "help", &mut so) {
            out.stdout.extend_from_slice(b"Usage:\tjq [OPTIONS] FILTER [FILES...]\n");
            return Err(Early::Exit(0));
        }
        if isoption(a, Some(b'V'), "version", &mut so) {
            // O jq 1.7.1-6 do Debian 13 se identifica como "jq-1.7".
            out.stdout.extend_from_slice(b"jq-1.7\n");
            return Err(Early::Exit(0));
        }
        if a.len() != so + 1 {
            return Err(die(out, &format!("jq: Unknown option {a}\n")));
        }
        i += 1;
    }
    Ok(p)
}

fn atoi(s: &str) -> i32 {
    let t = s.trim_start();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let n: i64 = digits.bytes().take_while(u8::is_ascii_digit).fold(0i64, |acc, d| acc.saturating_mul(10) + (d - b'0') as i64);
    let n = if neg { -n } else { n };
    n.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// Roda o jq com os argumentos (sem o argv[0]).
pub fn run(args: &[String], host: Host, opts: RunOpts) -> Output {
    let mut out = Output::default();
    let parsed = match parse_args(args, &host, &mut out) {
        Ok(p) => p,
        Err(Early::Exit(code)) => {
            out.exit = code;
            return out;
        }
    };
    let options = parsed.options;
    let dump_opts = DumpOpts {
        indent: if parsed.tab { 1 } else { parsed.indent.max(0) as usize },
        tab: parsed.tab,
        sort_keys: options & SORTED_OUTPUT != 0,
        ascii: options & ASCII_OUTPUT != 0,
    };

    let program = match &parsed.program {
        None => ".".to_string(),
        Some(p) if options & FROM_FILE != 0 => match (host.read_file)(p) {
            Ok(bytes) => skip_shebang(&String::from_utf8_lossy(&bytes)).to_string(),
            Err(e) => {
                out.stderr.extend_from_slice(format!("jq: Could not open {p}: {e}\n").as_bytes());
                out.exit = JQ_ERROR_SYSTEM;
                return out;
            }
        },
        Some(p) => p.clone(),
    };

    // Variáveis globais: argumentos nomeados, $ARGS e $ENV.
    let mut named_map = Map::default();
    for (k, v) in &parsed.named {
        named_map.insert(Val::utf8_str(k.clone()), v.clone());
    }
    let mut args_obj = Map::default();
    args_obj.insert(Val::utf8_str("positional".to_string()), Val::Arr(Rc::new(parsed.positional.clone())));
    args_obj.insert(Val::utf8_str("named".to_string()), Val::obj(named_map));
    let mut env_map = Map::default();
    for (k, v) in &host.env {
        env_map.insert(Val::utf8_str(k.clone()), Val::utf8_str(v.clone()));
    }
    let env_obj = Val::obj(env_map);
    let mut global_names: Vec<String> = parsed.named.iter().map(|(k, _)| k.clone()).collect();
    let mut global_vals: Vec<Val> = parsed.named.iter().map(|(_, v)| v.clone()).collect();
    global_names.push("ARGS".into());
    global_vals.push(Val::obj(args_obj));
    global_names.push("ENV".into());
    global_vals.push(env_obj.clone());

    let filter = match engine::compile(&program, &global_names, opts.engine) {
        Ok(f) => f,
        Err(e) => {
            for m in &e.messages {
                out.stderr.extend_from_slice(format!("jq: error: {m}\n").as_bytes());
            }
            let n = e.messages.len().max(1);
            let s = if n == 1 { "" } else { "s" };
            out.stderr.extend_from_slice(format!("jq: {n} compile error{s}\n").as_bytes());
            out.exit = JQ_ERROR_COMPILE;
            return out;
        }
    };

    let stdin = Rc::new(host.stdin);
    let read_file = host.read_file;
    let read: ReadFile = Box::new(move |f| read_file(f));
    let input = InputState::new(
        parsed.files.clone(),
        stdin,
        read,
        options & RAW_INPUT != 0,
        options & SLURP != 0,
        options & SEQ != 0,
        parsed.stream,
    );
    let rt = Runtime {
        stderr: std::cell::RefCell::new(Vec::new()),
        env_obj,
        now: host.now,
        input: std::cell::RefCell::new(input),
        hook: Hook {
            interrupt: opts.interrupt.clone(),
            timing: opts.timing,
            shared: opts.stats.clone(),
            ..Hook::default()
        },
    };
    let data = Data { lut: &filter.lut, rt: &rt };

    let mut ret = JQ_OK_NO_OUTPUT;
    let mut last_result: i32 = -1;

    let process = |value: Val, out: &mut Output| -> (i32, bool) {
        let ctx = Ctx::<JqKind>::new(&data, Vars::new(global_vals.clone()));
        let mut ret = JQ_OK_NO_OUTPUT;
        let mut halted = false;
        for item in filter.id.run((ctx, value)) {
            match item {
                Ok(v) => {
                    let is_str = matches!(v, Val::TStr(_) | Val::BStr(_));
                    if options & RAW_OUTPUT != 0 && is_str {
                        let bytes: Vec<u8> = match &v {
                            Val::TStr(b) | Val::BStr(b) => b.to_vec(),
                            _ => unreachable!(),
                        };
                        if options & ASCII_OUTPUT != 0 {
                            out.stdout.extend_from_slice(json::dump(&v, &DumpOpts { ascii: true, ..DumpOpts::COMPACT }).as_bytes());
                        } else if options & RAW_OUTPUT0 != 0 && bytes.contains(&0) {
                            flush_stderr(&rt, out);
                            out.stderr.extend_from_slice(
                                format!(
                                    "jq: error (at {}): Cannot dump a string containing NUL with --raw-output0 option\n",
                                    rt.input.borrow().position()
                                )
                                .as_bytes(),
                            );
                            return (JQ_ERROR_UNKNOWN, false);
                        } else {
                            out.stdout.extend_from_slice(&bytes);
                        }
                        ret = JQ_OK;
                    } else {
                        ret = if matches!(v, Val::Null | Val::Bool(false)) { JQ_OK_NULL_KIND } else { JQ_OK };
                        if options & SEQ != 0 {
                            out.stdout.push(0x1e);
                        }
                        out.stdout.extend_from_slice(json::dump(&v, &dump_opts).as_bytes());
                    }
                    if options & RAW_NO_LF == 0 {
                        out.stdout.push(b'\n');
                    }
                    if options & RAW_OUTPUT0 != 0 {
                        out.stdout.push(0);
                    }
                }
                Err(exn) => {
                    flush_stderr(&rt, out);
                    match exn.get_err() {
                        Ok(e) => {
                            let pos = rt.input.borrow().position();
                            match errors::translate(e) {
                                TopError::Msg(m) => {
                                    out.stderr.extend_from_slice(format!("jq: error (at {pos}): {m}\n").as_bytes())
                                }
                                TopError::NotString(v) => out.stderr.extend_from_slice(
                                    format!("jq: error (at {pos}) (not a string): {}\n", json::dump(&v, &DumpOpts::COMPACT))
                                        .as_bytes(),
                                ),
                            }
                            ret = JQ_ERROR_UNKNOWN;
                        }
                        Err(exn) => match exn.get_halt() {
                            Ok(code) => {
                                ret = code;
                                halted = true;
                            }
                            Err(_) => {
                                out.stderr.extend_from_slice("jq: error: exceção interna do jaq escapou\n".as_bytes());
                                ret = JQ_ERROR_UNKNOWN;
                            }
                        },
                    }
                    break;
                }
            }
        }
        flush_stderr(&rt, out);
        (ret, halted)
    };

    if options & PROVIDE_NULL != 0 {
        let (r, _h) = process(Val::Null, &mut out);
        ret = r;
    } else {
        loop {
            if rt.input.borrow().failures != 0 {
                break;
            }
            let next = rt.input.borrow_mut().next_value();
            {
                let mut i = rt.input.borrow_mut();
                out.stderr.extend_from_slice(&i.errors);
                i.errors.clear();
            }
            match next {
                None => break,
                Some(Ok(v)) => {
                    let (r, h) = process(v, &mut out);
                    ret = r;
                    if ret <= 0 && ret != JQ_OK_NO_OUTPUT {
                        last_result = (ret != JQ_OK_NULL_KIND) as i32;
                    }
                    if h {
                        break;
                    }
                }
                Some(Err(msg)) => {
                    if options & SEQ == 0 {
                        ret = JQ_ERROR_UNKNOWN;
                        out.stderr.extend_from_slice(format!("jq: parse error: {msg}\n").as_bytes());
                        break;
                    }
                    out.stderr.extend_from_slice(format!("jq: ignoring parse error: {msg}\n").as_bytes());
                }
            }
        }
    }
    if rt.input.borrow().failures != 0 {
        ret = JQ_ERROR_SYSTEM;
    }
    out.checkpoints = rt.hook.calls.get();
    out.max_gap_ns = rt.hook.max_gap_ns.get();
    out.exit = if options & EXIT_STATUS != 0 {
        if ret != JQ_OK_NO_OUTPUT {
            ret.abs()
        } else {
            match last_result {
                -1 => JQ_OK_NO_OUTPUT.abs(),
                0 => JQ_OK_NULL_KIND.abs(),
                _ => JQ_OK,
            }
        }
    } else if ret > 0 {
        ret
    } else {
        0
    };
    out
}

/// Despeja no stderr da saída o que as nativas (`stderr`, `debug`) e o leitor de entradas acumularam.
fn flush_stderr(rt: &Runtime, out: &mut Output) {
    let mut s = rt.stderr.borrow_mut();
    out.stderr.extend_from_slice(&s);
    s.clear();
    let mut i = rt.input.borrow_mut();
    out.stderr.extend_from_slice(&i.errors);
    i.errors.clear();
}

/// `skip_shebang` do main.c.
fn skip_shebang(p: &str) -> &str {
    if !p.starts_with("#!") {
        return p;
    }
    let Some(n) = p.find('\n') else { return p };
    if p.as_bytes().get(n + 1) != Some(&b'#') {
        return p;
    }
    let Some(n2) = p[n + 1..].find('\n').map(|i| n + 1 + i) else { return p };
    let b = p.as_bytes();
    if matches!(b.get(n2 + 1), Some(b'#') | None) || b[n2 - 1] != b'\\' || b[n2 - 2] == b'\\' {
        return p;
    }
    match p[n2 + 1..].find('\n') {
        Some(n3) => &p[n2 + 1 + n3 + 1..],
        None => p,
    }
}

/// Entrada do binário multicall (`jq` apontando pro executável do experimento): I/O do host.
pub fn main_std(args: &[String]) -> std::process::ExitCode {
    use std::io::{Read, Write};
    let mut stdin = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut stdin);
    let host = Host {
        stdin,
        read_file: Box::new(|path| {
            let meta = std::fs::metadata(path).map_err(|e| super::input::strerror(&e))?;
            if meta.is_dir() {
                return Err("Is a directory".into());
            }
            std::fs::read(path).map_err(|e| super::input::strerror(&e))
        }),
        env: std::env::vars().collect(),
        now: None,
    };
    let out = run(args, host, RunOpts::default());
    let _ = std::io::stdout().write_all(&out.stdout);
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().write_all(&out.stderr);
    std::process::ExitCode::from(out.exit.clamp(0, 255) as u8)
}
