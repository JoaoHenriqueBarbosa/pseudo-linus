//! Os formatos de texto do procfs, tirados do kernel 6.12 (`fs/proc/array.c`, `base.c`, `meminfo.c`,
//! `stat.c`...) e conferidos contra o oráculo. Cada função recebe os dados já prontos e devolve o conteúdo
//! do arquivo, quebra de linha no fim inclusive.

use std::io::Write as _;

use sysabi::Resource;

use super::data::*;

/// Um tick de `USER_HZ` (100 Hz), em ns.
const USER_TICK_NS: u64 = 10_000_000;
/// `FIXED_1` do `loadavg` (`FSHIFT` = 11).
const FIXED_1: u64 = 1 << 11;
/// Capabilities do root de um container Docker (`CapEff` do oráculo).
const CAPS_ROOT: u64 = 0xa804_25fb;
/// `Mems_allowed` do oráculo: 1024 nós em 32 palavras, só o nó 0 ligado.
const MEMS_ALLOWED: &str = "00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000000,00000001";

fn ticks(ns: u64) -> u64 {
    ns / USER_TICK_NS
}

/// `seq_escape_str(..., ESCAPE_SPACE | ESCAPE_SPECIAL, "\n\\")` do nome na linha `Name:`.
fn escape_comm(c: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(c.len());
    for &b in c {
        match b {
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\\' => out.extend_from_slice(b"\\\\"),
            _ => out.push(b),
        }
    }
    out
}

fn state_text(c: char) -> &'static str {
    match c {
        'R' => "R (running)",
        'S' => "S (sleeping)",
        'D' => "D (disk sleep)",
        'T' => "T (stopped)",
        't' => "t (tracing stop)",
        'Z' => "Z (zombie)",
        'X' => "X (dead)",
        'I' => "I (idle)",
        _ => "S (sleeping)",
    }
}

/// `PF_*` do `task->flags` que o `stat` mostra.
fn task_flags(p: &ProcData) -> u64 {
    const PF_EXITING: u64 = 0x4;
    const PF_POSTCOREDUMP: u64 = 0x8;
    const PF_FORKNOEXEC: u64 = 0x40;
    const PF_SIGNALED: u64 = 0x400;
    const PF_NOFREEZE: u64 = 0x8000;
    const PF_RANDOMIZE: u64 = 0x40_0000;
    let mut f = PF_RANDOMIZE;
    if p.fork_noexec {
        f |= PF_FORKNOEXEC;
    }
    if p.state == 'Z' {
        f |= PF_EXITING | PF_POSTCOREDUMP | PF_NOFREEZE;
    }
    if p.signaled {
        f |= PF_SIGNALED;
    }
    f
}

/// `/proc/<pid>/stat` e `/proc/<pid>/task/<tid>/stat`: os 52 campos do `do_task_stat`. Os endereços do
/// espaço de endereçamento (`startcode`, `startstack`, `arg_start`...) saem 0: o pseudo-processo não tem
/// mapa de memória, é o que o kernel mostra de um processo sem `mm`.
pub(super) fn stat(p: &ProcData) -> Vec<u8> {
    let mut o = Vec::with_capacity(320);
    let _ = write!(o, "{} (", p.tid);
    o.extend_from_slice(&p.comm);
    let (vsize, rss) = p.mem.map_or((0, 0), |m| (m.vm_size * 1024, m.vm_rss / 4));
    let sigmask = 0x7fff_ffff;
    let _ = write!(
        o,
        ") {} {} {} {} 0 -1 {} 0 0 0 0 {} {} {} {} {} {} {} 0 {} {} {} {} 0 0 0 0 0 {} {} {} {} {} 0 0 {} {} 0 0 0 0 0 0 0 0 0 0 0 0 {}\n",
        p.state,
        p.ppid,
        p.pgid,
        p.sid,
        task_flags(p),
        ticks(p.utime_ns),
        ticks(p.stime_ns),
        ticks(p.cutime_ns),
        ticks(p.cstime_ns),
        20 + p.nice,
        p.nice,
        p.num_threads,
        ticks(p.start_ns),
        vsize,
        rss,
        p.rlimits[Resource::Rss as usize].0,
        p.sig.pending & sigmask,
        p.sig.blocked & sigmask,
        p.sig.ignored & sigmask,
        p.sig.caught & sigmask,
        u8::from(p.state != 'R'),
        if p.secondary { -1 } else { 17 },
        p.last_cpu,
        p.exit_code,
    );
    o
}

