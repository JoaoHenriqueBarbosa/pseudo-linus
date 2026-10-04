//! `cd`, `pwd`, `pushd`, `popd`, `dirs`.

use sysabi::{AtFlags, Fd, FileType};

use super::{out, parse_int};
use crate::shell::{Exec, Shell, sys};
use crate::vars::Attrs;

/// Normaliza um caminho absoluto lexicamente (`.`, `..`, barras repetidas), como o `cd -L`.
pub fn canonicalize_logical(path: &[u8]) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = Vec::new();
    for c in path.split(|b| *b == b'/') {
        match c {
            b"" | b"." => {}
            b".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    if parts.is_empty() {
        return b"/".to_vec();
    }
    let mut out = Vec::new();
    for p in parts {
        out.push(b'/');
        out.extend_from_slice(p);
    }
    out
}

impl Shell {
    pub fn current_pwd(&self) -> Vec<u8> {
        match self.var_bytes("PWD") {
            Some(p) if p.starts_with(b"/") => p.to_vec(),
            _ => sys().getcwd().unwrap_or_else(|_| b"/".to_vec()),
        }
    }

    /// Troca de diretório e atualiza PWD/OLDPWD. `physical` resolve symlinks.
    pub fn change_dir(&mut self, target: &[u8], physical: bool) -> Result<(), sysabi::Errno> {
        let s = sys();
        let old = self.current_pwd();
        let logical = if target.starts_with(b"/") {
            canonicalize_logical(target)
        } else {
            let mut p = old.clone();
            p.push(b'/');
            p.extend_from_slice(target);
            canonicalize_logical(&p)
        };
        let new_pwd = if physical || self.opts.get("physical") {
            s.chdir(target)?;
            s.getcwd()?
        } else {
            match s.chdir(&logical) {
                Ok(()) => logical,
                Err(_) => {
                    s.chdir(target)?;
                    s.getcwd()?
                }
            }
        };
        let set = |sh: &mut Shell, name: &str, v: Vec<u8>| {
            if sh.vars.get(name).is_some_and(|x| x.attrs.has(Attrs::READONLY)) {
                return;
            }
            let _ = sh.assign_scalar(name, v, false);
        };
        set(self, "OLDPWD", old);
        set(self, "PWD", new_pwd);
        Ok(())
    }

    fn dir_error(&self, builtin: &str, shown: &[u8], e: sysabi::Errno) {
        let mut msg = format!("{builtin}: ").into_bytes();
        msg.extend_from_slice(shown);
        msg.extend_from_slice(b": ");
        msg.extend_from_slice(e.message().as_bytes());
        self.error_bytes(&msg);
    }

    /// Pilha completa, topo primeiro (o topo é o diretório corrente).
    pub fn dir_stack(&self) -> Vec<Vec<u8>> {
        let mut v = vec![self.current_pwd()];
        v.extend(self.dirstack.iter().rev().cloned());
        v
    }

    fn tilde_home(&self, p: &[u8]) -> Vec<u8> {
        if let Some(home) = self.var_bytes("HOME") {
            if !home.is_empty() && home != b"/" && p.starts_with(home) && (p.len() == home.len() || p[home.len()] == b'/') {
                let mut v = b"~".to_vec();
                v.extend_from_slice(&p[home.len()..]);
                return v;
            }
        }
        p.to_vec()
    }
}

fn is_dir(p: &[u8]) -> bool {
    sys().fstatat(Fd::CWD, p, AtFlags::empty()).is_ok_and(|st| st.file_type() == FileType::Directory)
}

