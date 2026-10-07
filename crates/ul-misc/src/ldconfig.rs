//! `ldconfig` da glibc 2.41 (pacote libc-bin do Debian 13), portado de `elf/ldconfig.c`,
//! `elf/cache.c` e `elf/readlib.c`: lê `/etc/ld.so.conf` (com `include`), varre os diretórios de
//! bibliotecas (ELF64 x86_64), cria os links de soname e gera `/etc/ld.so.cache` no formato
//! `glibc-ld.so.cache1.1`.
//!
//! Opções: `-c FORMAT`, `-C CACHE`, `-f CONF`, `-i`, `-l`, `-n`, `-N`, `-p`, `-r ROOT`, `-v`, `-V`,
//! `-X`, `-?`/`--help` e `--usage`.
//!
//! Limitações conhecidas: o cache auxiliar (`/var/cache/ldconfig/aux-cache`) não é gravado, os
//! diretórios `glibc-hwcaps` não são tratados, o formato `old`/`compat` do `-c` grava o mesmo cache
//! novo, e o `-r` é implementado prefixando os caminhos (o original faz `chroot`).

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Write;

use sysabi::{Errno, FileType, Fd, OFlags, RenameFlags, sys};
use ul_common::ctype::cstr_at;
use ul_common::fsutil;

use crate::binutils::elf::{Elf, rd32};
use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

const OPT_USAGE: i32 = 256;

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("format", HasArg::Required, 'c' as i32),
    LongOpt::new("ignore-aux-cache", HasArg::No, 'i' as i32),
    LongOpt::new("print-cache", HasArg::No, 'p' as i32),
    LongOpt::new("verbose", HasArg::No, 'v' as i32),
    LongOpt::new("help", HasArg::No, '?' as i32),
    LongOpt::new("usage", HasArg::No, OPT_USAGE),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

const HELP: &str = "Usage: ldconfig [OPTION...]
Configure Dynamic Linker Run Time Bindings.

  -c, --format=FORMAT        Format to use: new (default), old, or compat
  -C CACHE                   Use CACHE as cache file
  -f CONF                    Use CONF as configuration file
  -i, --ignore-aux-cache     Ignore auxiliary cache file
  -l                         Manually link individual libraries.
  -n                         Only process directories specified on the command
                             line.  Don't build cache.
  -N                         Don't build cache
  -p, --print-cache          Print cache
  -r ROOT                    Change to and use ROOT as root directory
  -v, --verbose              Generate verbose messages
  -X                         Don't update symbolic links
  -?, --help                 Give this help list
      --usage                Give a short usage message
  -V, --version              Print program version

Mandatory or optional arguments to long options are also mandatory or optional
for any corresponding short options.

For bug reporting instructions, please see:
<http://www.debian.org/Bugs/>.
";

const USAGE: &str = r#"Usage: ldconfig [-ilnNpvX?V] [-c FORMAT] [-C CACHE] [-f CONF] [-r ROOT]
            [--format=FORMAT] [--ignore-aux-cache] [--print-cache] [--verbose]
            [--help] [--usage] [--version]
"#;

const VERSION: &str = r#"ldconfig (Debian GLIBC 2.41-12+deb13u4) 2.41
Copyright (C) 2024 Free Software Foundation, Inc.
This is free software; see the source for copying conditions.  There is NO
warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
Written by Andreas Jaeger.
"#;

const GENERATOR: &str = "ldconfig (Debian GLIBC 2.41-12+deb13u4) stable release version 2.41";

const CACHE_MAGIC_NEW: &[u8] = b"glibc-ld.so.cache";
const CACHE_VERSION: &[u8] = b"1.1";
const EXTENSION_MAGIC: u32 = 0xeaa4_2174;

const FLAG_ELF_LIBC6: u32 = 0x0003;
const FLAG_X8664_LIB64: u32 = 0x0300;

