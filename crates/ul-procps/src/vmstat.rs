//! `vmstat` do procps-ng 4.0.4.
//!
//! Modos (`statMode` do original): a tabela padrão com cabeçalho e linhas repetidas por `delay` e
//! `count`, `-a` (memória ativa e inativa), `-w` (largo), `-t` (carimbo de hora), `-n` (cabeçalho só
//! uma vez), `-S` (unidade), `-s` (contadores), `-f` (forks), `-d` e `-D` (disco), `-p` (partição) e
//! `-m` (slabs). Combinar dois modos cai no uso, como no original.
//!
//! Os números vêm de `/proc/stat`, `/proc/meminfo`, `/proc/vmstat`, `/proc/diskstats` e
//! `/proc/slabinfo`. A primeira linha da tabela são médias desde o boot; as seguintes são a diferença
//! entre duas leituras dividida pelo `delay`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::time::Duration;

use sysabi::{Ctx, Errno, sys};
use ul_misc::util::getopt::{Getopt, HasArg, LongOpt};
use ul_misc::util::{io, time};

use crate::common::{self, Strtol, out};
use crate::procfs;

const USAGE: &str = "\nUsage:\n vmstat [options] [delay [count]]\n\nOptions:\n -a, --active           active/inactive memory\n -f, --forks            number of forks since boot\n -m, --slabs            slabinfo\n -n, --one-header       do not redisplay header\n -s, --stats            event counter statistics\n -d, --disk             disk statistics\n -D, --disk-sum         summarize disk statistics\n -p, --partition <dev>  partition specific statistics\n -S, --unit <char>      define display unit\n -w, --wide             wide output\n -t, --timestamp        show timestamp\n -y, --no-first         skips first line of output\n\n -h, --help     display this help and exit\n -V, --version  output version information and exit\n\nFor more details see vmstat(8).\n";

const LONGS: &[LongOpt] = &[
    LongOpt::new("active", HasArg::No, 'a' as i32),
    LongOpt::new("forks", HasArg::No, 'f' as i32),
    LongOpt::new("slabs", HasArg::No, 'm' as i32),
    LongOpt::new("one-header", HasArg::No, 'n' as i32),
    LongOpt::new("timestamp", HasArg::No, 't' as i32),
    LongOpt::new("disk", HasArg::No, 'd' as i32),
    LongOpt::new("disk-sum", HasArg::No, 'D' as i32),
    LongOpt::new("partition", HasArg::Required, 'p' as i32),
    LongOpt::new("stats", HasArg::No, 's' as i32),
    LongOpt::new("unit", HasArg::Required, 'S' as i32),
    LongOpt::new("wide", HasArg::No, 'w' as i32),
    LongOpt::new("no-first", HasArg::No, 'y' as i32),
    LongOpt::new("help", HasArg::No, 'h' as i32),
    LongOpt::new("version", HasArg::No, 'V' as i32),
];

const VMSUMSTAT: u32 = 0x01;
const SLABSTAT: u32 = 0x02;
const DISKSTAT: u32 = 0x04;
const PARTITIONSTAT: u32 = 0x08;
const DISKSUMSTAT: u32 = 0x10;
const FORKSTAT: u32 = 0x20;

/// `sysconf(_SC_CLK_TCK)`.
const HZ: u64 = procfs::HZ;
/// `winhi() - 3` com a janela padrão de 24 linhas (saída que não é terminal).
const ROWS: u64 = 21;

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

/// Unidade de exibição da memória (`-S`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unit {
    Bytes,
    Kilo,
    Kibi,
    Mega,
    Mebi,
}

impl Unit {
    /// Rótulo do `-s`.
    fn label(self) -> &'static str {
        match self {
            Unit::Bytes => "B",
            Unit::Kilo => "k",
            Unit::Kibi => "K",
            Unit::Mega => "m",
            Unit::Mebi => "M",
        }
    }

    /// Converte um valor em KiB.
    fn convert(self, kb: u64) -> u64 {
        match self {
            Unit::Bytes => kb.saturating_mul(1024),
            Unit::Kilo => kb.saturating_mul(1024) / 1000,
            Unit::Kibi => kb,
            Unit::Mega => kb.saturating_mul(1024) / 1_000_000,
            Unit::Mebi => kb / 1024,
        }
    }
}

