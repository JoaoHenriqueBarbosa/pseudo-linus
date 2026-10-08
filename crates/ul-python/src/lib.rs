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
pub mod bigint;
pub mod builtins;
pub mod builtins_ext;
pub mod carets;
pub mod classes;
pub mod compile;
pub mod cp437;
pub mod cpybc;
pub mod cpyops;
pub mod dictview;
pub mod finalize;
pub mod fold;
pub mod fork;
pub mod format;
pub mod frameobj;
pub mod generator;
pub mod generic;
pub mod globalsview;
pub mod gthread;
pub mod heapimage;
pub mod idna;
#[cfg(test)]
mod lang_tests;
pub mod lazy;
mod mangle;
pub mod methods;
pub mod modrun;
pub mod modules;
pub mod native_util;
pub mod object;
pub mod parser;
pub mod pep695;
pub mod stdbuf;
pub mod stdin;
pub mod suggest;
pub mod textcodec;
mod textcodec_tables;
#[cfg(test)]
mod stdlib_tests;
pub mod token;
pub mod tokenizer;
pub mod tbobj;
pub mod tracing;
pub mod typeattrs;
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
    // Como o `sys.stderr` do CPython: `backslashreplace`, que nunca falha (o texto cru é só reserva).
    let data = textcodec::encode_utf8(text, "backslashreplace").unwrap_or_else(|_| text.as_bytes().to_vec());
    let _ = sys::write_all(Fd::STDERR, &data);
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

/// Opções `-W` da linha de comando, lidas por `sys.warnoptions` (o interpretador roda noutra thread).
pub(crate) static WARN_OPTIONS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Nível de `-O` (0, 1 ou 2): com `-O` os `assert` somem e `__debug__` vira `False`.
pub(crate) static OPTIMIZE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Versão completa de `-VV` e de `sys.version`.
pub const VERSION_LONG: &str = "3.13.5 (main, Aug 10 2026, 12:06:59) [GCC 14.2.0]";

/// Campos de `sys.flags` que vêm da linha de comando, na ordem do CPython 3.13 (`optimize` é o `OPTIMIZE`):
/// debug, inspect, interactive, optimize, dont_write_bytecode, no_user_site, no_site, ignore_environment,
/// verbose, bytes_warning, quiet, hash_randomization, isolated, dev_mode, utf8_mode, warn_default_encoding,
/// safe_path, int_max_str_digits.
pub(crate) static CLI_FLAGS: std::sync::Mutex<[i64; 18]> = std::sync::Mutex::new(DEFAULT_FLAGS);
const DEFAULT_FLAGS: [i64; 18] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 4300];

/// Aplica uma opção de uma letra de `python3` ao vetor de `sys.flags`.
fn apply_flag(flags: &mut [i64; 18], letter: u8, optarg: Option<&[u8]>) {
    match letter {
        b'd' => flags[0] += 1,
        b'i' => {
            flags[1] = 1;
            flags[2] = 1;
        }
        b'B' => flags[4] = 1,
        b's' => flags[5] = 1,
        b'S' => flags[6] = 1,
        b'E' => flags[7] = 1,
        b'v' => flags[8] += 1,
        b'b' => flags[9] += 1,
        b'q' => flags[10] = 1,
        b'I' => {
            flags[12] = 1;
            flags[7] = 1;
            flags[5] = 1;
            flags[16] = 1;
        }
        b'P' => flags[16] = 1,
        b'X' => match optarg {
            Some(b"dev") => flags[13] = 1,
            Some(b"utf8" | b"utf8=1") => flags[14] = 1,
            _ => {}
        },
        _ => {}
    }
}

fn python3_main(_ctx: &mut Ctx, argv: &[OsString]) -> i32 {
    WARN_OPTIONS.lock().unwrap().clear();
    OPTIMIZE.store(0, std::sync::atomic::Ordering::Relaxed);
    *CLI_FLAGS.lock().unwrap() = DEFAULT_FLAGS;
    let program = argv.first().map(|a| String::from_utf8_lossy(a.as_bytes()).into_owned());
    let program = program.unwrap_or_else(|| "python3".to_string());
    let args: Vec<Vec<u8>> = argv.iter().skip(1).map(|a| a.as_bytes().to_vec()).collect();
    let mut getopt = GetOpt { args: &args, optind: 0, pos: 0, optarg: None };
    let mut print_version = 0u32;
    let mut command: Option<Vec<u8>> = None;
    let mut module: Option<Vec<u8>> = None;
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
            Opt::Found(b'O') => {
                let n = OPTIMIZE.load(std::sync::atomic::Ordering::Relaxed);
                OPTIMIZE.store(n.saturating_add(1).min(2), std::sync::atomic::Ordering::Relaxed);
            }
            Opt::Found(b'W') => {
                if let Some(a) = getopt.optarg.take() {
                    WARN_OPTIONS.lock().unwrap().push(String::from_utf8_lossy(&a).into_owned());
                }
            }
            // `-c` e `-m` encerram a lista de opções.
            Opt::Found(b'c') => {
                command = getopt.optarg.take();
                break;
            }
            Opt::Found(b'm') => {
                module = getopt.optarg.take();
                break;
            }
            Opt::Found(letter) => {
                let arg = getopt.optarg.take();
                apply_flag(&mut CLI_FLAGS.lock().unwrap(), letter, arg.as_deref());
            }
        }
    }
    let no_site = CLI_FLAGS.lock().unwrap()[6] != 0;
    let located = locate(&program, no_site);
    *LAYOUT.lock().unwrap() = Some(located);
    if print_version > 0 {
        if print_version > 1 {
            write_stdout(&format!("Python {VERSION_LONG}\n"));
        } else {
            write_stdout(&format!("Python {VERSION}\n"));
        }
        return 0;
    }
    if let Some(command) = command {
        let rest: Vec<Vec<u8>> = args[getopt.optind.min(args.len())..].to_vec();
        return run_command(&command, &rest);
    }
    if let Some(module) = module {
        let rest: Vec<Vec<u8>> = args[getopt.optind.min(args.len())..].to_vec();
        return run_module(&String::from_utf8_lossy(&module), &rest, &program);
    }
    let rest: Vec<Vec<u8>> = args[getopt.optind.min(args.len())..].to_vec();
    run_script(&rest, &program)
}

