//! `getopt(1)` do util-linux 2.41 (pacote util-linux do Debian 13): a versão "enhanced", com opções
//! longas, citação pra bash/sh e csh/tcsh e o modo de compatibilidade com o `getopt` do BSD.
//!
//! O algoritmo é o do `misc-utils/getopt.c` e a varredura é o `getopt_long`/`getopt_long_only` da
//! glibc 2.41 (módulo [`engine`]), então as mensagens de erro e a permutação do argv saem iguais.
//!
//! Códigos de saída: 0 sucesso, 1 o `getopt(3)` achou erro, 2 problema nos argumentos do próprio
//! `getopt(1)`, 3 erro interno, 4 pra `-T`.

pub mod engine;

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;
use engine::{Engine, LongDef};

const GETOPT_EXIT_CODE: i32 = 1;
const PARAMETER_EXIT_CODE: i32 = 2;
const TEST_EXIT_CODE: i32 = 4;

/// Retorno do `getopt(3)` pra operando em RETURN_IN_ORDER.
const NON_OPT: i32 = 1;
/// Retorno do `getopt(3)` pra opção longa (todas têm `flag`).
const LONG_OPT: i32 = 0;

#[derive(Copy, Clone, PartialEq, Eq)]
enum Shell {
    Bash,
    Tcsh,
}

struct Control {
    shell: Shell,
    optstr: Option<Vec<u8>>,
    name: Option<Vec<u8>>,
    long_options: Vec<LongDef>,
    quiet_errors: bool,
    quiet_output: bool,
    quote: bool,
}

