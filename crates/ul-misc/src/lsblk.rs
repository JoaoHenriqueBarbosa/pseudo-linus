//! `lsblk` do util-linux 2.41: lista dispositivos de bloco.
//!
//! Porte do `misc-utils/lsblk.c`, no recorte que o sandbox permite. O `sysabi` não lista diretórios
//! nem expõe dispositivos de bloco, então a árvore de dispositivos é sempre vazia: como num contêiner
//! sem privilégio e sem `/sys/dev/block`, o programa falha ao acessar o sysfs, e com o sysfs presente
//! imprime só o cabeçalho. Opções, colunas, `--help` e mensagens de erro seguem o original.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const SYSFS_DEV_BLOCK: &str = "/sys/dev/block";

/// Colunas conhecidas, com a ajuda de cada uma (ordem do `lsblk.c`).
const COLUMNS: &[(&str, &str)] = &[
    ("NAME", "device name"),
    ("KNAME", "internal kernel device name"),
    ("PATH", "path to the device node"),
    ("MAJ:MIN", "major:minor device number"),
    ("FSAVAIL", "filesystem size available"),
    ("FSSIZE", "filesystem size"),
    ("FSTYPE", "filesystem type"),
    ("FSUSED", "filesystem size used"),
    ("FSUSE%", "filesystem use percentage"),
    ("FSROOTS", "mounted filesystem roots"),
    ("FSVER", "filesystem version"),
    ("MOUNTPOINT", "where the device is mounted"),
    ("MOUNTPOINTS", "all locations where device is mounted"),
    ("LABEL", "filesystem LABEL"),
    ("UUID", "filesystem UUID"),
    ("PTUUID", "partition table identifier (usually UUID)"),
    ("PTTYPE", "partition table type"),
    ("PARTTYPE", "partition type code or UUID"),
    ("PARTTYPENAME", "partition type name"),
    ("PARTLABEL", "partition LABEL"),
    ("PARTUUID", "partition UUID"),
    ("PARTFLAGS", "partition flags"),
    ("RA", "read-ahead of the device"),
    ("RO", "read-only device"),
    ("RM", "removable device"),
    ("HOTPLUG", "removable or hotplug device (usb, pcmcia, ...)"),
    ("MODEL", "device identifier"),
    ("SERIAL", "disk serial number"),
    ("SIZE", "size of the device"),
    ("STATE", "state of the device"),
    ("OWNER", "user name"),
    ("GROUP", "group name"),
    ("MODE", "device node permissions"),
    ("ALIGNMENT", "alignment offset"),
    ("MIN-IO", "minimum I/O size"),
    ("OPT-IO", "optimal I/O size"),
    ("PHY-SEC", "physical sector size"),
    ("LOG-SEC", "logical sector size"),
    ("ROTA", "rotational device"),
    ("SCHED", "I/O scheduler name"),
    ("RQ-SIZE", "request queue size"),
    ("TYPE", "device type"),
    ("DISC-ALN", "discard alignment offset"),
    ("DISC-GRAN", "discard granularity"),
    ("DISC-MAX", "discard max bytes"),
    ("DISC-ZERO", "discard zeroes data"),
    ("WSAME", "write same max bytes"),
    ("WWN", "unique storage identifier"),
    ("RAND", "adds randomness"),
    ("PKNAME", "internal parent kernel device name"),
    ("HCTL", "Host:Channel:Target:Lun for SCSI"),
    ("TRAN", "device transport type"),
    ("SUBSYSTEMS", "de-duplicated chain of subsystems"),
    ("REV", "device revision"),
    ("VENDOR", "device vendor"),
    ("ZONED", "zone model"),
    ("DAX", "dax-capable device"),
    ("DISK-SEQ", "disk sequence number"),
    ("ZONE-SZ", "zone size"),
    ("ZONE-WGRAN", "zone write granularity"),
    ("ZONE-APP", "zone append max bytes"),
    ("ZONE-NR", "number of zones"),
    ("ZONE-OMAX", "maximum number of open zones"),
    ("ZONE-AMAX", "maximum number of active zones"),
];