fn is_regular(path: &str) -> bool {
    sys::stat(path.as_bytes()).is_ok_and(|st| st.mode & 0o170_000 == 0o100_000)
}

fn is_dir(path: &str) -> bool {
    sys::stat(path.as_bytes()).is_ok_and(|st| st.mode & 0o170_000 == 0o040_000)
}

/// Caminho absoluto e normalizado (`.`, `..` e `//` resolvidos sem tocar o disco), como o CPython mostra o
/// arquivo do script nos tracebacks e em `__file__`.
pub(crate) fn absolute_path(path: &str) -> String {
    let full = if path.starts_with('/') {
        path.to_string()
    } else {
        let cwd = sys::try_current().and_then(|p| p.getcwd().ok()).unwrap_or_default();
        format!("{}/{path}", String::from_utf8_lossy(&cwd).trim_end_matches('/'))
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in full.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    format!("/{}", parts.join("/"))
}

/// Onde o interpretador se encontra, como o `Modules/getpath.py` do CPython: o executável é o
/// caminho pelo qual ele foi chamado (procurado no `PATH`, sem seguir symlinks), e um `pyvenv.cfg`
/// ao lado dele ou um diretório acima faz do diretório do arquivo o `sys.prefix` de um venv.
#[derive(Clone)]
pub(crate) struct Layout {
    pub executable: String,
    pub base_executable: String,
    pub prefix: String,
    /// Os diretórios de pacotes de terceiros, na ordem do `sys.path`.
    pub site: Vec<String>,
}

const SYSTEM_SITE: [&str; 2] = ["/usr/local/lib/python3.13/dist-packages", "/usr/lib/python3/dist-packages"];

/// As entradas da stdlib no `sys.path`, antes dos pacotes de terceiros.
pub(crate) const STDLIB_PATH: [&str; 3] = ["/usr/lib/python313.zip", "/usr/lib/python3.13", "/usr/lib/python3.13/lib-dynload"];

static LAYOUT: std::sync::Mutex<Option<Layout>> = std::sync::Mutex::new(None);

/// O [`Layout`] do `python3` em execução (o do sistema, se nenhum foi calculado).
pub(crate) fn layout() -> Layout {
    LAYOUT.lock().unwrap().clone().unwrap_or_else(|| Layout {
        executable: "/usr/bin/python3".to_string(),
        base_executable: "/usr/bin/python3".to_string(),
        prefix: "/usr".to_string(),
        site: SYSTEM_SITE.iter().map(|s| s.to_string()).collect(),
    })
}

fn locate(program: &str, no_site: bool) -> Layout {
    let executable = if program.contains('/') {
        absolute_path(program)
    } else {
        let path = sys::try_current().and_then(|s| s.getenv(b"PATH")).unwrap_or_default();
        String::from_utf8_lossy(&path)
            .split(':')
            .map(|dir| if dir.is_empty() { "." } else { dir })
            .map(|dir| absolute_path(&format!("{dir}/{program}")))
            .find(|p| is_regular(p))
            .unwrap_or_else(|| "/usr/bin/python3".to_string())
    };
    let bin_dir = executable.rsplit_once('/').map_or("/", |(d, _)| if d.is_empty() { "/" } else { d }).to_string();
    let parent = bin_dir.rsplit_once('/').map_or("/", |(d, _)| if d.is_empty() { "/" } else { d }).to_string();
    let venv = [bin_dir, parent].into_iter().find_map(|dir| {
        let cfg = sys::read_file(format!("{}/pyvenv.cfg", dir.trim_end_matches('/')).as_bytes()).ok()?;
        Some((dir, String::from_utf8_lossy(&cfg).into_owned()))
    });
    let mut layout = Layout {
        executable: executable.clone(),
        base_executable: executable,
        prefix: "/usr".to_string(),
        site: SYSTEM_SITE.iter().map(|s| s.to_string()).collect(),
    };
    if let Some((dir, cfg)) = venv {
        let value = |key: &str| {
            cfg.lines().find_map(|line| {
                let (k, v) = line.split_once('=')?;
                (k.trim() == key).then(|| v.trim().to_string())
            })
        };
        let base_name = layout.executable.rsplit('/').next().unwrap_or("python3").to_string();
        layout.base_executable = value("executable")
            .or_else(|| value("home").map(|h| format!("{}/{base_name}", h.trim_end_matches('/'))))
            .unwrap_or_else(|| layout.base_executable.clone());
        let system = value("include-system-site-packages").is_some_and(|v| v.eq_ignore_ascii_case("true"));
        let mut site = vec![format!("{}/lib/python3.13/site-packages", dir.trim_end_matches('/'))];
        if system {
            site.extend(SYSTEM_SITE.iter().map(|s| s.to_string()));
        }
        layout.site = site;
        layout.prefix = dir;
    }
    if no_site {
        layout.site.clear();
    }
    layout
}

/// `python3 -m pacote.modulo args...`: procura no diretório atual (e em `sys.path`), roda como `__main__`.
fn run_module(name: &str, rest: &[Vec<u8>], _program: &str) -> i32 {
    let cwd = String::from_utf8_lossy(&sys::current().getcwd().unwrap_or_default()).into_owned();
    let cwd = cwd.trim_end_matches('/').to_string();
    let parts: Vec<&str> = name.split('.').collect();
    let search = |base: &str| -> Option<(String, String)> {
        let mut dir = base.to_string();
        for (i, part) in parts.iter().enumerate() {
            let last = i + 1 == parts.len();
            let pkg_dir = format!("{dir}/{part}");
            if last {
                let main = format!("{pkg_dir}/__main__.py");
                if is_dir(&pkg_dir) && is_regular(&main) {
                    return Some((main, name.to_string()));
                } else if is_regular(&format!("{dir}/{part}.py")) {
                    let package = parts[..i].join(".");
                    return Some((format!("{dir}/{part}.py"), package));
                }
            } else if is_dir(&pkg_dir) {
                // com ou sem `__init__.py` (pacote de namespace, PEP 420)
                dir = pkg_dir;
            } else {
                break;
            }
        }
        None
    };
    let embedded = modules::pysrc::source(&format!("{name}.__main__")).is_some() || modules::pysrc::source(name).is_some();
    // A ordem do `sys.path`: o diretório atual (fora no modo isolado ou com -P), a stdlib (embutida
    // ou no disco) e os pacotes de terceiros.
    let safe_path = CLI_FLAGS.lock().unwrap()[16] != 0;
    let found = (!safe_path).then(|| search(&cwd)).flatten().or_else(|| {
        if embedded {
            return None;
        }
        let site = layout().site;
        std::iter::once("/usr/lib/python3.13").chain(site.iter().map(String::as_str)).find_map(|d| search(d))
    });
    let (path, text, package) = match found {
        Some((path, package)) => {
            let text = sys::read_file(path.as_bytes()).unwrap_or_default();
            (path, String::from_utf8_lossy(&text).into_owned(), package)
        }
        None => {
            // Módulos embutidos (`json.tool`, `unittest`): o fonte vive no binário.
            let root = "/usr/lib/python3.13";
            let as_path = name.replace('.', "/");
            if let Some(src) = modules::pysrc::source(&format!("{name}.__main__")) {
                (format!("{root}/{as_path}/__main__.py"), src.to_string(), name.to_string())
            } else if let Some(src) = modules::pysrc::source(name) {
                let package = parts[..parts.len() - 1].join(".");
                (format!("{root}/{as_path}.py"), src.to_string(), package)
            } else {
                write_stderr(&format!("/usr/bin/python3: No module named {name}\n"));
                return 1;
            }
        }
    };
    let argv = std::iter::once(path.clone()).chain(rest.iter().map(|a| String::from_utf8_lossy(a).into_owned())).collect();
    let outcome = run_main(&text, argv, &path, true, Some((package, cwd)), Finish::Module);
    conclude(outcome, Finish::Module)
}

/// Como o resultado de uma execução vira saída e código de saída: o que difere entre `python3 arquivo`,
/// `-c` e `-m`. O filho de um `os.fork` termina do mesmo jeito que o pai terminaria.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Finish {
    /// Arquivo, `-` e o programa lido do stdin: o stdout pendente descarrega antes do erro.
    File,
    /// `-c`: o erro sai antes do stdout que ainda estava no buffer.
    Command,
    /// `-m`: como `-c`, e o `runpy` do CPython aparece no traceback do erro que sobe do módulo.
    Module,
}

