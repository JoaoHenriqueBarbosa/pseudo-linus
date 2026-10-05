//! `strings` do GNU binutils 2.44 (Debian 13), escrito a partir do comportamento observado em caixa
//! preta no oráculo, da man page e do `--help` (o código do binutils é GPL e não foi consultado).
//!
//! O que o programa faz:
//!
//! - Varre cada arquivo (ou o stdin, como `{standard input}`) atrás de sequências de pelo menos `-n`
//!   caracteres "gráficos" (padrão 4) e imprime cada uma seguida do separador (`-s`, padrão `\n`).
//!   Gráfico é tab ou ASCII imprimível; com `-e S` também os bytes acima de 127; com `-w` também
//!   `\n`, `\v`, `\f` e `\r`.
//! - `-e b/l/B/L` lê caracteres de 16 ou 32 bits; um caractere não gráfico devolve ao fluxo todos os
//!   bytes menos o primeiro, então a busca recomeça um byte depois do início dele (é assim que o
//!   original acha cadeias em deslocamento ímpar).
//! - `-U` diferente de `default` força `-e S` e trata sequências UTF-8 (líder `c0..ff`, até três
//!   bytes de continuação `80..bf`) como um caractere só, contado uma vez no mínimo: `invalid` as
//!   trata como não gráficas, `locale` imprime só o primeiro byte, `escape` imprime `\uXXXX` (com as
//!   contas do original, inclusive a de quatro bytes, que não dá o código Unicode), `hex` imprime
//!   `<0x...>` e `highlight` imprime o escape em vermelho quando a saída é terminal. Uma sequência
//!   inválida devolve ao fluxo o primeiro byte que não é continuação; quando isso termina uma cadeia
//!   já impressa e a cadeia seguinte começa exatamente nesse byte, o original informa o deslocamento
//!   dela somado de 2^32 (defeito reproduzido de propósito, porque aparece com `-t`).
//! - `-d` lê os cabeçalhos ELF (32/64 bits, as duas ordens de bytes) e varre só as seções alocadas
//!   que têm conteúdo no arquivo, cada uma separadamente, com os mesmos avisos do BFD para seção que
//!   passa do fim do arquivo e para índice de tabela de nomes corrompido. Arquivo que não é ELF
//!   reconhecível, ou ELF sem nenhuma seção dessas, é varrido inteiro, como no original.
//! - Opções numéricas (`-8`) usam o elemento do argv já permutado pelo `getopt` no índice que o
//!   `optind` tinha quando o dígito foi lido, como o original; daí `strings arq -5` falhar com
//!   `minimum string length is too small: ` (o elemento lido é `arq`, sem o primeiro caractere).
//! - `@arquivo` no argv é expandido antes de tudo (aspas simples e duplas, barra invertida), com os
//!   erros do libiberty para diretório e para expansão em laço.
//!
//! Divergências conhecidas: com `-d`, formatos que o BFD reconheceria além de ELF (PE, S-record,
//! Intel HEX, tekhex) são varridos inteiros; seção ELF comprimida (`SHF_COMPRESSED`) é varrida como
//! está, sem descomprimir.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, Fd, FileType, sys};

use crate::util::io::{self, File};
use crate::util::{Getopt, HasArg, LongOpt};

const SHORTOPTS: &str = "adfhHn:wot:e:T:s:U:Vv0123456789";

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("all", HasArg::No, 'a' as i32),
    LongOpt::new("bytes", HasArg::Required, 'n' as i32),
    LongOpt::new("data", HasArg::No, 'd' as i32),
    LongOpt::new("encoding", HasArg::Required, 'e' as i32),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("include-all-whitespace", HasArg::No, 'w' as i32),
    LongOpt::new("output-separator", HasArg::Required, 's' as i32),
    LongOpt::new("print-file-name", HasArg::No, 'f' as i32),
    LongOpt::new("radix", HasArg::Required, 't' as i32),
    LongOpt::new("target", HasArg::Required, 'T' as i32),
    LongOpt::new("unicode", HasArg::Required, 'U' as i32),
    LongOpt::new("version", HasArg::No, 'v' as i32),
];

/// Opções curtas que levam argumento (pra reconstruir a permutação do argv).
const SHORT_WITH_ARG: &[u8] = b"ntTesU";

/// Alvos que o BFD do Debian 13 (x86-64) conhece; nome fora da lista faz `-d` varrer o arquivo todo.
const TARGETS: &[&str] = &[
    "elf64-x86-64",
    "elf32-i386",
    "elf32-iamcu",
    "elf32-x86-64",
    "pei-i386",
    "pe-x86-64",
    "pei-x86-64",
    "elf64-little",
    "elf64-big",
    "elf32-little",
    "elf32-big",
    "pe-bigobj-x86-64",
    "pe-i386",
    "pdb",
    "srec",
    "symbolsrec",
    "verilog",
    "tekhex",
    "binary",
    "ihex",
    "plugin",
];

const VERSION_TEXT: &str = "GNU strings (GNU Binutils for Debian) 2.44\n\
Copyright (C) 2025 Free Software Foundation, Inc.\n\
This program is free software; you may redistribute it under the terms of\n\
the GNU General Public License version 3 or (at your option) any later version.\n\
This program has absolutely no warranty.\n";

const USAGE_BODY: &str = " Display printable strings in [file(s)] (stdin by default)\n\
\x20The options are:\n\
\x20 -a - --all                Scan the entire file, not just the data section [default]\n\
\x20 -d --data                 Only scan the data sections in the file\n\
\x20 -f --print-file-name      Print the name of the file before each string\n\
\x20 -n <number>               Locate & print any sequence of at least <number>\n\
\x20   --bytes=<number>         displayable characters.  (The default is 4).\n\
\x20 -t --radix={o,d,x}        Print the location of the string in base 8, 10 or 16\n\
\x20 -w --include-all-whitespace Include all whitespace as valid string characters\n\
\x20 -o                        An alias for --radix=o\n\
\x20 -T --target=<BFDNAME>     Specify the binary file format\n\
\x20 -e --encoding={s,S,b,l,B,L} Select character size and endianness:\n\
\x20                           s = 7-bit, S = 8-bit, {b,l} = 16-bit, {B,L} = 32-bit\n\
\x20 --unicode={default|locale|invalid|hex|escape|highlight}\n\
\x20 -U {d|l|i|x|e|h}          Specify how to treat UTF-8 encoded unicode characters\n\
\x20 -s --output-separator=<string> String used to separate strings in output.\n\
\x20 @<file>                   Read options from <file>\n\
\x20 -h --help                 Display this information\n\
\x20 -v -V --version           Print the program's version number\n";

/// Tratamento das sequências UTF-8 (`-U`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Unicode {
    Default,
    Invalid,
    Locale,
    Escape,
    Hex,
    Highlight,
}

impl Unicode {
    fn parse(arg: &[u8]) -> Option<Unicode> {
        Some(match arg {
            b"default" | b"d" => Unicode::Default,
            b"invalid" | b"i" => Unicode::Invalid,
            b"locale" | b"l" => Unicode::Locale,
            b"escape" | b"e" => Unicode::Escape,
            b"hex" | b"x" => Unicode::Hex,
            b"highlight" | b"h" => Unicode::Highlight,
            _ => return None,
        })
    }
}