/// `/proc/<pid>/statm`: tamanho, residente, compartilhado, texto, `lib` (sempre 0), dados mais pilha e
/// `dt` (sempre 0), em páginas de 4 KiB.
pub(super) fn statm(p: &ProcData) -> Vec<u8> {
    match p.mem {
        None => b"0 0 0 0 0 0 0\n".to_vec(),
        Some(m) => format!(
            "{} {} {} {} 0 {} 0\n",
            m.vm_size / 4,
            m.vm_rss / 4,
            (m.rss_file + m.rss_shmem) / 4,
            m.vm_exe / 4,
            (m.vm_data + m.vm_stk) / 4
        )
        .into_bytes(),
    }
}

/// Máscara de CPUs em hexa, em palavras de 32 bits separadas por vírgula (`%*pb`).
fn cpumask_hex(n: u32) -> String {
    let words = n.div_ceil(32).max(1);
    let mut parts = Vec::new();
    for w in (0..words).rev() {
        let bits = if w == words - 1 { n - 32 * w } else { 32 };
        let val: u64 = if bits >= 32 { 0xffff_ffff } else { (1u64 << bits) - 1 };
        if w == words - 1 {
            parts.push(format!("{val:x}"));
        } else {
            parts.push(format!("{val:08x}"));
        }
    }
    parts.join(",")
}

fn cpumask_list(n: u32) -> String {
    if n <= 1 { "0".to_string() } else { format!("0-{}", n - 1) }
}

/// Tamanho de uma linha `Vm*` (`%8lu kB`).
fn vm(o: &mut Vec<u8>, name: &str, kb: u64) {
    let _ = writeln!(o, "{name}:\t{kb:>8} kB");
}

/// `/proc/<pid>/status`, na ordem do 6.12. Um zumbi não tem `Umask` (sem `fs_struct`) nem o bloco de
/// memória (sem `mm`), e `FDSize` é 0.
pub(super) fn status(p: &ProcData) -> Vec<u8> {
    let mut o = Vec::with_capacity(1700);
    o.extend_from_slice(b"Name:\t");
    o.extend_from_slice(&escape_comm(&p.comm));
    o.push(b'\n');
    if p.state != 'Z' {
        let _ = writeln!(o, "Umask:\t{:04o}", p.umask & 0o7777);
    }
    let _ = writeln!(o, "State:\t{}", state_text(p.state));
    let _ = writeln!(o, "Tgid:\t{}\nNgid:\t0\nPid:\t{}\nPPid:\t{}\nTracerPid:\t0", p.pid, p.tid, p.ppid);
    let _ = writeln!(o, "Uid:\t{0}\t{0}\t{0}\t{0}\nGid:\t{1}\t{1}\t{1}\t{1}", p.uid, p.gid);
    let _ = writeln!(o, "FDSize:\t{}", p.fdsize);
    o.extend_from_slice(b"Groups:\t");
    for g in &p.groups {
        let _ = write!(o, "{g} ");
    }
    o.push(b'\n');
    let _ = writeln!(o, "NStgid:\t{}\nNSpid:\t{}\nNSpgid:\t{}\nNSsid:\t{}\nKthread:\t0", p.pid, p.tid, p.pgid, p.sid);
    if let Some(m) = p.mem {
        vm(&mut o, "VmPeak", m.vm_peak);
        vm(&mut o, "VmSize", m.vm_size);
        vm(&mut o, "VmLck", m.vm_lck);
        vm(&mut o, "VmPin", m.vm_pin);
        vm(&mut o, "VmHWM", m.vm_hwm);
        vm(&mut o, "VmRSS", m.vm_rss);
        vm(&mut o, "RssAnon", m.rss_anon);
        vm(&mut o, "RssFile", m.rss_file);
        vm(&mut o, "RssShmem", m.rss_shmem);
        vm(&mut o, "VmData", m.vm_data);
        vm(&mut o, "VmStk", m.vm_stk);
        vm(&mut o, "VmExe", m.vm_exe);
        vm(&mut o, "VmLib", m.vm_lib);
        vm(&mut o, "VmPTE", m.vm_pte);
        vm(&mut o, "VmSwap", m.vm_swap);
        vm(&mut o, "HugetlbPages", 0);
        o.extend_from_slice(b"CoreDumping:\t0\nTHP_enabled:\t1\nuntag_mask:\t0xffffffffffffffff\n");
    }
    let _ = writeln!(o, "Threads:\t{}", p.num_threads);
    let _ = writeln!(o, "SigQ:\t{}/{}", p.sigq, p.rlimits[Resource::Sigpending as usize].0);
    let _ = writeln!(o, "SigPnd:\t{:016x}", p.sig.pending);
    let _ = writeln!(o, "ShdPnd:\t{:016x}", p.sig.shared_pending);
    let _ = writeln!(o, "SigBlk:\t{:016x}", p.sig.blocked);
    let _ = writeln!(o, "SigIgn:\t{:016x}", p.sig.ignored);
    let _ = writeln!(o, "SigCgt:\t{:016x}", p.sig.caught);
    let eff = if p.uid == 0 { CAPS_ROOT } else { 0 };
    let _ = writeln!(o, "CapInh:\t{:016x}", 0);
    let _ = writeln!(o, "CapPrm:\t{eff:016x}");
    let _ = writeln!(o, "CapEff:\t{eff:016x}");
    let _ = writeln!(o, "CapBnd:\t{CAPS_ROOT:016x}");
    let _ = writeln!(o, "CapAmb:\t{:016x}", 0);
    o.extend_from_slice(
        b"NoNewPrivs:\t0\nSeccomp:\t2\nSeccomp_filters:\t1\nSpeculation_Store_Bypass:\tthread vulnerable\nSpeculationIndirectBranch:\tconditional enabled\n",
    );
    let _ = writeln!(o, "Cpus_allowed:\t{}", cpumask_hex(p.ncpus));
    let _ = writeln!(o, "Cpus_allowed_list:\t{}", cpumask_list(p.ncpus));
    let _ = writeln!(o, "Mems_allowed:\t{MEMS_ALLOWED}");
    o.extend_from_slice(b"Mems_allowed_list:\t0\n");
    let _ = writeln!(o, "voluntary_ctxt_switches:\t{}", p.voluntary_ctxt);
    let _ = writeln!(o, "nonvoluntary_ctxt_switches:\t{}", p.nonvoluntary_ctxt);
    o.extend_from_slice(b"x86_Thread_features:\t\nx86_Thread_features_locked:\t\n");
    o
}

