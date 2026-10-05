//! `unshare` do util-linux 2.41: roda um programa com alguns namespaces separados dos do pai.
//!
//! Porte do `sys-utils/unshare.c`. Todas as opções são lidas e validadas como no original. O sandbox
//! não oferece `unshare(2)` nem `setns(2)`, então pedir qualquer namespace dá o mesmo erro que o
//! original dá como root num contêiner sem `CAP_SYS_ADMIN`: `unshare failed: Operation not permitted`.
//! Sem namespaces pedidos, o programa (ou `$SHELL`) é executado normalmente, depois de `--root`/`--wd`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, sys};

use crate::setsid::execvp;
use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const EX_EXEC_FAILED: i32 = 126;
const EX_EXEC_ENOENT: i32 = 127;

const OPT_MOUNTPROC: i32 = 256;
const OPT_PROPAGATION: i32 = 257;
const OPT_SETGROUPS: i32 = 258;
const OPT_KILLCHILD: i32 = 259;
const OPT_KEEPCAPS: i32 = 260;
const OPT_MONOTONIC: i32 = 261;
const OPT_BOOTTIME: i32 = 262;
const OPT_MAPUSER: i32 = 263;
const OPT_MAPUSERS: i32 = 264;
const OPT_MAPGROUP: i32 = 265;
const OPT_MAPGROUPS: i32 = 266;
const OPT_MAPAUTO: i32 = 267;

const LONGS: &[LongOpt] = &[
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("mount", HasArg::Optional, b'm' as i32),
    LongOpt::new("uts", HasArg::Optional, b'u' as i32),
    LongOpt::new("ipc", HasArg::Optional, b'i' as i32),
    LongOpt::new("net", HasArg::Optional, b'n' as i32),
    LongOpt::new("pid", HasArg::Optional, b'p' as i32),
    LongOpt::new("user", HasArg::Optional, b'U' as i32),
    LongOpt::new("cgroup", HasArg::Optional, b'C' as i32),
    LongOpt::new("time", HasArg::Optional, b'T' as i32),
    LongOpt::new("fork", HasArg::No, b'f' as i32),
    LongOpt::new("kill-child", HasArg::Optional, OPT_KILLCHILD),
    LongOpt::new("mount-proc", HasArg::Optional, OPT_MOUNTPROC),
    LongOpt::new("map-user", HasArg::Required, OPT_MAPUSER),
    LongOpt::new("map-users", HasArg::Required, OPT_MAPUSERS),
    LongOpt::new("map-group", HasArg::Required, OPT_MAPGROUP),
    LongOpt::new("map-groups", HasArg::Required, OPT_MAPGROUPS),
    LongOpt::new("map-root-user", HasArg::No, b'r' as i32),
    LongOpt::new("map-current-user", HasArg::No, b'c' as i32),
    LongOpt::new("map-auto", HasArg::No, OPT_MAPAUTO),
    LongOpt::new("propagation", HasArg::Required, OPT_PROPAGATION),
    LongOpt::new("setgroups", HasArg::Required, OPT_SETGROUPS),
    LongOpt::new("keep-caps", HasArg::No, OPT_KEEPCAPS),
    LongOpt::new("setuid", HasArg::Required, b'S' as i32),
    LongOpt::new("setgid", HasArg::Required, b'G' as i32),
    LongOpt::new("root", HasArg::Required, b'R' as i32),
    LongOpt::new("wd", HasArg::Required, b'w' as i32),
    LongOpt::new("monotonic", HasArg::Required, OPT_MONOTONIC),
    LongOpt::new("boottime", HasArg::Required, OPT_BOOTTIME),
];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [options] [<program> [<argument>...]]

Run a program with some namespaces unshared from the parent.