/// Escreve o resultado de uma execução e devolve o código de saída.
pub(crate) fn conclude(mut outcome: Outcome, mode: Finish) -> i32 {
    if mode == Finish::Module {
        // O último bloco do traceback (se houver cadeia) mostra o `runpy` que chamou o módulo.
        const HEADER: &str = "Traceback (most recent call last):\n";
        if let Some(at) = outcome.stderr.rfind(HEADER) {
            let frames = "  File \"<frozen runpy>\", line 198, in _run_module_as_main\n  File \"<frozen runpy>\", line 88, in _run_code\n";
            outcome.stderr.insert_str(at + HEADER.len(), frames);
        }
    }
    // `-m` e `-c` mostram o erro antes de o stdout pendente descarregar (só os arquivos descarregam antes).
    finish(outcome, mode != Finish::File)
}

/// Escreve o resultado de uma execução e devolve o código de saída. Com `error_first`, o texto do erro
/// sai antes do stdout que ainda estava no buffer, como no CPython com `-c` e `-m`.
fn finish(outcome: Outcome, error_first: bool) -> i32 {
    let mut status = outcome.status;
    // O `flush_std_files` do `Py_FinalizeEx`: o stdout que não sai (leitor do pipe já fechado) vira
    // aviso no stderr e, se o programa ia sair com 0, o código passa a 120.
    let mut flush = || {
        if let Err(e) = sys::write_all(Fd::STDOUT, &outcome.stdout) {
            let e = crate::modules::osnative::os_error(e, None);
            write_stderr(&format!("Exception ignored on flushing sys.stdout:\n{}: {}\n", e.kind, e.msg));
            if status == 0 {
                status = 120;
            }
        }
    };
    if error_first && !outcome.stderr.is_empty() {
        write_stderr(&outcome.stderr);
        flush();
    } else {
        flush();
        if !outcome.stderr.is_empty() {
            write_stderr(&outcome.stderr);
        }
    }
    if status == UNHANDLED_INTERRUPT {
        return die_by_sigint();
    }
    status
}

