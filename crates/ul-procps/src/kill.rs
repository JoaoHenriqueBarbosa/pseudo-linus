//! `kill` do procps-ng 4.0.4 (`/usr/bin/kill`, não o builtin do bash).
//!
//! Comportamento observado no oráculo e reproduzido aqui:
//!
//! - Antes do getopt, o primeiro argumento com cara de sinal (`-9`, `-KILL`, `-sigterm`, `-RTMIN+1`)
//!   vira o sinal e sai da lista; o argumento de `-s`/`-q` não conta. Número aceito vai de 0 a 93
//!   (limite observado; 65 a 93 chegam ao kernel e dão EINVAL).
//! - Depois disso, um `-<dígitos>` que sobra é tratado como pid negativo (grupo): o kill é feito na
//!   hora e o programa termina sem mensagem, com o status invertido do original (0 se o kill falhou,
//!   1 se deu certo).
//! - `-l [sinal]` e `-L`: lista de 1 a 31 (o 29 é `POLL`); nome desconhecido avisa e sai com 0.
//! - Pid que não é número: `failed to parse argument: 'x'` e sai com 1; falha de kill:
//!   `kill: (pid): <strerror>` e segue, saindo com 1 no fim.
//! - `-q valor` usa `sigqueue` no original; aqui o valor é validado e o sinal sai por `kill`, porque o
//!   contrato não tem `sigqueue` (o valor não chega ao destino).
//!
//! As mensagens de `error(3)` usam o argv[0] inteiro, como o original.

use std::ffi::OsString;

use sysabi::{Ctx, Errno, KillTarget, Signal, sys};
use ul_common::signal::{self, Case, Sig29, Table};
use ul_misc::util::io;

use crate::common::{self, SIGNALS, out, signal_list};

const USAGE: &str = "\nUsage:\n kill [options] <pid> [...]\n\nOptions:\n <pid> [...]            send signal to every <pid> listed\n -<signal>, -s, --signal <signal>\n                        specify the <signal> to be sent\n -q, --queue <value>    integer value to be sent with the signal\n -l, --list=[<signal>]  list all signal names, or convert one to a name\n -L, --table            list all signal names in a nice table\n\n -h, --help     display this help and exit\n -V, --version  output version information and exit\n\nFor more details see kill(1).\n";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage_err() -> i32 {
    io::eprint(USAGE);
    1
}

/// Nome da tabela (com ou sem `SIG`, qualquer caixa, com os apelidos) ou `RTMIN+n` / `RTMAX-n`; o
/// tempo real só em maiúsculas (o original não aceita `rtmax-3` como opção).
fn named(s: &str) -> Option<i32> {
    signal::parse_name(s.as_bytes(), &SIGNALS).or_else(|| signal::parse_realtime(s.as_bytes(), Case::Exact))
}

/// `signal_name_to_number` do procps: [`named`] ou número. -1 quando não reconhece (o kill então dá
/// EINVAL no kernel).
pub fn signal_name_to_number(s: &str) -> i32 {
    if let Some(n) = named(s) {
        return n;
    }
    match common::parse_long(s) {
        Some(n) if (0..=i64::from(i32::MAX)).contains(&n) => n as i32,
        _ => -1,
    }
}

/// O argumento é um `-<sinal>`? Devolve o número.
fn sig_option(arg: &str) -> Option<i32> {
    let body = arg.strip_prefix('-')?;
    if body.is_empty() || body.starts_with('-') {
        return None;
    }
    if body.as_bytes()[0].is_ascii_digit() || body.starts_with('+') {
        let n = common::parse_long(body)?;
        return (0..=93).contains(&n).then_some(n as i32);
    }
    named(body)
}

