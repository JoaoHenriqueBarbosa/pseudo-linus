// O fluxo segue o yq 3.4.3 (yq/__init__.py e yq/parser.py), Copyright Andrey Kislyuk, licença
// Apache 2.0. Reescrito no pseudo-linus (2026, MIT) em Rust seguro.

//! `yq`: YAML em JSON pro `jq` (subprocesso, como no original) e, com `-y`/`-Y`, a saída do jq de
//! volta em YAML.
//!
//! Argumentos como o `argparse` do Python com `parse_known_args`: as opções do yq são consumidas,
//! as que o jq conhece e levam valor (`--arg`, `--indent`...) são reanexadas no fim, o resto vai pro
//! jq na ordem; o primeiro bloco de posicionais dá o filtro e os arquivos (posicionais depois de
//! uma opção desconhecida também vão pro jq, a esquisitice do argparse).

use std::ffi::OsString;
use std::io::{Read, Write};

use sysabi::{Ctx, Fd, sys};
use sysio::process::{Command, Stdio};

use crate::dump::{DumpOptions, dump_all};
use crate::load::{Constructor, Loader};
use crate::py::{json_error_text, raw_decode, to_json};
use crate::scanner::YamlError;

const USAGE: &str = "usage: yq [-h] [--yaml-output] [--yaml-roundtrip]
          [--yaml-output-grammar-version {1.1,1.2}] [--width WIDTH]
          [--indentless-lists] [--explicit-start] [--explicit-end]
          [--in-place] [--version]
          [jq_filter] [files ...]
";

const HELP: &str = "usage: yq [options] <jq filter> [input file...]
          [--indentless-lists] [--explicit-start] [--explicit-end]
          [--in-place] [--version]
          [jq_filter] [files ...]

yq: Command-line YAML processor - jq wrapper for YAML documents

yq transcodes YAML documents to JSON and passes them to jq.
See https://github.com/kislyuk/yq for more information.

positional arguments:
  jq_filter
  files