/// Nome e unidade de cada `RLIMIT_*`, na ordem dos números.
const LIMIT_NAMES: [(&str, Option<&str>); 16] = [
    ("Max cpu time", Some("seconds")),
    ("Max file size", Some("bytes")),
    ("Max data size", Some("bytes")),
    ("Max stack size", Some("bytes")),
    ("Max core file size", Some("bytes")),
    ("Max resident set", Some("bytes")),
    ("Max processes", Some("processes")),
    ("Max open files", Some("files")),
    ("Max locked memory", Some("bytes")),
    ("Max address space", Some("bytes")),
    ("Max file locks", Some("locks")),
    ("Max pending signals", Some("signals")),
    ("Max msgqueue size", Some("bytes")),
    ("Max nice priority", None),
    ("Max realtime priority", None),
    ("Max realtime timeout", Some("us")),
];

/// `/proc/<pid>/limits` (`proc_pid_limits`).
pub(super) fn limits(p: &ProcData) -> Vec<u8> {
    let mut o = Vec::with_capacity(1100);
    let _ = writeln!(o, "{:<25} {:<20} {:<20} {:<10}", "Limit", "Soft Limit", "Hard Limit", "Units");
    for (i, (name, unit)) in LIMIT_NAMES.iter().enumerate() {
        let (cur, max) = p.rlimits[i];
        let _ = write!(o, "{name:<25} ");
        if cur == u64::MAX {
            let _ = write!(o, "{:<20} ", "unlimited");
        } else {
            let _ = write!(o, "{cur:<20} ");
        }
        if max == u64::MAX {
            let _ = write!(o, "{:<20} ", "unlimited");
        } else {
            let _ = write!(o, "{max:<20} ");
        }
        match unit {
            Some(u) => {
                let _ = writeln!(o, "{u:<10}");
            }
            None => o.push(b'\n'),
        }
    }
    o
}

/// `/proc/<pid>/fdinfo/N`: `pos`, `flags` em octal com um 0 na frente (`0%o`), `mnt_id` e `ino`.
pub(super) fn fdinfo(i: &FdInfo) -> Vec<u8> {
    format!("pos:\t{}\nflags:\t0{:o}\nmnt_id:\t{}\nino:\t{}\n", i.pos, i.flags, i.mnt_id, i.ino).into_bytes()
}

/// `/proc/<pid>/schedstat`: tempo na CPU, tempo esperando na fila (não medido, 0) e trocas de contexto.
pub(super) fn schedstat(p: &ProcData) -> Vec<u8> {
    format!("{} 0 {}\n", p.sched_runtime_ns, p.sched_switches).into_bytes()
}

