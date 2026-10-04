//! Editor e hooks: processos filhos pelo `sysio::process::Command` (busca no PATH como o
//! `execvp`). Valor sem metacaractere de shell roda direto; com metacaractere roda por
//! `sh -c '<valor> "$@"' <valor> <arquivo>`, como descrito em git-var(1) e no comportamento do
//! oráculo.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;

use crate::config::Config;
use crate::error::{Fail, R, error};
use crate::os;

const SHELL_META: &[u8] = b"|&;<>()$`\\\"' \t\n*?[#~=%";

/// Monta o comando: direto, ou pelo shell.
fn command(prog: &[u8], args: &[&[u8]]) -> sysio::process::Command {
    if prog.iter().any(|c| SHELL_META.contains(c)) {
        let mut script = prog.to_vec();
        if !args.is_empty() {
            script.extend_from_slice(b" \"$@\"");
        }
        let mut c = sysio::process::Command::new("/bin/sh");
        c.arg("-c").arg(OsStr::from_bytes(&script)).arg(OsStr::from_bytes(prog));
        for a in args {
            c.arg(OsStr::from_bytes(a));
        }
        c
    } else {
        let mut c = sysio::process::Command::new(OsStr::from_bytes(prog));
        for a in args {
            c.arg(OsStr::from_bytes(a));
        }
        c
    }
}

/// Abre o editor em `path`. `Err` já com as mensagens do git impressas.
pub fn edit_file(cfg: &Config, path: &[u8]) -> R<()> {
    let Some(ed) = crate::cmd::misc::editor(cfg) else {
        error("Terminal is dumb, but EDITOR unset");
        return Err(Fail::Exit(1));
    };
    if ed == b":" {
        return Ok(());
    }
    os::flush_out();
    let name = os::lossy(&ed);
    match command(&ed, &[path]).status() {
        Ok(st) if st.success() => Ok(()),
        Ok(st) => {
            if st.code() == Some(127) && !ed.iter().any(|c| SHELL_META.contains(c)) {
                error(&format!("cannot run {name}: No such file or directory"));
                error(&format!("unable to start editor '{name}'"));
            } else {
                error(&format!("There was a problem with the editor '{name}'."));
            }
            Err(Fail::Exit(1))
        }
        Err(e) => {
            let msg = sysio::errno::strerror(&e);
            error(&format!("cannot run {name}: {msg}"));
            error(&format!("unable to start editor '{name}'"));
            Err(Fail::Exit(1))
        }
    }
}

/// Roda um hook se ele existir e for executável. `Ok(None)` se não há hook.
pub fn run_hook(hooks_dir: &[u8], name: &str, args: &[&[u8]], env: &[(&str, &[u8])]) -> R<Option<i32>> {
    let path = os::join(hooks_dir, name.as_bytes());
    let Ok(st) = os::stat(&path) else { return Ok(None) };
    if st.mode & 0o111 == 0 {
        if !name.is_empty() {
            crate::error::hint(&format!(
                "The '{}' hook was ignored because it's not set as executable.\nYou can disable this warning with `git config advice.ignoredHook false`.",
                os::lossy(&path)
            ));
        }
        return Ok(None);
    }
    os::flush_out();
    let mut c = sysio::process::Command::new(OsStr::from_bytes(&path));
    for a in args {
        c.arg(OsStr::from_bytes(a));
    }
    for (k, v) in env {
        c.env(k, OsStr::from_bytes(v));
    }
    match c.status() {
        Ok(st) => Ok(Some(st.code().unwrap_or(1))),
        Err(e) => Err(Fail::Fatal(format!("cannot exec '{}': {}", os::lossy(&path), sysio::errno::strerror(&e)))),
    }
}