struct Opts {
    unit: Unit,
    active: bool,
    wide: bool,
    timestamp: bool,
}

/// Memória em KiB, como o vmstat a consome.
#[derive(Default)]
struct Mem {
    total: u64,
    used: u64,
    free: u64,
    buffers: u64,
    cached: u64,
    active: u64,
    inactive: u64,
    swap_total: u64,
    swap_free: u64,
}

fn read_mem() -> Mem {
    let Some(m) = procfs::meminfo() else { return Mem::default() };
    let total = m.get("MemTotal");
    let free = m.get("MemFree");
    let available = if m.map.contains_key("MemAvailable") { m.get("MemAvailable") } else { free };
    let used = if available <= total { total - available } else { total.saturating_sub(free) };
    Mem {
        total,
        used,
        free,
        buffers: m.get("Buffers"),
        cached: m.get("Cached") + m.get("SReclaimable"),
        active: m.get("Active"),
        inactive: m.get("Inactive"),
        swap_total: m.get("SwapTotal"),
        swap_free: m.get("SwapFree"),
    }
}

/// Ticks de CPU de `/proc/stat`.
#[derive(Clone, Copy, Default)]
struct Cpu {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
    steal: u64,
    guest: u64,
    guest_nice: u64,
}

/// Uma leitura de `/proc/stat` e `/proc/vmstat`.
#[derive(Clone, Default)]
struct Snap {
    cpu: Cpu,
    intr: u64,
    ctxt: u64,
    btime: u64,
    forks: u64,
    running: u64,
    blocked: u64,
    vm: BTreeMap<String, u64>,
}

impl Snap {
    fn vm(&self, key: &str) -> u64 {
        self.vm.get(key).copied().unwrap_or(0)
    }
}

fn snapshot() -> Snap {
    let mut s = Snap::default();
    if let Some(data) = procfs::read("/proc/stat") {
        for line in String::from_utf8_lossy(&data).lines() {
            let mut it = line.split_ascii_whitespace();
            let Some(key) = it.next() else { continue };
            let nums: Vec<u64> = it.filter_map(|x| x.parse().ok()).collect();
            let n = |i: usize| nums.get(i).copied().unwrap_or(0);
            match key {
                "cpu" => {
                    s.cpu = Cpu {
                        user: n(0),
                        nice: n(1),
                        system: n(2),
                        idle: n(3),
                        iowait: n(4),
                        irq: n(5),
                        softirq: n(6),
                        steal: n(7),
                        guest: n(8),
                        guest_nice: n(9),
                    }
                }
                "intr" => s.intr = n(0),
                "ctxt" => s.ctxt = n(0),
                "btime" => s.btime = n(0),
                "processes" => s.forks = n(0),
                "procs_running" => s.running = n(0),
                "procs_blocked" => s.blocked = n(0),
                _ => {}
            }
        }
    }
    if let Some(data) = procfs::read("/proc/vmstat") {
        for line in String::from_utf8_lossy(&data).lines() {
            if let Some((k, v)) = line.split_once(' ')
                && let Ok(v) = v.trim().parse::<u64>()
            {
                s.vm.insert(k.to_string(), v);
            }
        }
    }
    s
}

/// Hora local ` AAAA-MM-DD HH:MM:SS`, com o espaço na frente.
fn stamp() -> String {
    let tz = time::local_tz();
    let (now, _) = procfs::now_realtime();
    let dt = time::civil(now, &tz);
    format!(" {:04}-{:02}-{:02} {:02}:{:02}:{:02}", dt.year(), dt.month(), dt.day(), dt.hour(), dt.minute(), dt.second())
}

/// Abreviação do fuso alinhada à direita na largura do cabeçalho do carimbo.
fn tz_column() -> String {
    let tz = time::local_tz();
    let (now, _) = procfs::now_realtime();
    format!(" {:>19}", time::abbreviation(now, &tz))
}

