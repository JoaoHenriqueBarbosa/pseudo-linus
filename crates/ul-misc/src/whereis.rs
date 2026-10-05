//! `whereis` do util-linux 2.41 (pacote util-linux do Debian 13): localiza os binários, as páginas de
//! manual e as fontes de um comando nos diretórios padrão (e em `PATH` e `MANPATH`).
//!
//! Porte direto do `misc-utils/whereis.c`, inclusive a leitura dos argumentos à mão (sem getopt):
//! opções e nomes se misturam, `-b`/`-m`/`-s` acumulam até a primeira busca, `-B`/`-M`/`-S` trocam a
//! lista daquele tipo (e a nova lista vai pro fim), `-f` fecha a lista de diretórios, `-u` só mostra
//! os nomes com mais de uma ocorrência, `-g` usa glob e `-l` imprime as listas efetivas. Cada
//! diretório entra uma vez por tipo (mesmo dispositivo e inode), com o caminho canônico.
//!
//! A ordem dos achados dentro de um diretório é a do `readdir` do sistema de arquivos.

use std::ffi::OsStr;
use std::ffi::OsString;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;

use sysabi::{AccessMode, AtFlags, Ctx, Fd, FileType, OFlags, sys};

use crate::util::fnmatch::fnmatch;
use crate::util::io;
use crate::util::ul;

const BIN_DIR: u32 = 1 << 1;
const MAN_DIR: u32 = 1 << 2;
const SRC_DIR: u32 = 1 << 3;
const ALL_DIRS: u32 = BIN_DIR | MAN_DIR | SRC_DIR;

const BINDIRS: &[&str] = &[
    "/usr/bin",
    "/usr/sbin",
    "/bin",
    "/sbin",
    "/lib/x86_64-linux-gnu",
    "/usr/lib/x86_64-linux-gnu",
    "/usr/local/lib/x86_64-linux-gnu",
    "/usr/lib",
    "/usr/lib32",
    "/usr/lib64",
    "/etc",
    "/usr/etc",
    "/lib",
    "/lib32",
    "/lib64",
    "/usr/games",
    "/usr/games/bin",
    "/usr/games/lib",
    "/usr/emacs/etc",
    "/usr/lib/emacs/*/etc",
    "/usr/TeX/bin",
    "/usr/tex/bin",
    "/usr/interviews/bin/LINUX",
    "/usr/X11R6/bin",
    "/usr/X386/bin",
    "/usr/bin/X11",
    "/usr/X11/bin",
    "/usr/X11R5/bin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/usr/local/etc",
    "/usr/local/lib",
    "/usr/local/games",
    "/usr/local/games/bin",
    "/usr/local/emacs/etc",
    "/usr/local/TeX/bin",
    "/usr/local/tex/bin",
    "/usr/local/bin/X11",
    "/usr/contrib",
    "/usr/hosts",
    "/usr/include",
    "/usr/g++-include",
    "/usr/ucb",
    "/usr/old",
    "/usr/new",
    "/usr/local",
    "/usr/libexec",
    "/usr/share",
    "/opt/*/bin",
];

const MANDIRS: &[&str] = &[
    "/usr/man/*",
    "/usr/share/man/*",
    "/usr/X386/man/*",
    "/usr/X11/man/*",
    "/usr/TeX/man/*",
    "/usr/interviews/man/mann",
    "/usr/share/info",
];

const SRCDIRS: &[&str] = &[
    "/usr/src/*",
    "/usr/src/lib/libc/*",
    "/usr/src/lib/libc/net/*",
    "/usr/src/ucb/pascal",
    "/usr/src/ucb/pascal/utilities",
    "/usr/src/undoc",
];

const USAGE: &str = "
Usage:
 whereis [options] [-BMS <dir>... -f] <name>

Locate the binary, source, and manual-page files for a command.

Options:
 -b         search only for binaries
 -B <dirs>  define binaries lookup path
 -m         search only for manuals and infos
 -M <dirs>  define man and info lookup path
 -s         search only for sources
 -S <dirs>  define sources lookup path
 -f         terminate <dirs> argument list
 -u         search for unusual entries
 -g         interpret name as glob (pathnames pattern)
 -l         output effective lookup paths

 -h, --help     display this help
 -V, --version  display version

