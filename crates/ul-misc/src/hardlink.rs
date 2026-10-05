//! `hardlink` do util-linux 2.41 (pacote util-linux do Debian 13): troca arquivos duplicados por
//! links físicos.
//!
//! Porte do `misc-utils/hardlink.c`: os arquivos regulares abaixo de cada operando (caminho canônico,
//! `nftw` sem seguir links) entram numa tabela por inode e noutra por (dispositivo, tamanho); dentro de
//! cada grupo de mesmo tamanho o "mestre" é o maior segundo `file_compare` (nº de links com `-m`/`-M`,
//! árvore com `-F`, data com `-O`, depois o menor inode) e os outros iguais em atributos e conteúdo
//! viram links dele, trocados de forma atômica por `<nome>.hardlink-temporary` + `rename`. No fim sai o
//! resumo (modo, método, arquivos, ligados, comparações, economia e duração).
//!
//! Diferenças deliberadas: o método de comparação (`memcmp`, `sha1`, `sha256`) só decide o nome que o
//! resumo mostra, o conteúdo é sempre comparado byte a byte; os grupos de tamanho são visitados em
//! ordem crescente de (dispositivo, tamanho), que é a ordem do `twalk` numa árvore ordenada, mas o
//! original visita em pós-ordem da árvore balanceada do `tsearch`, então com vários tamanhos a ordem
//! das mensagens de `-v` pode diferir; `--reflink` é aceito e cai no link físico, e os atributos
//! estendidos contam como iguais (o sandbox não tem xattr).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;

use regex_posix::{Regex, Syntax};
use sysabi::{AtFlags, Clock, Ctx, Errno, Fd, FileType, RenameFlags, Stat, sys};

use crate::util::io::{self, File};
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const OPT_REFLINK: i32 = 1000;
const OPT_SKIP_RELINKS: i32 = 1001;
const OPT_EXCLUDE_SUBTREE: i32 = 1002;
const OPT_MOUNT: i32 = 1003;

const LONGS: &[LongOpt] = &[
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("dry-run", HasArg::No, b'n' as i32),
    LongOpt::new("respect-name", HasArg::No, b'f' as i32),
    LongOpt::new("respect-dir", HasArg::No, b'd' as i32),
    LongOpt::new("ignore-mode", HasArg::No, b'p' as i32),
    LongOpt::new("ignore-owner", HasArg::No, b'o' as i32),
    LongOpt::new("ignore-time", HasArg::No, b't' as i32),
    LongOpt::new("respect-xattrs", HasArg::No, b'X' as i32),
    LongOpt::new("maximize", HasArg::No, b'm' as i32),
    LongOpt::new("minimize", HasArg::No, b'M' as i32),
    LongOpt::new("prioritize-trees", HasArg::No, b'F' as i32),
    LongOpt::new("keep-oldest", HasArg::No, b'O' as i32),
    LongOpt::new("exclude", HasArg::Required, b'x' as i32),
    LongOpt::new("include", HasArg::Required, b'i' as i32),
    LongOpt::new("exclude-subtree", HasArg::Required, OPT_EXCLUDE_SUBTREE),
    LongOpt::new("mount", HasArg::No, OPT_MOUNT),
    LongOpt::new("method", HasArg::Required, b'y' as i32),
    LongOpt::new("minimum-size", HasArg::Required, b's' as i32),
    LongOpt::new("maximum-size", HasArg::Required, b'S' as i32),
    LongOpt::new("reflink", HasArg::Optional, OPT_REFLINK),
    LongOpt::new("skip-reflinks", HasArg::No, OPT_SKIP_RELINKS),
    LongOpt::new("io-size", HasArg::Required, b'b' as i32),
    LongOpt::new("content", HasArg::No, b'c' as i32),
    LongOpt::new("quiet", HasArg::No, b'q' as i32),
    LongOpt::new("cache-size", HasArg::Required, b'r' as i32),
    LongOpt::new("list-duplicates", HasArg::No, b'l' as i32),
    LongOpt::new("zero", HasArg::No, b'z' as i32),
];

