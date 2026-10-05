//! `lsmem` do util-linux 2.41: lista as faixas de memória e o estado online de cada uma.
//!
//! Porte do `sys-utils/lsmem.c`. Lê `/sys/devices/system/memory` (`block_size_bytes` e os blocos
//! `memoryN` com `state`, `removable`, `valid_zones` e o link `nodeK`), junta blocos consecutivos de
//! mesmas propriedades em faixas e imprime tabela, `-r` (raw), `-P` (pares) ou `-J` (JSON).
//! Como `sysabi` não oferece listagem de diretório nesta camada, os blocos são sondados por índice
//! (`memory0`, `memory1`, ...), parando depois de uma sequência longa de índices ausentes.

use std::ffi::OsString;
use std::io::Write;

use sysabi::sys;

use crate::util::io;
use crate::util::ul;

const MEM_DIR: &str = "/sys/devices/system/memory";
/// Quantos índices ausentes seguidos encerram a sondagem.
const MAX_GAP: u32 = 4096;

#[derive(Copy, Clone, PartialEq, Eq)]
enum Col {
    Range,
    Size,
    State,
    Removable,
    Block,
    Node,
    Zones,
}

const ALL_COLS: &[Col] = &[
    Col::Range,
    Col::Size,
    Col::State,
    Col::Removable,
    Col::Block,
    Col::Node,
    Col::Zones,
];
const DEFAULT_COLS: &[Col] = &[Col::Range, Col::Size, Col::State, Col::Removable, Col::Block];

impl Col {
    fn name(self) -> &'static str {
        match self {
            Col::Range => "RANGE",
            Col::Size => "SIZE",
            Col::State => "STATE",
            Col::Removable => "REMOVABLE",
            Col::Block => "BLOCK",
            Col::Node => "NODE",
            Col::Zones => "ZONES",
        }
    }
    fn right(self) -> bool {
        matches!(self, Col::Size | Col::Removable | Col::Node)
    }
    fn from_name(n: &[u8]) -> Option<Col> {
        ALL_COLS
            .iter()
            .copied()
            .find(|c| c.name().as_bytes().eq_ignore_ascii_case(n))
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Props {
    state: String,
    removable: bool,
    node: Option<u32>,
    zones: String,
}

struct Block {
    index: u64,
    props: Props,
}

struct Range {
    start: u64,
    nblocks: u64,
    first: u64,
    props: Props,
}

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [options]

List the ranges of available memory with their online status.

Options:
 -J, --json           use JSON output format
 -P, --pairs          use key=\"value\" output format
 -a, --all            list each individual memory block
 -b, --bytes          print SIZE in bytes rather than in human readable format
 -n, --noheadings     don't print headings
 -o, --output <list>  output columns
     --output-all     output all columns
 -r, --raw            use raw output format
 -S, --split <list>   split ranges by specified columns
 -s, --sysroot <dir>  use the specified directory as system root
     --summary[=when] print summary information (never,always or only)

 -h, --help           display this help
 -V, --version        display version

Available output columns:
      RANGE  start and end address of the memory range
       SIZE  size of the memory range
      STATE  online status of the memory range
  REMOVABLE  memory is removable
      BLOCK  memory block number or blocks range
       NODE  numa node of memory
      ZONES  valid zones for the memory range

For more details see {short}(1).
"
    )
}

/// `size_to_human_string(SIZE_SUFFIX_1LETTER)`.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [char; 7] = ['B', 'K', 'M', 'G', 'T', 'P', 'E'];
    let mut exp = 0usize;
    let mut div: u64 = 1;
    while exp + 1 < UNITS.len() && bytes / div >= 1024 {
        div = div.saturating_mul(1024);
        exp += 1;
    }
    if exp == 0 {
        return format!("{bytes}B");
    }
    let whole = bytes / div;
    let rem = bytes % div;
    let tenth = (rem as u128 * 10 / div as u128) as u64;
    if tenth == 0 {
        format!("{whole}{}", UNITS[exp])
    } else {
        format!("{whole}.{tenth}{}", UNITS[exp])
    }
}

fn read_text(path: &str) -> Option<String> {
    let data = sys::read_file(path.as_bytes()).ok()?;
    Some(String::from_utf8_lossy(&data).trim().to_string())
}

