//! `su` do util-linux 2.41 (e o miolo compartilhado com `runuser`).
//!
//! Porte do `login-utils/su-common.c` sem terminal: opções, `--help`/`--version`, busca do usuário
//! em `/etc/passwd`, `init_groups` (exatamente `-g`/`-G`, ou os grupos do `/etc/group`), o
//! `modify_environment` (com `-w` e os PATHs do `/etc/login.defs`), o pai que espera o shell como o
//! `create_watching_parent`, e a troca de credenciais de verdade no filho. Quem não é root cai no
//! PAM como no contêiner do Debian: nenhuma senha passa. Ficam de fora o pty do `-P` e o resto da
//! sessão de PAM (só o `MAIL` do `pam_mail` é reproduzido).

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use sysabi::{Errno, ProcAttrs, WaitOptions, WaitStatus, WaitTarget, sys};

use crate::groupmgmt::{Spec, fields, is_data, name_eq, parse, parse_id, read_lines};
use crate::sg::lookup_group;
use crate::util::{io, ul};

const SU_USAGE: &str = r#"
Usage:
 su [options] [-] [<user> [<argument>...]]

Change the effective user ID and group ID to that of <user>.
A mere - implies -l.  If <user> is not given, root is assumed.

Options:
 -m, -p, --preserve-environment      do not reset environment variables
 -w, --whitelist-environment <list>  don't reset specified variables

 -g, --group <group>             specify the primary group
 -G, --supp-group <group>        specify a supplemental group

 -, -l, --login                  make the shell a login shell
 -c, --command <command>         pass a single command to the shell with -c
 --session-command <command>     pass a single command to the shell with -c
                                   and do not create a new session
 -f, --fast                      pass -f to the shell (for csh or tcsh)
 -s, --shell <shell>             run <shell> if /etc/shells allows it
 -P, --pty                       create a new pseudo-terminal
 -T, --no-pty                    do not create a new pseudo-terminal (bad security!)

 -h, --help                      display this help
 -V, --version                   display version

For more details see su(1).
"#;

const RUNUSER_USAGE: &str = r#"
Usage:
 runuser [options] -u <user> [[--] <command>]
 runuser [options] [-] [<user> [<argument>...]]

Run <command> with the effective user ID and group ID of <user>.  If -u is
not given, fall back to su(1)-compatible semantics and execute standard shell.
The options -c, -f, -l, and -s are mutually exclusive with -u.

Options:
 -u, --user <user>               username
 -m, -p, --preserve-environment      do not reset environment variables
 -w, --whitelist-environment <list>  don't reset specified variables

 -g, --group <group>             specify the primary group
 -G, --supp-group <group>        specify a supplemental group

 -, -l, --login                  make the shell a login shell
 -c, --command <command>         pass a single command to the shell with -c
 --session-command <command>     pass a single command to the shell with -c
                                   and do not create a new session
 -f, --fast                      pass -f to the shell (for csh or tcsh)
 -s, --shell <shell>             run <shell> if /etc/shells allows it
 -P, --pty                       create a new pseudo-terminal
 -T, --no-pty                    do not create a new pseudo-terminal (bad security!)

 -h, --help                      display this help
 -V, --version                   display version


For more details see runuser(1).
"#;

const SU_SPEC: Spec = &[
    (b'c', "command", true),
    (1, "session-command", true),
    (b'f', "fast", false),
    (b'g', "group", true),
    (b'G', "supp-group", true),
    (b'l', "login", false),
    (b'm', "preserve-environment", false),
    (b'p', "", false),
    (b'P', "pty", false),
    (b's', "shell", true),
    (b'h', "help", false),
    (b'V', "version", false),
    (b'w', "whitelist-environment", true),
];

const RUNUSER_SPEC: Spec = &[
    (b'c', "command", true),
    (1, "session-command", true),
    (b'f', "fast", false),
    (b'g', "group", true),
    (b'G', "supp-group", true),
    (b'l', "login", false),
    (b'm', "preserve-environment", false),
    (b'p', "", false),
    (b'P', "pty", false),
    (b's', "shell", true),
    (b'u', "user", true),
    (b'h', "help", false),
    (b'V', "version", false),
    (b'w', "whitelist-environment", true),
];

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args, false))
}

fn die(short: &str, msg: &str) -> i32 {
    io::eprint(format!("{short}: {msg}\n"));
    1
}