/// Configuração de varredura.
#[derive(Clone, Debug)]
struct Config {
    min: u64,
    print_names: bool,
    radix: Option<u8>,
    encoding: u8,
    unicode: Unicode,
    whitespace: bool,
    separator: Vec<u8>,
    data_only: bool,
    target: Option<Vec<u8>>,
    /// Saída em terminal (cor do `-U highlight`).
    tty: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            min: 4,
            print_names: false,
            radix: None,
            encoding: b's',
            unicode: Unicode::Default,
            whitespace: false,
            separator: b"\n".to_vec(),
            data_only: false,
            target: None,
            tty: false,
        }
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let prog = io::argv0(args);
    let argv = match expand_response_files(&prog, argv) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let mut cfg = Config::default();
    let rest: Vec<Vec<u8>> = argv.get(1..).unwrap_or(&[]).to_vec();
    let posix = sys::try_current().is_some_and(|s| s.getenv(b"POSIXLY_CORRECT").is_some());
    let mut g = Getopt::new(&rest, SHORTOPTS, LONGOPTS, posix);
    let mut numeric_opt = 0usize;
    // (índice do elemento, posição dentro do agrupamento) da última opção curta, pra saber se um
    // dígito é o último caractere do elemento (aí o `optind` da glibc já avançou).
    let mut cluster: Option<(usize, usize)> = None;
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&prog)));
                usage(&prog, false);
                return 1;
            }
        };
        let idx = g.index();
        let is_short = o.spelled.len() == 2 && !o.spelled.starts_with("--");
        let pos = match (is_short, cluster) {
            (true, Some((ci, p))) if ci == idx => p + 1,
            _ => 1,
        };
        cluster = is_short.then_some((idx, pos));
        let arg = o.arg.clone().unwrap_or_default();
        match u8::try_from(o.id).unwrap_or(0) {
            b'a' => cfg.data_only = false,
            b'd' => cfg.data_only = true,
            b'f' => cfg.print_names = true,
            b'h' | b'H' => {
                usage(&prog, true);
                return 0;
            }
            b'n' => match parse_min(&arg) {
                Ok(n) => cfg.min = n,
                Err(msg) => return fatal(&prog, &msg),
            },
            b'w' => cfg.whitespace = true,
            b'o' => cfg.radix = Some(b'o'),
            b't' => match arg.as_slice() {
                [c @ (b'o' | b'd' | b'x')] => cfg.radix = Some(*c),
                _ => {
                    usage(&prog, false);
                    return 1;
                }
            },
            b'e' => match arg.as_slice() {
                [c @ (b's' | b'S' | b'b' | b'l' | b'B' | b'L')] => cfg.encoding = *c,
                _ => {
                    usage(&prog, false);
                    return 1;
                }
            },
            b'T' => cfg.target = Some(arg),
            b's' => cfg.separator = arg,
            b'U' => match Unicode::parse(&arg) {
                Some(u) => cfg.unicode = u,
                None => {
                    let mut msg = b"invalid argument to -U/--unicode: ".to_vec();
                    msg.extend_from_slice(&arg);
                    return fatal(&prog, &msg);
                }
            },
            b'v' | b'V' => {
                let mut out = io::stdout();
                let _ = out.write_all(VERSION_TEXT.as_bytes());
                return 0;
            }
            b'0'..=b'9' => {
                let elem_len = rest.get(idx.wrapping_sub(1)).map_or(0, Vec::len);
                let last = pos + 1 >= elem_len;
                numeric_opt = if last { idx + 1 } else { idx };
            }
            _ => {}
        }
    }
    let operands = g.operands();
    if numeric_opt != 0 {
        let permuted = permuted_argv(&argv, posix);
        let elem = permuted.get(numeric_opt - 1).cloned().unwrap_or_default();
        let digits = elem.get(1..).unwrap_or(&[]);
        match parse_min(digits) {
            Ok(n) => cfg.min = n,
            Err(msg) => return fatal(&prog, &msg),
        }
    }
    if cfg.min == u64::from(u32::MAX) {
        return fatal(&prog, format!("minimum string length {} is too big", cfg.min).as_bytes());
    }
    if cfg.unicode != Unicode::Default {
        cfg.encoding = b'S';
    }
    cfg.tty = io::stdout_is_tty();

    let mut status = 0;
    let mut files_given = false;
    if operands.is_empty() {
        files_given = true;
        let mut out = Output::new(&cfg);
        let mut src = Source::fd(Fd::STDIN);
        scan(&mut src, 0, b"{standard input}", &cfg, &mut out);
        out.flush();
    } else {
        for op in &operands {
            if op == b"-" {
                cfg.data_only = false;
                continue;
            }
            files_given = true;
            if !process_file(&prog, op, &cfg) {
                status = 1;
            }
        }
    }
    if !files_given {
        usage(&prog, false);
        return 1;
    }
    status
}

/// `prog: msg` no stderr e código 1, como o `fatal` do binutils.
fn fatal(prog: &str, msg: &[u8]) -> i32 {
    let mut line = format!("{prog}: ").into_bytes();
    line.extend_from_slice(msg);
    line.push(b'\n');
    io::eprint(line);
    1
}

fn usage(prog: &str, to_stdout: bool) {
    let mut text = format!("Usage: {prog} [option(s)] [file(s)]\n");
    text.push_str(USAGE_BODY);
    text.push_str(&format!("{prog}: supported targets: {}\n", TARGETS.join(" ")));
    if to_stdout {
        text.push_str("Report bugs to <https://sourceware.org/bugzilla/>\n");
        let mut out = io::stdout();
        let _ = out.write_all(text.as_bytes());
    } else {
        io::eprint(text);
    }
}

/// `strtoul(s, &end, 0)` da glibc: espaço inicial, sinal, base pelo prefixo; estouro satura em
/// `ULONG_MAX`. Devolve o valor e quantos bytes foram consumidos (0 se não havia número).
fn strtoul(s: &[u8]) -> (u64, usize) {
    let mut i = 0;
    while i < s.len() && matches!(s[i], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
        i += 1;
    }
    let mut neg = false;
    if i < s.len() && (s[i] == b'+' || s[i] == b'-') {
        neg = s[i] == b'-';
        i += 1;
    }
    let mut base = 10u64;
    if i < s.len() && s[i] == b'0' {
        if matches!(s.get(i + 1), Some(b'x' | b'X')) && s.get(i + 2).is_some_and(u8::is_ascii_hexdigit) {
            base = 16;
            i += 2;
        } else {
            base = 8;
        }
    }
    let digits_start = i;
    let mut value: u64 = 0;
    let mut overflow = false;
    while i < s.len() {
        let d = match s[i] {
            c @ b'0'..=b'9' => u64::from(c - b'0'),
            c @ b'a'..=b'z' => u64::from(c - b'a') + 10,
            c @ b'A'..=b'Z' => u64::from(c - b'A') + 10,
            _ => break,
        };
        if d >= base {
            break;
        }
        match value.checked_mul(base).and_then(|v| v.checked_add(d)) {
            Some(v) => value = v,
            None => overflow = true,
        }
        i += 1;
    }
    if i == digits_start {
        return (0, 0);
    }
    if overflow {
        return (u64::MAX, i);
    }
    (if neg { value.wrapping_neg() } else { value }, i)
}

/// Valida o mínimo de `-n`/`--bytes`/`-NUM`, com as mensagens do original.
fn parse_min(s: &[u8]) -> Result<u64, Vec<u8>> {
    let (value, used) = strtoul(s);
    let with = |prefix: &str| {
        let mut m = prefix.as_bytes().to_vec();
        m.extend_from_slice(s);
        m
    };
    if used < s.len() {
        return Err(with("invalid integer argument "));
    }
    if value == 0 {
        return Err(with("minimum string length is too small: "));
    }
    if value > u64::from(u32::MAX) {
        return Err(with("minimum string length is too big: "));
    }
    Ok(value)
}

/// Opção longa que leva argumento (nome exato ou prefixo, como o `getopt_long`).
fn long_takes_arg(name: &[u8]) -> bool {
    let name = String::from_utf8_lossy(name);
    let found = LONGOPTS
        .iter()
        .find(|l| l.name == name)
        .or_else(|| LONGOPTS.iter().find(|l| l.name.starts_with(name.as_ref())));
    found.is_some_and(|l| l.has_arg == HasArg::Required)
}

/// O argv como fica depois que o `getopt_long` da glibc termina de permutar: `argv[0]`, as opções
/// com seus argumentos na ordem, o `--` (se houver) e depois os operandos na ordem. Com
/// `POSIXLY_CORRECT` nada é permutado.
fn permuted_argv(argv: &[Vec<u8>], posix: bool) -> Vec<Vec<u8>> {
    if posix {
        return argv.to_vec();
    }
    let mut opts: Vec<Vec<u8>> = argv.first().cloned().into_iter().collect();
    let mut nonopts: Vec<Vec<u8>> = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if a.as_slice() == b"--" {
            opts.push(a.clone());
            nonopts.extend(argv[i + 1..].iter().cloned());
            break;
        }
        if a.len() < 2 || a[0] != b'-' {
            nonopts.push(a.clone());
            i += 1;
            continue;
        }
        opts.push(a.clone());
        i += 1;
        if let Some(body) = a.strip_prefix(b"--") {
            if !body.contains(&b'=') && long_takes_arg(body) && i < argv.len() {
                opts.push(argv[i].clone());
                i += 1;
            }
        } else {
            let chars = &a[1..];
            for (k, c) in chars.iter().enumerate() {
                if SHORT_WITH_ARG.contains(c) {
                    if k + 1 == chars.len() && i < argv.len() {
                        opts.push(argv[i].clone());
                        i += 1;
                    }
                    break;
                }
            }
        }
    }
    opts.extend(nonopts);
    opts
}

