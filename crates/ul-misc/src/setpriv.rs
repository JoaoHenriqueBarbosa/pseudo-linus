//! `setpriv` do util-linux 2.41: roda um programa com outras configurações de privilégio Linux.
//!
//! Porte do `sys-utils/setpriv.c`. Todas as opções são lidas e validadas como no original, e `--dump` e
//! `--list-caps` funcionam sobre `/proc/self/status`. O sandbox não tem `setresuid`, `setgroups`,
//! `capset`, `prctl` nem LSM: as opções que mudariam o estado do processo falham com o erro que o
//! original dá como root num contêiner padrão quando a mudança não é permitida (`Operation not
//! permitted`), exceto quando pedem o valor que o processo já tem.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, sys};
use ul_common::signal;

use crate::setsid::execvp;
use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

/// Nomes de sinal que o `--pdeathsig` reconhece: qualquer caixa, `SIG` opcional, o 29 é `IO`, sem apelidos.
const SIGNALS: signal::Table = signal::Table { sig29: signal::Sig29::Io, case: signal::Case::Any, aliases: &[] };

const EX_EXEC_FAILED: i32 = 126;
const EX_EXEC_ENOENT: i32 = 127;
/// `SETPRIV_EXIT_PRIVERR` do original.
const EXIT_PRIVERR: i32 = 127;

const CAPS: &[&str] = &[
    "chown",
    "dac_override",
    "dac_read_search",
    "fowner",
    "fsetid",
    "kill",
    "setgid",
    "setuid",
    "setpcap",
    "linux_immutable",
    "net_bind_service",
    "net_broadcast",
    "net_admin",
    "net_raw",
    "ipc_lock",
    "ipc_owner",
    "sys_module",
    "sys_rawio",
    "sys_chroot",
    "sys_ptrace",
    "sys_pacct",
    "sys_admin",
    "sys_boot",
    "sys_nice",
    "sys_resource",
    "sys_time",
    "sys_tty_config",
    "mknod",
    "lease",
    "audit_write",
    "audit_control",
    "setfcap",
    "mac_override",
    "mac_admin",
    "syslog",
    "wake_alarm",
    "block_suspend",
    "audit_read",
    "perfmon",
    "bpf",
    "checkpoint_restore",
];

const O_NNP: i32 = 256;
const O_INH: i32 = 257;
const O_AMBIENT: i32 = 258;
const O_BOUNDING: i32 = 259;
const O_RUID: i32 = 260;
const O_EUID: i32 = 261;
const O_RGID: i32 = 262;
const O_EGID: i32 = 263;
const O_REUID: i32 = 264;
const O_REGID: i32 = 265;
const O_CLEAR_GROUPS: i32 = 266;
const O_KEEP_GROUPS: i32 = 267;
const O_INIT_GROUPS: i32 = 268;
const O_GROUPS: i32 = 269;
const O_SECUREBITS: i32 = 270;
const O_PDEATHSIG: i32 = 271;
const O_SELINUX: i32 = 272;
const O_APPARMOR: i32 = 273;
const O_RESET_ENV: i32 = 274;
const O_LIST_CAPS: i32 = 275;
const O_PTRACER: i32 = 276;
const O_LANDLOCK_ACCESS: i32 = 277;
const O_LANDLOCK_RULE: i32 = 278;