fn usage_text(short: &str) -> String {
    format!(
        "\nUsage:\n \
{short} <optstring> <parameters>\n \
{short} [options] [--] <optstring> <parameters>\n \
{short} [options] -o|--options <optstring> [options] [--] <parameters>\n\
\nParse command options.\n\
\nOptions:\n \
-a, --alternative             allow long options starting with single -\n \
-l, --longoptions <longopts>  the long options to be recognized\n \
-n, --name <progname>         the name under which errors are reported\n \
-o, --options <optstring>     the short options to be recognized\n \
-q, --quiet                   disable error reporting by getopt(3)\n \
-Q, --quiet-output            no normal output\n \
-s, --shell <shell>           set quoting conventions to those of <shell>\n \
-T, --test                    test for getopt(1) version\n \
-u, --unquoted                do not quote the output\n\
\n \
-h, --help                    display this help\n \
-V, --version                 display version\n\
\nFor more details see getopt(1).\n"
    )
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn basename(p: &[u8]) -> String {
    let b = p.rsplit(|c| *c == b'/').next().unwrap_or(p);
    io::lossy(b)
}

/// `parse_error`: o `warnx` opcional e a dica `Try 'getopt --help'`, saída 2.
fn parse_error(short: &str, message: Option<&str>) -> i32 {
    if let Some(m) = message {
        io::eprint(format!("{short}: {m}\n"));
    }
    io::eprint(format!("Try '{short} --help' for more information.\n"));
    PARAMETER_EXIT_CODE
}

fn env_is_set(name: &str) -> bool {
    sys::getenv(name).is_some()
}

/// `print_normalized`: o argumento citado pro shell escolhido (ou cru com `-u`), com um espaço na frente.
fn print_normalized(ctl: &Control, out: &mut impl Write, arg: &[u8]) {
    if !ctl.quote {
        let _ = out.write_all(b" ");
        let _ = out.write_all(arg);
        return;
    }
    let mut buf: Vec<u8> = Vec::with_capacity(arg.len() * 4 + 3);
    buf.push(b' ');
    buf.push(b'\'');
    for &c in arg {
        if ctl.shell == Shell::Tcsh {
            match c {
                b'\\' => {
                    buf.extend_from_slice(b"\\\\");
                    continue;
                }
                b'!' => {
                    buf.extend_from_slice(b"'\\!'");
                    continue;
                }
                b'\n' => {
                    buf.extend_from_slice(b"\\n");
                    continue;
                }
                b' ' | b'\t' | 0x0b | 0x0c | b'\r' => {
                    buf.extend_from_slice(b"'\\");
                    buf.push(c);
                    buf.push(b'\'');
                    continue;
                }
                _ => {}
            }
        }
        if c == b'\'' {
            buf.extend_from_slice(b"'\\''");
        } else {
            buf.push(c);
        }
    }
    buf.push(b'\'');
    let _ = out.write_all(&buf);
}

/// `generate_output`: varre `av` (com `av[0]` no papel de nome do programa) e escreve a linha pro shell.
fn generate_output(ctl: &Control, av: Vec<Vec<u8>>, long_only: bool, posix_env: bool) -> i32 {
    let mut exit_code = 0;
    let optstr = ctl.optstr.clone().unwrap_or_default();
    let mut eng = Engine::new(av, 0, posix_env);
    if ctl.quiet_errors {
        eng.opterr = false;
    }
    let mut out = io::stdout();
    loop {
        let opt = eng.getopt(&optstr, &ctl.long_options, long_only);
        if opt == -1 {
            break;
        }
        if opt == i32::from(b'?') || opt == i32::from(b':') {
            exit_code = GETOPT_EXIT_CODE;
        } else if !ctl.quiet_output {
            let optarg = eng.optarg.clone().unwrap_or_default();
            match opt {
                LONG_OPT => {
                    let lo = &ctl.long_options[eng.longind];
                    let _ = out.write_all(b" --");
                    let _ = out.write_all(&lo.name);
                    if lo.has_arg != 0 {
                        print_normalized(ctl, &mut out, &optarg);
                    }
                }
                NON_OPT => print_normalized(ctl, &mut out, &optarg),
                _ => {
                    let byte = opt as u8;
                    let _ = out.write_all(&[b' ', b'-', byte]);
                    if let Some(p) = optstr.iter().position(|&b| b == byte)
                        && optstr.get(p + 1) == Some(&b':')
                    {
                        print_normalized(ctl, &mut out, &optarg);
                    }
                }
            }
        }
    }
    if !ctl.quiet_output {
        let _ = out.write_all(b" --");
        for a in &eng.argv[eng.optind.min(eng.argv.len())..] {
            print_normalized(ctl, &mut out, a);
        }
        let _ = out.write_all(b"\n");
    }
    if out.flush().is_err() {
        return 3;
    }
    exit_code
}

fn add_short_options(ctl: &mut Control, options: &[u8], posix_env: bool) {
    if options.first() != Some(&b'+') && posix_env {
        let mut s = vec![b'+'];
        s.extend_from_slice(options);
        ctl.optstr = Some(s);
    } else {
        ctl.optstr = Some(options.to_vec());
    }
}

/// `add_long_options`: lista separada por vírgula ou espaço; `:` no fim pede argumento, `::` opcional.
/// `Err` com a mensagem de `parse_error`.
fn add_long_options(ctl: &mut Control, options: &[u8]) -> Result<(), &'static str> {
    for tok in options
        .split(|b| matches!(b, b',' | b' ' | b'\t' | b'\n'))
        .filter(|t| !t.is_empty())
    {
        let mut name = tok.to_vec();
        let len = name.len();
        let mut has_arg = 0u8;
        if name[len - 1] == b':' {
            let prev = if len >= 2 { name[len - 2] } else { 0 };
            if prev == b':' {
                name.truncate(len - 2);
                has_arg = 2;
            } else {
                name.truncate(len - 1);
                has_arg = 1;
            }
            if name.is_empty() {
                return Err("empty long option after -l or --long argument");
            }
        }
        let val = ctl.long_options.len() as i32;
        ctl.long_options.push(LongDef {
            name,
            has_arg,
            val,
            flag: true,
        });
    }
    Ok(())
}

fn shell_type(name: &[u8]) -> Option<Shell> {
    match name {
        b"bash" | b"sh" => Some(Shell::Bash),
        b"tcsh" | b"csh" => Some(Shell::Tcsh),
        _ => None,
    }
}

