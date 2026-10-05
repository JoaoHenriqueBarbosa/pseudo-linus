//! `logger` do util-linux 2.41 (pacote bsdutils do Debian 13): escreve mensagens no log do sistema.
//!
//! Porte do `misc-utils/logger.c` sem socket: o sysabi não tem `socket`/`connect`, então a entrega
//! ao `/dev/log` é omitida em silêncio, o mesmo que o original faz quando o socket não existe sem
//! `--socket-errors=on`. O resto é fiel: opções, `--help`, `--version`, validação de prioridade
//! (`facility.level`, por nome ou número), leitura de arquivo e stdin, e o eco de `-s`/`--stderr`
//! no formato `<tag>[<pid>]: <mensagem>`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const OPT_ID: i32 = 256;
const OPT_NO_ACT: i32 = 257;
const OPT_OCTET: i32 = 258;
const OPT_PRIO_PREFIX: i32 = 259;
const OPT_SOCKET_ERRORS: i32 = 260;
const OPT_RFC3164: i32 = 261;
const OPT_RFC5424: i32 = 262;
const OPT_SD_ID: i32 = 263;
const OPT_SD_PARAM: i32 = 264;
const OPT_MSGID: i32 = 265;
const OPT_JOURNALD: i32 = 266;

const LONGS: &[LongOpt] = &[
    LongOpt::new("id", HasArg::Optional, OPT_ID),
    LongOpt::new("file", HasArg::Required, b'f' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("octet-count", HasArg::No, OPT_OCTET),
    LongOpt::new("prio-prefix", HasArg::No, OPT_PRIO_PREFIX),
    LongOpt::new("priority", HasArg::Required, b'p' as i32),
    LongOpt::new("size", HasArg::Required, b'S' as i32),
    LongOpt::new("skip-empty", HasArg::No, b'e' as i32),
    LongOpt::new("socket-errors", HasArg::Required, OPT_SOCKET_ERRORS),
    LongOpt::new("stderr", HasArg::No, b's' as i32),
    LongOpt::new("tag", HasArg::Required, b't' as i32),
    LongOpt::new("socket", HasArg::Required, b'u' as i32),
    LongOpt::new("udp", HasArg::No, b'd' as i32),
    LongOpt::new("tcp", HasArg::No, b'T' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("rfc3164", HasArg::No, OPT_RFC3164),
    LongOpt::new("rfc5424", HasArg::Optional, OPT_RFC5424),
    LongOpt::new("sd-id", HasArg::Required, OPT_SD_ID),
    LongOpt::new("sd-param", HasArg::Required, OPT_SD_PARAM),
    LongOpt::new("msgid", HasArg::Required, OPT_MSGID),
    LongOpt::new("no-act", HasArg::No, OPT_NO_ACT),
    LongOpt::new("journald", HasArg::Optional, OPT_JOURNALD),
    LongOpt::new("server", HasArg::Required, b'n' as i32),
    LongOpt::new("port", HasArg::Required, b'P' as i32),
];

const FACILITIES: &[(&str, u32)] = &[
    ("auth", 4),
    ("authpriv", 10),
    ("cron", 9),
    ("daemon", 3),
    ("ftp", 11),
    ("kern", 0),
    ("lpr", 6),
    ("mail", 2),
    ("news", 7),
    ("security", 4),
    ("syslog", 5),
    ("user", 1),
    ("uucp", 8),
    ("local0", 16),
    ("local1", 17),
    ("local2", 18),
    ("local3", 19),
    ("local4", 20),
    ("local5", 21),
    ("local6", 22),
    ("local7", 23),
];

const LEVELS: &[(&str, u32)] = &[
    ("alert", 1),
    ("crit", 2),
    ("debug", 7),
    ("emerg", 0),
    ("err", 3),
    ("error", 3),
    ("info", 6),
    ("notice", 5),
    ("panic", 0),
    ("warn", 4),
    ("warning", 4),
];

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [options] [<message>]

Enter messages into the system log.

Options:
 -i                       log the logger command's PID
     --id[=<id>]          log the given <id>, or otherwise the PID
 -f, --file <file>        log the contents of this file
 -e, --skip-empty         do not log empty lines when processing files
     --no-act             do everything except the write the log
 -p, --priority <prio>    mark given message with this priority
     --octet-count        use rfc6587 octet counting
     --prio-prefix        look for a prefix on every line read from stdin
 -s, --stderr             output message to standard error as well
 -S, --size <size>        maximum size for a single message
     --socket-errors <on|off|auto>
                          print connection errors when using Unix sockets
 -T, --tcp                use TCP only
 -d, --udp                use UDP only
     --rfc3164            use the obsolete BSD syslog protocol
     --rfc5424[=<snip>]   use the syslog protocol (the default for remote);
                            <snip> can be notime, or notq, and/or nohost
     --sd-id <id>         rfc5424 structured data ID
     --sd-param <data>    rfc5424 structured data name=value
     --msgid <msgid>      set rfc5424 message id field
 -t, --tag <tag>          mark every line with this tag
 -n, --server <name>      write to this remote syslog server
 -P, --port <port>        use this port for UDP or TCP connection
 -u, --socket <socket>    write to this Unix socket
     --journald[=<file>]  write journald entry

 -h, --help               display this help
 -V, --version            display version

For more details see logger(1).
"
    )
}