const OPTSTR: &str = "VhvndfpotXcmMFOlzx:y:i:r:S:s:b:q";

const USAGE: &str = "
Usage:
 hardlink [options] <directory>|<file> ...

Consolidate duplicate files using hardlinks.

Options:
 -c, --content              compare only file contents, same as -pot
 -b, --io-size <size>       I/O buffer size for file reading
                              (speedup, using more RAM)
 -d, --respect-dir          directory names have to be identical
 -f, --respect-name         filenames have to be identical
 -i, --include <regex>      regular expression to include files/dirs
 -m, --maximize             maximize the hardlink count, remove the file with
                              lowest hardlink count
 -M, --minimize             reverse the meaning of -m
 -n, --dry-run              don't actually link anything
 -l, --list-duplicates      print every group of duplicate files
 -z, --zero                 delimit output with NULs instead of newlines
 -o, --ignore-owner         ignore owner changes
 -F, --prioritize-trees     files found in the earliest specified top-level
                              directory have higher priority (lower precedence
                              than minimize/maximize)
 -O, --keep-oldest          keep the oldest file of multiple equal files
                              (lower precedence than minimize/maximize)
 -p, --ignore-mode          ignore changes of file mode
 -q, --quiet                quiet mode - don't print anything
 -r, --cache-size <size>    memory limit for cached file content data
 -s, --minimum-size <size>  minimum size for files.
 -S, --maximum-size <size>  maximum size for files.
 -t, --ignore-time          ignore timestamps (when testing for equality)
 -v, --verbose              verbose output (repeat for more verbosity)
 -x, --exclude <regex>      regular expression to exclude files
     --exclude-subtree <regex>  regular expression to exclude directories
     --mount                stay within the same filesystem
 -X, --respect-xattrs       respect extended attributes
 -y, --method <name>        file content comparison method
     --reflink[=<when>]     create clone/CoW copies (auto, always, never)
     --skip-reflinks        skip already cloned files (enabled on --reflink)

 -h, --help                 display this help
 -V, --version              display version

For more details see hardlink(1).
";

/// Níveis de log (`enum log_level`).
const JLOG_SUMMARY: i32 = 0;
const JLOG_INFO: i32 = 1;
const JLOG_VERBOSE1: i32 = 2;
const JLOG_VERBOSE2: i32 = 3;

#[derive(Clone)]
struct Opts {
    include: Vec<Regex>,
    exclude: Vec<Regex>,
    exclude_subtree: Vec<Regex>,
    method: String,
    verbosity: i32,
    respect_mode: bool,
    respect_owner: bool,
    respect_name: bool,
    respect_dir: bool,
    respect_time: bool,
    respect_xattrs: bool,
    maximise: bool,
    minimise: bool,
    keep_oldest: bool,
    prio_trees: bool,
    dry_run: bool,
    list_duplicates: bool,
    within_mount: bool,
    line_delim: u8,
    min_size: u64,
    max_size: u64,
    quiet: bool,
    reflinks_skip: bool,
}

#[derive(Default)]
struct Stats {
    files: usize,
    linked: usize,
    xattr_comparisons: usize,
    comparisons: usize,
    ignored_reflinks: usize,
    saved: u64,
}

/// Um nome de arquivo dentro de um inode (`struct link`).
struct Link {
    path: Vec<u8>,
    basename: usize,
    dirname: usize,
}

/// Um inode visto na varredura (`struct file`).
struct FileEnt {
    dev: u64,
    ino: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    size: u64,
    mtime: i64,
    nlink: u64,
    /// O primeiro elemento é a cabeça da lista do original.
    links: Vec<Link>,
    tree: u16,
}

/// Chave da tabela por inode (`compare_nodes_ino`): dispositivo, inode e, conforme as opções, o
/// nome-base e a parte do diretório.
type InoKey = (u64, u64, Vec<u8>, (usize, Vec<u8>));