Options:
 -m, --mount[=<file>]      unshare mounts namespace
 -u, --uts[=<file>]        unshare UTS namespace (hostname etc)
 -i, --ipc[=<file>]        unshare System V IPC namespace
 -n, --net[=<file>]        unshare network namespace
 -p, --pid[=<file>]        unshare pid namespace
 -U, --user[=<file>]       unshare user namespace
 -C, --cgroup[=<file>]     unshare cgroup namespace
 -T, --time[=<file>]       unshare time namespace

 --mount-proc[=<dir>]      mount proc filesystem first (implies --mount)
 --mount-binfmt[=<dir>]    mount binfmt filesystem first (implies --user and --mount)
 -l, --load-interp <file>  load binfmt definition in the namespace (implies --mount-binfmt)
 --propagation slave|shared|private|unchanged
                           modify mount propagation in mount namespace
 -R, --root <dir>          run the command with root directory set to <dir>
 -w, --wd <dir>            change working directory to <dir>

 -S, --setuid <uid>        set uid in entered namespace
 -G, --setgid <gid>        set gid in entered namespace
 --map-user <uid>|<name>   map current user to uid (implies --user)
 --map-group <gid>|<name>  map current group to gid (implies --user)
 -r, --map-root-user       map current user to root (implies --user)
 -c, --map-current-user    map current user to itself (implies --user)
 --map-auto                map users and groups automatically (implies --user)
 --map-users <inneruid>:<outeruid>:<count>
                           map count users from outeruid to inneruid (implies --user)
 --map-groups <innergid>:<outergid>:<count>
                           map count groups from outergid to innergid (implies --user)

 -f, --fork                fork before launching <program>
 --kill-child[=<signame>]  when dying, kill the forked child (implies --fork)
                             defaults to SIGKILL

 --setgroups allow|deny    control the setgroups syscall in user namespaces
 --keep-caps               retain capabilities granted in user namespaces

 --monotonic <offset>      set clock monotonic offset (seconds) in time namespaces
 --boottime <offset>       set clock boottime offset (seconds) in time namespaces

 -h, --help                display this help
 -V, --version             display version

For more details see unshare(1).
"
    )
}

const CLONE_NEWNS: u32 = 0x0002_0000;
const CLONE_NEWCGROUP: u32 = 0x0200_0000;
const CLONE_NEWUTS: u32 = 0x0400_0000;
const CLONE_NEWIPC: u32 = 0x0800_0000;
const CLONE_NEWUSER: u32 = 0x1000_0000;
const CLONE_NEWPID: u32 = 0x2000_0000;
const CLONE_NEWNET: u32 = 0x4000_0000;
const CLONE_NEWTIME: u32 = 0x0000_0080;

/// `signame_to_signum`: aceita `SIGTERM`, `term` (qualquer caixa) e números.
fn signame_to_signum(name: &str) -> Option<i32> {
    if let Ok(n) = name.parse::<i32>() {
        return Some(n);
    }
    const SIGS: &[(&str, i32)] = &[
        ("HUP", 1),
        ("INT", 2),
        ("QUIT", 3),
        ("ILL", 4),
        ("TRAP", 5),
        ("ABRT", 6),
        ("IOT", 6),
        ("BUS", 7),
        ("FPE", 8),
        ("KILL", 9),
        ("USR1", 10),
        ("SEGV", 11),
        ("USR2", 12),
        ("PIPE", 13),
        ("ALRM", 14),
        ("TERM", 15),
        ("STKFLT", 16),
        ("CHLD", 17),
        ("CONT", 18),
        ("STOP", 19),
        ("TSTP", 20),
        ("TTIN", 21),
        ("TTOU", 22),
        ("URG", 23),
        ("XCPU", 24),
        ("XFSZ", 25),
        ("VTALRM", 26),
        ("PROF", 27),
        ("WINCH", 28),
        ("IO", 29),
        ("POLL", 29),
        ("PWR", 30),
        ("SYS", 31),
    ];
    let up = name.to_ascii_uppercase();
    let base = up.strip_prefix("SIG").unwrap_or(&up);
    SIGS.iter().find(|(n, _)| *n == base).map(|(_, v)| *v)
}

/// `get_user`/`get_group`: nome no `/etc/passwd` (ou `/etc/group`) ou número.
fn get_id(arg: &[u8], file: &[u8], what: &str) -> Result<u32, String> {
    let text = io::lossy(arg);
    if let Ok(data) = io::read_path(file) {
        for line in String::from_utf8_lossy(&data).lines() {
            let mut f = line.split(':');
            if f.next() == Some(text.as_str()) {
                if let Some(id) = f.nth(1).and_then(|s| s.parse::<u32>().ok()) {
                    return Ok(id);
                }
            }
        }
    }
    ul::strtou32_or_err(arg, what)
}

/// `strtos64_or_err`: inteiro de 64 bits com sinal.
fn strtos64(arg: &[u8], what: &str) -> Result<i64, String> {
    let text = io::lossy(arg);
    match text.trim_start().parse::<i64>() {
        Ok(v) => Ok(v),
        Err(e) => {
            use std::num::IntErrorKind::*;
            if matches!(e.kind(), PosOverflow | NegOverflow) {
                Err(format!("{what}: '{text}': {}", Errno::ERANGE.message()))
            } else {
                Err(format!("{what}: '{text}'"))
            }
        }
    }
}

