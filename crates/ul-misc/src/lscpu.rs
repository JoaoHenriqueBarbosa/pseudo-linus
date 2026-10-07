//! `lscpu` do util-linux 2.41: mostra a arquitetura da CPU.
//!
//! Porte enxuto do `sys-utils/lscpu.c` para x86: lê `/proc/cpuinfo` e `/sys/devices/system/cpu`
//! (topologia, caches, frequências, vulnerabilidades) e `/sys/devices/system/node`. Saídas: padrão
//! (`Campo: valor`), `-J`, `-e` (tabela estendida) e `-p` (parsable). Como `sysabi` não lista
//! diretórios nesta camada, as CPUs são sondadas por índice a partir de `possible`.

use std::ffi::OsString;
use std::io::Write;

use sysabi::sys;

use ul_common::fsutil::size_to_human_string;
use crate::util::io;
use crate::util::ul;

const CPU_DIR: &str = "/sys/devices/system/cpu";
const NODE_DIR: &str = "/sys/devices/system/node";

fn usage(short: &str) -> String {
    format!(
        "
Usage:
 {short} [options]

Display information about the CPU architecture.

Options:
 -a, --all               print both online and offline CPUs (default for -e)
 -b, --online            print online CPUs only (default for -p)
 -B, --bytes             print sizes in bytes rather than in human readable format
 -C, --caches[=<list>]   info about caches in extended readable format
 -c, --offline           print offline CPUs only
 -J, --json              use JSON for default or extended format
 -e, --extended[=<list>] print out an extended readable format
 -p, --parse[=<list>]    print out a parsable format
 -r, --raw               use raw output format (for -e, -p and -C)
 -s, --sysroot <dir>     use specified directory as system root
 -x, --hex               print hexadecimal masks rather than lists of CPUs
 -y, --physical          print physical instead of logical IDs
     --hierarchic[=when] use subsections in summary (auto, never, always)
     --output-all        print all available columns for -e, -p or -C

 -h, --help              display this help
 -V, --version           display version

Available output columns for -e or -p:
      BOGOMIPS  crude measurement of CPU speed
           CPU  logical CPU number
          CORE  logical core number
        SOCKET  logical socket number
       CLUSTER  logical cluster number
          NODE  logical NUMA node number
          BOOK  logical book number
        DRAWER  logical drawer number
         CACHE  shows how caches are shared between CPUs
  POLARIZATION  CPU dispatching mode on virtual hardware
       ADDRESS  physical address of a CPU
    CONFIGURED  shows if the hypervisor has allocated the CPU
        ONLINE  shows if Linux currently makes use of the CPU
           MHZ  shows the current MHz of the CPU
      SCALMHZ%  shows scaling percentage of the CPU frequency
        MAXMHZ  shows the maximum MHz of the CPU
        MINMHZ  shows the minimum MHz of the CPU
     MODELNAME  shows CPU model name

Available output columns for -C:
      ALL-SIZE  size of all system caches
         LEVEL  cache level
          NAME  cache name
      ONE-SIZE  size of one cache
          TYPE  cache type
          WAYS  ways of associativity
  ALLOC-POLICY  allocation policy
  WRITE-POLICY  write policy
      PHY-LINE  number of physical cache lines per cache tag
          SETS  number of sets in the cache (lines in a set have the same cache index)
 COHERENCY-SIZE  minimum amount of data in bytes transferred from memory to cache

For more details see {short}(1).
"
    )
}

fn read_text(path: &str) -> Option<String> {
    let data = sys::read_file(path.as_bytes()).ok()?;
    Some(String::from_utf8_lossy(&data).trim().to_string())
}

/// Interpreta uma lista de CPUs do kernel (`0-3,5`).
fn parse_list(s: &str) -> Vec<u32> {
    let mut v = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.split_once('-') {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                    v.extend(a..=b);
                }
            }
            None => {
                if let Ok(a) = part.parse::<u32>() {
                    v.push(a);
                }
            }
        }
    }
    v
}