fn own_long_options() -> Vec<LongDef> {
    let t: [(&str, u8, u8); 11] = [
        ("options", 1, b'o'),
        ("longoptions", 1, b'l'),
        ("quiet", 0, b'q'),
        ("quiet-output", 0, b'Q'),
        ("shell", 1, b's'),
        ("test", 0, b'T'),
        ("unquoted", 0, b'u'),
        ("help", 0, b'h'),
        ("alternative", 0, b'a'),
        ("name", 1, b'n'),
        ("version", 0, b'V'),
    ];
    t.iter()
        .map(|(n, a, v)| LongDef {
            name: n.as_bytes().to_vec(),
            has_arg: *a,
            val: i32::from(*v),
            flag: false,
        })
        .collect()
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = basename(&argv[0]);
    let compatible = env_is_set("GETOPT_COMPATIBLE");
    let posix_env = env_is_set("POSIXLY_CORRECT");

    if argv.len() == 1 {
        if compatible {
            // O getopt original não dava erro sem argumentos.
            let mut out = io::stdout();
            let _ = out.write_all(b" --\n");
            return 0;
        }
        return parse_error(&short, Some("missing optstring argument"));
    }

    let mut ctl = Control {
        shell: Shell::Bash,
        optstr: None,
        name: None,
        long_options: Vec::new(),
        quiet_errors: false,
        quiet_output: false,
        quote: true,
    };

    if argv[1].first() != Some(&b'-') || compatible {
        ctl.quote = false;
        let skip = argv[1]
            .iter()
            .take_while(|b| matches!(b, b'-' | b'+'))
            .count();
        ctl.optstr = Some(argv[1][skip..].to_vec());
        let mut av = vec![argv[0].clone()];
        av.extend(argv[2..].iter().cloned());
        return generate_output(&ctl, av, false, posix_env);
    }

    let mut long_only = false;
    let own_longs = own_long_options();
    let mut eng = Engine::new(argv.clone(), 1, posix_env);
    loop {
        let opt = eng.getopt(b"+ao:l:n:qQs:TuhV", &own_longs, false);
        if opt == -1 {
            break;
        }
        let optarg = eng.optarg.clone().unwrap_or_default();
        match u8::try_from(opt).unwrap_or(b'?') {
            b'a' => long_only = true,
            b'o' => add_short_options(&mut ctl, &optarg, posix_env),
            b'l' => {
                if let Err(m) = add_long_options(&mut ctl, &optarg) {
                    return parse_error(&short, Some(m));
                }
            }
            b'n' => ctl.name = Some(optarg),
            b'q' => ctl.quiet_errors = true,
            b'Q' => ctl.quiet_output = true,
            b's' => match shell_type(&optarg) {
                Some(s) => ctl.shell = s,
                None => {
                    return parse_error(&short, Some("unknown shell after -s or --shell argument"));
                }
            },
            b'T' => return TEST_EXIT_CODE,
            b'u' => ctl.quote = false,
            b'V' => {
                let mut out = io::stdout();
                let _ = out.write_all(format!("{short} from util-linux 2.41.5\n").as_bytes());
                return 0;
            }
            b'h' => {
                let mut out = io::stdout();
                let _ = out.write_all(usage_text(&short).as_bytes());
                return 0;
            }
            _ => return parse_error(&short, None),
        }
    }

    if ctl.optstr.is_none() {
        if eng.optind >= eng.argv.len() {
            return parse_error(&short, Some("missing optstring argument"));
        }
        let spec = eng.argv[eng.optind].clone();
        add_short_options(&mut ctl, &spec, posix_env);
        eng.optind += 1;
    }

    let mut av: Vec<Vec<u8>> = eng.argv[eng.optind - 1..].to_vec();
    av[0] = match &ctl.name {
        Some(n) => n.clone(),
        None => argv[0].clone(),
    };
    generate_output(&ctl, av, long_only, posix_env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sysabi::Program;
    use sysabi::testkit::TestKit;

    fn kit() -> TestKit {
        TestKit::new().programs([Program::bin("getopt", main)])
    }

    #[test]
    fn quotes_and_permutes() {
        let r = kit().run(
            &[
                "getopt",
                "-o",
                "ab:c::",
                "-l",
                "alpha,beta:,gamma::",
                "--",
                "-a",
                "-b",
                "x",
                "-cy",
                "--gamma",
                "foo",
                "it's",
            ],
            b"",
        );
        assert_eq!(
            r.stdout_str(),
            " -a -b 'x' -c 'y' --gamma '' -- 'foo' 'it'\\''s'\n"
        );
        assert_eq!(r.code(), 0);
    }

    #[test]
    fn unquoted_compat_form() {
        let r = kit().run(&["getopt", "ab:", "-a", "-b", "x", "y"], b"");
        assert_eq!(r.stdout_str(), " -a -b x -- y\n");
    }

    #[test]
    fn invalid_option_exits_one() {
        let r = kit().run(&["getopt", "-o", "a", "--", "-z"], b"");
        assert_eq!(
            (r.stdout_str().as_str(), r.stderr_str().as_str(), r.code()),
            (" --\n", "getopt: invalid option -- 'z'\n", 1)
        );
    }

    #[test]
    fn tcsh_quoting() {
        let r = kit().run(
            &["getopt", "-s", "tcsh", "-o", "a:", "--", "-a", "x y!\\z\nw"],
            b"",
        );
        assert_eq!(r.stdout_str(), " -a 'x'\\ 'y'\\!'\\\\z\\nw' --\n");
    }
}