/// O `status` de um `Outcome` cujo programa terminou com `KeyboardInterrupt` sem tratamento: o `Py_RunMain`
/// do CPython não sai com 1, e sim morrendo pelo SIGINT (`exit_sigint`), depois de descarregar tudo.
pub(crate) const UNHANDLED_INTERRUPT: i32 = -2;

/// O `exit_sigint` do CPython (bpo-1054041): o SIGINT volta à ação padrão e o processo o envia a si mesmo,
/// para quem espera ver `WIFSIGNALED` (o shell mostra 130). Se o sinal não matar, sai com `128 + SIGINT`.
fn die_by_sigint() -> i32 {
    if let Some(process) = sys::try_current() {
        let _ = process.sigaction(sysabi::Signal::SIGINT, sysabi::SigDisposition::Default);
        let _ = process.kill(sysabi::KillTarget::Pid(process.getpid()), sysabi::Signal::SIGINT);
    }
    128 + sysabi::Signal::SIGINT.0
}

/// O `PyErr_GivenExceptionMatches(exc, PyExc_KeyboardInterrupt)` do desfecho de um programa: a própria
/// classe ou uma subclasse (embutida ou de usuário).
fn is_keyboard_interrupt(e: &vm::PyException) -> bool {
    let kind = match &e.value {
        Some(object::Value::Instance(i)) => i.class().builtin_base.unwrap_or(e.kind),
        _ => e.kind,
    };
    object::exc_is_subclass(kind, "KeyboardInterrupt")
}

/// `python3 arquivo.py args...`, `python3 - args...` ou o programa lido do stdin.
fn run_script(rest: &[Vec<u8>], program: &str) -> i32 {
    let to_s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    if let Some(path) = rest.first().filter(|p| p.as_slice() != b"-") {
        // `python3 app.pyz` e `python3 diretório`: roda o `__main__.py` de dentro, com o arquivo em
        // `sys.path[0]`; o caminho vira absoluto, como no `config_run_filename_abspath` do CPython.
        let shown = absolute_path(&to_s(path));
        if let Some((main, text)) = modules::userimport::main_of_archive(&shown) {
            let argv = rest.iter().map(|a| to_s(a)).collect();
            let outcome = run_main(&text, argv, &main, true, Some((String::new(), shown)), Finish::File);
            return conclude(outcome, Finish::File);
        }
    }
    let (src, name, argv, file_mode) = match rest.first() {
        Some(path) if path.as_slice() != b"-" => {
            let text = match sys::read_file(path) {
                Ok(b) => b,
                Err(e) => {
                    let abs = if path.starts_with(b"/") {
                        to_s(path)
                    } else {
                        let cwd = sys::current().getcwd().unwrap_or_default();
                        format!("{}/{}", to_s(&cwd).trim_end_matches('/'), to_s(path))
                    };
                    write_stderr(&format!("{program}: can't open file '{abs}': [Errno {}] {}\n", e.0, e.message()));
                    return 2;
                }
            };
            (String::from_utf8_lossy(&text).into_owned(), absolute_path(&to_s(path)), rest.iter().map(|a| to_s(a)).collect(), true)
        }
        other => {
            let text = sys::read_to_end(Fd::STDIN).unwrap_or_default();
            let first = other.map_or_else(String::new, |_| "-".to_string());
            let argv = std::iter::once(first).chain(rest.iter().skip(1).map(|a| to_s(a))).collect();
            (String::from_utf8_lossy(&text).into_owned(), "<stdin>".to_string(), argv, true)
        }
    };
    let outcome = run_with(&src, argv, &name, file_mode);
    conclude(outcome, Finish::File)
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
    run_source_args(src, vec!["-c".to_string()])
}

/// Como `run_source`, com `sys.argv` explícito.
pub fn run_source_args(src: &str, argv: Vec<String>) -> Outcome {
    run_with(src, argv, "<string>", false)
}

/// Executa o texto com o nome de arquivo mostrado nos tracebacks (`file_mode` mostra a linha fonte).
pub fn run_with(src: &str, argv: Vec<String>, name: &str, file_mode: bool) -> Outcome {
    run_main(src, argv, name, file_mode, None, Finish::File)
}

