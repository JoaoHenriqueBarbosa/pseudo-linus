//! ul-python: `python3` (CPython 3.13.5 do Debian 13) do pseudo-linus.
//!
//! O plano completo está em `docs/python3-port.md`. Esta etapa traz a linha de comando do
//! `Python/initconfig.c` (`config_parse_cmdline` sobre o `_PyOS_GetOpt` de `Python/getopt.c`) e a
//! tabela de tokens (`token`). Já batem byte a byte: `-V`/`--version`, `-h`/`-?`/`--help`, opção
//! curta ou longa desconhecida, `-J` reservado e opção sem o argumento obrigatório (`-c`, `-m`,
//! `-W`, `-X`, `--check-hash-based-pycs`), todos com o código de saída do CPython.
//!
//! `-c cmd` executa o programa pelo compilador (`compile`) e pela VM (`vm`) da fatia 10; os demais
//! modos de execução (`-m`, arquivo, stdin) ainda terminam com a linha de uso no stderr e código 2,
//! o mesmo caminho de erro de uso do CPython.

pub mod ast;
pub mod compile;
pub mod object;
pub mod parser;
pub mod token;
pub mod tokenizer;
pub mod vm;

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
    /// Argumento da última opção que exige um (`_PyOS_optarg`).
    optarg: Option<Vec<u8>>,
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
        let args = self.args;
        let arg = &args[self.optind - 1];
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
            let rest = arg[self.pos.min(arg.len())..].to_vec();
            self.pos = 0;
            if !at_end {
                self.optarg = Some(rest);
                return Opt::Found(c);
            }
            if self.optind >= args.len() {
                write_stderr(&format!("Argument expected for the -{} option\n", char::from(c)));
                return Opt::Error;
            }
            self.optarg = Some(args[self.optind].clone());
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
    let mut getopt = GetOpt { args: &args, optind: 0, pos: 0, optarg: None };
    let mut print_version = 0u32;
    let mut command: Option<Vec<u8>> = None;
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
            Opt::Found(b'c') => {
                command = getopt.optarg.take();
                break;
            }
            Opt::Found(b'm') => break,
            Opt::Found(_) => {}
        }
    }
    if print_version > 0 {
        write_stdout(&format!("Python {VERSION}\n"));
        return 0;
    }
    if let Some(command) = command {
        return run_command(&command);
    }
    // `-m`, arquivo e stdin: fatias 13 e 18 (ver o doc do módulo).
    usage_error(&program)
}

/// Saída de `python3 -c` já pronta: stdout, stderr e código de saída.
pub struct Outcome {
    pub stdout: Vec<u8>,
    pub stderr: String,
    pub status: i32,
}

/// Executa o texto de um `-c`: analisa, compila e roda no nível de módulo. A execução corre numa
/// thread com pilha grande (reservada, não comprometida) para o limite de recursão de 1000 chamadas
/// caber na pilha nativa, já que a VM chama a si mesma a cada `def` invocada.
pub fn run_source(src: &str) -> Outcome {
    let owned = src.to_string();
    let spawned = std::thread::Builder::new().stack_size(1 << 30).spawn(move || run_source_inner(&owned));
    match spawned.map(|h| h.join()) {
        Ok(Ok(outcome)) => outcome,
        _ => Outcome {
            stdout: Vec::new(),
            stderr: "Fatal Python error: could not run the interpreter thread\n".into(),
            status: 1,
        },
    }
}

