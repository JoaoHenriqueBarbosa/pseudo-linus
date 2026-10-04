//! CLIs de compressão: `gzip` 1.13, `bzip2` 1.0.8, `xz` 5.8.1 (e `lzma`), `lzip` 1.25, `zstd` 1.5.7,
//! com os aliases que o Debian instala (`gunzip`, `zcat`, `bunzip2`, `bzcat`, `unxz`, `xzcat`,
//! `unlzma`, `lzcat`, `unzstd`, `zstdcat`).
//!
//! No Debian, `gunzip` e `zcat` são scripts de shell que tratam `--help` e `--version` no primeiro
//! argumento e fazem `exec gzip -d` (ou `-cd`); as mensagens saem então com o nome `gzip`. Os outros
//! aliases são links pro binário, que muda de comportamento pelo nome com que foi chamado.

use std::ffi::OsString;

use sysabi::Ctx;

use crate::sysutil;

pub mod bzip2;
pub mod common;
pub mod gzip;
pub mod lzip;
pub mod xz;
pub mod xzenc;
pub mod xzlist;

/// O `$0` do script: o caminho como foi chamado, ou `/usr/bin/<nome>` quando veio pelo PATH.
fn script_path(argv0: &[u8], name: &str) -> String {
    if argv0.contains(&b'/') { String::from_utf8_lossy(argv0).into_owned() } else { format!("/usr/bin/{name}") }
}

fn script_out(text: &str) -> i32 {
    match sysabi::sys::write_all(sysabi::Fd::STDOUT, text.as_bytes()) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

const GUNZIP_VERSION: &str = "gunzip (gzip) 1.13
Copyright (C) 2023 Free Software Foundation, Inc.
This is free software.  You may redistribute copies of it under the terms of
the GNU General Public License <https://www.gnu.org/licenses/gpl.html>.
There is NO WARRANTY, to the extent permitted by law.

Written by Paul Eggert.
";

const GUNZIP_USAGE: &str = "Uncompress FILEs (by default, in-place).

Mandatory arguments to long options are mandatory for short options too.

  -c, --stdout      write on standard output, keep original files unchanged
  -f, --force       force overwrite of output file and compress links
  -k, --keep        keep (don't delete) input files
  -l, --list        list compressed file contents
  -n, --no-name     do not save or restore the original name and timestamp
  -N, --name        save or restore the original name and timestamp
  -q, --quiet       suppress all warnings
  -r, --recursive   operate recursively on directories
  -S, --suffix=SUF  use suffix SUF on compressed files
      --synchronous synchronous output (safer if system crashes, but slower)
  -t, --test        test compressed file integrity
  -v, --verbose     verbose mode
      --help        display this help and exit
      --version     display version information and exit

With no FILE, or when FILE is -, read standard input.

Report bugs to <bug-gzip@gnu.org>.
";

const ZCAT_VERSION: &str = "zcat (gzip) 1.13
Copyright (C) 2023 Free Software Foundation, Inc.
This is free software.  You may redistribute copies of it under the terms of
the GNU General Public License <https://www.gnu.org/licenses/gpl.html>.
There is NO WARRANTY, to the extent permitted by law.

Written by Paul Eggert.
";

const ZCAT_USAGE: &str = "Uncompress FILEs to standard output.

  -f, --force       force; read compressed data even from a terminal
  -l, --list        list compressed file contents
  -q, --quiet       suppress all warnings
  -r, --recursive   operate recursively on directories
  -S, --suffix=SUF  use suffix SUF on compressed files
      --synchronous synchronous output (safer if system crashes, but slower)
  -t, --test        test compressed file integrity
  -v, --verbose     verbose mode
      --help        display this help and exit
      --version     display version information and exit

With no FILE, or when FILE is -, read standard input.

Report bugs to <bug-gzip@gnu.org>.
";

/// O que os scripts `gunzip`/`zcat` fazem: `--help`/`--version` no primeiro argumento, ou
/// `exec gzip <flag> "$@"`.
fn gzip_script(args: &[OsString], name: &str, flag: &str, usage: &str, version: &str) -> i32 {
    let argv = sysutil::args_bytes(args);
    match argv.get(1).map(Vec::as_slice) {
        Some(b"--help") => {
            let p = script_path(&argv[0], name);
            return script_out(&format!("Usage: {p} [OPTION]... [FILE]...\n{usage}"));
        }
        Some(b"--version") => return script_out(version),
        _ => {}
    }
    let mut v = vec![b"gzip".to_vec(), flag.as_bytes().to_vec()];
    v.extend(argv.into_iter().skip(1));
    gzip::main(&v)
}

pub fn gzip_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    gzip::main(&sysutil::args_bytes(args))
}

pub fn gunzip_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    gzip_script(args, "gunzip", "-d", GUNZIP_USAGE, GUNZIP_VERSION)
}

pub fn zcat_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    gzip_script(args, "zcat", "-cd", ZCAT_USAGE, ZCAT_VERSION)
}

fn pending(args: &[OsString]) -> i32 {
    let argv = sysutil::args_bytes(args);
    sysutil::error(&sysutil::argv0(&argv), "em construção");
    2
}

pub fn bzip2_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    bzip2::main(&sysutil::args_bytes(args))
}

/// `bunzip2` e `bzcat` são links pro `bzip2`, que escolhe o modo pelo nome.
pub fn bunzip2_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    bzip2::main(&sysutil::args_bytes(args))
}

pub fn bzcat_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    bzip2::main(&sysutil::args_bytes(args))
}

/// `xz` e os nomes ligados nele: o modo e o formato vêm do argv[0].
pub fn xz_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    xz::main(&sysutil::args_bytes(args))
}

pub fn unxz_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    xz::main(&sysutil::args_bytes(args))
}

pub fn xzcat_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    xz::main(&sysutil::args_bytes(args))
}

pub fn lzma_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    xz::main(&sysutil::args_bytes(args))
}

pub fn unlzma_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    xz::main(&sysutil::args_bytes(args))
}

pub fn lzcat_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    xz::main(&sysutil::args_bytes(args))
}

pub fn zstd_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    pending(args)
}

pub fn unzstd_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    pending(args)
}

pub fn zstdcat_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    pending(args)
}

pub fn lzip_main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    lzip::main(&sysutil::args_bytes(args))
}
