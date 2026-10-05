//! `mount` do util-linux 2.41: lista e monta sistemas de arquivos.
//!
//! Sem operandos lista os pontos de montagem de `/proc/self/mounts` (`-t` filtra por tipo). Com
//! operandos valida origem e destino como o libmount e, como a syscall `mount(2)` exige
//! `CAP_SYS_ADMIN`, falha com "permission denied" (root de contêiner sem privilégio) ou "only root
//! can do that" (usuário comum); `-f` pula a syscall e sai com 0.

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt};

const O_SOURCE: i32 = 256;
const O_TARGET: i32 = 257;
const O_TARGET_PREFIX: i32 = 258;
const O_OPTIONS_MODE: i32 = 259;
const O_OPTIONS_SOURCE: i32 = 260;
const O_OPTIONS_SOURCE_FORCE: i32 = 261;
const O_MAKE: i32 = 262;
const O_MAP: i32 = 263;
const O_ONLYONCE: i32 = 264;

const LONGS: &[LongOpt] = &[
    LongOpt::new("all", HasArg::No, b'a' as i32),
    LongOpt::new("bind", HasArg::No, b'B' as i32),
    LongOpt::new("no-canonicalize", HasArg::No, b'c' as i32),
    LongOpt::new("fake", HasArg::No, b'f' as i32),
    LongOpt::new("fork", HasArg::No, b'F' as i32),
    LongOpt::new("fstab", HasArg::Required, b'T' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("internal-only", HasArg::No, b'i' as i32),
    LongOpt::new("show-labels", HasArg::No, b'l' as i32),
    LongOpt::new("label", HasArg::Required, b'L' as i32),
    LongOpt::new("mkdir", HasArg::Optional, b'm' as i32),
    LongOpt::new("move", HasArg::No, b'M' as i32),
    LongOpt::new("no-mtab", HasArg::No, b'n' as i32),
    LongOpt::new("namespace", HasArg::Required, b'N' as i32),
    LongOpt::new("options", HasArg::Required, b'o' as i32),
    LongOpt::new("test-opts", HasArg::Required, b'O' as i32),
    LongOpt::new("read-only", HasArg::No, b'r' as i32),
    LongOpt::new("ro", HasArg::No, b'r' as i32),
    LongOpt::new("rbind", HasArg::No, b'R' as i32),
    LongOpt::new("rw", HasArg::No, b'w' as i32),
    LongOpt::new("read-write", HasArg::No, b'w' as i32),
    LongOpt::new("sloppy", HasArg::No, b's' as i32),
    LongOpt::new("types", HasArg::Required, b't' as i32),
    LongOpt::new("uuid", HasArg::Required, b'U' as i32),
    LongOpt::new("verbose", HasArg::No, b'v' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("source", HasArg::Required, O_SOURCE),
    LongOpt::new("target", HasArg::Required, O_TARGET),
    LongOpt::new("target-prefix", HasArg::Required, O_TARGET_PREFIX),
    LongOpt::new("options-mode", HasArg::Required, O_OPTIONS_MODE),
    LongOpt::new("options-source", HasArg::Required, O_OPTIONS_SOURCE),
    LongOpt::new("options-source-force", HasArg::No, O_OPTIONS_SOURCE_FORCE),
    LongOpt::new("make-shared", HasArg::No, O_MAKE),
    LongOpt::new("map-groups", HasArg::Required, O_MAP),
    LongOpt::new("map-users", HasArg::Required, O_MAP),
    LongOpt::new("onlyonce", HasArg::No, O_ONLYONCE),
    LongOpt::new("make-slave", HasArg::No, O_MAKE),
    LongOpt::new("make-private", HasArg::No, O_MAKE),
    LongOpt::new("make-unbindable", HasArg::No, O_MAKE),
    LongOpt::new("make-rshared", HasArg::No, O_MAKE),
    LongOpt::new("make-rslave", HasArg::No, O_MAKE),
    LongOpt::new("make-rprivate", HasArg::No, O_MAKE),
    LongOpt::new("make-runbindable", HasArg::No, O_MAKE),
];

const USAGE: &str = "
Usage:
 mount [-lhV]
 mount -a [options]
 mount [options] [--source] <source> | [--target] <directory>
 mount [options] <source> <directory>
 mount <operation> <mountpoint> [<target>]

Mount a filesystem.

Options:
 -a, --all               mount all filesystems mentioned in fstab
 -c, --no-canonicalize   don't canonicalize paths
 -f, --fake              dry run; skip the mount(2) syscall
 -F, --fork              fork off for each device (use with -a)
 -T, --fstab <path>      alternative file to /etc/fstab
 -i, --internal-only     don't call the mount.<type> helpers
 -l, --show-labels       show also filesystem labels
     --map-groups <inner>:<outer>:<count>
                         add the specified GID map to an ID-mapped mount
     --map-users <inner>:<outer>:<count>
                         add the specified UID map to an ID-mapped mount
     --map-users /proc/<pid>/ns/user
                         specify the user namespace for an ID-mapped mount
 -m, --mkdir[=<mode>]    alias to '-o X-mount.mkdir[=<mode>]'
 -n, --no-mtab           don't write to /etc/mtab
     --options-mode <mode>
                         what to do with options loaded from fstab
     --options-source <source>
                         mount options source
     --options-source-force
                         force use of options from fstab/mtab
     --onlyonce          check if filesystem is already mounted
 -o, --options <list>    comma-separated list of mount options
 -O, --test-opts <list>  limit the set of filesystems (use with -a)
 -r, --read-only         mount the filesystem read-only (same as -o ro)
 -t, --types <list>      limit the set of filesystem types
     --source <src>      explicitly specifies source (path, label, uuid)
     --target <target>   explicitly specifies mountpoint
     --target-prefix <path>
                         specifies path used for all mountpoints
 -v, --verbose           say what is being done
 -w, --rw, --read-write  mount the filesystem read-write (default)
 -N, --namespace <ns>    perform mount in another namespace

 -h, --help              display this help
 -V, --version           display version

Source:
 -L, --label <label>     synonym for LABEL=<label>
 -U, --uuid <uuid>       synonym for UUID=<uuid>
 LABEL=<label>           specifies device by filesystem label
 UUID=<uuid>             specifies device by filesystem UUID
 PARTLABEL=<label>       specifies device by partition label
 PARTUUID=<uuid>         specifies device by partition UUID
 ID=<id>                 specifies device by udev hardware ID
 <device>                specifies device by path
 <directory>             mountpoint for bind mounts (see --bind/rbind)
 <file>                  regular file for loopdev setup

Operations:
 -B, --bind              mount a subtree somewhere else (same as -o bind)
 -M, --move              move a subtree to some other place
 -R, --rbind             mount a subtree and all submounts somewhere else
 --make-shared           mark a subtree as shared
 --make-slave            mark a subtree as slave
 --make-private          mark a subtree as private
 --make-unbindable       mark a subtree as unbindable
 --make-rshared          recursively mark a whole subtree as shared
 --make-rslave           recursively mark a whole subtree as slave
 --make-rprivate         recursively mark a whole subtree as private
 --make-runbindable      recursively mark a whole subtree as unbindable

For more details see mount(8).
";

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// Desfaz os escapes octais (`\040`) de `/proc/self/mounts`.
fn unescape(f: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < f.len() {
        if f[i] == b'\\'
            && i + 3 < f.len()
            && f[i + 1..i + 4].iter().all(|b| (b'0'..=b'7').contains(b))
        {
            let v = (f[i + 1] - b'0') as u32 * 64 + (f[i + 2] - b'0') as u32 * 8 + (f[i + 3] - b'0') as u32;
            out.push(v as u8);
            i += 4;
        } else {
            out.push(f[i]);
            i += 1;
        }
    }
    out
}

fn list(types: Option<&[u8]>) -> i32 {
    let data = io::read_path(b"/proc/self/mounts").unwrap_or_default();
    let (negate, wanted): (bool, Vec<Vec<u8>>) = match types {
        Some(t) => {
            let neg = t.starts_with(b"no");
            let body = if neg { &t[2..] } else { t };
            (neg, body.split(|b| *b == b',').map(|s| s.to_vec()).collect())
        }
        None => (false, Vec::new()),
    };
    let mut out = io::stdout();
    for line in data.split(|b| *b == b'\n') {
        let f: Vec<&[u8]> = line.split(|b| *b == b' ').collect();
        if f.len() < 4 {
            continue;
        }
        if types.is_some() && wanted.iter().any(|w| w == f[2]) == negate {
            continue;
        }
        let mut l = unescape(f[0]);
        l.extend_from_slice(b" on ");
        l.extend(unescape(f[1]));
        l.extend_from_slice(b" type ");
        l.extend_from_slice(f[2]);
        l.extend_from_slice(b" (");
        l.extend_from_slice(f[3]);
        l.extend_from_slice(b")\n");
        let _ = out.write_all(&l);
    }
    0
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut all = false;
    let mut fake = false;
    let mut types: Option<Vec<u8>> = None;
    let mut fstab: Vec<u8> = b"/etc/fstab".to_vec();
    let mut source: Option<Vec<u8>> = None;
    let mut target: Option<Vec<u8>> = None;
    let mut bind = false;
    let mut op_only = false;
    let mut g = Getopt::from_env(&argv[1..], "aBcfFhilL:mMno:O:rRsU:vVwt:T:N:", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        let arg = o.arg.clone();
        match o.id {
            O_SOURCE => source = arg,
            O_TARGET => target = arg,
            O_MAKE | O_TARGET_PREFIX | O_OPTIONS_MODE | O_OPTIONS_SOURCE | O_OPTIONS_SOURCE_FORCE
            | O_MAP | O_ONLYONCE => {
                if o.id == O_MAKE {
                    op_only = true;
                }
            }
            _ => match o.short() {
                Some('a') => all = true,
                Some('f') => fake = true,
                Some('t') => types = arg,
                Some('T') => fstab = arg.unwrap_or_default(),
                Some('B') | Some('R') | Some('M') => {
                    bind = true;
                    op_only = o.short() == Some('M') || op_only;
                }
                Some('c') | Some('F') | Some('i') | Some('l') | Some('m') | Some('n') | Some('N')
                | Some('o') | Some('O') | Some('r') | Some('s') | Some('v') | Some('w') => {}
                Some('L') => {
                    source = Some([b"LABEL=".as_slice(), &arg.unwrap_or_default()].concat())
                }
                Some('U') => {
                    source = Some([b"UUID=".as_slice(), &arg.unwrap_or_default()].concat())
                }
                Some('V') => {
                    let mut out = io::stdout();
                    let _ = out.write_all(
                        format!("{short} from util-linux 2.41.5 (libmount 2.41.5: selinux, smack, btrfs, verity, namespaces, idmapping, fd-based-mount, statmount, statx, assert, debug)\n")
                            .as_bytes(),
                    );
                    return 0;
                }
                Some('h') => {
                    let mut out = io::stdout();
                    let _ = out.write_all(USAGE.as_bytes());
                    return 0;
                }
                _ => {
                    ul::errtryhelp(&short);
                    return 1;
                }
            },
        }
    }
    let ops = g.operands();

    if ops.is_empty() && source.is_none() && target.is_none() && !all {
        return list(types.as_deref());
    }
    if all {
        if !ops.is_empty() || source.is_some() || target.is_some() {
            ul::warnx(&short, "bad usage");
            ul::errtryhelp(&short);
            return 1;
        }
        if let Err(e) = io::read_path(&fstab) {
            ul::warn(&short, format!("{}", io::lossy(&fstab)), e);
            return 32;
        }
        return 0;
    }
    if ops.len() > 2 {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }

    // Resolve origem e destino: um operando só é destino (ou origem achada no fstab).
    let (src, tgt): (Option<Vec<u8>>, Vec<u8>) = match (ops.len(), &source, &target) {
        (2, None, None) => (Some(ops[0].clone()), ops[1].clone()),
        (1, None, None) => {
            let one = ops[0].clone();
            if op_only {
                (None, one)
            } else {
                let tab = io::read_path(&fstab).unwrap_or_default();
                let found = tab.split(|b| *b == b'\n').find_map(|l| {
                    let f: Vec<&[u8]> = l
                        .split(|b| *b == b' ' || *b == b'\t')
                        .filter(|s| !s.is_empty())
                        .collect();
                    if f.len() >= 2 && !f[0].starts_with(b"#") && (f[0] == one || f[1] == one) {
                        Some((f[0].to_vec(), f[1].to_vec()))
                    } else {
                        None
                    }
                });
                match found {
                    Some((s, t)) => (Some(s), t),
                    None => {
                        ul::warnx(
                            &short,
                            format!(
                                "{}: can't find in {}.",
                                io::lossy(&one),
                                io::lossy(&fstab)
                            ),
                        );
                        return 1;
                    }
                }
            }
        }
        (0, s, t) => match (s, t) {
            (Some(s), Some(t)) => (Some(s.clone()), t.clone()),
            (Some(one), None) | (None, Some(one)) => {
                ul::warnx(
                    &short,
                    format!(
                        "{}: can't find in {}.",
                        io::lossy(one),
                        io::lossy(&fstab)
                    ),
                );
                return 1;
            }
            _ => unreachable!(),
        },
        _ => {
            ul::warnx(&short, "bad usage");
            ul::errtryhelp(&short);
            return 1;
        }
    };

    if let Some(s) = &src
        && s.starts_with(b"/")
        && !bind
        && let Err(_) = sys::stat(s)
    {
        ul::warnx(
            &short,
            format!(
                "{}: special device {} does not exist.",
                io::lossy(s),
                io::lossy(s)
            ),
        );
        return 32;
    }
    if sys::stat(&tgt).is_err() {
        ul::warnx(
            &short,
            format!("{}: mount point does not exist.", io::lossy(&tgt)),
        );
        return 32;
    }
    if fake {
        return 0;
    }
    if sys::current().geteuid() != 0 {
        ul::warnx(&short, "only root can do that");
        return 32;
    }
    ul::warnx(
        &short,
        format!("{}: permission denied.", io::lossy(&tgt)),
    );
    32
}