/// `/proc/<pid>/task/<tid>/children`: cada filho seguido de um espaço, sem quebra de linha.
pub(super) fn children(kids: &[sysabi::Pid]) -> Vec<u8> {
    let mut o = Vec::new();
    for k in kids {
        let _ = write!(o, "{k} ");
    }
    o
}

// ---- globais ----

const CPU_BUGS: &str = "sysret_ss_attrs spectre_v1 spectre_v2 spec_store_bypass srso ibpb_no_ret tsa vmscape";

/// Uma linha do `meminfo`: o nome com dois pontos em 16 colunas, o valor em 8 e ` kB`.
fn mem_line(o: &mut Vec<u8>, name: &str, kb: u64) {
    let _ = writeln!(o, "{:<16}{:>8} kB", format!("{name}:"), kb);
}

/// Uma linha do `meminfo` sem unidade (`HugePages_*`).
fn mem_count(o: &mut Vec<u8>, name: &str, n: u64) {
    let _ = writeln!(o, "{:<16}{:>8}", format!("{name}:"), n);
}

/// `/proc/meminfo`: as linhas do 6.12 na ordem do kernel. Entram só as grandezas que o kernel do
/// sandbox acompanha (`MemSystem`); o que ele não tem (swap, slab, buffers, páginas de hugetlb, I/O em
/// andamento) sai 0, como numa máquina sem esses recursos.
pub(super) fn meminfo(m: &MemSystem) -> Vec<u8> {
    let mut o = Vec::with_capacity(1600);
    let cached = m.mapped + m.shmem;
    let used = m.anon + cached + m.kernel_stack + m.page_tables;
    let free = m.total.saturating_sub(used);
    let available = (free + m.mapped).min(m.total);
    let active_anon = m.anon + m.shmem;
    mem_line(&mut o, "MemTotal", m.total);
    mem_line(&mut o, "MemFree", free);
    mem_line(&mut o, "MemAvailable", available);
    mem_line(&mut o, "Buffers", 0);
    mem_line(&mut o, "Cached", cached);
    mem_line(&mut o, "SwapCached", 0);
    mem_line(&mut o, "Active", active_anon + m.mapped);
    mem_line(&mut o, "Inactive", 0);
    mem_line(&mut o, "Active(anon)", active_anon);
    mem_line(&mut o, "Inactive(anon)", 0);
    mem_line(&mut o, "Active(file)", m.mapped);
    mem_line(&mut o, "Inactive(file)", 0);
    mem_line(&mut o, "Unevictable", 0);
    mem_line(&mut o, "Mlocked", 0);
    mem_line(&mut o, "SwapTotal", 0);
    mem_line(&mut o, "SwapFree", 0);
    mem_line(&mut o, "Zswap", 0);
    mem_line(&mut o, "Zswapped", 0);
    mem_line(&mut o, "Dirty", 0);
    mem_line(&mut o, "Writeback", 0);
    mem_line(&mut o, "AnonPages", m.anon);
    mem_line(&mut o, "Mapped", m.mapped);
    mem_line(&mut o, "Shmem", m.shmem);
    mem_line(&mut o, "KReclaimable", 0);
    mem_line(&mut o, "Slab", 0);
    mem_line(&mut o, "SReclaimable", 0);
    mem_line(&mut o, "SUnreclaim", 0);
    mem_line(&mut o, "KernelStack", m.kernel_stack);
    mem_line(&mut o, "PageTables", m.page_tables);
    mem_line(&mut o, "SecPageTables", 0);
    mem_line(&mut o, "NFS_Unstable", 0);
    mem_line(&mut o, "Bounce", 0);
    mem_line(&mut o, "WritebackTmp", 0);
    mem_line(&mut o, "CommitLimit", m.total / 2);
    mem_line(&mut o, "Committed_AS", m.committed);
    mem_line(&mut o, "VmallocTotal", 34_359_738_367);
    mem_line(&mut o, "VmallocUsed", 0);
    mem_line(&mut o, "VmallocChunk", 0);
    mem_line(&mut o, "Percpu", 0);
    mem_line(&mut o, "HardwareCorrupted", 0);
    mem_line(&mut o, "AnonHugePages", 0);
    mem_line(&mut o, "ShmemHugePages", 0);
    mem_line(&mut o, "ShmemPmdMapped", 0);
    mem_line(&mut o, "FileHugePages", 0);
    mem_line(&mut o, "FilePmdMapped", 0);
    mem_line(&mut o, "Unaccepted", 0);
    mem_count(&mut o, "HugePages_Total", 0);
    mem_count(&mut o, "HugePages_Free", 0);
    mem_count(&mut o, "HugePages_Rsvd", 0);
    mem_count(&mut o, "HugePages_Surp", 0);
    mem_line(&mut o, "Hugepagesize", 2048);
    mem_line(&mut o, "Hugetlb", 0);
    let direct_2m = m.total & !2047;
    mem_line(&mut o, "DirectMap4k", m.total - direct_2m);
    mem_line(&mut o, "DirectMap2M", direct_2m);
    mem_line(&mut o, "DirectMap1G", 0);
    o
}