/// `decode()` do logger.c: um nome da tabela, ou um número (`strtoul`) quando começa com dígito.
fn decode(name: &str, table: &[(&str, u32)]) -> Option<u32> {
    if name.as_bytes().first().is_some_and(|b| b.is_ascii_digit()) {
        let end = name
            .bytes()
            .position(|b| !b.is_ascii_digit())
            .unwrap_or(name.len());
        if end != name.len() {
            return None;
        }
        return name.parse::<u32>().ok();
    }
    table
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| *v)
}

/// `pencode()`: `facility.level`, `level` sozinho (facility `user`) ou `facility.` com erro.
/// Devolve o valor de `pri` (facility << 3 | level) ou o texto fatal do `errx`.
fn pencode(s: &str) -> Result<u32, String> {
    let (fac, lev) = match s.split_once('.') {
        Some((f, l)) => (Some(f), l),
        None => (None, s),
    };
    let mut fac_val = 1;
    if let Some(f) = fac {
        let v = decode(f, FACILITIES).ok_or_else(|| format!("unknown facility name: {f}"))?;
        if v >= 24 {
            return Err(format!("unknown facility name: {f}"));
        }
        fac_val = v;
    }
    if lev.is_empty() {
        return Ok(fac_val << 3);
    }
    let l = decode(lev, LEVELS).ok_or_else(|| format!("unknown priority name: {lev}"))?;
    if l >= 8 {
        return Err(format!("unknown priority name: {lev}"));
    }
    Ok((fac_val << 3) | l)
}

struct Opts {
    tag: String,
    pid: Option<String>,
    stderr_echo: bool,
    skip_empty: bool,
    prio_prefix: bool,
    max_size: usize,
}

/// Uma linha lida de arquivo/stdin ou a mensagem da linha de comando: só o eco de `-s`, pois não há
/// socket pra onde entregar.
fn emit(o: &Opts, msg: &[u8]) {
    if o.stderr_echo {
        let mut line = o.tag.as_bytes().to_vec();
        if let Some(p) = &o.pid {
            line.push(b'[');
            line.extend_from_slice(p.as_bytes());
            line.push(b']');
        }
        line.extend_from_slice(b": ");
        let m = if msg.len() > o.max_size {
            &msg[..o.max_size]
        } else {
            msg
        };
        line.extend_from_slice(m);
        line.push(b'\n');
        io::eprint(line);
    }
}

