//! `dpkg-architecture` do dpkg 1.22 (Debian 13): calcula as variáveis `DEB_{BUILD,HOST,TARGET}_*`
//! pela tabela de CPUs e de tuplas do dpkg.
//!
//! Comandos: `-l` (padrão), `-L`, `-e`, `-i`, `-q`, `-s`, `-u`, `-c`, `--help` e `--version`. Opções:
//! `-a`, `-t`, `-A`, `-T`, `-W`, `-B`, `-E` e `-f`. A máquina de construção é sempre `amd64`; as
//! variáveis `DEB_HOST_*` e `DEB_TARGET_*` do ambiente valem quando não há `-a`/`-t` (e `-f` as
//! ignora).
//!
//! Divergências conhecidas: o texto exato do `--help` e a lista do `-L` são de memória.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;

const PROG: &str = "dpkg-architecture";

const USAGE: &str = "Usage: dpkg-architecture [<option>...] [<command>]

Commands:
  -l, --list                 list variables (default).
  -L, --list-known           list valid architectures (matching some criteria).
  -e, --equal <arch>         compare with host Debian architecture.
  -i, --is <arch-wildcard>   match against host Debian architecture.
  -q, --query <variable>     prints only the value of <variable>.
  -s, --print-set            print command to set environment variables.
  -u, --print-unset          print command to unset environment variables.
  -c, --command <command>    set environment and run <command> in it.
  -?, --help                 show this help message.
      --version              show the version.

Options:
  -a, --host-arch <arch>     set host Debian architecture.
  -t, --host-type <type>     set host GNU system type.
  -A, --target-arch <arch>   set target Debian architecture.
  -T, --target-type <type>   set target GNU system type.
  -W, --match-wildcard <arch-wildcard>
                             restrict architecture list matching <arch-wildcard>.
  -B, --match-bits <arch-bits>
                             restrict architecture list matching <arch-bits>.
  -E, --match-endian <arch-endian>
                             restrict architecture list matching <arch-endian>.
  -f, --force                force flag (override variables set in environment).
";

const VERSION: &str = "Debian dpkg-architecture version 1.22.22.

This is free software; see the GNU General Public License version 2 or
later for copying conditions. There is NO warranty.
";

/// `(nome Debian, nome GNU, bits, endian)`; o padrão de reconhecimento do dpkg não é usado aqui.
const CPUS: &[(&str, &str, u32, &str)] = &[
    ("alpha", "alpha", 64, "little"),
    ("amd64", "x86_64", 64, "little"),
    ("arc", "arc", 32, "little"),
    ("arm", "arm", 32, "little"),
    ("arm64", "aarch64", 64, "little"),
    ("armeb", "armeb", 32, "big"),
    ("avr32", "avr32", 32, "big"),
    ("hppa", "hppa", 32, "big"),
    ("i386", "i686", 32, "little"),
    ("ia64", "ia64", 64, "little"),
    ("loong64", "loongarch64", 64, "little"),
    ("m32r", "m32r", 32, "big"),
    ("m68k", "m68k", 32, "big"),
    ("mips", "mips", 32, "big"),
    ("mips64", "mips64", 64, "big"),
    ("mips64el", "mips64el", 64, "little"),
    ("mips64r6", "mipsisa64r6", 64, "big"),
    ("mips64r6el", "mipsisa64r6el", 64, "little"),
    ("mipsel", "mipsel", 32, "little"),
    ("mipsr6", "mipsisa32r6", 32, "big"),
    ("mipsr6el", "mipsisa32r6el", 32, "little"),
    ("nios2", "nios2", 32, "little"),
    ("or1k", "or1k", 32, "big"),
    ("powerpc", "powerpc", 32, "big"),
    ("powerpcel", "powerpcle", 32, "little"),
    ("ppc64", "powerpc64", 64, "big"),
    ("ppc64el", "powerpc64le", 64, "little"),
    ("riscv64", "riscv64", 64, "little"),
    ("s390", "s390", 32, "big"),
    ("s390x", "s390x", 64, "big"),
    ("sh3", "sh3", 32, "little"),
    ("sh3eb", "sh3eb", 32, "big"),
    ("sh4", "sh4", 32, "little"),
    ("sh4eb", "sh4eb", 32, "big"),
    ("sparc", "sparc", 32, "big"),
    ("sparc64", "sparc64", 64, "big"),
    ("tilegx", "tilegx", 64, "little"),
];