pub fn cd(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let mut physical = false;
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if a == b"--" {
            i += 1;
            break;
        }
        if a.len() > 1 && a[0] == b'-' && a[1..].iter().all(|c| matches!(c, b'L' | b'P' | b'e' | b'@')) {
            for c in &a[1..] {
                match c {
                    b'P' => physical = true,
                    b'L' => physical = false,
                    _ => {}
                }
            }
            i += 1;
            continue;
        }
        break;
    }
    let rest = &argv[i..];
    if rest.len() > 1 {
        sh.builtin_error("cd", "too many arguments");
        return Ok(1);
    }
    let (target, print) = match rest.first() {
        None => match sh.var_bytes("HOME") {
            Some(h) => (h.to_vec(), false),
            None => {
                sh.builtin_error("cd", "HOME not set");
                return Ok(1);
            }
        },
        Some(d) if d == b"-" => match sh.var_bytes("OLDPWD") {
            Some(o) if !o.is_empty() => (o.to_vec(), true),
            _ => {
                sh.builtin_error("cd", "OLDPWD not set");
                return Ok(1);
            }
        },
        Some(d) => (d.clone(), false),
    };
    if target.is_empty() {
        return Ok(0);
    }
    // CDPATH pra nomes relativos que não começam com `.`/`..`.
    let mut print = print;
    let mut resolved = target.clone();
    let dot_rel = target.starts_with(b"./") || target.starts_with(b"../") || target == b"." || target == b"..";
    if !target.starts_with(b"/") && !dot_rel {
        if let Some(cdpath) = sh.var_bytes("CDPATH").map(|v| v.to_vec()) {
            for dir in cdpath.split(|c| *c == b':') {
                let mut cand = if dir.is_empty() { b".".to_vec() } else { dir.to_vec() };
                cand.push(b'/');
                cand.extend_from_slice(&target);
                if is_dir(&cand) {
                    if !dir.is_empty() {
                        print = true;
                    }
                    resolved = cand;
                    break;
                }
            }
        }
    }
    match sh.change_dir(&resolved, physical) {
        Ok(()) => {
            if print {
                let mut p = sh.current_pwd();
                p.push(b'\n');
                out(sh, "cd", &p);
            }
            Ok(0)
        }
        Err(e) => {
            sh.dir_error("cd", &target, e);
            Ok(1)
        }
    }
}

pub fn pwd(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let mut physical = sh.opts.get("physical");
    for a in &argv[1..] {
        match a.as_slice() {
            b"-P" => physical = true,
            b"-L" => physical = false,
            b"--" => break,
            other if other.starts_with(b"-") => {
                sh.builtin_error("pwd", format!("{}: invalid option", String::from_utf8_lossy(&other[..2.min(other.len())])));
                let _ = crate::shell::write_fd(Fd::STDERR, b"pwd: usage: pwd [-LP]\n");
                return Ok(2);
            }
            _ => {}
        }
    }
    let p = if physical {
        sys().getcwd()
    } else {
        let pwd = sh.current_pwd();
        let s = sys();
        // PWD só vale se ainda for o diretório corrente.
        match (s.fstatat(Fd::CWD, &pwd, AtFlags::empty()), s.fstatat(Fd::CWD, b".", AtFlags::empty())) {
            (Ok(a), Ok(b)) if a.ino == b.ino && a.dev == b.dev => Ok(pwd),
            _ => s.getcwd(),
        }
    };
    match p {
        Ok(mut p) => {
            p.push(b'\n');
            Ok(if out(sh, "pwd", &p) { 0 } else { 1 })
        }
        Err(e) => {
            sh.builtin_error("pwd", format!("error retrieving current directory: getcwd: cannot access parent directories: {}", e.message()));
            Ok(1)
        }
    }
}

fn print_dirs(sh: &Shell, long: bool, per_line: bool, verbose: bool) -> Vec<u8> {
    let stack = sh.dir_stack();
    let mut out = Vec::new();
    for (i, d) in stack.iter().enumerate() {
        let shown = if long { d.clone() } else { sh.tilde_home(d) };
        if verbose {
            out.extend_from_slice(format!("{i:2}  ").as_bytes());
            out.extend(shown);
            out.push(b'\n');
        } else if per_line {
            out.extend(shown);
            out.push(b'\n');
        } else {
            if i > 0 {
                out.push(b' ');
            }
            out.extend(shown);
        }
    }
    if !verbose && !per_line {
        out.push(b'\n');
    }
    out
}

pub fn dirs(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let (mut long, mut per_line, mut verbose, mut clear) = (false, false, false, false);
    let mut index: Option<(bool, usize)> = None;
    for a in &argv[1..] {
        if (a.starts_with(b"+") || a.starts_with(b"-")) && a.len() > 1 && a[1].is_ascii_digit() {
            let n = parse_int(&a[1..]).unwrap_or(0) as usize;
            index = Some((a[0] == b'+', n));
            continue;
        }
        if a.starts_with(b"-") {
            for c in &a[1..] {
                match c {
                    b'l' => long = true,
                    b'p' => per_line = true,
                    b'v' => {
                        verbose = true;
                        per_line = true;
                    }
                    b'c' => clear = true,
                    _ => {
                        sh.builtin_error("dirs", format!("-{}: invalid option", *c as char));
                        let _ = crate::shell::write_fd(Fd::STDERR, b"dirs: usage: dirs [-clpv] [+N] [-N]\n");
                        return Ok(2);
                    }
                }
            }
        }
    }
    if clear {
        sh.dirstack.clear();
        return Ok(0);
    }
    if let Some((from_top, n)) = index {
        let stack = sh.dir_stack();
        let idx = if from_top { Some(n) } else { stack.len().checked_sub(n + 1) };
        match idx.and_then(|i| stack.get(i)) {
            Some(d) => {
                let mut v = if long { d.clone() } else { sh.tilde_home(d) };
                v.push(b'\n');
                out(sh, "dirs", &v);
                return Ok(0);
            }
            None => {
                sh.builtin_error("dirs", format!("{}{n}: directory stack index out of range", if from_top { "+" } else { "-" }));
                return Ok(1);
            }
        }
    }
    let text = print_dirs(sh, long, per_line, verbose);
    Ok(if out(sh, "dirs", &text) { 0 } else { 1 })
}

