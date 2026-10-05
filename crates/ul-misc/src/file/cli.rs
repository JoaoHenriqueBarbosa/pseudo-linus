// Porte para Rust do file.c, fsmagic.c e da parte de magic.c que abre e lê o arquivo, do file 5.46.
//
// Copyright (c) Ian F. Darwin 1986-1995.
// Software written by Ian F. Darwin and others;
// maintained 1995-present by Christos Zoulas and others.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions
// are met:
// 1. Redistributions of source code must retain the above copyright
//    notice immediately at the beginning of the file, without modification,
//    this list of conditions, and the following disclaimer.
// 2. Redistributions in binary form must reproduce the above copyright
//    notice, this list of conditions and the following disclaimer in the
//    documentation and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE AUTHOR AND CONTRIBUTORS ``AS IS'' AND
// ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
// ARE DISCLAIMED. IN NO EVENT SHALL THE AUTHOR OR CONTRIBUTORS BE LIABLE FOR
// ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS
// OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION)
// HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT
// LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY
// OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF
// SUCH DAMAGE.

//! O programa `file`: opções, o tipo pelo `stat` (diretório, link, vazio...) e a leitura do
//! conteúdo que vai pro [`file_buffer`](super::funcs::file_buffer).

use std::ffi::OsString;
use std::io::Write;

use sysabi::types::mode::*;
use sysabi::{AccessMode, AtFlags, Ctx, Errno, Fd, OFlags, sys};

use super::funcs::file_buffer;
use super::magic::*;
use super::softmagic::Buffer;
use super::wchar;
use crate::util::getopt::{Getopt, HasArg, LongOpt};
use crate::util::io;

const USAGE: &str = "Usage: file [-bcCdEhikLlNnprsSvzZ0] [--apple] [--extension] [--mime-encoding]
            [--mime-type] [-e <testname>] [-F <separator>]  [-f <namefile>]
            [-m <magicfiles>] [-P <parameter=value>] [--exclude-quiet]
            <file> ...
       file -C [-m <magicfiles>]
       file [--help]
";