struct Hl {
    o: Opts,
    stats: Stats,
    files: Vec<FileEnt>,
    by_ino: BTreeMap<InoKey, usize>,
    by_size: BTreeMap<(u64, u64), Vec<usize>>,
    curr_tree: u16,
    rootbasesz: usize,
    short: String,
}

fn cmp<T: Ord>(a: T, b: T) -> i32 {
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
    }
}

/// `size_to_human_string` com `SIZE_SUFFIX_3LETTER | SIZE_SUFFIX_SPACE | SIZE_DECIMAL_2DIGITS`.
fn human_size(bytes: u64) -> String {
    let mut shft = 10;
    while shft <= 60 {
        if bytes < (1u64 << shft) {
            break;
        }
        shft += 10;
    }
    let exp = shft - 10;
    let letters = b"BKMGTPE";
    let c = letters[if exp != 0 { (exp / 10) as usize } else { 0 }] as char;
    let mut dec = if exp != 0 { bytes / (1u64 << exp) } else { bytes };
    let mut frac = if exp != 0 { bytes % (1u64 << exp) } else { 0 };
    let suffix = if c == 'B' { " B".to_string() } else { format!(" {c}iB") };
    if frac != 0 {
        // três dígitos depois do ponto
        if frac >= u64::MAX / 1000 {
            frac = ((frac / 1024) * 1000) / (1u64 << (exp - 10));
        } else {
            frac = (frac * 1000) / (1u64 << exp);
        }
        // arredonda e guarda dois dígitos
        frac = (frac + 5) / 10;
        if frac == 100 {
            dec += 1;
            frac = 0;
        }
    }
    if frac != 0 {
        let mut s = format!("{dec}.{frac:02}");
        if s.ends_with('0') {
            s.pop();
        }
        s + &suffix
    } else {
        format!("{dec}{suffix}")
    }
}

impl Hl {
    fn enabled(&self, level: i32) -> bool {
        !self.o.quiet && level <= self.o.verbosity
    }

    fn jlog(&self, level: i32, msg: impl AsRef<[u8]>) {
        if !self.enabled(level) {
            return;
        }
        let mut out = io::stdout();
        let _ = out.write_all(msg.as_ref());
        let _ = out.write_all(b"\n");
    }

    fn print_stats(&self, start: &sysabi::TimeSpec) {
        let end = sys::try_current().and_then(|s| s.clock_gettime(Clock::Monotonic).ok()).unwrap_or_default();
        let mut sec = end.sec - start.sec;
        let mut usec = (i64::from(end.nsec) - i64::from(start.nsec)) / 1000;
        if usec < 0 {
            sec -= 1;
            usec += 1_000_000;
        }
        self.jlog(JLOG_SUMMARY, format!("{:<25} {}", "Mode:", if self.o.dry_run { "dry-run" } else { "real" }));
        self.jlog(JLOG_SUMMARY, format!("{:<25} {}", "Method:", self.o.method));
        self.jlog(JLOG_SUMMARY, format!("{:<25} {}", "Files:", self.stats.files));
        self.jlog(JLOG_SUMMARY, format!("{:<25} {} files", "Linked:", self.stats.linked));
        self.jlog(JLOG_SUMMARY, format!("{:<25} {} xattrs", "Compared:", self.stats.xattr_comparisons));
        self.jlog(JLOG_SUMMARY, format!("{:<25} {} files", "Compared:", self.stats.comparisons));
        if self.o.reflinks_skip {
            self.jlog(JLOG_SUMMARY, format!("{:<25} {} files", "Skipped reflinks:", self.stats.ignored_reflinks));
        }
        self.jlog(JLOG_SUMMARY, format!("{:<25} {}", "Saved:", human_size(self.stats.saved)));
        self.jlog(JLOG_SUMMARY, format!("{:<25} {}.{:06} seconds", "Duration:", sec, usec));
    }

