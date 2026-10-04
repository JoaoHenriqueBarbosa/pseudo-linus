//! Builtins do shell.

pub mod core;
pub mod declare;
pub mod dirs;
pub mod getopts;
pub mod jobs;
pub mod read;
pub mod setopt;
pub mod test;

use std::time::Duration;

use sysabi::{Errno, Fd};

use crate::shell::{Exec, Shell, write_fd};

/// Valor de atribuição já expandido (argumento de `declare x=...`).
#[derive(Clone, Debug)]
pub enum AssignedValue {
    Scalar(Vec<u8>),
    /// (chave, `+=`, valor) de `nome=(...)`.
    Array(Vec<(Option<Vec<u8>>, bool, Vec<u8>)>),
}

#[derive(Clone, Debug)]
pub struct AssignArg {
    pub name: String,
    pub index: Option<Vec<u8>>,
    pub append: bool,
    pub value: AssignedValue,
    /// Texto original (pro caso de o builtin não ser de declaração e precisar da palavra).
    pub raw: String,
}

/// Argumento de builtin: palavra expandida, ou atribuição (só em builtins de declaração).
#[derive(Clone, Debug)]
pub enum Arg {
    Word(Vec<u8>),
    Assign(AssignArg),
}

impl Arg {
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Arg::Word(w) => w,
            Arg::Assign(a) => a.raw.as_bytes(),
        }
    }

    /// Forma em bytes (`nome=valor` pra atribuição escalar).
    pub fn into_bytes(self) -> Vec<u8> {
        match self {
            Arg::Word(w) => w,
            Arg::Assign(a) => {
                let mut out = a.name.into_bytes();
                if let Some(i) = a.index {
                    out.push(b'[');
                    out.extend(i);
                    out.push(b']');
                }
                if a.append {
                    out.push(b'+');
                }
                out.push(b'=');
                match a.value {
                    AssignedValue::Scalar(v) => out.extend(v),
                    AssignedValue::Array(items) => {
                        out.push(b'(');
                        let parts: Vec<Vec<u8>> = items.into_iter().map(|(_, _, v)| v).collect();
                        out.extend(parts.join(&b' '));
                        out.push(b')');
                    }
                }
                out
            }
        }
    }

    /// Como aparece no xtrace.
    pub fn trace(&self) -> Vec<u8> {
        match self {
            Arg::Word(w) => crate::quote::xtrace_word(w),
            Arg::Assign(a) => crate::print::assign_trace(a),
        }
    }
}

/// Todos os builtins do bash 5.2 que este shell implementa.
pub const BUILTINS: &[&str] = &[
    ".", ":", "[", "alias", "bg", "break", "builtin", "caller", "cd", "command", "compgen", "continue", "declare", "dirs",
    "disown", "echo", "enable", "eval", "exec", "exit", "export", "false", "fg", "getopts", "hash", "help", "jobs", "kill",
    "let", "local", "logout", "mapfile", "popd", "printf", "pushd", "pwd", "read", "readarray", "readonly", "return", "set",
    "shift", "shopt", "source", "suspend", "test", "times", "trap", "true", "type", "typeset", "ulimit", "umask", "unalias",
    "unset", "wait",
];

pub fn is_builtin(sh: &Shell, name: &str) -> bool {
    BUILTINS.contains(&name) && !sh.disabled_builtins.contains(name)
}

