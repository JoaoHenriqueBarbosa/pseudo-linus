//! `mkfs` do util-linux 2.41: frontal que despacha para os `mkfs.<tipo>`.
//!
//! O oráculo (Debian 13) não traz nenhum `mkfs.<tipo>`, então o despacho falha como o `execvp` do
//! original quando o construtor não existe no sistema.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Ctx;

use crate::util::io;
use crate::util::ul;

const USAGE: &str = "
Usage:
 mkfs [options] [-t <type>] [fs-options] <device> [<size>]

Make a Linux filesystem.

Options:
 -t, --type=<type>  filesystem type; when unspecified, ext2 is used
     fs-options     parameters for the real filesystem builder
     <device>       path to the device to be used
     <size>         number of blocks to be used on the device
 -V, --verbose      explain what is being done;
                      specifying -V more than once will cause a dry-run
 -h, --help         display this help
 -V, --version      display version

For more details see mkfs(8).
";

pub fn main(ctx: &mut Ctx, args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);

    let mut fstype = String::from("ext2");
    let mut rest: Vec<OsString> = Vec::new();
    let mut i = 1;
    let mut opts_done = false;
    while i < argv.len() {
        let s = io::lossy(&argv[i]);
        if opts_done || !s.starts_with('-') || s == "-" {
            opts_done = true;
            rest.push(args[i].clone());
            i += 1;
            continue;
        }
        match s.as_str() {
            "-h" | "--help" => {
                let _ = io::stdout().write_all(USAGE.as_bytes());
                return 0;
            }
            "--version" => {
                ul::print_version(&short);
                return 0;
            }
            "-V" | "--verbose" => {}
            "-t" | "--type" => {
                i += 1;
                if i >= argv.len() {
                    ul::warnx(&short, format!("option requires an argument -- 't'"));
                    ul::errtryhelp(&short);
                    return 1;
                }
                fstype = io::lossy(&argv[i]);
            }
            _ => {
                if let Some(t) = s.strip_prefix("--type=") {
                    fstype = t.to_string();
                } else if let Some(t) = s.strip_prefix("-t") {
                    fstype = t.to_string();
                } else {
                    rest.push(args[i].clone());
                }
            }
        }
        i += 1;
    }
    if rest.is_empty() {
        io::eprint(USAGE.to_string());
        return 1;
    }
    let prog = format!("mkfs.{fstype}");
    let mut sub: Vec<OsString> = vec![OsString::from(&prog)];
    sub.extend(rest);
    // O oráculo não tem nenhum `mkfs.<tipo>`: o `execvp` do original falha sempre.
    let _ = (ctx, sub);
    ul::warnx(&short, format!("failed to execute {prog}: No such file or directory"));
    1
}