/// `run_with` de um módulo de `-m`: `(pacote, diretório atual)` define `__package__` e `sys.path[0]`.
fn run_main(
    src: &str,
    argv: Vec<String>,
    name: &str,
    file_mode: bool,
    module: Option<(String, String)>,
    finish_mode: Finish,
) -> Outcome {
    let owned = src.to_string();
    let name = name.to_string();
    // Como o CPython na partida: SIGPIPE e SIGXFSZ ignorados (a escrita num pipe fechado vira
    // BrokenPipeError em vez de matar o processo).
    if let Some(c) = &sys::try_current() {
        let _ = c.sigaction(sysabi::Signal::SIGPIPE, sysabi::SigDisposition::Ignore);
        let _ = c.sigaction(sysabi::Signal::SIGXFSZ, sysabi::SigDisposition::Ignore);
    }
    on_interpreter_thread(move || run_source_inner(&owned, argv, &name, file_mode, module, finish_mode)).unwrap_or_else(|| Outcome {
        stdout: Vec::new(),
        stderr: "Fatal Python error: could not run the interpreter thread\n".into(),
        status: 1,
    })
}

/// Roda `job` na thread do interpretador: pilha de 1 GiB (reservada, não comprometida) para o limite de
/// recursão de 1000 chamadas caber, e o pseudo-processo de quem chama instalado nela (uma thread nova não
/// o herda; `open`, stdin e stderr dependem dele). Os desvios de controle do kernel (`execve`, `_exit`,
/// morte por sinal) desempilham a thread e seguem até o pseudo-processo, que é quem troca o programa ou
/// termina; um panic de bug (mensagem `&str`/`String`) vira `None`.
pub(crate) fn on_interpreter_thread<T: Send + 'static>(job: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    let current = sys::try_current();
    let spawned = std::thread::Builder::new().stack_size(1 << 30).spawn(move || {
        if let Some(c) = current {
            sys::install(c);
        }
        job()
    });
    match spawned.map(|h| h.join()) {
        Ok(Ok(value)) => Some(value),
        Ok(Err(payload)) if !payload.is::<&'static str>() && !payload.is::<String>() => std::panic::resume_unwind(payload),
        _ => None,
    }
}

/// As globais que o `pymain` do CPython dá ao `__main__`: `__package__`, `__loader__`, `__spec__`,
/// `__annotations__`, `__builtins__` e, quando há arquivo, `__cached__`. Com `-m`, a spec é a do
/// módulo achado (e o `__cached__` aponta o `__pycache__`); no `-c` e no stdin o loader é o
/// `BuiltinImporter`.
fn main_globals(machine: &mut vm::Vm, name: &str, file_mode: bool, package: Option<&str>) {
    use object::Value;
    let builtins_module = modules::import(machine, "builtins");
    let builtins = builtins_module.clone().map(Value::Module);
    let frozen = modules::import(machine, "_frozen_importlib");
    let external = modules::import(machine, "_frozen_importlib_external");
    let attr = |m: &Option<std::rc::Rc<object::ModuleObj>>, n: &str| m.as_ref().and_then(|m| m.attrs.borrow().get(n).cloned());
    // O `_frozen_importlib` pede o `builtins` enquanto ainda roda, e o `builtins` construído nessa hora não
    // acha o `BuiltinImporter`: o `__loader__` dele entra aqui, já com o módulo pronto.
    if let (Some(module), Some(importer)) = (&builtins_module, attr(&frozen, "BuiltinImporter")) {
        module.attrs.borrow_mut().entry("__loader__".to_string()).or_insert(importer);
    }
    let mut spec = Value::None;
    let mut loader = attr(&frozen, "BuiltinImporter").unwrap_or(Value::None);
    let mut cached = Value::None;
    if let Some(package) = package {
        let stem = name.rsplit('/').next().unwrap_or(name).trim_end_matches(".py");
        let modname = match (package.is_empty(), stem) {
            (true, _) => stem.to_string(),
            (false, _) => format!("{package}.{stem}"),
        };
        let frozen_mod = name.starts_with("/usr/lib/python3.13/") && object::FROZEN_MODULES.contains(&modname.as_str());
        if let Some(make) = attr(&external, "_spec_for_module") {
            let args = vec![Value::str(modname), Value::str(name), Value::Bool(false), Value::Bool(frozen_mod)];
            if let Ok(s) = machine.call(&make, args, Vec::new()) {
                loader = machine.load_attr(&s, "loader").unwrap_or(Value::None);
                cached = machine.load_attr(&s, "cached").unwrap_or(Value::None);
                spec = s;
            }
        }
    } else if file_mode && name != "<stdin>" {
        if let Some(cls) = attr(&external, "SourceFileLoader") {
            let path = Value::str(absolute_path(name));
            loader = machine.call(&cls, vec![Value::str("__main__"), path], Vec::new()).unwrap_or(Value::None);
        }
    }
    let annotations = Value::dict(crate::object::Dict::default());
    let mut g = machine.globals.borrow_mut();
    g.insert("__package__".into(), match package {
        Some(p) => Value::str(p),
        None => Value::None,
    });
    g.insert("__loader__".into(), loader);
    g.insert("__spec__".into(), spec);
    g.entry("__annotations__".into()).or_insert(annotations);
    if let Some(b) = builtins {
        g.insert("__builtins__".into(), b);
    }
    if file_mode {
        g.insert("__file__".into(), object::Value::str(name));
        g.insert("__cached__".into(), cached);
    }
}