/// Expande `@arquivo` como o `expandargv` do libiberty: o conteúdo (até o primeiro NUL) vira
/// argumentos no lugar do `@arquivo`, que são examinados de novo; arquivo que não abre fica como
/// está; diretório é erro fatal; mais de 2000 expansões também.
fn expand_response_files(prog: &str, mut argv: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>, i32> {
    let mut i = 1;
    let mut budget = 2000u32;
    while i < argv.len() {
        if argv[i].first() != Some(&b'@') {
            i += 1;
            continue;
        }
        budget -= 1;
        if budget == 0 {
            io::eprint(format!("{prog}: error: too many @-files encountered\n"));
            return Err(1);
        }
        let path = argv[i][1..].to_vec();
        let Ok(mut f) = File::open(&path) else {
            i += 1;
            continue;
        };
        if sys::stat(&path).is_ok_and(|st| st.file_type() == FileType::Directory) {
            io::eprint(format!("{prog}: error: @-file refers to a directory\n"));
            return Err(1);
        }
        let Ok(data) = f.read_to_end_sys() else {
            i += 1;
            continue;
        };
        let text = &data[..data.iter().position(|&b| b == 0).unwrap_or(data.len())];
        let new = build_argv(text);
        argv.splice(i..=i, new);
    }
    Ok(argv)
}

/// Divide o conteúdo de um `@arquivo` em argumentos: espaço em branco separa; aspas simples
/// protegem tudo; aspas duplas protegem tudo menos `\"` e `\\`; fora de aspas a barra invertida
/// protege o próximo caractere. Aspas sem fechamento vão até o fim.
fn build_argv(text: &[u8]) -> Vec<Vec<u8>> {
    let is_space = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r');
    let mut out = Vec::new();
    let mut i = 0;
    loop {
        while i < text.len() && is_space(text[i]) {
            i += 1;
        }
        if i >= text.len() {
            break;
        }
        let mut arg = Vec::new();
        let (mut squote, mut dquote) = (false, false);
        while i < text.len() {
            let c = text[i];
            if squote {
                if c == b'\'' {
                    squote = false;
                } else {
                    arg.push(c);
                }
            } else if dquote {
                if c == b'"' {
                    dquote = false;
                } else if c == b'\\' && matches!(text.get(i + 1), Some(b'"' | b'\\')) {
                    i += 1;
                    arg.push(text[i]);
                } else {
                    arg.push(c);
                }
            } else if c == b'\\' {
                i += 1;
                if let Some(&n) = text.get(i) {
                    arg.push(n);
                }
            } else if c == b'\'' {
                squote = true;
            } else if c == b'"' {
                dquote = true;
            } else if is_space(c) {
                break;
            } else {
                arg.push(c);
            }
            i += 1;
        }
        out.push(arg);
    }
    out
}

/// Processa um operando; `false` quando houve erro (o código de saída vira 1).
fn process_file(prog: &str, path: &[u8], cfg: &Config) -> bool {
    let msg = |parts: &[&[u8]]| {
        let mut m = format!("{prog}: ").into_bytes();
        for p in parts {
            m.extend_from_slice(p);
        }
        m.push(b'\n');
        io::eprint(m);
    };
    match sys::stat(path) {
        Err(Errno::ENOENT) => {
            msg(&[b"'", path, b"': No such file"]);
            return false;
        }
        Err(e) => {
            msg(&[b"Warning: could not locate '", path, b"'.  reason: ", e.message().as_bytes()]);
            return false;
        }
        Ok(st) if st.file_type() == FileType::Directory => {
            msg(&[b"Warning: '", path, b"' is a directory"]);
            return false;
        }
        Ok(_) => {}
    }
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) => {
            msg(&[path, b": ", e.message().as_bytes()]);
            return false;
        }
    };
    let mut out = Output::new(cfg);
    let use_sections = cfg.data_only
        && cfg.target.as_deref().is_none_or(|t| t != b"binary" && (t == b"default" || TARGETS.iter().any(|n| n.as_bytes() == t)));
    let plan = if use_sections { elf_plan(prog, path, file.fd()) } else { Plan::Whole };
    match plan {
        Plan::Whole => {
            let mut src = Source::fd(file.fd());
            scan(&mut src, 0, path, cfg, &mut out);
        }
        Plan::Sections(sections) => {
            for sec in sections {
                match sec {
                    SectionPlan::Scan { offset, data } => {
                        let mut src = Source::slice(&data);
                        scan(&mut src, offset, path, cfg, &mut out);
                    }
                    SectionPlan::TooLarge { name, size } => {
                        out.flush_partial();
                        let mut a = b"error: ".to_vec();
                        a.extend_from_slice(path);
                        a.push(b'(');
                        a.extend_from_slice(&name);
                        a.extend_from_slice(format!(") is too large ({size:#x} bytes)").as_bytes());
                        msg(&[a.as_slice()]);
                        msg(&[path, b": Reading section ", name.as_slice(), b" failed: file truncated"]);
                    }
                }
            }
        }
    }
    out.flush();
    true
}

// ---------------------------------------------------------------------------------------------
// ELF (`-d`)
// ---------------------------------------------------------------------------------------------

enum Plan {
    Whole,
    Sections(Vec<SectionPlan>),
}

enum SectionPlan {
    Scan { offset: u64, data: Vec<u8> },
    TooLarge { name: Vec<u8>, size: u64 },
}

const SHT_NULL: u32 = 0;
const SHT_NOBITS: u32 = 8;
const SHF_ALLOC: u64 = 2;

/// Lê até `len` bytes em `offset` (menos se o arquivo acabar).
fn pread_vec(fd: Fd, offset: u64, len: usize) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    buf.try_reserve(len).ok()?;
    buf.resize(len, 0);
    let sys = sys::current();
    let mut got = 0;
    while got < len {
        match sys.pread(fd, &mut buf[got..], offset + got as u64) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(Errno::EINTR) => {}
            Err(_) => return None,
        }
    }
    buf.truncate(got);
    Some(buf)
}

#[derive(Clone, Copy)]
struct Endian(bool);

impl Endian {
    fn u16(self, b: &[u8], at: usize) -> u64 {
        let v = [b[at], b[at + 1]];
        u64::from(if self.0 { u16::from_le_bytes(v) } else { u16::from_be_bytes(v) })
    }

    fn u32(self, b: &[u8], at: usize) -> u64 {
        let v = [b[at], b[at + 1], b[at + 2], b[at + 3]];
        u64::from(if self.0 { u32::from_le_bytes(v) } else { u32::from_be_bytes(v) })
    }

    fn u64(self, b: &[u8], at: usize) -> u64 {
        let mut v = [0u8; 8];
        v.copy_from_slice(&b[at..at + 8]);
        if self.0 { u64::from_le_bytes(v) } else { u64::from_be_bytes(v) }
    }
}

struct Shdr {
    name: u64,
    kind: u32,
    flags: u64,
    offset: u64,
    size: u64,
    link: u64,
}

fn parse_shdr(b: &[u8], e: Endian, is64: bool) -> Shdr {
    if is64 {
        Shdr {
            name: e.u32(b, 0),
            kind: e.u32(b, 4) as u32,
            flags: e.u64(b, 8),
            offset: e.u64(b, 24),
            size: e.u64(b, 32),
            link: e.u32(b, 40),
        }
    } else {
        Shdr {
            name: e.u32(b, 0),
            kind: e.u32(b, 4) as u32,
            flags: e.u32(b, 8),
            offset: e.u32(b, 16),
            size: e.u32(b, 20),
            link: e.u32(b, 24),
        }
    }
}

fn beyond(offset: u64, size: u64, file_size: u64) -> bool {
    offset > file_size || size > file_size - offset
}

