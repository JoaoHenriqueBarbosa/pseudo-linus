//! `findmnt` do util-linux 2.41 (pacote util-linux do Debian 13): lista e procura sistemas de
//! arquivos montados.
//!
//! Porte enxuto do `misc-utils/findmnt.c`. A tabela vem de `/proc/self/mountinfo` (padrão, `-k`),
//! de `/proc/<tid>/mountinfo` (`-N`), do `/etc/fstab` (`-s`) ou do `/etc/mtab` (`-m`), ambos também
//! com `-F`. Saídas: árvore (padrão), lista (`-l`), raw (`-r`), pairs (`-P`) e JSON (`-J`).
//!
//! Fora do porte: colunas que dependem de `statfs` e `blkid` (SIZE, AVAIL, USED, USE%, LABEL, UUID,
//! PARTLABEL, PARTUUID saem vazias), `-D`, `-p`/`-w`, `-x`, resolução de tags em `-S`/`-e`, e os
//! caracteres de árvore UTF-8 (só ASCII: `|-` e `` `- ``).

use std::ffi::OsString;
use std::io::Write;

use sysabi::sys;

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt, display_width};

const OPT_OUTPUT_ALL: i32 = 256;
const OPT_PSEUDO: i32 = 257;
const OPT_REAL: i32 = 258;
const OPT_TREE: i32 = 259;
const OPT_VERBOSE: i32 = 260;
const OPT_SHADOWED: i32 = 261;