fn run_source_inner(
    src: &str,
    argv: Vec<String>,
    name: &str,
    file_mode: bool,
    main_module: Option<(String, String)>,
    finish_mode: Finish,
) -> Outcome {
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
            return Outcome { stdout: Vec::new(), stderr: syntax_error(&src, kind, &e, name), status: 1 };
        }
    };
    let code = match compile::compile_module(&module) {
        Ok(code) => code,
        Err(e) => {
            let stderr = if e.kind == "SyntaxError" {
                format!("  File \"{name}\", line {}\n{}: {}\n", e.lineno, e.kind, e.msg)
            } else {
                vm::format_traceback_in(
                    &vm::RuntimeError {
                        exc: vm::PyException { kind: e.kind, msg: e.msg, value: None, tb: Vec::new() },
                        lineno: e.lineno,
                    },
                    name,
                    Some(src.as_str()),
                )
            };
            return Outcome { stdout: Vec::new(), stderr, status: 1 };
        }
    };
    let mut machine = vm::Vm::with_argv(argv);
    machine.globals.borrow_mut().insert("__doc__".into(), object::Value::None);
    if file_mode && name != "<stdin>" {
        vm::register_source(name, &src);
        vm::register_source(&absolute_path(name), &src);
    }
    let mut code = code;
    code.set_filename("");
    let mut prelude: Result<(), vm::RuntimeError> = Ok(());
    // `init_import_site`: sem `-S`, o `site` do disco roda o `main()` antes do programa (venv,
    // site-packages, arquivos `.pth`, sitecustomize, `quit`/`exit`/`help`). Os testes de unidade rodam
    // sem a imagem no disco, e aí não há `site` para importar.
    let has_site = sysabi::sys::try_current().is_some_and(|_| is_regular("/usr/lib/python3.13/site.py"));
    if CLI_FLAGS.lock().unwrap()[6] == 0 && has_site {
        if let Err(e) = modules::import_checked(&mut machine, "site") {
            prelude = Err(vm::RuntimeError { exc: e, lineno: 0 });
        }
    }
    // `sys.path[0]`: o diretório do script, ou '' para -c e -m; fora no modo isolado e com -P.
    if CLI_FLAGS.lock().unwrap()[16] == 0 {
        if let Some(sysmod) = modules::import(&mut machine, "sys") {
            let script_dir = modules::import(&mut machine, "_sys").and_then(|m| m.attrs.borrow().get("script_dir").cloned());
            if let (Some(object::Value::List(path)), Some(dir)) = (sysmod.attrs.borrow().get("path"), script_dir) {
                path.borrow_mut().insert(0, dir);
            }
        }
    }
    main_globals(&mut machine, name, file_mode, main_module.as_ref().map(|(p, _)| p.as_str()));
    if let Some((package, cwd)) = &main_module {
        machine.globals.borrow_mut().insert("__package__".into(), object::Value::str(package.clone()));
        if let Some(sysmod) = modules::import(&mut machine, "sys").filter(|_| CLI_FLAGS.lock().unwrap()[16] == 0) {
            if let Some(object::Value::List(path)) = sysmod.attrs.borrow().get("path") {
                path.borrow_mut()[0] = object::Value::str(cwd.clone());
            }
        }
        // Como o CPython, importa o pacote pai (que roda o `__init__.py`) antes do módulo.
        if !package.is_empty() {
            if let Err(e) = modules::import_checked(&mut machine, package) {
                prelude = Err(vm::RuntimeError { exc: e, lineno: 0 });
            }
        }
    }
    // O `os.fork` leva isto ao filho: o que falta para terminar o programa como o pai terminaria.
    let shown_src = if name == "<stdin>" { None } else { Some(src.clone()) };
    fork::set_tail(fork::RunTail { name: name.to_string(), shown_src, finish_mode, verdict: None });
    let result = match prelude {
        Ok(()) => machine.run(&std::rc::Rc::new(code)),
        Err(e) => Err(e),
    };
    conclude_run(&mut machine, result, name, src.as_str())
}

/// O fim de um programa: calcula o desfecho (o erro é impresso antes do `atexit`, como no CPython), roda os
/// ganchos de saída e monta o `Outcome` (`src` é o texto que o traceback mostra; o programa lido do stdin não
/// tem como mostrar a linha fonte).
pub(crate) fn conclude_run(machine: &mut vm::Vm, result: Result<(), vm::RuntimeError>, name: &str, src: &str) -> Outcome {
    let verdict = match result {
        Ok(()) => fork::Verdict { status: 0, stderr: String::new() },
        Err(e) if e.exc.kind == "SystemExit" => {
            let (status, stderr) = system_exit(&e.exc);
            fork::Verdict { status, stderr }
        }
        Err(e) => {
            machine.prepare_error(&e.exc);
            let shown_src = if name == "<stdin>" { None } else { Some(src) };
            let status = if is_keyboard_interrupt(&e.exc) { UNHANDLED_INTERRUPT } else { 1 };
            fork::Verdict { status, stderr: vm::format_traceback_in(&e, name, shown_src) }
        }
    };
    fork::enter_exit_phase(&verdict);
    machine.run_exit_hooks();
    finish_exit(machine, verdict)
}