const DEFAULT_CACHE: &[u8] = b"/etc/ld.so.cache";
const DEFAULT_CONF: &[u8] = b"/etc/ld.so.conf";
const SYSTEM_DIRS: [&[u8]; 2] = [b"/lib/x86_64-linux-gnu", b"/usr/lib/x86_64-linux-gnu"];

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// Uma entrada do cache: nome da biblioteca (chave), caminho e flags.
#[derive(Clone, Debug)]
struct CacheEntry {
    key: Vec<u8>,
    path: Vec<u8>,
    flags: u32,
    hwcap: u64,
}

/// Um diretório a varrer e de onde ele veio.
struct DirEntryInfo {
    path: Vec<u8>,
    from: Option<(Vec<u8>, u32)>,
    dev: u64,
    ino: u64,
}

/// Um arquivo candidato a biblioteca dentro de um diretório.
struct Candidate {
    name: Vec<u8>,
    soname: Vec<u8>,
    flags: u32,
    is_link: bool,
}

struct Ldconfig {
    prog: String,
    root: Vec<u8>,
    verbose: bool,
    do_link: bool,
    dirs: Vec<DirEntryInfo>,
    cache: Vec<CacheEntry>,
}

/// `_dl_cache_libcmp`: comparação de nomes de biblioteca com trechos numéricos comparados como
/// números.
fn libcmp(a: &[u8], b: &[u8]) -> i32 {
    let (mut i, mut j) = (0usize, 0usize);
    let at = |s: &[u8], k: usize| -> u8 { s.get(k).copied().unwrap_or(0) };
    while at(a, i) != 0 {
        let (c1, c2) = (at(a, i), at(b, j));
        if c1.is_ascii_digit() {
            if c2.is_ascii_digit() {
                let mut v1: i64 = i64::from(c1 - b'0');
                let mut v2: i64 = i64::from(c2 - b'0');
                i += 1;
                j += 1;
                while at(a, i).is_ascii_digit() {
                    v1 = v1.saturating_mul(10).saturating_add(i64::from(at(a, i) - b'0'));
                    i += 1;
                }
                while at(b, j).is_ascii_digit() {
                    v2 = v2.saturating_mul(10).saturating_add(i64::from(at(b, j) - b'0'));
                    j += 1;
                }
                if v1 != v2 {
                    return if v1 > v2 { 1 } else { -1 };
                }
            } else {
                return 1;
            }
        } else if c2.is_ascii_digit() {
            return -1;
        } else if c1 != c2 {
            return i32::from(c1) - i32::from(c2);
        } else {
            i += 1;
            j += 1;
        }
    }
    i32::from(at(a, i)) - i32::from(at(b, j))
}

fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = dir.to_vec();
    if !p.ends_with(b"/") {
        p.push(b'/');
    }
    p.extend_from_slice(name);
    p
}

fn trim_ws(s: &[u8]) -> &[u8] {
    let is_ws = |b: &u8| matches!(*b, b' ' | b'\t' | b'\r' | b'\n');
    let start = s.iter().position(|b| !is_ws(b)).unwrap_or(s.len());
    let end = s.iter().rposition(|b| !is_ws(b)).map_or(start, |p| p + 1);
    &s[start..end]
}

/// Casamento de curinga (`*`, `?`, `[...]`) de um único componente de caminho.
fn wild(pat: &[u8], text: &[u8]) -> bool {
    match pat.first() {
        None => text.is_empty(),
        Some(b'*') => (0..=text.len()).any(|k| wild(&pat[1..], &text[k..])),
        Some(b'?') => !text.is_empty() && wild(&pat[1..], &text[1..]),
        Some(b'[') => {
            let Some(close) = pat[1..].iter().position(|b| *b == b']').map(|p| p + 1) else {
                return text.first() == Some(&b'[') && wild(&pat[1..], &text[1..]);
            };
            let Some(&c) = text.first() else {
                return false;
            };
            let set = &pat[1..close];
            let (neg, set) = match set.first() {
                Some(b'!') | Some(b'^') => (true, &set[1..]),
                _ => (false, set),
            };
            let mut hit = false;
            let mut k = 0;
            while k < set.len() {
                if k + 2 < set.len() && set[k + 1] == b'-' {
                    if set[k] <= c && c <= set[k + 2] {
                        hit = true;
                    }
                    k += 3;
                } else {
                    if set[k] == c {
                        hit = true;
                    }
                    k += 1;
                }
            }
            hit != neg && wild(&pat[close + 1..], &text[1..])
        }
        Some(&p) => text.first() == Some(&p) && wild(&pat[1..], &text[1..]),
    }
}