const LONGS: &[LongOpt] = &[
    LongOpt::new("all", HasArg::No, b'A' as i32),
    LongOpt::new("ascii", HasArg::No, b'a' as i32),
    LongOpt::new("bytes", HasArg::No, b'b' as i32),
    LongOpt::new("canonicalize", HasArg::No, b'c' as i32),
    LongOpt::new("nocanonicalize", HasArg::No, b'C' as i32),
    LongOpt::new("df", HasArg::No, b'D' as i32),
    LongOpt::new("direction", HasArg::Required, b'd' as i32),
    LongOpt::new("evaluate", HasArg::No, b'e' as i32),
    LongOpt::new("tab-file", HasArg::Required, b'F' as i32),
    LongOpt::new("first-only", HasArg::No, b'f' as i32),
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("invert", HasArg::No, b'i' as i32),
    LongOpt::new("json", HasArg::No, b'J' as i32),
    LongOpt::new("kernel", HasArg::No, b'k' as i32),
    LongOpt::new("list", HasArg::No, b'l' as i32),
    LongOpt::new("mountpoint", HasArg::Required, b'M' as i32),
    LongOpt::new("mtab", HasArg::No, b'm' as i32),
    LongOpt::new("task", HasArg::Required, b'N' as i32),
    LongOpt::new("noheadings", HasArg::No, b'n' as i32),
    LongOpt::new("options", HasArg::Required, b'O' as i32),
    LongOpt::new("output", HasArg::Required, b'o' as i32),
    LongOpt::new("output-all", HasArg::No, OPT_OUTPUT_ALL),
    LongOpt::new("pairs", HasArg::No, b'P' as i32),
    LongOpt::new("poll", HasArg::Optional, b'p' as i32),
    LongOpt::new("pseudo", HasArg::No, OPT_PSEUDO),
    LongOpt::new("raw", HasArg::No, b'r' as i32),
    LongOpt::new("real", HasArg::No, OPT_REAL),
    LongOpt::new("shadowed", HasArg::No, OPT_SHADOWED),
    LongOpt::new("fstab", HasArg::No, b's' as i32),
    LongOpt::new("source", HasArg::Required, b'S' as i32),
    LongOpt::new("submounts", HasArg::No, b'R' as i32),
    LongOpt::new("target", HasArg::Required, b'T' as i32),
    LongOpt::new("timeout", HasArg::Required, b'w' as i32),
    LongOpt::new("tree", HasArg::No, OPT_TREE),
    LongOpt::new("types", HasArg::Required, b't' as i32),
    LongOpt::new("uniq", HasArg::No, b'U' as i32),
    LongOpt::new("notruncate", HasArg::No, b'u' as i32),
    LongOpt::new("nofsroot", HasArg::No, b'v' as i32),
    LongOpt::new("verbose", HasArg::No, OPT_VERBOSE),
    LongOpt::new("verify", HasArg::No, b'x' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
];

/// Colunas na ordem do `--help`: nome, descrição, numérica no JSON.
const COLUMNS: &[(&str, &str, bool)] = &[
    ("SOURCE", "source device", false),
    ("TARGET", "mountpoint", false),
    ("FSTYPE", "filesystem type", false),
    ("OPTIONS", "all mount options", false),
    ("VFS-OPTIONS", "VFS specific mount options", false),
    ("FS-OPTIONS", "FS specific mount options", false),
    ("LABEL", "filesystem label", false),
    ("UUID", "filesystem UUID", false),
    ("PARTLABEL", "partition label", false),
    ("PARTUUID", "partition UUID", false),
    ("MAJ:MIN", "major:minor device number", false),
    ("ACTION", "action detected by --poll", false),
    ("OLD-TARGET", "old mountpoint saved by --poll", false),
    ("OLD-OPTIONS", "old mount options saved by --poll", false),
    ("SIZE", "filesystem size", false),
    ("AVAIL", "filesystem size available", false),
    ("USED", "filesystem size used", false),
    ("USE%", "filesystem use percentage", false),
    ("FSROOT", "filesystem root", false),
    ("TID", "task ID", true),
    ("ID", "mount ID", true),
    ("OPT-FIELDS", "optional mount fields", false),
    ("PROPAGATION", "VFS propagation flags", false),
    ("FREQ", "dump(8) period in days [fstab only]", true),
    ("PASSNO", "pass number on parallel fsck(8) [fstab only]", true),
];

const DEFAULT_COLUMNS: &[&str] = &["TARGET", "SOURCE", "FSTYPE", "OPTIONS"];

const PSEUDOFS: &[&str] = &[
    "anon_inodefs",
    "autofs",
    "bdev",
    "binfmt_misc",
    "cgroup",
    "cgroup2",
    "configfs",
    "cpuset",
    "debugfs",
    "devfs",
    "devpts",
    "devtmpfs",
    "dlmfs",
    "efivarfs",
    "fuse.gvfs-fuse-daemon",
    "fusectl",
    "hugetlbfs",
    "mqueue",
    "nfsd",
    "none",
    "nsfs",
    "overlay",
    "pipefs",
    "proc",
    "pstore",
    "ramfs",
    "resctrl",
    "rootfs",
    "rpc_pipefs",
    "securityfs",
    "selinuxfs",
    "sockfs",
    "spufs",
    "sysfs",
    "tmpfs",
    "tracefs",
];

fn usage(short: &str) -> String {
    let mut s = format!(
        "
Usage:
 {short} [options]
 {short} [options] <device> | <mountpoint>
 {short} [options] <device> <mountpoint>
 {short} [options] [--source <device>] [--target <path> | --mountpoint <dir>]

Find a (mounted) filesystem.

Options:
 -s, --fstab            search in static table of filesystems
 -m, --mtab             search in table of mounted filesystems (includes user space mount options)
 -k, --kernel           search in kernel table of mounted filesystems (default)

 -p, --poll[=<list>]    monitor changes in table of mounted filesystems
 -w, --timeout <num>    upper limit in milliseconds that --poll will block

 -A, --all              disable all built-in filters, print all filesystems
 -a, --ascii            use ASCII chars for tree formatting
 -b, --bytes            print sizes in bytes rather than in human readable format
 -C, --nocanonicalize   don't canonicalize when comparing paths
 -c, --canonicalize     canonicalize printed paths
 -D, --df               imitate the output of df(1)
 -d, --direction <word> direction of search, 'forward' or 'backward'
 -e, --evaluate         convert tags (LABEL,UUID,PARTUUID,PARTLABEL) to device names
 -F, --tab-file <path>  alternative file for -s, -m or -k options
 -f, --first-only       print the first found filesystem only
 -i, --invert           invert the sense of matching
 -J, --json             use JSON output format
 -l, --list             use list format output
 -N, --task <tid>       use alternative namespace (/proc/<tid>/mountinfo file)
 -n, --noheadings       don't print column headings
 -O, --options <list>   limit the set of filesystems by mount options
 -o, --output <list>    the output columns to be shown
     --output-all       output all available columns
 -P, --pairs            use key=\"value\" output format
     --pseudo           print only pseudo-filesystems
 -R, --submounts        print all submounts for the selected filesystems
     --real             print only real filesystems
 -r, --raw              use raw output format
 -S, --source <string>  the device to mount (by name, maj:min, LABEL=, UUID=, PARTUUID=, PARTLABEL=)
     --shadowed         print only filesystems over-mounted by another filesystem
 -t, --types <list>     limit the set of filesystems by FS types
 -T, --target <path>    the path to the filesystem to use
     --tree             enable tree format output is possible
 -M, --mountpoint <dir> the mountpoint directory
 -U, --uniq             ignore filesystems with duplicate target
 -u, --notruncate       don't truncate text in columns
 -v, --nofsroot         don't print [/dir] for bind or btrfs mounts
 -x, --verify           verify mount table content (default is fstab)
     --verbose          print more details

 -h, --help             display this help
 -V, --version          display version

Available output columns:
"
    );
    for (name, help, _) in COLUMNS {
        s.push_str(&format!(" {name:>11}  {help}\n"));
    }
    s.push_str("\nFor more details see findmnt(8).\n");
    s
}

/// Uma linha da tabela de montagens (mountinfo, fstab ou mtab).
#[derive(Clone, Default)]
struct Fs {
    id: Option<u64>,
    parent: Option<u64>,
    maj_min: String,
    root: String,
    target: String,
    vfs: String,
    fsopts: String,
    options: String,
    optfields: Vec<String>,
    fstype: String,
    source: String,
    freq: Option<u64>,
    passno: Option<u64>,
}

/// Desfaz o escape octal (`\040`) do kernel.
fn unmangle(s: &[u8]) -> String {
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
    String::from_utf8_lossy(&out).into_owned()
}

/// Junta as opções de VFS e as do sistema de arquivos como o libmount (o `rw`/`ro` repetido cai).
fn merge_options(vfs: &str, fsopts: &str) -> String {
    let mut rest = fsopts;
    if !vfs.is_empty() {
        let first = rest.split(',').next().unwrap_or("");
        if first == "rw" || first == "ro" {
            rest = rest.get(first.len() + 1..).unwrap_or("");
        }
    }
    match (vfs.is_empty(), rest.is_empty()) {
        (_, true) => vfs.to_string(),
        (true, false) => rest.to_string(),
        (false, false) => format!("{vfs},{rest}"),
    }
}

fn parse_mountinfo(data: &[u8]) -> Vec<Fs> {
    let mut out = Vec::new();
    for line in data.split(|b| *b == b'\n') {
        let f: Vec<&[u8]> = line
            .split(|b| *b == b' ')
            .filter(|x| !x.is_empty())
            .collect();
        if f.len() < 10 {
            continue;
        }
        let num = |s: &[u8]| std::str::from_utf8(s).ok().and_then(|t| t.parse::<u64>().ok());
        let Some(sep) = f.iter().position(|x| *x == b"-") else {
            continue;
        };
        if sep < 6 || f.len() < sep + 4 {
            continue;
        }
        let vfs = String::from_utf8_lossy(f[5]).into_owned();
        let fsopts = String::from_utf8_lossy(f[sep + 3]).into_owned();
        out.push(Fs {
            id: num(f[0]),
            parent: num(f[1]),
            maj_min: String::from_utf8_lossy(f[2]).into_owned(),
            root: unmangle(f[3]),
            target: unmangle(f[4]),
            options: merge_options(&vfs, &fsopts),
            vfs,
            fsopts,
            optfields: f[6..sep]
                .iter()
                .map(|x| String::from_utf8_lossy(x).into_owned())
                .collect(),
            fstype: String::from_utf8_lossy(f[sep + 1]).into_owned(),
            source: unmangle(f[sep + 2]),
            freq: None,
            passno: None,
        });
    }
    out
}

fn parse_fstab(data: &[u8]) -> Vec<Fs> {
    let mut out = Vec::new();
    for line in data.split(|b| *b == b'\n') {
        let f: Vec<&[u8]> = line
            .split(|b| b" \t".contains(b))
            .filter(|x| !x.is_empty())
            .collect();
        if f.is_empty() || f[0][0] == b'#' || f.len() < 3 {
            continue;
        }
        let num = |i: usize| {
            f.get(i)
                .and_then(|s| std::str::from_utf8(s).ok())
                .and_then(|t| t.parse::<u64>().ok())
        };
        out.push(Fs {
            source: unmangle(f[0]),
            target: unmangle(f[1]),
            fstype: unmangle(f[2]),
            options: f.get(3).map(|s| unmangle(s)).unwrap_or_default(),
            freq: num(4),
            passno: num(5),
            ..Fs::default()
        });
    }
    out
}

/// `streq_paths` da libmount: iguais ignorando barras finais.
fn streq_paths(a: &str, b: &str) -> bool {
    fn trim(p: &str) -> &str {
        let mut end = p.len();
        while end > 1 && p.as_bytes()[end - 1] == b'/' {
            end -= 1;
        }
        &p[..end]
    }
    trim(a) == trim(b)
}

/// Caminho absoluto sem `.`, `..` e barras repetidas (sem resolver symlinks).
fn normalize(path: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !path.starts_with('/') {
        if let Ok(cwd) = sys::current().getcwd() {
            for p in String::from_utf8_lossy(&cwd).split('/') {
                if !p.is_empty() {
                    parts.push(p.to_string());
                }
            }
        }
    }
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(p.to_string()),
        }
    }
    if parts.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", parts.join("/"))
    }
}