/// Plano de varredura com `-d`: as seções alocadas com conteúdo de um ELF reconhecível, ou o
/// arquivo inteiro.
fn elf_plan(prog: &str, path: &[u8], fd: Fd) -> Plan {
    let warn = |what: &str| {
        let mut m = format!("{prog}: warning: ").into_bytes();
        m.extend_from_slice(path);
        m.extend_from_slice(what.as_bytes());
        m.push(b'\n');
        io::eprint(m);
    };
    let Ok(st) = sys::current().fstat(fd) else { return Plan::Whole };
    if st.file_type() != FileType::Regular {
        return Plan::Whole;
    }
    let file_size = st.size;
    let Some(h) = pread_vec(fd, 0, 64) else { return Plan::Whole };
    if h.len() < 16 || &h[..4] != b"\x7fELF" || h[6] != 1 {
        return Plan::Whole;
    }
    let is64 = match h[4] {
        1 => false,
        2 => true,
        _ => return Plan::Whole,
    };
    let e = match h[5] {
        1 => Endian(true),
        2 => Endian(false),
        _ => return Plan::Whole,
    };
    let ehsize = if is64 { 64 } else { 52 };
    if h.len() < ehsize {
        return Plan::Whole;
    }
    let e_type = e.u16(&h, 16);
    if e_type == 4 {
        return Plan::Whole;
    }
    let (shoff, shentsize, e_shnum, e_shstrndx) = if is64 {
        (e.u64(&h, 40), e.u16(&h, 58), e.u16(&h, 60), e.u16(&h, 62))
    } else {
        (e.u32(&h, 32), e.u16(&h, 46), e.u16(&h, 48), e.u16(&h, 50))
    };
    if shoff == 0 {
        return Plan::Whole;
    }
    let entsize: u64 = if is64 { 64 } else { 40 };
    if shentsize != entsize || beyond(shoff, entsize, file_size) {
        return Plan::Whole;
    }
    let Some(sh0) = pread_vec(fd, shoff, entsize as usize) else { return Plan::Whole };
    if sh0.len() < entsize as usize {
        return Plan::Whole;
    }
    let sh0 = parse_shdr(&sh0, e, is64);
    let shnum = if e_shnum == 0 { sh0.size } else { e_shnum };
    if shnum == 0 {
        return Plan::Whole;
    }
    let Some(table_len) = shnum.checked_mul(entsize) else { return Plan::Whole };
    if beyond(shoff, table_len, file_size) {
        return Plan::Whole;
    }
    let Some(table) = pread_vec(fd, shoff, table_len as usize) else { return Plan::Whole };
    if (table.len() as u64) < table_len {
        return Plan::Whole;
    }
    let shdrs: Vec<Shdr> = table.chunks(entsize as usize).map(|c| parse_shdr(c, e, is64)).collect();
    let shstrndx = if e_shstrndx == 0xffff { sh0.link } else { e_shstrndx };
    if shstrndx == 0 || shstrndx >= shnum {
        warn(" has a corrupt string table index");
        return Plan::Whole;
    }
    if shdrs.iter().skip(1).any(|s| s.kind != SHT_NOBITS && beyond(s.offset, s.size, file_size)) {
        warn(" has a section extending past end of file");
    }
    let strtab = {
        let s = &shdrs[shstrndx as usize];
        if s.kind != SHT_NOBITS && !beyond(s.offset, s.size, file_size) {
            pread_vec(fd, s.offset, s.size as usize).unwrap_or_default()
        } else {
            Vec::new()
        }
    };
    let name_of = |off: u64| -> Vec<u8> {
        let Ok(start) = usize::try_from(off) else { return Vec::new() };
        let tail = strtab.get(start..).unwrap_or(&[]);
        tail[..tail.iter().position(|&b| b == 0).unwrap_or(tail.len())].to_vec()
    };
    let mut plan = Vec::new();
    for s in shdrs.iter().skip(1) {
        if s.kind == SHT_NULL || s.kind == SHT_NOBITS || s.flags & SHF_ALLOC == 0 || s.size == 0 {
            continue;
        }
        if beyond(s.offset, s.size, file_size) {
            plan.push(SectionPlan::TooLarge { name: name_of(s.name), size: s.size });
            continue;
        }
        sys::checkpoint();
        match pread_vec(fd, s.offset, s.size as usize) {
            Some(data) => plan.push(SectionPlan::Scan { offset: s.offset, data }),
            None => plan.push(SectionPlan::TooLarge { name: name_of(s.name), size: s.size }),
        }
    }
    if plan.is_empty() { Plan::Whole } else { Plan::Sections(plan) }
}

// ---------------------------------------------------------------------------------------------
// Varredura
// ---------------------------------------------------------------------------------------------

/// Fonte de bytes: um fd lido em blocos ou um pedaço já em memória (seção de ELF).
enum Source<'a> {
    Fd { fd: Fd, buf: Vec<u8>, pos: usize, len: usize, done: bool },
    Slice { data: &'a [u8], pos: usize },
}

impl<'a> Source<'a> {
    fn fd(fd: Fd) -> Source<'static> {
        Source::Fd { fd, buf: vec![0; 64 * 1024], pos: 0, len: 0, done: false }
    }

    fn slice(data: &'a [u8]) -> Source<'a> {
        Source::Slice { data, pos: 0 }
    }

    fn next(&mut self) -> Option<u8> {
        match self {
            Source::Fd { fd, buf, pos, len, done } => {
                if *pos >= *len {
                    if *done {
                        return None;
                    }
                    sys::checkpoint();
                    loop {
                        match sys::read(*fd, buf) {
                            Ok(0) => {
                                *done = true;
                                return None;
                            }
                            Ok(n) => {
                                *len = n;
                                *pos = 0;
                                break;
                            }
                            Err(Errno::EINTR) => {}
                            Err(_) => {
                                *done = true;
                                return None;
                            }
                        }
                    }
                }
                let b = buf[*pos];
                *pos += 1;
                Some(b)
            }
            Source::Slice { data, pos } => {
                let b = *data.get(*pos)?;
                *pos += 1;
                if *pos % (64 * 1024) == 0 {
                    sys::checkpoint();
                }
                Some(b)
            }
        }
    }
}

/// Um caractere aceito numa cadeia.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Char {
    /// Byte (ou caractere de 16/32 bits já reduzido a byte).
    Byte(u8),
    /// Sequência UTF-8 de 2 a 4 bytes (modos `-U` que a aceitam).
    Utf8(Vec<u8>),
}

enum Next {
    Eof,
    Graphic(Char),
    /// Não gráfico; `true` quando um byte lido à frente foi devolvido ao fluxo (sequência UTF-8
    /// inválida), o que dispara o defeito de deslocamento do original.
    Stop(bool),
}

struct Scanner<'s, 'a> {
    src: &'s mut Source<'a>,
    pushback: Vec<u8>,
    address: u64,
}

impl Scanner<'_, '_> {
    fn byte(&mut self) -> Option<u8> {
        let b = match self.pushback.pop() {
            Some(b) => b,
            None => self.src.next()?,
        };
        self.address = self.address.wrapping_add(1);
        Some(b)
    }

    /// Devolve bytes ao fluxo (em ordem de leitura).
    fn unget(&mut self, bytes: &[u8]) {
        for &b in bytes.iter().rev() {
            self.pushback.push(b);
        }
        self.address = self.address.wrapping_sub(bytes.len() as u64);
    }

    fn next_char(&mut self, cfg: &Config) -> Next {
        let width = match cfg.encoding {
            b'b' | b'l' => 2,
            b'B' | b'L' => 4,
            _ => 1,
        };
        if width == 1 {
            let Some(b) = self.byte() else { return Next::Eof };
            if cfg.unicode != Unicode::Default && b >= 0x80 {
                return self.utf8(b, cfg);
            }
            return if is_graphic(u32::from(b), cfg) { Next::Graphic(Char::Byte(b)) } else { Next::Stop(false) };
        }
        let mut bytes = [0u8; 4];
        for slot in bytes.iter_mut().take(width) {
            match self.byte() {
                Some(b) => *slot = b,
                None => return Next::Eof,
            }
        }
        let value = match cfg.encoding {
            b'l' => u32::from(u16::from_le_bytes([bytes[0], bytes[1]])),
            b'b' => u32::from(u16::from_be_bytes([bytes[0], bytes[1]])),
            b'L' => u32::from_le_bytes(bytes),
            _ => u32::from_be_bytes(bytes),
        };
        if is_graphic(value, cfg) {
            Next::Graphic(Char::Byte(value as u8))
        } else {
            self.unget(&bytes[1..width]);
            Next::Stop(false)
        }
    }

