//! ul-python: `python3` (CPython 3.13.5 do Debian 13) do pseudo-linus.
//!
//! O plano completo está em `docs/python3-port.md`. Esta etapa traz a linha de comando do
//! `Python/initconfig.c` (`config_parse_cmdline` sobre o `_PyOS_GetOpt` de `Python/getopt.c`) e a
//! tabela de tokens (`token`). Já batem byte a byte: `-V`/`--version`, `-h`/`-?`/`--help`, opção
//! curta ou longa desconhecida, `-J` reservado e opção sem o argumento obrigatório (`-c`, `-m`,
//! `-W`, `-X`, `--check-hash-based-pycs`), todos com o código de saída do CPython.
//!
//! A execução de código (`-c`, `-m`, arquivo, stdin) depende do interpretador, que entra na fatia 10
//! do plano; até lá esses usos terminam com a linha de uso no stderr e código 2, o mesmo caminho de
//! erro de uso do CPython.

pub mod ast;
pub mod object;
pub mod token;
pub mod tokenizer;

use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;

use sysabi::{sys, Ctx, Fd, Program};

/// Versão informada por `python3 -V` no Debian 13.
pub const VERSION: &str = "3.13.5";

/// Programas deste crate.
pub fn programs() -> Vec<Program> {
    vec![Program::bin("python3", python3_main), Program::bin("python3.13", python3_main)]
}

/// `usage_line` do `initconfig.c`.
fn usage_line(program: &str) -> String {
    format!("usage: {program} [option] ... [-c cmd | -m mod | file | -] [arg] ...\n")
}

/// `usage_help` do `initconfig.c` do 3.13.
const USAGE_HELP: &str = "\
Options (and corresponding environment variables):
-b     : issue warnings about converting bytes/bytearray to str and comparing
         bytes/bytearray with str or bytes with int. (-bb: issue errors)
-B     : don't write .pyc files on import; also PYTHONDONTWRITEBYTECODE=x
-c cmd : program passed in as string (terminates option list)
-d     : turn on parser debugging output (for experts only, only works on
         debug builds); also PYTHONDEBUG=x
-E     : ignore PYTHON* environment variables (such as PYTHONPATH)
-h     : print this help message and exit (also -? or --help)
-i     : inspect interactively after running script; forces a prompt even
         if stdin does not appear to be a terminal; also PYTHONINSPECT=x
-I     : isolate Python from the user's environment (implies -E, -P and -s)
-m mod : run library module as a script (terminates option list)
-O     : remove assert and __debug__-dependent statements; add .opt-1 before
         .pyc extension; also PYTHONOPTIMIZE=x
-OO    : do -O changes and also discard docstrings; add .opt-2 before
         .pyc extension
-P     : don't prepend a potentially unsafe path to sys.path; also
         PYTHONSAFEPATH
-q     : don't print version and copyright messages on interactive startup
-s     : don't add user site directory to sys.path; also PYTHONNOUSERSITE=x
-S     : don't imply 'import site' on initialization
-u     : force the stdout and stderr streams to be unbuffered;
         this option has no effect on stdin; also PYTHONUNBUFFERED=x
-v     : verbose (trace import statements); also PYTHONVERBOSE=x
         can be supplied multiple times to increase verbosity
-V     : print the Python version number and exit (also --version)
         when given twice, print more information about the build
-W arg : warning control; arg is action:message:category:module:lineno
         also PYTHONWARNINGS=arg
-x     : skip first line of source, allowing use of non-Unix forms of #!cmd
-X opt : set implementation-specific option
--check-hash-based-pycs always|default|never:
         control how Python invalidates hash-based .pyc files
--help-env: print help about Python environment variables and exit
--help-xoptions: print help about implementation-specific -X options and exit
--help-all: print complete help information and exit

Arguments:
file   : program read from script file
-      : program read from stdin (default; interactive mode if a tty)
arg ...: arguments passed to program in sys.argv[1:]
";

/// Opções curtas do 3.13 (`PROGRAM_OPTS`); `:` marca as que levam argumento.
const SHORT_OPTS: &[u8] = b"bBc:dEhiIJm:OPqRsStuvVW:xX:?";

/// Opções longas: nome, se exige argumento e a opção curta equivalente (`\0` para as só longas).
const LONG_OPTS: &[(&str, bool, u8)] = &[
    ("check-hash-based-pycs", true, 0),
    ("help-all", false, 1),
    ("help-env", false, 2),
    ("help-xoptions", false, 3),
    ("help", false, b'h'),
    ("version", false, b'V'),
];