/// `/proc/cpuinfo`: um bloco por CPU virtual, com o modelo do oráculo. Topologia sem SMT: `n` núcleos
/// num pacote só, `core id` e `apicid` iguais ao número da CPU.
pub(super) fn cpuinfo(ncpus: u32) -> Vec<u8> {
    let mut o = Vec::with_capacity(1500 * ncpus as usize);
    for i in 0..ncpus {
        let _ = write!(
            o,
            "processor\t: {i}\nvendor_id\t: AuthenticAMD\ncpu family\t: 25\nmodel\t\t: 80\nmodel name\t: AMD Ryzen 7 5700\n\
             stepping\t: 0\nmicrocode\t: 0xa500011\ncpu MHz\t\t: 4497.448\ncache size\t: 512 KB\nphysical id\t: 0\n\
             siblings\t: {ncpus}\ncore id\t\t: {i}\ncpu cores\t: {ncpus}\napicid\t\t: {i}\ninitial apicid\t: {i}\n\
             fpu\t\t: yes\nfpu_exception\t: yes\ncpuid level\t: 16\nwp\t\t: yes\nflags\t\t: {CPU_FLAGS}\n\
             bugs\t\t: {CPU_BUGS}\nbogomips\t: 7386.49\nTLB size\t: 2560 4K pages\nclflush size\t: 64\n\
             cache_alignment\t: 64\naddress sizes\t: 48 bits physical, 48 bits virtual\n\
             power management: ts ttp tm hwpstate cpb eff_freq_ro [13] [14]\n\n"
        );
    }
    o
}

/// Um tempo em ns como `segundos.centésimos` (`/proc/uptime`).
fn secs_centi(ns: u64) -> String {
    format!("{}.{:02}", ns / 1_000_000_000, (ns % 1_000_000_000) / 10_000_000)
}

/// Tempo ocioso de uma CPU: o que sobra do tempo de vida depois do que ela trabalhou.
fn idle_ns(s: &SysData, c: &CpuTimes) -> u64 {
    s.uptime_ns.saturating_sub(c.user_ns + c.nice_ns + c.system_ns)
}

/// `/proc/uptime`: tempo desde o boot e o ocioso somado de todas as CPUs.
pub(super) fn uptime(s: &SysData) -> Vec<u8> {
    let idle: u64 = s.cpu.iter().map(|c| idle_ns(s, c)).sum();
    format!("{} {}\n", secs_centi(s.uptime_ns), secs_centi(idle)).into_bytes()
}

/// Uma média de carga em ponto fixo como `inteiro.centésimos`, com o arredondamento do `loadavg_proc_show`.
fn load_text(avg: u64) -> String {
    let a = avg + FIXED_1 / 200;
    format!("{}.{:02}", a >> 11, ((a & (FIXED_1 - 1)) * 100) >> 11)
}

/// `/proc/loadavg`: três médias, `rodando/existentes` e o último pid alocado.
pub(super) fn loadavg(s: &SysData) -> Vec<u8> {
    format!(
        "{} {} {} {}/{} {}\n",
        load_text(s.load[0]),
        load_text(s.load[1]),
        load_text(s.load[2]),
        s.procs_running,
        s.nr_threads,
        s.last_pid
    )
    .into_bytes()
}

