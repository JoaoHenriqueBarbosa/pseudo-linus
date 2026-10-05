//! `gettext` e `ngettext` do gettext-runtime 0.23.1 (Debian 13), portados de
//! `gettext-runtime/src/gettext.c`, `ngettext.c` e `escapes.h`.
//!
//! O sandbox não tem catálogos de mensagens (`.mo`), então `dgettext`/`dngettext` nunca acham tradução
//! e devolvem o próprio texto: o singular quando `n == 1` e o plural nos outros casos (a regra inglesa
//! que a libintl usa sem catálogo). Todo o resto é do programa de verdade: as opções (`-d`, `-c`, `-e`,
//! `-E`, `-n`, `-s`, `-h`, `-V` e as longas), a expansão de escapes do `-e` (com o `\c` do
//! `gettext -s`), o corte na primeira NUL (o `fputs` do C), a leitura do COUNT pelo `strtoul` e as
//! mensagens de erro e de uso. `TEXTDOMAIN`, `TEXTDOMAINDIR` e `-d` só escolhem um domínio sem
//! catálogo, então não mudam a saída.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

/// Diretório padrão de catálogos que o `--help` imprime.
const LOCALEDIR: &str = "/usr/share/locale";

const GETTEXT_LONGOPTS: &[LongOpt] = &[
    LongOpt::new("context", HasArg::Required, 'c' as i32),
    LongOpt::new("domain", HasArg::Required, 'd' as i32),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("shell-script", HasArg::No, 's' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

const NGETTEXT_LONGOPTS: &[LongOpt] = &[
    LongOpt::new("context", HasArg::Required, 'c' as i32),
    LongOpt::new("domain", HasArg::Required, 'd' as i32),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

/// `--version`; `{pn}` é o último componente do `argv[0]`.
const VERSION: &str = "{pn} (GNU gettext-runtime) 0.23.1
Copyright (C) 1995-2024 Free Software Foundation, Inc.
License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.
Written by Ulrich Drepper.
";

/// `--help` do gettext; `{pn}` é o `argv[0]` inteiro e `{dir}` o diretório de catálogos.
const GETTEXT_HELP: &str = "Usage: {pn} [OPTION] [[TEXTDOMAIN] MSGID]
or:    {pn} [OPTION] -s [MSGID]...

Display native language translation of a textual message.

  -d, --domain=TEXTDOMAIN   retrieve translated messages from TEXTDOMAIN
  -c, --context=CONTEXT     specify context for MSGID
  -e                        enable expansion of some escape sequences
  -n                        suppress trailing newline
  -E                        (ignored for compatibility)
  [TEXTDOMAIN] MSGID        retrieve translated message corresponding
                            to MSGID from TEXTDOMAIN

Informative output:
  -h, --help                display this help and exit
  -V, --version             display version information and exit

If the TEXTDOMAIN parameter is not given, the domain is determined from the
environment variable TEXTDOMAIN.  If the message catalog is not found in the
regular directory, another location can be specified with the environment
variable TEXTDOMAINDIR.
When used with the -s option the program behaves like the 'echo' command.
But it does not simply copy its arguments to stdout.  Instead those messages
found in the selected catalog are translated.
Standard search directory: {dir}

Report bugs in the bug tracker at <https://savannah.gnu.org/projects/gettext>
or by email to <bug-gettext@gnu.org>.
";

const NGETTEXT_HELP: &str = "Usage: {pn} [OPTION] [TEXTDOMAIN] MSGID MSGID-PLURAL COUNT

Display native language translation of a textual message whose grammatical
form depends on a number.

  -d, --domain=TEXTDOMAIN   retrieve translated message from TEXTDOMAIN
  -c, --context=CONTEXT     specify context for MSGID
  -e                        enable expansion of some escape sequences
  -E                        (ignored for compatibility)
  [TEXTDOMAIN]              retrieve translated message from TEXTDOMAIN
  MSGID MSGID-PLURAL        translate MSGID (singular) / MSGID-PLURAL (plural)
  COUNT                     choose singular/plural form based on this value

Informative output:
  -h, --help                display this help and exit
  -V, --version             display version information and exit

If the TEXTDOMAIN parameter is not given, the domain is determined from the
environment variable TEXTDOMAIN.  If the message catalog is not found in the
regular directory, another location can be specified with the environment
variable TEXTDOMAINDIR.
Standard search directory: {dir}

Report bugs in the bug tracker at <https://savannah.gnu.org/projects/gettext>
or by email to <bug-gettext@gnu.org>.
";

pub fn gettext_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run_gettext(args))
}

pub fn ngettext_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run_ngettext(args))
}