For more details see whereis(1).
";

/// Um diretório da lista de busca (`struct wh_dirlist`).
struct Dir {
    typ: u32,
    dev: u64,
    ino: u64,
    path: Vec<u8>,
}

struct Whereis {
    dirs: Vec<Dir>,
    uflag: bool,
    use_glob: bool,
    short: String,
}

/// Entradas de um diretório como o `readdir`, com `.` e `..`; `None` se não abre.
fn readdir_all(path: &[u8]) -> Option<Vec<Vec<u8>>> {
    let s = sys::current();
    let fd = s.openat(Fd::CWD, path, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC, 0).ok()?;
    let mut out = Vec::new();
    loop {
        match s.getdents(fd) {
            Ok(b) if b.is_empty() => break,
            Ok(b) => out.extend(b.into_iter().map(|e| e.name)),
            Err(_) => break,
        }
    }
    let _ = s.close(fd);
    Some(out)
}

/// `canonicalize_path`: o `realpath`, ou o caminho como veio quando falha.
fn canonicalize_path(p: &[u8]) -> Vec<u8> {
    match sysio::fs::canonicalize(OsStr::from_bytes(p)) {
        Ok(c) => c.as_os_str().as_bytes().to_vec(),
        Err(_) => p.to_vec(),
    }
}

/// `filename_equal`: o nome `dp` casa com o padrão `cp` pro tipo de diretório (sufixos de manual e
/// de fonte incluídos).
fn filename_equal(cp: &[u8], dp: &[u8], typ: u32, use_glob: bool) -> bool {
    if use_glob {
        return fnmatch(cp, dp);
    }
    if typ & SRC_DIR != 0 && dp.len() >= 2 && dp[0] == b's' && dp[1] == b'.' && filename_equal(cp, &dp[2..], typ, use_glob) {
        return true;
    }

    // `i` é size_t no original: pode dar a volta e o laço abaixo depende disso.
    let mut i: usize = dp.len();
    if typ & MAN_DIR != 0 {
        if i > 1 && dp.ends_with(b".Z") {
            i -= 2;
        } else if i > 2 && dp.ends_with(b".gz") {
            i -= 3;
        } else if i > 2 && dp.ends_with(b".xz") {
            i -= 3;
        } else if i > 3 && dp.ends_with(b".bz2") {
            i -= 4;
        } else if i > 3 && dp.ends_with(b".zst") {
            i -= 4;
        }
    }
    let (mut c, mut d) = (0usize, 0usize);
    while c < cp.len() && d < dp.len() && cp[c] == dp[d] {
        c += 1;
        d += 1;
        i = i.wrapping_sub(1);
    }
    if c == cp.len() && d == dp.len() {
        return true;
    }
    if typ & BIN_DIR == 0 && c == cp.len() {
        let is_dot = dp.get(d) == Some(&b'.');
        d += 1;
        if is_dot {
            i = i.wrapping_sub(1);
            while i > 0 && d < dp.len() {
                i = i.wrapping_sub(1);
                let ch = dp[d];
                d += 1;
                if ch == b'.' {
                    let a = dp.get(d);
                    d += 1;
                    if a != Some(&b'C') {
                        return false;
                    }
                    return dp.get(d).is_none();
                }
            }
            return true;
        }
    }
    false
}

impl Whereis {
    /// `dirlist_add_dir`.
    fn add_dir(&mut self, typ: u32, dir: &[u8]) {
        let s = sys::current();
        if s.faccessat(Fd::CWD, dir, AccessMode::R_OK, AtFlags::empty()).is_err() {
            return;
        }
        let Ok(st) = sys::stat(dir) else { return };
        if st.file_type() != FileType::Directory {
            return;
        }
        if self.dirs.iter().any(|d| d.ino == st.ino && d.dev == st.dev && d.typ == typ) {
            return;
        }
        self.dirs.push(Dir { typ, dev: st.dev, ino: st.ino, path: canonicalize_path(dir) });
    }