fn write_stdout(text: &str) {
    // O CPython não confere a escrita do -V/-h; uma falha aqui não muda o código de saída.
    let _ = sys::write_all(Fd::STDOUT, text.as_bytes());
}

fn write_stderr(text: &str) {
    let _ = sys::write_all(Fd::STDERR, text.as_bytes());
}

/// Erro de uso (`config_usage(1, program)`) e código 2.
fn usage_error(program: &str) -> i32 {
    write_stderr(&format!("{}Try `python -h' for more information.\n", usage_line(program)));
    2
}

/// Resultado de uma chamada ao `_PyOS_GetOpt`.
enum Opt {
    /// Fim das opções.
    End,
    /// Opção reconhecida (curta, ou o código da longa).
    Found(u8),
    /// Erro já relatado no stderr (o `'_'` do CPython).
    Error,
}

/// Estado do `_PyOS_GetOpt` (`_PyOS_optind` e a posição dentro de um grupo de opções curtas).
struct GetOpt<'a> {
    args: &'a [Vec<u8>],
    optind: usize,
    pos: usize,
}

impl GetOpt<'_> {
    fn next(&mut self) -> Opt {
        if self.pos == 0 {
            let Some(arg) = self.args.get(self.optind) else { return Opt::End };
            if arg.len() < 2 || arg[0] != b'-' {
                return Opt::End;
            }
            if arg.as_slice() == b"--" {
                self.optind += 1;
                return Opt::End;
            }
            if arg[1] == b'-' {
                return self.long(arg);
            }
            self.pos = 1;
            self.optind += 1;
        }
        let arg = &self.args[self.optind - 1];
        let c = arg[self.pos];
        self.pos += 1;
        let at_end = self.pos >= arg.len();
        if c == b'J' {
            write_stderr("-J is reserved for Jython\n");
            return Opt::Error;
        }
        let Some(i) = SHORT_OPTS.iter().position(|o| *o == c && c != b':') else {
            write_stderr(&format!("Unknown option: -{}\n", char::from(c)));
            return Opt::Error;
        };
        if SHORT_OPTS.get(i + 1) == Some(&b':') {
            // O argumento é o resto do grupo, ou o próximo item do argv.
            self.pos = 0;
            if !at_end {
                return Opt::Found(c);
            }
            if self.optind >= self.args.len() {
                write_stderr(&format!("Argument expected for the -{} option\n", char::from(c)));
                return Opt::Error;
            }
            self.optind += 1;
            return Opt::Found(c);
        }
        if at_end {
            self.pos = 0;
        }
        Opt::Found(c)
    }

    fn long(&mut self, arg: &[u8]) -> Opt {
        let name = &arg[2..];
        self.optind += 1;
        let Some(&(long, needs_arg, code)) = LONG_OPTS.iter().find(|(n, _, _)| n.as_bytes() == name)
        else {
            write_stderr(&format!("unknown option {}\n", String::from_utf8_lossy(arg)));
            return Opt::Error;
        };
        if needs_arg {
            if self.optind >= self.args.len() {
                write_stderr(&format!("Argument expected for the --{long} options\n"));
                return Opt::Error;
            }
            self.optind += 1;
        }
        Opt::Found(code)
    }
}

fn python3_main(_ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    let program = argv.first().map(|a| String::from_utf8_lossy(a.as_bytes()).into_owned());
    let program = program.unwrap_or_else(|| "python3".to_string());
    let args: Vec<Vec<u8>> = argv.iter().skip(1).map(|a| a.as_bytes().to_vec()).collect();
    let mut getopt = GetOpt { args: &args, optind: 0, pos: 0 };
    let mut print_version = 0u32;
    loop {
        match getopt.next() {
            Opt::End => break,
            Opt::Error => return usage_error(&program),
            Opt::Found(b'h' | b'?') => {
                write_stdout(&usage_line(&program));
                write_stdout(USAGE_HELP);
                return 0;
            }
            Opt::Found(b'V') => print_version += 1,
            // `-c` e `-m` encerram a lista de opções.
            Opt::Found(b'c' | b'm') => break,
            Opt::Found(_) => {}
        }
    }
    if print_version > 0 {
        write_stdout(&format!("Python {VERSION}\n"));
        return 0;
    }
    // Execução de código: fatia 10 do plano (ver o doc do módulo).
    usage_error(&program)
}