/// Parte do `argv[0]` depois da última `/` (o `last_component` do gnulib).
fn last_component(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `error (EXIT_FAILURE, 0, msg)` da glibc: `programa: mensagem` no stderr.
fn error_msg(argv0: &str, msg: &str) {
    io::eprint(format!("{argv0}: {msg}\n"));
}

fn print_version(argv0: &str) {
    let text = VERSION.replace("{pn}", last_component(argv0));
    let _ = io::stdout().write_all(text.as_bytes());
}

/// O diretório de catálogos que o `--help` cita (o `IN_HELP2MAN` do help2man troca por um marcador).
fn localedir_for_help() -> &'static str {
    if sys::getenv("IN_HELP2MAN").is_none() {
        LOCALEDIR
    } else {
        "@localedir@"
    }
}

fn print_help(template: &str, argv0: &str) {
    let text = template
        .replace("{pn}", argv0)
        .replace("{dir}", localedir_for_help());
    let _ = io::stdout().write_all(text.as_bytes());
}

/// `usage (EXIT_FAILURE)`: só a dica no stderr.
fn usage_failure(argv0: &str) -> i32 {
    io::eprint(format!("Try '{argv0} --help' for more information.\n"));
    1
}

/// Algarismo octal de um byte, se for.
fn octal(b: Option<&u8>) -> Option<u32> {
    match b {
        Some(&c) if (b'0'..=b'7').contains(&c) => Some(u32::from(c - b'0')),
        _ => None,
    }
}

/// `expand_escapes` do `escapes.h`: troca `\a \b \f \n \r \t \v \\` e os octais `\NNN` (até três
/// dígitos). Com `c_seen`, `\c` é engolido e liga a marca (que no `gettext -s` tira o `\n` final);
/// sem ele o `\c` passa com a barra. Barra seguida de qualquer outro caractere passa intacta. O
/// resultado pode ter NUL no meio (`\0`): quem imprime corta na primeira, como o `fputs`.
fn expand_escapes(s: &[u8], mut c_seen: Option<&mut bool>) -> Vec<u8> {
    let mut cp = 0usize;
    // Procura o primeiro escape reconhecido; sem nenhum, a entrada volta como veio.
    loop {
        while cp < s.len() && s[cp] != b'\\' {
            cp += 1;
        }
        if cp >= s.len() || cp + 1 >= s.len() {
            return s.to_vec();
        }
        if b"abcfnrtv\\01234567".contains(&s[cp + 1]) {
            break;
        }
        cp += 1;
    }
    let mut out: Vec<u8> = s[..cp].to_vec();
    loop {
        // Aqui s[cp] == '\\'.
        cp += 1;
        let c = s.get(cp).copied().unwrap_or(0);
        match c {
            b'a' => {
                out.push(0x07);
                cp += 1;
            }
            b'b' => {
                out.push(0x08);
                cp += 1;
            }
            b'f' => {
                out.push(0x0c);
                cp += 1;
            }
            b'n' => {
                out.push(b'\n');
                cp += 1;
            }
            b'r' => {
                out.push(b'\r');
                cp += 1;
            }
            b't' => {
                out.push(b'\t');
                cp += 1;
            }
            b'v' => {
                out.push(0x0b);
                cp += 1;
            }
            b'\\' => {
                out.push(b'\\');
                cp += 1;
            }
            b'0'..=b'7' => {
                let mut ch = u32::from(c - b'0');
                cp += 1;
                if let Some(d) = octal(s.get(cp)) {
                    ch = ch * 8 + d;
                    cp += 1;
                    if let Some(d) = octal(s.get(cp)) {
                        ch = ch * 8 + d;
                        cp += 1;
                    }
                }
                out.push((ch & 0xff) as u8);
            }
            b'c' => match c_seen.as_deref_mut() {
                Some(flag) => {
                    *flag = true;
                    cp += 1;
                }
                None => out.push(b'\\'),
            },
            _ => out.push(b'\\'),
        }
        while cp < s.len() && s[cp] != b'\\' {
            out.push(s[cp]);
            cp += 1;
        }
        if cp >= s.len() {
            break;
        }
    }
    out
}