/// Executa o builtin `name`.
pub fn run(sh: &mut Shell, name: &str, args: &[Arg]) -> Exec {
    let words = || -> Vec<Vec<u8>> { args.iter().map(|a| a.as_bytes().to_vec()).collect() };
    match name {
        "declare" | "typeset" | "local" | "export" | "readonly" => declare::run(sh, name, args),
        "unset" => declare::unset(sh, &words()),
        _ => {
            let argv = words();
            match name {
                ":" | "true" => Ok(0),
                "false" => Ok(1),
                "echo" => core::echo(sh, &argv),
                "printf" => core::printf(sh, &argv),
                "exit" | "logout" => core::exit(sh, &argv),
                "return" => core::return_(sh, &argv),
                "break" | "continue" => core::break_continue(sh, name, &argv),
                "shift" => core::shift(sh, &argv),
                "eval" => core::eval(sh, &argv),
                "." | "source" => core::source(sh, &argv),
                "exec" => core::exec(sh, &argv),
                "command" => core::command(sh, &argv),
                "builtin" => core::builtin(sh, &argv),
                "type" => core::type_(sh, &argv),
                "hash" => core::hash(sh, &argv),
                "enable" => core::enable(sh, &argv),
                "help" => core::help(sh, &argv),
                "times" => core::times(sh, &argv),
                "caller" => core::caller(sh, &argv),
                "let" => core::let_(sh, &argv),
                "alias" => core::alias(sh, &argv),
                "unalias" => core::unalias(sh, &argv),
                "umask" => core::umask(sh, &argv),
                "ulimit" => core::ulimit(sh, &argv),
                "compgen" => core::compgen(sh, &argv),
                "test" | "[" => test::run(sh, &argv),
                "read" => read::read(sh, &argv),
                "mapfile" | "readarray" => read::mapfile(sh, &argv),
                "set" => setopt::set(sh, &argv),
                "shopt" => setopt::shopt(sh, &argv),
                "trap" => jobs::trap(sh, &argv),
                "kill" => jobs::kill(sh, &argv),
                "wait" => jobs::wait(sh, &argv),
                "jobs" => jobs::jobs(sh, &argv),
                "fg" | "bg" => jobs::fg_bg(sh, name, &argv),
                "disown" => jobs::disown(sh, &argv),
                "suspend" => jobs::suspend(sh, &argv),
                "getopts" => getopts::getopts(sh, &argv),
                "cd" => dirs::cd(sh, &argv),
                "pwd" => dirs::pwd(sh, &argv),
                "pushd" => dirs::pushd(sh, &argv),
                "popd" => dirs::popd(sh, &argv),
                "dirs" => dirs::dirs(sh, &argv),
                _ => {
                    sh.error(format!("{name}: command not found"));
                    Ok(127)
                }
            }
        }
    }
}

/// Escreve a saída de um builtin no stdout. Erro de escrita sai como o bash
/// (`echo: write error: Bad file descriptor`) e o builtin devolve 1.
pub fn out(sh: &Shell, builtin: &str, data: &[u8]) -> bool {
    match write_fd(Fd::STDOUT, data) {
        Ok(()) => true,
        Err(e) => {
            write_error(sh, builtin, e);
            false
        }
    }
}

pub fn write_error(sh: &Shell, builtin: &str, e: Errno) {
    sh.builtin_error(builtin, format!("write error: {}", e.message()));
}

/// Opções no estilo do `internal_getopt` do bash: `-abc`, `-o arg`, `--` encerra. Devolve
/// (opções com argumento opcional, índice do primeiro operando) ou a opção inválida.
pub struct Opts {
    pub flags: Vec<(u8, Option<Vec<u8>>)>,
    pub rest: usize,
    /// Opções com `+` em vez de `-` (`declare +x`).
    pub plus: Vec<u8>,
}

pub enum OptError {
    Invalid(u8),
    MissingArg(u8),
}

/// `spec`: letras aceitas; uma letra seguida de `:` exige argumento. `allow_plus` aceita `+x`.
pub fn parse_opts(args: &[Vec<u8>], spec: &str, allow_plus: bool) -> Result<Opts, OptError> {
    let spec = spec.as_bytes();
    let takes = |c: u8| spec.iter().position(|x| *x == c).is_some_and(|i| spec.get(i + 1) == Some(&b':'));
    let known = |c: u8| c != b':' && spec.contains(&c);
    let mut flags = Vec::new();
    let mut plus = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let a = &args[i];
        if a == b"--" {
            i += 1;
            break;
        }
        let is_minus = a.len() > 1 && a[0] == b'-';
        let is_plus = allow_plus && a.len() > 1 && a[0] == b'+';
        if !is_minus && !is_plus {
            break;
        }
        let mut j = 1;
        while j < a.len() {
            let c = a[j];
            if !known(c) {
                return Err(OptError::Invalid(c));
            }
            if takes(c) {
                let val = if j + 1 < a.len() {
                    a[j + 1..].to_vec()
                } else {
                    i += 1;
                    match args.get(i) {
                        Some(v) => v.clone(),
                        None => return Err(OptError::MissingArg(c)),
                    }
                };
                if is_plus {
                    plus.push(c);
                } else {
                    flags.push((c, Some(val)));
                }
                break;
            }
            if is_plus {
                plus.push(c);
            } else {
                flags.push((c, None));
            }
            j += 1;
        }
        i += 1;
    }
    Ok(Opts { flags, rest: i, plus })
}