fn header(o: &Opts) -> String {
    let mut s = String::new();
    if o.wide {
        s.push_str("--procs-- -----------------------memory---------------------- ---swap-- -----io---- -system-- ----------cpu----------");
    } else {
        s.push_str("procs -----------memory---------- ---swap-- -----io---- -system-- -------cpu-------");
    }
    if o.timestamp {
        s.push_str(" -----timestamp-----");
    }
    s.push('\n');
    let (m3, m4) = if o.active { ("inact", "active") } else { ("buff", "cache") };
    let line = if o.wide {
        format!(
            "{:>4} {:>4} {:>12} {:>12} {:>12} {:>12} {:>4} {:>4} {:>5} {:>5} {:>4} {:>4} {:>3} {:>3} {:>3} {:>3} {:>3} {:>3}",
            "r", "b", "swpd", "free", m3, m4, "si", "so", "bi", "bo", "in", "cs", "us", "sy", "id", "wa", "st", "gu"
        )
    } else {
        format!(
            "{:>2} {:>2} {:>6} {:>6} {:>6} {:>6} {:>4} {:>4} {:>5} {:>5} {:>4} {:>4} {:>2} {:>2} {:>2} {:>2} {:>2} {:>2}",
            "r", "b", "swpd", "free", m3, m4, "si", "so", "bi", "bo", "in", "cs", "us", "sy", "id", "wa", "st", "gu"
        )
    };
    s.push_str(&line);
    if o.timestamp {
        s.push_str(&tz_column());
    }
    s.push('\n');
    s
}

/// Os cinco números de memória da tabela.
fn mem_columns(o: &Opts, m: &Mem) -> [u64; 4] {
    let c = |v: u64| o.unit.convert(v);
    let swpd = c(m.swap_total.saturating_sub(m.swap_free));
    if o.active {
        [swpd, c(m.free), c(m.inactive), c(m.active)]
    } else {
        [swpd, c(m.free), c(m.buffers), c(m.cached)]
    }
}

/// Uma linha da tabela. `rates` são `si so bi bo in cs`, `cpu` são `us sy id wa st gu` em porcento.
fn line(o: &Opts, running: u64, blocked: u64, mem: [u64; 4], rates: [u64; 6], cpu: [u64; 6]) -> String {
    let mut s = if o.wide {
        format!(
            "{:4} {:4} {:12} {:12} {:12} {:12} {:4} {:4} {:5} {:5} {:4} {:4} {:3} {:3} {:3} {:3} {:3} {:3}",
            running, blocked, mem[0], mem[1], mem[2], mem[3], rates[0], rates[1], rates[2], rates[3], rates[4], rates[5], cpu[0], cpu[1], cpu[2], cpu[3], cpu[4], cpu[5]
        )
    } else {
        format!(
            "{:2} {:2} {:6} {:6} {:6} {:6} {:4} {:4} {:5} {:5} {:4} {:4} {:2} {:2} {:2} {:2} {:2} {:2}",
            running, blocked, mem[0], mem[1], mem[2], mem[3], rates[0], rates[1], rates[2], rates[3], rates[4], rates[5], cpu[0], cpu[1], cpu[2], cpu[3], cpu[4], cpu[5]
        )
    };
    if o.timestamp {
        s.push_str(&stamp());
    }
    s.push('\n');
    s
}

/// Porcentagens de CPU de um intervalo de ticks, e o divisor (`Div`) pra reaproveitar nas taxas.
fn cpu_split(d: &Cpu) -> (u64, [u64; 6]) {
    let duse = d.user + d.nice;
    let dsys = d.system + d.irq + d.softirq;
    let mut didl = d.idle;
    let diow = d.iowait;
    let dstl = d.steal;
    let dgue = d.guest + d.guest_nice;
    let mut div = duse + dsys + didl + diow + dstl;
    if div == 0 {
        div = 1;
        didl = 1;
    }
    let half = div / 2;
    let pct = |v: u64| (100 * v + half) / div;
    (div, [pct(duse), pct(dsys), pct(didl), pct(diow), pct(dstl), pct(dgue)])
}

fn cpu_diff(a: &Cpu, b: &Cpu) -> Cpu {
    let d = |x: u64, y: u64| x.saturating_sub(y);
    Cpu {
        user: d(a.user, b.user),
        nice: d(a.nice, b.nice),
        system: d(a.system, b.system),
        idle: d(a.idle, b.idle),
        iowait: d(a.iowait, b.iowait),
        irq: d(a.irq, b.irq),
        softirq: d(a.softirq, b.softirq),
        steal: d(a.steal, b.steal),
        guest: d(a.guest, b.guest),
        guest_nice: d(a.guest_nice, b.guest_nice),
    }
}