/// O que o `fputs` do C enxerga: a string até a primeira NUL.
fn c_string(s: &[u8]) -> &[u8] {
    match s.iter().position(|b| *b == 0) {
        Some(p) => &s[..p],
        None => s,
    }
}

/// `strtoul (count, &endp, 10)` seguido do teste do `ngettext`: `Some(n)` só se o texto inteiro é um
/// número sem erro de faixa; senão `None` e o chamador usa 99 (plural).
fn parse_count(count: &[u8]) -> Option<u64> {
    if count.is_empty() {
        return None;
    }
    let mut i = 0;
    while i < count.len() && matches!(count[i], b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
        i += 1;
    }
    let mut negative = false;
    if i < count.len() && (count[i] == b'+' || count[i] == b'-') {
        negative = count[i] == b'-';
        i += 1;
    }
    let digits_start = i;
    let mut value: u64 = 0;
    let mut overflow = false;
    while i < count.len() && count[i].is_ascii_digit() {
        let d = u64::from(count[i] - b'0');
        match value.checked_mul(10).and_then(|v| v.checked_add(d)) {
            Some(v) => value = v,
            None => overflow = true,
        }
        i += 1;
    }
    // Sem dígitos o endp volta ao começo (e não é NUL, pois o texto não é vazio).
    if i == digits_start || i != count.len() || overflow {
        return None;
    }
    Some(if negative {
        value.wrapping_neg()
    } else {
        value
    })
}

fn run_gettext(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut do_help = false;
    let mut do_shell = false;
    let mut do_version = false;
    let mut do_expand = false;
    let mut inhibit_added_newline = false;

    let mut getopt = Getopt::from_env(&argv[1..], "+c:d:eEhnsV", GETTEXT_LONGOPTS);
    while let Some(r) = getopt.next_opt() {
        match r {
            Ok(opt) => match opt.short() {
                // Contexto e domínio só importam com catálogo.
                Some('c') | Some('d') | Some('E') => {}
                Some('e') => do_expand = true,
                Some('h') => do_help = true,
                Some('n') => inhibit_added_newline = true,
                Some('s') => do_shell = true,
                Some('V') => do_version = true,
                _ => return usage_failure(&argv0),
            },
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                return usage_failure(&argv0);
            }
        }
    }
    let operands = getopt.operands();

    if do_version {
        print_version(&argv0);
        return 0;
    }
    if do_help {
        print_help(GETTEXT_HELP, &argv0);
        return 0;
    }

    let mut out = io::stdout();
    if !do_shell {
        // Uma mensagem só vai pro stdout: [DOMÍNIO] MSGID. Sem catálogo, o texto sai como veio.
        let msgid = match operands.len() {
            0 => {
                error_msg(&argv0, "missing arguments");
                return 1;
            }
            1 => &operands[0],
            2 => &operands[1],
            _ => {
                error_msg(&argv0, "too many arguments");
                return 1;
            }
        };
        let text = if do_expand {
            expand_escapes(msgid, Some(&mut inhibit_added_newline))
        } else {
            msgid.clone()
        };
        let _ = out.write_all(c_string(&text));
    } else {
        // Emula o `echo`: todos os argumentos são mensagens, separados por um espaço.
        let count = operands.len();
        for (i, msgid) in operands.iter().enumerate() {
            let text = if do_expand {
                expand_escapes(msgid, Some(&mut inhibit_added_newline))
            } else {
                msgid.clone()
            };
            let _ = out.write_all(c_string(&text));
            if i + 1 < count {
                let _ = out.write_all(b" ");
            }
        }
        if !inhibit_added_newline {
            let _ = out.write_all(b"\n");
        }
    }
    0
}