/// `(abi, libc, os, cpu)` com o nome Debian da arquitetura, ou `<cpu>` pra qualquer CPU.
const TUPLES: &[(&str, &str, &str, &str, &str)] = &[
    ("eabi", "uclibc", "linux", "arm", "uclibc-linux-armel"),
    ("base", "uclibc", "linux", "<cpu>", "uclibc-linux-<cpu>"),
    ("eabihf", "musl", "linux", "arm", "musl-linux-armhf"),
    ("eabi", "musl", "linux", "arm", "musl-linux-armel"),
    ("base", "musl", "linux", "<cpu>", "musl-linux-<cpu>"),
    ("eabihf", "gnu", "linux", "arm", "armhf"),
    ("eabi", "gnu", "linux", "arm", "armel"),
    ("abin32", "gnu", "linux", "mips64r6el", "mipsn32r6el"),
    ("abin32", "gnu", "linux", "mips64r6", "mipsn32r6"),
    ("abin32", "gnu", "linux", "mips64el", "mipsn32el"),
    ("abin32", "gnu", "linux", "mips64", "mipsn32"),
    ("abi64", "gnu", "linux", "mips64r6el", "mips64r6el"),
    ("abi64", "gnu", "linux", "mips64r6", "mips64r6"),
    ("abi64", "gnu", "linux", "mips64el", "mips64el"),
    ("abi64", "gnu", "linux", "mips64", "mips64"),
    ("spe", "gnu", "linux", "powerpc", "powerpcspe"),
    ("x32", "gnu", "linux", "amd64", "x32"),
    ("base", "gnu", "linux", "<cpu>", "<cpu>"),
    ("eabihf", "gnu", "kfreebsd", "arm", "kfreebsd-armhf"),
    ("base", "gnu", "kfreebsd", "<cpu>", "kfreebsd-<cpu>"),
    ("base", "gnu", "knetbsd", "<cpu>", "knetbsd-<cpu>"),
    ("base", "gnu", "kopensolaris", "<cpu>", "kopensolaris-<cpu>"),
    ("base", "gnu", "hurd", "<cpu>", "hurd-<cpu>"),
    ("base", "bsd", "dragonflybsd", "<cpu>", "dragonflybsd-<cpu>"),
    ("base", "bsd", "freebsd", "<cpu>", "freebsd-<cpu>"),
    ("base", "bsd", "openbsd", "<cpu>", "openbsd-<cpu>"),
    ("base", "bsd", "netbsd", "<cpu>", "netbsd-<cpu>"),
    ("base", "bsd", "darwin", "<cpu>", "darwin-<cpu>"),
    ("base", "sysv", "aix", "<cpu>", "aix-<cpu>"),
    ("base", "sysv", "solaris", "<cpu>", "solaris-<cpu>"),
    ("base", "tos", "mint", "m68k", "mint-m68k"),
];

#[derive(Clone, Debug)]
struct Arch {
    name: String,
    abi: String,
    libc: String,
    os: String,
    cpu: String,
}

fn cpu_info(cpu: &str) -> Option<&'static (&'static str, &'static str, u32, &'static str)> {
    CPUS.iter().find(|c| c.0 == cpu)
}

/// Todos os pares nome/tupla conhecidos.
fn all_arches() -> Vec<Arch> {
    let mut v = Vec::new();
    for t in TUPLES {
        if t.3 == "<cpu>" {
            for c in CPUS {
                v.push(Arch {
                    name: t.4.replace("<cpu>", c.0),
                    abi: t.0.to_string(),
                    libc: t.1.to_string(),
                    os: t.2.to_string(),
                    cpu: c.0.to_string(),
                });
            }
        } else {
            v.push(Arch {
                name: t.4.to_string(),
                abi: t.0.to_string(),
                libc: t.1.to_string(),
                os: t.2.to_string(),
                cpu: t.3.to_string(),
            });
        }
    }
    v
}