/// `mnt_match_fstype`: lista separada por vírgula; o prefixo `no` na lista inteira inverte.
fn match_fstype(fstype: &str, list: &str) -> bool {
    let (inv, body) = match list.strip_prefix("no") {
        Some(rest) => (true, rest),
        None => (false, list),
    };
    let found = body
        .split(',')
        .any(|t| t == fstype || (inv && t.strip_prefix("no") == Some(fstype)));
    found != inv
}

/// `mnt_match_options`: cada item precisa estar presente; `noX` exige a ausência de `X`.
fn match_options(opts: &str, list: &str) -> bool {
    let have: Vec<&str> = opts.split(',').collect();
    let has = |want: &str| {
        have.iter().any(|h| {
            *h == want || (!want.contains('=') && h.split('=').next() == Some(want))
        })
    };
    list.split(',').filter(|x| !x.is_empty()).all(|item| {
        if let Some(neg) = item.strip_prefix("no") {
            if !neg.is_empty() && !has(item) {
                return !has(neg);
            }
        }
        has(item)
    })
}

fn is_pseudo(fs: &Fs) -> bool {
    PSEUDOFS.contains(&fs.fstype.as_str())
}

fn column_index(name: &str) -> Option<usize> {
    COLUMNS
        .iter()
        .position(|(n, _, _)| n.eq_ignore_ascii_case(name))
}

