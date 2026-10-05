//! `nsenter` do util-linux 2.41: roda um programa com os namespaces de outros processos.
//!
//! Porte do `sys-utils/nsenter.c`. Todas as opções são lidas e validadas como no original. O sandbox
//! não oferece `setns(2)`, então entrar em qualquer namespace dá o mesmo erro que o original dá como
//! root num contêiner sem `CAP_SYS_ADMIN`: `reassociate to namespace 'ns/xxx' failed: Operation not
//! permitted`. Sem namespaces pedidos, o programa (ou `$SHELL`) é executado normalmente.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{AtFlags, Ctx, Errno, Fd, sys};

use crate::setsid::execvp;
use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const EX_EXEC_FAILED: i32 = 126;
const EX_EXEC_ENOENT: i32 = 127;

const OPT_PRESERVE_CRED: i32 = 256;
const OPT_USER_PARENT: i32 = 257;

const LONGS: &[LongOpt] = &[
    LongOpt::new("all", HasArg::No, b'a' as i32),
    LongOpt::new("target", HasArg::Required, b't' as i32),
    LongOpt::new("mount", HasArg::Optional, b'm' as i32),
    LongOpt::new("uts", HasArg::Optional, b'u' as i32),
    LongOpt::new("ipc", HasArg::Optional, b'i' as i32),
    LongOpt::new("net", HasArg::Optional, b'n' as i32),
    LongOpt::new("pid", HasArg::Optional, b'p' as i32),
    LongOpt::new("user", HasArg::Optional, b'U' as i32),
    LongOpt::new("cgroup", HasArg::Optional, b'C' as i32),
    LongOpt::new("time", HasArg::Optional, b'T' as i32),
    LongOpt::new("setuid", HasArg::Optional, b'S' as i32),
    LongOpt::new("setgid", HasArg::Optional, b'G' as i32),
    LongOpt::new("root", HasArg::Optional, b'r' as i32),
    LongOpt::new("wd", HasArg::Optional, b'w' as i32),
    LongOpt::new("no-fork", HasArg::No, b'F' as i32),
    LongOpt::new("follow-context", HasArg::No, b'Z' as i32),
    LongOpt::new("preserve-credentials", HasArg::No, OPT_PRESERVE_CRED),
    LongOpt::new("user-parent", HasArg::No, OPT_USER_PARENT),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [options] [<program> [<argument>...]]

Run a program with namespaces of other processes.

Options:
 -a, --all              enter all namespaces
 -t, --target <pid>     target process to get namespaces from
 -m, --mount[=<file>]   enter mount namespace
 -u, --uts[=<file>]     enter UTS namespace (hostname etc)
 -i, --ipc[=<file>]     enter System V IPC namespace
 -n, --net[=<file>]     enter network namespace
 -p, --pid[=<file>]     enter pid namespace
 -C, --cgroup[=<file>]  enter cgroup namespace
 -U, --user[=<file>]    enter user namespace
     --user-parent      enter parent user namespace
 -T, --time[=<file>]    enter time namespace

 -S, --setuid[=<uid>]   set uid in entered namespace
 -G, --setgid[=<gid>]   set gid in entered namespace
     --preserve-credentials do not touch uids or gids
 -r, --root[=<dir>]     set the root directory
 -w, --wd[=<dir>]       set the working directory
 -F, --no-fork          do not fork before exec'ing <program>
 -Z, --follow-context   set SELinux context according to --target PID

 -h, --help             display this help
 -V, --version          display version

For more details see nsenter(1).
"
    )
}

/// Namespaces na ordem da tabela do original: nome do arquivo em `/proc/<pid>/ns/`.
#[derive(Clone, Copy, PartialEq)]
enum Ns {
    User,
    Cgroup,
    Ipc,
    Net,
    Pid,
    Mnt,
    Uts,
    Time,
}

