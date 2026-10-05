//! Helpers do Linux-PAM 1.7 do Debian 13: `mkhomedir_helper`, `pam_timestamp_check`, `unix_chkpwd`,
//! `unix_update`, `pwhistory_helper`, `faillock` e `pam_getenv`.
//!
//! Os binários `unix_chkpwd`, `unix_update` e `pwhistory_helper` só aceitam ser chamados pelo módulo
//! PAM com a senha em um pipe no stdin; fora disso recusam com a mesma mensagem e o código
//! `PAM_SYSTEM_ERR` do original (sem o `sleep(10)` de desencorajamento, que só atrasaria os testes).
//! A verificação de hash exige `crypt(3)`, que o sandbox não tem: só os casos sem hash (campo de senha
//! vazio com `nullok`) ou sem usuário são decididos aqui; senha contra hash real é sempre recusada.
//! O `faillock` lê o diretório de tallies e imprime o cabeçalho do usuário; não decodifica registros.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Fd, sys};

use crate::groupmgmt::{fields, is_data, name_eq, read_lines};
use crate::util::io::{self, File};

const PAM_SYSTEM_ERR: i32 = 4;
const PAM_PERM_DENIED: i32 = 6;
const PAM_AUTH_ERR: i32 = 7;
const PAM_USER_UNKNOWN: i32 = 10;

/// Campos de `/etc/passwd` do usuário com esse nome.
fn pw_by_name(name: &[u8]) -> Option<Vec<Vec<u8>>> {
    let lines = read_lines(b"/etc/passwd").ok()?;
    lines
        .iter()
        .filter(|l| is_data(l))
        .find(|l| name_eq(l, name))
        .map(|l| fields(l).iter().map(|x| x.to_vec()).collect())
}

fn stdin_is_tty() -> bool {
    sys::current().isatty(Fd::STDIN)
}

/// A recusa dos helpers chamados fora do módulo PAM.
fn inappropriate() -> i32 {
    io::eprint(
        "This binary is not designed for running in this way\n-- the system administrator has been informed\n",
    );
    PAM_SYSTEM_ERR
}

// ---------------------------------------------------------------------------------------------
// mkhomedir_helper
// ---------------------------------------------------------------------------------------------

const MKHOMEDIR_USAGE: &str =
    "Usage: mkhomedir_helper <username> [<umask> [<path-to-skel> [<home-mode>]]]\n";

pub fn mkhomedir_helper_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| {
        let argv = io::args_bytes(args);
        if argv.len() < 2 || argv.len() > 5 {
            io::eprint(MKHOMEDIR_USAGE);
            return PAM_SYSTEM_ERR;
        }
        let Some(pw) = pw_by_name(&argv[1]) else {
            return PAM_USER_UNKNOWN;
        };
        let home = pw.get(5).cloned().unwrap_or_default();
        // Diretório pessoal já existente: nada a fazer.
        if File::open(&home).is_ok() {
            return 0;
        }
        let skel = argv.get(3).cloned().unwrap_or_else(|| b"/etc/skel".to_vec());
        if File::open(&skel).is_err() {
            return PAM_PERM_DENIED;
        }
        // Criar o diretório e copiar o skel não é possível aqui: mesma falha de quem não consegue.
        PAM_PERM_DENIED
    })
}

// ---------------------------------------------------------------------------------------------
// pam_timestamp_check
// ---------------------------------------------------------------------------------------------

const TIMESTAMP_USAGE: &str = "Usage: pam_timestamp_check [-k] [-d] [target user]\n";

pub fn pam_timestamp_check_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| {
        let argv = io::args_bytes(args);
        let mut operands = 0;
        let mut done_opts = false;
        for a in &argv[1..] {
            if !done_opts && a.as_slice() == b"--" {
                done_opts = true;
            } else if !done_opts && a.len() > 1 && a[0] == b'-' {
                for &c in &a[1..] {
                    if c != b'k' && c != b'd' {
                        io::eprint(format!("pam_timestamp_check: invalid option -- '{}'\n", c as char));
                        io::eprint(TIMESTAMP_USAGE);
                        return 2;
                    }
                }
            } else {
                operands += 1;
            }
        }
        if operands > 1 {
            io::eprint(TIMESTAMP_USAGE);
            return 2;
        }
        // Sem timestamp gravado para o terminal: não autenticado.
        PAM_AUTH_ERR
    })
}