    fn match_any(list: &[Regex], what: &[u8]) -> bool {
        list.iter().any(|r| r.is_match(what))
    }

    fn filename(f: &FileEnt) -> &[u8] {
        let l = &f.links[0];
        &l.path[l.basename..]
    }

    fn dirname(f: &FileEnt) -> (usize, &[u8]) {
        let l = &f.links[0];
        let sz = l.basename.saturating_sub(l.dirname);
        (sz, &l.path[l.dirname.min(l.path.len())..(l.dirname + sz).min(l.path.len())])
    }

    /// `file_compare`: positivo quando `a` deve ser o mestre em vez de `b`.
    fn file_compare(&self, a: usize, b: usize) -> i32 {
        let (fa, fb) = (&self.files[a], &self.files[b]);
        if fa.dev == fb.dev && fa.ino == fb.ino {
            return 0;
        }
        let mut res = 0;
        if res == 0 && self.o.maximise {
            res = cmp(fa.nlink, fb.nlink);
        }
        if res == 0 && self.o.minimise {
            res = cmp(fb.nlink, fa.nlink);
        }
        if res == 0 && self.o.prio_trees {
            res = cmp(fa.tree, fb.tree);
        }
        if res == 0 {
            res = if self.o.keep_oldest { cmp(fb.mtime, fa.mtime) } else { cmp(fa.mtime, fb.mtime) };
        }
        if res == 0 {
            res = cmp(fb.ino, fa.ino);
        }
        res
    }

    /// `file_xattrs_equal`: o sandbox não tem xattr, então são sempre iguais.
    fn xattrs_equal(&mut self, a: usize, b: usize) -> bool {
        let pa = io::lossy(&self.files[a].links[0].path);
        let pb = io::lossy(&self.files[b].links[0].path);
        self.jlog(JLOG_VERBOSE1, format!("Comparing xattrs of {pa} to {pb}"));
        self.stats.xattr_comparisons += 1;
        true
    }

    /// `file_may_link_to`.
    fn may_link_to(&mut self, a: usize, b: usize) -> bool {
        let (fa, fb) = (&self.files[a], &self.files[b]);
        let basic = fa.size == fb.size
            && !fa.links.is_empty()
            && !fb.links.is_empty()
            && fa.dev == fb.dev
            && fa.ino != fb.ino
            && (!self.o.respect_mode || fa.mode == fb.mode)
            && (!self.o.respect_owner || (fa.uid == fb.uid && fa.gid == fb.gid))
            && (!self.o.respect_time || fa.mtime == fb.mtime)
            && (!self.o.respect_name || Self::filename(fa) == Self::filename(fb))
            && (!self.o.respect_dir || Self::dirname(fa) == Self::dirname(fb));
        if !basic {
            return false;
        }
        !self.o.respect_xattrs || self.xattrs_equal(a, b)
    }

    /// `file_link`: troca todos os nomes de `b` por links de `a`. `Err` com o errno da falha.
    fn file_link(&mut self, a: usize, b: usize) -> Result<(), Option<Errno>> {
        loop {
            let a_path = self.files[a].links[0].path.clone();
            let b_path = self.files[b].links[0].path.clone();
            if self.enabled(JLOG_INFO) {
                let ssz = human_size(self.files[a].size);
                let dry = if self.o.dry_run { "[DryRun] " } else { "" };
                self.jlog(JLOG_INFO, format!("{dry}Linking {} to {} (-{ssz})", io::lossy(&a_path), io::lossy(&b_path)));
            }

            if !self.o.dry_run {
                let mut new_path = b_path.clone();
                new_path.extend_from_slice(b".hardlink-temporary");
                let s = sys::current();
                if let Err(e) = s.linkat(Fd::CWD, &a_path, Fd::CWD, &new_path, AtFlags::empty()) {
                    ul::warn(&self.short, format!("cannot link {} to {}", io::lossy(&a_path), io::lossy(&new_path)), e);
                    return Err(Some(e));
                }
                if let Err(e) = s.renameat2(Fd::CWD, &new_path, Fd::CWD, &b_path, RenameFlags::empty()) {
                    ul::warn(&self.short, format!("cannot rename {} to {}", io::lossy(&a_path), io::lossy(&new_path)), e);
                    let _ = s.unlinkat(Fd::CWD, &new_path, AtFlags::empty());
                    return Err(Some(e));
                }
            }

            // Update statistics
            self.stats.linked += 1;

            // Increase the link count of this file, and set stat() of other file
            self.files[a].nlink += 1;
            self.files[b].nlink = self.files[b].nlink.wrapping_sub(1);
            if self.files[b].nlink == 0 {
                self.stats.saved += self.files[a].size;
            }

            // Move the link from file b to a
            let link = self.files[b].links.remove(0);
            let at = 1.min(self.files[a].links.len());
            self.files[a].links.insert(at, link);

            // Do it again
            if self.files[b].links.is_empty() {
                return Ok(());
            }
        }
    }

