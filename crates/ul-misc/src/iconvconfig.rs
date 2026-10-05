//! `iconvconfig` da glibc 2.41 (libc-bin do Debian 13), portado de `iconv/iconvconfig.c`: lê os
//! arquivos `gconv-modules` e `gconv-modules.d/*.conf` e grava o cache `gconv-modules.cache` de
//! carga rápida do `iconv`.
//!
//! Opções: `-o`/`--output=ARQUIVO`, `--nostdlib`, `--prefix=CAMINHO`, `-?`/`--help`, `--usage` e
//! `-V`. Os diretórios dados na linha de comando entram antes do padrão
//! (`/usr/lib/x86_64-linux-gnu/gconv`).
//!
//! O layout do cache segue o `gconv_cache.h` (cabeçalho, strings, tabela de hash, tabela de
//! módulos), mas a ordem das strings e dos módulos é simplificada: o conteúdo é equivalente para o
//! leitor, não necessariamente idêntico byte a byte ao do original.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Write;

use sysabi::{Fd, OFlags, RenameFlags, sys};

use crate::util::io;
use crate::util::{Getopt, HasArg, LongOpt};

const OPT_USAGE: i32 = 256;
const OPT_NOSTDLIB: i32 = 257;
const OPT_PREFIX: i32 = 258;

