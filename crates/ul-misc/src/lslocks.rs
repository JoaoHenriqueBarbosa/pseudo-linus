//! `lslocks` do util-linux 2.41: lista os locks de arquivo do sistema.
//!
//! Porte do `misc-utils/lslocks.c`. Lê `/proc/locks` e, para o `COMMAND`, `/proc/<pid>/comm`.
//! O `SIZE` vem de um `stat` nos descritores abertos do processo (`/proc/<pid>/fd/*`) cujo inode
//! casa com o do lock. `sysabi` não expõe `readlink` nesta camada, então `PATH` fica vazio quando
//! não há como resolvê-lo.

use std::ffi::OsString;
use std::io::Write;

use sysabi::sys;

use crate::lsmem::human_size;
use crate::util::io;
use crate::util::ul;

#[derive(Copy, Clone, PartialEq, Eq)]
enum Col {
    Command,
    Pid,
    Type,
    Size,
    Mode,
    Mandatory,
    Start,
    End,
    Path,
    Blocker,
}

const ALL_COLS: &[Col] = &[
    Col::Command,
    Col::Pid,
    Col::Type,
    Col::Size,
    Col::Mode,
    Col::Mandatory,
    Col::Start,
    Col::End,
    Col::Path,
    Col::Blocker,
];
const DEFAULT_COLS: &[Col] = &[
    Col::Command,
    Col::Pid,
    Col::Type,
    Col::Size,
    Col::Mode,
    Col::Mandatory,
    Col::Start,
    Col::End,
    Col::Path,
];

impl Col {
    fn name(self) -> &'static str {
        match self {
            Col::Command => "COMMAND",
            Col::Pid => "PID",
            Col::Type => "TYPE",
            Col::Size => "SIZE",
            Col::Mode => "MODE",
            Col::Mandatory => "M",
            Col::Start => "START",
            Col::End => "END",
            Col::Path => "PATH",
            Col::Blocker => "BLOCKER",
        }
    }
    fn numeric(self) -> bool {
        matches!(
            self,
            Col::Pid | Col::Size | Col::Mandatory | Col::Start | Col::End | Col::Blocker
        )
    }
    fn from_name(n: &[u8]) -> Option<Col> {
        ALL_COLS
            .iter()
            .copied()
            .find(|c| c.name().as_bytes().eq_ignore_ascii_case(n))
    }
}

struct Lock {
    command: String,
    pid: i64,
    kind: String,
    size: Option<u64>,
    mode: String,
    mandatory: bool,
    start: String,
    end: String,
    path: String,
    blocker: Option<i64>,
}

const USAGE: &str = "
Usage:
 lslocks [options]

List local system locks.

Options:
 -b, --bytes            print SIZE in bytes rather than in human readable format
 -J, --json             use JSON output format
 -i, --noinaccessible   ignore locks without read permissions
 -n, --noheadings       don't print headings
 -o, --output <list>    output columns (see --list-columns)
     --output-all       output all columns
 -p, --pid <pid>        display only locks held by this process
 -r, --raw              use the raw output format
 -u, --notruncate       don't truncate text in columns

 -H, --list-columns     list the available columns
 -h, --help             display this help
 -V, --version          display version

For more details see lslocks(8).
";

const LIST_COLUMNS: &str = "COMMAND <string>        command of the process holding the lock
    PID <integer>       PID of the process holding the lock
   TYPE <string>        kind of lock
   SIZE <string|number> size of the lock, use <number> if --bytes is given
  INODE <integer>       inode number
MAJ:MIN <string>        major:minor device number
   MODE <string>        lock access mode
      M <boolean>       mandatory state of the lock: 0 (none), 1 (set)
  START <integer>       relative byte offset of the lock
    END <integer>       ending offset of the lock
   PATH <string>        path of the locked file
BLOCKER <integer>       PID of the process blocking the lock
HOLDERS <string>        holders of the lock
";

fn usage(short: &str) -> String {
    USAGE.replace("lslocks", short)
}