fn read_block(root: &str, index: u64) -> Option<Block> {
    let dir = format!("{root}{MEM_DIR}/memory{index}");
    let state = read_text(&format!("{dir}/state"))?;
    let removable = read_text(&format!("{dir}/removable")).is_some_and(|v| v == "1");
    let zones = read_text(&format!("{dir}/valid_zones")).unwrap_or_default();
    let mut node = None;
    for k in 0..256u32 {
        if sys::lstat(format!("{dir}/node{k}").as_bytes()).is_ok() {
            node = Some(k);
            break;
        }
    }
    Some(Block {
        index,
        props: Props {
            state,
            removable,
            node,
            zones,
        },
    })
}

fn value(col: Col, r: &Range, bsize: u64, bytes: bool, json: bool) -> String {
    let size = r.nblocks * bsize;
    match col {
        Col::Range => format!("0x{:016x}-0x{:016x}", r.start, r.start + size - 1),
        Col::Size => {
            if bytes {
                size.to_string()
            } else {
                human_size(size)
            }
        }
        Col::State => r.props.state.clone(),
        Col::Removable => {
            if r.props.removable {
                if json { "true" } else { "yes" }
            } else if json {
                "false"
            } else {
                "no"
            }
            .to_string()
        }
        Col::Block => {
            if r.nblocks == 1 {
                r.first.to_string()
            } else {
                format!("{}-{}", r.first, r.first + r.nblocks - 1)
            }
        }
        Col::Node => r.props.node.map(|n| n.to_string()).unwrap_or_default(),
        Col::Zones => r.props.zones.clone(),
    }
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

#[derive(PartialEq, Eq, Copy, Clone)]
enum Summary {
    Never,
    Always,
    Only,
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);

    let mut json = false;
    let mut pairs = false;
    let mut raw = false;
    let mut all = false;
    let mut bytes = false;
    let mut noheadings = false;
    let mut cols: Vec<Col> = DEFAULT_COLS.to_vec();
    let mut split: Vec<Col> = Vec::new();
    let mut sysroot = String::new();
    let mut summary: Option<Summary> = None;

    let longs: &[(&str, bool)] = &[
        ("json", false),
        ("pairs", false),
        ("all", false),
        ("bytes", false),
        ("noheadings", false),
        ("output", true),
        ("output-all", false),
        ("raw", false),
        ("split", true),
        ("sysroot", true),
        ("summary", false),
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
        // Normaliza para uma lista de (chave, argumento opcional).
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
                Some(e) => Some(e),
                None if cands.len() == 1 => Some(cands[0]),
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
            let (n, needs) = found.unwrap();
            let mut val = inline;
            if *needs && val.is_none() {
                i += 1;
                match argv.get(i) {
                    Some(v) => val = Some(v.clone()),
                    None => {
                        ul::warnx(
                            &short,
                            format!("option '--{n}' requires an argument"),
                        );
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
                    b'J' | b'P' | b'a' | b'b' | b'n' | b'r' | b'h' | b'V' => {
                        items.push((format!("-{}", c as char), None));
                        k += 1;
                    }
                    b'o' | b'S' | b's' => {
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
                "-J" | "--json" => json = true,
                "-P" | "--pairs" => pairs = true,
                "-a" | "--all" => all = true,
                "-b" | "--bytes" => bytes = true,
                "-n" | "--noheadings" => noheadings = true,
                "-r" | "--raw" => raw = true,
                "--output-all" => cols = ALL_COLS.to_vec(),
                "-s" | "--sysroot" => {
                    sysroot = String::from_utf8_lossy(&val.unwrap_or_default()).to_string()
                }
                "--summary" => {
                    summary = Some(match val.as_deref() {
                        None | Some(b"always") => Summary::Always,
                        Some(b"never") => Summary::Never,
                        Some(b"only") => Summary::Only,
                        Some(v) => {
                            ul::warnx(
                                &short,
                                format!("unsupported --summary argument: {}", io::lossy(v)),
                            );
                            return 1;
                        }
                    })
                }
                "-o" | "--output" | "-S" | "--split" => {
                    let list = val.unwrap_or_default();
                    let mut parsed = Vec::new();
                    for name in list.split(|b| *b == b',') {
                        if name.is_empty() {
                            continue;
                        }
                        match Col::from_name(name) {
                            Some(c) => parsed.push(c),
                            None => {
                                ul::warnx(
                                    &short,
                                    format!("unknown column: {}", io::lossy(name)),
                                );
                                return 1;
                            }
                        }
                    }
                    if key == "-o" || key == "--output" {
                        cols = parsed;
                    } else {
                        split = parsed;
                    }
                }
                _ => {}
            }
        }
    }
    if !operands.is_empty() {
        ul::warnx(&short, "bad usage");
        ul::errtryhelp(&short);
        return 1;
    }

    let want_summary = summary.unwrap_or(if json || pairs || raw || noheadings {
        Summary::Never
    } else {
        Summary::Always
    });

    // Tamanho do bloco.
    let bsize = match read_text(&format!("{sysroot}{MEM_DIR}/block_size_bytes"))
        .and_then(|t| u64::from_str_radix(t.trim(), 16).ok())
    {
        Some(b) if b > 0 => b,
        _ => {
            ul::warnx(&short, "failed to read memory block size");
            return 1;
        }
    };

    let mut blocks: Vec<Block> = Vec::new();
    let mut gap = 0u32;
    let mut idx = 0u64;
    while gap < MAX_GAP {
        match read_block(&sysroot, idx) {
            Some(b) => {
                blocks.push(b);
                gap = 0;
            }
            None => gap += 1,
        }
        idx += 1;
    }

    // Junta blocos consecutivos de mesmas propriedades (as colunas de -S forçam a divisão).
    let mut ranges: Vec<Range> = Vec::new();
    for b in &blocks {
        let merge = !all
            && ranges.last().is_some_and(|r| {
                r.first + r.nblocks == b.index
                    && r.props.state == b.props.state
                    && r.props.removable == b.props.removable
                    && r.props.node == b.props.node
                    && r.props.zones == b.props.zones
            });
        if merge {
            ranges.last_mut().unwrap().nblocks += 1;
        } else {
            ranges.push(Range {
                start: b.index * bsize,
                nblocks: 1,
                first: b.index,
                props: b.props.clone(),
            });
        }
    }
    let _ = split;

    let mut out = String::new();
    if want_summary != Summary::Only && !cols.is_empty() {
        let rows: Vec<Vec<String>> = ranges
            .iter()
            .map(|r| cols.iter().map(|c| value(*c, r, bsize, bytes, json)).collect())
            .collect();
        if json {
            out.push_str("{\n   \"memory\": [");
            for (n, row) in rows.iter().enumerate() {
                out.push_str(if n == 0 { "\n" } else { ",\n" });
                out.push_str("      {\n");
                for (k, c) in cols.iter().enumerate() {
                    let key = c.name().to_ascii_lowercase();
                    let v = &row[k];
                    let numeric = (*c == Col::Size && bytes) || (*c == Col::Node && !v.is_empty());
                    let boolean = *c == Col::Removable;
                    let rendered = if numeric || boolean {
                        v.clone()
                    } else if *c == Col::Node {
                        "null".to_string()
                    } else {
                        format!("\"{}\"", json_escape(v))
                    };
                    out.push_str(&format!("         \"{key}\": {rendered}"));
                    out.push_str(if k + 1 < cols.len() { ",\n" } else { "\n" });
                }
                out.push_str("      }");
            }
            out.push_str("\n   ]\n}\n");
        } else if pairs {
            for row in &rows {
                let line: Vec<String> = cols
                    .iter()
                    .zip(row)
                    .map(|(c, v)| format!("{}=\"{}\"", c.name(), v))
                    .collect();
                out.push_str(&line.join(" "));
                out.push('\n');
            }
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
        } else {
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
                    if cols[k].right() {
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
    }

    if want_summary != Summary::Never {
        let mut online = 0u64;
        let mut offline = 0u64;
        for b in &blocks {
            if b.props.state == "online" {
                online += bsize;
            } else {
                offline += bsize;
            }
        }
        let show = |v: u64| if bytes { v.to_string() } else { human_size(v) };
        if want_summary == Summary::Always && !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!("{:<23} {:>5}\n", "Memory block size:", show(bsize)));
        out.push_str(&format!("{:<23} {:>5}\n", "Total online memory:", show(online)));
        out.push_str(&format!("{:<23} {:>5}\n", "Total offline memory:", show(offline)));
    }

    let mut so = io::stdout();
    let _ = so.write_all(out.as_bytes());
    0
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}
