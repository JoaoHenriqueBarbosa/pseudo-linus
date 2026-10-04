//! CLI do jq 1.7.1: porte do `src/main.c` (opções, variáveis, laço de entradas, impressão e códigos
//! de saída 0/1/2/3/4/5).

use std::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;

use jaq_core::{Ctx as JaqCtx, Vars};
use jaq_json::jqfmt::{self, DumpOpts};
use jaq_json::{Map, Rc, Val};
use sysabi::Ctx;

use crate::engine::{self, Data, JqKind, Runtime};
use crate::input::InputState;
use crate::io::{self, Stdout};
use crate::time;

/// Versão que o jq do Debian 13 informa (`jq-1.7`).
const JQ_VERSION: &str = "1.7";

/// `JQ_CONFIG` do pacote do Debian (o `--build-configuration`).
const JQ_CONFIG: &str = "--build=x86_64-linux-gnu --prefix=/usr '--includedir=${prefix}/include' '--mandir=${prefix}/share/man' '--infodir=${prefix}/share/info' --sysconfdir=/etc --localstatedir=/var --disable-option-checking --disable-silent-rules '--libdir=${prefix}/lib/x86_64-linux-gnu' --runstatedir=/run --disable-maintainer-mode --disable-dependency-tracking --disable-static build_alias=x86_64-linux-gnu 'CFLAGS=-g -O2 -Werror=implicit-function-declaration -ffile-prefix-map=/build/reproducible-path/jq-1.7.1=. -fstack-protector-strong -fstack-clash-protection -Wformat -Werror=format-security -fcf-protection' 'LDFLAGS=-Wl,-z,relro -Wl,-z,now' 'CPPFLAGS=-Wdate-time -D_FORTIFY_SOURCE=2'";

// Opções (os bits do main.c).
const SLURP: u32 = 1;
const RAW_INPUT: u32 = 2;
const PROVIDE_NULL: u32 = 4;
const RAW_OUTPUT: u32 = 8;
const RAW_OUTPUT0: u32 = 16;
const ASCII_OUTPUT: u32 = 32;
const COLOR_OUTPUT: u32 = 64;
const NO_COLOR_OUTPUT: u32 = 128;
const SORTED_OUTPUT: u32 = 256;
const FROM_FILE: u32 = 512;
const RAW_NO_LF: u32 = 1024;
const UNBUFFERED_OUTPUT: u32 = 2048;
const EXIT_STATUS: u32 = 4096;
const SEQ: u32 = 16384;

const JQ_OK: i32 = 0;
const JQ_OK_NULL_KIND: i32 = -1;
const JQ_ERROR_SYSTEM: i32 = 2;
const JQ_ERROR_COMPILE: i32 = 3;
const JQ_OK_NO_OUTPUT: i32 = -4;
const JQ_ERROR_UNKNOWN: i32 = 5;