const HELP: &str = "Usage: file [OPTION...] [FILE...]
Determine type of FILEs.

      --help                 display this help and exit
  -v, --version              output version information and exit
  -m, --magic-file LIST      use LIST as a colon-separated list of magic
                               number files
  -z, --uncompress           try to look inside compressed files
  -Z, --uncompress-noreport  only print the contents of compressed files
  -b, --brief                do not prepend filenames to output lines
  -c, --checking-printout    print the parsed form of the magic file, use in
                               conjunction with -m to debug a new magic file
                               before installing it
  -e, --exclude TEST         exclude TEST from the list of test to be
                               performed for file. Valid tests are:
                               apptype, ascii, cdf, compress, csv, elf,
                               encoding, soft, tar, json, simh,
                               text, tokens
      --exclude-quiet TEST   like exclude, but ignore unknown tests
  -f, --files-from FILE      read the filenames to be examined from FILE
  -F, --separator STRING     use string as separator instead of `:'
  -i, --mime                 output MIME type strings (--mime-type and
                               --mime-encoding)
      --apple                output the Apple CREATOR/TYPE
      --extension            output a slash-separated list of extensions
      --mime-type            output the MIME type
      --mime-encoding        output the MIME encoding
  -k, --keep-going           don't stop at the first match
  -l, --list                 list magic strength
  -L, --dereference          follow symlinks (default if POSIXLY_CORRECT is set)
  -h, --no-dereference       don't follow symlinks (default if POSIXLY_CORRECT is not set) (default)
  -n, --no-buffer            do not buffer output
  -N, --no-pad               do not pad output
  -0, --print0               terminate filenames with ASCII NUL
  -p, --preserve-date        preserve access times on files
  -P, --parameter            set file engine parameter limits
                                   bytes 7340032 max bytes to look inside file
                               elf_notes     256 max ELF notes processed
                               elf_phnum    2048 max ELF prog sections processed
                               elf_shnum   32768 max ELF sections processed
                               elf_shsize 134217728 max ELF section size
                                encoding   65536 max bytes to scan for encoding
                                   indir      50 recursion limit for indirection
                                    name     100 use limit for name/use magic
                                   regex    8192 length limit for REGEX searches
                                 magwarn      64 maximum number of magic warnings
  -r, --raw                  don't translate unprintable chars to \\ooo
  -s, --special-files        treat special (block/char devices) files as
                             ordinary ones
  -S, --no-sandbox           disable system call sandboxing
  -C, --compile              compile file specified by -m
  -d, --debug                print debugging messages

Report bugs to https://bugs.astron.com/
";

const OPT_HELP: i32 = 1001;
const OPT_APPLE: i32 = 1002;
const OPT_EXTENSIONS: i32 = 1003;
const OPT_MIME_TYPE: i32 = 1004;
const OPT_MIME_ENCODING: i32 = 1005;
const OPT_EXCLUDE_QUIET: i32 = 1006;

const LONGS: &[LongOpt] = &[
    LongOpt::new("help", HasArg::No, OPT_HELP),
    LongOpt::new("version", HasArg::No, b'v' as i32),
    LongOpt::new("magic-file", HasArg::Required, b'm' as i32),
    LongOpt::new("uncompress", HasArg::No, b'z' as i32),
    LongOpt::new("uncompress-noreport", HasArg::No, b'Z' as i32),
    LongOpt::new("brief", HasArg::No, b'b' as i32),
    LongOpt::new("checking-printout", HasArg::No, b'c' as i32),
    LongOpt::new("exclude", HasArg::Required, b'e' as i32),
    LongOpt::new("exclude-quiet", HasArg::Required, OPT_EXCLUDE_QUIET),
    LongOpt::new("files-from", HasArg::Required, b'f' as i32),
    LongOpt::new("separator", HasArg::Required, b'F' as i32),
    LongOpt::new("mime", HasArg::No, b'i' as i32),
    LongOpt::new("apple", HasArg::No, OPT_APPLE),
    LongOpt::new("extension", HasArg::No, OPT_EXTENSIONS),
    LongOpt::new("mime-type", HasArg::No, OPT_MIME_TYPE),
    LongOpt::new("mime-encoding", HasArg::No, OPT_MIME_ENCODING),
    LongOpt::new("keep-going", HasArg::No, b'k' as i32),
    LongOpt::new("list", HasArg::No, b'l' as i32),
    LongOpt::new("dereference", HasArg::No, b'L' as i32),
    LongOpt::new("no-dereference", HasArg::No, b'h' as i32),
    LongOpt::new("no-buffer", HasArg::No, b'n' as i32),
    LongOpt::new("no-pad", HasArg::No, b'N' as i32),
    LongOpt::new("print0", HasArg::No, b'0' as i32),
    LongOpt::new("preserve-date", HasArg::No, b'p' as i32),
    LongOpt::new("parameter", HasArg::Required, b'P' as i32),
    LongOpt::new("raw", HasArg::No, b'r' as i32),
    LongOpt::new("special-files", HasArg::No, b's' as i32),
    LongOpt::new("no-sandbox", HasArg::No, b'S' as i32),
    LongOpt::new("compile", HasArg::No, b'C' as i32),
    LongOpt::new("debug", HasArg::No, b'd' as i32),
];

const NV: &[(&str, u32)] = &[
    ("apptype", MAGIC_NO_CHECK_APPTYPE),
    ("ascii", MAGIC_NO_CHECK_TEXT),
    ("cdf", MAGIC_NO_CHECK_CDF),
    ("compress", MAGIC_NO_CHECK_COMPRESS),
    ("csv", MAGIC_NO_CHECK_CSV),
    ("elf", MAGIC_NO_CHECK_ELF),
    ("encoding", MAGIC_NO_CHECK_ENCODING),
    ("soft", MAGIC_NO_CHECK_SOFT),
    ("tar", MAGIC_NO_CHECK_TAR),
    ("json", MAGIC_NO_CHECK_JSON),
    ("simh", MAGIC_NO_CHECK_SIMH),
    ("text", MAGIC_NO_CHECK_TEXT),
    ("tokens", MAGIC_NO_CHECK_TOKENS),
];

fn errno_msg(e: Errno) -> String {
    e.message()
}

fn stat_of(path: &[u8], follow: bool) -> Result<sysabi::Stat, Errno> {
    if follow { sys::stat(path) } else { sys::lstat(path) }
}

/// `handle_mime()`.
fn handle_mime(ms: &mut MagicSet, mime: u32, s: &str) -> Result<(), Fail> {
    if mime & MAGIC_MIME_TYPE != 0 {
        ms.print(format!("inode/{s}").as_bytes())?;
        if mime & MAGIC_MIME_ENCODING != 0 {
            ms.print(b"; charset=")?;
        }
    }
    if mime & MAGIC_MIME_ENCODING != 0 {
        ms.print(b"binary")?;
    }
    Ok(())
}

fn bad_link(ms: &mut MagicSet, err: Errno, buf: &[u8]) -> i32 {
    let mime = ms.flags & MAGIC_MIME;
    if mime & MAGIC_MIME_TYPE != 0 {
        if ms.print(b"inode/symlink").is_err() {
            return -1;
        }
    } else if mime == 0 {
        if ms.flags & MAGIC_ERROR != 0 {
            ms.file_error(err.0, &format!("broken symbolic link to {}", io::lossy(buf)));
            return -1;
        }
        let mut t = b"broken symbolic link to ".to_vec();
        t.extend_from_slice(buf);
        if ms.print(&t).is_err() {
            return -1;
        }
    }
    1
}

fn major(rdev: u64) -> u64 {
    ((rdev >> 8) & 0xfff) | ((rdev >> 32) & !0xfff)
}

fn minor(rdev: u64) -> u64 {
    (rdev & 0xff) | ((rdev >> 12) & !0xff)
}

/// `file_fsmagic()`: o que o `stat` já diz. 1 = já descreveu, 0 = segue pro conteúdo, -1 erro.
fn file_fsmagic(ms: &mut MagicSet, fn_: Option<&[u8]>, sb: &mut Option<sysabi::Stat>) -> i32 {
    let Some(fname) = fn_ else { return 0 };
    let mime = ms.flags & MAGIC_MIME;
    let silent = ms.flags & (MAGIC_APPLE | MAGIC_EXTENSION) != 0;
    let st = match stat_of(fname, ms.flags & MAGIC_SYMLINK != 0) {
        Ok(s) => s,
        Err(e) => {
            if ms.flags & MAGIC_ERROR != 0 {
                ms.file_error(e.0, &format!("cannot stat `{}'", io::lossy(fname)));
                return -1;
            }
            let mut t = b"cannot open `".to_vec();
            t.extend_from_slice(fname);
            t.extend_from_slice(format!("' ({})", errno_msg(e)).as_bytes());
            if ms.print(&t).is_err() {
                return -1;
            }
            return 0;
        }
    };
    *sb = Some(st.clone());
    let did = std::cell::Cell::new(0);
    let comma = || {
        did.set(did.get() + 1);
        if did.get() > 1 { ", " } else { "" }
    };
    let r: Result<i32, Fail> = (|| {
        let mut ret = 1;
        if !silent && mime == 0 {
            if st.mode & 0o4000 != 0 {
                ms.print(format!("{}setuid", comma()).as_bytes())?;
            }
            if st.mode & 0o2000 != 0 {
                ms.print(format!("{}setgid", comma()).as_bytes())?;
            }
            if st.mode & 0o1000 != 0 {
                ms.print(format!("{}sticky", comma()).as_bytes())?;
            }
        }
        match st.mode & S_IFMT {
            S_IFDIR => {
                if mime != 0 {
                    handle_mime(ms, mime, "directory")?;
                } else if !silent {
                    ms.print(format!("{}directory", comma()).as_bytes())?;
                }
            }
            S_IFCHR | S_IFBLK => {
                let chr = st.mode & S_IFMT == S_IFCHR;
                if ms.flags & MAGIC_DEVICES != 0 {
                    ret = 0;
                } else if mime != 0 {
                    handle_mime(ms, mime, if chr { "chardevice" } else { "blockdevice" })?;
                } else if !silent {
                    let what = if chr { "character special" } else { "block special" };
                    ms.print(format!("{}{what} ({}/{})", comma(), major(st.rdev), minor(st.rdev)).as_bytes())?;
                }
            }
            S_IFIFO => {
                if ms.flags & MAGIC_DEVICES != 0 {
                } else if mime != 0 {
                    handle_mime(ms, mime, "fifo")?;
                } else if !silent {
                    ms.print(format!("{}fifo (named pipe)", comma()).as_bytes())?;
                }
            }
            S_IFLNK => {
                let link = sys::try_current().ok_or(Fail).and_then(|s| s.readlinkat(Fd::CWD, fname).map_err(|_| Fail));
                let buf = match link {
                    Ok(b) if !b.is_empty() => b,
                    _ => {
                        if mime != 0 {
                            handle_mime(ms, mime, "symlink")?;
                        } else if !silent {
                            let mut t = format!("{}unreadable symlink `", comma()).into_bytes();
                            t.extend_from_slice(fname);
                            t.extend_from_slice(b"' (Invalid argument)");
                            ms.print(&t)?;
                        }
                        return Ok(ret);
                    }
                };
                if let Err(e) = sys::stat(fname) {
                    return Ok(bad_link(ms, e, &buf));
                }
                let target: Vec<u8> = if buf.first() == Some(&b'/') {
                    buf.clone()
                } else {
                    match fname.iter().rposition(|&c| c == b'/') {
                        None => buf.clone(),
                        Some(p) => {
                            let mut t = fname[..=p].to_vec();
                            t.extend_from_slice(&buf);
                            t
                        }
                    }
                };
                if let Err(e) = sys::stat(&target) {
                    return Ok(bad_link(ms, e, &buf));
                }
                if mime != 0 {
                    handle_mime(ms, mime, "symlink")?;
                } else if !silent {
                    let mut t = format!("{}symbolic link to ", comma()).into_bytes();
                    t.extend_from_slice(&buf);
                    ms.print(&t)?;
                }
            }
            S_IFSOCK => {
                if mime != 0 {
                    handle_mime(ms, mime, "socket")?;
                } else if !silent {
                    ms.print(format!("{}socket", comma()).as_bytes())?;
                }
            }
            S_IFREG => {
                if ms.flags & MAGIC_DEVICES == 0 && st.size == 0 {
                    if mime != 0 {
                        handle_mime(ms, mime, "x-empty")?;
                    } else if !silent {
                        ms.print(format!("{}empty", comma()).as_bytes())?;
                    }
                } else {
                    ret = 0;
                }
            }
            m => {
                ms.file_error(0, &format!("invalid mode 0{m:o}"));
                return Err(Fail);
            }
        }
        if !silent && mime == 0 && did.get() > 0 && ret == 0 {
            ms.print(b" ")?;
        }
        if ret == 1 && silent {
            return Ok(0);
        }
        Ok(ret)
    })();
    r.unwrap_or(-1)
}