    /// `inserter`: um arquivo achado pela varredura.
    fn insert_file(&mut self, fpath: &[u8], st: &Stat, base: usize) {
        if st.file_type() != FileType::Regular {
            return;
        }
        let included = Self::match_any(&self.o.include, fpath);
        let excluded = Self::match_any(&self.o.exclude, fpath);
        if (!self.o.exclude.is_empty() && excluded && !included) || (self.o.exclude.is_empty() && !self.o.include.is_empty() && !included) {
            self.jlog(JLOG_VERBOSE1, format!("Skipped (excluded) {}", io::lossy(fpath)));
            return;
        }

        self.stats.files += 1;

        if st.size < self.o.min_size {
            self.jlog(JLOG_VERBOSE1, format!("Skipped (smaller than configured size) {}", io::lossy(fpath)));
            return;
        }

        self.jlog(
            JLOG_VERBOSE2,
            format!(" {:>5}: [{}/{}/{}] {}", self.stats.files, st.dev, st.ino, st.nlink, io::lossy(fpath)),
        );

        if self.o.max_size > 0 && st.size > self.o.max_size {
            self.jlog(JLOG_VERBOSE1, format!("Skipped (greater than configured size) {}", io::lossy(fpath)));
            return;
        }

        let link = Link { path: fpath.to_vec(), basename: base, dirname: self.rootbasesz };
        let ent = FileEnt {
            dev: st.dev,
            ino: st.ino,
            mode: st.mode,
            uid: st.uid,
            gid: st.gid,
            size: st.size,
            mtime: st.mtime.sec,
            nlink: st.nlink,
            links: vec![link],
            tree: self.curr_tree,
        };
        let key: InoKey = (
            st.dev,
            st.ino,
            if self.o.respect_name { Self::filename(&ent).to_vec() } else { Vec::new() },
            if self.o.respect_dir {
                let (n, d) = Self::dirname(&ent);
                (n, d.to_vec())
            } else {
                (0, Vec::new())
            },
        );

        if let Some(&idx) = self.by_ino.get(&key) {
            // Already known inode, add link to inode information
            if self.files[idx].links.iter().any(|l| l.path == fpath) {
                self.jlog(JLOG_VERBOSE1, format!("Skipped (specified more than once) {}", io::lossy(fpath)));
            } else {
                let l = ent.links.into_iter().next();
                if let Some(l) = l {
                    self.files[idx].links.insert(0, l);
                }
            }
            return;
        }

        // New inode, insert into by-size table
        let idx = self.files.len();
        self.files.push(ent);
        self.by_ino.insert(key, idx);
        let skey = (st.dev, st.size);
        match self.by_size.get(&skey).cloned() {
            None => {
                self.by_size.insert(skey, vec![idx]);
            }
            Some(mut chain) => {
                if self.file_compare(idx, chain[0]) >= 0 {
                    chain.insert(0, idx);
                } else {
                    let mut pos = 0;
                    loop {
                        if pos + 1 < chain.len() && self.file_compare(idx, chain[pos + 1]) < 0 {
                            pos += 1;
                            continue;
                        }
                        chain.insert(pos + 1, idx);
                        break;
                    }
                }
                self.by_size.insert(skey, chain);
            }
        }
    }

