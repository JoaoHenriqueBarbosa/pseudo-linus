//! `swaplabel` do util-linux 2.41: mostra (ou troca) o rótulo e o UUID de uma área de swap.
//!
//! Porte do `disk-utils/swaplabel.c`. A sondagem lê o cabeçalho da página (assinatura
//! `SWAPSPACE2` no fim da primeira página de 4096 bytes, UUID em 0x40c, rótulo em 0x41c). Gravar um
//! rótulo ou UUID novo exige escrita posicionada em dispositivo, que o `sysabi` não oferece: `-L` e
//! `-U` em uma área válida terminam com erro de escrita.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("label", HasArg::Required, b'L' as i32),
    LongOpt::new("uuid", HasArg::Required, b'U' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

const USAGE: &str = "
Usage:
 swaplabel [options] <device>

Display or change the label or UUID of a swap area.

Options:
 -L, --label <label> specify a new label
 -U, --uuid <uuid>   specify a new uuid

 -h, --help          display this help
 -V, --version       display version

For more details see swaplabel(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut change = false;
    let mut g = Getopt::from_env(&argv[1..], "L:U:Vh", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        match o.short() {
            Some('L') | Some('U') => change = true,
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }

    let ops = g.operands();
    if ops.is_empty() {
        ul::warnx(&short, "no device specified");
        ul::errtryhelp(&short);
        return 1;
    }
    let dev = io::lossy(&ops[0]);
    let probe_err = |e: Errno| {
        ul::warn(&short, format!("{dev}: unable to probe device"), e);
        1
    };
    let data = match io::read_path(&ops[0]) {
        Ok(d) => d,
        Err(e) => return probe_err(e),
    };
    if data.is_empty() {
        return probe_err(Errno::EINVAL);
    }
    let valid = data.len() >= 4096
        && (&data[4096 - 10..4096] == b"SWAPSPACE2" || &data[4096 - 10..4096] == b"SWAP-SPACE");
    if !valid {
        ul::warnx(&short, format!("{dev}: not a valid swap partition"));
        return 1;
    }
    if change {
        ul::warn(&short, format!("{dev}: failed to write"), Errno::ENOSYS);
        return 1;
    }
    let mut out = String::new();
    if data.len() >= 0x42c {
        let label = &data[0x41c..0x42c];
        let end = label.iter().position(|b| *b == 0).unwrap_or(16);
        if end > 0 {
            out.push_str(&format!("LABEL: {}\n", io::lossy(&label[..end])));
        }
        let u = &data[0x40c..0x41c];
        if u.iter().any(|b| *b != 0) {
            let h: Vec<String> = u.iter().map(|b| format!("{b:02x}")).collect();
            let h = h.concat();
            out.push_str(&format!(
                "UUID:  {}-{}-{}-{}-{}\n",
                &h[0..8],
                &h[8..12],
                &h[12..16],
                &h[16..20],
                &h[20..32]
            ));
        }
    }
    let mut so = io::stdout();
    let _ = so.write_all(out.as_bytes());
    0
}