fn arch_by_name(name: &str) -> Option<Arch> {
    all_arches().into_iter().find(|a| a.name == name)
}

impl Arch {
    fn gnu_system(&self) -> String {
        let abi = match self.abi.as_str() {
            "base" => String::new(),
            a => a.to_string(),
        };
        match (self.os.as_str(), self.libc.as_str()) {
            ("linux", libc) => format!("linux-{libc}{abi}"),
            ("kfreebsd", _) => "kfreebsd-gnu".to_string(),
            ("knetbsd", _) => "knetbsd-gnu".to_string(),
            ("kopensolaris", _) => "kopensolaris-gnu".to_string(),
            ("hurd", _) => "gnu".to_string(),
            ("mint", _) => "mint".to_string(),
            (os, _) => os.to_string(),
        }
    }

    fn gnu_cpu(&self) -> String {
        cpu_info(&self.cpu).map(|c| c.1.to_string()).unwrap_or_else(|| self.cpu.clone())
    }

    fn gnu_type(&self) -> String {
        format!("{}-{}", self.gnu_cpu(), self.gnu_system())
    }

    fn multiarch(&self) -> String {
        if self.cpu == "i386" {
            return match self.os.as_str() {
                "linux" => "i386-linux-gnu".to_string(),
                "kfreebsd" => "i386-kfreebsd-gnu".to_string(),
                "hurd" => "i386-gnu".to_string(),
                _ => self.gnu_type(),
            };
        }
        self.gnu_type()
    }

    fn bits(&self) -> u32 {
        if self.abi == "x32" || self.abi == "abin32" {
            return 32;
        }
        cpu_info(&self.cpu).map(|c| c.2).unwrap_or(0)
    }

    fn endian(&self) -> &'static str {
        cpu_info(&self.cpu).map(|c| c.3).unwrap_or("")
    }
}

fn uerr(msg: &str) -> i32 {
    let _ = io::flush_stdout();
    io::eprint(format!(
        "{PROG}: error: {msg}\n\nUse '{PROG} --help' for program usage information.\n"
    ));
    2
}

fn die(msg: &str) -> i32 {
    let _ = io::flush_stdout();
    io::eprint(format!("{PROG}: error: {msg}\n"));
    255
}

fn out(s: &str) {
    let mut o = io::stdout();
    let _ = o.write_all(s.as_bytes());
}

/// Casa a arquitetura com um curinga (`any`, `linux-any`, `any-amd64`, `musl-linux-any`...).
fn wildcard_match(a: &Arch, wildcard: &str) -> bool {
    if wildcard == a.name {
        return true;
    }
    let mut parts: Vec<String> = wildcard.split('-').map(str::to_string).collect();
    if parts.len() > 4 {
        return false;
    }
    if !wildcard.contains("any") {
        // Nome Debian puro que não é a própria arquitetura.
        return match arch_by_name(wildcard) {
            Some(w) => w.abi == a.abi && w.libc == a.libc && w.os == a.os && w.cpu == a.cpu,
            None => false,
        };
    }
    while parts.len() < 4 {
        parts.insert(0, "any".to_string());
    }
    let fields = [&a.abi, &a.libc, &a.os, &a.cpu];
    parts.iter().zip(fields.iter()).all(|(p, f)| p == "any" || p == *f)
}

fn host_build_arch() -> Arch {
    arch_by_name("amd64").unwrap_or(Arch {
        name: "amd64".into(),
        abi: "base".into(),
        libc: "gnu".into(),
        os: "linux".into(),
        cpu: "amd64".into(),
    })
}