fn has_wild(s: &[u8]) -> bool {
    s.iter().any(|b| matches!(*b, b'*' | b'?' | b'['))
}

fn dirname(p: &[u8]) -> Vec<u8> {
    match fsutil::dirname(p) {
        b"" => b".".to_vec(),
        d => d.to_vec(),
    }
}

impl Ldconfig {
    /// Caminho no sistema de arquivos, considerando o `-r`.
    fn rp(&self, path: &[u8]) -> Vec<u8> {
        if self.root.is_empty() {
            path.to_vec()
        } else {
            let mut p = self.root.clone();
            if !path.starts_with(b"/") {
                p.push(b'/');
            }
            p.extend_from_slice(path);
            p
        }
    }

    /// `error (0, errno, ...)`: mensagem com o nome do programa e, se houver, o texto do errno.
    fn warn(&self, msg: &str, errno: Option<Errno>) {
        let _ = io::flush_stdout();
        match errno {
            Some(e) => io::eprint(format!("{}: {}: {}\n", self.prog, msg, e.message())),
            None => io::eprint(format!("{}: {}\n", self.prog, msg)),
        }
    }

    fn add_dir(&mut self, line: &[u8], from: Option<(Vec<u8>, u32)>) {
        let mut path = line.to_vec();
        while path.len() > 1 && path.ends_with(b"/") {
            path.pop();
        }
        if path.is_empty() {
            return;
        }
        let st = match sys::stat(&self.rp(&path)) {
            Ok(st) => st,
            Err(e) => {
                if self.verbose {
                    self.warn(&format!("Can't stat {}", io::lossy(&path)), Some(e));
                }
                return;
            }
        };
        if let Some(prev) = self
            .dirs
            .iter()
            .find(|d| d.dev == st.dev && d.ino == st.ino)
        {
            if self.verbose {
                self.warn(
                    &format!("Path `{}' given more than once", io::lossy(&path)),
                    None,
                );
                let show = |f: &Option<(Vec<u8>, u32)>| match f {
                    None => "system dirs".to_string(),
                    Some((file, 0)) => io::lossy(file),
                    Some((file, l)) => format!("{}:{}", io::lossy(file), l),
                };
                io::eprint(format!("(from {} and {})\n", show(&prev.from), show(&from)));
            }
            return;
        }
        self.dirs.push(DirEntryInfo {
            path,
            from,
            dev: st.dev,
            ino: st.ino,
        });
    }

    /// `parse_conf`: um arquivo de configuração com `include` e `hwcap`.
    fn parse_conf(&mut self, file: &[u8], depth: u32) {
        let data = match io::read_path(&self.rp(file)) {
            Ok(d) => d,
            Err(Errno::ENOENT) => return,
            Err(e) => {
                self.warn(
                    &format!(
                        "Warning: ignoring configuration file that cannot be opened: {}",
                        io::lossy(file)
                    ),
                    Some(e),
                );
                return;
            }
        };
        for (idx, raw) in data.split(|b| *b == b'\n').enumerate() {
            let lineno = idx as u32 + 1;
            let line = match raw.iter().position(|b| *b == b'#') {
                Some(p) => &raw[..p],
                None => raw,
            };
            let line = trim_ws(line);
            if line.is_empty() {
                continue;
            }
            if line.len() > 7
                && line.starts_with(b"include")
                && matches!(line[7], b' ' | b'\t')
            {
                if depth >= 16 {
                    continue;
                }
                for pat in line[7..]
                    .split(|b| matches!(*b, b' ' | b'\t'))
                    .filter(|p| !p.is_empty())
                {
                    self.parse_include(file, pat, depth + 1);
                }
            } else if line.starts_with(b"hwcap")
                && line.get(5).is_none_or(|b| matches!(*b, b' ' | b'\t'))
            {
                continue;
            } else {
                self.add_dir(line, Some((file.to_vec(), lineno)));
            }
        }
    }