    fn utf8(&mut self, lead: u8, cfg: &Config) -> Next {
        if lead < 0xc0 {
            return Next::Stop(false);
        }
        let need = if lead < 0xe0 {
            1
        } else if lead < 0xf0 {
            2
        } else {
            3
        };
        let mut seq = vec![lead];
        for _ in 0..need {
            match self.byte() {
                None => return Next::Stop(false),
                Some(c) if (0x80..0xc0).contains(&c) => seq.push(c),
                Some(c) => {
                    self.unget(&[c]);
                    return Next::Stop(true);
                }
            }
        }
        if cfg.unicode == Unicode::Invalid { Next::Stop(false) } else { Next::Graphic(Char::Utf8(seq)) }
    }
}

fn is_graphic(c: u32, cfg: &Config) -> bool {
    let Ok(b) = u8::try_from(c) else { return false };
    b == b'\t'
        || (0x20..=0x7e).contains(&b)
        || (cfg.encoding == b'S' && b > 127)
        || (cfg.whitespace && matches!(b, b'\n' | b'\x0b' | b'\x0c' | b'\r'))
}

/// Saída de uma varredura: acumula em blocos e entrega ao stdout do processo (que tem a
/// bufferização da glibc).
struct Output {
    pending: Vec<u8>,
    unicode: Unicode,
    tty: bool,
}

impl Output {
    fn new(cfg: &Config) -> Output {
        Output { pending: Vec::new(), unicode: cfg.unicode, tty: cfg.tty }
    }

    fn put(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() >= 64 * 1024 {
            self.flush_partial();
        }
    }

    fn put_char(&mut self, c: &Char) {
        match c {
            Char::Byte(b) => self.put(&[*b]),
            Char::Utf8(seq) => match self.unicode {
                Unicode::Locale => self.put(&seq[..1]),
                Unicode::Hex => {
                    let mut s = String::from("<0x");
                    for b in seq {
                        s.push_str(&format!("{b:02x}"));
                    }
                    s.push('>');
                    self.put(s.as_bytes());
                }
                Unicode::Highlight if self.tty => {
                    let s = format!("\x1b[31;47m{}\x1b[0m", escape_form(seq));
                    self.put(s.as_bytes());
                }
                _ => self.put(escape_form(seq).as_bytes()),
            },
        }
    }

    fn flush_partial(&mut self) {
        if !self.pending.is_empty() {
            let mut out = io::stdout();
            let _ = out.write_all(&self.pending);
            self.pending.clear();
        }
    }

    fn flush(&mut self) {
        self.flush_partial();
    }
}

/// `\uXXXX` com as contas do original: em quatro bytes os bits do líder vão pra posição 22 e os do
/// segundo byte pra 14, então o valor impresso não é o código Unicode.
fn escape_form(seq: &[u8]) -> String {
    let c = |i: usize| u32::from(seq[i] & 0x3f);
    match seq.len() {
        2 => format!("\\u{:04x}", (u32::from(seq[0] & 0x1f) << 6) | c(1)),
        3 => format!("\\u{:04x}", (u32::from(seq[0] & 0x0f) << 12) | (c(1) << 6) | c(2)),
        _ => format!("\\u{:06x}", (u32::from(seq[0] & 0x07) << 22) | (c(1) << 14) | (c(2) << 6) | c(3)),
    }
}

/// Varre um fluxo e imprime as cadeias. `base` é o deslocamento do primeiro byte no arquivo.
fn scan(src: &mut Source<'_>, base: u64, name: &[u8], cfg: &Config, out: &mut Output) {
    let mut sc = Scanner { src, pushback: Vec::new(), address: base };
    // Posição do byte devolvido ao fluxo depois de uma cadeia impressa (defeito do original).
    let mut flagged_at: Option<u64> = None;
    'tryline: loop {
        let start = sc.address;
        let flagged = flagged_at.take() == Some(start);
        let mut chars: Vec<Char> = Vec::new();
        while (chars.len() as u64) < cfg.min {
            match sc.next_char(cfg) {
                Next::Eof => return,
                Next::Graphic(c) => chars.push(c),
                Next::Stop(_) => continue 'tryline,
            }
        }
        if cfg.print_names {
            out.put(name);
            out.put(b": ");
        }
        if let Some(radix) = cfg.radix {
            let shown = if flagged { start.wrapping_add(1 << 32) } else { start };
            let text = match radix {
                b'o' => format!("{shown:7o} "),
                b'd' => format!("{shown:7} "),
                _ => format!("{shown:7x} "),
            };
            out.put(text.as_bytes());
        }
        for c in &chars {
            out.put_char(c);
        }
        drop(chars);
        loop {
            match sc.next_char(cfg) {
                Next::Eof => break,
                Next::Graphic(c) => out.put_char(&c),
                Next::Stop(put_back) => {
                    if put_back {
                        flagged_at = Some(sc.address);
                    }
                    break;
                }
            }
        }
        out.put(&cfg.separator);
    }
}

#[cfg(test)]
mod tests {
    //! Saídas esperadas capturadas do `strings` do binutils 2.44 no oráculo
    //! (`pseudo-linus-oracle:719900900623`, Debian 13), com `LC_ALL=C.UTF-8`.
    use super::*;
    use sysabi::Program;
    use sysabi::testkit::TestKit;

    const SAMPLE: &[u8] = b"hello world\0abc\0abcd\0\x01\x02longer string here\nsecond line\ttab\x7f\xffxyzw\0";

    fn kit() -> TestKit {
        TestKit::new().programs([Program::bin("strings", main)]).dir("/w", 0o755).cwd("/w")
    }

    fn run_with(files: &[(&str, &[u8])], args: &[&str], stdin: &[u8]) -> (Vec<u8>, String, i32) {
        let k = kit();
        for (name, data) in files {
            k.put_file(format!("/w/{name}").as_bytes(), data, 0o644);
        }
        let mut argv = vec!["strings"];
        argv.extend_from_slice(args);
        let r = k.run(&argv, stdin);
        let code = r.code();
        (r.stdout.clone(), r.stderr_str(), code)
    }

    fn out(files: &[(&str, &[u8])], args: &[&str]) -> String {
        String::from_utf8_lossy(&run_with(files, args, b"").0).into_owned()
    }

    #[test]
    fn default_scan() {
        assert_eq!(out(&[("a", SAMPLE)], &["a"]), "hello world\nabcd\nlonger string here\nsecond line\ttab\nxyzw\n");
    }

    #[test]
    fn min_length_options() {
        let want = "hello world\nabc\nabcd\nlonger string here\nsecond line\ttab\nxyzw\n";
        assert_eq!(out(&[("a", SAMPLE)], &["-n", "3", "a"]), want);
        assert_eq!(out(&[("a", SAMPLE)], &["-3", "a"]), want);
        assert_eq!(out(&[("a", SAMPLE)], &["--bytes=3", "a"]), want);
        assert_eq!(out(&[("a", SAMPLE)], &["-12", "a"]), "longer string here\nsecond line\ttab\n");
    }

    #[test]
    fn min_length_bases() {
        let f: &[u8] = b"12345678\0abcdefghi\0";
        assert_eq!(out(&[("n", f)], &["-n", "010", "n"]), "12345678\nabcdefghi\n");
        assert_eq!(out(&[("n", f)], &["-n", "0x9", "n"]), "abcdefghi\n");
        assert_eq!(out(&[("n", f)], &["-010", "n"]), "12345678\nabcdefghi\n");
        assert_eq!(out(&[("n", f)], &["-n", " 9", "n"]), "abcdefghi\n");
    }

    #[test]
    fn min_length_errors() {
        let cases: &[(&[&str], &str)] = &[
            (&["-n", "0", "a"], "strings: minimum string length is too small: 0\n"),
            (&["-n", "abc", "a"], "strings: invalid integer argument abc\n"),
            (&["-n", "-1", "a"], "strings: minimum string length is too big: -1\n"),
            (&["-n", "3x", "a"], "strings: invalid integer argument 3x\n"),
            (&["-n", "4294967296", "a"], "strings: minimum string length is too big: 4294967296\n"),
            (&["-n", "4294967295", "a"], "strings: minimum string length 4294967295 is too big\n"),
            (&["-0", "a"], "strings: minimum string length is too small: 0\n"),
            (&["-n", "", "a"], "strings: minimum string length is too small: \n"),
        ];
        for (args, want) in cases {
            let (o, e, c) = run_with(&[("a", SAMPLE)], args, b"");
            assert_eq!((o.as_slice(), e.as_str(), c), (&b""[..], *want, 1), "{args:?}");
        }
    }

