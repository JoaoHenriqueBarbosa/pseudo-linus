//! `wipefs` do util-linux 2.41: lista (e apagaria) assinaturas de sistemas de arquivos.
//!
//! Este porte implementa a listagem (formatos tabela, `-p` e `-J`), os filtros `-t`/`-o` e a validação
//! das opções. A sondagem reaproveita a do `blkid`. Apagar assinaturas exige escrita posicionada em
//! dispositivo, que o sandbox não oferece: `-a` e `-o` sem `-n` terminam com erro de escrita.

use std::ffi::OsString;
use std::io::Write;

use sysabi::Ctx;

use crate::blkid;
use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const LONGS: &[LongOpt] = &[
    LongOpt::new("all", HasArg::No, b'a' as i32),
    LongOpt::new("backup", HasArg::Optional, b'b' as i32),
    LongOpt::new("force", HasArg::No, b'f' as i32),
    LongOpt::new("noheadings", HasArg::No, b'i' as i32),
    LongOpt::new("json", HasArg::No, b'J' as i32),
    LongOpt::new("lock", HasArg::Optional, 0x100),
    LongOpt::new("no-act", HasArg::No, b'n' as i32),
    LongOpt::new("offset", HasArg::Required, b'o' as i32),
    LongOpt::new("output", HasArg::Required, b'O' as i32),
    LongOpt::new("parsable", HasArg::No, b'p' as i32),
    LongOpt::new("quiet", HasArg::No, b'q' as i32),
    LongOpt::new("types", HasArg::Required, b't' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
];

const USAGE: &str = "
Usage:
 wipefs [options] <device>

Wipe signatures from a device.

Options:
 -a, --all            wipe all magic strings (BE CAREFUL!)
 -b, --backup[=<dir>] create a signature backup in <dir> or $HOME
 -f, --force          force erasure
 -i, --noheadings     don't print headings
 -J, --json           use JSON output format
 -n, --no-act         do everything except the actual write() call
 -o, --offset <num>   offset to erase, in bytes
 -O, --output <list>  COLUMNS to display (see below)
 -p, --parsable       print out in parsable instead of printable format
 -q, --quiet          suppress output messages
 -t, --types <list>   limit the set of filesystem, RAIDs or partition tables
     --lock[=<mode>] use exclusive device lock (yes, no or nonblock)
 -h, --help           display this help
 -V, --version        display version

Arguments:
 Values for <num> may be followed by a suffix: KiB, MiB,
 GiB, TiB, PiB, EiB, ZiB, or YiB (where the \"iB\" is optional).

Available output columns:
     UUID  partition/filesystem UUID
    LABEL  filesystem LABEL
   LENGTH  magic string length
     TYPE  superblock type
   OFFSET  magic string offset
    USAGE  type description
   DEVICE  block device name

For more details see wipefs(8).
";

const COLUMNS: &[&str] = &["UUID", "LABEL", "LENGTH", "OFFSET", "TYPE", "USAGE", "DEVICE"];

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn parse_offset(s: &str) -> Option<u64> {
    if let Some(h) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut all = false;
    let mut noact = false;
    let mut noheadings = false;
    let mut json = false;
    let mut parsable = false;
    let mut quiet = false;
    let mut offsets: Vec<u64> = Vec::new();
    let mut types: Option<String> = None;
    let mut cols: Vec<String> = ["OFFSET", "UUID", "LABEL", "LENGTH", "TYPE", "DEVICE"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut cols_set = false;

    let mut g = Getopt::from_env(&argv[1..], "ab::fiJnO:o:pqt:Vh", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        let arg = o.arg.clone().map(|a| io::lossy(&a)).unwrap_or_default();
        match o.short() {
            Some('a') => all = true,
            Some('b') | Some('f') => {}
            Some('i') => noheadings = true,
            Some('J') => json = true,
            Some('n') => noact = true,
            Some('p') => parsable = true,
            Some('q') => quiet = true,
            Some('t') => types = Some(arg),
            Some('o') => match parse_offset(&arg) {
                Some(n) => offsets.push(n),
                None => {
                    ul::warnx(&short, format!("invalid offset argument: '{arg}'"));
                    return 1;
                }
            },
            Some('O') => {
                let list: Vec<String> = arg.split(',').map(|s| s.to_uppercase()).collect();
                for c in &list {
                    if !COLUMNS.contains(&c.as_str()) {
                        ul::warnx(&short, format!("unknown column: {c}"));
                        return 1;
                    }
                }
                cols = list;
                cols_set = true;
            }
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            Some('h') => {
                let mut out = io::stdout();
                let _ = out.write_all(USAGE.as_bytes());
                return 0;
            }
            None if o.id == 0x100 => {
                let m = arg.as_str();
                if !m.is_empty() && !matches!(m, "yes" | "no" | "nonblock") {
                    ul::warnx(&short, format!("unsupported lock mode: {m}"));
                    return 1;
                }
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }
    let _ = cols_set;

    let devs = g.operands();
    if devs.is_empty() {
        ul::warnx(&short, "no device specified".to_string());
        ul::errtryhelp(&short);
        return 1;
    }

    let erase = (all || !offsets.is_empty()) && !noact;
    let mut status = 0;
    let mut out = io::stdout();
    for d in &devs {
        let name = io::lossy(d);
        let head = match blkid::read_head(d) {
            Ok(h) if h.is_empty() => {
                // Arquivo sem conteúdo (ou /dev/null): a libblkid não inicializa a sondagem.
                let _ = out.flush();
                ul::warn(&short, format!("error: {name}: probing initialization failed"), sysabi::Errno::EINVAL);
                status = 1;
                continue;
            }
            Ok(h) => h,
            Err(e) => {
                let _ = out.flush();
                ul::warn(&short, format!("error: {name}: probing initialization failed"), e);
                status = 1;
                continue;
            }
        };
        let Some(sig) = blkid::probe(&head) else { continue };
        if let Some(t) = &types {
            let (neg, list) = match t.strip_prefix("no") {
                Some(r) => (true, r),
                None => (false, t.as_str()),
            };
            let hit = list.split(',').any(|x| x == sig.ty);
            if hit == neg {
                continue;
            }
        }
        let off = sig.magic_offset as u64;
        if !offsets.is_empty() && !offsets.contains(&off) {
            continue;
        }
        let cell = |c: &str| -> String {
            match c {
                "OFFSET" => format!("0x{off:x}"),
                "UUID" => sig.uuid.clone().unwrap_or_default(),
                "LABEL" => sig.label.as_deref().map(io::lossy).unwrap_or_default(),
                "LENGTH" => sig.magic.len().to_string(),
                "TYPE" => sig.ty.to_string(),
                "USAGE" => sig.usage.to_string(),
                _ => name.clone(),
            }
        };
        if !(all || !offsets.is_empty()) || noact {
            if json {
                let body: Vec<String> = cols
                    .iter()
                    .map(|c| format!("\"{}\": \"{}\"", c.to_lowercase(), cell(c)))
                    .collect();
                let _ = out.write_all(
                    format!("{{\n   \"signatures\": [\n      {{{}}}\n   ]\n}}\n", body.join(", ")).as_bytes(),
                );
            } else if parsable {
                let line: Vec<String> = cols.iter().map(|c| cell(c)).collect();
                let _ = out.write_all(format!("{}\n", line.join(",")).as_bytes());
            } else {
                if !noheadings {
                    let _ = out.write_all(format!("{}\n", cols.join(" ")).as_bytes());
                }
                let line: Vec<String> = cols.iter().map(|c| cell(c)).collect();
                let _ = out.write_all(format!("{}\n", line.join(" ")).as_bytes());
            }
        }
        if erase {
            let _ = out.flush();
            ul::warnx(&short, format!("{name}: failed to erase {} signature: write not supported", sig.ty));
            status = 1;
        } else if noact && !quiet {
            // -n: nada é escrito; a mensagem de apagamento também não sai.
        }
    }
    let _ = out.flush();
    status
}