    /// `include PADRÃO`: relativo ao diretório do arquivo que inclui; curingas só no último
    /// componente, resultado em ordem de bytes.
    fn parse_include(&mut self, from_file: &[u8], pattern: &[u8], depth: u32) {
        let full = if pattern.starts_with(b"/") {
            pattern.to_vec()
        } else {
            join(&dirname(from_file), pattern)
        };
        let (dir, base) = match full.iter().rposition(|b| *b == b'/') {
            Some(0) => (b"/".to_vec(), full[1..].to_vec()),
            Some(i) => (full[..i].to_vec(), full[i + 1..].to_vec()),
            None => (b".".to_vec(), full.clone()),
        };
        if !has_wild(&base) {
            if sys::stat(&self.rp(&full)).is_ok() {
                self.parse_conf(&full, depth);
            }
            return;
        }
        let Ok(entries) = sys::read_dir(&self.rp(&dir)) else {
            return;
        };
        let mut names: Vec<Vec<u8>> = entries
            .into_iter()
            .map(|e| e.name)
            .filter(|n| (!n.starts_with(b".") || base.starts_with(b".")) && wild(&base, n))
            .collect();
        names.sort();
        for n in names {
            self.parse_conf(&join(&dir, &n), depth);
        }
    }

    /// `process_file`: `Ok(None)` quando não é biblioteca, `Ok(Some((soname, flags)))` quando é.
    fn process_file(&self, shown: &[u8], data: &[u8]) -> Option<(Option<Vec<u8>>, u32)> {
        if data.len() < 4 || &data[..4] != b"\x7fELF" {
            self.warn(
                &format!(
                    "{} is not an ELF file - it has the wrong magic bytes at the start.\n",
                    io::lossy(shown)
                ),
                None,
            );
            return None;
        }
        let elf = Elf::parse(data)?;
        let e_type = u16::from_le_bytes([*data.get(16)?, *data.get(17)?]);
        let e_machine = u16::from_le_bytes([*data.get(18)?, *data.get(19)?]);
        if e_type != 3 || e_machine != 62 {
            return None;
        }
        let mut soname = None;
        if let Some(di) = elf.sections.iter().position(|s| s.kind == 6) {
            let strtab = elf.section_data(elf.sections[di].link as usize);
            for ent in elf.section_data(di).chunks_exact(16) {
                let tag = i64::from_le_bytes(ent[..8].try_into().ok()?);
                let val = u64::from_le_bytes(ent[8..].try_into().ok()?) as usize;
                if tag == 14 {
                    let tail = strtab.get(val..).unwrap_or(&[]);
                    let end = tail.iter().position(|b| *b == 0).unwrap_or(tail.len());
                    soname = Some(tail[..end].to_vec());
                    break;
                }
                if tag == 0 {
                    break;
                }
            }
        }
        Some((soname, FLAG_ELF_LIBC6 | FLAG_X8664_LIB64))
    }

