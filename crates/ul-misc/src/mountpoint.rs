//! `mountpoint` do util-linux 2.41 (pacote util-linux do Debian 13): diz se um diretório ou arquivo é
//! ponto de montagem.
//!
//! Porte do `sys-utils/mountpoint.c`: o caminho é procurado entre os alvos de `/proc/self/mountinfo`
//! de trás pra frente (como `mnt_table_find_target` com `MNT_ITER_BACKWARD`): primeiro o caminho como
//! veio, depois o caminho canônico, depois os alvos canonizados. Sem o mountinfo vale o teste
//! tradicional (dispositivo diferente do de `..`, ou mesmo inode). `-d` imprime o `maj:min` do sistema
//! de arquivos, `-x` o do dispositivo de bloco, `-q` cala tudo. Saída 0 (é ponto de montagem), 32 (não
//! é) ou 1 (erro).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Fd, FileType, Stat, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const OPT_NOFOLLOW: i32 = 128;
const MOUNTPOINT_EXIT_NOMNT: i32 = 32;

const LONGS: &[LongOpt] = &[
    LongOpt::new("quiet", HasArg::No, b'q' as i32),
    LongOpt::new("nofollow", HasArg::No, OPT_NOFOLLOW),
    LongOpt::new("fs-devno", HasArg::No, b'd' as i32),
    LongOpt::new("devno", HasArg::No, b'x' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [-qd] /path/to/directory
 {short} -x /dev/device

Check whether a directory or file is a mountpoint.

Options:
 -q, --quiet        quiet mode - don't print anything
     --nofollow     do not follow symlink
 -d, --fs-devno     print maj:min device number of the filesystem
 -x, --devno        print maj:min device number of the block device

 -h, --help         display this help
 -V, --version      display version

For more details see mountpoint(1).
"
    )
}

