//! `savelog`, `add-shell` e `remove-shell` do debianutils 5.23 (Debian 13).
//!
//! No Debian os três são scripts `/bin/sh` (copiados byte a byte pra `debscripts/`); aqui cada programa
//! roda o mesmo script no `sh` do sistema com o `$0` que o kernel entregaria (o caminho do programa),
//! então o comportamento, as mensagens e os códigos de saída são os do original, inclusive o que vem
//! dos comandos que o script chama (`gzip`, `date`, `mv`, `chown`, `grep`...). `add-shell` e
//! `remove-shell` agem sobre `$DPKG_ROOT/etc/shells`.

use std::ffi::OsString;

use sysabi::{Ctx, Errno, ProcAttrs, SpawnSpec, WaitOptions, WaitStatus, WaitTarget, sys};

use crate::util::io;

const SAVELOG: &str = include_str!("debscripts/savelog.sh");
const ADD_SHELL: &str = include_str!("debscripts/add-shell.sh");
const REMOVE_SHELL: &str = include_str!("debscripts/remove-shell.sh");

/// O `$0` do script: o `argv[0]` quando tem `/`, senão o caminho em que o programa está instalado
/// (é o que o kernel passa ao interpretador depois que o shell achou o programa no `PATH`).
fn script_name(argv0: &[u8], installed: &str) -> Vec<u8> {
    if argv0.contains(&b'/') {
        argv0.to_vec()
    } else {
        installed.as_bytes().to_vec()
    }
}

/// Roda `script` no `/bin/sh` com `$0` e os argumentos do programa; devolve o código de saída.
fn run_script(args: &[OsString], script: &str, installed: &str, errexit: bool) -> i32 {
    let argv = io::args_bytes(args);
    let mut v: Vec<Vec<u8>> = vec![b"sh".to_vec()];
    if errexit {
        v.push(b"-e".to_vec());
    }
    v.push(b"-c".to_vec());
    v.push(script.as_bytes().to_vec());
    v.push(script_name(&argv[0], installed));
    v.extend(argv[1..].iter().cloned());

    let _ = io::flush_stdout();
    let s = sys::current();
    let pid = match s.spawn(SpawnSpec { path: b"/bin/sh".to_vec(), argv: v, attrs: ProcAttrs::default() }) {
        Ok(p) => p,
        Err(e) => {
            io::eprint(format!("{}: /bin/sh: {}\n", io::lossy(&argv[0]), e.message()));
            return 127;
        }
    };
    loop {
        match s.wait4(WaitTarget::Pid(pid), WaitOptions::empty()) {
            Ok(Some((_, WaitStatus::Exited(c)))) => return c,
            Ok(Some((_, WaitStatus::Signaled { signal, .. }))) => return 128 + signal.0,
            Ok(Some(_)) | Ok(None) => continue,
            Err(Errno::EINTR) => continue,
            Err(_) => return 1,
        }
    }
}

pub fn savelog_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run_script(args, SAVELOG, "/usr/bin/savelog", false))
}

pub fn add_shell_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run_script(args, ADD_SHELL, "/usr/sbin/add-shell", true))
}

pub fn remove_shell_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run_script(args, REMOVE_SHELL, "/usr/sbin/remove-shell", true))
}