const LONGOPTS: &[LongOpt] = &[
    LongOpt::new("output", HasArg::Required, 'o' as i32),
    LongOpt::new("nostdlib", HasArg::No, OPT_NOSTDLIB),
    LongOpt::new("prefix", HasArg::Required, OPT_PREFIX),
    LongOpt::new("help", HasArg::No, '?' as i32),
    LongOpt::new("usage", HasArg::No, OPT_USAGE),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

const HELP: &str = r#"Usage: iconvconfig [OPTION...] [DIR...]
Create fastloading iconv module configuration file.

      --nostdlib             Do not search standard directories, only those on
                             the command line
  -o, --output=FILE          Put output in FILE instead of installed location
                             (--prefix does not apply to FILE)
      --prefix=PATH          Prefix used for all file accesses
  -?, --help                 Give this help list
      --usage                Give a short usage message
  -V, --version              Print program version

Mandatory or optional arguments to long options are also mandatory or optional
for any corresponding short options.

For bug reporting instructions, please see:
<http://www.debian.org/Bugs/>.
"#;

const USAGE: &str = r#"Usage: iconvconfig [-?V] [-o FILE] [--nostdlib] [--output=FILE] [--prefix=PATH]
            [--help] [--usage] [--version] [DIR...]
"#;

const VERSION: &str = "iconvconfig (Debian GLIBC 2.41-12+deb13u4) 2.41
Copyright (C) 2024 Free Software Foundation, Inc.
This is free software; see the source for copying conditions.  There is NO
warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
Written by Ulrich Drepper.
";

const GCONV_DIR: &[u8] = b"/usr/lib/x86_64-linux-gnu/gconv";
const CACHE_MAGIC: u32 = 0x2001_0324;

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// Uma linha `module` do `gconv-modules`.
struct Module {
    from: Vec<u8>,
    to: Vec<u8>,
    dir: Vec<u8>,
    file: Vec<u8>,
}

fn upper(s: &[u8]) -> Vec<u8> {
    s.to_ascii_uppercase()
}

/// Nome de conjunto de caracteres normalizado: maiúsculas e `//` no fim.
fn norm(s: &[u8]) -> Vec<u8> {
    let mut n = upper(s);
    if !n.ends_with(b"/") {
        n.extend_from_slice(b"//");
    }
    n
}

/// `hash_string` do `iconvconfig.c` (o hash ELF).
fn hash_string(s: &[u8]) -> u32 {
    let mut h: u32 = 0;
    for &c in s {
        h = (h << 4).wrapping_add(u32::from(c));
        let g = h & 0xf000_0000;
        if g != 0 {
            h ^= g >> 24;
            h ^= g;
        }
    }
    h
}

fn is_prime(n: usize) -> bool {
    if n < 2 {
        return false;
    }
    let mut d = 2;
    while d * d <= n {
        if n % d == 0 {
            return false;
        }
        d += 1;
    }
    true
}

fn next_prime(mut n: usize) -> usize {
    while !is_prime(n) {
        n += 1;
    }
    n
}

fn put32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn parse_file(path: &[u8], dir: &[u8], aliases: &mut Vec<(Vec<u8>, Vec<u8>)>, mods: &mut Vec<Module>) {
    let Ok(data) = io::read_path(path) else {
        return;
    };
    for raw in data.split(|b| *b == b'\n') {
        let line = match raw.iter().position(|b| *b == b'#') {
            Some(p) => &raw[..p],
            None => raw,
        };
        let words: Vec<&[u8]> = line
            .split(|b| matches!(*b, b' ' | b'\t' | b'\r'))
            .filter(|w| !w.is_empty())
            .collect();
        match words.as_slice() {
            [b"alias", from, to, ..] => aliases.push((norm(from), norm(to))),
            [b"module", from, to, file, ..] => {
                let (d, f) = match file.iter().rposition(|b| *b == b'/') {
                    Some(i) => (file[..i].to_vec(), file[i + 1..].to_vec()),
                    None => (Vec::new(), file.to_vec()),
                };
                let _ = dir;
                mods.push(Module {
                    from: norm(from),
                    to: norm(to),
                    dir: d,
                    file: f,
                });
            }
            _ => {}
        }
    }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut output: Option<Vec<u8>> = None;
    let mut nostdlib = false;
    let mut prefix: Vec<u8> = Vec::new();

    let mut getopt = Getopt::from_env(&argv[1..], "o:?V", LONGOPTS);
    while let Some(r) = getopt.next_opt() {
        let opt = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!(
                    "{}\nTry `iconvconfig --help' or `iconvconfig --usage' for more information.\n",
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
            id if id == 'o' as i32 => output = Some(arg),
            OPT_NOSTDLIB => nostdlib = true,
            OPT_PREFIX => prefix = arg,
            _ => {}
        }
    }
    let mut dirs = getopt.operands();
    if !nostdlib {
        dirs.push(GCONV_DIR.to_vec());
    }
    let pre = |p: &[u8]| -> Vec<u8> {
        let mut v = prefix.clone();
        v.extend_from_slice(p);
        v
    };

    let mut aliases: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut mods: Vec<Module> = Vec::new();
    for d in &dirs {
        let mut file = d.clone();
        file.extend_from_slice(b"/gconv-modules");
        parse_file(&pre(&file), d, &mut aliases, &mut mods);
        let mut confd = d.clone();
        confd.extend_from_slice(b"/gconv-modules.d");
        if let Ok(entries) = sys::read_dir(&pre(&confd)) {
            let mut names: Vec<Vec<u8>> = entries
                .into_iter()
                .map(|e| e.name)
                .filter(|n| n.ends_with(b".conf"))
                .collect();
            names.sort();
            for n in names {
                let mut f = confd.clone();
                f.push(b'/');
                f.extend_from_slice(&n);
                parse_file(&pre(&f), d, &mut aliases, &mut mods);
            }
        }
    }

    // Tabela de strings (sem repetição) e de módulos.
    let mut strings: Vec<u8> = Vec::new();
    let mut seen: HashMap<Vec<u8>, u32> = HashMap::new();
    let mut intern = |s: &[u8], strings: &mut Vec<u8>| -> u32 {
        if let Some(o) = seen.get(s) {
            return *o;
        }
        let off = strings.len() as u32;
        strings.extend_from_slice(s);
        strings.push(0);
        seen.insert(s.to_vec(), off);
        off
    };
    // Nome -> índice do módulo; apelidos apontam para o alvo.
    let mut names: Vec<(Vec<u8>, u32)> = Vec::new();
    let mut module_rows: Vec<[u32; 6]> = Vec::new();
    for (i, m) in mods.iter().enumerate() {
        let canon = intern(&m.from, &mut strings);
        let fromdir = intern(&m.dir, &mut strings);
        let fromname = intern(&m.file, &mut strings);
        let todir = intern(&m.dir, &mut strings);
        let toname = intern(&m.to, &mut strings);
        module_rows.push([canon, fromdir, fromname, todir, toname, 0]);
        if !names.iter().any(|(n, _)| *n == m.from) {
            names.push((m.from.clone(), i as u32));
        }
    }
    for (alias, target) in &aliases {
        if let Some((_, idx)) = names.iter().find(|(n, _)| n == target).cloned()
            && !names.iter().any(|(n, _)| n == alias)
        {
            names.push((alias.clone(), idx));
        }
    }
    let mut name_offsets: Vec<(u32, u32)> = Vec::new();
    for (n, idx) in &names {
        name_offsets.push((intern(n, &mut strings), *idx));
    }
    while strings.len() % 4 != 0 {
        strings.push(0);
    }

    let hash_size = next_prime(name_offsets.len() * 4 / 3 + 1).max(3);
    let mut hash: Vec<(u32, u32)> = vec![(0, 0); hash_size];
    let mut used = vec![false; hash_size];
    for ((off, idx), (name, _)) in name_offsets.iter().zip(names.iter()) {
        let h = hash_string(name) as usize;
        let mut slot = h % hash_size;
        let step = 1 + h % (hash_size - 2);
        while used[slot] {
            slot = (slot + step) % hash_size;
        }
        used[slot] = true;
        hash[slot] = (*off, *idx);
    }

    let header_size = 24u32;
    let string_offset = header_size;
    let hash_offset = string_offset + strings.len() as u32;
    let module_offset = hash_offset + (hash_size as u32) * 8;
    let mut out = Vec::new();
    put32(&mut out, CACHE_MAGIC);
    put32(&mut out, string_offset);
    put32(&mut out, hash_offset);
    put32(&mut out, hash_size as u32);
    put32(&mut out, module_offset);
    put32(&mut out, module_offset + module_rows.len() as u32 * 24);
    out.extend_from_slice(&strings);
    for (off, idx) in &hash {
        put32(&mut out, *off);
        put32(&mut out, *idx);
    }
    for row in &module_rows {
        for v in row {
            put32(&mut out, *v);
        }
    }

    let target = match output {
        Some(o) => o,
        None => pre(b"/usr/lib/x86_64-linux-gnu/gconv/gconv-modules.cache"),
    };
    let mut tmp = target.clone();
    tmp.extend_from_slice(format!(".{}", sys::current().getpid()).as_bytes());
    let mut file = match io::File::open_with(&tmp, OFlags::WRONLY | OFlags::CREAT | OFlags::EXCL, 0o644) {
        Ok(f) => f,
        Err(e) => {
            io::eprint(format!(
                "{argv0}: cannot create temporary file: {}\n",
                e.message()
            ));
            return 1;
        }
    };
    let res = file.write_all(&out);
    drop(file);
    let s = sys::current();
    if let Err(e) = res {
        let _ = s.unlinkat(Fd::CWD, &tmp, sysabi::AtFlags::empty());
        io::eprint(format!("{argv0}: error while writing to output file: {e}\n"));
        return 1;
    }
    if let Err(e) = s.renameat2(Fd::CWD, &tmp, Fd::CWD, &target, RenameFlags::empty()) {
        let _ = s.unlinkat(Fd::CWD, &tmp, sysabi::AtFlags::empty());
        io::eprint(format!(
            "{argv0}: cannot rename temporary file: {}\n",
            e.message()
        ));
        return 1;
    }
    0
}