impl Opts {
    pub fn has(&self, c: u8) -> bool {
        self.flags.iter().any(|(f, _)| *f == c)
    }

    pub fn value(&self, c: u8) -> Option<&[u8]> {
        self.flags.iter().rev().find(|(f, _)| *f == c).and_then(|(_, v)| v.as_deref())
    }

    pub fn has_plus(&self, c: u8) -> bool {
        self.plus.contains(&c)
    }
}

/// Mensagem padrão de opção inválida mais o uso.
pub fn opt_error(sh: &Shell, builtin: &str, e: OptError, usage: &str) -> i32 {
    match e {
        OptError::Invalid(c) => sh.builtin_error(builtin, format!("-{}: invalid option", c as char)),
        OptError::MissingArg(c) => sh.builtin_error(builtin, format!("-{}: option requires an argument", c as char)),
    }
    if !usage.is_empty() {
        let _ = write_fd(Fd::STDERR, format!("{builtin}: usage: {usage}\n").as_bytes());
    }
    2
}

/// Formata `TIMEFORMAT` (`%R %U %S %P`, precisão `%3R`, forma longa `%lR`).
pub fn format_timeformat(fmt: &[u8], real: Duration, user: Duration, sys: Duration) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < fmt.len() {
        let c = fmt[i];
        if c != b'%' {
            out.push(c);
            i += 1;
            continue;
        }
        i += 1;
        if i >= fmt.len() {
            out.push(b'%');
            break;
        }
        if fmt[i] == b'%' {
            out.push(b'%');
            i += 1;
            continue;
        }
        let mut prec = 3usize;
        if fmt[i].is_ascii_digit() {
            prec = ((fmt[i] - b'0') as usize).min(3);
            i += 1;
        }
        let mut long = false;
        if i < fmt.len() && fmt[i] == b'l' {
            long = true;
            i += 1;
        }
        if i >= fmt.len() {
            break;
        }
        let which = fmt[i];
        i += 1;
        let d = match which {
            b'R' => real,
            b'U' => user,
            b'S' => sys,
            b'P' => {
                let cpu = (user + sys).as_secs_f64();
                let r = real.as_secs_f64();
                let pct = if r > 0.0 { cpu * 100.0 / r } else { 0.0 };
                out.extend(format!("{pct:.prec$}").as_bytes());
                continue;
            }
            other => {
                out.push(b'%');
                out.push(other);
                continue;
            }
        };
        out.extend(fmt_secs(d, prec, long).as_bytes());
    }
    out
}

fn fmt_secs(d: Duration, prec: usize, long: bool) -> String {
    let total_ms = d.as_millis() as u64;
    let secs = total_ms / 1000;
    let frac = total_ms % 1000;
    let frac_s = match prec {
        0 => String::new(),
        1 => format!(".{}", frac / 100),
        2 => format!(".{:02}", frac / 10),
        _ => format!(".{frac:03}"),
    };
    if long {
        format!("{}m{}{}s", secs / 60, secs % 60, frac_s)
    } else {
        format!("{secs}{frac_s}")
    }
}

/// Converte bytes em número inteiro como o bash nos builtins (`exit 3`, `shift 2`): espaços em
/// volta permitidos, sinal opcional, só dígitos decimais.
pub fn parse_int(v: &[u8]) -> Option<i64> {
    let s = std::str::from_utf8(v).ok()?;
    let t = s.trim_matches(|c: char| c == ' ' || c == '\t' || c == '\n');
    if t.is_empty() {
        return None;
    }
    let (neg, digits) = match t.as_bytes()[0] {
        b'-' => (true, &t[1..]),
        b'+' => (false, &t[1..]),
        _ => (false, t),
    };
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut n: i64 = 0;
    for c in digits.bytes() {
        n = n.checked_mul(10)?.checked_add((c - b'0') as i64)?;
    }
    Some(if neg { -n } else { n })
}

pub use core::select_loop;