    #[test]
    fn numeric_option_uses_permuted_argv() {
        // `strings a -5`: o original lê o elemento `a` do argv permutado (`a` sem o primeiro byte).
        let (_, e, c) = run_with(&[("a", SAMPLE)], &["a", "-5"], b"");
        assert_eq!((e.as_str(), c), ("strings: minimum string length is too small: \n", 1));
        let (_, e, _) = run_with(&[("a", SAMPLE)], &["a", "-5", "-n", "3"], b"");
        assert_eq!(e, "strings: invalid integer argument n\n");
        let (_, e, _) = run_with(&[("a", SAMPLE)], &["-13a", "a"], b"");
        assert_eq!(e, "strings: invalid integer argument trings\n");
        let (_, e, _) = run_with(&[("a", SAMPLE)], &["-a3", "a"], b"");
        assert_eq!(e, "strings: invalid integer argument a3\n");
        assert_eq!(out(&[("a", SAMPLE)], &["-6", "a", "-n", "3"]), "hello world\nlonger string here\nsecond line\ttab\n");
        assert_eq!(out(&[("a", SAMPLE)], &["-3", "-n5", "a"]), "hello world\nabc\nabcd\nlonger string here\nsecond line\ttab\nxyzw\n");
    }

    #[test]
    fn radix_formats() {
        assert_eq!(
            out(&[("a", SAMPLE)], &["-t", "x", "a"]),
            "      0 hello world\n     10 abcd\n     17 longer string here\n     2a second line\ttab\n     3b xyzw\n"
        );
        assert_eq!(
            out(&[("a", SAMPLE)], &["-o", "a"]),
            "      0 hello world\n     20 abcd\n     27 longer string here\n     52 second line\ttab\n     73 xyzw\n"
        );
        assert_eq!(out(&[("a", SAMPLE)], &["--radix=d", "-n", "15", "a"]), "     23 longer string here\n     42 second line\ttab\n");
        let mut big = vec![0u8; 10_000_000];
        big.extend_from_slice(b"found it");
        assert_eq!(out(&[("b", &big)], &["-t", "d", "b"]), "10000000 found it\n");
        assert_eq!(out(&[("b", &big)], &["-t", "o", "b"]), "46113200 found it\n");
    }

    #[test]
    fn radix_and_encoding_arg_errors_print_usage() {
        for args in [&["-t", "q", "a"][..], &["-t", "xx", "a"], &["-e", "q", "a"], &["-e", "ss", "a"]] {
            let (o, e, c) = run_with(&[("a", SAMPLE)], args, b"");
            assert!(o.is_empty());
            assert!(e.starts_with("Usage: strings [option(s)] [file(s)]\n"), "{args:?}");
            assert!(e.ends_with("binary ihex plugin\n"));
            assert_eq!(c, 1);
        }
    }

    #[test]
    fn print_file_names_and_separator() {
        assert_eq!(out(&[("a", SAMPLE)], &["-f", "-n", "11", "a"]), "a: hello world\na: longer string here\na: second line\ttab\n");
        assert_eq!(out(&[("a", SAMPLE)], &["-s", "|", "a"]), "hello world|abcd|longer string here|second line\ttab|xyzw|");
        assert_eq!(out(&[("a", SAMPLE)], &["-s", "", "-n", "11", "a"]), "hello worldlonger string heresecond line\ttab");
        assert_eq!(
            out(&[("a", SAMPLE)], &["-f", "-t", "d", "-s", "::", "-n", "11", "a"]),
            "a:       0 hello world::a:      23 longer string here::a:      42 second line\ttab::"
        );
    }

    #[test]
    fn stdin_and_dash() {
        let (o, _, c) = run_with(&[], &[], b"from stdin\0zz");
        assert_eq!((o.as_slice(), c), (&b"from stdin\n"[..], 0));
        let (o, _, _) = run_with(&[], &["-f", "-t", "o"], b"x\0yyyyyy\0");
        assert_eq!(o, b"{standard input}:       2 yyyyyy\n");
        let (o, e, c) = run_with(&[], &["-"], b"from stdin\0");
        assert!(o.is_empty());
        assert!(e.starts_with("Usage: strings"));
        assert_eq!(c, 1);
        assert_eq!(out(&[("a", SAMPLE)], &["-", "-n", "11", "a"]), "hello world\nlonger string here\nsecond line\ttab\n");
    }

    #[test]
    fn file_errors() {
        let (o, e, c) = run_with(&[("a", b"hello\0")], &["nonexist", "a"], b"");
        assert_eq!((o.as_slice(), e.as_str(), c), (&b"hello\n"[..], "strings: 'nonexist': No such file\n", 1));
        let k = kit();
        k.put_dir(b"/w/d", 0o755);
        k.put_file(b"/w/a", b"hello\0", 0o644);
        let r = k.run(&["strings", "d", "a/b", "a"], b"");
        assert_eq!(
            r.stderr_str(),
            "strings: Warning: 'd' is a directory\nstrings: Warning: could not locate 'a/b'.  reason: Not a directory\n"
        );
        assert_eq!((r.stdout_str().as_str(), r.code()), ("hello\n", 1));
        let (o, e, c) = run_with(&[("empty", b"")], &["empty"], b"");
        assert_eq!((o.len(), e.as_str(), c), (0, "", 0));
    }

    #[test]
    fn options_errors_and_help() {
        let (o, e, c) = run_with(&[], &["-Q"], b"");
        assert!(o.is_empty());
        assert!(e.starts_with("strings: invalid option -- 'Q'\nUsage: strings [option(s)] [file(s)]\n"));
        assert!(!e.contains("Report bugs"));
        assert_eq!(c, 1);
        let (_, e, _) = run_with(&[], &["--foo"], b"");
        assert!(e.starts_with("strings: unrecognized option '--foo'\nUsage:"));
        let (_, e, _) = run_with(&[], &["--all=x"], b"");
        assert!(e.starts_with("strings: option '--all' doesn't allow an argument\n"));
        let (o, e, c) = run_with(&[], &["-H", "-Q"], b"");
        let o = String::from_utf8(o).unwrap();
        assert!(o.starts_with("Usage: strings [option(s)] [file(s)]\n Display printable strings"));
        assert!(o.ends_with("binary ihex plugin\nReport bugs to <https://sourceware.org/bugzilla/>\n"));
        assert_eq!((e.as_str(), c), ("", 0));
    }

    #[test]
    fn version() {
        let (o, _, c) = run_with(&[], &["-v", "-Q"], b"");
        assert_eq!(o, VERSION_TEXT.as_bytes());
        assert_eq!(c, 0);
        let (o, _, _) = run_with(&[], &["--version", "nonexist"], b"");
        assert!(o.starts_with(b"GNU strings (GNU Binutils for Debian) 2.44\n"));
    }

    #[test]
    fn option_errors_are_sequential() {
        let (_, e, _) = run_with(&[("m", b"abc\0")], &["-n", "0", "-Q", "m"], b"");
        assert_eq!(e, "strings: minimum string length is too small: 0\n");
        let (_, e, _) = run_with(&[("m", b"abc\0")], &["-U", "q", "-n", "0", "m"], b"");
        assert_eq!(e, "strings: invalid argument to -U/--unicode: q\n");
        let (o, _, c) = run_with(&[("m", b"abc\0")], &["-v", "-n", "0"], b"");
        assert!(o.starts_with(b"GNU strings"));
        assert_eq!(c, 0);
    }

    #[test]
    fn eight_bit_and_whitespace() {
        let (o, _, _) = run_with(&[("a", SAMPLE)], &["-e", "S", "a"], b"");
        assert_eq!(o, b"hello world\nabcd\nlonger string here\nsecond line\ttab\n\xffxyzw\n");
        let (o, _, _) = run_with(&[("a", SAMPLE)], &["-w", "a"], b"");
        assert_eq!(o, b"hello world\nabcd\nlonger string here\nsecond line\ttab\nxyzw\n");
        let (o, _, _) = run_with(&[], &["-w", "-s", "|"], b"ab\ncd\0");
        assert_eq!(o, b"ab\ncd|");
        let (o, _, _) = run_with(&[], &["-e", "S", "-w"], b"a\xe9\nb\xa0c\0");
        assert_eq!(o, b"a\xe9\nb\xa0c\n");
    }