fn usage(progname: &str, code: i32, short: bool) -> String {
    let mut s = format!(
        "jq - commandline JSON processor [version {JQ_VERSION}]\n\nUsage:\t{p} [options] <jq filter> [file...]\n\t{p} [options] --args <jq filter> [strings...]\n\t{p} [options] --jsonargs <jq filter> [JSON_TEXTS...]\n\njq is a tool for processing JSON inputs, applying the given filter to\nits JSON text inputs and producing the filter's results as JSON on\nstandard output.\n\nThe simplest filter is ., which copies jq's input to its output\nunmodified except for formatting. For more advanced filters see\nthe jq(1) manpage (\"man jq\") and/or https://jqlang.github.io/jq/.\n\nExample:\n\n\t$ echo '{{\"foo\": 0}}' | jq .\n\t{{\n\t  \"foo\": 0\n\t}}\n\n",
        p = progname
    );
    if short {
        s.push_str(&format!("For listing the command options, use {progname} --help.\n"));
    } else {
        s.push_str(concat!(
            "Command options:\n",
            "  -n, --null-input          use `null` as the single input value;\n",
            "  -R, --raw-input           read each line as string instead of JSON;\n",
            "  -s, --slurp               read all inputs into an array and use it as\n",
            "                            the single input value;\n",
            "  -c, --compact-output      compact instead of pretty-printed output;\n",
            "  -r, --raw-output          output strings without escapes and quotes;\n",
            "      --raw-output0         implies -r and output NUL after each output;\n",
            "  -j, --join-output         implies -r and output without newline after\n",
            "                            each output;\n",
            "  -a, --ascii-output        output strings by only ASCII characters\n",
            "                            using escape sequences;\n",
            "  -S, --sort-keys           sort keys of each object on output;\n",
            "  -C, --color-output        colorize JSON output;\n",
            "  -M, --monochrome-output   disable colored output;\n",
            "      --tab                 use tabs for indentation;\n",
            "      --indent n            use n spaces for indentation (max 7 spaces);\n",
            "      --unbuffered          flush output stream after each output;\n",
            "      --stream              parse the input value in streaming fashion;\n",
            "      --stream-errors       implies --stream and report parse error as\n",
            "                            an array;\n",
            "      --seq                 parse input/output as application/json-seq;\n",
            "  -f, --from-file file      load filter from the file;\n",
            "  -L directory              search modules from the directory;\n",
            "      --arg name value      set $name to the string value;\n",
            "      --argjson name value  set $name to the JSON value;\n",
            "      --slurpfile name file set $name to an array of JSON values read\n",
            "                            from the file;\n",
            "      --rawfile name file   set $name to string contents of file;\n",
            "      --args                consume remaining arguments as positional\n",
            "                            string values;\n",
            "      --jsonargs            consume remaining arguments as positional\n",
            "                            JSON values;\n",
            "  -e, --exit-status         set exit status code based on the output;\n",
            "  -V, --version             show the version;\n",
            "  --build-configuration     show jq's build configuration;\n",
            "  -h, --help                show the help;\n",
            "  --                        terminates argument processing;\n\n",
            "Named arguments are also available as $ARGS.named[], while\n",
            "positional arguments are available as $ARGS.positional[].\n",
        ));
    }
    let _ = code;
    s
}

fn isoptish(t: &[u8]) -> bool {
    t.first() == Some(&b'-') && t.get(1).is_some_and(|c| *c == b'-' || c.is_ascii_alphabetic())
}

fn isoption(t: &[u8], short: Option<u8>, long: &str, short_opts: &mut usize) -> bool {
    if t.first() != Some(&b'-') || t.get(1) == Some(&b'-') {
        *short_opts = 0;
    }
    if t.first() != Some(&b'-') {
        return false;
    }
    if t.get(1) == Some(&b'-') {
        return &t[2..] == long.as_bytes();
    }
    match short {
        Some(c) if t.contains(&c) => {
            *short_opts += 1;
            true
        }
        _ => false,
    }
}