fn column_value(col: &str, fs: &Fs, nofsroot: bool) -> String {
    let num = |v: Option<u64>| v.map(|n| n.to_string()).unwrap_or_default();
    match col {
        "SOURCE" => {
            if !nofsroot && fs.id.is_some() && !fs.root.is_empty() && fs.root != "/" && fs.source.starts_with('/')
            {
                format!("{}[{}]", fs.source, fs.root)
            } else {
                fs.source.clone()
            }
        }
        "TARGET" => fs.target.clone(),
        "FSTYPE" => fs.fstype.clone(),
        "OPTIONS" => fs.options.clone(),
        "VFS-OPTIONS" => fs.vfs.clone(),
        "FS-OPTIONS" => fs.fsopts.clone(),
        "MAJ:MIN" => fs.maj_min.clone(),
        "FSROOT" => fs.root.clone(),
        "ID" => num(fs.id),
        "TID" => String::new(),
        "OPT-FIELDS" => fs.optfields.join(","),
        "PROPAGATION" => {
            if fs.id.is_none() {
                return String::new();
            }
            let mut v: Vec<&str> = Vec::new();
            for f in &fs.optfields {
                if f.starts_with("shared:") {
                    v.push("shared");
                } else if f.starts_with("master:") {
                    v.push("slave");
                } else if f == "unbindable" {
                    v.push("unbindable");
                }
            }
            if v.is_empty() {
                "private".to_string()
            } else {
                v.join(",")
            }
        }
        "FREQ" => num(fs.freq),
        "PASSNO" => num(fs.passno),
        _ => String::new(),
    }
}