/// `/proc/stat`: tempos em ticks de 100 Hz (usuário, nice, sistema, ocioso e seis colunas que o sandbox
/// não tem, todas 0: iowait, irq, softirq, steal, guest e guest_nice), trocas de contexto, boot,
/// processos criados e em execução. Sem interrupções de dispositivo, `intr` e `softirq` são 0.
pub(super) fn stat_global(s: &SysData) -> Vec<u8> {
    let mut o = Vec::with_capacity(512);
    let (mut user, mut nice, mut system, mut idle) = (0, 0, 0, 0);
    for c in &s.cpu {
        user += c.user_ns;
        nice += c.nice_ns;
        system += c.system_ns;
        idle += idle_ns(s, c);
    }
    let _ = writeln!(o, "cpu  {} {} {} {} 0 0 0 0 0 0", ticks(user), ticks(nice), ticks(system), ticks(idle));
    for (i, c) in s.cpu.iter().enumerate() {
        let _ = writeln!(
            o,
            "cpu{i} {} {} {} {} 0 0 0 0 0 0",
            ticks(c.user_ns),
            ticks(c.nice_ns),
            ticks(c.system_ns),
            ticks(idle_ns(s, c))
        );
    }
    let _ = write!(
        o,
        "intr 0\nctxt {}\nbtime {}\nprocesses {}\nprocs_running {}\nprocs_blocked {}\nsoftirq 0 0 0 0 0 0 0 0 0 0 0\n",
        s.ctxt, s.btime, s.forks, s.procs_running, s.procs_blocked
    );
    o
}