const LONGS: &[LongOpt] = &[
    LongOpt::new("dump", HasArg::No, b'd' as i32),
    LongOpt::new("nnp", HasArg::No, O_NNP),
    LongOpt::new("no-new-privs", HasArg::No, O_NNP),
    LongOpt::new("inh-caps", HasArg::Required, O_INH),
    LongOpt::new("ambient-caps", HasArg::Required, O_AMBIENT),
    LongOpt::new("bounding-set", HasArg::Required, O_BOUNDING),
    LongOpt::new("ruid", HasArg::Required, O_RUID),
    LongOpt::new("euid", HasArg::Required, O_EUID),
    LongOpt::new("rgid", HasArg::Required, O_RGID),
    LongOpt::new("egid", HasArg::Required, O_EGID),
    LongOpt::new("reuid", HasArg::Required, O_REUID),
    LongOpt::new("regid", HasArg::Required, O_REGID),
    LongOpt::new("clear-groups", HasArg::No, O_CLEAR_GROUPS),
    LongOpt::new("keep-groups", HasArg::No, O_KEEP_GROUPS),
    LongOpt::new("init-groups", HasArg::No, O_INIT_GROUPS),
    LongOpt::new("groups", HasArg::Required, O_GROUPS),
    LongOpt::new("securebits", HasArg::Required, O_SECUREBITS),
    LongOpt::new("pdeathsig", HasArg::Required, O_PDEATHSIG),
    LongOpt::new("selinux-label", HasArg::Required, O_SELINUX),
    LongOpt::new("apparmor-profile", HasArg::Required, O_APPARMOR),
    LongOpt::new("landlock-access", HasArg::Required, O_LANDLOCK_ACCESS),
    LongOpt::new("landlock-rule", HasArg::Required, O_LANDLOCK_RULE),
    LongOpt::new("reset-env", HasArg::No, O_RESET_ENV),
    LongOpt::new("list-caps", HasArg::No, O_LIST_CAPS),
    LongOpt::new("ptracer", HasArg::Required, O_PTRACER),
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
 {short} [options] <program> [<argument>...]

Run a program with different privilege settings.

Options:
 -d, --dump                  show current state (and do not exec)
 --nnp, --no-new-privs       disallow granting new privileges
 --ambient-caps <caps>       set ambient capabilities
 --inh-caps <caps>           set inheritable capabilities
 --bounding-set <caps>       set capability bounding set
 --ruid <uid|user>           set real uid
 --euid <uid|user>           set effective uid
 --rgid <gid|group>          set real gid
 --egid <gid|group>          set effective gid
 --reuid <uid|user>          set real and effective uid
 --regid <gid|group>         set real and effective gid
 --clear-groups              clear supplementary groups
 --keep-groups               keep supplementary groups
 --init-groups               initialize supplementary groups
 --groups <group>[,...]      set supplementary group(s) by GID or name
 --securebits <bits>         set securebits
 --pdeathsig keep|clear|<signame>
                             set or clear parent death signal
 --ptracer <pid>|any|none    allow ptracing from the given process
 --selinux-label <label>     set SELinux label
 --apparmor-profile <pr>     set AppArmor profile
 --landlock-access <access>  add Landlock access
 --landlock-rule <rule>      add Landlock rule
 --seccomp-filter <file>     load seccomp filter from file
 --reset-env                 clear all environment and initialize
                               HOME, SHELL, USER, LOGNAME and PATH

 -h, --help                  display this help
 -V, --version               display version

 This tool can be dangerous.  Read the manpage, and be careful.

For more details see setpriv(1).

Landlock accesses:
 Access: fs
 Rule types: path-beneath
 Rules: execute,write-file,read-file,read-dir,remove-dir,remove-file,make-char,make-dir,make-reg,make-sock,make-fifo,make-block,make-sym,refer,truncate
"
    )
}

fn die(short: &str, msg: impl AsRef<str>) -> i32 {
    ul::warnx(short, msg);
    1
}

fn fail(short: &str, what: &str) -> i32 {
    ul::warn(short, what, Errno::EPERM);
    EXIT_PRIVERR
}

fn cap_index(name: &str) -> Option<usize> {
    if let Some(n) = name.strip_prefix("cap_") {
        return CAPS.iter().position(|c| *c == n);
    }
    CAPS.iter().position(|c| *c == name)
}

/// Valida `(+|-)cap[,...]` e `all` como o `parse_cap_list` do original.
fn check_cap_list(short: &str, arg: &str) -> Result<(), i32> {
    for tok in arg.split(',') {
        let t = tok.strip_prefix(['+', '-']).unwrap_or(tok);
        if t == "all" {
            continue;
        }
        if let Some(n) = t.strip_prefix("cap_") {
            if n.parse::<u32>().is_ok() {
                continue;
            }
        }
        if t.parse::<u32>().is_ok() || cap_index(t).is_some() {
            continue;
        }
        return Err(die(short, format!("unknown capability \"{t}\"")));
    }
    Ok(())
}

fn cap_names(mask: u64) -> String {
    let v: Vec<&str> = (0..CAPS.len()).filter(|i| mask >> i & 1 == 1).map(|i| CAPS[i]).collect();
    if v.is_empty() {
        "[none]".to_string()
    } else {
        v.join(",")
    }
}