/// O resto da saída depois do `atexit`, no pai e no filho de um `fork` feito numa função dele: finaliza os
/// objetos que ainda vivem e entrega o stdout pendente com o desfecho do programa.
pub(crate) fn finish_exit(machine: &mut vm::Vm, verdict: fork::Verdict) -> Outcome {
    machine.finalize_at_exit();
    let stdout = std::mem::take(&mut *machine.stdout.borrow_mut());
    let captured = std::mem::take(&mut *machine.stderr_capture.borrow_mut());
    Outcome { stdout, stderr: captured + &verdict.stderr, status: verdict.status }
}

/// Código de saída e texto de stderr de um `SystemExit` que chegou ao topo (como o
/// `handle_system_exit` do CPython): sem argumento ou `None` sai com 0, um inteiro é o código e
/// qualquer outro valor é impresso no stderr com saída 1.
fn system_exit(e: &vm::PyException) -> (i32, String) {
    let args: Vec<object::Value> = match &e.value {
        Some(object::Value::Exception(x)) => x.args.clone(),
        Some(object::Value::Instance(i)) => match i.dict.borrow().get("args") {
            Some(object::Value::Tuple(t)) => t.to_vec(),
            _ => Vec::new(),
        },
        _ if e.msg.is_empty() => Vec::new(),
        _ => vec![object::Value::str(e.msg.clone())],
    };
    match args.as_slice() {
        [] | [object::Value::None] => (0, String::new()),
        [object::Value::Int(n)] => ((*n & 0xff) as i32, String::new()),
        [object::Value::Bool(b)] => (i32::from(*b), String::new()),
        [other, ..] => (1, format!("{}\n", object::to_str(other))),
    }
}