fn unreadable_info(ms: &mut MagicSet, md: u32, file: Option<&[u8]>) -> Result<(), Fail> {
    if let Some(f) = file
        && let Some(s) = sys::try_current()
    {
        if s.faccessat(Fd::CWD, f, AccessMode::W_OK, AtFlags::empty()).is_ok() {
            ms.print(b"writable, ")?;
        }
        if s.faccessat(Fd::CWD, f, AccessMode::X_OK, AtFlags::empty()).is_ok() {
            ms.print(b"executable, ")?;
        }
    }
    if md & S_IFMT == S_IFREG {
        ms.print(b"regular file, ")?;
    }
    ms.print(b"no read permission")
}

/// `file_or_fd()`: a descrição (ou `None` com o erro em `ms`).
fn file_or_fd(ms: &mut MagicSet, inname: Option<&[u8]>) -> Option<Vec<u8>> {
    ms.reset();
    let mut sb: Option<sysabi::Stat> = None;
    match file_fsmagic(ms, inname, &mut sb) {
        -1 => return ms.getbuffer(),
        0 => {}
        _ => return ms.getbuffer(),
    }
    let sysc = sys::try_current()?;
    let (fd, opened) = match inname {
        Some(name) => match sys::open(name, OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC, 0) {
            Ok(fd) => (fd, true),
            Err(_) => {
                if let Ok(st) = sys::stat(name) {
                    let _ = unreadable_info(ms, st.mode, Some(name));
                }
                return ms.getbuffer();
            }
        },
        None => (Fd::STDIN, false),
    };
    let st = sysc.fstat(fd).ok();
    let ispipe = st.as_ref().is_some_and(|s| s.mode & S_IFMT == S_IFIFO);
    let bytes_max = ms.params.bytes_max;
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 65536];
    if ispipe {
        loop {
            if buf.len() >= bytes_max {
                break;
            }
            let want = (bytes_max - buf.len()).min(chunk.len());
            match sys::read(fd, &mut chunk[..want]) {
                Ok(0) => break,
                Ok(n) => {
                    buf.extend_from_slice(&chunk[..n]);
                    if n < 4096 {
                        break;
                    }
                }
                Err(Errno::EINTR) => {}
                Err(Errno::EAGAIN) => {
                    sysc.sched_yield();
                }
                Err(_) => break,
            }
        }
        if buf.is_empty()
            && let Some(name) = inname
        {
            let md = st.as_ref().map_or(0, |s| s.mode);
            let _ = unreadable_info(ms, md, Some(name));
            if opened {
                let _ = sys::close(fd);
            }
            return ms.getbuffer();
        }
    } else {
        loop {
            if buf.len() >= bytes_max {
                break;
            }
            let want = (bytes_max - buf.len()).min(chunk.len());
            match sys::read(fd, &mut chunk[..want]) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(Errno::EINTR) => {}
                Err(e) => {
                    let what = match inname {
                        Some(n) => io::lossy(n),
                        None => "/dev/stdin".to_string(),
                    };
                    ms.file_error(e.0, &format!("cannot read `{what}'"));
                    if opened {
                        let _ = sys::close(fd);
                    }
                    return None;
                }
            }
        }
    }
    let (mode, size) = st.as_ref().map_or((0, 0), |s| (s.mode, s.size));
    let b = Buffer::new(&buf, mode, size, Some(fd));
    let r = file_buffer(ms, &b, inname);
    drop(b);
    if opened {
        let _ = sys::close(fd);
    }
    if r == -1 {
        return None;
    }
    ms.getbuffer()
}