    #[test]
    fn sixteen_bit_resync() {
        assert_eq!(out(&[("w", b"h\0e\0l\0l\0o\0\0\0\xe9\0t\0o\0o\0o\0\0\0x\0")], &["-e", "l", "-t", "x", "w"]), "      0 hello\n      e tooo\n");
        assert_eq!(out(&[("w", b"\0h\0e\0l\0l\0o\0\0")], &["-e", "l", "w"]), "hello\n");
        assert_eq!(out(&[("w", b"\0h\0e\0l\0l\0o\0\0")], &["-e", "b", "w"]), "hello\n");
        assert_eq!(out(&[("w", b"a\0b\0\x01X\0c\0d\0e\0f\0\0")], &["-e", "l", "-t", "d", "w"]), "      5 Xcdef\n");
        assert_eq!(out(&[("w", b"a\0b\0c\0d\0e")], &["-e", "l", "w"]), "abcd\n");
        assert_eq!(out(&[("w", b"a\0\n\0b\0\r\0c\0\0\0")], &["-e", "l", "-w", "w"]), "a\nb\rc\n");
    }

    #[test]
    fn thirty_two_bit() {
        let d32: &[u8] = b"h\0\0\0e\0\0\0l\0\0\0l\0\0\0o\0\0\0\0\0\0\0";
        assert_eq!(out(&[("d", d32)], &["-e", "L", "-t", "x", "d"]), "      0 hello\n");
        assert_eq!(out(&[("d", d32)], &["-e", "B", "d"]), "ello\n");
        let x: &[u8] = b"a\0\0\0b\0\0\0\0\x01\0\0c\0\0\0d\0\0\0e\0\0\0f\0\0\0";
        assert_eq!(out(&[("d", x)], &["-e", "L", "-t", "d", "d"]), "     12 cdef\n");
        let y: &[u8] = b"a\0\0\0b\0\0\0\x01X\0\0c\0\0\0d\0\0\0e\0\0\0f\0\0\0\0\0\0\0";
        assert_eq!(out(&[("d", y)], &["-e", "L", "-t", "d", "d"]), "     12 cdef\n");
    }

    const UTF: &[u8] = b"caf\xc3\xa9 au lait\0euro \xe2\x82\xac sign\0smile \xf0\x9f\x98\x80 face\0bad \xc3 byte here\0\xc3\xa9\xc3\xa9\0\xc3\xa9\xc3\xa9\xc3\xa9\xc3\xa9\0";

    #[test]
    fn unicode_escape_and_hex() {
        assert_eq!(
            out(&[("u", UTF)], &["-U", "e", "u"]),
            "caf\\u00e9 au lait\neuro \\u20ac sign\nsmile \\u07c600 face\nbad \n byte here\n\\u00e9\\u00e9\\u00e9\\u00e9\n"
        );
        assert_eq!(
            out(&[("u", UTF)], &["--unicode=hex", "-n", "5", "u"]),
            "caf<0xc3a9> au lait\neuro <0xe282ac> sign\nsmile <0xf09f9880> face\n byte here\n"
        );
        assert_eq!(out(&[("u", UTF)], &["-U", "e", "-n", "2", "u"]).lines().nth(5), Some("\\u00e9\\u00e9"));
        assert_eq!(out(&[("u", b"aaaa\xf7\xbf\xbf\xbfbbbb\0aaaa\xf1\x80\x80\x81bbbb\0")], &["-Ue", "u"]), "aaaa\\u1cfcfffbbbb\naaaa\\u400001bbbb\n");
    }

    #[test]
    fn unicode_locale_invalid_highlight() {
        let (o, _, _) = run_with(&[("u", UTF)], &["-U", "l", "u"], b"");
        assert_eq!(o, b"caf\xc3 au lait\neuro \xe2 sign\nsmile \xf0 face\nbad \n byte here\n\xc3\xc3\xc3\xc3\n");
        assert_eq!(out(&[("u", UTF)], &["-U", "i", "u"]), " au lait\neuro \n sign\nsmile \n face\nbad \n byte here\n");
        assert_eq!(out(&[("u", b"caf\xc3\xa9 ok\0")], &["-U", "h", "u"]), "caf\\u00e9 ok\n");
        assert_eq!(out(&[("u", b"caf\xc3\xa9 ok\0")], &["-U", "default", "-e", "S", "u"]), "caf\u{e9} ok\n");
    }

    #[test]
    fn unicode_forces_eight_bit() {
        let f: &[u8] = b"caf\xc3\xa9\xc3\xa9 x\0";
        assert_eq!(out(&[("u", f)], &["-e", "l", "-U", "e", "u"]), "caf\\u00e9\\u00e9 x\n");
        assert_eq!(out(&[("u", f)], &["-U", "e", "-e", "l", "u"]), "caf\\u00e9\\u00e9 x\n");
    }

    #[test]
    fn unicode_offset_defect() {
        let o = |data: &[u8], args: &[&str]| {
            let mut a = args.to_vec();
            a.extend_from_slice(&["-t", "d", "t"]);
            out(&[("t", data)], &a).replace('\n', "|")
        };
        assert_eq!(o(b"AAAA\xc3BBBB\0", &["-U", "x"]), "      0 AAAA|4294967301 BBBB|");
        assert_eq!(o(b"AA\xc3BBBB\0", &["-U", "x"]), "      3 BBBB|");
        assert_eq!(o(b"AAAA\x01\xc3BBBB\0", &["-U", "x"]), "      0 AAAA|      6 BBBB|");
        assert_eq!(o(b"AAAA\xc3\xc3BBBB\0", &["-U", "x"]), "      0 AAAA|      6 BBBB|");
        assert_eq!(o(b"AAAA\xc3\xc3\xa9BBBB\0", &["-U", "x"]), "      0 AAAA|4294967301 <0xc3a9>BBBB|");
        assert_eq!(
            o(b"ABCD\xc3EFGH\xc3IJKL\xc3MNOP\0", &["-U", "x"]),
            "      0 ABCD|4294967301 EFGH|4294967306 IJKL|4294967311 MNOP|"
        );
        assert_eq!(o(b"ABCD\xe2\x82EFGH\0", &["-U", "x"]), "      0 ABCD|4294967302 EFGH|");
        assert_eq!(o(b"ABCD\xe2\x82\x01EFGH\0", &["-U", "x"]), "      0 ABCD|      7 EFGH|");
        assert_eq!(o(b"ABCD\xc3E\x01FGHI\0", &["-U", "x"]), "      0 ABCD|      7 FGHI|");
        assert_eq!(o(b"ABCD\xc3EFGH\0", &["-U", "i"]), "      0 ABCD|4294967301 EFGH|");
        assert_eq!(o(b"ABCD\xc3EFGH\0", &["-U", "d"]), "      0 ABCD|      5 EFGH|");
        assert_eq!(o(b"ABCD\xc3\nEFGH\0", &["-U", "x", "-w"]), "      0 ABCD|4294967301 |EFGH|");
    }

    #[test]
    fn unicode_bad_argument() {
        for bad in ["q", "esc", "E", "HEX", ""] {
            let (_, e, c) = run_with(&[("u", UTF)], &["-U", bad, "u"], b"");
            assert_eq!((e, c), (format!("strings: invalid argument to -U/--unicode: {bad}\n"), 1));
        }
    }