fn json_escape(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

fn read_text(path: &str) -> Option<String> {
    let data = sys::read_file(path.as_bytes()).ok()?;
    Some(String::from_utf8_lossy(&data).trim_end().to_string())
}

/// Procura nos descritores do processo o que tem o inode do lock e devolve o tamanho do arquivo.
fn file_size(pid: i64, ino: u64) -> Option<u64> {
    let dir = format!("/proc/{pid}/fd");
    let entries = sys::read_dir(dir.as_bytes()).ok()?;
    for e in entries {
        let name = String::from_utf8_lossy(&e.name).to_string();
        if name == "." || name == ".." {
            continue;
        }
        if let Ok(st) = sys::stat(format!("{dir}/{name}").as_bytes()) {
            if st.ino == ino {
                return Some(st.size);
            }
        }
    }
    None
}

fn parse_locks(pid_filter: Option<i64>, noinaccessible: bool) -> Vec<Lock> {
    let text = read_text("/proc/locks").unwrap_or_default();
    // Primeiro passe: o dono de cada id, para achar o bloqueador de uma linha "->".
    let mut owners: Vec<(String, i64)> = Vec::new();
    let mut rows: Vec<(bool, Vec<String>)> = Vec::new();
    for line in text.lines() {
        let mut f: Vec<String> = line.split_whitespace().map(|s| s.to_string()).collect();
        let blocked = f.get(1).is_some_and(|s| s == "->");
        if blocked {
            f.remove(1);
        }
        if f.len() < 8 {
            continue;
        }
        let pid: i64 = f[4].parse().unwrap_or(0);
        if !blocked {
            owners.push((f[0].clone(), pid));
        }
        rows.push((blocked, f));
    }
    let mut out = Vec::new();
    for (blocked, f) in rows {
        // f: id: kind advisory/mandatory mode pid maj:min:ino start end
        let pid: i64 = f[4].parse().unwrap_or(0);
        if pid_filter.is_some_and(|p| p != pid) {
            continue;
        }
        let ino: u64 = f[5].rsplit(':').next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let command = read_text(&format!("/proc/{pid}/comm")).unwrap_or_default();
        if noinaccessible && command.is_empty() {
            continue;
        }
        let kind = match f[1].as_str() {
            "FLOCK" => "FLOCK",
            "POSIX" => "POSIX",
            "OFDLCK" => "OFDLCK",
            "LEASE" => "LEASE",
            other => other,
        }
        .to_string();
        let mut mode = match f[3].as_str() {
            "WRITE" => "WRITE",
            "READ" => "READ",
            o => o,
        }
        .to_string();
        if blocked {
            mode.push('*');
        }
        let blocker = if blocked {
            owners.iter().find(|(id, _)| *id == f[0]).map(|(_, p)| *p)
        } else {
            None
        };
        out.push(Lock {
            command,
            pid,
            kind,
            size: file_size(pid, ino),
            mode,
            mandatory: f[2] == "MANDATORY",
            start: f[6].clone(),
            end: f[7].clone(),
            path: String::new(),
            blocker,
        });
    }
    out
}

fn value(c: Col, l: &Lock, bytes: bool) -> String {
    match c {
        Col::Command => l.command.clone(),
        Col::Pid => l.pid.to_string(),
        Col::Type => l.kind.clone(),
        Col::Size => match l.size {
            Some(s) if bytes => s.to_string(),
            Some(s) => human_size(s),
            None => String::new(),
        },
        Col::Mode => l.mode.clone(),
        Col::Mandatory => (l.mandatory as u8).to_string(),
        Col::Start => l.start.clone(),
        Col::End => l.end.clone(),
        Col::Path => l.path.clone(),
        Col::Blocker => l.blocker.map(|b| b.to_string()).unwrap_or_default(),
    }
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);

    let mut json = false;
    let mut raw = false;
    let mut bytes = false;
    let mut noheadings = false;
    let mut noinaccessible = false;
    let mut list_columns = false;
    let mut pid_filter: Option<i64> = None;
    let mut cols: Vec<Col> = DEFAULT_COLS.to_vec();

    let longs: &[(&str, bool)] = &[
        ("bytes", false),
        ("list-columns", false),
        ("noinaccessible", false),
        ("noheadings", false),
        ("output", true),
        ("output-all", false),
        ("pid", true),
        ("raw", false),
        ("notruncate", false),
        ("json", false),
        ("help", false),
        ("version", false),
    ];

    let mut i = 1;
    let mut operands = Vec::new();
    while i < argv.len() {
        let a = argv[i].as_slice();
        if a == b"--" {
            operands.extend(argv[i + 1..].iter().cloned());
            break;
        }
        if a.len() < 2 || a[0] != b'-' {
            operands.push(a.to_vec());
            i += 1;
            continue;
        }
        let mut items: Vec<(String, Option<Vec<u8>>)> = Vec::new();
        if a.starts_with(b"--") {
            let (name, inline) = match a.iter().position(|b| *b == b'=') {
                Some(p) => (a[2..p].to_vec(), Some(a[p + 1..].to_vec())),
                None => (a[2..].to_vec(), None),
            };
            let name_s = String::from_utf8_lossy(&name).to_string();
            let exact = longs.iter().find(|(n, _)| *n == name_s);
            let cands: Vec<_> = longs.iter().filter(|(n, _)| n.starts_with(&name_s)).collect();
            let found = match exact {
                Some(e) => e,
                None if cands.len() == 1 => cands[0],
                None if cands.is_empty() => {
                    ul::warnx(&short, format!("unrecognized option '{}'", io::lossy(a)));
                    ul::errtryhelp(&short);
                    return 1;
                }
                None => {
                    ul::warnx(&short, format!("option '{}' is ambiguous", io::lossy(a)));
                    ul::errtryhelp(&short);
                    return 1;
                }
            };
            let (n, needs) = found;
            let mut val = inline;
            if *needs && val.is_none() {
                i += 1;
                match argv.get(i) {
                    Some(v) => val = Some(v.clone()),
                    None => {
                        ul::warnx(&short, format!("option '--{n}' requires an argument"));
                        ul::errtryhelp(&short);
                        return 1;
                    }
                }
            }
            items.push((format!("--{n}"), val));
        } else {
            let mut k = 1;
            while k < a.len() {
                let c = a[k];
                match c {
                    b'b' | b'H' | b'i' | b'n' | b'r' | b'u' | b'J' | b'h' | b'V' => {
                        items.push((format!("-{}", c as char), None));
                        k += 1;
                    }
                    b'o' | b'p' => {
                        let val = if k + 1 < a.len() {
                            a[k + 1..].to_vec()
                        } else {
                            i += 1;
                            match argv.get(i) {
                                Some(v) => v.clone(),
                                None => {
                                    ul::warnx(
                                        &short,
                                        format!("option requires an argument -- '{}'", c as char),
                                    );
                                    ul::errtryhelp(&short);
                                    return 1;
                                }
                            }
                        };
                        items.push((format!("-{}", c as char), Some(val)));
                        break;
                    }
                    _ => {
                        ul::warnx(&short, format!("invalid option -- '{}'", c as char));
                        ul::errtryhelp(&short);
                        return 1;
                    }
                }
            }
        }
        i += 1;
        for (key, val) in items {
            match key.as_str() {
                "-h" | "--help" => {
                    let mut out = io::stdout();
                    let _ = out.write_all(usage(&short).as_bytes());
                    return 0;
                }
                "-V" | "--version" => {
                    ul::print_version(&short);
                    return 0;
                }
                "-b" | "--bytes" => bytes = true,
                "-H" | "--list-columns" => list_columns = true,
                "-i" | "--noinaccessible" => noinaccessible = true,
                "-n" | "--noheadings" => noheadings = true,
                "-r" | "--raw" => raw = true,
                "-J" | "--json" => json = true,
                "-u" | "--notruncate" => {}
                "--output-all" => cols = ALL_COLS.to_vec(),
                "-p" | "--pid" => {
                    let v = val.unwrap_or_default();
                    match ul::strtou64_or_err(&v, "failed to parse pid") {
                        Ok(n) => pid_filter = Some(n as i64),
                        Err(m) => {
                            ul::warnx(&short, m);
                            return 1;
                        }
                    }
                }
                "-o" | "--output" => {
                    let list = val.unwrap_or_default();
                    let mut parsed = Vec::new();
                    for name in list.split(|b| *b == b',') {
                        if name.is_empty() {
                            continue;
                        }
                        match Col::from_name(name) {
                            Some(c) => parsed.push(c),
                            None => {
                                ul::warnx(&short, format!("unknown column: {}", io::lossy(name)));
                                return 1;
                            }
                        }
                    }
                    cols = parsed;
                }
                _ => {}
            }
        }
    }
    // Operandos são ignorados, como no original.
    let _ = operands;

    let mut out = String::new();
    if list_columns {
        out.push_str(LIST_COLUMNS);
        let mut so = io::stdout();
        let _ = so.write_all(out.as_bytes());
        return 0;
    }

    let locks = parse_locks(pid_filter, noinaccessible);
    let rows: Vec<Vec<String>> = locks
        .iter()
        .map(|l| cols.iter().map(|c| value(*c, l, bytes)).collect())
        .collect();

    if rows.is_empty() && !json {
        // Sem travas, a tabela não imprime nem o cabeçalho.
    } else if json {
        out.push_str("{\n   \"locks\": [");
        for (n, row) in rows.iter().enumerate() {
            out.push_str(if n == 0 { "\n" } else { ",\n" });
            out.push_str("      {\n");
            for (k, c) in cols.iter().enumerate() {
                let v = &row[k];
                let rendered = if *c == Col::Mandatory {
                    (v == "1").to_string()
                } else if c.numeric() && !v.is_empty() && (*c != Col::Size || bytes) {
                    v.clone()
                } else if v.is_empty() {
                    "null".to_string()
                } else {
                    format!("\"{}\"", json_escape(v))
                };
                out.push_str(&format!(
                    "         \"{}\": {rendered}",
                    c.name().to_ascii_lowercase()
                ));
                out.push_str(if k + 1 < cols.len() { ",\n" } else { "\n" });
            }
            out.push_str("      }");
        }
        out.push_str("\n   ]\n}\n");
    } else if raw {
        if !noheadings {
            let h: Vec<&str> = cols.iter().map(|c| c.name()).collect();
            out.push_str(&h.join(" "));
            out.push('\n');
        }
        for row in &rows {
            out.push_str(&row.join(" "));
            out.push('\n');
        }
    } else if !cols.is_empty() {
        let mut widths: Vec<usize> = cols.iter().map(|c| c.name().len()).collect();
        for row in &rows {
            for (k, v) in row.iter().enumerate() {
                widths[k] = widths[k].max(v.chars().count());
            }
        }
        let fmt_row = |cells: Vec<&str>| -> String {
            let mut line = String::new();
            for (k, cell) in cells.iter().enumerate() {
                if k > 0 {
                    line.push(' ');
                }
                let pad = widths[k].saturating_sub(cell.chars().count());
                if cols[k].numeric() {
                    line.push_str(&" ".repeat(pad));
                    line.push_str(cell);
                } else if k + 1 < cols.len() {
                    line.push_str(cell);
                    line.push_str(&" ".repeat(pad));
                } else {
                    line.push_str(cell);
                }
            }
            line.push('\n');
            line
        };
        if !noheadings {
            out.push_str(&fmt_row(cols.iter().map(|c| c.name()).collect()));
        }
        for row in &rows {
            out.push_str(&fmt_row(row.iter().map(|s| s.as_str()).collect()));
        }
    }

    let mut so = io::stdout();
    let _ = so.write_all(out.as_bytes());
    0
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}