/// `atoi`.
fn atoi(s: &str) -> i32 {
    let t = s.trim_start();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let n: i64 = digits.bytes().take_while(u8::is_ascii_digit).fold(0i64, |acc, d| acc.saturating_mul(10).saturating_add((d - b'0') as i64));
    (if neg { -n } else { n }).clamp(i32::MIN as i64, i32::MAX as i64) as i32
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

/// Opções de impressão (`dumpopts`).
#[derive(Clone)]
struct Dump {
    pretty: bool,
    tab: bool,
    indent: usize,
    color: bool,
}

impl Dump {
    /// `JV_PRINT_INDENT_FLAGS(n)`.
    fn indent_flags(n: i32) -> (bool, bool, usize) {
        if !(0..=7).contains(&n) {
            (true, true, 0)
        } else if n == 0 {
            (false, false, 0)
        } else {
            (true, false, n as usize)
        }
    }
}

fn die(progname: &str, msg: &str) -> i32 {
    io::stderr(format!("{msg}Use {progname} --help for help with command-line options,\nor see the jq manpage, or online docs  at https://jqlang.github.io/jq\n").as_bytes());
    2
}

fn lossy(b: &[u8]) -> String {
    jaq_json::jqparse::utf8_lossy(b)
}

/// Entrada do programa `jq`.
pub fn jq_main(ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let progname = argv.first().map(|a| lossy(a.as_bytes())).unwrap_or_else(|| "jq".into());
    let args: Vec<&[u8]> = argv.iter().skip(1).map(|a| a.as_bytes()).collect();
    let sys = ctx.sys().clone();

    let mut options: u32 = 0;
    let (mut pretty, mut tab, mut indent) = Dump::indent_flags(2);
    let mut program: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut named: Vec<(String, Val)> = Vec::new();
    let mut positional: Vec<Val> = Vec::new();
    let mut further_strings = false;
    let mut further_json = false;
    let mut args_done = false;
    let mut stream = false;
    let mut lib_paths: Option<Vec<String>> = None;
    let mut stdout = Stdout::new();

    let has_named = |named: &Vec<(String, Val)>, k: &str| named.iter().any(|(n, _)| n == k);

    let mut i = 0;
    while i < args.len() {
        let a = args[i];
        let mut so = 0usize;
        if args_done || !isoptish(a) {
            if program.is_none() {
                program = Some(lossy(a));
            } else if further_strings {
                positional.push(Val::from(lossy(a)));
            } else if further_json {
                match jaq_json::jqparse::parse_single(a) {
                    Ok(v) => positional.push(v),
                    Err(_) => return die(&progname, &format!("{progname}: invalid JSON text passed to --jsonargs\n")),
                }
            } else {
                files.push(lossy(a));
            }
            i += 1;
            continue;
        }
        if a == b"--" {
            args_done = true;
            i += 1;
            continue;
        }
        if a.get(1) == Some(&b'L') {
            let paths = lib_paths.get_or_insert_with(Vec::new);
            if a.len() > 2 {
                paths.push(lossy(&a[2..]));
            } else if i + 1 >= args.len() {
                return die(&progname, "-L takes a parameter: (e.g. -L /search/path or -L/search/path)\n");
            } else {
                paths.push(lossy(args[i + 1]));
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
        flag!(Some(b's'), "slurp", { options |= SLURP; });
        flag!(Some(b'r'), "raw-output", { options |= RAW_OUTPUT; });
        flag!(None, "raw-output0", { options |= RAW_OUTPUT | RAW_NO_LF | RAW_OUTPUT0; });
        flag!(Some(b'j'), "join-output", { options |= RAW_OUTPUT | RAW_NO_LF; });
        flag!(Some(b'c'), "compact-output", {
            pretty = false;
            tab = false;
            indent = 0;
        });
        flag!(Some(b'C'), "color-output", { options |= COLOR_OUTPUT; });
        flag!(Some(b'M'), "monochrome-output", { options |= NO_COLOR_OUTPUT; });
        flag!(Some(b'a'), "ascii-output", { options |= ASCII_OUTPUT; });
        if isoption(a, None, "unbuffered", &mut so) {
            options |= UNBUFFERED_OUTPUT;
            i += 1;
            continue;
        }
        flag!(Some(b'S'), "sort-keys", { options |= SORTED_OUTPUT; });
        flag!(Some(b'R'), "raw-input", { options |= RAW_INPUT; });
        flag!(Some(b'n'), "null-input", { options |= PROVIDE_NULL; });
        flag!(Some(b'f'), "from-file", { options |= FROM_FILE; });
        if isoption(a, None, "tab", &mut so) {
            indent = 0;
            tab = true;
            pretty = true;
            i += 1;
            continue;
        }
        if isoption(a, None, "indent", &mut so) {
            if i + 1 >= args.len() {
                return die(&progname, &format!("{progname}: --indent takes one parameter\n"));
            }
            let n = atoi(&lossy(args[i + 1]));
            if !(-1..=7).contains(&n) {
                return die(&progname, &format!("{progname}: --indent takes a number between -1 and 7\n"));
            }
            (pretty, tab, indent) = Dump::indent_flags(n);
            i += 2;
            continue;
        }
        if isoption(a, None, "seq", &mut so) {
            options |= SEQ;
            i += 1;
            continue;
        }
        if isoption(a, None, "stream", &mut so) || isoption(a, None, "stream-errors", &mut so) {
            stream = true;
            i += 1;
            continue;
        }
        flag!(Some(b'e'), "exit-status", { options |= EXIT_STATUS; });
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
                return die(&progname, &format!("{progname}: --arg takes two parameters (e.g. --arg varname value)\n"));
            }
            let name = lossy(args[i + 1]);
            if !has_named(&named, &name) {
                named.push((name, Val::from(lossy(args[i + 2]))));
            }
            i += 3;
            continue;
        }
        if isoption(a, None, "argjson", &mut so) {
            if i + 2 >= args.len() {
                return die(&progname, &format!("{progname}: --argjson takes two parameters (e.g. --argjson varname text)\n"));
            }
            let name = lossy(args[i + 1]);
            if !has_named(&named, &name) {
                match jaq_json::jqparse::parse_single(args[i + 2]) {
                    Ok(v) => named.push((name, v)),
                    Err(_) => return die(&progname, &format!("{progname}: invalid JSON text passed to --argjson\n")),
                }
            }
            i += 3;
            continue;
        }
        let raw = isoption(a, None, "rawfile", &mut so);
        if raw || isoption(a, None, "slurpfile", &mut so) {
            let which = if raw { "rawfile" } else { "slurpfile" };
            if i + 2 >= args.len() {
                return die(&progname, &format!("{progname}: --{which} takes two parameters (e.g. --{which} varname filename)\n"));
            }
            let name = lossy(args[i + 1]);
            let file = lossy(args[i + 2]);
            if !has_named(&named, &name) {
                let data = io::load_file(&file).map_err(|e| format!("Could not open {file}: {e}"));
                let v = data.and_then(|bytes| {
                    if raw {
                        Ok(Val::from(lossy(&bytes)))
                    } else {
                        jaq_json::jqparse::parse_all(&bytes).map(|vs| Val::Arr(Rc::new(vs)))
                    }
                });
                match v {
                    Ok(v) => named.push((name, v)),
                    Err(msg) => {
                        io::stderr(format!("{progname}: Bad JSON in --{which} {name} {file}: {msg}\n").as_bytes());
                        return JQ_ERROR_SYSTEM;
                    }
                }
            }
            i += 3;
            continue;
        }
        if isoption(a, None, "debug-dump-disasm", &mut so) {
            i += 1;
            continue;
        }
        flag!(None, "debug-trace=all", {});
        if isoption(a, None, "debug-trace", &mut so) {
            i += 1;
            continue;
        }
        if isoption(a, Some(b'h'), "help", &mut so) {
            stdout.write(usage(&progname, 0, false).as_bytes());
            stdout.flush();
            return 0;
        }
        if isoption(a, Some(b'V'), "version", &mut so) {
            stdout.write(format!("jq-{JQ_VERSION}\n").as_bytes());
            return finish(&mut stdout, options, JQ_OK, -1);
        }
        if isoption(a, None, "build-configuration", &mut so) {
            stdout.write(format!("{JQ_CONFIG}\n").as_bytes());
            return finish(&mut stdout, options, JQ_OK, -1);
        }
        if isoption(a, None, "run-tests", &mut so) {
            io::stderr(b"jq: --run-tests is not supported here\n");
            return JQ_ERROR_SYSTEM;
        }
        if a.len() != so + 1 {
            return die(&progname, &format!("{progname}: Unknown option {}\n", lossy(a)));
        }
        i += 1;
    }

    // Cor: terminal liga, `NO_COLOR` não vazio desliga; `-C` força e `-M` desliga.
    let mut color = false;
    if stdout.is_tty() {
        color = true;
        if ctx.getenv("NO_COLOR").is_some_and(|v| !v.is_empty()) {
            color = false;
        }
    }
    if options & COLOR_OUTPUT != 0 {
        color = true;
    }
    if options & NO_COLOR_OUTPUT != 0 {
        color = false;
    }
    let mut colors = None;
    if let Some(spec) = ctx.getenv("JQ_COLORS") {
        match jqfmt::parse_colors(&lossy(&spec)) {
            Some(c) => colors = Some(c),
            None => io::stderr(b"Failed to set $JQ_COLORS\n"),
        }
    }
    let palette = colors.unwrap_or_else(|| jqfmt::DEFAULT_COLORS.map(String::from));
    let dump = Dump { pretty, tab, indent, color };
    let dump_opts = DumpOpts {
        pretty: dump.pretty,
        tab: dump.tab,
        indent: dump.indent,
        sort_keys: options & SORTED_OUTPUT != 0,
        ascii: options & ASCII_OUTPUT != 0,
        colors: dump.color.then(|| palette.clone()),
    };
    let debug_opts = DumpOpts { pretty: false, tab: false, indent: 0, ..dump_opts.clone() };

    let search_list = match &lib_paths {
        None => engine::str_array(&["~/.jq", "$ORIGIN/../lib/jq", "$ORIGIN/../lib"]),
        Some(p) => Val::Arr(Rc::new(p.iter().map(|s| Val::from(s.clone())).collect())),
    };
    let jq_origin = dirname(&progname);
    let cwd = sys.getcwd().map(|c| lossy(&c)).unwrap_or_else(|_| ".".into());

    let program = match program {
        Some(p) => p,
        None => {
            if !sys.isatty(sysabi::Fd::STDOUT) || !sys.isatty(sysabi::Fd::STDIN) {
                ".".to_string()
            } else {
                io::stderr(usage(&progname, 2, true).as_bytes());
                return 2;
            }
        }
    };
    let (program_text, prog_origin) = if options & FROM_FILE != 0 {
        match io::load_file(&program) {
            Ok(bytes) => (skip_shebang(&lossy(&bytes)).to_string(), absolute(&dirname(&program), &cwd)),
            Err(e) => {
                io::stderr(format!("{progname}: Could not open {program}: {e}\n").as_bytes());
                return finish(&mut stdout, options, JQ_ERROR_SYSTEM, -1);
            }
        }
    } else {
        (program, cwd.clone())
    };

    // Variáveis globais: argumentos nomeados, $ARGS, $JQ_BUILD_CONFIGURATION e $ENV.
    let mut named_map = Map::default();
    for (k, v) in &named {
        named_map.insert(Val::from(k.clone()), v.clone());
    }
    let mut args_obj = Map::default();
    args_obj.insert(Val::str("positional"), Val::Arr(Rc::new(positional)));
    args_obj.insert(Val::str("named"), Val::obj(named_map));
    let env_obj = engine::env_object(&sys.environ());
    let mut global_names: Vec<String> = named.iter().map(|(k, _)| k.clone()).collect();
    let mut global_vals: Vec<Val> = named.iter().map(|(_, v)| v.clone()).collect();
    if !has_named(&named, "ARGS") {
        global_names.push("ARGS".into());
        global_vals.push(Val::obj(args_obj));
    }
    if !has_named(&named, "JQ_BUILD_CONFIGURATION") {
        global_names.push("JQ_BUILD_CONFIGURATION".into());
        global_vals.push(Val::str(JQ_CONFIG));
    }
    global_names.push("ENV".into());
    global_vals.push(env_obj.clone());

    let filter = match engine::compile(&program_text, &global_names) {
        Ok(f) => f,
        Err(e) => {
            for m in &e.messages {
                io::stderr(format!("jq: error: {m}\n").as_bytes());
            }
            let n = e.messages.len().max(1);
            let s = if n == 1 { "" } else { "s" };
            io::stderr(format!("jq: {n} compile error{s}\n").as_bytes());
            return finish(&mut stdout, options, JQ_ERROR_COMPILE, -1);
        }
    };

    let input = InputState::new(files, options & RAW_INPUT != 0, options & SLURP != 0, options & SEQ != 0, stream);
    let rt = Runtime {
        sys: sys.clone(),
        ticks: Cell::new(0),
        env_obj,
        input: RefCell::new(input),
        debug_opts,
        halted: RefCell::new(None),
        jq_origin,
        prog_origin,
        search_list,
        tz: time::TimeZone::from_process(&*sys),
    };
    let data = Data { lut: &filter.lut, rt: &rt };
    let mut last_result: i32 = -1;

    let run = || -> i32 {
        let mut ret = JQ_OK_NO_OUTPUT;
        let process = |value: Val, stdout: &mut Stdout| -> i32 {
            let ctx = JaqCtx::<JqKind>::new(&data, Vars::new(global_vals.clone()));
            let mut ret = JQ_OK_NO_OUTPUT;
            for item in filter.id.run((ctx, value)) {
                match item {
                    Ok(v) => {
                        if options & RAW_OUTPUT != 0 && v.is_str() {
                            let bytes = v.str_bytes().unwrap_or_default();
                            if options & ASCII_OUTPUT != 0 {
                                let o = DumpOpts { ascii: true, ..DumpOpts::compact() };
                                stdout.write(&jqfmt::dump(&v, &o));
                            } else if options & RAW_OUTPUT0 != 0 && bytes.contains(&0) {
                                let pos = rt.input.borrow().position();
                                io::stderr(
                                    format!("jq: error (at {pos}): Cannot dump a string containing NUL with --raw-output0 option\n")
                                        .as_bytes(),
                                );
                                return JQ_ERROR_UNKNOWN;
                            } else {
                                stdout.write(bytes);
                            }
                            ret = JQ_OK;
                        } else {
                            ret = if matches!(v, Val::Null | Val::Bool(false)) { JQ_OK_NULL_KIND } else { JQ_OK };
                            if options & SEQ != 0 {
                                stdout.write(b"\x1e");
                            }
                            stdout.write(&jqfmt::dump(&v, &dump_opts));
                        }
                        if options & RAW_NO_LF == 0 {
                            stdout.write(b"\n");
                        }
                        if options & RAW_OUTPUT0 != 0 {
                            stdout.write(b"\0");
                        }
                        if options & UNBUFFERED_OUTPUT != 0 {
                            stdout.flush();
                        }
                    }
                    Err(exn) => {
                        match exn.get_err() {
                            Ok(e) => {
                                let pos = rt.input.borrow().position();
                                let msg = e.into_val();
                                match msg.as_str() {
                                    Some(m) => io::stderr(format!("jq: error (at {pos}): {m}\n").as_bytes()),
                                    None => io::stderr(
                                        format!("jq: error (at {pos}) (not a string): {}\n", jqfmt::dump_compact(&msg))
                                            .as_bytes(),
                                    ),
                                }
                                return JQ_ERROR_UNKNOWN;
                            }
                            Err(_halt) => return halt_result(&rt),
                        }
                    }
                }
            }
            ret
        };

        if options & PROVIDE_NULL != 0 {
            ret = process(Val::Null, &mut stdout);
        } else {
            loop {
                if rt.input.borrow().failures != 0 {
                    break;
                }
                let next = rt.input.borrow_mut().next_value();
                match next {
                    None => break,
                    Some(Ok(v)) => {
                        ret = process(v, &mut stdout);
                        if ret <= 0 && ret != JQ_OK_NO_OUTPUT {
                            last_result = (ret != JQ_OK_NULL_KIND) as i32;
                        }
                        if rt.halted.borrow().is_some() {
                            break;
                        }
                    }
                    Some(Err(msg)) => {
                        if options & SEQ == 0 {
                            ret = JQ_ERROR_UNKNOWN;
                            io::stderr(format!("jq: parse error: {msg}\n").as_bytes());
                            break;
                        }
                        io::stderr(format!("jq: ignoring parse error: {msg}\n").as_bytes());
                    }
                }
            }
        }
        if rt.input.borrow().failures != 0 {
            ret = JQ_ERROR_SYSTEM;
        }
        ret
    };
    let ret = crate::catch_oom(run);
    finish(&mut stdout, options, ret, last_result)
}

/// Fim do `halt`/`halt_error`: código de saída e mensagem no stderr (sem prefixo).
fn halt_result(rt: &Runtime) -> i32 {
    let halted = rt.halted.borrow();
    let Some((code, msg)) = halted.as_ref() else { return JQ_ERROR_UNKNOWN };
    let ret = match code {
        None => JQ_OK,
        Some(c) => {
            if c.is_nan() || *c >= 2_147_483_648.0 || *c <= -2_147_483_649.0 {
                i32::MIN
            } else {
                c.trunc() as i32
            }
        }
    };
    match msg {
        Some(Val::TStr(b) | Val::BStr(b)) => io::stderr(b),
        Some(Val::Null) | None => {}
        Some(other) => io::stderr(format!("{}\n", jqfmt::dump_compact(other)).as_bytes()),
    }
    ret
}

/// Saída do main.c: fecha o stdout (erro de escrita vira 2) e aplica o `--exit-status`.
fn finish(stdout: &mut Stdout, options: u32, mut ret: i32, last_result: i32) -> i32 {
    stdout.flush();
    if let Some(e) = stdout.error {
        io::stderr(format!("jq: error: writing output failed: {}\n", io::strerror(e)).as_bytes());
        ret = JQ_ERROR_SYSTEM;
    }
    if options & EXIT_STATUS != 0 {
        if ret != JQ_OK_NO_OUTPUT {
            ret.wrapping_abs()
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
    }
}

/// `dirname(3)`.
fn dirname(p: &str) -> String {
    let t = p.trim_end_matches('/');
    if t.is_empty() {
        return if p.starts_with('/') { "/".into() } else { ".".into() };
    }
    match t.rfind('/') {
        None => ".".into(),
        Some(i) => {
            let d = t[..i].trim_end_matches('/');
            if d.is_empty() { "/".into() } else { d.to_string() }
        }
    }
}

/// `realpath` simplificado de um diretório (absoluto a partir do cwd, sem `.`).
fn absolute(dir: &str, cwd: &str) -> String {
    let joined = if dir.starts_with('/') { dir.to_string() } else { format!("{cwd}/{dir}") };
    let mut parts: Vec<&str> = Vec::new();
    for c in joined.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    format!("/{}", parts.join("/"))
}