struct Opts {
    bflag: u32,
    nulsep: u32,
    nopad: bool,
    nobuffer: bool,
    separator: Vec<u8>,
}

/// `process()`.
fn process(ms: &mut MagicSet, o: &Opts, inname: &[u8], wid: usize, out: &mut Vec<u8>) -> i32 {
    let std_in = inname == b"-";
    let c = if o.nulsep > 1 { 0u8 } else { b'\n' };
    if wid > 0 && o.bflag == 0 {
        let pname: &[u8] = if std_in { b"/dev/stdin" } else { inname };
        if ms.flags & MAGIC_RAW == 0 {
            out.extend_from_slice(&wchar::fname_print(pname));
        } else {
            out.extend_from_slice(pname);
        }
        if o.nulsep > 0 {
            out.push(0);
        }
        if o.nulsep < 2 {
            out.extend_from_slice(&o.separator);
            let w = if o.nopad { 0 } else { wid.saturating_sub(wchar::mbswidth(inname, ms.flags & MAGIC_RAW != 0)) };
            out.extend_from_slice(" ".repeat(w).as_bytes());
            out.push(b' ');
        }
    }
    let ty = file_or_fd(ms, if std_in { None } else { Some(inname) });
    let failed = match ty {
        Some(t) => {
            out.extend_from_slice(&t);
            out.push(c);
            false
        }
        None => {
            out.extend_from_slice(b"ERROR: ");
            out.extend_from_slice(&ms.error_text().unwrap_or_default());
            out.push(c);
            true
        }
    };
    flush(out, o.nobuffer);
    i32::from(failed)
}