fn status_field(status: &str, key: &str) -> Option<String> {
    status
        .lines()
        .find_map(|l| l.strip_prefix(key).map(|r| r.trim().to_string()))
}

fn hex_field(status: &str, key: &str) -> u64 {
    status_field(status, key)
        .and_then(|v| u64::from_str_radix(&v, 16).ok())
        .unwrap_or(0)
}

fn dump() -> i32 {
    let data = io::read_path(b"/proc/self/status").unwrap_or_default();
    let st = String::from_utf8_lossy(&data).into_owned();
    let ids = |key: &str| -> (String, String) {
        let v = status_field(&st, key).unwrap_or_default();
        let f: Vec<&str> = v.split_whitespace().collect();
        (
            f.first().copied().unwrap_or("0").to_string(),
            f.get(1).copied().unwrap_or("0").to_string(),
        )
    };
    let (ruid, euid) = ids("Uid:");
    let (rgid, egid) = ids("Gid:");
    let groups = status_field(&st, "Groups:").unwrap_or_default();
    let groups: Vec<&str> = groups.split_whitespace().collect();
    let nnp = status_field(&st, "NoNewPrivs:").unwrap_or_else(|| "0".into());
    let mut out = io::stdout();
    let _ = write!(
        out,
        "ruid: {ruid}\neuid: {euid}\nrgid: {rgid}\negid: {egid}\nSupplementary groups: {}\nno_new_privs: {nnp}\nInheritable capabilities: {}\nAmbient capabilities: {}\nCapability bounding set: {}\nSecurebits: [none]\n",
        if groups.is_empty() { "[none]".to_string() } else { groups.join(",") },
        cap_names(hex_field(&st, "CapInh:")),
        cap_names(hex_field(&st, "CapAmb:")),
        cap_names(hex_field(&st, "CapBnd:")),
    );
    0
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);
    let sys = sys::current();

    let mut do_dump = false;
    let mut list_caps = false;
    let mut nnp = false;
    let mut have_groups = false;
    let mut init_groups = false;
    let mut have_ruid_or_reuid = false;
    let mut reset_env = false;
    // O que exigiria uma syscall que o sandbox não tem (nome da falha do original).
    let mut wanted: Vec<&'static str> = Vec::new();
    let mut uids: Vec<(&'static str, u32)> = Vec::new();

    let mut g = Getopt::from_env(&argv[1..], "+dhV", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        let arg = o.arg.clone().unwrap_or_default();
        let text = io::lossy(&arg);
        match o.id {
            x if x == b'd' as i32 => do_dump = true,
            x if x == b'h' as i32 => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
                return 0;
            }
            x if x == b'V' as i32 => {
                ul::print_version(&short);
                return 0;
            }
            O_NNP => nnp = true,
            O_LIST_CAPS => list_caps = true,
            O_RESET_ENV => reset_env = true,
            O_INH | O_AMBIENT | O_BOUNDING => {
                if let Err(c) = check_cap_list(&short, &text) {
                    return c;
                }
                wanted.push(match o.id {
                    O_INH => "set inheritable capabilities",
                    O_AMBIENT => "set ambient capabilities",
                    _ => "set capability bounding set",
                });
            }
            O_RUID | O_EUID | O_REUID => {
                let (name, what) = match o.id {
                    O_RUID => ("ruid", "failed to parse ruid"),
                    O_EUID => ("euid", "failed to parse euid"),
                    _ => ("reuid", "failed to parse reuid"),
                };
                match get_id(&arg, b"/etc/passwd", what) {
                    Ok(v) => uids.push((name, v)),
                    Err(m) => return die(&short, m),
                }
                if o.id != O_EUID {
                    have_ruid_or_reuid = true;
                }
            }
            O_RGID | O_EGID | O_REGID => {
                let (name, what) = match o.id {
                    O_RGID => ("rgid", "failed to parse rgid"),
                    O_EGID => ("egid", "failed to parse egid"),
                    _ => ("regid", "failed to parse regid"),
                };
                match get_id(&arg, b"/etc/group", what) {
                    Ok(v) => uids.push((name, v)),
                    Err(m) => return die(&short, m),
                }
            }
            O_CLEAR_GROUPS | O_KEEP_GROUPS | O_INIT_GROUPS | O_GROUPS => {
                if have_groups {
                    return die(
                        &short,
                        "mutually exclusive arguments: --clear-groups --keep-groups --init-groups --groups",
                    );
                }
                have_groups = true;
                match o.id {
                    O_INIT_GROUPS => init_groups = true,
                    O_GROUPS => {
                        for tok in text.split(',') {
                            if get_id(tok.as_bytes(), b"/etc/group", "").is_err() {
                                return die(&short, format!("group not found: {tok}"));
                            }
                        }
                        wanted.push("setgroups failed");
                    }
                    O_CLEAR_GROUPS => wanted.push("setgroups failed"),
                    _ => {}
                }
            }
            O_SECUREBITS => {
                for tok in text.split(',') {
                    let t = tok.strip_prefix(['+', '-']).unwrap_or(tok);
                    let ok = matches!(
                        t,
                        "noroot"
                            | "noroot_locked"
                            | "no_setuid_fixup"
                            | "no_setuid_fixup_locked"
                            | "keep_caps_locked"
                            | "no_cap_ambient_raise"
                            | "no_cap_ambient_raise_locked"
                    );
                    if !ok {
                        return die(&short, "bad securebits string");
                    }
                }
                wanted.push("set securebits");
            }
            O_PDEATHSIG => {
                if text != "keep" && text != "clear" {
                    let n = text.to_ascii_uppercase();
                    let n = n.strip_prefix("SIG").unwrap_or(&n);
                    if n.parse::<i32>().is_err() && signal::parse_name(text.as_bytes(), &SIGNALS).is_none() {
                        return die(&short, format!("unknown signal: {text}"));
                    }
                }
                if text != "keep" {
                    wanted.push("set parent death signal");
                }
            }
            O_SELINUX => wanted.push("set SELinux label"),
            O_APPARMOR => wanted.push("set AppArmor profile"),
            O_LANDLOCK_ACCESS | O_LANDLOCK_RULE => wanted.push("landlock"),
            O_PTRACER => {
                if text != "none" && text.parse::<u32>().is_err() {
                    return die(&short, format!("invalid ptracer: '{text}'"));
                }
                wanted.push("set ptracer");
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    if list_caps {
        let mut out = io::stdout();
        for c in CAPS {
            let _ = writeln!(out, "{c}");
        }
        return 0;
    }
    let cmd = g.operands();
    if init_groups && !have_ruid_or_reuid && !uids.iter().any(|(n, _)| *n == "reuid" || *n == "ruid") {
        return die(&short, "--init-groups requires --ruid or --reuid");
    }
    if do_dump {
        if !cmd.is_empty() {
            return die(&short, "--dump  does not take a program");
        }
        return dump();
    }
    if cmd.is_empty() {
        ul::warnx(&short, "No program specified");
        return 1;
    }

    // Ids: pedir o valor que o processo já tem funciona; mudar exige setresuid/setresgid.
    let (cuid, cgid) = (sys.geteuid(), sys.getegid());
    for (name, v) in &uids {
        let same = match *name {
            "ruid" | "euid" | "reuid" => *v == cuid,
            _ => *v == cgid,
        };
        if !same {
            let what = if name.ends_with("uid") { "setresuid failed" } else { "setresgid failed" };
            return fail(&short, what);
        }
    }
    if let Some(w) = wanted.first() {
        return fail(&short, w);
    }
    if nnp {
        // PR_SET_NO_NEW_PRIVS sem efeito observável aqui: o exec seguinte não ganha privilégio.
    }
    if reset_env {
        for e in sys.environ() {
            if let Some(p) = e.iter().position(|b| *b == b'=') {
                let _ = sys.unsetenv(&e[..p]);
            }
        }
        let home = b"/root".to_vec();
        let _ = sys.setenv(b"HOME", &home);
        let _ = sys.setenv(b"SHELL", b"/bin/sh");
        let _ = sys.setenv(b"USER", b"root");
        let _ = sys.setenv(b"LOGNAME", b"root");
        let _ = sys.setenv(b"PATH", b"/usr/local/bin:/usr/bin:/bin");
    }

    let _ = io::flush_stdout();
    let e = execvp(&cmd[0], &cmd);
    ul::warn(&short, format!("executing {} failed", io::lossy(&cmd[0])), e);
    if e == Errno::ENOENT {
        EX_EXEC_ENOENT
    } else {
        EX_EXEC_FAILED
    }
}

/// `get_user`/`get_group`: nome em `/etc/passwd` ou `/etc/group`, ou número.
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
