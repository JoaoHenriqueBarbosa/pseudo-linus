//! A imagem base do sandbox: a árvore de um container Debian 13 (`debian:trixie`), com os arquivos de
//! `/etc` copiados do oráculo da bancada (`crates/kernel/image/`), um executável por programa embutido em
//! `/usr/bin` e `/usr/sbin`, e o `/dev` mínimo.

use sysabi::{AtFlags, Errno, OFlags, Program, SetTime, TimeSpec};
use vfs::{Caller, Namespace, Opened, Start, WritePos, makedev};

use crate::dev;
use crate::exec::builtin_file;

/// Tamanhos dos executáveis reais do Debian 13, tirados do oráculo (`real/sizes.txt`).
const REAL_SIZES: &str = include_str!("../real/sizes.txt");

/// O tamanho do executável `path` no Debian, quando ele é maior que o molde do ELF embutido.
fn real_size(path: &str) -> Option<u64> {
    REAL_SIZES
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once(' '))
        .find(|(_, p)| *p == path)
        .and_then(|(s, _)| s.parse::<u64>().ok())
        .filter(|s| *s > crate::exec::REAL_TRUE.len() as u64)
}

/// mtime dos arquivos da imagem: 2026-09-18 00:00:00 UTC (a data da imagem do oráculo).
pub(crate) const IMAGE_TIME: TimeSpec = TimeSpec { sec: 1_789_689_600, nsec: 0 };

/// Os symlinks de programas do Debian 13 (`real/links.txt`).
const REAL_LINKS: &str = include_str!("../real/links.txt");

fn real_links() -> impl Iterator<Item = (&'static str, &'static str)> {
    REAL_LINKS.lines().filter(|l| !l.starts_with('#')).filter_map(|l| l.split_once(' '))
}

/// O conteúdo do symlink `path` no Debian, se ele é um dos links de programa da tabela.
pub(crate) fn debian_link(path: &str) -> Option<&'static [u8]> {
    real_links().find(|(l, _)| *l == path).map(|(_, t)| t.as_bytes())
}

/// O caminho absoluto que o link `link` com conteúdo `target` aponta.
fn link_dest(link: &str, target: &str) -> String {
    if target.starts_with('/') {
        return target.to_string();
    }
    let dir = link.rsplit_once('/').map_or("", |(d, _)| d);
    let mut parts: Vec<&str> = dir.split('/').filter(|c| !c.is_empty()).collect();
    for comp in target.split('/') {
        match comp {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            c => parts.push(c),
        }
    }
    format!("/{}", parts.join("/"))
}

/// Se o link `path` da tabela chega, seguindo a tabela, a um dos `programs` ou a um arquivo
/// copiado do oráculo.
fn link_reaches_program(path: &str, programs: &[String], depth: usize) -> bool {
    let Some((_, target)) = real_links().find(|(l, _)| *l == path) else {
        return programs.iter().any(|p| p == path) || COPIED_TREES.iter().any(|i| matches!(i, File(p, _, _) if *p == path));
    };
    depth < 8 && link_reaches_program(&link_dest(path, target), programs, depth + 1)
}

/// Grupo `shadow` e grupo `mail` do Debian.
const GID_SHADOW: u32 = 42;
const GID_MAIL: u32 = 8;