/// Formata uma lista ordenada como faixas (`0-3,5`).
fn fmt_list(v: &[u32]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < v.len() {
        let mut j = i;
        while j + 1 < v.len() && v[j + 1] == v[j] + 1 {
            j += 1;
        }
        if !out.is_empty() {
            out.push(',');
        }
        if j == i {
            out.push_str(&v[i].to_string());
        } else if j == i + 1 {
            out.push_str(&format!("{},{}", v[i], v[j]));
        } else {
            out.push_str(&format!("{}-{}", v[i], v[j]));
        }
        i = j + 1;
    }
    out
}

#[derive(Clone)]
struct CacheInfo {
    level: u32,
    kind: String,
    size: u64,
    shared: String,
}

struct Cpu {
    id: u32,
    online: bool,
    package: u32,
    core: u32,
    caches: Vec<CacheInfo>,
}

fn parse_cache_size(s: &str) -> u64 {
    let s = s.trim();
    let (num, mult) = match s.chars().last() {
        Some('K') => (&s[..s.len() - 1], 1024),
        Some('M') => (&s[..s.len() - 1], 1024 * 1024),
        Some('G') => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1),
    };
    num.parse::<u64>().unwrap_or(0) * mult
}

fn json_escape(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o
}

/// Posição (índice de primeira aparição) de `key` em `seen`, inserindo se novo.
fn first_seen<T: PartialEq + Clone>(seen: &mut Vec<T>, key: &T) -> usize {
    match seen.iter().position(|k| k == key) {
        Some(p) => p,
        None => {
            seen.push(key.clone());
            seen.len() - 1
        }
    }
}

#[derive(Copy, Clone, PartialEq)]
enum Mode {
    Default,
    Extended,
    Parse,
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let short = ul::short_name(args);

    let mut mode = Mode::Default;
    let mut json = false;
    let mut bytes = false;
    let mut want: Option<bool> = None; // Some(true)=online, Some(false)=offline
    let mut all = false;
    let mut sysroot = String::new();
    let mut col_list: Option<String> = None;

    let mut i = 1;
    while i < argv.len() {
        let a = argv[i].as_slice();
        i += 1;
        if a.len() < 2 || a[0] != b'-' {
            ul::warnx(&short, "bad usage");
            ul::errtryhelp(&short);
            return 1;
        }
        if a.starts_with(b"--") {
            let (name, inline) = match a.iter().position(|b| *b == b'=') {
                Some(p) => (
                    String::from_utf8_lossy(&a[2..p]).to_string(),
                    Some(String::from_utf8_lossy(&a[p + 1..]).to_string()),
                ),
                None => (String::from_utf8_lossy(&a[2..]).to_string(), None),
            };
            let longs = [
                "all", "online", "bytes", "caches", "offline", "json", "extended", "parse",
                "sysroot", "hex", "physical", "output-all", "help", "version",
            ];
            let cands: Vec<&&str> = longs.iter().filter(|n| n.starts_with(&name)).collect();
            let full = if longs.contains(&name.as_str()) {
                name.clone()
            } else if cands.len() == 1 {
                cands[0].to_string()
            } else if cands.is_empty() {
                ul::warnx(&short, format!("unrecognized option '{}'", io::lossy(a)));
                ul::errtryhelp(&short);
                return 1;
            } else {
                ul::warnx(&short, format!("option '{}' is ambiguous", io::lossy(a)));
                ul::errtryhelp(&short);
                return 1;
            };
            match full.as_str() {
                "all" => all = true,
                "online" => want = Some(true),
                "offline" => want = Some(false),
                "bytes" => bytes = true,
                "json" => json = true,
                "extended" => {
                    mode = Mode::Extended;
                    col_list = inline;
                }
                "parse" => {
                    mode = Mode::Parse;
                    col_list = inline;
                }
                "caches" | "hex" | "physical" | "output-all" => {}
                "sysroot" => match inline.or_else(|| {
                    let v = argv.get(i).map(|v| String::from_utf8_lossy(v).to_string());
                    if v.is_some() {
                        i += 1;
                    }
                    v
                }) {
                    Some(v) => sysroot = v,
                    None => {
                        ul::warnx(&short, "option '--sysroot' requires an argument");
                        ul::errtryhelp(&short);
                        return 1;
                    }
                },
                "help" => {
                    let mut out = io::stdout();
                    let _ = out.write_all(usage(&short).as_bytes());
                    return 0;
                }
                "version" => {
                    ul::print_version(&short);
                    return 0;
                }
                _ => {}
            }
            continue;
        }
        let mut k = 1;
        while k < a.len() {
            let c = a[k];
            k += 1;
            match c {
                b'a' => all = true,
                b'b' => want = Some(true),
                b'c' => want = Some(false),
                b'B' => bytes = true,
                b'J' => json = true,
                b'x' | b'y' | b'C' => {
                    if c == b'C' && k < a.len() {
                        k = a.len();
                    }
                }
                b'e' | b'p' => {
                    mode = if c == b'e' { Mode::Extended } else { Mode::Parse };
                    if k < a.len() {
                        col_list = Some(String::from_utf8_lossy(&a[k..]).to_string());
                        k = a.len();
                    }
                }
                b's' => {
                    let v = if k < a.len() {
                        let v = String::from_utf8_lossy(&a[k..]).to_string();
                        k = a.len();
                        Some(v)
                    } else {
                        let v = argv.get(i).map(|v| String::from_utf8_lossy(v).to_string());
                        if v.is_some() {
                            i += 1;
                        }
                        v
                    };
                    match v {
                        Some(v) => sysroot = v,
                        None => {
                            ul::warnx(&short, "option requires an argument -- 's'");
                            ul::errtryhelp(&short);
                            return 1;
                        }
                    }
                }
                b'h' => {
                    let mut out = io::stdout();
                    let _ = out.write_all(usage(&short).as_bytes());
                    return 0;
                }
                b'V' => {
                    ul::print_version(&short);
                    return 0;
                }
                _ => {
                    ul::warnx(&short, format!("invalid option -- '{}'", c as char));
                    ul::errtryhelp(&short);
                    return 1;
                }
            }
        }
    }