/// `SyntaxError` como o `print_exception` do CPython o mostra para `-c`: arquivo e linha, a linha
/// fonte sem a indentação, os `^` sob o trecho e a mensagem.
fn syntax_error(src: &str, kind: &str, e: &parser::ParseError, name: &str) -> String {
    let mut out = format!("  File \"{name}\", line {}\n", e.lineno);
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

fn run_command(command: &[u8], rest: &[Vec<u8>]) -> i32 {
    let src = String::from_utf8_lossy(command);
    let mut argv = vec!["-c".to_string()];
    argv.extend(rest.iter().map(|a| String::from_utf8_lossy(a).into_owned()));
    let outcome = run_main(&src, argv, "<string>", false, None, Finish::Command);
    conclude(outcome, Finish::Command)
}

/// `Vm::rust_nest`: 1 no laço mais externo (incluindo chamadas Python simples), 2 dentro de um callback
/// que uma nativa despacha por `Vm::call`.
#[cfg(test)]
mod rust_nest_tests {
    use super::object::{Kw, NativeFn, Value};
    use super::vm::{PyException, Vm};
    use std::rc::Rc;

    fn nest(vm: &mut Vm, _: Vec<Value>, _: Kw) -> Result<Value, PyException> {
        Ok(Value::Int(vm.rust_nest.get() as i64))
    }

    fn ints(v: &Value) -> Vec<i64> {
        let Value::List(l) = v else { panic!("not a list") };
        l.borrow().iter().map(|x| if let Value::Int(n) = x { *n } else { panic!("not an int") }).collect()
    }

    #[test]
    fn counts_active_run_loops() {
        let src = "seen = []\n\
                   seen.append(nest())\n\
                   def f():\n    return nest()\n\
                   seen.append(f())\n\
                   l = [1, 2]\n\
                   l.sort(key=lambda x: seen.append(nest()) or x)\n\
                   sorted([1, 2], key=lambda x: seen.append(nest()) or x)\n\
                   seen.append(nest())\n";
        let module = super::parser::parse_module(src).ok().expect("parse");
        let code = Rc::new(super::compile::compile_module(&module).ok().expect("compile"));
        let mut machine = Vm::new();
        machine.globals.borrow_mut().insert("nest".into(), Value::NativeFn(Rc::new(NativeFn { name: "nest", f: nest })));
        assert!(machine.run(&code).is_ok());
        // `list.sort(key=)` recursa (2); o `key=` de `sorted` roda em quadro do laço (1).
        assert_eq!(ints(machine.globals.borrow().get("seen").expect("seen")), vec![1, 1, 2, 2, 1, 1, 1]);
        assert_eq!(machine.rust_nest.get(), 0);
    }
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
            "Traceback (most recent call last):\n  File \"<string>\", line 2, in <module>\n    1/0\n    ~^~\n\
             ZeroDivisionError: division by zero\n"
        );
        let syn = run_source("1 +");
        assert_eq!(syn.status, 1);
        assert!(syn.stderr.starts_with("  File \"<string>\", line 1\n    1 +\n"), "{}", syn.stderr);
        assert!(syn.stderr.ends_with("SyntaxError: invalid syntax\n"), "{}", syn.stderr);
    }

    #[test]
    fn file_mode_traceback_shows_source() {
        let out = run_with("x = 1\nprint(1/0)\n", vec!["t.py".into()], "t.py", true);
        assert_eq!(
            out.stderr,
            "Traceback (most recent call last):\n  File \"t.py\", line 2, in <module>\n    print(1/0)\n          ~^~\nZeroDivisionError: division by zero\n"
        );
    }

    #[test]
    fn print_exc_shows_carets_like_cpython() {
        let src = r##"import sys
import traceback


def f(x):
    return 1 / x


def g(d):
    return d["k"]["z"]


class A:
    def m(self):
        return self.nope.attr


def run(case):
    try:
        case()
    except Exception:
        traceback.print_exc(file=sys.stdout)


run(lambda: f(0))
run(lambda: g({"k": {}}))
run(lambda: A().m())
run(lambda: [x.y for x in [1]])
run(lambda: [i for i in 5])
run(lambda: None + 1)
run(lambda: len(5))
run(lambda: f(1)(2))
"##;
        // O script é um arquivo no kernel de teste: o `linecache` do Debian faz `os.stat` nele, como no CPython.
        let kit = crate::stdlib_tests::with_debian_stdlib(sysabi::testkit::TestKit::new().programs(crate::programs()))
            .file("/tmp/t.py", src, 0o644)
            .cwd("/tmp");
        let out = kit.run(&["python3", "t.py"], b"");
        assert_eq!(out.status, sysabi::WaitStatus::Exited(0), "{}", out.stderr_str());
        assert_eq!(
            // O CPython mostra o caminho absoluto do script; o esperado abaixo o abrevia.
            out.stdout_str().replace("\"/tmp/t.py\"", "\"t.py\""),
            r##"Traceback (most recent call last):
  File "t.py", line 20, in run
    case()
    ~~~~^^
  File "t.py", line 25, in <lambda>
    run(lambda: f(0))
                ~^^^
  File "t.py", line 6, in f
    return 1 / x
           ~~^~~
ZeroDivisionError: division by zero
Traceback (most recent call last):
  File "t.py", line 20, in run
    case()
    ~~~~^^
  File "t.py", line 26, in <lambda>
    run(lambda: g({"k": {}}))
                ~^^^^^^^^^^^
  File "t.py", line 10, in g
    return d["k"]["z"]
           ~~~~~~^^^^^
KeyError: 'z'
Traceback (most recent call last):
  File "t.py", line 20, in run
    case()
    ~~~~^^
  File "t.py", line 27, in <lambda>
    run(lambda: A().m())
                ~~~~~^^
  File "t.py", line 15, in m
    return self.nope.attr
           ^^^^^^^^^
AttributeError: 'A' object has no attribute 'nope'
Traceback (most recent call last):
  File "t.py", line 20, in run
    case()
    ~~~~^^
  File "t.py", line 28, in <lambda>
    run(lambda: [x.y for x in [1]])
                 ^^^
AttributeError: 'int' object has no attribute 'y'
Traceback (most recent call last):
  File "t.py", line 20, in run
    case()
    ~~~~^^
  File "t.py", line 29, in <lambda>
    run(lambda: [i for i in 5])
                            ^
TypeError: 'int' object is not iterable
Traceback (most recent call last):
  File "t.py", line 20, in run
    case()
    ~~~~^^
  File "t.py", line 30, in <lambda>
    run(lambda: None + 1)
                ~~~~~^~~
TypeError: unsupported operand type(s) for +: 'NoneType' and 'int'
Traceback (most recent call last):
  File "t.py", line 20, in run
    case()
    ~~~~^^
  File "t.py", line 31, in <lambda>
    run(lambda: len(5))
                ~~~^^^
TypeError: object of type 'int' has no len()
Traceback (most recent call last):
  File "t.py", line 20, in run
    case()
    ~~~~^^
  File "t.py", line 32, in <lambda>
    run(lambda: f(1)(2))
                ~~~~^^^
TypeError: 'float' object is not callable
"##
        );
    }

    #[test]
    fn modules_json_csv_sys() {
        let src = "import csv, sys, json\n\
                   w = csv.writer(sys.stdout, lineterminator='\\n')\n\
                   w.writerow(json.loads('[\"a,b\", \"c\\\\\"d\", 1]'))\n\
                   print(json.dumps(['é', None, True], ensure_ascii=False), sys.argv)\n\
                   try:\n    import nope\nexcept ImportError as e:\n    print(e)\n";
        let out = run_source(src);
        assert_eq!(out.stderr, "");
        assert_eq!(String::from_utf8_lossy(&out.stdout), "\"a,b\",\"c\"\"d\",1\n[\"é\", null, true] ['-c']\nNo module named 'nope'\n");
    }

    #[test]
    fn from_import() {
        let src = "from json import dumps as d, loads\nfrom sys import argv\nprint(d([1]), loads('2'), argv)\n\
                   try:\n    from json import nope\nexcept ImportError as e:\n    print(e)\n";
        let out = run_source(src);
        assert_eq!(out.stderr, "");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "[1] 2 ['-c']\ncannot import name 'nope' from 'json' (/usr/lib/python3.13/json/__init__.py)\n"
        );
    }

    #[test]
    fn sequence_builtins() {
        let src = "print(sorted([3, 1, 2]), min(4, 2, 9), max([1, 7]), sum([1, 2, 3]), abs(-5))\n\
                   print(list(zip([1, 2], 'ab')), list(enumerate('xy', start=1)), any([0, 1]), all([]))\n\
                   print(float('1.5'), bool(''), ord('a'), chr(98), tuple([1]), sorted([1, 2], reverse=True))\n";
        let out = run_source(src);
        assert_eq!(out.stderr, "");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "[1, 2, 3] 2 7 6 5\n[(1, 'a'), (2, 'b')] [(1, 'x'), (2, 'y')] True True\n1.5 False 97 b (1,) [2, 1]\n"
        );
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
