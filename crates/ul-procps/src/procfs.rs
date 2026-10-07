//! Leitura do `/proc` no formato do Linux, como a libproc2 do procps faz: `/proc/<pid>/stat` (com o
//! `comm` entre parênteses, que pode ter espaço e `)`), `status`, `statm`, `cmdline`, `environ`,
//! `wchan`, `task/`, e os arquivos do sistema (`/proc/stat`, `meminfo`, `uptime`, `loadavg`,
//! `sys/kernel/pid_max`).
//!
//! Enquanto o procfs do kernel do pseudo-linus não expõe tudo, cada leitura tem um fallback
//! documentado: a lista de processos e o pai, grupo, sessão, estado e nome vêm de
//! `Syscalls::list_processes`; uid 0, tempos e memória 0, sem terminal; o tempo desde o boot vem de
//! `clock_gettime(CLOCK_BOOTTIME)` e a hora do boot de `CLOCK_REALTIME - CLOCK_BOOTTIME`.

use std::collections::BTreeMap;

use sysabi::{Clock, Fd, Pid, ProcInfo, sys};

/// `sysconf(_SC_CLK_TCK)` do Linux x86_64.
pub const HZ: u64 = 100;
/// Página de 4 KiB.
pub const PAGE_KB: u64 = 4;

/// Lê um arquivo do `/proc` inteiro; `None` se não existe ou não dá pra ler.
pub fn read(path: &str) -> Option<Vec<u8>> {
    sys::read_file(path.as_bytes()).ok()
}

fn read_str(path: &str) -> Option<String> {
    read(path).map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Campos de `/proc/<pid>/stat` (proc(5)), na ordem do kernel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stat {
    pub pid: Pid,
    pub comm: Vec<u8>,
    pub state: char,
    pub ppid: Pid,
    pub pgrp: Pid,
    pub session: Pid,
    pub tty_nr: i32,
    pub tpgid: i32,
    pub flags: u64,
    pub minflt: u64,
    pub cminflt: u64,
    pub majflt: u64,
    pub cmajflt: u64,
    pub utime: u64,
    pub stime: u64,
    pub cutime: i64,
    pub cstime: i64,
    pub priority: i64,
    pub nice: i64,
    pub num_threads: i64,
    pub itrealvalue: i64,
    pub starttime: u64,
    pub vsize: u64,
    pub rss: i64,
    pub rsslim: u64,
    pub startcode: u64,
    pub endcode: u64,
    pub startstack: u64,
    pub kstkesp: u64,
    pub kstkeip: u64,
    pub signal: u64,
    pub blocked: u64,
    pub sigignore: u64,
    pub sigcatch: u64,
    pub wchan: u64,
    pub nswap: u64,
    pub cnswap: u64,
    pub exit_signal: i32,
    pub processor: i32,
    pub rt_priority: u32,
    pub policy: u32,
}

/// Interpreta `/proc/<pid>/stat`. O `comm` vai do primeiro `(` ao último `)`, como o kernel escreve
/// (o nome pode conter espaço e parênteses).
pub fn parse_stat(data: &[u8]) -> Option<Stat> {
    let open = data.iter().position(|b| *b == b'(')?;
    let close = data.iter().rposition(|b| *b == b')')?;
    if close < open {
        return None;
    }
    let pid: Pid = std::str::from_utf8(&data[..open]).ok()?.trim().parse().ok()?;
    let comm = data[open + 1..close].to_vec();
    let rest = String::from_utf8_lossy(&data[close + 1..]).into_owned();
    let f: Vec<&str> = rest.split_ascii_whitespace().collect();
    // f[0] é o campo 3 (state).
    let u = |i: usize| -> u64 { f.get(i).and_then(|s| s.parse::<u64>().ok().or_else(|| s.parse::<i64>().ok().map(|v| v as u64))).unwrap_or(0) };
    let i = |i: usize| -> i64 { f.get(i).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0) };
    let state = f.first().and_then(|s| s.chars().next())?;
    Some(Stat {
        pid,
        comm,
        state,
        ppid: i(1) as Pid,
        pgrp: i(2) as Pid,
        session: i(3) as Pid,
        tty_nr: i(4) as i32,
        tpgid: i(5) as i32,
        flags: u(6),
        minflt: u(7),
        cminflt: u(8),
        majflt: u(9),
        cmajflt: u(10),
        utime: u(11),
        stime: u(12),
        cutime: i(13),
        cstime: i(14),
        priority: i(15),
        nice: i(16),
        num_threads: i(17),
        itrealvalue: i(18),
        starttime: u(19),
        vsize: u(20),
        rss: i(21),
        rsslim: u(22),
        startcode: u(23),
        endcode: u(24),
        startstack: u(25),
        kstkesp: u(26),
        kstkeip: u(27),
        signal: u(28),
        blocked: u(29),
        sigignore: u(30),
        sigcatch: u(31),
        wchan: u(32),
        nswap: u(33),
        cnswap: u(34),
        exit_signal: i(35) as i32,
        processor: i(36) as i32,
        rt_priority: u(37) as u32,
        policy: u(38) as u32,
    })
}