    /// `nftw(path, inserter, 20, FTW_PHYS ...)`: visita `path` e, se for diretório, o conteúdo em ordem
    /// de `readdir`.
    fn walk(&mut self, fpath: &[u8], root_dev: u64, top: bool) {
        sys::checkpoint();
        let st = match sys::lstat(fpath) {
            Ok(s) => s,
            Err(e) => {
                // FTW_NS
                ul::warn(&self.short, format!("cannot read {}", io::lossy(fpath)), e);
                return;
            }
        };
        let base = fpath.iter().rposition(|&b| b == b'/').map_or(0, |p| p + 1);
        if st.file_type() == FileType::Directory {
            if self.o.within_mount && !top && st.dev != root_dev {
                return;
            }
            if !self.o.exclude_subtree.is_empty() && Self::match_any(&self.o.exclude_subtree, fpath) {
                self.jlog(JLOG_VERBOSE1, format!("Skipped (excluded subtree) {}", io::lossy(fpath)));
                return;
            }
            let entries = match sys::read_dir(fpath) {
                Ok(e) => e,
                Err(e) => {
                    // FTW_DNR
                    ul::warn(&self.short, format!("cannot read {}", io::lossy(fpath)), e);
                    return;
                }
            };
            for e in entries {
                let mut child = fpath.to_vec();
                if !child.ends_with(b"/") {
                    child.push(b'/');
                }
                child.extend_from_slice(&e.name);
                self.walk(&child, root_dev, false);
            }
            return;
        }
        if st.file_type() == FileType::Symlink {
            return;
        }
        self.insert_file(fpath, &st, base);
    }

    /// `visitor`: processa um grupo (cadeia ordenada do mestre pros candidatos).
    fn visit(&mut self, chain: &[usize]) {
        let mut mi = 0;
        while mi < chain.len() {
            sys::checkpoint();
            let mut master = chain[mi];
            if self.files[master].links.is_empty() {
                mi += 1;
                continue;
            }
            let mut oi = mi + 1;
            while oi < chain.len() {
                let other = chain[oi];
                if self.files[other].links.is_empty() {
                    oi += 1;
                    continue;
                }
                // check file attributes, etc.
                if !self.may_link_to(master, other) {
                    let p = io::lossy(&self.files[other].links[0].path);
                    self.jlog(JLOG_VERBOSE2, format!("Skipped (attributes mismatch) {p}"));
                    oi += 1;
                    continue;
                }

                let eq = self.content_equal(master, other);
                self.stats.comparisons += 1;
                if !eq {
                    let p = io::lossy(&self.files[other].links[0].path);
                    self.jlog(JLOG_VERBOSE2, format!("Skipped (content mismatch) {p}"));
                    oi += 1;
                    continue;
                }

                if let Err(Some(Errno::EMLINK)) = self.file_link(master, other) {
                    master = other;
                    mi = oi;
                }
                oi += 1;
            }
            mi += 1;
        }

        // final cleanup
        if self.o.list_duplicates {
            let mut out = io::stdout();
            for &i in chain {
                if self.files[i].nlink > 1 {
                    for l in &self.files[i].links {
                        // O original imprime o endereço da estrutura: um número estável e único serve.
                        let addr = 0x5555_0000_0000usize + i * 0x80;
                        let _ = out.write_all(format!("{addr:016}\t").as_bytes());
                        let _ = out.write_all(&l.path);
                        let _ = out.write_all(&[self.o.line_delim]);
                    }
                }
            }
        }
    }