fn run_source_inner(src: &str) -> Outcome {
    // O `-c` do CPython compila o texto como um arquivo que termina em nova linha.
    let mut src = src.to_string();
    if !src.ends_with('\n') {
        src.push('\n');
    }
    let module = match parser::parse_module(&src) {
        Ok(module) => module,
        Err(e) => {
            let kind = match e.kind {
                parser::ErrorKind::Syntax => "SyntaxError",
                parser::ErrorKind::Indentation => "IndentationError",
                parser::ErrorKind::Tab => "TabError",
            };
            return Outcome { stdout: Vec::new(), stderr: syntax_error(&src, kind, &e), status: 1 };
        }
    };
    let code = match compile::compile_module(&module) {
        Ok(code) => code,
        Err(e) => {
            let stderr = if e.kind == "SyntaxError" {
                format!("  File \"<string>\", line {}\n{}: {}\n", e.lineno, e.kind, e.msg)
            } else {
                vm::format_traceback(&vm::RuntimeError {
                    exc: vm::PyException { kind: e.kind, msg: e.msg, value: None, tb: Vec::new() },
                    lineno: e.lineno,
                })
            };
            return Outcome { stdout: Vec::new(), stderr, status: 1 };
        }
    };
    let mut machine = vm::Vm::new();
    let result = machine.run(&code);
    let stdout = std::mem::take(&mut machine.stdout);
    match result {
        Ok(()) => Outcome { stdout, stderr: String::new(), status: 0 },
        Err(e) => Outcome { stdout, stderr: vm::format_traceback(&e), status: 1 },
    }
}

/// `SyntaxError` como o `print_exception` do CPython o mostra para `-c`: arquivo e linha, a linha
/// fonte sem a indentação, os `^` sob o trecho e a mensagem.
fn syntax_error(src: &str, kind: &str, e: &parser::ParseError) -> String {
    let mut out = format!("  File \"<string>\", line {}\n", e.lineno);
    if let Some(line) = src.lines().nth(e.lineno.saturating_sub(1)) {
        let trimmed = line.trim_start();
        let indent = line.chars().count() - trimmed.chars().count();
        let trimmed = trimmed.trim_end();
        if !trimmed.is_empty() {
            out.push_str(&format!("    {trimmed}\n"));
            let start = e.offset.saturating_sub(1).saturating_sub(indent);
            let width = if e.end_lineno == e.lineno && e.end_offset > e.offset { e.end_offset - e.offset } else { 1 };
            out.push_str(&format!("    {}{}\n", " ".repeat(start), "^".repeat(width)));
        }
    }
    out.push_str(&format!("{kind}: {}\n", e.msg));
    out
}

fn run_command(command: &[u8]) -> i32 {
    let src = String::from_utf8_lossy(command);
    let outcome = run_source(&src);
    let _ = sys::write_all(Fd::STDOUT, &outcome.stdout);
    if !outcome.stderr.is_empty() {
        write_stderr(&outcome.stderr);
    }
    outcome.status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_source_prints_and_reports() {
        let ok = run_source("print('hi')");
        assert_eq!((ok.stdout.as_slice(), ok.stderr.as_str(), ok.status), (&b"hi\n"[..], "", 0));
        let err = run_source("print(1)\n1/0");
        assert_eq!(err.stdout, b"1\n");
        assert_eq!(err.status, 1);
        assert_eq!(
            err.stderr,
            "Traceback (most recent call last):\n  File \"<string>\", line 2, in <module>\n\
             ZeroDivisionError: division by zero\n"
        );
        let syn = run_source("1 +");
        assert_eq!(syn.status, 1);
        assert!(syn.stderr.starts_with("  File \"<string>\", line 1\n    1 +\n"), "{}", syn.stderr);
        assert!(syn.stderr.ends_with("SyntaxError: invalid syntax\n"), "{}", syn.stderr);
    }

    #[test]
    fn getopt_captures_command() {
        let args: Vec<Vec<u8>> = vec![b"-c".to_vec(), b"print(1)".to_vec(), b"x".to_vec()];
        let mut g = GetOpt { args: &args, optind: 0, pos: 0, optarg: None };
        assert!(matches!(g.next(), Opt::Found(b'c')));
        assert_eq!(g.optarg.as_deref(), Some(&b"print(1)"[..]));
        let args: Vec<Vec<u8>> = vec![b"-Bcpass".to_vec()];
        let mut g = GetOpt { args: &args, optind: 0, pos: 0, optarg: None };
        assert!(matches!(g.next(), Opt::Found(b'B')));
        assert!(matches!(g.next(), Opt::Found(b'c')));
        assert_eq!(g.optarg.as_deref(), Some(&b"pass"[..]));
    }
}