/// Linha desde o boot: taxas em cima do tempo de CPU acumulado.
fn since_boot_line(o: &Opts, s: &Snap, m: &Mem) -> String {
    let (div, cpu) = cpu_split(&s.cpu);
    let half = div / 2;
    let rate = |v: u64| (v * HZ + half) / div;
    let swap = |v: u64| (v * o.unit.convert(4) * HZ + half) / div;
    let rates = [swap(s.vm("pswpin")), swap(s.vm("pswpout")), rate(s.vm("pgpgin")), rate(s.vm("pgpgout")), rate(s.intr), rate(s.ctxt)];
    line(o, s.running, s.blocked, mem_columns(o, m), rates, cpu)
}

/// Linha de um intervalo de `secs` segundos entre duas leituras.
fn delta_line(o: &Opts, prev: &Snap, cur: &Snap, m: &Mem, secs: u64) -> String {
    let (_, cpu) = cpu_split(&cpu_diff(&cur.cpu, &prev.cpu));
    let half = secs / 2;
    let d = |a: u64, b: u64| a.saturating_sub(b);
    let rate = |v: u64| (v + half) / secs;
    let swap = |v: u64| (v * o.unit.convert(4) + half) / secs;
    let rates = [
        swap(d(cur.vm("pswpin"), prev.vm("pswpin"))),
        swap(d(cur.vm("pswpout"), prev.vm("pswpout"))),
        rate(d(cur.vm("pgpgin"), prev.vm("pgpgin"))),
        rate(d(cur.vm("pgpgout"), prev.vm("pgpgout"))),
        rate(d(cur.intr, prev.intr)),
        rate(d(cur.ctxt, prev.ctxt)),
    ];
    line(o, cur.running, cur.blocked, mem_columns(o, m), rates, cpu)
}

struct Loop {
    /// Segundos entre linhas; `None` sem `delay`.
    delay: Option<u64>,
    count: i64,
    /// `delay` sem `count`: repete até o fim do processo.
    infinite: bool,
    one_header: bool,
    /// `-y`: não imprime a linha de médias desde o boot.
    no_first: bool,
}

fn sleep_for(secs: u64) {
    let _ = io::flush_stdout();
    let _ = sys::current().nanosleep(Duration::from_secs(secs));
}

fn table(o: &Opts, lp: &Loop) -> i32 {
    out(header(o));
    let first = snapshot();
    if !lp.no_first {
        out(since_boot_line(o, &first, &read_mem()));
    }
    let Some(delay) = lp.delay else { return 0 };
    let mut prev = first;
    let mut i: i64 = 1;
    while lp.infinite || i < lp.count {
        sleep_for(delay);
        if !lp.one_header && (i as u64).is_multiple_of(ROWS) {
            out(header(o));
        }
        let cur = snapshot();
        out(delta_line(o, &prev, &cur, &read_mem(), delay));
        prev = cur;
        i += 1;
    }
    0
}

fn sum_stat(o: &Opts) -> i32 {
    let s = snapshot();
    let m = read_mem();
    let u = o.unit;
    let l = u.label();
    let mem = |v: u64, what: &str| out(format!("{:13} {l} {what}\n", u.convert(v)));
    mem(m.total, "total memory");
    mem(m.used, "used memory");
    mem(m.active, "active memory");
    mem(m.inactive, "inactive memory");
    mem(m.free, "free memory");
    mem(m.buffers, "buffer memory");
    mem(m.cached, "swap cache");
    mem(m.swap_total, "total swap");
    mem(m.swap_total.saturating_sub(m.swap_free), "used swap");
    mem(m.swap_free, "free swap");
    let c = s.cpu;
    let n = |v: u64, what: &str| out(format!("{v:13} {what}\n"));
    n(c.user, "non-nice user cpu ticks");
    n(c.nice, "nice user cpu ticks");
    n(c.system, "system cpu ticks");
    n(c.idle, "idle cpu ticks");
    n(c.iowait, "IO-wait cpu ticks");
    n(c.irq, "IRQ cpu ticks");
    n(c.softirq, "softirq cpu ticks");
    n(c.steal, "stolen cpu ticks");
    n(c.guest, "non-nice guest cpu ticks");
    n(c.guest_nice, "nice guest cpu ticks");
    out(format!("{:13} K paged in\n", s.vm("pgpgin")));
    out(format!("{:13} K paged out\n", s.vm("pgpgout")));
    n(s.vm("pswpin"), "pages swapped in");
    n(s.vm("pswpout"), "pages swapped out");
    n(s.intr, "interrupts");
    n(s.ctxt, "CPU context switches");
    n(s.btime, "boot time");
    n(s.forks, "forks");
    0
}