    /// `ul_fileeq`: o conteúdo dos dois inodes é igual?
    fn content_equal(&self, a: usize, b: usize) -> bool {
        let read = |p: &[u8]| File::open(p).and_then(|mut f| f.read_to_end_sys());
        match (read(&self.files[a].links[0].path), read(&self.files[b].links[0].path)) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut o = Opts {
        include: Vec::new(),
        exclude: Vec::new(),
        exclude_subtree: Vec::new(),
        method: "sha256".to_string(),
        verbosity: 0,
        respect_mode: true,
        respect_owner: true,
        respect_name: false,
        respect_dir: false,
        respect_time: true,
        respect_xattrs: false,
        maximise: false,
        minimise: false,
        keep_oldest: false,
        prio_trees: false,
        dry_run: false,
        list_duplicates: false,
        within_mount: false,
        line_delim: b'\n',
        min_size: 1,
        max_size: 0,
        quiet: false,
        reflinks_skip: false,
    };
    let mut content_only = false;
    // err_exclusive_options: {q, v}
    let mut excl_qv: Option<char> = None;

    let mut g = Getopt::from_env(&argv[1..], OPTSTR, LONGS);
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(x) => x,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        let arg = opt.arg.clone();
        let c = opt.short();
        if matches!(c, Some('q') | Some('v')) {
            let cc = c.unwrap_or('q');
            match excl_qv {
                None => excl_qv = Some(cc),
                Some(p) if p != cc => {
                    io::eprint(format!("{short}: mutually exclusive arguments: --quiet --verbose\n"));
                    return 1;
                }
                _ => {}
            }
        }
        let size_arg = |what: &str| -> Result<u64, String> { ul::strtosize_or_err(arg.as_deref().unwrap_or(b""), what) };
        let regex_arg = |list: &mut Vec<Regex>| -> Result<(), String> {
            let pat = arg.clone().unwrap_or_default();
            match Regex::new(&pat, Syntax::POSIX_EXTENDED) {
                Ok(re) => {
                    // register_regex insere na frente; a ordem não muda o resultado do "qualquer casa"
                    list.insert(0, re);
                    Ok(())
                }
                Err(e) => Err(format!("could not compile regular expression {}: {}", io::lossy(&pat), e.message())),
            }
        };
        let fail = |m: String| -> i32 {
            ul::warnx(&short, m);
            1
        };
        match opt.id {
            OPT_REFLINK => {
                // 0 never, 1 auto, 2 always
                let mut reflink_mode = 1;
                if let Some(a) = &arg {
                    match a.as_slice() {
                        b"auto" => reflink_mode = 1,
                        b"always" => reflink_mode = 2,
                        b"never" => reflink_mode = 0,
                        _ => return fail(format!("unsupported reflink mode; {}", io::lossy(a))),
                    }
                }
                if reflink_mode != 0 {
                    o.reflinks_skip = true;
                }
            }
            OPT_SKIP_RELINKS => o.reflinks_skip = true,
            OPT_EXCLUDE_SUBTREE => {
                if let Err(m) = regex_arg(&mut o.exclude_subtree) {
                    return fail(m);
                }
            }
            OPT_MOUNT => o.within_mount = true,
            _ => match c {
                Some('p') => o.respect_mode = false,
                Some('o') => o.respect_owner = false,
                Some('t') => o.respect_time = false,
                Some('X') => o.respect_xattrs = true,
                Some('m') => o.maximise = true,
                Some('M') => o.minimise = true,
                Some('O') => o.keep_oldest = true,
                Some('F') => o.prio_trees = true,
                Some('f') => o.respect_name = true,
                Some('d') => o.respect_dir = true,
                Some('v') => o.verbosity += 1,
                Some('q') => o.quiet = true,
                Some('c') => content_only = true,
                Some('n') => o.dry_run = true,
                Some('x') => {
                    if let Err(m) = regex_arg(&mut o.exclude) {
                        return fail(m);
                    }
                }
                Some('y') => o.method = io::lossy(&arg.unwrap_or_default()),
                Some('i') => {
                    if let Err(m) = regex_arg(&mut o.include) {
                        return fail(m);
                    }
                }
                Some('s') => match size_arg("failed to parse minimum size") {
                    Ok(n) => o.min_size = n,
                    Err(m) => return fail(m),
                },
                Some('S') => match size_arg("failed to parse maximum size") {
                    Ok(n) => o.max_size = n,
                    Err(m) => return fail(m),
                },
                Some('r') => {
                    if let Err(m) = size_arg("failed to parse cache size") {
                        return fail(m);
                    }
                }
                Some('b') => {
                    if let Err(m) = size_arg("failed to parse I/O size") {
                        return fail(m);
                    }
                }
                Some('l') => {
                    o.list_duplicates = true;
                    o.dry_run = true;
                    o.quiet = true;
                }
                Some('z') => o.line_delim = 0,
                Some('h') => {
                    let mut out = io::stdout();
                    let _ = out.write_all(USAGE.as_bytes());
                    return 0;
                }
                Some('V') => {
                    let mut out = io::stdout();
                    let _ = out.write_all(format!("{short} from util-linux 2.41.5 (features: reflink, cryptoapi, ftw_skip_subtree)\n").as_bytes());
                    return 0;
                }
                _ => {
                    ul::errtryhelp(&short);
                    return 1;
                }
            },
        }
    }
    if content_only {
        o.respect_mode = false;
        o.respect_name = false;
        o.respect_dir = false;
        o.respect_owner = false;
        o.respect_time = false;
        o.respect_xattrs = false;
    }