fn flush(out: &mut Vec<u8>, now: bool) {
    let mut so = io::stdout();
    let _ = so.write_all(out);
    out.clear();
    if now {
        let _ = io::flush_stdout();
    }
}

fn setparam(ms: &mut MagicSet, p: &str) -> Result<(), String> {
    let Some((name, value)) = p.split_once('=') else { return Err(format!("missing = in {p}")) };
    let v: usize = value.parse().map_err(|_| format!("Invalid parameter value {value}"))?;
    let pm = &mut ms.params;
    match name {
        "bytes" => pm.bytes_max = v,
        "elf_notes" => pm.elf_notes_max = v as u16,
        "elf_phnum" => pm.elf_phnum_max = v as u16,
        "elf_shnum" => pm.elf_shnum_max = v as u16,
        "elf_shsize" => pm.elf_shsize_max = v,
        "encoding" => pm.encoding_max = v,
        "indir" => pm.indir_max = v as u16,
        "name" => pm.name_max = v as u16,
        "regex" => pm.regex_max = v as u16,
        "magwarn" => pm.magwarn_max = v,
        _ => return Err(format!("Unknown param {name}")),
    }
    Ok(())
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn usage() -> i32 {
    io::eprint(USAGE);
    1
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let mut flags: u32 = 0;
    if sys::getenv("POSIXLY_CORRECT").is_some() {
        flags |= MAGIC_SYMLINK;
    }
    let mut o = Opts { bflag: 0, nulsep: 0, nopad: false, nobuffer: false, separator: b":".to_vec() };
    let mut errflg = 0;
    let mut files_from: Vec<Vec<u8>> = Vec::new();
    let mut params: Vec<String> = Vec::new();
    let mut g = Getopt::from_env(&argv[1.min(argv.len())..], "bcCde:Ef:F:hiklLm:nNpP:rsSvzZ0", LONGS);
    while let Some(opt) = g.next_opt() {
        let opt = match opt {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message("file")));
                errflg += 1;
                continue;
            }
        };
        match opt.id {
            OPT_HELP => {
                let _ = io::stdout().write_all(HELP.as_bytes());
                return 0;
            }
            OPT_APPLE => flags |= MAGIC_APPLE,
            OPT_EXTENSIONS => flags |= MAGIC_EXTENSION,
            OPT_MIME_TYPE => flags |= MAGIC_MIME_TYPE,
            OPT_MIME_ENCODING => flags |= MAGIC_MIME_ENCODING,
            OPT_EXCLUDE_QUIET => {
                let a = opt.arg_str();
                if let Some((_, v)) = NV.iter().find(|(n, _)| *n == a) {
                    flags |= v;
                }
            }
            id => match u8::try_from(id).unwrap_or(0) {
                b'0' => o.nulsep += 1,
                b'b' => o.bflag += 1,
                b'c' | b'C' | b'l' | b'd' | b'm' | b'p' | b'S' => {}
                b'E' => flags |= MAGIC_ERROR,
                b'e' => {
                    let a = opt.arg_str();
                    match NV.iter().find(|(n, _)| *n == a) {
                        Some((_, v)) => flags |= v,
                        None => errflg += 1,
                    }
                }
                b'f' => files_from.push(opt.arg.clone().unwrap_or_default()),
                b'F' => o.separator = opt.arg.clone().unwrap_or_default(),
                b'i' => flags |= MAGIC_MIME,
                b'k' => flags |= MAGIC_CONTINUE,
                b'n' => o.nobuffer = true,
                b'N' => o.nopad = true,
                b'P' => params.push(opt.arg_str()),
                b'r' => flags |= MAGIC_RAW,
                b's' => flags |= MAGIC_DEVICES,
                b'v' => {
                    let _ = io::stdout().write_all(b"file-5.46\nmagic file from /etc/magic:/usr/share/misc/magic\n");
                    return 0;
                }
                b'z' => flags |= MAGIC_COMPRESS,
                b'Z' => flags |= MAGIC_COMPRESS | MAGIC_COMPRESS_TRANSP,
                b'L' => flags |= MAGIC_SYMLINK,
                b'h' => flags &= !MAGIC_SYMLINK,
                _ => errflg += 1,
            },
        }
    }
    if errflg > 0 {
        return usage();
    }
    let mut ms = MagicSet::new(flags, vec![builtin_db()]);
    ms.tz = crate::util::time::local_tz();
    for p in &params {
        if let Err(msg) = setparam(&mut ms, p) {
            io::eprint(format!("file: {msg}\n"));
            return 1;
        }
    }
    let operands = g.operands();
    let mut e = 0;
    let mut out = Vec::new();
    for ff in &files_from {
        e |= unwrap(&mut ms, &o, ff, &mut out);
    }
    if operands.is_empty() {
        if files_from.is_empty() {
            return usage();
        }
        flush(&mut out, true);
        return e;
    }
    let raw = ms.flags & MAGIC_RAW != 0;
    let wid = operands.iter().map(|a| wchar::mbswidth(a, raw)).max().unwrap_or(0);
    if o.bflag == 2 {
        o.bflag = u32::from(operands.len() <= 1);
    }
    for a in &operands {
        sys::checkpoint();
        e |= process(&mut ms, &o, a, wid, &mut out);
    }
    flush(&mut out, false);
    e
}

/// `unwrap()`: os nomes de um arquivo (`-f`), um por linha.
fn unwrap(ms: &mut MagicSet, o: &Opts, fname: &[u8], out: &mut Vec<u8>) -> i32 {
    let data = if fname == b"-" {
        sys::read_to_end(Fd::STDIN)
    } else {
        sys::read_file(fname)
    };
    let data = match data {
        Ok(d) => d,
        Err(err) => {
            flush(out, true);
            io::eprint(format!("file: Cannot open `{}' ({})\n", io::lossy(fname), err.message()));
            return 1;
        }
    };
    let lines: Vec<&[u8]> = data.split_inclusive(|&c| c == b'\n').map(|l| l.strip_suffix(b"\n").unwrap_or(l)).collect();
    let raw = ms.flags & MAGIC_RAW != 0;
    let wid = if fname == b"-" { 1 } else { lines.iter().map(|l| wchar::mbswidth(l, raw)).max().unwrap_or(0) };
    let mut e = 0;
    for l in lines {
        e |= process(ms, o, l, wid, out);
    }
    e
}