pub(crate) fn run(args: &[OsString], runuser: bool) -> i32 {
    let short = if runuser { "runuser" } else { "su" };
    let argv = io::args_bytes(args);
    let sys = sys::current();
    let euid = sys.geteuid() as u64;
    if runuser && euid != 0 {
        return die(short, "may not be used by non-root users");
    }
    let spec = if runuser { RUNUSER_SPEC } else { SU_SPEC };
    let Some(o) = parse(short, &argv, spec) else {
        io::eprint(format!("Try '{short} --help' for more information.\n"));
        return 1;
    };

    let mut command: Option<Vec<u8>> = None;
    let mut fast = false;
    let mut login = false;
    let mut preserve = false;
    let mut shell_opt: Option<Vec<u8>> = None;
    let mut group_opt: Option<Vec<u8>> = None;
    let mut supp: Vec<Vec<u8>> = Vec::new();
    let mut user_opt: Option<Vec<u8>> = None;
    let mut whitelist: Vec<Vec<u8>> = Vec::new();
    let mut same_session = false;
    for (k, v) in &o.vals {
        let v = v.clone().unwrap_or_default();
        match *k {
            b'h' => {
                let _ = io::stdout()
                    .write_all(if runuser { RUNUSER_USAGE } else { SU_USAGE }.as_bytes());
                return 0;
            }
            b'V' => {
                let _ = io::stdout().write_all(format!("{short} from util-linux 2.41.5\n").as_bytes());
                return 0;
            }
            b'c' => {
                command = Some(v);
                same_session = false;
            }
            1 => {
                command = Some(v);
                same_session = true;
            }
            b'w' => whitelist.extend(v.split(|b| *b == b',').filter(|x| !x.is_empty()).map(<[u8]>::to_vec)),
            b'f' => fast = true,
            b'g' => group_opt = Some(v),
            b'G' => supp.push(v),
            b'l' => login = true,
            b'm' | b'p' => preserve = true,
            b's' => shell_opt = Some(v),
            b'u' => user_opt = Some(v),
            _ => {}
        }
    }
    if user_opt.is_some() && (shell_opt.is_some() || fast || command.is_some() || login) {
        return die(
            short,
            "options --{shell,fast,command,session-command,login} and --user are mutually exclusive",
        );
    }
    if user_opt.is_some() && o.rest.is_empty() {
        return die(short, "no command was specified");
    }
    if login && preserve {
        io::eprint(format!(
            "{short}: ignoring --preserve-environment, it's mutually exclusive with --login\n"
        ));
    }

    let mut rest = o.rest.clone();
    let user: Vec<u8> = match &user_opt {
        Some(u) => u.clone(),
        None => {
            if rest.first().is_some_and(|a| a.as_slice() == b"-") {
                login = true;
                rest.remove(0);
            }
            if rest.is_empty() { b"root".to_vec() } else { rest.remove(0) }
        }
    };

    if (group_opt.is_some() || !supp.is_empty()) && euid != 0 {
        return die(short, "only root can specify alternative groups");
    }
    let mut gid_override: Option<u64> = None;
    let mut supp_gids: Vec<u64> = Vec::new();
    for (i, g) in group_opt.iter().chain(supp.iter()).enumerate() {
        match lookup_group(g) {
            Some(grp) => {
                if i == 0 && group_opt.is_some() {
                    gid_override = Some(grp.gid);
                } else {
                    supp_gids.push(grp.gid);
                }
            }
            None => {
                return die(short, &format!("group {} does not exist", io::lossy(g)));
            }
        }
    }

    let entry = read_lines(b"/etc/passwd").ok().and_then(|lines| {
        lines
            .iter()
            .find(|l| is_data(l) && name_eq(l, &user))
            .map(|l| fields(l).iter().map(|x| x.to_vec()).collect::<Vec<_>>())
    });
    let pw = match entry {
        Some(f) if f.len() >= 7 && !f[0].is_empty() && !f[5].is_empty() => f,
        _ => {
            return die(
                short,
                &format!(
                    "user {} does not exist or the user entry does not contain all the required fields",
                    io::lossy(&user)
                ),
            );
        }
    };

    let uid = parse_id(&pw[2]).unwrap_or(0);
    let pw_gid = parse_id(&pw[3]).unwrap_or(0);
    // `-G` sem `-g`: o primeiro grupo suplementar vira o primário.
    let gid = gid_override.or(supp_gids.first().copied()).unwrap_or(pw_gid);

    // Quem não é root passa pelo PAM (`common-auth`): sem terminal o pam_unix lê a senha do stdin, e
    // nenhuma senha passa (as contas do sistema não têm senha). O pam_faildelay segura 1 s quando
    // nem houve resposta; com uma senha errada, o atraso do pam_unix soma uns 3 s.
    if !runuser && (sys.getuid() != 0 || euid != 0) {
        io::eprint("Password: ");
        let mut line = Vec::new();
        let mut b = [0u8; 1];
        let mut answered = false;
        while let Ok(1) = sys.read(sysabi::Fd::STDIN, &mut b) {
            answered = true;
            if b[0] == b'\n' {
                break;
            }
            line.push(b[0]);
        }
        let _ = sys.nanosleep(Duration::from_millis(if answered { 3200 } else { 1000 }));
        return die(short, "Authentication failure");
    }

    // `init_groups`: com `-g`/`-G` os grupos são exatamente esses; sem, os do `/etc/group` (initgroups).
    let mut groups: Vec<u32> = vec![gid as u32];
    if group_opt.is_some() || !supp_gids.is_empty() {
        groups.extend(supp_gids.iter().map(|g| *g as u32));
    } else if let Ok(lines) = read_lines(b"/etc/group") {
        for l in lines.iter().filter(|l| is_data(l)) {
            let f = fields(l);
            if f.len() >= 4 && f[3].split(|b| *b == b',').any(|m| m == pw[0].as_slice()) {
                if let Some(g) = parse_id(f[2]) {
                    groups.push(g as u32);
                }
            }
        }
    }
    groups.dedup();

    let shell: Option<Vec<u8>> = if user_opt.is_some() {
        None
    } else {
        Some(match shell_opt {
            Some(s) => s,
            None if pw[6].is_empty() => b"/bin/sh".to_vec(),
            None => pw[6].clone(),
        })
    };
    let env = new_environment(&sys.environ(), &pw, uid, shell.as_deref(), login, preserve, &whitelist, runuser);
    let (prog, exec_argv) = match &shell {
        None => (rest[0].clone(), rest.clone()),
        Some(sh) => {
            let mut name = sh.rsplit(|b| *b == b'/').next().unwrap_or(sh).to_vec();
            if login {
                name.insert(0, b'-');
            }
            let mut a = vec![name];
            if fast {
                a.push(b"-f".to_vec());
            }
            if let Some(c) = &command {
                a.push(b"-c".to_vec());
                a.push(c.clone());
            }
            a.extend(rest.iter().cloned());
            (sh.clone(), a)
        }
    };

    // `create_watching_parent`: o su fica como pai do shell (a sessão do PAM) e devolve o status dele.
    // O filho troca de identidade, abre sessão nova com `-c` e, no login, vai pro home.
    let _ = io::flush_stdout();
    let home = pw[5].clone();
    let new_session = command.is_some() && !same_session;
    let child_short = short.to_string();
    let body: sysabi::ProcessFn = Box::new(move || {
        let sys = sys::current();
        if new_session {
            let _ = sys.setsid();
        }
        if let Err(e) = sys.setgroups(&groups) {
            ul::warn(&child_short, "cannot set groups", e);
            return 1;
        }
        if let Err(e) = sys.setgid(gid as u32) {
            ul::warn(&child_short, "cannot set group id", e);
            return 1;
        }
        if let Err(e) = sys.setuid(uid as u32) {
            ul::warn(&child_short, "cannot set user id", e);
            return 1;
        }
        if login {
            if let Err(e) = sys.chdir(&home) {
                ul::warn(&child_short, format!("warning: cannot change directory to {}", io::lossy(&home)), e);
            }
        }
        let msg = format!("failed to execute {}", io::lossy(&prog));
        let e = execvpe(&prog, &exec_argv, &env);
        ul::warn(&child_short, msg, e);
        if e == Errno::ENOENT { 127 } else { 126 }
    });
    let name = match &shell {
        Some(sh) => sh.clone(),
        None => rest[0].clone(),
    };
    let child = match sys.spawn_fn(ProcAttrs::default(), name, body) {
        Ok(p) => p,
        Err(e) => {
            ul::warn(short, "cannot create child process", e);
            return 1;
        }
    };
    let status = loop {
        match sys.wait4(WaitTarget::Pid(child), WaitOptions::empty()) {
            Ok(Some((_, st))) => break st,
            Err(Errno::EINTR) | Ok(None) => continue,
            Err(e) => {
                ul::warn(short, "waitpid", e);
                return 1;
            }
        }
    };
    match status {
        WaitStatus::Exited(code) => code & 0xff,
        WaitStatus::Signaled { signal, core_dumped } => {
            io::eprint(format!("{}{}\n", signal.description(), if core_dumped { " (core dumped)" } else { "" }));
            128 + signal.0
        }
        WaitStatus::Stopped(_) | WaitStatus::Continued => 1,
    }
}