    /// `create_links`: garante `dir/soname -> libname`. Devolve se o link mudou.
    fn create_links(&self, dir: &[u8], libname: &[u8], soname: &[u8]) -> bool {
        let mut changed = false;
        if libname != soname {
            let full_soname = join(dir, soname);
            let real = self.rp(&full_soname);
            let mut create = true;
            if let Ok(st) = sys::lstat(&real) {
                if st.file_type() == FileType::Symlink {
                    let cur = sys::current().readlinkat(Fd::CWD, &real).unwrap_or_default();
                    if cur == libname {
                        create = false;
                    }
                } else {
                    self.warn(
                        &format!("{} is not a symbolic link\n", io::lossy(&full_soname)),
                        None,
                    );
                    create = false;
                }
            }
            if create && self.do_link {
                let mut tmp = real.clone();
                tmp.extend_from_slice(format!(".{}", sys::current().getpid()).as_bytes());
                let s = sys::current();
                let _ = s.unlinkat(Fd::CWD, &tmp, sysabi::AtFlags::empty());
                let res = s.symlinkat(libname, Fd::CWD, &tmp).and_then(|()| {
                    s.renameat2(Fd::CWD, &tmp, Fd::CWD, &real, RenameFlags::empty())
                });
                match res {
                    Ok(()) => changed = true,
                    Err(e) => {
                        let _ = s.unlinkat(Fd::CWD, &tmp, sysabi::AtFlags::empty());
                        self.warn(
                            &format!(
                                "Can't link {} to {}",
                                io::lossy(&full_soname),
                                io::lossy(libname)
                            ),
                            Some(e),
                        );
                    }
                }
            }
        }
        if self.verbose {
            let mut out = io::stdout();
            let _ = write!(
                out,
                "\t{} -> {}{}\n",
                io::lossy(soname),
                io::lossy(libname),
                if changed { " (changed)" } else { "" }
            );
        }
        changed
    }

    /// `search_dir`: varre um diretório, cria os links e acrescenta as entradas ao cache.
    fn search_dir(&mut self, index: usize) {
        let (dir, from) = {
            let d = &self.dirs[index];
            (d.path.clone(), d.from.clone())
        };
        let real_dir = self.rp(&dir);
        let entries = match sys::read_dir(&real_dir) {
            Ok(e) => e,
            Err(e) => {
                if self.verbose {
                    self.warn(&format!("Can't open directory {}", io::lossy(&dir)), Some(e));
                }
                return;
            }
        };
        if self.verbose {
            let mut out = io::stdout();
            let _ = write!(out, "{}:", io::lossy(&dir));
            match &from {
                Some((f, 0)) => {
                    let _ = write!(out, " (from {})", io::lossy(f));
                }
                Some((f, l)) => {
                    let _ = write!(out, " (from {}:{})", io::lossy(f), l);
                }
                None => {}
            }
            let _ = out.write_all(b"\n");
        }
        let mut names: Vec<Vec<u8>> = entries.into_iter().map(|e| e.name).collect();
        names.sort();
        let mut cands: Vec<Candidate> = Vec::new();
        for name in names {
            let looks_lib = (name.starts_with(b"lib") || name.starts_with(b"ld-"))
                && name.windows(3).any(|w| w == b".so");
            if !looks_lib {
                continue;
            }
            let len = name.len();
            if name.ends_with(b".#prelink#")
                || (len >= 17 && name[len - 17..].starts_with(b".#prelink#."))
                || name.contains(&b';')
                || name.ends_with(b".dpkg-new")
                || name.ends_with(b".dpkg-tmp")
            {
                continue;
            }
            let full = join(&dir, &name);
            let real = self.rp(&full);
            let Ok(lst) = sys::lstat(&real) else {
                continue;
            };
            let is_link = lst.file_type() == FileType::Symlink;
            let st = match sys::stat(&real) {
                Ok(st) => st,
                Err(e) => {
                    self.warn(&format!("Cannot stat {}", io::lossy(&full)), Some(e));
                    continue;
                }
            };
            if st.file_type() != FileType::Regular {
                continue;
            }
            let Ok(data) = io::read_path(&real) else {
                continue;
            };
            let Some((soname, flags)) = self.process_file(&full, &data) else {
                continue;
            };
            let soname = soname.unwrap_or_else(|| name.clone());
            cands.push(Candidate {
                name,
                soname,
                flags,
                is_link,
            });
        }
        // Um grupo por soname, na ordem de aparição; vence o nome maior pelo `libcmp`.
        let mut order: Vec<Vec<u8>> = Vec::new();
        for c in &cands {
            if !(c.is_link && c.name != c.soname) && !order.contains(&c.soname) {
                order.push(c.soname.clone());
            }
        }
        let mut found: Vec<CacheEntry> = Vec::new();
        for soname in &order {
            let mut best: Option<&Candidate> = None;
            for c in cands
                .iter()
                .filter(|c| &c.soname == soname && !(c.is_link && c.name != c.soname))
            {
                if best.is_none_or(|b| libcmp(&c.name, &b.name) > 0) {
                    best = Some(c);
                }
            }
            let Some(best) = best else {
                continue;
            };
            self.create_links(&dir, &best.name, soname);
            found.push(CacheEntry {
                key: soname.clone(),
                path: join(&dir, soname),
                flags: best.flags,
                hwcap: 0,
            });
        }
        // Links de desenvolvimento (`libfoo.so -> libfoo.so.1`) entram com o próprio nome.
        for c in cands.iter().filter(|c| c.is_link && c.name != c.soname) {
            found.push(CacheEntry {
                key: c.name.clone(),
                path: join(&dir, &c.name),
                flags: c.flags,
                hwcap: 0,
            });
        }
        self.cache.extend(found);
    }