options:
  -h, --help            show this help message and exit
  --yaml-output, --yml-output, -y
                        Transcode jq JSON output back into YAML and emit it
  --yaml-roundtrip, --yml-roundtrip, -Y
                        Transcode jq JSON output back into YAML and emit it. Preserve YAML tags and styles by representing them as extra items in their enclosing mappings and sequences while in JSON. This option is incompatible with jq filters that do not expect these extra items.
  --yaml-output-grammar-version, --yml-out-ver {1.1,1.2}
                        When using --yaml-output, specify output grammar (the default is 1.1 and will be changed to 1.2 in a future version). Setting this to 1.2 will cause strings like 'on' and 'no' to be emitted unquoted.
  --width, -w WIDTH     When using --yaml-output, specify string wrap width
  --indentless-lists, --indentless
                        When using --yaml-output, indent block style lists (sequences) with 0 spaces instead of 2
  --explicit-start      When using --yaml-output, always emit explicit document start (\"---\")
  --explicit-end        When using --yaml-output, always emit explicit document end (\"...\")
  --in-place, -i        Edit files in place (no backup - use caution)
  --version             show program's version number and exit

";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Format {
    Json,
    Yaml,
    Annotated,
    Other,
}

/// Uma entrada já aberta (o `argparse.FileType` abre na hora da análise dos argumentos).
struct Input {
    name: String,
    data: Vec<u8>,
}

struct Args {
    format: Format,
    grammar12: bool,
    width: Option<i64>,
    indentless: bool,
    explicit_start: bool,
    explicit_end: bool,
    expand_aliases: bool,
    max_expansion: i64,
    in_place: bool,
    filter: Option<String>,
    files: Vec<String>,
    jq_args: Vec<String>,
    spec: Vec<(String, Vec<String>)>,
}

enum Exit {
    Code(i32),
}

fn usage_error(msg: &str) -> Exit {
    sysio::io::eprint_fmt(format_args!("{USAGE}yq: error: {msg}\n"));
    Exit::Code(2)
}

fn take(args: &[String], i: &mut usize, n: usize, opt: &str) -> Result<Vec<String>, Exit> {
    let mut out = Vec::new();
    while out.len() < n {
        match args.get(*i) {
            Some(a) if !(a.starts_with('-') && a.len() > 1 && !looks_negative(a)) => {
                out.push(a.clone());
                *i += 1;
            }
            _ => {
                let what = if n == 1 { "expected one argument".to_string() } else { format!("expected {n} arguments") };
                return Err(usage_error(&format!("argument {opt}: {what}")));
            }
        }
    }
    Ok(out)
}

fn looks_negative(a: &str) -> bool {
    a[1..].parse::<f64>().is_ok()
}

fn parse_args(argv: &[String]) -> Result<Args, Exit> {
    let mut a = Args {
        format: Format::Json,
        grammar12: false,
        width: None,
        indentless: false,
        explicit_start: false,
        explicit_end: false,
        expand_aliases: true,
        max_expansion: 1024,
        in_place: false,
        filter: None,
        files: Vec::new(),
        jq_args: Vec::new(),
        spec: Vec::new(),
    };
    let jq_spec: &[(&str, usize)] = &[
        ("--indent", 1),
        ("-f", 1),
        ("--from-file", 1),
        ("-L", 1),
        ("--arg", 2),
        ("--argjson", 2),
        ("--slurpfile", 2),
        ("--argfile", 2),
        ("--rawfile", 2),
    ];
    let mut positionals_done = false;
    let mut in_positionals = false;
    let mut i = 0;
    let mut only_positionals = false;
    while i < argv.len() {
        let arg = argv[i].clone();
        i += 1;
        let is_opt = !only_positionals && arg.starts_with('-') && arg.len() > 1 && !looks_negative(&arg);
        if !only_positionals && arg == "--" {
            only_positionals = true;
            continue;
        }
        if !is_opt {
            if positionals_done && !in_positionals {
                a.jq_args.push(arg);
                continue;
            }
            in_positionals = true;
            if a.filter.is_none() {
                a.filter = Some(arg);
            } else {
                a.files.push(arg);
            }
            continue;
        }
        if in_positionals {
            in_positionals = false;
            positionals_done = true;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let value = |i: &mut usize, opt: &str| -> Result<String, Exit> {
            match &inline {
                Some(v) => Ok(v.clone()),
                None => take(argv, i, 1, opt).map(|mut v| v.remove(0)),
            }
        };
        match name.as_str() {
            "-h" | "--help" => {
                print_help();
                return Err(Exit::Code(0));
            }
            "--version" => {
                let _ = sysio::io::stdout().write_all(b"yq 3.4.3\n");
                return Err(Exit::Code(0));
            }
            "--yaml-output" | "--yml-output" | "-y" => a.format = Format::Yaml,
            "--yaml-roundtrip" | "--yml-roundtrip" | "-Y" => a.format = Format::Annotated,
            "--xml-output" | "-x" | "--toml-output" | "-t" => a.format = Format::Other,
            "--output-format" => {
                let v = value(&mut i, "--output-format")?;
                a.format = match v.as_str() {
                    "json" => Format::Json,
                    "yaml" => Format::Yaml,
                    "annotated_yaml" => Format::Annotated,
                    _ => Format::Other,
                };
            }
            "--yaml-output-grammar-version" | "--yml-out-ver" => {
                let v = value(&mut i, "--yaml-output-grammar-version/--yml-out-ver")?;
                match v.as_str() {
                    "1.1" => a.grammar12 = false,
                    "1.2" => a.grammar12 = true,
                    _ => {
                        return Err(usage_error(&format!(
                            "argument --yaml-output-grammar-version/--yml-out-ver: invalid choice: '{v}' (choose from 1.1, 1.2)"
                        )));
                    }
                }
            }
            "--width" | "-w" => {
                let v = value(&mut i, "--width/-w")?;
                match v.trim().parse::<i64>() {
                    Ok(n) => a.width = Some(n),
                    Err(_) => return Err(usage_error(&format!("argument --width/-w: invalid int value: '{v}'"))),
                }
            }
            "--max-expansion-factor" => {
                let v = value(&mut i, "--max-expansion-factor")?;
                match v.trim().parse::<i64>() {
                    Ok(n) => a.max_expansion = n,
                    Err(_) => return Err(usage_error(&format!("argument --max-expansion-factor: invalid int value: '{v}'"))),
                }
            }
            "--xml-item-depth" | "--xml-root" | "--xml-force-list" => {
                value(&mut i, &name)?;
            }
            "--indentless-lists" | "--indentless" => a.indentless = true,
            "--explicit-start" => a.explicit_start = true,
            "--explicit-end" => a.explicit_end = true,
            "--no-expand-aliases" => a.expand_aliases = false,
            "--xml-dtd" => {}
            "--in-place" | "-i" => a.in_place = true,
            "--args" | "--jsonargs" => {
                let rest: Vec<String> = argv[i..].to_vec();
                i = argv.len();
                a.spec.push((name.clone(), rest));
            }
            _ => {
                if let Some(&(opt, n)) = jq_spec.iter().find(|(o, _)| *o == name) {
                    let vals = match &inline {
                        Some(v) if n == 1 => vec![v.clone()],
                        _ => take(argv, &mut i, n, opt)?,
                    };
                    a.spec.push((opt.to_string(), vals));
                } else {
                    a.jq_args.push(arg);
                }
            }
        }
    }
    Ok(a)
}

fn print_help() {
    let mut out = sysio::io::stdout();
    let _ = out.write_all(HELP.as_bytes());
    let _ = sysio::io::flush_stdout();
    let _ = Command::new("jq").arg("--help").status();
}

fn open_input(name: &str) -> Result<Input, Exit> {
    if name == "-" {
        let mut data = Vec::new();
        let _ = sysio::io::stdin().read_to_end(&mut data);
        return Ok(Input { name: "<stdin>".to_string(), data });
    }
    match sysio::fs::read(name) {
        Ok(data) => Ok(Input { name: name.to_string(), data }),
        Err(e) => {
            let n = e.raw_os_error().unwrap_or(2);
            let msg = sysio::errno::strerror(&e);
            Err(usage_error(&format!("argument files: can't open '{name}': [Errno {n}] {msg}: '{name}'")))
        }
    }
}

/// O texto como o `open()` do Python o lê: UTF-8 com quebras universais.
fn decode_text(data: &[u8]) -> Result<String, YamlError> {
    match std::str::from_utf8(data) {
        Ok(s) => Ok(s.replace("\r\n", "\n").replace('\r', "\n")),
        Err(e) => {
            let pos = e.valid_up_to();
            let byte = data[pos];
            let reason = match e.error_len() {
                None => "unexpected end of data",
                Some(_) if (0xc2..=0xf4).contains(&byte) => "invalid continuation byte",
                Some(_) => "invalid start byte",
            };
            Err(YamlError::plain(
                "UnicodeDecodeError",
                format!("'utf-8' codec can't decode byte 0x{byte:02x} in position {pos}: {reason}"),
            ))
        }
    }
}

fn error_running(e: &YamlError, name: &str) -> String {
    format!("yq: Error running jq: {}: {}.\n", e.kind, e.message(name))
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    sysio::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv: Vec<String> = args.iter().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let mut a = match parse_args(&argv) {
        Ok(a) => a,
        Err(Exit::Code(c)) => return c,
    };
    // Os arquivos abrem durante a análise (o FileType), antes de qualquer outra coisa.
    let mut inputs = Vec::new();
    for f in a.files.clone() {
        match open_input(&f) {
            Ok(i) => inputs.push(i),
            Err(Exit::Code(c)) => return c,
        }
    }
    let mut null_input = false;
    let mut jq_args: Vec<String> = Vec::new();
    for arg in std::mem::take(&mut a.jq_args) {
        let mut arg = arg;
        if arg.starts_with('-') && !arg.starts_with("--") {
            if arg.contains('n') {
                null_input = true;
            }
            if arg.contains('i') {
                a.in_place = true;
            }
            if arg.contains('y') {
                a.format = Format::Yaml;
            } else if arg.contains('Y') {
                a.format = Format::Annotated;
            } else if arg.contains('x') {
                a.format = Format::Other;
            }
            arg = arg.replace(['i', 'x', 'y', 'Y'], "");
        }
        if a.format != Format::Json {
            arg = arg.replace('C', "");
            if arg == "-" {
                continue;
            }
        }
        jq_args.push(arg);
    }
    for (opt, vals) in &a.spec {
        jq_args.push(opt.clone());
        jq_args.extend(vals.iter().cloned());
    }
    if let Some(filter) = a.filter.clone() {
        if jq_args.iter().any(|x| x == "--from-file" || x == "-f") {
            match open_input(&filter) {
                Ok(i) => inputs.insert(0, i),
                Err(Exit::Code(c)) => return c,
            }
        } else {
            let at = match jq_args.iter().position(|x| x == "--args" || x == "--jsonargs") {
                Some(p) => p + 1,
                None => jq_args.len(),
            };
            jq_args.insert(at, filter);
            if null_input {
                inputs.insert(0, Input { name: "/dev/null".to_string(), data: Vec::new() });
            }
        }
    }
    let stdin_tty = sys::try_current().is_some_and(|s| s.isatty(Fd::STDIN));
    if inputs.is_empty() {
        if stdin_tty {
            print_help();
            return 2;
        }
        match open_input("-") {
            Ok(i) => inputs.push(i),
            Err(Exit::Code(c)) => return c,
        }
    }
    if a.in_place {
        if !matches!(a.format, Format::Yaml | Format::Annotated | Format::Other) {
            sysio::io::eprint_fmt(format_args!("yq: -i/--in-place can only be used with -y/-Y/-t/-x\n"));
            return 1;
        }
        if inputs.len() == 1 && inputs[0].name == "<stdin>" {
            sysio::io::eprint_fmt(format_args!("yq: -i/--in-place can only be used with filename arguments, not on standard input\n"));
            return 1;
        }
    }
    if a.format == Format::Other {
        sysio::io::eprint_fmt(format_args!("yq: Error running jq: Exception: Unknown output format.\n"));
        return 1;
    }
    if a.in_place {
        let mut code = 0;
        for input in inputs {
            let name = input.name.clone();
            let mut out = Vec::new();
            code = yq(&a, &jq_args, vec![input], Some(&mut out));
            if sysio::fs::write(&name, &out).is_err() {
                return 1;
            }
            if code != 0 {
                return code;
            }
        }
        return code;
    }
    yq(&a, &jq_args, inputs, None)
}

/// Um documento carregado e convertido em JSON, mais o tamanho de texto YAML que ele ocupou.
fn load_docs(
    input: &Input,
    annotations: bool,
    max_expansion: i64,
    mut sink: impl FnMut(String) -> bool,
) -> Result<(), String> {
    let text = decode_text(&input.data).map_err(|e| error_running(&e, &input.name))?;
    let mut loader = Loader::new(&text);
    loader.annotations = annotations;
    let mut last = 0usize;
    loop {
        let node = match loader.next_document() {
            Ok(Some(n)) => n,
            Ok(None) => return Ok(()),
            Err(e) => return Err(error_running(&e, &input.name)),
        };
        let doc = Constructor::new(annotations).construct_document(&node).map_err(|e| error_running(&e, &input.name))?;
        let pos = node.borrow().end.index;
        let doc_len = pos.saturating_sub(last) as i64;
        let mut json = String::new();
        to_json(&doc, &mut json).map_err(|e| error_running(&e, &input.name))?;
        if json.chars().count() as i64 > doc_len.saturating_mul(max_expansion) {
            return Err("yq: Error: detected unsafe YAML entity expansion\n".to_string());
        }
        json.push('\n');
        if !sink(json) {
            return Ok(());
        }
        last = pos;
    }
}

fn yq(a: &Args, jq_args: &[String], inputs: Vec<Input>, out_buf: Option<&mut Vec<u8>>) -> i32 {
    let converting = a.format != Format::Json;
    let mut cmd = Command::new("jq");
    cmd.args(jq_args).stdin(Stdio::piped());
    if converting || out_buf.is_some() {
        cmd.stdout(Stdio::piped());
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            sysio::io::eprint_fmt(format_args!(
                "yq: Error starting jq: FileNotFoundError: [Errno 2] {}: 'jq'. Is jq installed and available on PATH?\n",
                sysio::errno::strerror(&e)
            ));
            return 1;
        }
    };
    if !converting {
        // O JSON vai pro jq à medida que cada documento fica pronto.
        let mut stdin = child.stdin.take();
        let mut failure = None;
        for input in &inputs {
            let r = load_docs(input, false, a.max_expansion, |json| {
                stdin.as_mut().is_some_and(|s| s.write_all(json.as_bytes()).is_ok())
            });
            if let Err(msg) = r {
                failure = Some(msg);
                break;
            }
        }
        drop(stdin);
        let reader = child.stdout.take().map(|mut o| {
            let mut v = Vec::new();
            let _ = o.read_to_end(&mut v);
            v
        });
        let status = child.wait();
        if let (Some(buf), Some(v)) = (out_buf, reader) {
            buf.extend_from_slice(&v);
        }
        if let Some(msg) = failure {
            sysio::io::eprint_fmt(format_args!("{msg}"));
            return 1;
        }
        return status.ok().and_then(|s| s.code()).unwrap_or(1);
    }
    // -y/-Y: todos os documentos num buffer, o jq roda inteiro e a saída volta em YAML.
    let annotations = a.format == Format::Annotated;
    let mut buffer = String::new();
    for input in &inputs {
        if let Err(msg) = load_docs(input, annotations, a.max_expansion, |json| {
            buffer.push_str(&json);
            true
        }) {
            let _ = child.kill();
            let _ = child.wait();
            sysio::io::eprint_fmt(format_args!("{msg}"));
            return 1;
        }
    }
    let stdin = child.stdin.take();
    let writer = sysio::thread::spawn(move || {
        if let Some(mut s) = stdin {
            let _ = s.write_all(buffer.as_bytes());
        }
    });
    let mut jq_out = Vec::new();
    if let Some(mut o) = child.stdout.take() {
        let _ = o.read_to_end(&mut jq_out);
    }
    let _ = writer.join();
    let status = child.wait();
    let code = status.ok().and_then(|s| s.code()).unwrap_or(1);
    let text: Vec<char> = String::from_utf8_lossy(&jq_out).chars().collect();
    let mut docs = Vec::new();
    let mut pos = 0;
    let mut decode_error = None;
    while pos < text.len() {
        match raw_decode(&text, pos) {
            Ok((v, end)) => {
                docs.push(v);
                pos = end + 1;
            }
            Err(e) => {
                decode_error = Some(format!("yq: Error running jq: JSONDecodeError: {}.\n", json_error_text(&text[pos..], &crate::py::JsonError { msg: e.msg, pos: e.pos - pos })));
                break;
            }
        }
    }
    let opts = DumpOptions {
        width: a.width,
        indentless: a.indentless,
        explicit_start: a.explicit_start,
        explicit_end: a.explicit_end,
        annotations,
        grammar12: a.grammar12,
    };
    let yaml = if docs.is_empty() && decode_error.is_none() { String::new() } else { dump_all(&docs, &opts) };
    // O dump_all escreve documento a documento; num erro de leitura sai só o que veio antes.
    let yaml = if decode_error.is_some() && docs.is_empty() { String::new() } else { yaml };
    match out_buf {
        Some(buf) => buf.extend_from_slice(yaml.as_bytes()),
        None => {
            let _ = sysio::io::stdout().write_all(yaml.as_bytes());
        }
    }
    if let Some(msg) = decode_error {
        let _ = sysio::io::flush_stdout();
        sysio::io::eprint_fmt(format_args!("{msg}"));
        return 1;
    }
    code
}