const DEFAULT_COLS: &[&str] = &["NAME", "MAJ:MIN", "RM", "SIZE", "RO", "TYPE", "MOUNTPOINTS"];
const FS_COLS: &[&str] = &[
    "NAME", "FSTYPE", "FSVER", "LABEL", "UUID", "FSAVAIL", "FSUSE%", "MOUNTPOINTS",
];
const PERMS_COLS: &[&str] = &["NAME", "SIZE", "OWNER", "GROUP", "MODE"];
const TOPO_COLS: &[&str] = &[
    "NAME", "ALIGNMENT", "MIN-IO", "OPT-IO", "PHY-SEC", "LOG-SEC", "ROTA", "SCHED", "RQ-SIZE",
    "RA", "WSAME",
];
const DISCARD_COLS: &[&str] = &["NAME", "DISC-ALN", "DISC-GRAN", "DISC-MAX", "DISC-ZERO"];
const SCSI_COLS: &[&str] = &["NAME", "HCTL", "TYPE", "VENDOR", "MODEL", "REV", "SERIAL", "TRAN"];
const NVME_COLS: &[&str] = &["NAME", "TYPE", "TRAN", "MODEL", "SERIAL"];
const VIRTIO_COLS: &[&str] = &["NAME", "TYPE", "TRAN", "SERIAL"];
const ZONED_COLS: &[&str] = &[
    "NAME", "ZONED", "ZONE-SZ", "ZONE-WGRAN", "ZONE-APP", "ZONE-NR", "ZONE-OMAX", "ZONE-AMAX",
];