enum Item {
    Dir(&'static str, u32),
    File(&'static str, &'static [u8], u32),
    Link(&'static str, &'static str),
}

use Item::*;

// `/usr/share/zoneinfo` do tzdata e `/usr/share/terminfo`, `/usr/share/tabset` e `/etc/terminfo` do
// ncurses-base do Debian 13 (copiados do oráculo; ver build.rs).
include!(concat!(env!("OUT_DIR"), "/copied_trees.rs"));

const ROOT_TREE: &[Item] = &[
    Link("/bin", "usr/bin"),
    Dir("/boot", 0o755),
    Dir("/dev", 0o755),
    Dir("/etc", 0o755),
    Dir("/home", 0o755),
    Link("/lib", "usr/lib"),
    Link("/lib64", "usr/lib64"),
    Dir("/media", 0o755),
    Dir("/mnt", 0o755),
    Dir("/opt", 0o755),
    Dir("/proc", 0o555),
    Dir("/root", 0o700),
    Dir("/run", 0o755),
    Dir("/run/lock", 0o1777),
    Link("/sbin", "usr/sbin"),
    Dir("/srv", 0o755),
    Dir("/sys", 0o555),
    Dir("/tmp", 0o1777),
    Dir("/usr", 0o755),
    Dir("/usr/bin", 0o755),
    Dir("/usr/games", 0o755),
    Dir("/usr/include", 0o755),
    Dir("/usr/lib", 0o755),
    Dir("/usr/lib64", 0o755),
    Dir("/usr/libexec", 0o755),
    Dir("/usr/local", 0o755),
    Dir("/usr/local/bin", 0o755),
    Dir("/usr/local/etc", 0o755),
    Dir("/usr/local/games", 0o755),
    Dir("/usr/local/include", 0o755),
    Dir("/usr/local/lib", 0o755),
    Dir("/usr/local/libexec", 0o755),
    Link("/usr/local/man", "share/man"),
    Dir("/usr/local/sbin", 0o755),
    Dir("/usr/local/share", 0o755),
    Dir("/usr/local/src", 0o755),
    Dir("/usr/sbin", 0o755),
    Dir("/usr/share", 0o755),
    Dir("/usr/src", 0o755),
    Dir("/var", 0o755),
    Dir("/var/backups", 0o755),
    Dir("/var/cache", 0o755),
    Dir("/var/lib", 0o755),
    Dir("/var/local", 0o755),
    Link("/var/lock", "/run/lock"),
    Dir("/var/log", 0o755),
    Dir("/var/mail", 0o2775),
    Dir("/var/opt", 0o755),
    Link("/var/run", "/run"),
    Dir("/var/spool", 0o755),
    Dir("/var/tmp", 0o1777),
    Dir("/work", 0o755),
    Dir("/etc/skel", 0o755),
    File("/usr/lib/os-release", include_bytes!("../image/usr/lib/os-release"), 0o644),
    Link("/etc/os-release", "../usr/lib/os-release"),
    File("/etc/passwd", include_bytes!("../image/etc/passwd"), 0o644),
    File("/etc/group", include_bytes!("../image/etc/group"), 0o644),
    File("/etc/shadow", include_bytes!("../image/etc/shadow"), 0o640),
    File("/etc/gshadow", include_bytes!("../image/etc/gshadow"), 0o640),
    File("/etc/debian_version", include_bytes!("../image/etc/debian_version"), 0o644),
    File("/etc/profile", include_bytes!("../image/etc/profile"), 0o644),
    File("/etc/bash.bashrc", include_bytes!("../image/etc/bash.bashrc"), 0o644),
    File("/etc/shells", include_bytes!("../image/etc/shells"), 0o644),
    File("/etc/nsswitch.conf", include_bytes!("../image/etc/nsswitch.conf"), 0o644),
    File("/etc/issue", include_bytes!("../image/etc/issue"), 0o644),
    File("/etc/issue.net", include_bytes!("../image/etc/issue.net"), 0o644),
    File("/etc/environment", include_bytes!("../image/etc/environment"), 0o644),
    File("/etc/motd", include_bytes!("../image/etc/motd"), 0o644),
    File("/etc/host.conf", include_bytes!("../image/etc/host.conf"), 0o644),
    // Do netbase 6.5; o /etc/networks é o do contêiner do oráculo (fora de pacote).
    File("/etc/services", include_bytes!("../image/etc/services"), 0o644),
    File("/etc/protocols", include_bytes!("../image/etc/protocols"), 0o644),
    File("/etc/rpc", include_bytes!("../image/etc/rpc"), 0o644),
    File("/etc/networks", include_bytes!("../image/etc/networks"), 0o644),
    File("/etc/fstab", include_bytes!("../image/etc/fstab"), 0o644),
    // Como no container: UTC, e o glibc lê o tzfile por este link.
    Link("/etc/localtime", "/usr/share/zoneinfo/Etc/UTC"),
    Link("/etc/mtab", "/proc/mounts"),
    File("/etc/skel/.bashrc", include_bytes!("../image/etc/skel/.bashrc"), 0o644),
    File("/etc/skel/.profile", include_bytes!("../image/etc/skel/.profile"), 0o644),
    File("/etc/skel/.bash_logout", include_bytes!("../image/etc/skel/.bash_logout"), 0o644),
    File("/root/.bashrc", include_bytes!("../image/root/.bashrc"), 0o644),
    File("/root/.profile", include_bytes!("../image/root/.profile"), 0o644),
];

fn mkdir(ns: &Namespace, cx: &Caller, path: &[u8], mode: u32) -> Result<(), Errno> {
    match ns.mkdir(cx, &Start::Cwd, path, mode) {
        Ok(()) | Err(Errno::EEXIST) => {}
        Err(e) => return Err(e),
    }
    // mkdir aplica só 0o1777; setgid e o modo exato vêm do chmod.
    ns.chmod(cx, &Start::Cwd, path, mode, AtFlags::empty())
}

/// Cria (ou substitui) um arquivo regular com conteúdo e modo exatos.
pub(crate) fn put_file(ns: &Namespace, cx: &Caller, path: &[u8], data: &[u8], mode: u32) -> Result<(), Errno> {
    put_file_sized(ns, cx, path, data, mode, None)
}

/// [`put_file`] que estende o arquivo até `size`: o que passa de `data` vira buraco, que não ocupa
/// memória (é como um executável embutido ganha o tamanho do binário real do Debian).
fn put_file_sized(ns: &Namespace, cx: &Caller, path: &[u8], data: &[u8], mode: u32, size: Option<u64>) -> Result<(), Errno> {
    let o = ns.open(cx, &Start::Cwd, path, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::NOFOLLOW, mode & 0o7777)?;
    match o {
        Opened::File { handle, loc, .. } => {
            let mut off = 0usize;
            while off < data.len() {
                let (n, _) = handle.write(cx, WritePos::At(off as u64), &data[off..])?;
                off += n;
            }
            if let Some(size) = size {
                ns.truncate_loc(cx, &loc, size, false)?;
            }
        }
        _ => return Err(Errno::EISDIR),
    }
    ns.chmod(cx, &Start::Cwd, path, mode, AtFlags::empty())
}

fn stamp(ns: &Namespace, cx: &Caller, path: &[u8]) -> Result<(), Errno> {
    ns.utimens(cx, &Start::Cwd, path, SetTime::At(IMAGE_TIME), SetTime::At(IMAGE_TIME), AtFlags::SYMLINK_NOFOLLOW)
}

/// Monta a raiz: árvore, `/etc` e um executável por programa.
pub(crate) fn build_root(ns: &Namespace, cx: &Caller, programs: &[Program], hostname: &str) -> Result<(), Errno> {
    let mut stamped: Vec<Vec<u8>> = Vec::new();
    for item in ROOT_TREE.iter().chain(COPIED_TREES) {
        match item {
            Dir(p, m) => mkdir(ns, cx, p.as_bytes(), *m)?,
            File(p, data, m) => put_file(ns, cx, p.as_bytes(), data, *m)?,
            Link(p, t) => ns.symlink(cx, t.as_bytes(), &Start::Cwd, p.as_bytes())?,
        }
        let p = match item {
            Dir(p, _) | File(p, _, _) | Link(p, _) => p,
        };
        stamped.push(p.as_bytes().to_vec());
    }
    ns.chown(cx, &Start::Cwd, b"/etc/shadow", None, Some(GID_SHADOW), AtFlags::empty())?;
    ns.chown(cx, &Start::Cwd, b"/etc/gshadow", None, Some(GID_SHADOW), AtFlags::empty())?;
    ns.chown(cx, &Start::Cwd, b"/var/mail", None, Some(GID_MAIL), AtFlags::empty())?;
    put_file(ns, cx, b"/etc/hostname", format!("{hostname}\n").as_bytes(), 0o644)?;
    // O /etc/hosts do contêiner do oráculo (`--network none`): sem linha pro nome da máquina, então
    // `hostid` dá 00000000 e `hostname -f`/`-i` falham com "Temporary failure in name resolution".
    let hosts = "127.0.0.1\tlocalhost\n::1\tlocalhost ip6-localhost ip6-loopback\nfe00::0\tip6-localnet\nff00::0\tip6-mcastprefix\nff02::1\tip6-allnodes\nff02::2\tip6-allrouters\n";
    put_file(ns, cx, b"/etc/hosts", hosts.as_bytes(), 0o644)?;
    stamped.push(b"/etc/hostname".to_vec());
    stamped.push(b"/etc/hosts".to_vec());
    let program_paths: Vec<String> = programs.iter().map(Program::path).collect();
    for p in programs {
        let path = p.path();
        // Diretório fora da árvore padrão: cria os pais.
        let mut acc = Vec::new();
        for comp in p.dir.split('/').filter(|c| !c.is_empty()) {
            acc.push(b'/');
            acc.extend_from_slice(comp.as_bytes());
            match ns.mkdir(cx, &Start::Cwd, &acc, 0o755) {
                Ok(()) | Err(Errno::EEXIST) => {}
                Err(e) => return Err(e),
            }
        }
        // No Debian, este nome é um symlink para outro programa: vira o link, mais abaixo.
        if real_links().any(|(l, _)| l == path) && link_reaches_program(&path, &program_paths, 0) {
            continue;
        }
        put_file_sized(ns, cx, path.as_bytes(), &builtin_file(&path), 0o755, real_size(&path))?;
        stamped.push(path.into_bytes());
    }
    // Os symlinks do Debian que chegam a algo da imagem (programa, diretório ou arquivo copiado).
    let mut links: Vec<&str> = Vec::new();
    for (link, target) in real_links() {
        match ns.symlink(cx, target.as_bytes(), &Start::Cwd, link.as_bytes()) {
            Ok(()) => links.push(link),
            Err(Errno::EEXIST | Errno::ENOENT) => {}
            Err(e) => return Err(e),
        }
    }
    // Poda os que ficaram pendurados (alvo ausente), até estabilizar: um link pode apontar para outro.
    // Os de `/etc/alternatives` ficam como no Debian slim, onde os das manpages já são pendurados.
    loop {
        let dangling: Vec<&str> = links
            .iter()
            .copied()
            .filter(|l| !l.starts_with("/etc/alternatives/"))
            .filter(|l| ns.stat(cx, &Start::Cwd, l.as_bytes(), AtFlags::empty()).is_err())
            .collect();
        if dangling.is_empty() {
            break;
        }
        for l in &dangling {
            ns.unlink(cx, &Start::Cwd, l.as_bytes(), AtFlags::empty())?;
        }
        links.retain(|l| !dangling.contains(l));
    }
    stamped.extend(links.iter().map(|l| l.as_bytes().to_vec()));
    // Carimbos por último (criar filhos mexe no mtime dos diretórios).
    for p in stamped.iter().rev() {
        stamp(ns, cx, p)?;
    }
    stamp(ns, cx, b"/")?;
    Ok(())
}

/// Monta o `/dev` (um tmpfs próprio, como no container).
pub(crate) fn build_dev(ns: &Namespace, cx: &Caller) -> Result<(), Errno> {
    let s = &Start::Cwd;
    let chr = sysabi::mode::S_IFCHR;
    for (name, d) in [
        ("null", dev::DEV_NULL),
        ("zero", dev::DEV_ZERO),
        ("full", dev::DEV_FULL),
        ("random", dev::DEV_RANDOM),
        ("urandom", dev::DEV_URANDOM),
        ("tty", dev::DEV_TTY),
    ] {
        let p = format!("/dev/{name}");
        ns.mknod(cx, s, p.as_bytes(), chr | 0o666, makedev(d.0, d.1))?;
        ns.chmod(cx, s, p.as_bytes(), 0o666, AtFlags::empty())?;
    }
    ns.symlink(cx, b"/proc/self/fd", s, b"/dev/fd")?;
    ns.symlink(cx, b"/proc/self/fd/0", s, b"/dev/stdin")?;
    ns.symlink(cx, b"/proc/self/fd/1", s, b"/dev/stdout")?;
    ns.symlink(cx, b"/proc/self/fd/2", s, b"/dev/stderr")?;
    mkdir(ns, cx, b"/dev/shm", 0o1777)?;
    mkdir(ns, cx, b"/dev/pts", 0o755)?;
    // Como no contêiner (devpts com `ptmxmode=666`): `/dev/pts/ptmx` é o multiplexador e `/dev/ptmx`
    // aponta pra ele.
    let (ma, mi) = crate::tty::DEV_PTMX;
    ns.mknod(cx, s, b"/dev/pts/ptmx", chr | 0o666, makedev(ma, mi))?;
    ns.chmod(cx, s, b"/dev/pts/ptmx", 0o666, AtFlags::empty())?;
    ns.symlink(cx, b"pts/ptmx", s, b"/dev/ptmx")?;
    Ok(())
}
