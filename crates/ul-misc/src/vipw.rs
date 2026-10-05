//! `vipw` e `vigr` do shadow 4.17 (Debian 13): o mesmo binário, escolhido pelo nome do argv0.
//!
//! Opções, mensagens e códigos de saída do original sobre `/etc/{passwd,shadow,group,gshadow}` (sob
//! `-R`, com o prefixo; o `chroot` real não existe no sandbox). O editor é o de `$VISUAL`, `$EDITOR`
//! ou `vi`, chamado por `sh -c`. A trava é o arquivo `NOME.lock`, criado e removido como no original.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{AtFlags, Fd, OFlags, ProcAttrs, WaitOptions, WaitStatus, WaitTarget, sys};

use crate::groupmgmt::{Spec, join, parse};
use crate::setsid::execvp;
use crate::util::io::{self, File};
use crate::util::ul;

fn usage_text(prog: &str) -> String {
    format!(
        "Usage: {prog} [options]\n\nOptions:\n  -g, --group                   edit group database\n  -h, --help                    display this help message and exit\n  -p, --passwd                  edit passwd database\n  -q, --quiet                   quiet mode\n  -R, --root CHROOT_DIR         directory to chroot into\n  -s, --shadow                  edit shadow or gshadow database\n\n"
    )
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn write_bytes(path: &[u8], data: &[u8]) -> bool {
    match File::open_with(path, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, 0o644) {
        Ok(mut f) => f.write_all(data).is_ok(),
        Err(_) => false,
    }
}

fn unlink(path: &[u8]) {
    let _ = sys::current().unlinkat(Fd::CWD, path, AtFlags::empty());
}

fn run(args: &[OsString]) -> i32 {
    let prog = ul::short_name(args).to_string();
    let argv = io::args_bytes(args);
    let mut edit_group = prog == "vigr";
    let mut shadow = false;
    let mut quiet = false;
    let spec: Spec = &[
        (b'g', "group", false),
        (b'h', "help", false),
        (b'p', "passwd", false),
        (b'q', "quiet", false),
        (b'R', "root", true),
        (b's', "shadow", false),
    ];
    let Some(o) = parse(&prog, &argv, spec) else {
        io::eprint(usage_text(&prog));
        return 1;
    };
    for (k, _) in &o.vals {
        match *k {
            b'h' => {
                let _ = io::stdout().write_all(usage_text(&prog).as_bytes());
                return 0;
            }
            b'g' => edit_group = true,
            b'p' => edit_group = false,
            b'q' => quiet = true,
            b's' => shadow = true,
            _ => {}
        }
    }
    if !o.rest.is_empty() {
        io::eprint(usage_text(&prog));
        return 1;
    }
    let prefix = o.get(b'R').unwrap_or_default();
    let name = match (edit_group, shadow) {
        (false, false) => "passwd",
        (false, true) => "shadow",
        (true, false) => "group",
        (true, true) => "gshadow",
    };
    let (other, other_cmd) = match name {
        "passwd" => ("shadow", "vipw -s"),
        "shadow" => ("passwd", "vipw"),
        "group" => ("gshadow", "vigr -s"),
        _ => ("group", "vigr"),
    };
    let path = join(&prefix, &format!("/etc/{name}"));
    let lock = join(&prefix, &format!("/etc/{name}.lock"));
    let edit = join(&prefix, &format!("/etc/{name}.edit"));

    if sys::stat(&join(&prefix, "/etc")).is_err() {
        io::eprint(format!("{prog}: Couldn't lock file: No such file or directory\n"));
        return 5;
    }
    if File::open_with(&lock, OFlags::WRONLY | OFlags::CREAT | OFlags::EXCL, 0o600).is_err() {
        io::eprint(format!("{prog}: Couldn't lock file: File exists\n"));
        return 5;
    }
    let fail = |msg: String| -> i32 {
        unlink(&edit);
        unlink(&lock);
        io::eprint(msg);
        1
    };

    let Ok(data) = io::read_path(&path) else {
        return fail(format!("{prog}: /etc/{name}: No such file or directory\n"));
    };
    if !write_bytes(&edit, &data) {
        return fail(format!("{prog}: {}: Permission denied\n", io::lossy(&edit)));
    }

    let sys = sys::current();
    let editor = sys
        .getenv(b"VISUAL")
        .filter(|e| !e.is_empty())
        .or_else(|| sys.getenv(b"EDITOR").filter(|e| !e.is_empty()))
        .unwrap_or_else(|| b"vi".to_vec());
    let mut cmd = editor.clone();
    cmd.push(b' ');
    cmd.extend_from_slice(&edit);
    let sh_argv = vec![b"sh".to_vec(), b"-c".to_vec(), cmd];
    let _ = io::flush_stdout();
    let body: sysabi::ProcessFn = Box::new(move || {
        let _ = execvp(b"/bin/sh", &sh_argv);
        127
    });
    let child = match sys.spawn_fn(ProcAttrs::default(), b"sh".to_vec(), body) {
        Ok(p) => p,
        Err(_) => return fail(format!("{prog}: fork: Operation not permitted\n")),
    };
    let status = loop {
        match sys.wait4(WaitTarget::Any, WaitOptions::empty()) {
            Ok(Some((p, st))) if p == child => break st,
            Ok(_) | Err(_) => return fail(format!("{prog}: {}: No child processes\n", io::lossy(&editor))),
        }
    };
    if !matches!(status, WaitStatus::Exited(0)) {
        return fail(format!("{prog}: {}: Operation not permitted\n", io::lossy(&editor)));
    }

    let Ok(new_data) = io::read_path(&edit) else {
        return fail(format!("{prog}: {}: No such file or directory\n", io::lossy(&edit)));
    };
    if new_data == data {
        unlink(&edit);
        unlink(&lock);
        let _ = io::stdout().write_all(format!("{prog}: no changes made\n").as_bytes());
        return 0;
    }
    let mut backup = path.clone();
    backup.push(b'-');
    if !write_bytes(&backup, &data) || !write_bytes(&path, &new_data) {
        return fail(format!("{prog}: failed to write /etc/{name}\n"));
    }
    unlink(&edit);
    unlink(&lock);
    if !quiet {
        let _ = io::stdout().write_all(
            format!(
                "You have modified /etc/{name}.\nYou may need to modify /etc/{other} for consistency.\nPlease use the command '{other_cmd}' to do so.\n"
            )
            .as_bytes(),
        );
    }
    0
}