/// As variáveis de um prefixo (`BUILD`, `HOST`, `TARGET`).
fn vars(prefix: &str, a: &Arch, gnu_type: Option<&str>) -> Vec<(String, String)> {
    let (gc, gs, gt) = match gnu_type {
        Some(t) => {
            let (c, s) = t.split_once('-').unwrap_or((t, ""));
            (c.to_string(), s.to_string(), t.to_string())
        }
        None => (a.gnu_cpu(), a.gnu_system(), a.gnu_type()),
    };
    let k = |s: &str| format!("DEB_{prefix}_{s}");
    vec![
        (k("ARCH"), a.name.clone()),
        (k("ARCH_ABI"), a.abi.clone()),
        (k("ARCH_BITS"), a.bits().to_string()),
        (k("ARCH_CPU"), a.cpu.clone()),
        (k("ARCH_ENDIAN"), a.endian().to_string()),
        (k("ARCH_LIBC"), a.libc.clone()),
        (k("ARCH_OS"), a.os.clone()),
        (k("GNU_CPU"), gc),
        (k("GNU_SYSTEM"), gs),
        (k("GNU_TYPE"), gt),
        (k("MULTIARCH"), a.multiarch()),
    ]
}

/// Resolve `-a`/`-t` (ou o ambiente) numa arquitetura e num tipo GNU opcional.
fn resolve(kind: &str, arch: Option<String>, gnu: Option<String>, def: &Arch) -> Result<(Arch, Option<String>), i32> {
    match (arch, gnu) {
        (None, None) => Ok((def.clone(), None)),
        (Some(a), g) => match arch_by_name(&a) {
            Some(x) => Ok((x, g)),
            None => match g {
                Some(g) => {
                    // Arquitetura desconhecida com tipo GNU explícito: usa o tipo como veio.
                    let cpu = g.split('-').next().unwrap_or("").to_string();
                    Ok((
                        Arch { name: a, abi: "base".into(), libc: "gnu".into(), os: "linux".into(), cpu },
                        Some(g),
                    ))
                }
                None => Err(die(&format!(
                    "unknown Debian architecture {a}, you must specify GNU system type, too"
                ))),
            },
        },
        (None, Some(g)) => match all_arches().into_iter().find(|x| x.gnu_type() == g) {
            Some(x) => Ok((x, None)),
            None => Err(die(&format!(
                "unknown GNU system type {g} for {kind}, you must specify Debian architecture, too"
            ))),
        },
    }
}

