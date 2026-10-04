//! `set` e `shopt`.

use sysabi::Fd;

use super::out;
use crate::options::{Options, SET_OPTIONS, SHOPT_OPTIONS};
use crate::shell::{Exec, Shell, write_fd};

const SET_USAGE: &[u8] = b"set: usage: set [-abefhkmnptuvxBCEHPT] [-o option-name] [--] [-] [arg ...]\n";

fn list_set_o(sh: &Shell, plus_form: bool) -> Vec<u8> {
    let mut text = String::new();
    for (i, (name, _)) in SET_OPTIONS.iter().enumerate() {
        let on = sh.opts.get_index(i);
        if plus_form {
            text.push_str(&format!("set {}o {name}\n", if on { '-' } else { '+' }));
        } else {
            text.push_str(&format!("{name:<15}\t{}\n", if on { "on" } else { "off" }));
        }
    }
    text.into_bytes()
}

/// Liga/desliga uma opção do `set -o`, com os efeitos colaterais.
fn set_named(sh: &mut Shell, name: &str, on: bool) -> bool {
    if name == "posix" {
        sh.posix = on;
    }
    sh.opts.set(name, on)
}

pub fn set(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    if argv.len() == 1 {
        let mut text = Vec::new();
        for (n, v) in sh.vars.visible() {
            if let Some(l) = super::declare::set_line(&n, v) {
                text.extend(l);
                text.push(b'\n');
            }
        }
        let mut names: Vec<&String> = sh.funcs.keys().collect();
        names.sort();
        for f in names {
            text.extend(crate::print::function_text(&sh.funcs[f]).into_bytes());
            text.push(b'\n');
        }
        return Ok(if out(sh, "set", &text) { 0 } else { 1 });
    }
    let mut i = 1;
    let mut set_params: Option<usize> = None;
    while i < argv.len() {
        let a = &argv[i];
        if a == b"--" {
            set_params = Some(i + 1);
            break;
        }
        if a == b"-" {
            sh.opts.set("xtrace", false);
            sh.opts.set("verbose", false);
            set_params = Some(i + 1);
            break;
        }
        if a.len() < 2 || (a[0] != b'-' && a[0] != b'+') {
            set_params = Some(i);
            break;
        }
        let on = a[0] == b'-';
        let mut j = 1;
        while j < a.len() {
            let c = a[j];
            if c == b'o' {
                // `-o nome` ou `-o` sozinho (lista).
                match argv.get(i + 1) {
                    Some(n) if j + 1 == a.len() => {
                        let name = String::from_utf8_lossy(n).into_owned();
                        if !set_named(sh, &name, on) {
                            sh.builtin_error("set", format!("{name}: invalid option name"));
                            return Ok(2);
                        }
                        i += 1;
                    }
                    _ => {
                        let text = list_set_o(sh, !on);
                        out(sh, "set", &text);
                    }
                }
                j += 1;
                continue;
            }
            match Options::letter_index(c) {
                Some(idx) => {
                    let name = SET_OPTIONS[idx].0;
                    set_named(sh, name, on);
                }
                None => {
                    sh.builtin_error("set", format!("{}{}: invalid option", a[0] as char, c as char));
                    let _ = write_fd(Fd::STDERR, SET_USAGE);
                    return Ok(2);
                }
            }
            j += 1;
        }
        i += 1;
    }
    if let Some(start) = set_params {
        sh.params = argv[start.min(argv.len())..].to_vec();
    }
    Ok(0)
}

pub fn shopt(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let (mut s, mut u, mut q, mut p, mut o) = (false, false, false, false, false);
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if a == b"--" {
            i += 1;
            break;
        }
        if a.len() < 2 || a[0] != b'-' {
            break;
        }
        for c in &a[1..] {
            match c {
                b's' => s = true,
                b'u' => u = true,
                b'q' => q = true,
                b'p' => p = true,
                b'o' => o = true,
                _ => {
                    sh.builtin_error("shopt", format!("-{}: invalid option", *c as char));
                    let _ = write_fd(Fd::STDERR, b"shopt: usage: shopt [-pqsu] [-o] [optname ...]\n");
                    return Ok(2);
                }
            }
        }
        i += 1;
    }
    let names = &argv[i..];
    if s && u {
        sh.builtin_error("shopt", "cannot set and unset shell options simultaneously");
        return Ok(1);
    }
    let get = |sh: &Shell, n: &str| -> Option<bool> {
        if o {
            Options::set_index(n).map(|idx| sh.opts.get_index(idx))
        } else {
            Options::shopt_index(n).map(|idx| sh.opts.shopt_index_value(idx))
        }
    };
    let line = |n: &str, on: bool, pform: bool| -> String {
        if pform {
            if o {
                format!("set {}o {n}\n", if on { '-' } else { '+' })
            } else {
                format!("shopt {} {n}\n", if on { "-s" } else { "-u" })
            }
        } else {
            format!("{n:<15}\t{}\n", if on { "on" } else { "off" })
        }
    };
    if s || u {
        if names.is_empty() {
            // Lista as que estão no estado pedido.
            let mut text = String::new();
            let all: Vec<&str> = if o { SET_OPTIONS.iter().map(|x| x.0).collect() } else { SHOPT_OPTIONS.iter().map(|x| x.0).collect() };
            for n in all {
                let on = get(sh, n).unwrap_or(false);
                if on == s {
                    text.push_str(&line(n, on, p));
                }
            }
            if !q {
                out(sh, "shopt", text.as_bytes());
            }
            return Ok(0);
        }
        let mut status = 0;
        for n in names {
            let name = String::from_utf8_lossy(n).into_owned();
            let ok = if o { set_named(sh, &name, s) } else { sh.opts.set_shopt(&name, s) };
            if name == "expand_aliases" {
                // Muda o parse dos próximos comandos.
                sh.parse_generation += 1;
            }
            if !ok {
                sh.builtin_error("shopt", format!("{name}: invalid {}option name", if o { "" } else { "shell " }));
                status = 1;
            }
        }
        return Ok(status);
    }
    let mut status = 0;
    let mut text = String::new();
    if names.is_empty() {
        let all: Vec<&str> = if o { SET_OPTIONS.iter().map(|x| x.0).collect() } else { SHOPT_OPTIONS.iter().map(|x| x.0).collect() };
        for n in all {
            let on = get(sh, n).unwrap_or(false);
            text.push_str(&line(n, on, p));
        }
    } else {
        for n in names {
            let name = String::from_utf8_lossy(n).into_owned();
            match get(sh, &name) {
                Some(on) => {
                    if !on {
                        status = 1;
                    }
                    text.push_str(&line(&name, on, p));
                }
                None => {
                    sh.builtin_error("shopt", format!("{name}: invalid {}option name", if o { "" } else { "shell " }));
                    status = 1;
                }
            }
        }
    }
    if !q {
        out(sh, "shopt", text.as_bytes());
    }
    Ok(status)
}