/// `/proc/<pid>/status`: chave e valor (sem o tab), na ordem do arquivo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub fields: Vec<(String, String)>,
}

impl Status {
    pub fn parse(data: &[u8]) -> Status {
        let text = String::from_utf8_lossy(data);
        let fields = text
            .lines()
            .filter_map(|l| l.split_once(':'))
            .map(|(k, v)| (k.to_string(), v.trim_start_matches(['\t', ' ']).to_string()))
            .collect();
        Status { fields }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// Os quatro ids de `Uid:`/`Gid:` (real, efetivo, salvo, de sistema de arquivos).
    pub fn ids(&self, key: &str) -> Option<[u32; 4]> {
        let v: Vec<u32> = self.get(key)?.split_ascii_whitespace().filter_map(|s| s.parse().ok()).collect();
        (v.len() == 4).then(|| [v[0], v[1], v[2], v[3]])
    }

    /// Valor em kB de uma linha `VmRSS:\t  3340 kB`.
    pub fn kb(&self, key: &str) -> Option<u64> {
        self.get(key)?.split_ascii_whitespace().next()?.parse().ok()
    }

    pub fn hex(&self, key: &str) -> Option<u64> {
        u64::from_str_radix(self.get(key)?.trim(), 16).ok()
    }

    pub fn num(&self, key: &str) -> Option<i64> {
        self.get(key)?.trim().parse().ok()
    }

    /// `Name:` sem os escapes do kernel (`\n` e `\\`).
    pub fn name(&self) -> Option<Vec<u8>> {
        let v = self.get("Name")?;
        let mut out = Vec::new();
        let b = v.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && i + 1 < b.len() {
                match b[i + 1] {
                    b'n' => out.push(b'\n'),
                    b'\\' => out.push(b'\\'),
                    other => {
                        out.push(b'\\');
                        out.push(other);
                    }
                }
                i += 2;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        Some(out)
    }
}

/// `/proc/<pid>/statm`, em páginas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Statm {
    pub size: u64,
    pub resident: u64,
    pub shared: u64,
    pub text: u64,
    pub lib: u64,
    pub data: u64,
    pub dt: u64,
}

pub fn parse_statm(data: &[u8]) -> Option<Statm> {
    let text = String::from_utf8_lossy(data);
    let v: Vec<u64> = text.split_ascii_whitespace().filter_map(|s| s.parse().ok()).collect();
    if v.len() < 7 {
        return None;
    }
    Some(Statm { size: v[0], resident: v[1], shared: v[2], text: v[3], lib: v[4], data: v[5], dt: v[6] })
}

/// Separa um bloco `a\0b\0c\0` (cmdline, environ) em argumentos. Um bloco sem o NUL final (o
/// processo reescreveu a área) conta o resto como último argumento.
pub fn split_nul(data: &[u8]) -> Vec<Vec<u8>> {
    if data.is_empty() {
        return Vec::new();
    }
    let body = data.strip_suffix(b"\0").unwrap_or(data);
    body.split(|b| *b == 0).map(<[u8]>::to_vec).collect()
}

/// Um processo (ou uma thread) como os programas enxergam.
#[derive(Clone, Debug, Default)]
pub struct Proc {
    /// pid do processo (tgid).
    pub tgid: Pid,
    /// id da thread (igual a `tgid` na linha do processo).
    pub tid: Pid,
    pub stat: Stat,
    pub status: Option<Status>,
    pub statm: Option<Statm>,
    /// `None`: não deu pra ler; `Some(vazio)`: thread de kernel ou zumbi.
    pub cmdline: Option<Vec<Vec<u8>>>,
    pub environ: Option<Vec<Vec<u8>>>,
    /// Conteúdo de `/proc/<pid>/wchan` (`0` quando roda).
    pub wchan: Option<String>,
    /// real, efetivo, salvo, de sistema de arquivos.
    pub uids: [u32; 4],
    pub gids: [u32; 4],
    /// Dono do diretório `/proc/<pid>` (é o que o top mostra como USER).
    pub dir_uid: u32,
    pub dir_gid: u32,
    /// Veio só de `list_processes` (sem `/proc/<pid>/stat`).
    pub fallback: bool,
}

impl Proc {
    pub fn pid(&self) -> Pid {
        self.tgid
    }