pub fn pushd(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let mut no_cd = false;
    let mut target: Option<Vec<u8>> = None;
    for a in &argv[1..] {
        if a == b"-n" {
            no_cd = true;
        } else if target.is_none() {
            target = Some(a.clone());
        }
    }
    let cur = sh.current_pwd();
    match target {
        None => {
            // Troca os dois do topo.
            let Some(second) = sh.dirstack.pop() else {
                sh.builtin_error("pushd", "no other directory");
                return Ok(1);
            };
            if let Err(e) = sh.change_dir(&second, false) {
                sh.dir_error("pushd", &second, e);
                sh.dirstack.push(second);
                return Ok(1);
            }
            sh.dirstack.push(cur);
        }
        Some(t) if (t.starts_with(b"+") || t.starts_with(b"-")) && t.len() > 1 && t[1..].iter().all(|c| c.is_ascii_digit()) => {
            let n = parse_int(&t[1..]).unwrap_or(0) as usize;
            let mut stack = sh.dir_stack();
            let idx = if t[0] == b'+' { Some(n) } else { stack.len().checked_sub(n + 1) };
            let Some(idx) = idx.filter(|i| *i < stack.len()) else {
                sh.builtin_error("pushd", format!("{}: directory stack index out of range", String::from_utf8_lossy(&t)));
                return Ok(1);
            };
            stack.rotate_left(idx);
            let top = stack[0].clone();
            if let Err(e) = sh.change_dir(&top, false) {
                sh.dir_error("pushd", &top, e);
                return Ok(1);
            }
            sh.dirstack = stack[1..].iter().rev().cloned().collect();
        }
        Some(t) => {
            if no_cd {
                sh.dirstack.push(t);
            } else {
                if let Err(e) = sh.change_dir(&t, false) {
                    sh.dir_error("pushd", &t, e);
                    return Ok(1);
                }
                sh.dirstack.push(cur);
            }
        }
    }
    let text = print_dirs(sh, false, false, false);
    out(sh, "pushd", &text);
    Ok(0)
}

pub fn popd(sh: &mut Shell, argv: &[Vec<u8>]) -> Exec {
    let mut no_cd = false;
    let mut index: Option<(bool, usize)> = None;
    for a in &argv[1..] {
        if a == b"-n" {
            no_cd = true;
        } else if (a.starts_with(b"+") || a.starts_with(b"-")) && a.len() > 1 && a[1..].iter().all(|c| c.is_ascii_digit()) {
            index = Some((a[0] == b'+', parse_int(&a[1..]).unwrap_or(0) as usize));
        }
    }
    if sh.dirstack.is_empty() {
        sh.builtin_error("popd", "directory stack empty");
        return Ok(1);
    }
    let mut stack = sh.dir_stack();
    let idx = match index {
        None => 0,
        Some((true, n)) => n,
        Some((false, n)) => match stack.len().checked_sub(n + 1) {
            Some(i) => i,
            None => {
                sh.builtin_error("popd", format!("-{n}: directory stack index out of range"));
                return Ok(1);
            }
        },
    };
    if idx >= stack.len() {
        sh.builtin_error("popd", format!("+{idx}: directory stack index out of range"));
        return Ok(1);
    }
    stack.remove(idx);
    if idx == 0 && !no_cd {
        let top = stack[0].clone();
        if let Err(e) = sh.change_dir(&top, false) {
            sh.dir_error("popd", &top, e);
            return Ok(1);
        }
    }
    sh.dirstack = stack[1..].iter().rev().cloned().collect();
    let text = print_dirs(sh, false, false, false);
    out(sh, "popd", &text);
    Ok(0)
}