/// Escape do modo raw: espaço, controle e barra invertida viram `\xNN`.
fn escape_raw(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if c == ' ' || c == '\\' || (c as u32) < 0x20 || c as u32 == 0x7f {
            o.push_str(&format!("\\x{:02x}", c as u32));
        } else {
            o.push(c);
        }
    }
    o
}

/// Escape do modo pairs: aspas, barra invertida e controle viram `\xNN`.
fn escape_pairs(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if c == '"' || c == '\\' || (c as u32) < 0x20 || c as u32 == 0x7f {
            o.push_str(&format!("\\x{:02x}", c as u32));
        } else {
            o.push(c);
        }
    }
    o
}

fn json_escape(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\t' => o.push_str("\\t"),
            '\r' => o.push_str("\\r"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

#[derive(PartialEq, Clone, Copy)]
enum Mode {
    Tree,
    List,
    Raw,
    Pairs,
    Json,
}

struct Out<'a> {
    table: &'a [Fs],
    cols: &'a [String],
    nofsroot: bool,
}

impl Out<'_> {
    fn json_node(&self, idx: usize, children: &[Vec<usize>], depth: usize, tree: bool, buf: &mut String) {
        let brace = " ".repeat(6 + 6 * depth);
        let key = " ".repeat(9 + 6 * depth);
        let fs = &self.table[idx];
        buf.push_str(&format!("{brace}{{\n"));
        let has_children = tree && !children[idx].is_empty();
        for (i, c) in self.cols.iter().enumerate() {
            let v = column_value(c, fs, self.nofsroot);
            let numeric = column_index(c).map(|k| COLUMNS[k].2).unwrap_or(false);
            let lit = if v.is_empty() {
                "null".to_string()
            } else if numeric {
                v
            } else {
                format!("\"{}\"", json_escape(&v))
            };
            let comma = if i + 1 < self.cols.len() || has_children { "," } else { "" };
            buf.push_str(&format!("{key}\"{}\": {lit}{comma}\n", c.to_lowercase()));
        }
        if has_children {
            buf.push_str(&format!("{key}\"children\": [\n"));
            let n = children[idx].len();
            for (k, ch) in children[idx].iter().enumerate() {
                self.json_node(*ch, children, depth + 1, tree, buf);
                if k + 1 < n {
                    buf.pop();
                    buf.push_str(",\n");
                }
            }
            buf.push_str(&format!("{key}]\n"));
        }
        buf.push_str(&format!("{brace}}}\n"));
    }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut mode_set: Option<Mode> = None;
    let mut tree_flag = false;
    let mut noheadings = false;
    let mut nofsroot = false;
    let mut invert = false;
    let mut first_only = false;
    let mut uniq = false;
    let mut submounts = false;
    let mut pseudo = false;
    let mut real = false;
    let mut shadowed = false;
    let mut fstab_mode = false;
    let mut mtab_mode = false;
    let mut tab_file: Option<Vec<u8>> = None;
    let mut task: Option<String> = None;
    let mut direction: Option<bool> = None; // Some(true) = backward
    let mut types: Option<String> = None;
    let mut opts_filter: Option<String> = None;
    let mut source: Option<String> = None;
    let mut target: Option<String> = None;
    let mut mountpoint: Option<String> = None;
    let mut output: Vec<String> = Vec::new();
    let mut output_all = false;

    let mut g = Getopt::from_env(
        &argv[1..],
        "AabCcDd:ehiJkF:fN:nO:o:pPRrsS:T:M:t:UuvVw:x",
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
        let arg = o.arg_str();
        match o.id {
            OPT_OUTPUT_ALL => output_all = true,
            OPT_PSEUDO => pseudo = true,
            OPT_REAL => real = true,
            OPT_SHADOWED => shadowed = true,
            OPT_TREE => tree_flag = true,
            OPT_VERBOSE => {}
            id if id == b'A' as i32 || id == b'a' as i32 || id == b'b' as i32 => {}
            id if id == b'c' as i32 || id == b'C' as i32 || id == b'e' as i32 => {}
            id if id == b'u' as i32 => {}
            id if id == b'D' as i32 => {
                ul::warnx(&short, "--df is not supported by this build");
                return 1;
            }
            id if id == b'p' as i32 || id == b'w' as i32 => {
                ul::warnx(&short, "--poll is not supported by this build");
                return 1;
            }
            id if id == b'x' as i32 => {
                ul::warnx(&short, "--verify is not supported by this build");
                return 1;
            }
            id if id == b'd' as i32 => match arg.as_str() {
                "forward" => direction = Some(false),
                "backward" => direction = Some(true),
                _ => {
                    ul::warnx(&short, format!("unknown direction '{arg}'"));
                    ul::errtryhelp(&short);
                    return 1;
                }
            },
            id if id == b'F' as i32 => tab_file = o.arg.clone(),
            id if id == b'f' as i32 => first_only = true,
            id if id == b'i' as i32 => invert = true,
            id if id == b'J' as i32 => mode_set = Some(Mode::Json),
            id if id == b'k' as i32 => {
                fstab_mode = false;
                mtab_mode = false;
            }
            id if id == b'l' as i32 => mode_set = Some(Mode::List),
            id if id == b'M' as i32 => mountpoint = Some(arg),
            id if id == b'm' as i32 => {
                mtab_mode = true;
                fstab_mode = false;
            }
            id if id == b'N' as i32 => task = Some(arg),
            id if id == b'n' as i32 => noheadings = true,
            id if id == b'O' as i32 => opts_filter = Some(arg),
            id if id == b'o' as i32 => {
                let mut list = arg.as_str();
                if let Some(rest) = list.strip_prefix('+') {
                    for d in DEFAULT_COLUMNS {
                        output.push((*d).to_string());
                    }
                    list = rest;
                }
                for name in list.split(',').filter(|x| !x.is_empty()) {
                    match column_index(name) {
                        Some(k) => output.push(COLUMNS[k].0.to_string()),
                        None => {
                            ul::warnx(&short, format!("unknown column: {name}"));
                            return 1;
                        }
                    }
                }
            }
            id if id == b'P' as i32 => mode_set = Some(Mode::Pairs),
            id if id == b'r' as i32 => mode_set = Some(Mode::Raw),
            id if id == b'R' as i32 => submounts = true,
            id if id == b's' as i32 => {
                fstab_mode = true;
                mtab_mode = false;
            }
            id if id == b'S' as i32 => source = Some(arg),
            id if id == b'T' as i32 => target = Some(arg),
            id if id == b't' as i32 => types = Some(arg),
            id if id == b'U' as i32 => uniq = true,
            id if id == b'v' as i32 => nofsroot = true,
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

    let operands: Vec<String> = g
        .operands()
        .iter()
        .map(|x| String::from_utf8_lossy(x).into_owned())
        .collect();
    if !operands.is_empty() && (source.is_some() || target.is_some() || mountpoint.is_some()) {
        ul::warnx(
            &short,
            "options --target and --source can't be used together with command line element that is not an option",
        );
        ul::errtryhelp(&short);
        return 1;
    }
    let mut swap_arg: Option<String> = None;
    match operands.len() {
        0 => {}
        1 => swap_arg = Some(operands[0].clone()),
        2 => {
            source = Some(operands[0].clone());
            target = Some(operands[1].clone());
        }
        _ => {
            ul::warnx(&short, "too many arguments");
            ul::errtryhelp(&short);
            return 1;
        }
    }
    if output_all {
        output = COLUMNS.iter().map(|(n, _, _)| (*n).to_string()).collect();
    }
    if fstab_mode || mtab_mode {
        if output.is_empty() {
            output = DEFAULT_COLUMNS.iter().map(|s| (*s).to_string()).collect();
        }
    } else if output.is_empty() {
        output = DEFAULT_COLUMNS.iter().map(|s| (*s).to_string()).collect();
    }

    // Tabela de entrada.
    let table: Vec<Fs> = if fstab_mode || mtab_mode {
        let path: Vec<u8> = tab_file.clone().unwrap_or_else(|| {
            if fstab_mode {
                b"/etc/fstab".to_vec()
            } else {
                b"/etc/mtab".to_vec()
            }
        });
        match sys::read_file(&path) {
            Ok(d) => parse_fstab(&d),
            Err(e) => {
                ul::warn(&short, io::lossy(&path), e);
                return 1;
            }
        }
    } else {
        let path: Vec<u8> = match (&tab_file, &task) {
            (Some(f), _) => f.clone(),
            (None, Some(t)) => format!("/proc/{t}/mountinfo").into_bytes(),
            _ => b"/proc/self/mountinfo".to_vec(),
        };
        match sys::read_file(&path) {
            Ok(d) => parse_mountinfo(&d),
            Err(e) => {
                ul::warn(&short, io::lossy(&path), e);
                return 1;
            }
        }
    };

    // -T: o ponto de montagem que contém o caminho.
    let target_mp: Option<String> = target.as_ref().map(|t| {
        let mut cn = normalize(t);
        loop {
            if table.iter().any(|f| streq_paths(&f.target, &cn)) {
                return cn;
            }
            if cn == "/" {
                return String::new();
            }
            match cn.rfind('/') {
                Some(0) => cn = "/".to_string(),
                Some(i) => cn.truncate(i),
                None => return String::new(),
            }
        }
    });

    let searching = source.is_some()
        || target.is_some()
        || mountpoint.is_some()
        || swap_arg.is_some()
        || types.is_some()
        || opts_filter.is_some();

    let pred = |idx: usize, fs: &Fs| -> bool {
        let mut ok = true;
        if let Some(t) = &types {
            ok &= match_fstype(&fs.fstype, t);
        }
        if let Some(o) = &opts_filter {
            ok &= match_options(&fs.options, o);
        }
        if let Some(s) = &source {
            ok &= fs.source == *s || fs.maj_min == *s;
        }
        if let Some(mp) = &target_mp {
            ok &= !mp.is_empty() && streq_paths(&fs.target, mp);
        }
        if let Some(m) = &mountpoint {
            ok &= streq_paths(&fs.target, &normalize(m)) || streq_paths(&fs.target, m);
        }
        if let Some(a) = &swap_arg {
            ok &= fs.source == *a || streq_paths(&fs.target, a) || fs.maj_min == *a;
        }
        if searching && invert {
            ok = !ok;
        }
        if pseudo {
            ok &= is_pseudo(fs);
        }
        if real {
            ok &= !is_pseudo(fs) && fs.fstype != "swap";
        }
        if shadowed {
            ok &= table[idx + 1..].iter().any(|o| o.target == fs.target);
        }
        ok
    };

    let backward = direction.unwrap_or(searching && !fstab_mode && !mtab_mode);
    let order: Vec<usize> = if backward {
        (0..table.len()).rev().collect()
    } else {
        (0..table.len()).collect()
    };

    let mut selected = vec![false; table.len()];
    let mut picked: Vec<usize> = Vec::new();
    let mut seen_targets: Vec<&str> = Vec::new();
    for &i in &order {
        if !pred(i, &table[i]) {
            continue;
        }
        if uniq {
            if seen_targets.contains(&table[i].target.as_str()) {
                continue;
            }
            seen_targets.push(table[i].target.as_str());
        }
        selected[i] = true;
        picked.push(i);
        if first_only {
            break;
        }
    }
    if submounts {
        for &p in picked.clone().iter() {
            let base = table[p].target.trim_end_matches('/').to_string();
            for (j, f) in table.iter().enumerate() {
                if !selected[j] && f.target.starts_with(&format!("{base}/")) {
                    selected[j] = true;
                }
            }
        }
    }
    if !selected.iter().any(|s| *s) {
        return 1;
    }

    let mode = match mode_set {
        Some(m) => m,
        None => {
            if fstab_mode || mtab_mode {
                Mode::List
            } else {
                Mode::Tree
            }
        }
    };
    let _ = tree_flag;
    let tree = mode == Mode::Tree || (mode == Mode::Json && !fstab_mode && !mtab_mode && mode_set.is_none());
    let tree = tree && !fstab_mode && !mtab_mode;
    let tree = if mode == Mode::Json { !fstab_mode && !mtab_mode } else { tree };

    // Estrutura de árvore: cada selecionado pendura no ancestral selecionado mais próximo.
    let n = table.len();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut roots: Vec<usize> = Vec::new();
    let mut flat: Vec<usize> = Vec::new();
    if tree {
        for i in 0..n {
            if !selected[i] {
                continue;
            }
            let mut cur = table[i].parent;
            let mut anc: Option<usize> = None;
            let mut guard = 0;
            while let Some(pid) = cur {
                guard += 1;
                if guard > n + 1 {
                    break;
                }
                let Some(pi) = table.iter().position(|f| f.id == Some(pid)) else {
                    break;
                };
                if pi == i {
                    break;
                }
                if selected[pi] {
                    anc = Some(pi);
                    break;
                }
                cur = table[pi].parent;
            }
            match anc {
                Some(a) => children[a].push(i),
                None => roots.push(i),
            }
        }
    } else {
        let mut seen = vec![false; n];
        for &i in &order {
            if selected[i] && !seen[i] {
                seen[i] = true;
                flat.push(i);
            }
        }
        for i in 0..n {
            if selected[i] && !seen[i] {
                flat.push(i);
            }
        }
    }

    let o = Out {
        table: &table,
        cols: &output,
        nofsroot,
    };
    let mut buf = String::new();

    if mode == Mode::Json {
        buf.push_str("{\n   \"filesystems\": [\n");
        let list: Vec<usize> = if tree { roots.clone() } else { flat.clone() };
        let cnt = list.len();
        for (k, idx) in list.iter().enumerate() {
            o.json_node(*idx, &children, 0, tree, &mut buf);
            if k + 1 < cnt {
                buf.pop();
                buf.push_str(",\n");
            }
        }
        buf.push_str("   ]\n}\n");
        let _ = io::stdout().write_all(buf.as_bytes());
        return 0;
    }

    // Linhas com o prefixo de árvore (só a primeira coluna).
    let mut rows: Vec<(usize, String)> = Vec::new();
    if tree {
        fn walk(
            node: usize,
            prefix: &str,
            cont: &str,
            children: &[Vec<usize>],
            rows: &mut Vec<(usize, String)>,
        ) {
            rows.push((node, prefix.to_string()));
            let kids = &children[node];
            for (k, ch) in kids.iter().enumerate() {
                let last = k + 1 == kids.len();
                let p = format!("{cont}{}", if last { "`-" } else { "|-" });
                let c = format!("{cont}{}", if last { "  " } else { "| " });
                walk(*ch, &p, &c, children, rows);
            }
        }
        for r in &roots {
            walk(*r, "", "", &children, &mut rows);
        }
    } else {
        for i in &flat {
            rows.push((*i, String::new()));
        }
    }

    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|(idx, prefix)| {
            output
                .iter()
                .enumerate()
                .map(|(ci, c)| {
                    let v = column_value(c, &table[*idx], nofsroot);
                    if ci == 0 && !prefix.is_empty() {
                        format!("{prefix}{v}")
                    } else {
                        v
                    }
                })
                .collect()
        })
        .collect();

    match mode {
        Mode::Pairs => {
            for row in &cells {
                let parts: Vec<String> = output
                    .iter()
                    .zip(row.iter())
                    .map(|(c, v)| format!("{c}=\"{}\"", escape_pairs(v)))
                    .collect();
                buf.push_str(&parts.join(" "));
                buf.push('\n');
            }
        }
        Mode::Raw => {
            if !noheadings {
                buf.push_str(&output.join(" "));
                buf.push('\n');
            }
            for row in &cells {
                let parts: Vec<String> = row.iter().map(|v| escape_raw(v)).collect();
                buf.push_str(&parts.join(" "));
                buf.push('\n');
            }
        }
        _ => {
            let ncol = output.len();
            let mut widths: Vec<usize> = output.iter().map(|c| c.len()).collect();
            if noheadings {
                widths = vec![0; ncol];
            }
            for row in &cells {
                for (i, v) in row.iter().enumerate() {
                    widths[i] = widths[i].max(display_width(v));
                }
            }
            let emit = |row: &[String], buf: &mut String| {
                for (i, v) in row.iter().enumerate() {
                    if i + 1 == ncol {
                        buf.push_str(v);
                    } else {
                        buf.push_str(v);
                        for _ in display_width(v)..widths[i] {
                            buf.push(' ');
                        }
                        buf.push(' ');
                    }
                }
                buf.push('\n');
            };
            if !noheadings {
                emit(&output, &mut buf);
            }
            for row in &cells {
                emit(row, &mut buf);
            }
        }
    }
    let _ = io::stdout().write_all(buf.as_bytes());
    0
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}