    pub fn comm(&self) -> &[u8] {
        &self.stat.comm
    }

    pub fn euid(&self) -> u32 {
        self.uids[1]
    }

    pub fn ruid(&self) -> u32 {
        self.uids[0]
    }

    pub fn egid(&self) -> u32 {
        self.gids[1]
    }

    pub fn rgid(&self) -> u32 {
        self.gids[0]
    }

    /// RSS em KiB (o `VmRSS` do status; sem status, `rss * 4` do stat).
    pub fn rss_kb(&self) -> u64 {
        if let Some(v) = self.status.as_ref().and_then(|s| s.kb("VmRSS")) {
            return v;
        }
        (self.stat.rss.max(0) as u64) * PAGE_KB
    }

    pub fn vsz_kb(&self) -> u64 {
        self.stat.vsize / 1024
    }

    /// Argumentos de linha de comando (vazio para thread de kernel, zumbi ou quando não há como
    /// ler).
    pub fn args(&self) -> &[Vec<u8>] {
        self.cmdline.as_deref().unwrap_or(&[])
    }

    pub fn is_kernel_thread(&self) -> bool {
        self.stat.flags & 0x0020_0000 != 0
    }

    /// Linha de comando como a libproc2 entrega: argumentos separados por espaço; sem argumentos,
    /// `[comm]`, com ` <defunct>` no zumbi.
    pub fn cmdline_string(&self) -> Vec<u8> {
        let args = self.args();
        if args.is_empty() {
            let mut s = Vec::with_capacity(self.stat.comm.len() + 12);
            s.push(b'[');
            s.extend_from_slice(&self.stat.comm);
            s.push(b']');
            if self.stat.state == 'Z' {
                s.extend_from_slice(b" <defunct>");
            }
            return s;
        }
        args.join(&b' ')
    }

    /// Segundos de vida do processo, com `uptime` em segundos desde o boot.
    pub fn age(&self, uptime: f64) -> f64 {
        uptime - self.stat.starttime as f64 / HZ as f64
    }
}

/// O que carregar de cada processo além do `stat`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Want {
    pub status: bool,
    pub statm: bool,
    pub cmdline: bool,
    pub environ: bool,
    pub wchan: bool,
    pub dir_owner: bool,
}

impl Want {
    pub fn all() -> Want {
        Want { status: true, statm: true, cmdline: true, environ: true, wchan: true, dir_owner: true }
    }
}

/// Pids listados em `/proc` (só os diretórios numéricos), em ordem crescente, como o procfs lista.
/// `None` se o `/proc` não existe ou não tem processo nenhum.
fn proc_dir_pids(dir: &str) -> Option<Vec<Pid>> {
    let entries = sys::read_dir(dir.as_bytes()).ok()?;
    let mut pids: Vec<Pid> = entries
        .iter()
        .filter_map(|e| std::str::from_utf8(&e.name).ok()?.parse::<Pid>().ok())
        .filter(|p| *p > 0)
        .collect();
    pids.sort_unstable();
    pids.dedup();
    (!pids.is_empty()).then_some(pids)
}

/// Fonte da lista de processos de uma varredura.
pub struct Snapshot {
    /// Processos (linhas de processo, não threads) em ordem crescente de pid.
    pub procs: Vec<Proc>,
}

/// Processos que o kernel conhece por `list_processes`, por pid.
fn kernel_list() -> BTreeMap<Pid, ProcInfo> {
    match sys::try_current() {
        Some(s) => s.list_processes().into_iter().map(|p| (p.pid, p)).collect(),
        None => BTreeMap::new(),
    }
}