/// `parse_map_range`: `outer,inner,count`.
fn parse_map(arg: &[u8]) -> Result<(u32, u32, u32), String> {
    let text = io::lossy(arg);
    let parts: Vec<&str> = text.split(',').collect();
    if parts.len() != 3 {
        return Err(format!("invalid mapping '{text}'"));
    }
    let n = |s: &str| s.parse::<u32>().map_err(|_| format!("invalid mapping '{text}'"));
    Ok((n(parts[0])?, n(parts[1])?, n(parts[2])?))
}

fn die(short: &str, msg: impl AsRef<str>) -> i32 {
    ul::warnx(short, msg);
    1
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut flags: u32 = 0;
    let mut forkit = false;
    let mut keepcaps = false;
    let mut mapuser: Option<u32> = None;
    let mut mapgroup: Option<u32> = None;
    let mut map_users: Option<(u32, u32, u32)> = None;
    let mut map_groups: Option<(u32, u32, u32)> = None;
    let mut map_auto = false;
    let mut setgroups: Option<&'static str> = None;
    let mut propagation = true;
    let mut procmnt: Option<String> = None;
    let mut kill_child: Option<i32> = None;
    let mut newroot: Option<Vec<u8>> = None;
    let mut newdir: Option<Vec<u8>> = None;
    let mut uid: Option<u32> = None;
    let mut gid: Option<u32> = None;
    let mut force_monotonic = false;
    let mut force_boottime = false;
    let mut npersists = 0usize;
    let sys = sys::current();

    let mut g = Getopt::from_env(&argv[1..], "+fhVmuinpCUTrR:w:S:G:c", LONGS);
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
        match o.id {
            x if x == b'h' as i32 => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
                return 0;
            }
            x if x == b'V' as i32 => {
                ul::print_version(&short);
                return 0;
            }
            x if x == b'm' as i32 => flags |= CLONE_NEWNS,
            x if x == b'u' as i32 => flags |= CLONE_NEWUTS,
            x if x == b'i' as i32 => flags |= CLONE_NEWIPC,
            x if x == b'n' as i32 => flags |= CLONE_NEWNET,
            x if x == b'p' as i32 => flags |= CLONE_NEWPID,
            x if x == b'U' as i32 => flags |= CLONE_NEWUSER,
            x if x == b'C' as i32 => flags |= CLONE_NEWCGROUP,
            x if x == b'T' as i32 => flags |= CLONE_NEWTIME,
            x if x == b'f' as i32 => forkit = true,
            x if x == b'r' as i32 => {
                flags |= CLONE_NEWUSER;
                mapuser = Some(0);
            }
            x if x == b'c' as i32 => {
                flags |= CLONE_NEWUSER;
                mapuser = Some(sys.geteuid());
                mapgroup = Some(sys.getegid());
            }
            x if x == b'R' as i32 => newroot = arg.clone(),
            x if x == b'w' as i32 => newdir = arg.clone(),
            x if x == b'S' as i32 => {
                match ul::strtou32_or_err(&arg.clone().unwrap_or_default(), "failed to parse uid") {
                    Ok(v) => uid = Some(v),
                    Err(m) => return die(&short, m),
                }
            }
            x if x == b'G' as i32 => {
                match ul::strtou32_or_err(&arg.clone().unwrap_or_default(), "failed to parse gid") {
                    Ok(v) => gid = Some(v),
                    Err(m) => return die(&short, m),
                }
            }
            OPT_MOUNTPROC => {
                flags |= CLONE_NEWNS;
                procmnt = Some(arg.as_deref().map(io::lossy).unwrap_or_else(|| "/proc".into()));
            }
            OPT_PROPAGATION => {
                let a = io::lossy(&arg.clone().unwrap_or_default());
                match a.as_str() {
                    "slave" | "private" | "shared" => propagation = true,
                    "unchanged" => propagation = false,
                    _ => return die(&short, format!("unsupported propagation mode: {a}")),
                }
            }
            OPT_SETGROUPS => {
                let a = io::lossy(&arg.clone().unwrap_or_default());
                setgroups = Some(match a.as_str() {
                    "allow" => "allow",
                    "deny" => "deny",
                    _ => return die(&short, format!("unsupported --setgroups argument '{a}'")),
                });
            }
            OPT_KILLCHILD => {
                forkit = true;
                kill_child = Some(9);
                if let Some(a) = &arg {
                    let name = io::lossy(a);
                    match signame_to_signum(&name) {
                        Some(n) if n >= 0 => kill_child = Some(n),
                        _ => return die(&short, format!("unknown signal: {name}")),
                    }
                }
            }
            OPT_KEEPCAPS => keepcaps = true,
            OPT_MONOTONIC => {
                if let Err(m) = strtos64(&arg.clone().unwrap_or_default(), "failed to parse monotonic offset") {
                    return die(&short, m);
                }
                force_monotonic = true;
            }
            OPT_BOOTTIME => {
                if let Err(m) = strtos64(&arg.clone().unwrap_or_default(), "failed to parse boottime offset") {
                    return die(&short, m);
                }
                force_boottime = true;
            }
            OPT_MAPUSER => {
                flags |= CLONE_NEWUSER;
                match get_id(&arg.clone().unwrap_or_default(), b"/etc/passwd", "failed to parse uid") {
                    Ok(v) => mapuser = Some(v),
                    Err(m) => return die(&short, m),
                }
            }
            OPT_MAPGROUP => {
                flags |= CLONE_NEWUSER;
                match get_id(&arg.clone().unwrap_or_default(), b"/etc/group", "failed to parse gid") {
                    Ok(v) => mapgroup = Some(v),
                    Err(m) => return die(&short, m),
                }
            }
            OPT_MAPUSERS => {
                flags |= CLONE_NEWUSER;
                match parse_map(&arg.clone().unwrap_or_default()) {
                    Ok(v) => map_users = Some(v),
                    Err(m) => return die(&short, m),
                }
            }
            OPT_MAPGROUPS => {
                flags |= CLONE_NEWUSER;
                match parse_map(&arg.clone().unwrap_or_default()) {
                    Ok(v) => map_groups = Some(v),
                    Err(m) => return die(&short, m),
                }
            }
            OPT_MAPAUTO => {
                flags |= CLONE_NEWUSER;
                map_auto = true;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
        // Persistência (`--mount=<file>` etc.): o argumento opcional dos namespaces.
        if matches!(o.short(), Some('m' | 'u' | 'i' | 'n' | 'p' | 'U' | 'C' | 'T')) && arg.is_some() {
            npersists += 1;
        }
    }

    if (force_monotonic || force_boottime) && flags & CLONE_NEWTIME == 0 {
        return die(
            &short,
            "options --monotonic and --boottime require unsharing of a time namespace (-T)",
        );
    }
    // Sem user namespace, `--setgroups` e `--keep-caps` caem no erro do próprio unshare(2).
    let needs_unshare = setgroups.is_some() || keepcaps;
    if map_auto && (mapuser.is_some() || mapgroup.is_some() || map_users.is_some() || map_groups.is_some()) {
        return die(
            &short,
            "options --map-auto and --map-user/--map-users/--map-group/--map-groups are mutually exclusive",
        );
    }
    let _ = (propagation, &procmnt, kill_child, npersists);

    if flags != 0 || needs_unshare {
        // unshare(2) sem CAP_SYS_ADMIN: o sandbox corre como o root de um contêiner padrão.
        ul::warn(&short, "unshare failed", Errno::EPERM);
        return 1;
    }

    let cmd = g.operands();

    // `--fork` sem namespaces: o efeito observável é o do exec direto.
    let _ = forkit;
    if let Some(root) = &newroot {
        if let Err(e) = sys.fstatat(sysabi::Fd::CWD, root, sysabi::AtFlags::empty()) {
            ul::warn(
                &short,
                format!("cannot change root directory to '{}'", io::lossy(root)),
                e,
            );
            return 1;
        }
        ul::warn(
            &short,
            format!("cannot change root directory to '{}'", io::lossy(root)),
            Errno::EPERM,
        );
        return 1;
    }
    if let Some(dir) = &newdir {
        if let Err(e) = sys.chdir(dir) {
            ul::warn(
                &short,
                format!("cannot change working directory to '{}'", io::lossy(dir)),
                e,
            );
            return 1;
        }
    }
    if let Some(g) = gid {
        if g != sys.getegid() {
            ul::warn(&short, "setgid failed", Errno::EPERM);
            return 1;
        }
    }
    if let Some(u) = uid {
        if u != sys.geteuid() {
            ul::warn(&short, "setuid failed", Errno::EPERM);
            return 1;
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