    let root = sysroot;
    let cpuinfo = read_text(&format!("{root}/proc/cpuinfo")).unwrap_or_default();
    let mut first: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for line in cpuinfo.lines() {
        if line.trim().is_empty() && !first.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            first
                .entry(k.trim().to_string())
                .or_insert_with(|| v.trim().to_string());
        }
    }
    let fld = |k: &str| first.get(k).cloned();

    let possible = parse_list(&read_text(&format!("{root}{CPU_DIR}/possible")).unwrap_or_default());
    let online_list =
        parse_list(&read_text(&format!("{root}{CPU_DIR}/online")).unwrap_or_default());
    let mut cpus: Vec<Cpu> = Vec::new();
    for id in possible {
        let dir = format!("{root}{CPU_DIR}/cpu{id}");
        let online = online_list.contains(&id);
        let pkg = read_text(&format!("{dir}/topology/physical_package_id"))
            .and_then(|v| v.parse::<i64>().ok());
        let core =
            read_text(&format!("{dir}/topology/core_id")).and_then(|v| v.parse::<i64>().ok());
        if !online && pkg.is_none() && sys::stat(dir.as_bytes()).is_err() {
            continue;
        }
        let mut caches = Vec::new();
        for n in 0..16 {
            let cd = format!("{dir}/cache/index{n}");
            let Some(level) = read_text(&format!("{cd}/level")).and_then(|v| v.parse().ok())
            else {
                break;
            };
            caches.push(CacheInfo {
                level,
                kind: read_text(&format!("{cd}/type")).unwrap_or_default(),
                size: parse_cache_size(&read_text(&format!("{cd}/size")).unwrap_or_default()),
                shared: read_text(&format!("{cd}/shared_cpu_list")).unwrap_or_default(),
            });
        }
        cpus.push(Cpu {
            id,
            online,
            package: pkg.unwrap_or(0).max(0) as u32,
            core: core.unwrap_or(id as i64).max(0) as u32,
            caches,
        });
    }

    // NUMA.
    let mut nodes: Vec<Vec<u32>> = Vec::new();
    for n in 0..1024u32 {
        match read_text(&format!("{root}{NODE_DIR}/node{n}/cpulist")) {
            Some(l) => nodes.push(parse_list(&l)),
            None => {
                if n > 0 {
                    break;
                }
            }
        }
    }
    let node_of = |id: u32| nodes.iter().position(|l| l.contains(&id));

    let mut pkgs: Vec<u32> = Vec::new();
    let mut cores: Vec<(u32, u32)> = Vec::new();
    for c in &cpus {
        first_seen(&mut pkgs, &c.package);
        first_seen(&mut cores, &(c.package, c.core));
    }

    // Identificadores de cache por (nível, tipo), na ordem de primeira aparição.
    let cache_kinds: [(u32, &str, &str); 4] =
        [(1, "Data", "L1d"), (1, "Instruction", "L1i"), (2, "Unified", "L2"), (3, "Unified", "L3")];
    let mut cache_ids: Vec<Vec<String>> = vec![Vec::new(); 4];
    let cache_id_of = |cache_ids: &mut Vec<Vec<String>>, c: &Cpu, k: usize| -> Option<usize> {
        let (lvl, kind, _) = cache_kinds[k];
        let ci = c.caches.iter().find(|ci| ci.level == lvl && ci.kind == kind)?;
        Some(first_seen(&mut cache_ids[k], &ci.shared))
    };

    let selected: Vec<&Cpu> = cpus
        .iter()
        .filter(|c| match want {
            Some(true) => c.online,
            Some(false) => !c.online,
            None => mode != Mode::Default && (all || mode == Mode::Extended) || c.online || mode == Mode::Default,
        })
        .filter(|c| !(mode == Mode::Parse && want.is_none() && !all && !c.online))
        .collect();

    let freq = |id: u32, f: &str| -> Option<String> {
        let v = read_text(&format!("{root}{CPU_DIR}/cpu{id}/cpufreq/{f}"))?;
        let khz: f64 = v.parse().ok()?;
        Some(format!("{:.4}", khz / 1000.0))
    };

    let mut out = String::new();
    match mode {
        Mode::Parse => {
            out.push_str(
                "# The following is the parsable format, which can be fed to other\n# programs. Each different item in every column has an unique ID\n# starting from zero.\n",
            );
            out.push_str("# CPU,Core,Socket,Node,,L1d,L1i,L2,L3\n");
            for c in selected {
                let core = cores.iter().position(|k| *k == (c.package, c.core)).unwrap_or(0);
                let sock = pkgs.iter().position(|k| *k == c.package).unwrap_or(0);
                let node = node_of(c.id).map(|n| n.to_string()).unwrap_or_default();
                let mut line = format!("{},{},{},{},", c.id, core, sock, node);
                for k in 0..4 {
                    line.push(',');
                    if let Some(id) = cache_id_of(&mut cache_ids, c, k) {
                        line.push_str(&id.to_string());
                    }
                }
                out.push_str(&line);
                out.push('\n');
            }
            let _ = col_list;
        }
        Mode::Extended => {
            let headers = [
                "CPU", "NODE", "SOCKET", "CORE", "L1d:L1i:L2:L3", "ONLINE", "MAXMHZ", "MINMHZ",
                "MHZ",
            ];
            let mut rows: Vec<Vec<String>> = Vec::new();
            for c in &selected {
                let core = cores.iter().position(|k| *k == (c.package, c.core)).unwrap_or(0);
                let sock = pkgs.iter().position(|k| *k == c.package).unwrap_or(0);
                let node = node_of(c.id).map(|n| n.to_string()).unwrap_or_default();
                let caches: Vec<String> = (0..4)
                    .map(|k| {
                        cache_id_of(&mut cache_ids, c, k)
                            .map(|v| v.to_string())
                            .unwrap_or_default()
                    })
                    .collect();
                let mhz = |f: &str| freq(c.id, f).unwrap_or_default();
                rows.push(vec![
                    c.id.to_string(),
                    node,
                    sock.to_string(),
                    core.to_string(),
                    caches.join(":"),
                    if c.online { "yes" } else { "no" }.to_string(),
                    mhz("cpuinfo_max_freq"),
                    mhz("cpuinfo_min_freq"),
                    mhz("scaling_cur_freq"),
                ]);
            }
            if json {
                out.push_str("{\n   \"cpus\": [");
                for (n, r) in rows.iter().enumerate() {
                    out.push_str(if n == 0 { "\n" } else { ",\n" });
                    out.push_str("      {\n");
                    for (k, h) in headers.iter().enumerate() {
                        let key = h.to_ascii_lowercase();
                        let v = &r[k];
                        let rendered = if *h == "ONLINE" {
                            if v == "yes" { "\"yes\"".into() } else { "\"no\"".into() }
                        } else if v.is_empty() {
                            "null".to_string()
                        } else if matches!(*h, "CPU" | "NODE" | "SOCKET" | "CORE") {
                            v.clone()
                        } else {
                            format!("\"{}\"", json_escape(v))
                        };
                        out.push_str(&format!("         \"{key}\": {rendered}"));
                        out.push_str(if k + 1 < headers.len() { ",\n" } else { "\n" });
                    }
                    out.push_str("      }");
                }
                out.push_str("\n   ]\n}\n");
            } else {
                let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
                for r in &rows {
                    for (k, v) in r.iter().enumerate() {
                        widths[k] = widths[k].max(v.len());
                    }
                }
                let line = |cells: Vec<&str>| -> String {
                    let mut l = String::new();
                    for (k, cell) in cells.iter().enumerate() {
                        if k > 0 {
                            l.push(' ');
                        }
                        let pad = widths[k].saturating_sub(cell.len());
                        if k == 4 {
                            l.push_str(cell);
                            if k + 1 < cells.len() {
                                l.push_str(&" ".repeat(pad));
                            }
                        } else {
                            l.push_str(&" ".repeat(pad));
                            l.push_str(cell);
                        }
                    }
                    l.push('\n');
                    l
                };
                out.push_str(&line(headers.to_vec()));
                for r in &rows {
                    out.push_str(&line(r.iter().map(|s| s.as_str()).collect()));
                }
            }
        }
        Mode::Default => {
            let mut items: Vec<(String, String)> = Vec::new();
            let flags = fld("flags").unwrap_or_default();
            let has = |f: &str| flags.split_whitespace().any(|x| x == f);
            let x86_64 = has("lm");
            items.push((
                "Architecture:".into(),
                if x86_64 { "x86_64" } else { "i686" }.into(),
            ));
            items.push((
                "CPU op-mode(s):".into(),
                if x86_64 { "32-bit, 64-bit" } else { "32-bit" }.into(),
            ));
            if let Some(a) = fld("address sizes") {
                let a = a.replace(" bits physical,", " bits physical,");
                let parts: Vec<&str> = a.split(',').map(|s| s.trim()).collect();
                let mut phys = String::new();
                let mut virt = String::new();
                for p in &parts {
                    if p.contains("physical") {
                        phys = p.replace(" physical", "");
                    } else if p.contains("virtual") {
                        virt = p.replace(" virtual", "");
                    }
                }
                items.push((
                    "Address sizes:".into(),
                    format!("{phys} physical, {virt} virtual"),
                ));
            }
            items.push(("Byte Order:".into(), "Little Endian".into()));
            items.push(("CPU(s):".into(), cpus.len().to_string()));
            let on: Vec<u32> = cpus.iter().filter(|c| c.online).map(|c| c.id).collect();
            let off: Vec<u32> = cpus.iter().filter(|c| !c.online).map(|c| c.id).collect();
            items.push(("On-line CPU(s) list:".into(), fmt_list(&on)));
            if !off.is_empty() {
                items.push(("Off-line CPU(s) list:".into(), fmt_list(&off)));
            }
            if let Some(v) = fld("vendor_id") {
                items.push(("Vendor ID:".into(), v));
            }
            if let Some(v) = fld("model name") {
                items.push(("Model name:".into(), v));
            }
            if let Some(v) = fld("cpu family") {
                items.push(("CPU family:".into(), v));
            }
            if let Some(v) = fld("model") {
                items.push(("Model:".into(), v));
            }
            let nsock = pkgs.len().max(1);
            let ncores = cores.len().max(1);
            let nthreads = cpus.len().max(1);
            items.push((
                "Thread(s) per core:".into(),
                (nthreads / ncores).max(1).to_string(),
            ));
            items.push((
                "Core(s) per socket:".into(),
                (ncores / nsock).max(1).to_string(),
            ));
            items.push(("Socket(s):".into(), nsock.to_string()));
            if let Some(v) = fld("stepping") {
                items.push(("Stepping:".into(), v));
            }
            if let Some(c0) = cpus.first() {
                if let Some(v) = freq(c0.id, "cpuinfo_max_freq") {
                    items.push(("CPU max MHz:".into(), v));
                }
                if let Some(v) = freq(c0.id, "cpuinfo_min_freq") {
                    items.push(("CPU min MHz:".into(), v));
                }
            }
            if let Some(v) = fld("bogomips") {
                items.push(("BogoMIPS:".into(), v));
            }
            if !flags.is_empty() {
                items.push(("Flags:".into(), flags.clone()));
            }
            if has("vmx") {
                items.push(("Virtualization:".into(), "VT-x".into()));
            } else if has("svm") {
                items.push(("Virtualization:".into(), "AMD-V".into()));
            }
            for k in 0..4 {
                let (lvl, kind, label) = cache_kinds[k];
                let mut seen: Vec<String> = Vec::new();
                let mut one = 0;
                for c in &cpus {
                    if let Some(ci) = c.caches.iter().find(|ci| ci.level == lvl && ci.kind == kind)
                    {
                        one = ci.size;
                        first_seen(&mut seen, &ci.shared);
                    }
                }
                if seen.is_empty() {
                    continue;
                }
                let total = one * seen.len() as u64;
                let sz = if bytes { total.to_string() } else { size_to_human_string(total, false, false) };
                items.push((
                    format!("{label} cache:"),
                    format!(
                        "{sz} ({} instance{})",
                        seen.len(),
                        if seen.len() == 1 { "" } else { "s" }
                    ),
                ));
            }
            if !nodes.is_empty() {
                items.push(("NUMA node(s):".into(), nodes.len().to_string()));
                for (n, l) in nodes.iter().enumerate() {
                    items.push((format!("NUMA node{n} CPU(s):"), fmt_list(l)));
                }
            }
            let mut vulns: Vec<(String, String)> = Vec::new();
            for name in [
                "gather_data_sampling", "ghostwrite", "indirect_target_selection", "itlb_multihit",
                "l1tf", "mds", "meltdown", "mmio_stale_data", "old_microcode",
                "reg_file_data_sampling", "retbleed", "spec_rstack_overflow", "spec_store_bypass",
                "spectre_v1", "spectre_v2", "srbds", "tsx_async_abort", "vmscape",
            ] {
                if let Some(v) = read_text(&format!("{root}{CPU_DIR}/vulnerabilities/{name}")) {
                    let mut label = name.replace('_', " ");
                    if let Some(f) = label.get_mut(0..1) {
                        f.make_ascii_uppercase();
                    }
                    vulns.push((format!("Vulnerability {label}:"), v));
                }
            }
            items.extend(vulns);

            if json {
                out.push_str("{\n   \"lscpu\": [");
                for (n, (k, v)) in items.iter().enumerate() {
                    out.push_str(if n == 0 { "\n" } else { ",\n" });
                    out.push_str(&format!(
                        "      {{\n         \"field\": \"{}\",\n         \"data\": \"{}\"\n      }}",
                        json_escape(k),
                        json_escape(v)
                    ));
                }
                out.push_str("\n   ]\n}\n");
            } else {
                let w = items.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0) + 1;
                for (k, v) in &items {
                    out.push_str(&format!("{k:<w$}{v}\n"));
                }
            }
        }
    }

    let mut so = io::stdout();
    let _ = so.write_all(out.as_bytes());
    0
}

pub fn main(_ctx: &mut sysabi::Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}