const ORDER: &[(Ns, &str)] = &[
    (Ns::User, "ns/user"),
    (Ns::Cgroup, "ns/cgroup"),
    (Ns::Ipc, "ns/ipc"),
    (Ns::Net, "ns/net"),
    (Ns::Pid, "ns/pid"),
    (Ns::Mnt, "ns/mnt"),
    (Ns::Uts, "ns/uts"),
    (Ns::Time, "ns/time"),
];

/// Um namespace pedido: arquivo explícito (`--net=<file>`) ou o do `--target`.
#[derive(Default, Clone)]
struct Want {
    on: bool,
    file: Option<Vec<u8>>,
}

fn slot(ns: Ns) -> usize {
    ORDER.iter().position(|(n, _)| *n == ns).unwrap_or(0)
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);
    let sys = sys::current();

    let mut want: Vec<Want> = vec![Want::default(); ORDER.len()];
    let mut do_all = false;
    let mut target: Option<u64> = None;
    let mut user_parent = false;
    let mut preserve = false;
    let mut uid: Option<u32> = None;
    let mut gid: Option<u32> = None;
    let mut do_setuid = false;
    let mut do_setgid = false;
    let mut root: Option<Option<Vec<u8>>> = None;
    let mut wd: Option<Option<Vec<u8>>> = None;

    let mut g = Getopt::from_env(
        &argv[1..],
        "+ahVt:m::u::i::n::p::C::U::T::S::G::r::w::FZ",
        LONGS,
    );
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        let arg = o.arg.clone();
        let ns = match o.id {
            x if x == b'h' as i32 => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
                return 0;
            }
            x if x == b'V' as i32 => {
                ul::print_version(&short);
                return 0;
            }
            x if x == b'a' as i32 => {
                do_all = true;
                None
            }
            x if x == b't' as i32 => {
                match ul::strtou64_or_err(&arg.clone().unwrap_or_default(), "failed to parse PID") {
                    Ok(v) => target = Some(v),
                    Err(m) => {
                        ul::warnx(&short, m);
                        return 1;
                    }
                }
                None
            }
            x if x == b'm' as i32 => Some(Ns::Mnt),
            x if x == b'u' as i32 => Some(Ns::Uts),
            x if x == b'i' as i32 => Some(Ns::Ipc),
            x if x == b'n' as i32 => Some(Ns::Net),
            x if x == b'p' as i32 => Some(Ns::Pid),
            x if x == b'C' as i32 => Some(Ns::Cgroup),
            x if x == b'U' as i32 => Some(Ns::User),
            x if x == b'T' as i32 => Some(Ns::Time),
            OPT_USER_PARENT => {
                user_parent = true;
                None
            }
            x if x == b'S' as i32 => {
                if let Some(a) = &arg {
                    match ul::strtou32_or_err(a, "failed to parse uid") {
                        Ok(v) => uid = Some(v),
                        Err(m) => {
                            ul::warnx(&short, m);
                            return 1;
                        }
                    }
                }
                do_setuid = true;
                None
            }
            x if x == b'G' as i32 => {
                if let Some(a) = &arg {
                    match ul::strtou32_or_err(a, "failed to parse gid") {
                        Ok(v) => gid = Some(v),
                        Err(m) => {
                            ul::warnx(&short, m);
                            return 1;
                        }
                    }
                }
                do_setgid = true;
                None
            }
            OPT_PRESERVE_CRED => {
                preserve = true;
                None
            }
            x if x == b'r' as i32 => {
                root = Some(arg.clone());
                None
            }
            x if x == b'w' as i32 => {
                wd = Some(arg.clone());
                None
            }
            x if x == b'F' as i32 || x == b'Z' as i32 => None,
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        };
        if let Some(ns) = ns {
            let w = &mut want[slot(ns)];
            w.on = true;
            w.file = arg;
        }
    }

    let cmd = g.operands();

    if do_all && target.is_none() {
        ul::warnx(&short, "no target PID specified for --all");
        return 1;
    }
    if do_all {
        for w in want.iter_mut() {
            w.on = true;
        }
    }

    // Abre os arquivos de namespace: o do --target ou o dado na opção.
    let mut to_enter: Vec<&str> = Vec::new();
    for (i, (_, name)) in ORDER.iter().enumerate() {
        let w = &want[i];
        if !w.on {
            continue;
        }
        let path: Vec<u8> = match (&w.file, target) {
            (Some(f), _) => f.clone(),
            (None, Some(pid)) => format!("/proc/{pid}/{name}").into_bytes(),
            (None, None) => {
                ul::warnx(&short, format!("no target PID specified for {name}"));
                return 1;
            }
        };
        if let Err(e) = sys.fstatat(Fd::CWD, &path, AtFlags::empty()) {
            ul::warn(&short, format!("cannot open {}", io::lossy(&path)), e);
            return 1;
        }
        to_enter.push(name);
    }
    if user_parent {
        let pid = target.map(|p| p.to_string()).unwrap_or_else(|| "self".into());
        let path = format!("/proc/{pid}/ns/user");
        if let Err(e) = sys.fstatat(Fd::CWD, path.as_bytes(), AtFlags::empty()) {
            ul::warn(&short, format!("cannot open {path}"), e);
            return 1;
        }
        to_enter.insert(0, "ns/user");
    }

    // --root e --wd: sem arquivo, vêm de /proc/<pid>/root e /proc/<pid>/cwd do alvo.
    let mut new_root: Option<Vec<u8>> = None;
    let mut new_wd: Option<Vec<u8>> = None;
    for (opt, what, sub, out) in [
        (&root, "--root", "root", &mut new_root),
        (&wd, "--wd", "cwd", &mut new_wd),
    ] {
        let Some(o) = opt else { continue };
        let path: Vec<u8> = match (o, target) {
            (Some(p), _) => p.clone(),
            (None, Some(pid)) => format!("/proc/{pid}/{sub}").into_bytes(),
            (None, None) => {
                ul::warnx(&short, format!("no target PID specified for {what}"));
                return 1;
            }
        };
        if let Err(e) = sys.fstatat(Fd::CWD, &path, AtFlags::empty()) {
            ul::warn(&short, format!("cannot open {}", io::lossy(&path)), e);
            return 1;
        }
        *out = Some(path);
    }

    // setns(2) sem CAP_SYS_ADMIN: o sandbox corre como o root de um contêiner padrão.
    if let Some(first) = to_enter.first() {
        ul::warn(
            &short,
            format!("reassociate to namespace '{first}' failed"),
            Errno::EPERM,
        );
        return 1;
    }
    if let Some(r) = &new_root {
        ul::warn(&short, "chroot", Errno::EPERM);
        let _ = r;
        return 1;
    }
    if let Some(d) = &new_wd {
        if let Err(e) = sys.chdir(d) {
            ul::warn(&short, "cannot change working directory", e);
            return 1;
        }
    }

    if !preserve {
        if do_setgid {
            let want_gid = gid.unwrap_or(0);
            if want_gid != sys.getegid() {
                ul::warn(&short, "setgid failed", Errno::EPERM);
                return 1;
            }
        }
        if do_setuid {
            let want_uid = uid.unwrap_or(0);
            if want_uid != sys.geteuid() {
                ul::warn(&short, "setuid failed", Errno::EPERM);
                return 1;
            }
        }
    }

    let cmd: Vec<Vec<u8>> = if cmd.is_empty() {
        let shell = sys.getenv(b"SHELL").unwrap_or_else(|| b"/bin/sh".to_vec());
        vec![shell]
    } else {
        cmd
    };
    let _ = io::flush_stdout();
    let e = execvp(&cmd[0], &cmd);
    ul::warn(&short, format!("failed to execute {}", io::lossy(&cmd[0])), e);
    if e == Errno::ENOENT {
        EX_EXEC_ENOENT
    } else {
        EX_EXEC_FAILED
    }
}