/// `modify_environment` do su-common.c, mais o que o PAM do su põe (o `pam_mail`; o runuser não tem).
#[allow(clippy::too_many_arguments)]
fn new_environment(
    cur: &[Vec<u8>],
    pw: &[Vec<u8>],
    uid: u64,
    shell: Option<&[u8]>,
    login: bool,
    preserve: bool,
    whitelist: &[Vec<u8>],
    runuser: bool,
) -> Vec<Vec<u8>> {
    let name_of = |e: &[u8]| e.split(|b| *b == b'=').next().unwrap_or(e).to_vec();
    let mut env: Vec<Vec<u8>> = cur.to_vec();
    let set = |env: &mut Vec<Vec<u8>>, k: &[u8], v: &[u8]| {
        let mut kv = k.to_vec();
        kv.push(b'=');
        kv.extend_from_slice(v);
        match env.iter_mut().find(|e| name_of(e) == k) {
            Some(e) => *e = kv,
            None => env.push(kv),
        }
    };
    if login {
        env.retain(|e| {
            let n = name_of(e);
            n == b"TERM" || whitelist.iter().any(|w| *w == n)
        });
        set(&mut env, b"HOME", &pw[5]);
        if let Some(s) = shell {
            set(&mut env, b"SHELL", s);
        }
        set(&mut env, b"USER", &pw[0]);
        set(&mut env, b"LOGNAME", &pw[0]);
        let path = login_defs_path(if uid == 0 { b"ENV_SUPATH" } else { b"ENV_PATH" });
        set(&mut env, b"PATH", &path);
    } else if !preserve {
        set(&mut env, b"HOME", &pw[5]);
        if let Some(s) = shell {
            set(&mut env, b"SHELL", s);
        }
        if uid != 0 {
            set(&mut env, b"USER", &pw[0]);
            set(&mut env, b"LOGNAME", &pw[0]);
        }
    }
    if !runuser {
        let mut mail = b"/var/mail/".to_vec();
        mail.extend_from_slice(&pw[0]);
        set(&mut env, b"MAIL", &mail);
    }
    env
}