/// Uma linha de `/proc/diskstats`.
struct Disk {
    name: String,
    f: [u64; 11],
}

fn read_disks() -> Vec<Disk> {
    let Some(data) = procfs::read("/proc/diskstats") else { return Vec::new() };
    let mut v = Vec::new();
    for line in String::from_utf8_lossy(&data).lines() {
        let t: Vec<&str> = line.split_ascii_whitespace().collect();
        if t.len() < 14 {
            continue;
        }
        let mut f = [0u64; 11];
        for (i, x) in t[3..14].iter().enumerate() {
            f[i] = x.parse().unwrap_or(0);
        }
        v.push(Disk { name: t[2].to_string(), f });
    }
    v
}

/// Partição e não disco: o nome termina em dígito, fora `loop`, `ram` e os `nvme0n1` sem `p`.
fn is_partition(name: &str) -> bool {
    if !name.ends_with(|c: char| c.is_ascii_digit()) {
        return false;
    }
    if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("zram") || name.starts_with("nbd") {
        return false;
    }
    if name.starts_with("nvme") || name.starts_with("mmcblk") {
        return name.rfind('p').is_some_and(|i| i > 0 && name[i + 1..].chars().all(|c| c.is_ascii_digit()) && name[..i].ends_with(|c: char| c.is_ascii_digit()));
    }
    true
}

fn disk_header(o: &Opts) -> String {
    let mut s = String::from("disk- ------------reads------------ ------------writes----------- -----IO------");
    if o.timestamp {
        s.push_str(" -----timestamp-----");
    }
    s.push_str("\n       total merged sectors      ms  total merged sectors      ms    cur    sec");
    if o.timestamp {
        s.push_str(&tz_column());
    }
    s.push('\n');
    s
}

fn disk_stat(o: &Opts, lp: &Loop) -> i32 {
    out(disk_header(o));
    let mut i: i64 = 0;
    loop {
        for d in read_disks().iter().filter(|d| !is_partition(&d.name)) {
            let f = &d.f;
            let mut row = format!(
                "{:<5} {:6} {:6} {:7} {:7} {:6} {:6} {:7} {:7} {:6} {:6}",
                d.name,
                f[0] as u32,
                f[1] as u32,
                f[2],
                f[3] as u32,
                f[4] as u32,
                f[5] as u32,
                f[6],
                f[7] as u32,
                (f[8] / 1000) as u32,
                (f[9] / 1000) as u32
            );
            if o.timestamp {
                row.push_str(&stamp());
            }
            row.push('\n');
            out(row);
        }
        i += 1;
        let Some(delay) = lp.delay else { break };
        if !lp.infinite && i >= lp.count {
            break;
        }
        sleep_for(delay);
    }
    0
}

fn disk_sum() -> i32 {
    let disks = read_disks();
    let ndisks = disks.iter().filter(|d| !is_partition(&d.name)).count();
    let nparts = disks.len() - ndisks;
    let mut t = [0u64; 11];
    for d in disks.iter().filter(|d| !is_partition(&d.name)) {
        for (a, b) in t.iter_mut().zip(d.f.iter()) {
            *a += *b;
        }
    }
    out(format!("{ndisks:13} disks\n"));
    out(format!("{nparts:13} partitions\n"));
    out(format!("{:13} total reads\n", t[0] as u32));
    out(format!("{:13} merged reads\n", t[1] as u32));
    out(format!("{:13} read sectors\n", t[2]));
    out(format!("{:13} milli reading\n", t[3] as u32));
    out(format!("{:13} writes\n", t[4] as u32));
    out(format!("{:13} merged writes\n", t[5] as u32));
    out(format!("{:13} written sectors\n", t[6]));
    out(format!("{:13} milli writing\n", t[7] as u32));
    out(format!("{:13} inprogress IO\n", (t[8] / 1000) as u32));
    out(format!("{:13} milli spent IO\n", t[9] as u32));
    out(format!("{:13} milli weighted IO\n", t[10] as u32));
    0
}

