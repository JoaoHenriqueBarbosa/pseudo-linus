//! `su` do util-linux 2.41 (e o miolo compartilhado com `runuser`).
//!
//! Porte do `login-utils/su-common.c` sem PAM nem terminal: opções, `--help`/`--version`, busca do
//! usuário em `/etc/passwd` e as mensagens e códigos de saída de quem roda como root. O sandbox não
//! tem `setuid`/`setgroups`: trocar para outro uid ou gid falha como o original num contêiner sem
//! capacidades (`cannot set groups: Operation not permitted`); quando o alvo é o próprio usuário, o
//! shell (ou o comando) é executado de verdade. Ficam de fora o reset de ambiente, o `chdir` do
//! login, a lista `-w`, o pty do `-P` e a sessão de PAM.

use std::ffi::OsString;
use std::io::Write;

use sysabi::sys;

use crate::groupmgmt::{Spec, fields, is_data, name_eq, parse, parse_id, read_lines};
use crate::sg::{exec_or_fail, lookup_group};
use crate::util::io;

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
    let egid = sys.getegid() as u64;
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
            b'c' | 1 => command = Some(v),
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
    for (i, g) in group_opt.iter().chain(supp.iter()).enumerate() {
        match lookup_group(g) {
            Some(grp) => {
                if i == 0 && group_opt.is_some() {
                    gid_override = Some(grp.gid);
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
    let gid = gid_override.or_else(|| parse_id(&pw[3])).unwrap_or(0);
    if uid != euid || gid != egid {
        io::eprint(format!("{short}: cannot set groups: Operation not permitted\n"));
        return 1;
    }

    if user_opt.is_some() {
        let prog = rest[0].clone();
        let msg = format!("failed to execute {}", io::lossy(&prog));
        return exec_or_fail(short, msg, &prog, &rest);
    }
    let shell: Vec<u8> = match shell_opt {
        Some(s) => s,
        None if pw[6].is_empty() => b"/bin/sh".to_vec(),
        None => pw[6].clone(),
    };
    let mut name = shell.rsplit(|b| *b == b'/').next().unwrap_or(&shell).to_vec();
    if login {
        name.insert(0, b'-');
    }
    let mut exec_argv = vec![name];
    if fast {
        exec_argv.push(b"-f".to_vec());
    }
    if let Some(c) = command {
        exec_argv.push(b"-c".to_vec());
        exec_argv.push(c);
    }
    exec_argv.extend(rest);
    let msg = format!("failed to execute {}", io::lossy(&shell));
    exec_or_fail(short, msg, &shell, &exec_argv)
}