/// `strtol_or_err`: número inteiro ou a mensagem do `error(3)` (com o errno que sobrou: ENOENT pra
/// texto vazio, ERANGE pra estouro).
fn strtol_or_err(argv0: &str, s: &str, what: &str) -> Result<i64, i32> {
    let fail = |suffix: &str| {
        io::eprint(format!("{argv0}: {what}: '{s}'{suffix}\n"));
        Err(1)
    };
    if s.is_empty() {
        return fail(&format!(": {}", Errno::ENOENT.message()));
    }
    match common::strtol(s) {
        common::Strtol::Ok(v) => Ok(v),
        common::Strtol::Range(_) => fail(&format!(": {}", Errno::ERANGE.message())),
        common::Strtol::Invalid => fail(""),
    }
}

/// Tabela do `-L`: sete por linha, `%2d %-8s`, a sétima sem preenchimento.
pub fn signal_table() -> String {
    let names = signal::standard_names(Sig29::Poll);
    let mut s = String::new();
    for (i, name) in names.iter().enumerate() {
        let n = i + 1;
        if n % 7 == 0 {
            s.push_str(&format!("{n:2} {name}\n"));
        } else {
            s.push_str(&format!("{n:2} {name:<8}"));
        }
    }
    if !names.len().is_multiple_of(7) {
        s.push('\n');
    }
    s
}

/// Só os nomes da tabela, sem os apelidos (`IO`, `IOT`, `CLD` não convertem no `-l`, só como sinal).
const LISTED: Table = Table { aliases: &[], ..SIGNALS };

/// `kill -l <sinal>`: número vira nome, nome vira número.
fn list_one(argv0: &str, arg: &str) {
    if arg.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        if let Some(name) = common::parse_long(arg).and_then(|n| i32::try_from(n).ok()).and_then(|n| signal::standard_name(n, Sig29::Poll)) {
            out(format!("{name}\n"));
            return;
        }
    } else if let Some(n) = signal::parse_name(arg.as_bytes(), &LISTED) {
        out(format!("{n}\n"));
        return;
    }
    io::eprint(format!("{argv0}: unknown signal name {arg}\n"));
}

fn target_of(pid: i32) -> KillTarget {
    match pid {
        -1 => KillTarget::All,
        p if p <= 0 => KillTarget::Group(-p),
        p => KillTarget::Pid(p),
    }
}