fn run_ngettext(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut do_help = false;
    let mut do_version = false;
    let mut do_expand = false;

    let mut getopt = Getopt::from_env(&argv[1..], "+c:d:eEhV", NGETTEXT_LONGOPTS);
    while let Some(r) = getopt.next_opt() {
        match r {
            Ok(opt) => match opt.short() {
                Some('c') | Some('d') | Some('E') => {}
                Some('e') => do_expand = true,
                Some('h') => do_help = true,
                Some('V') => do_version = true,
                _ => return usage_failure(&argv0),
            },
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                return usage_failure(&argv0);
            }
        }
    }
    let operands = getopt.operands();

    if do_version {
        print_version(&argv0);
        return 0;
    }
    if do_help {
        print_help(NGETTEXT_HELP, &argv0);
        return 0;
    }

    // [DOMÍNIO] MSGID MSGID-PLURAL COUNT.
    let base = match operands.len() {
        3 => 0,
        4 => 1,
        0..=2 => {
            error_msg(&argv0, "missing arguments");
            return 1;
        }
        _ => {
            error_msg(&argv0, "too many arguments");
            return 1;
        }
    };
    // COUNT inválido conta como plural (99).
    let n = parse_count(&operands[base + 2]).unwrap_or(99);
    let (msgid, msgid_plural) = if do_expand {
        (
            expand_escapes(&operands[base], None),
            expand_escapes(&operands[base + 1], None),
        )
    } else {
        (operands[base].clone(), operands[base + 1].clone())
    };
    // Sem catálogo: a regra inglesa, singular só para n == 1.
    let chosen = if n == 1 { &msgid } else { &msgid_plural };
    let _ = io::stdout().write_all(c_string(chosen));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use sysabi::Program;
    use sysabi::testkit::TestKit;

    fn kit() -> TestKit {
        TestKit::new().programs([
            Program::bin("gettext", gettext_main),
            Program::bin("ngettext", ngettext_main),
        ])
    }

    #[test]
    fn escapes_follow_escapes_h() {
        let mut c = false;
        assert_eq!(
            expand_escapes(b"a\\tb\\n\\x41\\101\\0z", Some(&mut c)),
            b"a\tb\n\\x41A\0z"
        );
        assert!(!c);
        assert_eq!(expand_escapes(b"a\\cb", Some(&mut c)), b"ab");
        assert!(c);
        assert_eq!(expand_escapes(b"a\\cb", None), b"a\\cb");
        assert_eq!(expand_escapes(b"x\\", None), b"x\\");
        assert_eq!(expand_escapes(b"\\0101\\1011", None), b"\x081A1");
        assert_eq!(expand_escapes(b"a\\qb\\\\c", None), b"a\\qb\\c");
    }

    #[test]
    fn count_parsing_is_strtoul() {
        assert_eq!(parse_count(b"1"), Some(1));
        assert_eq!(parse_count(b" +1"), Some(1));
        assert_eq!(parse_count(b"-1"), Some(u64::MAX));
        assert_eq!(parse_count(b"-18446744073709551615"), Some(1));
        assert_eq!(parse_count(b""), None);
        assert_eq!(parse_count(b"1x"), None);
        assert_eq!(parse_count(b"x"), None);
        assert_eq!(parse_count(b"18446744073709551616"), None);
    }

    #[test]
    fn gettext_modes() {
        let k = kit();
        let r = k.run(&["gettext", "msg"], b"");
        assert_eq!(r.stdout_str(), "msg");
        let r = k.run(&["gettext", "-s", "-n", "a", "b"], b"");
        assert_eq!(r.stdout_str(), "a b");
        let r = k.run(&["gettext", "-s", "-e", "x\\cy", "z"], b"");
        assert_eq!(r.stdout_str(), "xy z");
        let r = k.run(&["gettext"], b"");
        assert_eq!(
            (r.stderr_str().as_str(), r.code()),
            ("gettext: missing arguments\n", 1)
        );
        let r = k.run(&["gettext", "a", "b", "c"], b"");
        assert_eq!(
            (r.stderr_str().as_str(), r.code()),
            ("gettext: too many arguments\n", 1)
        );
    }

    #[test]
    fn ngettext_picks_form() {
        let k = kit();
        assert_eq!(k.run(&["ngettext", "a", "b", "1"], b"").stdout_str(), "a");
        assert_eq!(k.run(&["ngettext", "a", "b", "2"], b"").stdout_str(), "b");
        assert_eq!(k.run(&["ngettext", "a", "b", "x"], b"").stdout_str(), "b");
        let r = k.run(&["ngettext", "a", "b"], b"");
        assert_eq!(
            (r.stderr_str().as_str(), r.code()),
            ("ngettext: missing arguments\n", 1)
        );
    }
}