    #[test]
    fn response_files() {
        let k = kit();
        k.put_file(b"/w/a", b"abc\0hello\0", 0o644);
        k.put_file(b"/w/opts", b"-n 3\n-f a", 0o644);
        k.put_file(b"/w/q1", b"-s 'a\\'b' a", 0o644);
        k.put_file(b"/w/q2", b"-s \"a\\\"b\" a", 0o644);
        k.put_file(b"/w/q3", b"-s \"x\\y\" a", 0o644);
        k.put_file(b"/w/q4", b"-s a\\ b a", 0o644);
        k.put_file(b"/w/loop", b"@loop", 0o644);
        k.put_dir(b"/w/d", 0o755);
        assert_eq!(k.run(&["strings", "@opts"], b"").stdout_str(), "a: abc\na: hello\n");
        assert_eq!(k.run(&["strings", "@q1"], b"").stdout_str(), "");
        assert_eq!(k.run(&["strings", "@q2"], b"").stdout_str(), "helloa\"b");
        assert_eq!(k.run(&["strings", "@q3"], b"").stdout_str(), "hellox\\y");
        assert_eq!(k.run(&["strings", "@q4"], b"").stdout_str(), "helloa b");
        let r = k.run(&["strings", "@loop"], b"");
        assert_eq!((r.stderr_str().as_str(), r.code()), ("strings: error: too many @-files encountered\n", 1));
        let r = k.run(&["strings", "@d"], b"");
        assert_eq!((r.stderr_str().as_str(), r.code()), ("strings: error: @-file refers to a directory\n", 1));
        let r = k.run(&["strings", "@nonexist"], b"");
        assert_eq!(r.stderr_str(), "strings: '@nonexist': No such file\n");
        let r = k.run(&["strings", "--", "@opts"], b"");
        assert_eq!(r.stderr_str(), "strings: '-n': No such file\nstrings: '3': No such file\nstrings: '-f': No such file\n");
    }

    #[test]
    fn build_argv_quotes() {
        let v = |s: &[u8]| build_argv(s);
        assert_eq!(v(b"  a  b\n"), vec![b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(v(b"''"), vec![Vec::<u8>::new()]);
        assert_eq!(v(b"a''b"), vec![b"ab".to_vec()]);
        assert_eq!(v(b"a\"b c\"d"), vec![b"ab cd".to_vec()]);
        assert_eq!(v(b"\"a\\\\b\""), vec![b"a\\b".to_vec()]);
        assert_eq!(v(b"a\\"), vec![b"a".to_vec()]);
        assert_eq!(v(b"-s \"unterminated a"), vec![b"-s".to_vec(), b"unterminated a".to_vec()]);
    }

    #[test]
    fn strtoul_and_permutation_helpers() {
        assert_eq!(strtoul(b"0x10"), (16, 4));
        assert_eq!(strtoul(b"0x"), (0, 1));
        assert_eq!(strtoul(b"010"), (8, 3));
        assert_eq!(strtoul(b"-1"), (u64::MAX, 2));
        assert_eq!(strtoul(b"99999999999999999999999"), (u64::MAX, 23));
        assert_eq!(strtoul(b" +"), (0, 0));
        let a = |v: &[&str]| v.iter().map(|s| s.as_bytes().to_vec()).collect::<Vec<_>>();
        assert_eq!(permuted_argv(&a(&["strings", "a", "-n", "3", "-5", "b"]), false), a(&["strings", "-n", "3", "-5", "a", "b"]));
        assert_eq!(permuted_argv(&a(&["strings", "a", "-s,", "--radix", "x", "--", "-5"]), false), a(&["strings", "-s,", "--radix", "x", "--", "a", "-5"]));
        assert_eq!(permuted_argv(&a(&["strings", "a", "-5"]), true), a(&["strings", "a", "-5"]));
    }

    /// `t64.o` gerado no oráculo com `as --64` (seções `.text`, `.data`, `.bss`, `.rodata`,
    /// `.comment`, `.mynote` alocada, `.myw`/`.myw2` contíguas e `.nonalloc`).
    const T64_O: &str = "f0VMRgIBAQAAAAAAAAAAAAEAPgABAAAAAAAAAAAAAAAAAAAAAAAAAHABAAAAAAAAAAAAAEAAAAAAAEAADQAMAHRleHQgc2VjdGlvbiBzdHJpbmcAkGRhdGEgc2VjdGlvbiBzdHJpbmcAcm9kYXRhIHNlY3Rpb24gc3RyaW5nAGNvbW1lbnQgc3RyaW5nIGhlcmUAYWxsb2Mgbm90ZSBzdHJpbmcAd3JpdGFibGUgc3RyMWNvbnRpbnVlcyBoZXJlAG5vbmFsbG9jIHN0cmluZwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAQAAABAAAQAAAAAAAAAAAAAAAAAAAAAAAF9zdGFydAAALnN5bXRhYgAuc3RydGFiAC5zaHN0cnRhYgAudGV4dAAuZGF0YQAuYnNzAC5yb2RhdGEALmNvbW1lbnQALm15bm90ZQAubXl3AC5teXcyAC5ub25hbGxvYwAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAGwAAAAEAAAAGAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAVAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAACEAAAABAAAAAwAAAAAAAAAAAAAAAAAAAFUAAAAAAAAAFAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAnAAAACAAAAAMAAAAAAAAAAAAAAAAAAABpAAAAAAAAAEAAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAALAAAAAEAAAACAAAAAAAAAAAAAAAAAAAAaQAAAAAAAAAWAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAADQAAAABAAAAAAAAAAAAAAAAAAAAAAAAAH8AAAAAAAAAFAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAA9AAAABwAAAAIAAAAAAAAAAAAAAAAAAACTAAAAAAAAABIAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAARQAAAAEAAAADAAAAAAAAAAAAAAAAAAAApQAAAAAAAAANAAAAAAAAAAAAAAAAAAAAAQAAAAAAAAAAAAAAAAAAAEoAAAABAAAAAwAAAAAAAAAAAAAAAAAAALIAAAAAAAAADwAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAABQAAAAAQAAAAAAAAAAAAAAAAAAAAAAAADBAAAAAAAAABAAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAAAQAAAAIAAAAAAAAAAAAAAAAAAAAAAAAA2AAAAAAAAAAwAAAAAAAAAAsAAAABAAAACAAAAAAAAAAYAAAAAAAAAAkAAAADAAAAAAAAAAAAAAAAAAAAAAAAAAgBAAAAAAAACAAAAAAAAAAAAAAAAAAAAAEAAAAAAAAAAAAAAAAAAAARAAAAAwAAAAAAAAAAAAAAAAAAAAAAAAAQAQAAAAAAAFoAAAAAAAAAAAAAAAAAAAABAAAAAAAAAAAAAAAAAAAA";

    fn b64(s: &str) -> Vec<u8> {
        let mut table = [255u8; 256];
        for (i, c) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/".iter().enumerate() {
            table[*c as usize] = i as u8;
        }
        let mut out = Vec::new();
        let mut acc = 0u32;
        let mut bits = 0;
        for &c in s.as_bytes() {
            if c == b'=' {
                break;
            }
            acc = (acc << 6) | u32::from(table[c as usize]);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((acc >> bits) as u8);
            }
        }
        out
    }

    #[test]
    fn data_sections_of_elf() {
        let obj = b64(T64_O);
        assert_eq!(
            out(&[("t.o", &obj)], &["-d", "-t", "x", "t.o"]),
            "     40 text section string\n     55 data section string\n     69 rodata section string\n     93 alloc note string\n     a5 writable str1\n     b2 continues here\n"
        );
        let all = out(&[("t.o", &obj)], &["-t", "x", "t.o"]);
        assert!(all.contains("     7f comment string here\n     93 alloc note string\n     a5 writable str1continues here\n"));
        // `-` volta a varrer tudo nos arquivos seguintes; alvo `binary` e alvo inválido também.
        assert_eq!(out(&[("t.o", &obj)], &["-d", "-", "t.o"]), all.lines().map(|l| format!("{}\n", &l[8..])).collect::<String>());
        assert_eq!(out(&[("t.o", &obj)], &["-d", "-T", "binary", "t.o"]), out(&[("t.o", &obj)], &["t.o"]));
        assert_eq!(out(&[("t.o", &obj)], &["-d", "-T", "bogus", "t.o"]), out(&[("t.o", &obj)], &["t.o"]));
        assert_eq!(out(&[("t.o", &obj)], &["-d", "-T", "elf64-big", "-n", "16", "t.o"]), "text section string\ndata section string\nrodata section string\nalloc note string\n");
    }

    #[test]
    fn data_sections_fall_back_on_non_elf() {
        assert_eq!(out(&[("a", SAMPLE)], &["-d", "a"]), out(&[("a", SAMPLE)], &["a"]));
        let (o, _, _) = run_with(&[], &["-d", "-t", "x"], b"abcd\0");
        assert_eq!(o, b"      0 abcd\n");
    }
}