/// `/proc/filesystems`, a lista do kernel do oráculo.
pub(super) const FILESYSTEMS: &str = "nodev\tsysfs\nnodev\ttmpfs\nnodev\tproc\nnodev\tcgroup\nnodev\tcgroup2\nnodev\tdevtmpfs\n\
nodev\tdebugfs\nnodev\ttracefs\nnodev\tsecurityfs\nnodev\tsockfs\nnodev\tbpf\nnodev\tpipefs\nnodev\tramfs\n\
nodev\thugetlbfs\nnodev\tdevpts\n\tfuseblk\nnodev\tfuse\nnodev\tfusectl\nnodev\tvirtiofs\nnodev\tmqueue\n\
nodev\tresctrl\nnodev\tpstore\nnodev\tefivarfs\n\tbtrfs\n\text3\n\text2\n\text4\nnodev\tautofs\nnodev\tconfigfs\n\
\tvfat\nnodev\tbinfmt_misc\nnodev\toverlay\n";

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN_LIMITS: &str = include_str!("../../../../testbench/golden/linux-facts/proc/limits.txt");
    const GOLDEN_STATUS: &str = include_str!("../../../../testbench/golden/linux-facts/proc/status.txt");
    const GOLDEN_CPUINFO: &str = include_str!("../../../../testbench/golden/linux-facts/proc/cpuinfo.txt");
    const GOLDEN_FILESYSTEMS_FIRST: &str = "nodev\tsysfs\nnodev\ttmpfs\nnodev\tproc\n";

    /// Os rlimits do container Debian da bancada.
    fn container_rlimits() -> [(u64, u64); 16] {
        let inf = u64::MAX;
        let mut r = [(inf, inf); 16];
        r[Resource::Stack as usize] = (8 << 20, inf);
        r[Resource::Nofile as usize] = (1_073_741_816, 1_073_741_816);
        r[Resource::Memlock as usize] = (8 << 20, 8 << 20);
        r[Resource::Sigpending as usize] = (127_077, 127_077);
        r[Resource::Msgqueue as usize] = (819_200, 819_200);
        r[Resource::Nice as usize] = (0, 0);
        r[Resource::Rtprio as usize] = (0, 0);
        r
    }

    /// O `cat` do `status.txt` dourado.
    fn golden_cat() -> ProcData {
        ProcData {
            pid: 59,
            tid: 59,
            ppid: 7,
            pgid: 1,
            sid: 1,
            state: 'R',
            comm: b"cat".to_vec(),
            groups: vec![0],
            umask: 0o022,
            num_threads: 1,
            mem: Some(MemData {
                vm_peak: 3280,
                vm_size: 3280,
                vm_hwm: 1768,
                vm_rss: 1768,
                rss_anon: 116,
                rss_file: 1652,
                vm_data: 488,
                vm_stk: 132,
                vm_exe: 24,
                vm_lib: 1588,
                vm_pte: 48,
                ..MemData::default()
            }),
            rlimits: container_rlimits(),
            sigq: 1,
            fdsize: 64,
            ncpus: 16,
            ..ProcData::default()
        }
    }

    #[test]
    fn limits_match_the_oracle() {
        assert_eq!(String::from_utf8(limits(&golden_cat())).unwrap(), GOLDEN_LIMITS);
    }

    #[test]
    fn status_matches_the_oracle() {
        assert_eq!(String::from_utf8(status(&golden_cat())).unwrap(), GOLDEN_STATUS);
    }

    #[test]
    fn zombie_status_has_no_umask_and_no_vm_block() {
        let z = ProcData { state: 'Z', mem: None, fdsize: 0, ..golden_cat() };
        let s = String::from_utf8(status(&z)).unwrap();
        assert!(s.contains("State:\tZ (zombie)\n"));
        assert!(s.contains("FDSize:\t0\n"));
        assert!(!s.contains("Umask:") && !s.contains("VmSize:") && !s.contains("untag_mask:"));
        assert_eq!(statm(&z), b"0 0 0 0 0 0 0\n".to_vec());
    }

    #[test]
    fn stat_has_52_fields_and_the_kernel_layout() {
        let p = ProcData {
            utime_ns: 12_345_678_901,
            start_ns: 4_000_000_000,
            nice: 5,
            last_cpu: 3,
            ..golden_cat()
        };
        let line = String::from_utf8(stat(&p)).unwrap();
        let f: Vec<&str> = line.trim_end().split(' ').collect();
        assert_eq!(f.len(), 52, "{line}");
        assert_eq!(&f[..8], ["59", "(cat)", "R", "7", "1", "1", "0", "-1"]);
        assert_eq!(f[8], "4194304", "flags: PF_RANDOMIZE");
        assert_eq!(f[13], "1234", "utime em ticks de 100 Hz");
        assert_eq!(f[17], "25", "priority = 20 + nice");
        assert_eq!(f[18], "5");
        assert_eq!(f[21], "400", "starttime em ticks");
        assert_eq!(f[22], "3358720", "vsize em bytes");
        assert_eq!(f[23], "442", "rss em páginas");
        assert_eq!(f[24], "18446744073709551615", "rsslim ilimitado");
        assert_eq!(f[34], "0", "wchan de um processo rodando");
        assert_eq!(f[37], "17", "exit_signal");
        assert_eq!(f[38], "3", "processor");
        assert_eq!(f[51], "0", "exit_code");
    }

    #[test]
    fn zombie_stat_matches_the_oracle_layout() {
        let z = ProcData {
            pid: 10,
            tid: 10,
            ppid: 9,
            state: 'Z',
            comm: b"perl".to_vec(),
            mem: None,
            fork_noexec: true,
            exit_code: 3 << 8,
            last_cpu: 12,
            start_ns: 22_749_010_000_000,
            rlimits: container_rlimits(),
            ..ProcData::default()
        };
        let line = String::from_utf8(stat(&z)).unwrap();
        assert_eq!(
            line,
            "10 (perl) Z 9 0 0 0 -1 4227148 0 0 0 0 0 0 0 0 20 0 0 0 2274901 0 0 18446744073709551615 0 0 0 0 0 0 0 0 0 1 0 0 17 12 0 0 0 0 0 0 0 0 0 0 0 0 768\n"
        );
    }

    #[test]
    fn statm_is_in_pages() {
        assert_eq!(statm(&golden_cat()), b"820 442 413 6 0 155 0\n".to_vec());
    }

    #[test]
    fn cpumask_formats() {
        assert_eq!(cpumask_hex(16), "ffff");
        assert_eq!(cpumask_hex(2), "3");
        assert_eq!(cpumask_hex(32), "ffffffff");
        assert_eq!(cpumask_hex(40), "ff,ffffffff");
        assert_eq!(cpumask_list(1), "0");
        assert_eq!(cpumask_list(16), "0-15");
    }

    #[test]
    fn fdinfo_flags_are_octal_with_a_leading_zero() {
        let i = FdInfo { pos: 2, flags: 0o100000, mnt_id: 644, ino: 2 };
        assert_eq!(fdinfo(&i), b"pos:\t2\nflags:\t0100000\nmnt_id:\t644\nino:\t2\n".to_vec());
        let p = FdInfo { pos: 0, flags: 0, mnt_id: 16, ino: 3_072_732 };
        assert!(String::from_utf8(fdinfo(&p)).unwrap().contains("flags:\t00\n"));
    }

    #[test]
    fn loadavg_rounds_like_the_kernel() {
        // 6,60 em ponto fixo, mais o FIXED_1 / 200 do arredondamento.
        assert_eq!(load_text(13_517), "6.60");
        assert_eq!(load_text(0), "0.00");
        let s = SysData { procs_running: 3, nr_threads: 2185, last_pid: 62, load: [13_517, 11_856, 9_540], ..SysData::default() };
        assert_eq!(String::from_utf8(loadavg(&s)).unwrap(), "6.60 5.79 4.66 3/2185 62\n");
    }

    #[test]
    fn uptime_has_two_decimals_and_sums_the_idle_of_every_cpu() {
        let s = SysData {
            ncpus: 2,
            uptime_ns: 123_606_670_000_000,
            cpu: vec![CpuTimes { user_ns: 1_000_000_000, ..CpuTimes::default() }, CpuTimes::default()],
            ..SysData::default()
        };
        assert_eq!(String::from_utf8(uptime(&s)).unwrap(), "123606.67 247212.34\n");
    }

    #[test]
    fn global_stat_lists_one_line_per_cpu() {
        let s = SysData {
            ncpus: 2,
            uptime_ns: 10_000_000_000,
            btime: 1_791_198_929,
            cpu: vec![
                CpuTimes { user_ns: 1_000_000_000, nice_ns: 20_000_000, system_ns: 0 },
                CpuTimes { user_ns: 2_000_000_000, nice_ns: 0, system_ns: 0 },
            ],
            ctxt: 7,
            forks: 42,
            procs_running: 2,
            ..SysData::default()
        };
        let t = String::from_utf8(stat_global(&s)).unwrap();
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines[0], "cpu  300 2 0 1698 0 0 0 0 0 0");
        assert_eq!(lines[1], "cpu0 100 2 0 898 0 0 0 0 0 0");
        assert_eq!(lines[2], "cpu1 200 0 0 800 0 0 0 0 0 0");
        assert!(t.contains("\nctxt 7\nbtime 1791198929\nprocesses 42\nprocs_running 2\nprocs_blocked 0\n"));
    }

    #[test]
    fn meminfo_columns_match_the_kernel() {
        let m = MemSystem { total: 32_735_068, anon: 1000, mapped: 2000, shmem: 80, ..MemSystem::default() };
        let t = String::from_utf8(meminfo(&m)).unwrap();
        assert!(t.starts_with("MemTotal:       32735068 kB\nMemFree:        32731988 kB\n"), "{t}");
        assert!(t.contains("\nActive(anon):       1080 kB\n"));
        assert!(t.contains("\nUnevictable:           0 kB\n"));
        assert!(t.contains("\nHugePages_Total:       0\n"));
        assert!(t.contains("\nHugepagesize:       2048 kB\n"));
        assert!(t.ends_with("DirectMap1G:           0 kB\n"));
    }

    #[test]
    fn cpuinfo_blocks_follow_the_oracle() {
        let ours = String::from_utf8(cpuinfo(16)).unwrap();
        let first = ours.split("\n\n").next().unwrap().replace("cpu cores\t: 16", "cpu cores\t: 8");
        assert_eq!(format!("{first}\n"), GOLDEN_CPUINFO);
        assert_eq!(ours.matches("processor\t:").count(), 16);
        assert!(ours.ends_with("[13] [14]\n\n"));
    }

    #[test]
    fn filesystems_start_like_the_oracle() {
        assert!(FILESYSTEMS.starts_with(GOLDEN_FILESYSTEMS_FIRST));
        assert!(FILESYSTEMS.ends_with("\tvfat\nnodev\tbinfmt_misc\nnodev\toverlay\n"));
    }

    #[test]
    fn comm_is_escaped_in_status() {
        assert_eq!(escape_comm(b"a\nb\\c"), b"a\\nb\\\\c".to_vec());
    }
}