fn stat_from_info(info: &ProcInfo) -> Stat {
    Stat {
        pid: info.pid,
        comm: info.comm.iter().take(15).copied().collect(),
        state: info.state,
        ppid: info.ppid,
        pgrp: info.pgid,
        session: info.sid,
        tty_nr: 0,
        tpgid: -1,
        priority: 20,
        nice: 0,
        num_threads: 1,
        exit_signal: 17,
        ..Stat::default()
    }
}

/// Lê um processo (ou a thread `tid` dele, quando `tid != tgid`) do diretório `base`
/// (`/proc/<pid>` ou `/proc/<pid>/task/<tid>`).
fn load(base: &str, tgid: Pid, tid: Pid, want: Want, info: Option<&ProcInfo>) -> Option<Proc> {
    let (stat, fallback) = match read(&format!("{base}/stat")).and_then(|d| parse_stat(&d)) {
        Some(s) => (s, false),
        None => (stat_from_info(info?), true),
    };
    let mut p = Proc { tgid, tid, stat, fallback, ..Proc::default() };
    // O status é lido sempre: carrega os uids e gids, que quase todo programa usa.
    p.status = read(&format!("{base}/status")).map(|d| Status::parse(&d));
    if let Some(st) = &p.status {
        if let Some(u) = st.ids("Uid") {
            p.uids = u;
        }
        if let Some(g) = st.ids("Gid") {
            p.gids = g;
        }
    }
    if want.statm {
        p.statm = read(&format!("{base}/statm")).and_then(|d| parse_statm(&d));
    }
    if want.cmdline {
        p.cmdline = read(&format!("{base}/cmdline")).map(|d| split_nul(&d));
    }
    if want.environ {
        p.environ = read(&format!("{base}/environ")).map(|d| split_nul(&d));
    }
    if want.wchan {
        p.wchan = read_str(&format!("{base}/wchan"));
    }
    if want.dir_owner {
        if let Ok(st) = sys::stat(base.as_bytes()) {
            p.dir_uid = st.uid;
            p.dir_gid = st.gid;
        }
    } else {
        p.dir_uid = p.uids[1];
        p.dir_gid = p.gids[1];
    }
    if p.status.is_none() && p.fallback {
        // Sem status nenhum: uid do dono do diretório, se houver, senão 0.
        p.uids = [p.dir_uid; 4];
        p.gids = [p.dir_gid; 4];
    }
    Some(p)
}

/// Varre os processos do sistema.
pub fn scan(want: Want) -> Snapshot {
    let kernel = kernel_list();
    let mut procs = Vec::new();
    match proc_dir_pids("/proc") {
        Some(pids) => {
            for (n, pid) in pids.into_iter().enumerate() {
                if n % 64 == 0 {
                    sys::checkpoint();
                }
                if let Some(p) = load(&format!("/proc/{pid}"), pid, pid, want, kernel.get(&pid)) {
                    procs.push(p);
                }
            }
        }
        None => {
            for info in kernel.values() {
                let mut p = Proc { tgid: info.pid, tid: info.pid, stat: stat_from_info(info), fallback: true, ..Proc::default() };
                if want.cmdline {
                    p.cmdline = read(&format!("/proc/{}/cmdline", info.pid)).map(|d| split_nul(&d));
                }
                procs.push(p);
            }
        }
    }
    Snapshot { procs }
}

/// Threads de um processo (`/proc/<pid>/task/*`), em ordem crescente de tid. Sem `task/`, a única
/// thread é o próprio processo.
pub fn threads(p: &Proc, want: Want) -> Vec<Proc> {
    let dir = format!("/proc/{}/task", p.tgid);
    let Some(tids) = proc_dir_pids(&dir) else { return vec![p.clone()] };
    let mut out = Vec::new();
    for tid in tids {
        if tid == p.tgid {
            // A linha da thread principal usa os dados por thread, não os do grupo.
            match load(&format!("{dir}/{tid}"), p.tgid, tid, want, None) {
                Some(t) => out.push(t),
                None => out.push(p.clone()),
            }
        } else if let Some(t) = load(&format!("{dir}/{tid}"), p.tgid, tid, want, None) {
            out.push(t);
        }
    }
    if out.is_empty() {
        out.push(p.clone());
    }
    out
}