    let operands = g.operands();
    if operands.is_empty() {
        ul::warnx(&short, "no directory or file specified");
        return 1;
    }

    let start = sys::try_current().and_then(|s| s.clock_gettime(Clock::Monotonic).ok()).unwrap_or_default();

    let mut hl = Hl {
        o,
        stats: Stats::default(),
        files: Vec::new(),
        by_ino: BTreeMap::new(),
        by_size: BTreeMap::new(),
        curr_tree: 0,
        rootbasesz: 0,
        short: short.clone(),
    };

    // ul_fileeq_init: memcmp, sha1 e sha256 existem; os outros caem em memcmp.
    if !matches!(hl.o.method.as_str(), "memcmp" | "sha1" | "sha256") {
        hl.jlog(JLOG_INFO, format!("cannot initialize {} method, use 'memcmp' fallback", hl.o.method));
        hl.o.method = "memcmp".to_string();
    }

    hl.jlog(JLOG_VERBOSE2, "Scanning [device/inode/links]:");
    for arg in &operands {
        let path = match sysio::fs::canonicalize(std::ffi::OsStr::from_bytes(arg)) {
            Ok(p) => p.as_os_str().as_bytes().to_vec(),
            Err(e) => {
                ul::warn(&short, format!("cannot get realpath: {}", io::lossy(arg)), Errno::from_io(&e));
                continue;
            }
        };
        if hl.o.respect_dir {
            hl.rootbasesz = path.len();
        }
        if hl.o.prio_trees {
            hl.curr_tree += 1;
        }
        let root_dev = sys::lstat(&path).map(|s| s.dev).unwrap_or(0);
        hl.walk(&path, root_dev, true);
        hl.rootbasesz = 0;
    }

    // twalk: um grupo por (dispositivo, tamanho)
    let groups: Vec<Vec<usize>> = hl.by_size.values().cloned().collect();
    for chain in &groups {
        hl.visit(chain);
    }

    // atexit: o resumo
    hl.print_stats(&start);
    if io::flush_stdout().is_err() {
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::human_size;

    #[test]
    fn sizes_like_size_to_human_string() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(12), "12 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1024), "1 KiB");
        assert_eq!(human_size(1536), "1.5 KiB");
        assert_eq!(human_size(1025), "1 KiB");
        assert_eq!(human_size(10 * 1024 * 1024), "10 MiB");
        assert_eq!(human_size(1024 * 1024 + 1024 * 100), "1.1 MiB");
    }
}