const LONGS: &[LongOpt] = &[
    LongOpt::new("all", HasArg::No, b'a' as i32),
    LongOpt::new("noempty", HasArg::No, b'A' as i32),
    LongOpt::new("bytes", HasArg::No, b'b' as i32),
    LongOpt::new("nodeps", HasArg::No, b'd' as i32),
    LongOpt::new("discard", HasArg::No, b'D' as i32),
    LongOpt::new("dedup", HasArg::Required, b'E' as i32),
    LongOpt::new("exclude", HasArg::Required, b'e' as i32),
    LongOpt::new("fs", HasArg::No, b'f' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("include", HasArg::Required, b'I' as i32),
    LongOpt::new("json", HasArg::No, b'J' as i32),
    LongOpt::new("tree", HasArg::Optional, b'T' as i32),
    LongOpt::new("list", HasArg::No, b'l' as i32),
    LongOpt::new("merge", HasArg::No, b'M' as i32),
    LongOpt::new("perms", HasArg::No, b'm' as i32),
    LongOpt::new("noheadings", HasArg::No, b'n' as i32),
    LongOpt::new("nvme", HasArg::No, b'N' as i32),
    LongOpt::new("output", HasArg::Required, b'o' as i32),
    LongOpt::new("output-all", HasArg::No, b'O' as i32),
    LongOpt::new("pairs", HasArg::No, b'P' as i32),
    LongOpt::new("filter", HasArg::Required, b'Q' as i32),
    LongOpt::new("paths", HasArg::No, b'p' as i32),
    LongOpt::new("raw", HasArg::No, b'r' as i32),
    LongOpt::new("inverse", HasArg::No, b's' as i32),
    LongOpt::new("ascii", HasArg::No, b'i' as i32),
    LongOpt::new("scsi", HasArg::No, b'S' as i32),
    LongOpt::new("virtio", HasArg::No, b'v' as i32),
    LongOpt::new("sort", HasArg::Required, b'x' as i32),
    LongOpt::new("topology", HasArg::No, b't' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("width", HasArg::Required, b'w' as i32),
    LongOpt::new("shell", HasArg::No, b'y' as i32),
    LongOpt::new("zoned", HasArg::No, b'z' as i32),
    LongOpt::new("sysroot", HasArg::Required, 0x100),
    LongOpt::new("properties-by", HasArg::Required, 0x101),
];

fn usage(_short: &str) -> String {
    r#"
Usage:
 lsblk [options] [<device> ...]

List information about block devices.

Options:
 -A, --noempty        don't print empty devices
 -D, --discard        print discard capabilities
 -E, --dedup <column> de-duplicate output by <column>
 -I, --include <list> show only devices with specified major numbers
 -J, --json           use JSON output format
 -M, --merge          group parents of sub-trees (RAIDs, Multi-path)
 -O, --output-all     output all columns
 -P, --pairs          use key="value" output format
 -Q, --filter <expr>  print only lines matching the expression
     --highlight <expr> colorize lines matching the expression
     --ct-filter <expr> restrict the next counter
     --ct <name>[:<param>[:<func>]] define a custom counter
 -S, --scsi           output info about SCSI devices
 -N, --nvme           output info about NVMe devices
 -v, --virtio         output info about virtio devices
 -T, --tree[=<column>] use tree format output
 -a, --all            print all devices
 -b, --bytes          print SIZE in bytes instead of a human-readable format
 -d, --nodeps         don't print slaves or holders
 -e, --exclude <list> exclude devices by major number (default: RAM disks)
 -f, --fs             output info about filesystems
 -i, --ascii          use ascii characters only
 -l, --list           use list format output
 -m, --perms          output info about permissions
 -n, --noheadings     don't print headings
 -o, --output <list>  output columns (see --list-columns)
 -p, --paths          print complete device path
 -r, --raw            use raw output format
 -s, --inverse        inverse dependencies
 -t, --topology       output info about topology
 -w, --width <num>    specifies output width as number of characters
 -x, --sort <column>  sort output by <column>
 -y, --shell          use column names that can be used as shell variables
 -z, --zoned          print zone related information
     --sysroot <dir>  use specified directory as system root
     --properties-by <list>
                      methods used to gather data (default: file,udev,blkid)

 -H, --list-columns   list the available columns
 -h, --help           display this help
 -V, --version        display version

For more details see lsblk(8).
"#.to_string()
}

fn find_col(name: &str) -> Option<&'static str> {
    COLUMNS
        .iter()
        .map(|c| c.0)
        .find(|c| c.eq_ignore_ascii_case(name))
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut cols: Vec<&'static str> = DEFAULT_COLS.to_vec();
    let mut json = false;
    let mut pairs = false;
    let mut raw = false;
    let mut noheadings = false;
    let mut sysroot = String::new();
    let mut explicit_out: Option<String> = None;

    let mut g = Getopt::from_env(
        &argv[1..],
        "AabDdE:e:fhI:iJlMmNnOo:PpQ:rSstTvVw:x:yz",
        LONGS,
    );
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
            x if x == b'h' as i32 => {
                let mut out = io::stdout();
                let _ = out.write_all(usage(&short).as_bytes());
                return 0;
            }
            x if x == b'V' as i32 => {
                ul::print_version(&short);
                return 0;
            }
            x if x == b'J' as i32 => json = true,
            x if x == b'P' as i32 => pairs = true,
            x if x == b'r' as i32 => raw = true,
            x if x == b'n' as i32 => noheadings = true,
            x if x == b'O' as i32 => cols = COLUMNS.iter().map(|c| c.0).collect(),
            x if x == b'f' as i32 => cols = FS_COLS.to_vec(),
            x if x == b'm' as i32 => cols = PERMS_COLS.to_vec(),
            x if x == b't' as i32 => cols = TOPO_COLS.to_vec(),
            x if x == b'D' as i32 => cols = DISCARD_COLS.to_vec(),
            x if x == b'S' as i32 => cols = SCSI_COLS.to_vec(),
            x if x == b'N' as i32 => cols = NVME_COLS.to_vec(),
            x if x == b'v' as i32 => cols = VIRTIO_COLS.to_vec(),
            x if x == b'z' as i32 => cols = ZONED_COLS.to_vec(),
            x if x == b'o' as i32 => explicit_out = Some(o.arg_str()),
            x if x == b'x' as i32 || x == b'E' as i32 => {
                let name = o.arg_str();
                if find_col(&name).is_none() {
                    ul::warnx(&short, format!("unknown column: {name}"));
                    return 1;
                }
            }
            x if x == b'w' as i32 => {
                if let Err(m) = ul::strtou32_or_err(&o.arg.clone().unwrap_or_default(), "invalid column width argument") {
                    ul::warnx(&short, m);
                    return 1;
                }
            }
            x if x == 0x100 => sysroot = o.arg_str(),
            _ => {}
        }
    }

    if let Some(list) = explicit_out {
        let mut parsed: Vec<&'static str> = Vec::new();
        let mut rest = list.as_str();
        if let Some(r) = rest.strip_prefix('+') {
            parsed = cols.clone();
            rest = r;
        }
        for name in rest.split(',').filter(|n| !n.is_empty()) {
            match find_col(name) {
                Some(c) => parsed.push(c),
                None => {
                    ul::warnx(&short, format!("unknown column: {name}"));
                    return 1;
                }
            }
        }
        cols = parsed;
    }
    if cols.is_empty() {
        ul::warnx(&short, "no output columns specified");
        return 1;
    }

    // O sysfs é a fonte de todos os dispositivos: sem ele, nada a listar.
    let dir = format!("{sysroot}{SYSFS_DEV_BLOCK}");
    if let Err(e) = sys::stat(dir.as_bytes()) {
        ul::warn(&short, format!("failed to access sysfs directory: {dir}"), e);
        return 1;
    }

    let ops = g.operands();
    if !ops.is_empty() {
        let mut failed = 0;
        for d in &ops {
            let is_blk = sys::stat(d).is_ok_and(|st| st.mode & 0o170000 == 0o060000);
            if !is_blk {
                ul::warnx(&short, format!("{}: not a block device", io::lossy(d)));
                failed += 1;
            }
        }
        // Todos falharam: 32; só alguns: 64.
        if failed == ops.len() {
            return 32;
        }
        if failed > 0 {
            return 64;
        }
    }

    let mut out = String::new();
    if json {
        out.push_str("{\n   \"blockdevices\": [\n   ]\n}\n");
    } else if pairs {
        // Sem dispositivos, nada a imprimir.
    } else if !noheadings {
        out.push_str(&cols.join(" "));
        out.push('\n');
    }
    let _ = raw;
    let mut so = io::stdout();
    let _ = so.write_all(out.as_bytes());
    0
}
