//! Escolha das CPUs do diferencial: a mais ociosa (por `/proc/stat`) pra medir, e outra ociosa, fora do
//! par SMT dela, pra thread que coordena.

use std::collections::BTreeSet;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rustix::thread::{CpuSet, sched_getaffinity, sched_setaffinity};
use serde::Serialize;

/// Contadores de uma CPU em `/proc/stat` (em ticks de USER_HZ).
#[derive(Clone, Copy, Debug)]
struct CpuTimes {
    idle: u64,
    total: u64,
}

fn read_proc_stat() -> Result<Vec<(usize, CpuTimes)>> {
    let text = std::fs::read_to_string("/proc/stat").context("ler /proc/stat")?;
    let mut out = Vec::new();
    for line in text.lines() {
        // A linha agregada é "cpu  ..."; as por CPU são "cpuN ...".
        let Some(rest) = line.strip_prefix("cpu") else { continue };
        if !rest.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        let mut fields = rest.split_whitespace();
        let Some(id) = fields.next().and_then(|s| s.parse::<usize>().ok()) else { continue };
        let nums: Vec<u64> = fields.filter_map(|f| f.parse().ok()).collect();
        if nums.len() < 5 {
            continue;
        }
        // user nice system idle iowait irq softirq steal (guest já está somado em user).
        let idle = nums[3] + nums[4];
        let total: u64 = nums.iter().take(8).sum();
        out.push((id, CpuTimes { idle, total }));
    }
    Ok(out)
}

/// CPUs do mesmo núcleo físico (`thread_siblings_list`).
pub fn smt_siblings(cpu: usize) -> BTreeSet<usize> {
    let path = format!("/sys/devices/system/cpu/cpu{cpu}/topology/thread_siblings_list");
    let mut set = BTreeSet::new();
    if let Ok(text) = std::fs::read_to_string(path) {
        for part in text.trim().split(',') {
            if let Some((a, b)) = part.split_once('-') {
                if let (Ok(a), Ok(b)) = (a.parse::<usize>(), b.parse::<usize>()) {
                    set.extend(a..=b);
                }
            } else if let Ok(a) = part.parse::<usize>() {
                set.insert(a);
            }
        }
    }
    set.insert(cpu);
    set
}

/// Escolha feita e o retrato de ocupação usado.
#[derive(Clone, Debug, Serialize)]
pub struct CpuChoice {
    pub measure_cpu: usize,
    pub helper_cpu: usize,
    /// Fração ociosa de cada CPU permitida durante a amostra.
    pub idle_fraction: Vec<(usize, f64)>,
}

/// Amostra `/proc/stat` por `sample` e escolhe as CPUs.
pub fn choose(sample: Duration) -> Result<CpuChoice> {
    let allowed = sched_getaffinity(None).context("sched_getaffinity")?;
    let a = read_proc_stat()?;
    std::thread::sleep(sample);
    let b = read_proc_stat()?;
    let mut idle: Vec<(usize, f64)> = Vec::new();
    for (id, tb) in &b {
        if *id >= CpuSet::MAX_CPU || !allowed.is_set(*id) {
            continue;
        }
        let Some((_, ta)) = a.iter().find(|(i, _)| i == id) else { continue };
        let dt = tb.total.saturating_sub(ta.total).max(1);
        let di = tb.idle.saturating_sub(ta.idle);
        idle.push((*id, di as f64 / dt as f64));
    }
    if idle.len() < 2 {
        bail!("menos de duas CPUs permitidas");
    }
    let mut ranked = idle.clone();
    ranked.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
    let measure_cpu = ranked[0].0;
    let siblings = smt_siblings(measure_cpu);
    let helper_cpu = ranked.iter().map(|r| r.0).find(|c| !siblings.contains(c)).unwrap_or(ranked[1].0);
    Ok(CpuChoice { measure_cpu, helper_cpu, idle_fraction: idle })
}

/// Pares de CPUs pros cenários de 2 CPUs: o irmão SMT da CPU medida (domínio de balanceamento de 2
/// CPUs) e outra CPU ociosa em outro núcleo, fora da CPU de coordenação e do irmão dela.
pub fn pairs(choice: &CpuChoice) -> (usize, Option<usize>) {
    let m = choice.measure_cpu;
    let sib = smt_siblings(m).into_iter().find(|&c| c != m).unwrap_or(m);
    let mut excluded = smt_siblings(m);
    excluded.extend(smt_siblings(choice.helper_cpu));
    let mut ranked = choice.idle_fraction.clone();
    ranked.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
    let other = ranked.iter().map(|r| r.0).find(|c| !excluded.contains(c));
    (sib, other)
}

/// Fixa a thread que chama numa CPU.
pub fn pin_current_thread(cpu: usize) -> Result<()> {
    let mut set = CpuSet::new();
    set.set(cpu);
    sched_setaffinity(None, &set).with_context(|| format!("sched_setaffinity na CPU {cpu}"))?;
    Ok(())
}

/// Solta a thread que chama em todas as CPUs que o processo tinha no início.
pub fn unpin_current_thread(all: &CpuSet) -> Result<()> {
    sched_setaffinity(None, all).context("sched_setaffinity de volta")?;
    Ok(())
}

/// Número de CPUs online (`/sys/devices/system/cpu/online`), que é o que o kernel usa na fatia base.
pub fn online_cpus() -> u32 {
    let text = std::fs::read_to_string("/sys/devices/system/cpu/online").unwrap_or_default();
    let mut n = 0u32;
    for part in text.trim().split(',') {
        if let Some((a, b)) = part.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) {
                n += b - a + 1;
            }
        } else if part.parse::<u32>().is_ok() {
            n += 1;
        }
    }
    if n == 0 { std::thread::available_parallelism().map(|p| p.get() as u32).unwrap_or(1) } else { n }
}