fn send(pid: i32, sig: i32) -> Result<(), Errno> {
    sys::current().kill(target_of(pid), Signal(sig))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut rest: Vec<String> = argv[1..].iter().map(|a| String::from_utf8_lossy(a).into_owned()).collect();
    if rest.is_empty() {
        return usage_err();
    }
    // skill_sig_option: o primeiro "-<sinal>" vira o sinal (pulando o argumento de -s e -q).
    let mut signo = Signal::SIGTERM.0;
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        if a == "--" {
            break;
        }
        if a == "-s" || a == "-q" || a == "--signal" || a == "--queue" {
            i += 2;
            continue;
        }
        if let Some(n) = sig_option(a) {
            signo = n;
            rest.remove(i);
            break;
        }
        i += 1;
    }
    // getopt "l::Ls:hVq:" com permutação.
    let mut operands: Vec<String> = Vec::new();
    let mut idx = 0;
    while idx < rest.len() {
        let a = rest[idx].clone();
        idx += 1;
        if a == "--" {
            operands.extend(rest[idx..].iter().cloned());
            break;
        }
        if let Some(long) = a.strip_prefix("--") {
            let (name, val) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            let opts = ["list", "table", "signal", "help", "version", "queue"];
            let matches: Vec<&str> = opts.iter().copied().filter(|o| o.starts_with(name)).collect();
            let opt = if opts.contains(&name) { name } else if matches.len() == 1 { matches[0] } else {
                // Opção longa desconhecida: o getopt deixa optopt em 0 e o original imprime o
                // caractere NUL com "%c".
                io::eprint(format!("{argv0}: invalid argument \0\n"));
                return usage_err();
            };
            match opt {
                "list" => {
                    let v = val.or_else(|| rest.get(idx).filter(|n| !n.starts_with('-')).cloned());
                    match v {
                        Some(s) => list_one(&argv0, &s),
                        None => out(signal_list()),
                    }
                    return 0;
                }
                "table" => {
                    out(signal_table());
                    return 0;
                }
                "help" => {
                    out(USAGE);
                    return 0;
                }
                "version" => {
                    out("kill from procps-ng 4.0.4\n");
                    return 0;
                }
                "signal" | "queue" => {
                    let v = match val {
                        Some(v) => v,
                        None => match rest.get(idx) {
                            Some(v) => {
                                idx += 1;
                                v.clone()
                            }
                            None => {
                                io::eprint(format!("{argv0}: invalid argument {}\n", &opt[..1]));
                                return usage_err();
                            }
                        },
                    };
                    if opt == "signal" {
                        signo = signal_name_to_number(&v);
                    } else if let Err(code) = strtol_or_err(&argv0, &v, "must be an integer value to be passed with the signal.") {
                        return code;
                    }
                }
                _ => unreachable!("tabela de opções longas"),
            }
            continue;
        }
        if a.len() < 2 || !a.starts_with('-') {
            operands.push(a);
            continue;
        }
        // Grupo de opções curtas.
        let body: Vec<char> = a[1..].chars().collect();
        let mut k = 0;
        while k < body.len() {
            let c = body[k];
            k += 1;
            match c {
                'l' => {
                    let attached: String = body[k..].iter().collect();
                    let v = if !attached.is_empty() {
                        Some(attached)
                    } else {
                        rest.get(idx).filter(|n| !n.starts_with('-')).cloned()
                    };
                    match v {
                        Some(s) => list_one(&argv0, &s),
                        None => out(signal_list()),
                    }
                    return 0;
                }
                'L' => {
                    out(signal_table());
                    return 0;
                }
                'h' => {
                    out(USAGE);
                    return 0;
                }
                'V' => {
                    out("kill from procps-ng 4.0.4\n");
                    return 0;
                }
                's' | 'q' => {
                    let attached: String = body[k..].iter().collect();
                    let v = if !attached.is_empty() {
                        k = body.len();
                        attached
                    } else if let Some(v) = rest.get(idx) {
                        idx += 1;
                        v.clone()
                    } else {
                        io::eprint(format!("{argv0}: invalid argument {c}\n"));
                        return usage_err();
                    };
                    if c == 's' {
                        signo = signal_name_to_number(&v);
                    } else if let Err(code) = strtol_or_err(&argv0, &v, "must be an integer value to be passed with the signal.") {
                        return code;
                    }
                }
                d if d.is_ascii_digit() => {
                    // Pid negativo sem "--": kill imediato e fim, com o status invertido do
                    // original e sem mensagem.
                    return match strtol_or_err(&argv0, &a, "failed to parse argument") {
                        Ok(pid) => match send(pid as i32, signo) {
                            Ok(()) => 1,
                            Err(_) => 0,
                        },
                        Err(code) => code,
                    };
                }
                other => {
                    io::eprint(format!("{argv0}: invalid argument {other}\n"));
                    return usage_err();
                }
            }
        }
    }
    if operands.is_empty() {
        return usage_err();
    }
    let mut status = 0;
    for op in &operands {
        let pid = match strtol_or_err(&argv0, op, "failed to parse argument") {
            Ok(v) => v as i32,
            Err(code) => return code,
        };
        if let Err(e) = send(pid, signo) {
            io::eprint(format!("{argv0}: ({pid}): {}\n", e.message()));
            status = 1;
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_procps() {
        let t = signal_table();
        assert!(t.starts_with(" 1 HUP      2 INT      3 QUIT"));
        assert!(t.contains(" 7 BUS\n 8 FPE"));
        assert!(t.ends_with("29 POLL    30 PWR     31 SYS     \n"));
        assert_eq!(sig_option("-9"), Some(9));
        assert_eq!(sig_option("-93"), Some(93));
        assert_eq!(sig_option("-94"), None);
        assert_eq!(sig_option("-sigusr1"), Some(10));
        assert_eq!(sig_option("-RTMIN+1"), Some(35));
        assert_eq!(sig_option("-rtmax-3"), None);
        assert_eq!(signal_name_to_number("xyz"), -1);
    }
}