/// `major()` da glibc.
fn major(dev: u64) -> u32 {
    (((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff)) as u32
}

/// `minor()` da glibc.
fn minor(dev: u64) -> u32 {
    ((dev & 0xff) | ((dev >> 12) & !0xff)) as u32
}

/// Desfaz o escape octal (`\040`) que o kernel aplica aos campos do mountinfo.
fn unmangle(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'\\'
            && i + 3 < s.len()
            && s[i + 1..i + 4].iter().all(|b| (b'0'..=b'7').contains(b))
        {
            out.push(((s[i + 1] - b'0') << 6) | ((s[i + 2] - b'0') << 3) | (s[i + 3] - b'0'));
            i += 4;
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

/// Uma linha útil do mountinfo: o `maj:min` e o alvo.
struct MountEntry {
    major: u32,
    minor: u32,
    target: Vec<u8>,
}

/// `mnt_new_table_from_file(_PATH_PROC_MOUNTINFO)`: `None` se o arquivo não abre.
fn read_mountinfo() -> Option<Vec<MountEntry>> {
    let data = sys::read_file(b"/proc/self/mountinfo").ok()?;
    let mut out = Vec::new();
    for line in data.split(|b| *b == b'\n') {
        let fields: Vec<&[u8]> = line
            .split(|b| *b == b' ')
            .filter(|f| !f.is_empty())
            .collect();
        if fields.len() < 5 {
            continue;
        }
        let devno = fields[2];
        let Some(colon) = devno.iter().position(|b| *b == b':') else {
            continue;
        };
        let parse = |s: &[u8]| {
            std::str::from_utf8(s)
                .ok()
                .and_then(|t| t.parse::<u32>().ok())
        };
        let (Some(major), Some(minor)) = (parse(&devno[..colon]), parse(&devno[colon + 1..]))
        else {
            continue;
        };
        out.push(MountEntry {
            major,
            minor,
            target: unmangle(fields[4]),
        });
    }
    Some(out)
}

/// `streq_paths` da libmount: iguais ignorando barras finais (`/proc/` e `/proc`).
fn streq_paths(a: &[u8], b: &[u8]) -> bool {
    fn trim(p: &[u8]) -> &[u8] {
        let mut end = p.len();
        while end > 1 && p[end - 1] == b'/' {
            end -= 1;
        }
        &p[..end]
    }
    trim(a) == trim(b)
}

/// `realpath(3)`: o caminho absoluto sem `.`, `..` nem symlinks; `None` se algum componente falha.
fn realpath(path: &[u8]) -> Option<Vec<u8>> {
    if path.is_empty() {
        return None;
    }
    let mut pending: Vec<Vec<u8>> = Vec::new();
    let push_parts = |pending: &mut Vec<Vec<u8>>, p: &[u8]| {
        // Empilhados ao contrário: o topo é o próximo componente.
        for part in p.split(|b| *b == b'/').rev() {
            if !part.is_empty() && part != b"." {
                pending.push(part.to_vec());
            }
        }
    };
    let mut done: Vec<Vec<u8>> = Vec::new();
    if path[0] != b'/' {
        let cwd = sys::current().getcwd().ok()?;
        push_parts(&mut pending, path);
        for part in cwd.split(|b| *b == b'/') {
            if !part.is_empty() {
                done.push(part.to_vec());
            }
        }
    } else {
        push_parts(&mut pending, path);
    }
    let join = |parts: &[Vec<u8>]| -> Vec<u8> {
        let mut s = Vec::new();
        for p in parts {
            s.push(b'/');
            s.extend_from_slice(p);
        }
        if s.is_empty() {
            s.push(b'/');
        }
        s
    };
    let mut links = 0;
    while let Some(part) = pending.pop() {
        if part == b".." {
            done.pop();
            continue;
        }
        done.push(part);
        let full = join(&done);
        let st = sys::lstat(&full).ok()?;
        if st.file_type() == FileType::Symlink {
            links += 1;
            if links > 40 {
                return None;
            }
            let target = sys::current().readlinkat(Fd::CWD, &full).ok()?;
            done.pop();
            if target.first() == Some(&b'/') {
                done.clear();
            }
            push_parts(&mut pending, &target);
        } else if !pending.is_empty() && st.file_type() != FileType::Directory {
            return None;
        }
    }
    Some(join(&done))
}

/// `mnt_table_find_target(tb, path, MNT_ITER_BACKWARD)`: o `maj:min` do ponto de montagem achado.
fn find_target(table: &[MountEntry], path: &[u8]) -> Option<(u32, u32)> {
    // 1) o caminho como veio
    if let Some(e) = table.iter().rev().find(|e| streq_paths(&e.target, path)) {
        return Some((e.major, e.minor));
    }
    // 2) o caminho canônico
    let cn = realpath(path)?;
    if let Some(e) = table.iter().rev().find(|e| streq_paths(&e.target, &cn)) {
        return Some((e.major, e.minor));
    }
    // 3) os alvos canonizados
    table
        .iter()
        .rev()
        .find(|e| realpath(&e.target).is_some_and(|t| streq_paths(&t, &cn)))
        .map(|e| (e.major, e.minor))
}

/// `dir_to_device`: o dispositivo do sistema de arquivos montado em `path`, ou `None` se não é ponto
/// de montagem.
fn dir_to_device(path: &[u8], st: &Stat) -> Option<(u32, u32)> {
    let Some(table) = read_mountinfo() else {
        // Fallback: o jeito tradicional, comparando com o diretório pai.
        let cn = realpath(path)?;
        let mut buf = cn;
        buf.extend_from_slice(b"/..");
        let pst = sys::stat(&buf).ok()?;
        if st.dev != pst.dev || st.ino == pst.ino {
            return Some((major(st.dev), minor(st.dev)));
        }
        return None;
    };
    find_target(&table, path)
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut quiet = false;
    let mut nofollow = false;
    let mut fs_devno = false;
    let mut dev_devno = false;

    let mut g = Getopt::from_env(&argv[1..], "qdxhV", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        match o.id {
            OPT_NOFOLLOW => nofollow = true,
            id if id == b'q' as i32 => quiet = true,
            id if id == b'd' as i32 => fs_devno = true,
            id if id == b'x' as i32 => dev_devno = true,
            id if id == b'h' as i32 => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
                return 0;
            }
            id if id == b'V' as i32 => {
                ul::print_version(&short);
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    let operands = g.operands();
    if operands.len() != 1 {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }
    if nofollow && dev_devno {
        ul::warnx(&short, "--devno and --nofollow are mutually exclusive");
        return 1;
    }

    let path = &operands[0];
    let st = match if nofollow {
        sys::lstat(path)
    } else {
        sys::stat(path)
    } {
        Ok(st) => st,
        Err(e) => {
            if !quiet {
                ul::warn(&short, io::lossy(path), e);
            }
            return 1;
        }
    };

    let mut out = io::stdout();
    if dev_devno {
        if st.file_type() != FileType::BlockDevice {
            if !quiet {
                ul::warnx(&short, format!("{}: not a block device", io::lossy(path)));
            }
            return MOUNTPOINT_EXIT_NOMNT;
        }
        let _ = out.write_all(format!("{}:{}\n", major(st.rdev), minor(st.rdev)).as_bytes());
        return 0;
    }

    let dev = if nofollow && st.file_type() == FileType::Symlink {
        None
    } else {
        dir_to_device(path, &st)
    };
    let Some((maj, min)) = dev else {
        if !quiet {
            let mut line = path.clone();
            line.extend_from_slice(b" is not a mountpoint\n");
            let _ = out.write_all(&line);
        }
        return MOUNTPOINT_EXIT_NOMNT;
    };
    if fs_devno {
        let _ = out.write_all(format!("{maj}:{min}\n").as_bytes());
    } else if !quiet {
        let mut line = path.clone();
        line.extend_from_slice(b" is a mountpoint\n");
        let _ = out.write_all(&line);
    }
    0
}