    /// `dirlist_add_subdir`: o primeiro `*` do caminho vira cada subdiretório do pai.
    fn add_subdir(&mut self, typ: u32, dir: &str) {
        let Some(star) = dir.find('*') else { return };
        let prefix = &dir[..star];
        let postfix = &dir[star + 1..];
        let Some(entries) = readdir_all(prefix.as_bytes()) else { return };
        for name in entries {
            if name == b"." || name == b".." {
                continue;
            }
            let mut p = prefix.as_bytes().to_vec();
            p.extend_from_slice(&name);
            p.extend_from_slice(postfix.as_bytes());
            self.add_dir(typ, &p);
        }
    }

    fn construct_dirlist(&mut self, typ: u32, paths: &[&str]) {
        for p in paths {
            if p.contains('*') {
                self.add_subdir(typ, p);
            } else {
                self.add_dir(typ, p.as_bytes());
            }
        }
    }

    /// `construct_dirlist_from_env`: elementos de `$name` separados por `:` (vazios ignorados, como o
    /// `strtok_r`).
    fn construct_from_env(&mut self, name: &str, typ: u32) {
        let Some(v) = sys::getenv(name) else { return };
        for tok in v.split(|&b| b == b':').filter(|t| !t.is_empty()) {
            self.add_dir(typ, tok);
        }
    }

    fn free_dirlist(&mut self, typ: u32) {
        self.dirs.retain(|d| d.typ & typ == 0);
    }

    /// `findin`: imprime os nomes de `dir` que casam com `pattern`.
    fn findin(&self, out: &mut impl Write, dir: &[u8], pattern: &[u8], count: &mut u32, wait: &mut Option<Vec<u8>>, typ: u32) {
        let Some(entries) = readdir_all(dir) else { return };
        for name in entries {
            if !filename_equal(pattern, &name, typ, self.use_glob) {
                continue;
            }
            let mut full = dir.to_vec();
            full.push(b'/');
            full.extend_from_slice(&name);
            if self.uflag && *count == 0 {
                *wait = Some(full);
            } else if self.uflag && *count == 1 && wait.is_some() {
                let mut line = pattern.to_vec();
                line.extend_from_slice(b": ");
                line.extend_from_slice(wait.as_deref().unwrap_or(b""));
                line.push(b' ');
                line.extend_from_slice(&full);
                let _ = out.write_all(&line);
                *wait = None;
            } else {
                let _ = out.write_all(b" ");
                let _ = out.write_all(&full);
            }
            *count += 1;
        }
    }

    /// `lookup`.
    fn lookup(&self, out: &mut impl Write, pattern: &[u8], want: u32) {
        // canonicalize pattern: tira o diretório
        let patbuf: Vec<u8> = match pattern.iter().rposition(|&b| b == b'/') {
            Some(p) => pattern[p + 1..].to_vec(),
            None => pattern.to_vec(),
        };
        if !self.uflag {
            // if -u not specified then we always print the pattern
            let _ = out.write_all(&patbuf);
            let _ = out.write_all(b":");
        }
        let mut count = 0u32;
        let mut wait: Option<Vec<u8>> = None;
        for d in &self.dirs {
            if d.typ & want != 0 && !d.path.is_empty() {
                self.findin(out, &d.path, &patbuf, &mut count, &mut wait, d.typ);
            }
        }
        if !self.uflag || count > 1 {
            let _ = out.write_all(b"\n");
        }
    }

    fn list_dirlist(&self, out: &mut impl Write) {
        for d in &self.dirs {
            if d.path.is_empty() {
                continue;
            }
            let label: &[u8] = match d.typ {
                BIN_DIR => b"bin: ",
                MAN_DIR => b"man: ",
                _ => b"src: ",
            };
            let _ = out.write_all(label);
            let _ = out.write_all(&d.path);
            let _ = out.write_all(b"\n");
        }
    }

    /// `construct_dirlist_from_argv`: os diretórios a partir de `*idx`, até o primeiro argumento que
    /// começa com `-`; `idx` termina no último usado.
    fn construct_from_argv(&mut self, idx: &mut usize, argv: &[Vec<u8>], typ: u32) {
        let mut j = *idx;
        while j < argv.len() {
            if argv[j].first() == Some(&b'-') {
                break;
            }
            self.add_dir(typ, &argv[j]);
            *idx = j;
            j += 1;
        }
    }