fn env(name: &str) -> Option<String> {
    sys::try_current()
        .and_then(|s| s.getenv(name.as_bytes()))
        .map(|v| String::from_utf8_lossy(&v).into_owned())
        .filter(|v| !v.is_empty())
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

#[derive(PartialEq)]
enum Cmd {
    List,
    ListKnown,
    Equal(String),
    Is(String),
    Query(String),
    PrintSet,
    PrintUnset,
    Command,
}

fn run(args: &[OsString]) -> i32 {
    let argv: Vec<String> = io::args_bytes(args)
        .iter()
        .skip(1)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect();
    let mut host_arch: Option<String> = None;
    let mut host_type: Option<String> = None;
    let mut target_arch: Option<String> = None;
    let mut target_type: Option<String> = None;
    let mut force = false;
    let mut cmd: Option<Cmd> = None;
    let mut mw: Option<String> = None;
    let mut mb: Option<String> = None;
    let mut me: Option<String> = None;
    let mut command: Vec<String> = Vec::new();

    let mut i = 0usize;
    while i < argv.len() {
        let a = argv[i].clone();
        i += 1;
        // Normaliza `-aVALOR` e `--opt=valor` em (opção, valor opcional).
        let (opt, inline): (String, Option<String>) = if let Some(l) = a.strip_prefix("--") {
            match l.split_once('=') {
                Some((n, v)) => (format!("--{n}"), Some(v.to_string())),
                None => (a.clone(), None),
            }
        } else if a.starts_with('-') && a.len() > 2 {
            (a[..2].to_string(), Some(a[2..].to_string()))
        } else {
            (a.clone(), None)
        };
        let takes = matches!(
            opt.as_str(),
            "-a" | "--host-arch" | "-t" | "--host-type" | "-A" | "--target-arch" | "-T" | "--target-type"
                | "-W" | "--match-wildcard" | "-B" | "--match-bits" | "-E" | "--match-endian" | "-e" | "--equal"
                | "-i" | "--is" | "-q" | "--query"
        );
        let mut val = String::new();
        if takes {
            match inline.clone() {
                Some(v) => val = v,
                None => {
                    if i < argv.len() {
                        i += 1;
                        val = argv[i - 1].clone();
                    } else {
                        return uerr(&format!("missing value for option {opt}"));
                    }
                }
            }
        } else if inline.is_some() && !opt.starts_with("--") {
            // Opções curtas sem valor agrupadas (`-lf`) não são aceitas pelo Getopt::Long do dpkg.
            return uerr(&format!("unknown option or argument {a}"));
        }
        let mut setcmd = |c: Cmd| -> bool {
            cmd = Some(c);
            true
        };
        match opt.as_str() {
            "-a" | "--host-arch" => host_arch = Some(val),
            "-t" | "--host-type" => host_type = Some(val),
            "-A" | "--target-arch" => target_arch = Some(val),
            "-T" | "--target-type" => target_type = Some(val),
            "-W" | "--match-wildcard" => mw = Some(val),
            "-B" | "--match-bits" => mb = Some(val),
            "-E" | "--match-endian" => me = Some(val),
            "-f" | "--force" => force = true,
            "-l" | "--list" => {
                setcmd(Cmd::List);
            }
            "-L" | "--list-known" => {
                setcmd(Cmd::ListKnown);
            }
            "-e" | "--equal" => {
                setcmd(Cmd::Equal(val));
            }
            "-i" | "--is" => {
                setcmd(Cmd::Is(val));
            }
            "-q" | "--query" => {
                setcmd(Cmd::Query(val));
            }
            "-s" | "--print-set" => {
                setcmd(Cmd::PrintSet);
            }
            "-u" | "--print-unset" => {
                setcmd(Cmd::PrintUnset);
            }
            "-c" | "--command" => {
                setcmd(Cmd::Command);
                command = argv[i..].to_vec();
                if let Some(v) = inline {
                    command.insert(0, v);
                }
                i = argv.len();
            }
            "-?" | "--help" => {
                out(USAGE);
                return 0;
            }
            "--version" => {
                out(VERSION);
                return 0;
            }
            _ => return uerr(&format!("unknown option or argument {a}")),
        }
    }

    let build = host_build_arch();
    if cmd == Some(Cmd::ListKnown) {
        let mut names: Vec<String> = all_arches()
            .into_iter()
            .filter(|a| mw.as_deref().is_none_or(|w| wildcard_match(a, w)))
            .filter(|a| mb.as_deref().is_none_or(|b| a.bits().to_string() == b))
            .filter(|a| me.as_deref().is_none_or(|e| a.endian() == e))
            .map(|a| a.name)
            .collect();
        names.sort();
        names.dedup();
        let mut s = String::new();
        for n in names {
            s.push_str(&n);
            s.push('\n');
        }
        out(&s);
        return 0;
    }

    // Anfitrião e alvo: opção, senão ambiente (a menos de `-f`), senão a máquina de construção.
    let host_env_arch = if force || host_arch.is_some() || host_type.is_some() { None } else { env("DEB_HOST_ARCH") };
    let host_env_type =
        if force || host_arch.is_some() || host_type.is_some() { None } else { env("DEB_HOST_GNU_TYPE") };
    let (host, host_gnu) = match resolve(
        "host",
        host_arch.clone().or(host_env_arch),
        host_type.clone().or(host_env_type),
        &build,
    ) {
        Ok(r) => r,
        Err(c) => return c,
    };
    let tgt_set = target_arch.is_some() || target_type.is_some();
    let t_env_arch = if force || tgt_set { None } else { env("DEB_TARGET_ARCH") };
    let t_env_type = if force || tgt_set { None } else { env("DEB_TARGET_GNU_TYPE") };
    let have_target = target_arch.is_some() || target_type.is_some() || t_env_arch.is_some() || t_env_type.is_some();
    let (target, target_gnu) = if have_target {
        match resolve("target", target_arch.or(t_env_arch), target_type.or(t_env_type), &host) {
            Ok(r) => r,
            Err(c) => return c,
        }
    } else {
        (host.clone(), host_gnu.clone())
    };

    let mut all: Vec<(String, String)> = Vec::new();
    all.extend(vars("BUILD", &build, None));
    all.extend(vars("HOST", &host, host_gnu.as_deref()));
    all.extend(vars("TARGET", &target, target_gnu.as_deref()));

    match cmd.unwrap_or(Cmd::List) {
        Cmd::Equal(a) => {
            if host.name == a { 0 } else { 1 }
        }
        Cmd::Is(w) => {
            if wildcard_match(&host, &w) { 0 } else { 1 }
        }
        Cmd::Query(q) => match all.iter().find(|(k, _)| *k == q) {
            Some((_, v)) => {
                out(&format!("{v}\n"));
                0
            }
            None => uerr(&format!("{q} is not a supported variable name")),
        },
        Cmd::PrintSet => {
            let sets: Vec<String> = all.iter().map(|(k, v)| format!("{k}={v};")).collect();
            let names: Vec<&str> = all.iter().map(|(k, _)| k.as_str()).collect();
            out(&format!("{} export {}\n", sets.join(" "), names.join(" ")));
            0
        }
        Cmd::PrintUnset => {
            let names: Vec<&str> = all.iter().map(|(k, _)| k.as_str()).collect();
            out(&format!("unset {}\n", names.join(" ")));
            0
        }
        Cmd::Command => {
            if command.is_empty() {
                return uerr("missing command");
            }
            exec_with(&command, &all)
        }
        _ => {
            let mut s = String::new();
            for (k, v) in &all {
                s.push_str(&format!("{k}={v}\n"));
            }
            out(&s);
            0
        }
    }
}

/// `exec` do comando com as variáveis somadas ao ambiente corrente.
fn exec_with(command: &[String], vars: &[(String, String)]) -> i32 {
    let sc = sys::current();
    let mut envp: Vec<Vec<u8>> = sc
        .environ()
        .into_iter()
        .filter(|e| {
            let name = e.split(|&b| b == b'=').next().unwrap_or(&[]);
            !vars.iter().any(|(k, _)| k.as_bytes() == name)
        })
        .collect();
    for (k, v) in vars {
        envp.push(format!("{k}={v}").into_bytes());
    }
    let argv: Vec<Vec<u8>> = command.iter().map(|s| s.clone().into_bytes()).collect();
    let prog = command[0].as_bytes().to_vec();
    let mut candidates: Vec<Vec<u8>> = Vec::new();
    if prog.contains(&b'/') {
        candidates.push(prog.clone());
    } else {
        let path = sc.getenv(b"PATH").unwrap_or_else(|| b"/usr/local/bin:/usr/bin:/bin".to_vec());
        for d in path.split(|&b| b == b':') {
            let mut p = if d.is_empty() { b".".to_vec() } else { d.to_vec() };
            p.push(b'/');
            p.extend_from_slice(&prog);
            candidates.push(p);
        }
    }
    let mut last = sysabi::Errno::ENOENT;
    for c in candidates {
        if sys::stat(&c).is_err() {
            continue;
        }
        last = sc.execve(&c, &argv, Some(&envp));
    }
    let _ = io::flush_stdout();
    io::eprint(format!("{PROG}: error: cannot execute program {}: {}\n", command[0], last.message()));
    255
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amd64_vars() {
        let a = arch_by_name("amd64").unwrap();
        assert_eq!(a.gnu_type(), "x86_64-linux-gnu");
        assert_eq!(a.bits(), 64);
        let h = arch_by_name("armhf").unwrap();
        assert_eq!(h.gnu_type(), "arm-linux-gnueabihf");
        assert_eq!(arch_by_name("i386").unwrap().multiarch(), "i386-linux-gnu");
        assert_eq!(arch_by_name("mips64el").unwrap().gnu_type(), "mips64el-linux-gnuabi64");
        assert_eq!(arch_by_name("x32").unwrap().gnu_type(), "x86_64-linux-gnux32");
    }

    #[test]
    fn wildcards() {
        let a = arch_by_name("amd64").unwrap();
        assert!(wildcard_match(&a, "any"));
        assert!(wildcard_match(&a, "linux-any"));
        assert!(wildcard_match(&a, "any-amd64"));
        assert!(!wildcard_match(&a, "any-i386"));
        assert!(!wildcard_match(&a, "kfreebsd-any"));
    }
}