/// O pid do próprio processo como o `/proc/self` diz (no kernel real é o `getpid`).
pub fn self_pid() -> Pid {
    let sys = sys::current();
    if let Ok(target) = sys.readlinkat(Fd::CWD, b"/proc/self")
        && let Some(p) = std::str::from_utf8(&target).ok().and_then(|s| s.parse().ok()) {
            return p;
        }
    sys.getpid()
}

/// `/proc/self/stat` do próprio processo; sem ele, o que dá pra saber pelas syscalls.
pub fn self_stat() -> Stat {
    if let Some(s) = read("/proc/self/stat").and_then(|d| parse_stat(&d)) {
        return s;
    }
    let sys = sys::current();
    let pid = sys.getpid();
    match sys.list_processes().into_iter().find(|p| p.pid == pid) {
        Some(info) => stat_from_info(&info),
        None => Stat { pid, ppid: sys.getppid(), tty_nr: 0, tpgid: -1, ..Stat::default() },
    }
}

/// `/proc/sys/kernel/pid_max` (4194304 se não der pra ler, o valor do Debian 13 de 64 bits).
pub fn pid_max() -> u64 {
    read_str("/proc/sys/kernel/pid_max").and_then(|s| s.trim().parse().ok()).unwrap_or(4_194_304)
}

/// Segundos desde o boot: `CLOCK_BOOTTIME`.
pub fn uptime_clock() -> f64 {
    match sys::try_current().and_then(|s| s.clock_gettime(Clock::Boottime).ok()) {
        Some(t) => t.sec as f64 + f64::from(t.nsec) / 1e9,
        None => 0.0,
    }
}

/// `/proc/uptime` (segundos desde o boot e tempo ocioso somado das CPUs); sem ele,
/// `CLOCK_BOOTTIME` e ocioso 0.
pub fn uptime_file() -> Option<(f64, f64)> {
    let s = read_str("/proc/uptime")?;
    let mut it = s.split_ascii_whitespace();
    let up = it.next()?.parse().ok()?;
    let idle = it.next().and_then(|x| x.parse().ok()).unwrap_or(0.0);
    Some((up, idle))
}

/// Agora em segundos desde a época, com fração.
pub fn now_realtime() -> (i64, u32) {
    match sys::try_current().and_then(|s| s.clock_gettime(Clock::Realtime).ok()) {
        Some(t) => (t.sec, t.nsec),
        None => (0, 0),
    }
}

/// `/proc/loadavg`: as três médias, tarefas rodando/total e o último pid.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadAvg {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

pub fn loadavg() -> Option<LoadAvg> {
    let s = read_str("/proc/loadavg")?;
    let v: Vec<f64> = s.split_ascii_whitespace().take(3).filter_map(|x| x.parse().ok()).collect();
    (v.len() == 3).then(|| LoadAvg { one: v[0], five: v[1], fifteen: v[2] })
}

/// Uma linha `cpu` de `/proc/stat`, em ticks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
    pub guest: u64,
    pub guest_nice: u64,
}

impl CpuTimes {
    pub fn total(&self) -> u64 {
        self.user + self.nice + self.system + self.idle + self.iowait + self.irq + self.softirq + self.steal
    }
}

/// `/proc/stat`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SysStat {
    pub cpu: CpuTimes,
    /// Linhas `cpuN`, na ordem do arquivo, com o número.
    pub cpus: Vec<(usize, CpuTimes)>,
    pub btime: Option<u64>,
    pub procs_running: Option<u64>,
    pub procs_blocked: Option<u64>,
    pub processes: Option<u64>,
    pub ctxt: Option<u64>,
}

pub fn parse_sysstat(data: &[u8]) -> SysStat {
    let text = String::from_utf8_lossy(data);
    let mut out = SysStat::default();
    for line in text.lines() {
        let mut it = line.split_ascii_whitespace();
        let Some(key) = it.next() else { continue };
        let nums: Vec<u64> = it.filter_map(|x| x.parse().ok()).collect();
        let n = |i: usize| nums.get(i).copied().unwrap_or(0);
        if let Some(rest) = key.strip_prefix("cpu") {
            let t = CpuTimes {
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
            };
            if rest.is_empty() {
                out.cpu = t;
            } else if let Ok(id) = rest.parse() {
                out.cpus.push((id, t));
            }
            continue;
        }
        match key {
            "btime" => out.btime = nums.first().copied(),
            "procs_running" => out.procs_running = nums.first().copied(),
            "procs_blocked" => out.procs_blocked = nums.first().copied(),
            "processes" => out.processes = nums.first().copied(),
            "ctxt" => out.ctxt = nums.first().copied(),
            _ => {}
        }
    }
    out
}