    fn bad_usage(&self) -> i32 {
        ul::warnx(&self.short, "bad usage");
        ul::errtryhelp(&self.short);
        1
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);
    let argc = argv.len();

    if argc <= 1 {
        ul::warnx(&short, "not enough arguments");
        ul::errtryhelp(&short);
        return 1;
    }
    if argv[1] == b"--help" {
        let mut out = io::stdout();
        let _ = out.write_all(USAGE.as_bytes());
        return 0;
    }
    if argv[1] == b"--version" {
        ul::print_version(&short);
        return 0;
    }

    let mut w = Whereis { dirs: Vec::new(), uflag: false, use_glob: false, short: short.clone() };
    w.construct_dirlist(BIN_DIR, BINDIRS);
    w.construct_from_env("PATH", BIN_DIR);
    w.construct_dirlist(MAN_DIR, MANDIRS);
    w.construct_from_env("MANPATH", MAN_DIR);
    w.construct_dirlist(SRC_DIR, SRCDIRS);

    let mut out = io::stdout();
    let mut want = ALL_DIRS;
    let mut want_resetable = false;
    let mut opt_f_missing = false;

    let mut i = 1usize;
    while i < argc {
        let arg = argv[i].clone();
        let arg_i = i;

        if arg.first() != Some(&b'-') {
            w.lookup(&mut out, &arg, want);
            // O "want" é cumulativo e só reinicia depois de usado.
            want_resetable = true;
            i += 1;
            continue;
        }

        let mut k = 1;
        while k < arg.len() {
            match arg[k] {
                b'f' => opt_f_missing = false,
                b'u' => {
                    w.uflag = true;
                    opt_f_missing = false;
                }
                c @ (b'B' | b'M' | b'S') => {
                    if k + 1 < arg.len() {
                        return w.bad_usage();
                    }
                    let typ = match c {
                        b'B' => BIN_DIR,
                        b'M' => MAN_DIR,
                        _ => SRC_DIR,
                    };
                    i += 1;
                    w.free_dirlist(typ);
                    w.construct_from_argv(&mut i, &argv, typ);
                    opt_f_missing = true;
                }
                c @ (b'b' | b'm' | b's') => {
                    if want_resetable {
                        want = ALL_DIRS;
                        want_resetable = false;
                    }
                    let bit = match c {
                        b'b' => BIN_DIR,
                        b'm' => MAN_DIR,
                        _ => SRC_DIR,
                    };
                    want = if want == ALL_DIRS { bit } else { want | bit };
                    opt_f_missing = false;
                }
                b'l' => w.list_dirlist(&mut out),
                b'g' => w.use_glob = true,
                b'V' => {
                    ul::print_version(&short);
                    return 0;
                }
                b'h' => {
                    let _ = out.write_all(USAGE.as_bytes());
                    return 0;
                }
                _ => return w.bad_usage(),
            }
            if arg_i < i {
                // andou pro próximo argv[]
                break;
            }
            k += 1;
        }
        i += 1;
        sys::checkpoint();
    }

    w.free_dirlist(ALL_DIRS);
    if opt_f_missing {
        ul::warnx(&short, "option -f is missing");
        return 1;
    }
    if out.flush().is_err() {
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_names_like_whereis() {
        let eq = |p: &str, d: &str, t: u32| filename_equal(p.as_bytes(), d.as_bytes(), t, false);
        assert!(eq("ls", "ls", BIN_DIR));
        assert!(!eq("ls", "ls.1", BIN_DIR));
        assert!(eq("ls", "ls.1.gz", MAN_DIR));
        assert!(eq("ls", "ls.1", MAN_DIR));
        assert!(!eq("ls", "lsx.1", MAN_DIR));
        assert!(eq("ls", "ls.c", SRC_DIR));
        assert!(eq("ls", "s.ls", SRC_DIR));
        assert!(!eq("ls", "ls.tar.gz", SRC_DIR));
    }
}