fn partition_stat(name: &str) -> i32 {
    let disks = read_disks();
    let Some(d) = disks.iter().find(|d| d.name == name && is_partition(&d.name)) else {
        common::warn("vmstat", "Partition was not found");
        return 1;
    };
    out(format!("{name:<10} reads   read sectors  writes    requested writes\n"));
    out(format!("{:>10} {:>10} {:>10} {:>10} {:>10}\n", "", d.f[0], d.f[2], d.f[4], d.f[6]));
    0
}

fn slab_stat() -> i32 {
    let Some(data) = procfs::read("/proc/slabinfo") else {
        common::warn("vmstat", &format!("Unable to create slabinfo structure: {}", Errno::EACCES.message()));
        return 1;
    };
    out(format!("{:<24} {:>6} {:>6} {:>6} {:>6}\n", "Cache", "Num", "Total", "Size", "Pages"));
    for line in String::from_utf8_lossy(&data).lines() {
        if line.starts_with('#') || line.starts_with("slabinfo") {
            continue;
        }
        let t: Vec<&str> = line.split_ascii_whitespace().collect();
        if t.len() < 6 {
            continue;
        }
        let n = |i: usize| t[i].parse::<u64>().unwrap_or(0);
        let name: String = t[0].chars().take(24).collect();
        out(format!("{:<24} {:6} {:6} {:6} {:6}\n", name, n(1), n(2), n(3), n(5)));
    }
    0
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let mut o = Opts { unit: Unit::Kibi, active: false, wide: false, timestamp: false };
    let mut mode = 0u32;
    let mut one_header = false;
    let mut no_first = false;
    let mut partition = String::new();
    let mut g = Getopt::from_env(&argv[1..], "afmnsdDp:S:hVwty", LONGS);
    while let Some(r) = g.next_opt() {
        let opt = match r {
            Ok(opt) => opt,
            Err(e) => {
                io::eprint(format!("{}\n{USAGE}", e.message(&argv0)));
                return 1;
            }
        };
        match opt.short() {
            Some('V') => {
                out("vmstat from procps-ng 4.0.4\n");
                return 0;
            }
            Some('h') => {
                out(USAGE);
                return 0;
            }
            Some('d') => mode = DISKSTAT,
            Some('a') => o.active = true,
            Some('f') => mode = FORKSTAT,
            Some('m') => mode = SLABSTAT,
            Some('D') => mode = DISKSUMSTAT,
            Some('n') => one_header = true,
            Some('y') => no_first = true,
            Some('p') => {
                mode = PARTITIONSTAT;
                let a = opt.arg_str();
                partition = a.strip_prefix("/dev/").unwrap_or(&a).to_string();
            }
            Some('s') => mode = VMSUMSTAT,
            Some('S') => {
                let a = opt.arg_str();
                let mut it = a.chars();
                let unit = match (it.next(), it.next()) {
                    (Some('b' | 'B'), _) => Some(Unit::Bytes),
                    (Some('k'), _) => Some(Unit::Kilo),
                    (Some('K'), _) => Some(Unit::Kibi),
                    (Some('m'), _) => Some(Unit::Mega),
                    (Some('M'), _) => Some(Unit::Mebi),
                    _ => None,
                };
                match unit {
                    Some(u) => o.unit = u,
                    None => {
                        common::warn("vmstat", "-S requires k, K, m or M (default is KiB)");
                        return 1;
                    }
                }
            }
            Some('w') => o.wide = true,
            Some('t') => o.timestamp = true,
            _ => unreachable!("tabela de opções do vmstat"),
        }
    }
    let operands = g.operands().to_vec();
    let parse = |s: &str| -> Result<i64, i32> {
        match common::strtol(s) {
            Strtol::Ok(v) => Ok(v),
            Strtol::Invalid => {
                common::warn("vmstat", &format!("failed to parse argument: '{s}'"));
                Err(1)
            }
            Strtol::Range(_) => {
                common::warn("vmstat", &format!("failed to parse argument: '{s}': {}", Errno::ERANGE.message()));
                Err(1)
            }
        }
    };
    let mut delay: Option<u64> = None;
    let mut count: i64 = 1;
    let mut infinite = false;
    let mut idx = 0;
    if idx < operands.len() {
        let a = String::from_utf8_lossy(&operands[idx]).into_owned();
        idx += 1;
        let v = match parse(&a) {
            Ok(v) => v,
            Err(c) => return c,
        };
        if v < 1 {
            common::warn("vmstat", "delay must be positive integer");
            return 1;
        }
        if v > i64::from(u32::MAX) {
            common::warn("vmstat", "too large delay value");
            return 1;
        }
        delay = Some(v as u64);
        infinite = true;
    }
    if idx < operands.len() {
        let a = String::from_utf8_lossy(&operands[idx]).into_owned();
        idx += 1;
        count = match parse(&a) {
            Ok(v) => v,
            Err(c) => return c,
        };
        infinite = false;
    }
    if idx < operands.len() {
        io::eprint(USAGE);
        return 1;
    }
    let lp = Loop { delay, count, infinite, one_header, no_first };
    match mode {
        0 => table(&o, &lp),
        VMSUMSTAT => sum_stat(&o),
        FORKSTAT => {
            out(format!("{:13} forks\n", snapshot().forks));
            0
        }
        SLABSTAT => slab_stat(),
        DISKSTAT => disk_stat(&o, &lp),
        PARTITIONSTAT => partition_stat(&partition),
        DISKSUMSTAT => disk_sum(),
        _ => {
            io::eprint(USAGE);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Opts {
        Opts { unit: Unit::Kibi, active: false, wide: false, timestamp: false }
    }

    #[test]
    fn header_columns_line_up_with_the_rows() {
        let h = header(&opts());
        let lines: Vec<&str> = h.lines().collect();
        assert_eq!(lines[0], "procs -----------memory---------- ---swap-- -----io---- -system-- -------cpu-------");
        assert_eq!(lines[1], " r  b   swpd   free   buff  cache   si   so    bi    bo   in   cs us sy id wa st gu");
        // Valores que cabem nas colunas (o procps deixa número largo estourar, sem realinhar).
        let row = line(&opts(), 1, 0, [0, 913_284, 62_228, 100], [0; 6], [3, 1, 96, 0, 0, 0]);
        assert_eq!(row.len(), lines[1].len() + 1);
    }

    #[test]
    fn wide_header_matches_the_wide_row_width() {
        let o = Opts { wide: true, ..opts() };
        let h = header(&o);
        let lines: Vec<&str> = h.lines().collect();
        assert_eq!(lines[0].len(), lines[1].len());
        let row = line(&o, 1, 0, [0; 4], [0; 6], [0; 6]);
        assert_eq!(row.len(), lines[1].len() + 1);
    }

    #[test]
    fn units_convert_from_kib() {
        assert_eq!(Unit::Mebi.convert(2048), 2);
        assert_eq!(Unit::Kilo.convert(1000), 1024);
        assert_eq!(Unit::Bytes.convert(1), 1024);
        assert_eq!(Unit::Mega.convert(1_000_000), 1024);
    }

    #[test]
    fn cpu_percentages_round_and_survive_an_idle_interval() {
        let (div, p) = cpu_split(&Cpu { user: 1, idle: 3, ..Cpu::default() });
        assert_eq!(div, 4);
        assert_eq!(p, [25, 0, 75, 0, 0, 0]);
        let (div, p) = cpu_split(&Cpu::default());
        assert_eq!(div, 1);
        assert_eq!(p[2], 100);
    }

    #[test]
    fn partitions_are_told_from_disks() {
        assert!(is_partition("sda1"));
        assert!(!is_partition("sda"));
        assert!(!is_partition("loop0"));
        assert!(!is_partition("nvme0n1"));
        assert!(is_partition("nvme0n1p2"));
    }
}