// ---------------------------------------------------------------------------------------------
// unix_chkpwd, unix_update, pwhistory_helper
// ---------------------------------------------------------------------------------------------

pub fn unix_chkpwd_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| {
        let argv = io::args_bytes(args);
        if stdin_is_tty() || argv.len() != 3 {
            return inappropriate();
        }
        let user = &argv[1];
        let nullok = argv[2].as_slice() == b"nullok";
        let data = io::read_stdin().unwrap_or_default();
        let pass: &[u8] = data.split(|b| *b == 0).next().unwrap_or(&[]);
        let hash = read_lines(b"/etc/shadow")
            .ok()
            .and_then(|lines| {
                lines
                    .iter()
                    .filter(|l| is_data(l))
                    .find(|l| name_eq(l, user))
                    .map(|l| fields(l).get(1).map(|x| x.to_vec()).unwrap_or_default())
            })
            .or_else(|| pw_by_name(user).map(|f| f.get(1).cloned().unwrap_or_default()));
        match hash {
            Some(h) if h.is_empty() && nullok && pass.is_empty() => 0,
            _ => 1,
        }
    })
}

pub fn unix_update_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| {
        let argv = io::args_bytes(args);
        if stdin_is_tty() || argv.len() != 5 {
            return inappropriate();
        }
        PAM_AUTH_ERR
    })
}

pub fn pwhistory_helper_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| {
        let argv = io::args_bytes(args);
        if stdin_is_tty() || argv.len() != 4 {
            return inappropriate();
        }
        PAM_AUTH_ERR
    })
}

// ---------------------------------------------------------------------------------------------
// faillock
// ---------------------------------------------------------------------------------------------

const FAILLOCK_USAGE: &str = "Usage: faillock [--dir /path/to/tally-directory] [--user username] [--reset] [--legacy-output]\n";

pub fn faillock_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| {
        let argv = io::args_bytes(args);
        let mut dir: Vec<u8> = b"/var/run/faillock".to_vec();
        let mut user: Option<Vec<u8>> = None;
        let mut reset = false;
        let mut i = 1;
        while i < argv.len() {
            match argv[i].as_slice() {
                b"--dir" | b"--user" | b"--conf" => {
                    i += 1;
                    if i >= argv.len() {
                        io::eprint(FAILLOCK_USAGE);
                        return 2;
                    }
                    match argv[i - 1].as_slice() {
                        b"--dir" => dir = argv[i].clone(),
                        b"--user" => user = Some(argv[i].clone()),
                        _ => {}
                    }
                }
                b"--reset" => reset = true,
                b"--legacy-output" => {}
                _ => {
                    io::eprint(FAILLOCK_USAGE);
                    return 2;
                }
            }
            i += 1;
        }

        match user {
            Some(u) => {
                if pw_by_name(&u).is_none() {
                    io::eprint(format!("faillock: No such user {}\n", io::lossy(&u)));
                    return 3;
                }
                if reset {
                    return 0;
                }
                let mut out = io::stdout();
                let _ = out.write_all(format!("{}:\n", io::lossy(&u)).as_bytes());
                let _ = out.write_all(
                    format!("{:<19} {:<5} {:<48} {:<5}\n", "When", "Type", "Source", "Valid").as_bytes(),
                );
                0
            }
            None => match File::open(&dir) {
                Ok(_) => 0,
                Err(e) => {
                    io::eprint(format!("faillock: Error opening the tally directory: {}\n", e.message()));
                    3
                }
            },
        }
    })
}

// ---------------------------------------------------------------------------------------------
// pam_getenv
// ---------------------------------------------------------------------------------------------

const GETENV_USAGE: &str = "Usage: pam_getenv [-l] [-s service] [name ...]\n";

pub fn pam_getenv_main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| {
        let argv = io::args_bytes(args);
        if argv.len() < 2 {
            io::eprint(GETENV_USAGE);
            return 1;
        }
        for name in &argv[1..] {
            if name.len() > 1 && name[0] == b'-' {
                io::eprint(format!("pam_getenv: invalid option -- '{}'\n", name[1] as char));
                io::eprint(GETENV_USAGE);
                return 1;
            }
        }
        // Ambiente da sessão PAM: vazio fora de uma sessão, então nenhuma variável existe.
        1
    })
}
