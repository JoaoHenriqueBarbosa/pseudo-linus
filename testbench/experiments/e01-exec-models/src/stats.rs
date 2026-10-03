//! Estatística simples e leitura de memória do processo e do cgroup.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Percentil por posição (`p` em 0..=100) de uma amostra; ordena uma cópia.
pub fn percentile(samples: &[u64], p: f64) -> u64 {
    if samples.is_empty() {
        return 0;
    }
    let mut v = samples.to_vec();
    v.sort_unstable();
    let idx = ((p / 100.0) * (v.len() - 1) as f64).round() as usize;
    v[idx.min(v.len() - 1)]
}

pub fn median_f64(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut v = samples.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

#[derive(Clone, Debug, Serialize)]
pub struct Dist {
    pub n: usize,
    pub p50: u64,
    pub p90: u64,
    pub p99: u64,
    pub max: u64,
}

impl Dist {
    pub fn of(samples: &[u64]) -> Dist {
        Dist {
            n: samples.len(),
            p50: percentile(samples, 50.0),
            p90: percentile(samples, 90.0),
            p99: percentile(samples, 99.0),
            max: samples.iter().copied().max().unwrap_or(0),
        }
    }
}

/// Arredonda pra 3 casas (deixa o JSON legível).
pub fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// Tempo de CPU do processo inteiro (todas as threads), em ns.
pub fn process_cpu_ns() -> u64 {
    let ts = rustix::time::clock_gettime(rustix::time::ClockId::ProcessCPUTime);
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// Tempo de CPU da thread corrente, em ns. Não conta o tempo em que o host deixou a thread fora da CPU,
/// então é bem menos sensível a outros processos disputando a máquina do que o tempo de parede.
pub fn thread_cpu_ns() -> u64 {
    let ts = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// VmRSS do próprio processo, em KiB.
pub fn rss_kib() -> u64 {
    status_field("VmRSS:").unwrap_or(0)
}

fn status_field(name: &str) -> Option<u64> {
    let text = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = text.lines().find(|l| l.starts_with(name))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// Diretório do cgroup v2 do próprio processo.
pub fn own_cgroup_dir() -> Option<PathBuf> {
    let text = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let rel = text.lines().find_map(|l| l.strip_prefix("0::"))?;
    Some(PathBuf::from(format!("/sys/fs/cgroup{rel}")))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CgroupMem {
    pub current: u64,
    pub anon: u64,
    pub kernel_stack: u64,
    pub pagetables: u64,
    pub slab: u64,
}

/// Memória do cgroup (bytes): total e as parcelas que interessam (pilha de kernel por thread, tabelas
/// de página, slab).
pub fn cgroup_mem() -> Option<CgroupMem> {
    let dir = own_cgroup_dir()?;
    let current: u64 = std::fs::read_to_string(dir.join("memory.current")).ok()?.trim().parse().ok()?;
    let stat = std::fs::read_to_string(dir.join("memory.stat")).ok()?;
    let get = |k: &str| -> u64 {
        stat.lines()
            .find_map(|l| {
                let mut it = l.split_whitespace();
                (it.next() == Some(k)).then(|| it.next().and_then(|v| v.parse().ok()).unwrap_or(0))
            })
            .unwrap_or(0)
    };
    Some(CgroupMem {
        current,
        anon: get("anon"),
        kernel_stack: get("kernel_stack"),
        pagetables: get("pagetables"),
        slab: get("slab"),
    })
}