fn process_lines(o: &Opts, data: &[u8]) {
    let mut rest = data;
    while !rest.is_empty() {
        let (line, next) = match rest.iter().position(|b| *b == b'\n') {
            Some(i) => (&rest[..i], &rest[i + 1..]),
            None => (rest, &rest[rest.len()..]),
        };
        rest = next;
        if o.skip_empty && line.is_empty() {
            continue;
        }
        let mut l = line;
        if o.prio_prefix && l.first() == Some(&b'<') {
            if let Some(end) = l.iter().position(|b| *b == b'>') {
                if end > 1 && l[1..end].iter().all(u8::is_ascii_digit) {
                    l = &l[end + 1..];
                }
            }
        }
        emit(o, l);
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);
    let sys = sys::current();

    let mut o = Opts {
        tag: String::new(),
        pid: None,
        stderr_echo: false,
        skip_empty: false,
        prio_prefix: false,
        max_size: 1024,
    };
    let mut file: Option<Vec<u8>> = None;
    let mut have_tag = false;

    let mut g = Getopt::from_env(&argv[1..], "+f:ehin:p:sS:t:u:dTP:V", LONGS);
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(x) => x,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        match opt.id {
            x if x == b'f' as i32 => file = opt.arg.clone(),
            x if x == b'e' as i32 => o.skip_empty = true,
            x if x == b'i' as i32 || x == OPT_ID => {
                o.pid = Some(match opt.arg_str() {
                    s if opt.arg.is_some() => s,
                    _ => sys.getpid().to_string(),
                });
            }
            x if x == b'p' as i32 => {
                if let Err(m) = pencode(&opt.arg_str()) {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
            x if x == b's' as i32 => o.stderr_echo = true,
            x if x == b'S' as i32 => match ul::strtosize_or_err(
                opt.arg.as_deref().unwrap_or_default(),
                "failed to parse message size",
            ) {
                Ok(n) => {
                    if n < 16 {
                        ul::warnx(&short, "minimum message size is 16");
                        return 1;
                    }
                    o.max_size = n as usize;
                }
                Err(m) => {
                    ul::warnx(&short, m);
                    return 1;
                }
            },
            x if x == b't' as i32 => {
                o.tag = opt.arg_str();
                have_tag = true;
            }
            x if x == OPT_PRIO_PREFIX => o.prio_prefix = true,
            x if x == OPT_SOCKET_ERRORS => {
                let a = opt.arg_str();
                if !matches!(a.as_str(), "on" | "off" | "auto") {
                    ul::warnx(&short, format!("invalid socket-errors argument: {a}"));
                    return 1;
                }
            }
            x if x == b'h' as i32 => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
                return 0;
            }
            x if x == b'V' as i32 => {
                ul::print_version(&short);
                return 0;
            }
            x if x == b'u' as i32
                || x == b'd' as i32
                || x == b'T' as i32
                || x == b'n' as i32
                || x == b'P' as i32
                || x == OPT_NO_ACT
                || x == OPT_OCTET
                || x == OPT_RFC3164
                || x == OPT_RFC5424
                || x == OPT_SD_ID
                || x == OPT_SD_PARAM
                || x == OPT_MSGID
                || x == OPT_JOURNALD => {}
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    if !have_tag {
        o.tag = if sys.geteuid() == 0 {
            "root".to_string()
        } else {
            "user".to_string()
        };
    }

    let operands = g.operands();
    if !operands.is_empty() {
        let msg: Vec<u8> = operands.join(&b' ');
        emit(&o, &msg);
    } else if let Some(path) = file {
        match io::read_path(&path) {
            Ok(d) => process_lines(&o, &d),
            Err(e) => {
                ul::warn(&short, format!("file {}", io::lossy(&path)), e);
                return 1;
            }
        }
    } else {
        match io::read_stdin() {
            Ok(d) => process_lines(&o, &d),
            Err(_) => return 1,
        }
    }
    0
}