/// O `PATH=` de `ENV_PATH`/`ENV_SUPATH` no `/etc/login.defs`, com os padrões do util-linux.
fn login_defs_path(key: &[u8]) -> Vec<u8> {
    let default: &[u8] = if key == b"ENV_SUPATH" {
        b"/usr/local/sbin:/usr/local/bin:/sbin:/bin:/usr/sbin:/usr/bin"
    } else {
        b"/usr/local/bin:/bin:/usr/bin"
    };
    let Ok(lines) = read_lines(b"/etc/login.defs") else { return default.to_vec() };
    for l in &lines {
        let mut it = l.split(|b| *b == b' ' || *b == b'\t').filter(|x| !x.is_empty());
        if it.next() == Some(key) {
            if let Some(v) = it.next() {
                return v.strip_prefix(b"PATH=").unwrap_or(v).to_vec();
            }
        }
    }
    default.to_vec()
}

/// `execvp` com o ambiente novo: a busca no PATH usa o PATH desse ambiente, como o su faz depois de
/// trocar o `environ`.
fn execvpe(file: &[u8], argv: &[Vec<u8>], env: &[Vec<u8>]) -> Errno {
    let sys = sys::current();
    if file.contains(&b'/') {
        return sys.execve(file, argv, Some(env));
    }
    let path = env
        .iter()
        .find_map(|e| e.strip_prefix(b"PATH="))
        .map(<[u8]>::to_vec)
        .unwrap_or_else(|| b"/bin:/usr/bin".to_vec());
    let mut last = Errno::ENOENT;
    for dir in path.split(|b| *b == b':') {
        let mut full = if dir.is_empty() { b".".to_vec() } else { dir.to_vec() };
        full.push(b'/');
        full.extend_from_slice(file);
        let e = sys.execve(&full, argv, Some(env));
        if e != Errno::ENOENT && e != Errno::ENOTDIR {
            last = e;
        }
    }
    last
}