pub fn sysstat() -> Option<SysStat> {
    read("/proc/stat").map(|d| parse_sysstat(&d))
}

/// Hora do boot em segundos desde a época: `btime` de `/proc/stat`; sem ele, agora menos o
/// `CLOCK_BOOTTIME`.
pub fn boot_time() -> f64 {
    if let Some(b) = sysstat().and_then(|s| s.btime) {
        return b as f64;
    }
    let (sec, nsec) = now_realtime();
    sec as f64 + f64::from(nsec) / 1e9 - uptime_clock()
}

/// `/proc/meminfo`: chave -> valor em kB (as linhas sem unidade também entram).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemInfo {
    pub map: BTreeMap<String, u64>,
}

impl MemInfo {
    pub fn parse(data: &[u8]) -> MemInfo {
        let text = String::from_utf8_lossy(data);
        let mut map = BTreeMap::new();
        for line in text.lines() {
            if let Some((k, v)) = line.split_once(':')
                && let Some(n) = v.split_ascii_whitespace().next().and_then(|x| x.parse().ok()) {
                    map.insert(k.trim().to_string(), n);
                }
        }
        MemInfo { map }
    }

    pub fn get(&self, key: &str) -> u64 {
        self.map.get(key).copied().unwrap_or(0)
    }

}

pub fn meminfo() -> Option<MemInfo> {
    read("/proc/meminfo").map(|d| MemInfo::parse(&d))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_with_parentheses_and_spaces_in_comm() {
        let s = parse_stat(b"50 (x) (y) R 1 50 1 34817 50 4194304 300 0 0 0 50000 2000 0 0 15 -5 1 0 20000 10485760 1024 18446744073709551615 1 2 3 0 0 0 0 0 0 0 0 0 17 1 0 0 0 0 0\n").unwrap();
        assert_eq!(s.pid, 50);
        assert_eq!(s.comm, b"x) (y");
        assert_eq!(s.state, 'R');
        assert_eq!(s.tty_nr, 34817);
        assert_eq!(s.nice, -5);
        assert_eq!(s.utime, 50000);
        assert_eq!(s.starttime, 20000);
        assert_eq!(s.vsize, 10485760);
        assert_eq!(s.rss, 1024);
        assert_eq!(s.rsslim, u64::MAX);
        assert_eq!(s.processor, 1);
    }

    #[test]
    fn status_and_statm() {
        let st = Status::parse(b"Name:\ta\\nb\nUid:\t1\t2\t3\t4\nVmRSS:\t    3340 kB\nSigCgt:\t000000004b813efb\n");
        assert_eq!(st.name().unwrap(), b"a\nb");
        assert_eq!(st.ids("Uid"), Some([1, 2, 3, 4]));
        assert_eq!(st.kb("VmRSS"), Some(3340));
        assert_eq!(st.hex("SigCgt"), Some(0x4b81_3efb));
        let m = parse_statm(b"1122 835 512 201 0 180 0\n").unwrap();
        assert_eq!(m.resident, 835);
        assert_eq!(split_nul(b"a\0b c\0"), vec![b"a".to_vec(), b"b c".to_vec()]);
        assert!(split_nul(b"").is_empty());
    }

    #[test]
    fn sysstat_and_meminfo() {
        let s = parse_sysstat(b"cpu  1 2 3 4 5 6 7 8 0 0\ncpu0 1 1 1 1 1 1 1 1 0 0\nbtime 1768477400\nprocs_running 2\n");
        assert_eq!(s.cpu.total(), 36);
        assert_eq!(s.cpus.len(), 1);
        assert_eq!(s.btime, Some(1_768_477_400));
        let m = MemInfo::parse(b"MemTotal:        8161656 kB\nHugePages_Total:       0\n");
        assert_eq!(m.get("MemTotal"), 8_161_656);
        assert!(m.map.contains_key("HugePages_Total"));
    }
}