    fn manual_link(&self, library: &[u8]) {
        let real = self.rp(library);
        let st = match sys::lstat(&real) {
            Ok(st) => st,
            Err(e) => {
                self.warn(&format!("Cannot lstat {}", io::lossy(library)), Some(e));
                return;
            }
        };
        if st.file_type() != FileType::Regular {
            self.warn(
                &format!(
                    "Ignored file {} since it is not a regular file.",
                    io::lossy(library)
                ),
                None,
            );
            return;
        }
        let (path, libname) = match library.iter().rposition(|b| *b == b'/') {
            Some(0) => (b"/".to_vec(), library[1..].to_vec()),
            Some(i) => (library[..i].to_vec(), library[i + 1..].to_vec()),
            None => (b".".to_vec(), library.to_vec()),
        };
        let data = io::read_path(&real).unwrap_or_default();
        let is_elf = data.len() >= 4 && &data[..4] == b"\x7fELF";
        let Some((soname, _flags)) = is_elf.then(|| self.process_file(library, &data)).flatten() else {
            self.warn(
                &format!(
                    "No link created since soname could not be found for {}",
                    io::lossy(library)
                ),
                None,
            );
            return;
        };
        let soname = soname.unwrap_or_else(|| libname.clone());
        self.create_links(&path, &libname, &soname);
    }
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Monta o arquivo `glibc-ld.so.cache1.1`.
fn build_cache(entries: &mut [CacheEntry]) -> Vec<u8> {
    // `compare` do cache.c: ordem decrescente de nome (libcmp), depois flags e hwcap decrescentes.
    entries.sort_by(|a, b| {
        0.cmp(&libcmp(&a.key, &b.key))
            .then(b.flags.cmp(&a.flags))
            .then(b.hwcap.cmp(&a.hwcap))
    });
    let n = entries.len();
    let base = 48 + 24 * n as u32;
    let mut strings: Vec<u8> = Vec::new();
    let mut seen: HashMap<Vec<u8>, u32> = HashMap::new();
    let mut add = |s: &[u8], strings: &mut Vec<u8>| -> u32 {
        if let Some(o) = seen.get(s) {
            return *o;
        }
        let off = base + strings.len() as u32;
        strings.extend_from_slice(s);
        strings.push(0);
        seen.insert(s.to_vec(), off);
        off
    };
    let mut table: Vec<u8> = Vec::new();
    for e in entries.iter() {
        let key = add(&e.key, &mut strings);
        let val = add(&e.path, &mut strings);
        put32(&mut table, e.flags);
        put32(&mut table, key);
        put32(&mut table, val);
        put32(&mut table, 0);
        table.extend_from_slice(&e.hwcap.to_le_bytes());
    }
    let ext_off = (base as usize + strings.len()).div_ceil(4) * 4;
    let mut out = Vec::new();
    out.extend_from_slice(CACHE_MAGIC_NEW);
    out.extend_from_slice(CACHE_VERSION);
    put32(&mut out, n as u32);
    put32(&mut out, strings.len() as u32);
    out.push(2); // endianness: little
    out.extend_from_slice(&[0, 0, 0]);
    put32(&mut out, ext_off as u32);
    out.extend_from_slice(&[0u8; 12]);
    out.extend_from_slice(&table);
    out.extend_from_slice(&strings);
    out.resize(ext_off, 0);
    // Extensão: uma seção com a string do gerador.
    put32(&mut out, EXTENSION_MAGIC);
    put32(&mut out, 1);
    put32(&mut out, 0); // tag_generator
    put32(&mut out, 0);
    put32(&mut out, (ext_off + 8 + 16) as u32);
    put32(&mut out, GENERATOR.len() as u32);
    out.extend_from_slice(GENERATOR.as_bytes());
    out
}


fn flag_description(flags: u32) -> String {
    let mut s = String::new();
    match flags & 0xff {
        0 => s.push_str("libc4"),
        1 => s.push_str("ELF"),
        2 => s.push_str("libc5"),
        3 => s.push_str("libc6"),
        _ => s.push_str("unknown"),
    }
    match flags & 0xff00 {
        0 => {}
        0x0100 | 0x0400 | 0x0500 | 0x0700 => s.push_str(",64bit"),
        0x0300 => s.push_str(",x86-64"),
        0x0600 => s.push_str(",N32"),
        0x0800 => s.push_str(",x32"),
        0x0900 => s.push_str(",hard-float"),
        0x0a00 => s.push_str(",AArch64"),
        0x0b00 => s.push_str(",soft-float"),
        other => s.push_str(&format!(",{other}")),
    }
    s
}

/// `print_cache` (`-p`). Devolve o status de saída.
fn print_cache(prog: &str, name: &[u8], path: &[u8]) -> i32 {
    let data = match io::read_path(path) {
        Ok(d) => d,
        Err(e) => {
            io::eprint(format!(
                "{prog}: Can't open cache file {}\n: {}\n",
                io::lossy(name),
                e.message()
            ));
            return 1;
        }
    };
    let magic_len = CACHE_MAGIC_NEW.len() + CACHE_VERSION.len();
    if data.len() < 48 || data[..magic_len] != [CACHE_MAGIC_NEW, CACHE_VERSION].concat()[..] {
        io::eprint(format!("{prog}: File is not a cache file.\n\n"));
        return 1;
    }
    let nlibs = rd32(&data, 20).unwrap_or(0) as usize;
    let mut out = io::stdout();
    let _ = write!(
        out,
        "{nlibs} libs found in cache `{}'\n",
        io::lossy(name)
    );
    for i in 0..nlibs {
        let at = 48 + i * 24;
        let (Some(flags), Some(key), Some(val)) =
            (rd32(&data, at), rd32(&data, at + 4), rd32(&data, at + 8))
        else {
            break;
        };
        let hwcap = data
            .get(at + 16..at + 24)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap_or([0; 8])))
            .unwrap_or(0);
        let mut desc = flag_description(flags);
        if hwcap != 0 {
            desc.push_str(&format!(", hwcap: {hwcap:#x}"));
        }
        let _ = write!(
            out,
            "\t{} ({desc}) => {}\n",
            io::lossy(cstr_at(&data, key as usize)),
            io::lossy(cstr_at(&data, val as usize))
        );
    }
    let _ = write!(out, "Cache generated by: {GENERATOR}\n");
    0
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut cache_file: Vec<u8> = DEFAULT_CACHE.to_vec();
    let mut conf_file: Vec<u8> = DEFAULT_CONF.to_vec();
    let mut root: Vec<u8> = Vec::new();
    let (mut verbose, mut print, mut manual, mut only_cline) = (false, false, false, false);
    let (mut build_cache_opt, mut do_link) = (true, true);

    let mut getopt = Getopt::from_env(&argv[1..], "c:C:f:ilnNpr:vVX?", LONGOPTS);
    while let Some(r) = getopt.next_opt() {
        let opt = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!(
                    "{}\nTry `ldconfig --help' or `ldconfig --usage' for more information.\n",
                    e.message(&argv0)
                ));
                return 64;
            }
        };
        let arg = opt.arg.clone().unwrap_or_default();
        match opt.id {
            OPT_USAGE => {
                let _ = io::stdout().write_all(USAGE.as_bytes());
                return 0;
            }
            id if id == '?' as i32 => {
                let _ = io::stdout().write_all(HELP.as_bytes());
                return 0;
            }
            id if id == 'V' as i32 => {
                let _ = io::stdout().write_all(VERSION.as_bytes());
                return 0;
            }
            id if id == 'c' as i32 => {
                if !matches!(arg.as_slice(), b"old" | b"new" | b"compat") {
                    io::eprint(format!("{argv0}: Unknown format {}\n", io::lossy(&arg)));
                    return 1;
                }
            }
            id if id == 'C' as i32 => {
                cache_file = arg;
                build_cache_opt = true;
            }
            id if id == 'f' as i32 => conf_file = arg,
            id if id == 'i' as i32 => {}
            id if id == 'l' as i32 => manual = true,
            id if id == 'n' as i32 => {
                only_cline = true;
                build_cache_opt = false;
            }
            id if id == 'N' as i32 => build_cache_opt = false,
            id if id == 'p' as i32 => print = true,
            id if id == 'r' as i32 => root = arg,
            id if id == 'v' as i32 => verbose = true,
            id if id == 'X' as i32 => do_link = false,
            _ => {}
        }
    }
    let operands = getopt.operands();

    while root.len() > 1 && root.ends_with(b"/") {
        root.pop();
    }
    if root == b"/" {
        root.clear();
    }
    let mut ld = Ldconfig {
        prog: argv0.clone(),
        root,
        verbose,
        do_link,
        dirs: Vec::new(),
        cache: Vec::new(),
    };
    if print {
        let path = ld.rp(&cache_file);
        let status = print_cache(&argv0, &cache_file, &path);
        let _ = io::flush_stdout();
        return status;
    }

    if manual {
        for lib in &operands {
            ld.manual_link(lib);
        }
        return 0;
    }

    for dir in &operands {
        ld.add_dir(dir, Some((b"<command line>".to_vec(), 0)));
    }
    if !only_cline {
        ld.parse_conf(&conf_file, 0);
        for d in SYSTEM_DIRS {
            ld.add_dir(d, None);
        }
    }

    for i in 0..ld.dirs.len() {
        ld.search_dir(i);
    }

    if build_cache_opt {
        let image = build_cache(&mut ld.cache);
        let real = ld.rp(&cache_file);
        let mut tmp = real.clone();
        tmp.push(b'~');
        let mut file = match io::File::open_with(
            &tmp,
            OFlags::WRONLY | OFlags::CREAT | OFlags::EXCL,
            0o644,
        ) {
            Ok(f) => f,
            Err(e) => {
                ld.warn(
                    &format!("Can't create temporary cache file {}", io::lossy(&tmp)),
                    Some(e),
                );
                let _ = io::flush_stdout();
                return 1;
            }
        };
        let written = file.write_all(&image);
        drop(file);
        let s = sys::current();
        if let Err(e) = written {
            let _ = s.unlinkat(Fd::CWD, &tmp, sysabi::AtFlags::empty());
            ld.warn("Writing of cache data failed", Some(sysabi::Errno::from_io(&e)));
            let _ = io::flush_stdout();
            return 1;
        }
        if let Err(e) = s.renameat2(Fd::CWD, &tmp, Fd::CWD, &real, RenameFlags::empty()) {
            let _ = s.unlinkat(Fd::CWD, &tmp, sysabi::AtFlags::empty());
            ld.warn("renaming temporary cache file failed", Some(e));
            let _ = io::flush_stdout();
            return 1;
        }
    }
    if io::flush_stdout().is_err() {
        return 1;
    }
    0
}