const CPU_FLAGS: &str = "fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush mmx fxsr sse sse2 ht syscall nx mmxext fxsr_opt pdpe1gb rdtscp lm constant_tsc rep_good nopl xtopology nonstop_tsc cpuid extd_apicid aperfmperf rapl pni pclmulqdq monitor ssse3 fma cx16 sse4_1 sse4_2 x2apic movbe popcnt aes xsave avx f16c rdrand lahf_lm cmp_legacy svm extapic cr8_legacy abm sse4a misalignsse 3dnowprefetch osvw ibs skinit wdt tce topoext perfctr_core perfctr_nb bpext perfctr_llc mwaitx cpb cat_l3 cdp_l3 hw_pstate ssbd mba ibrs ibpb stibp vmmcall fsgsbase bmi1 avx2 smep bmi2 erms invpcid cqm rdt_a rdseed adx smap clflushopt clwb sha_ni xsaveopt xsavec xgetbv1 xsaves cqm_llc cqm_occup_llc cqm_mbm_total cqm_mbm_local user_shstk clzero irperf xsaveerptr rdpru wbnoinvd cppc arat npt lbrv svm_lock nrip_save tsc_scale vmcb_clean flushbyasid decodeassists pausefilter pfthreshold avic v_vmsave_vmload vgif v_spec_ctrl umip pku ospke vaes vpclmulqdq rdpid overflow_recov succor smca fsrm debug_swap";
