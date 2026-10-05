//! `fsck` do util-linux 2.41: frontal que despacha para os `fsck.<tipo>`.
//!
//! Sem `fstab` com entradas verificáveis nem os verificadores de cada tipo, o que o sandbox exibe
//! é o título, a ajuda, a versão e a falha ao procurar o `fsck.<tipo>` de um dispositivo.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Ctx;

use crate::util::io;
use crate::util::ul;

const USAGE: &str = "
Usage:
 fsck [options] -- [fs-options] [<filesystem> ...]

Check and repair a Linux filesystem.

Options:
 -A         check all filesystems
 -C [<fd>]  display progress bar; file descriptor is for GUIs
 -l         lock the device to guarantee exclusive access
 -M         do not check mounted filesystems
 -N         do not execute, just show what would be done
 -P         check filesystems in parallel, including root
 -R         skip root filesystem; useful only with '-A'
 -r [<fd>]  report statistics for each device checked;
            file descriptor is for GUIs
 -s         serialize the checking operations
 -T         do not show the title on startup
 -t <type>  specify filesystem types to be checked;
            <type> is allowed to be a comma-separated list
 -V         explain what is being done

 -?, --help     display this help
     --version  display version

See the specific fsck.* commands for available fs-options.
For more details see fsck(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);

    let mut notitle = false;
    let mut types_next = false;
    let mut devices: Vec<String> = Vec::new();
    let mut only_operands = false;
    for a in &argv[1..] {
        let s = io::lossy(a);
        if types_next {
            types_next = false;
            continue;
        }
        if only_operands || !s.starts_with('-') || s == "-" {
            devices.push(s);
            continue;
        }
        match s.as_str() {
            "--" => only_operands = true,
            "-?" | "--help" => {
                let _ = io::stdout().write_all(USAGE.as_bytes());
                return 0;
            }
            "--version" => {
                ul::print_version(&short);
                return 0;
            }
            "-t" => types_next = true,
            _ => {
                if s.starts_with('-') && !s.starts_with("--") && s[1..].contains('T') {
                    notitle = true;
                }
            }
        }
    }
    if !notitle {
        ul::print_version(&short);
    }
    let mut status = 0;
    for dev in &devices {
        ul::warnx(&short, "fsck.auto: not found");
        status = 8;
        let _ = dev;
    }
    status
}
