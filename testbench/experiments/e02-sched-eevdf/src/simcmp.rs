//! Os cenários do diferencial (H13) no simulador do crate `sched`.
//!
//! O simulador roda com os parâmetros do host: fatia `0,70 ms * (1 + ilog2(min(CPUs online, 8)))`
//! (2,8 ms com 16 CPUs), tick de `1 / HZ` (4 ms com HZ=250), features padrão da 6.12.101. A fase do
//! tick e o instante de criação variam com a semente, pra que repetições no simulador tenham a mesma
//! natureza das repetições no host. A thread que dorme no simulador gasta por volta o mesmo tempo de
//! CPU medido no host (`sleeper_cpu_per_iter_ns`), e o timer dela dispara no vencimento duro
//! (pedido + folga) ou no tick que cair antes disso, como um hrtimer.

use sched::{Features, SimConfig, SimTask, Tunables, simulate};

use crate::rng::SplitMix;

/// HZ do kernel do host, lido do `/boot/config-<release>`; 250 se não der pra ler.
pub fn host_hz() -> (u64, bool) {
    let release = std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let path = format!("/boot/config-{}", release.trim());
    if let Ok(text) = std::fs::read_to_string(path) {
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("CONFIG_HZ=")
                && let Ok(hz) = v.trim().parse()
            {
                return (hz, true);
            }
        }
    }
    (250, false)
}

fn config(online_cpus: u32, hz: u64, warmup_ns: u64, window_ns: u64, seed: u64) -> SimConfig {
    let mut rng = SplitMix::new(seed);
    let tunables = Tunables::linux_6_12_101(online_cpus, hz);
    SimConfig::single_cpu(tunables, Features::default(), warmup_ns + window_ns, rng.below(tunables.tick_nsec), warmup_ns)
}

/// Divisão de CPU entre laços de CPU com os nices dados.
pub fn shares(online_cpus: u32, hz: u64, nices: &[i32], warmup_ns: u64, window_ns: u64, seed: u64) -> Vec<f64> {
    let mut rng = SplitMix::new(seed ^ 0xfeed);
    let tasks: Vec<SimTask> = nices
        .iter()
        .enumerate()
        .map(|(i, &n)| SimTask { start_ns: rng.below(200_000), ..SimTask::cpu_bound(&format!("hog{i}"), n) })
        .collect();
    let report = simulate(config(online_cpus, hz, warmup_ns, window_ns, seed), &[], &tasks);
    report.tasks.iter().map(|t| t.share).collect()
}

/// Latências de wakeup (ns) de uma tarefa que dorme `sleep_ns` em laço com `hogs` laços nice 0.
#[allow(clippy::too_many_arguments)]
pub fn latencies(
    online_cpus: u32,
    hz: u64,
    hogs: usize,
    run_ns: u64,
    slack_ns: u64,
    warmup_ns: u64,
    window_ns: u64,
    seed: u64,
) -> Vec<u64> {
    let mut rng = SplitMix::new(seed ^ 0xbeef);
    let mut tasks: Vec<SimTask> = (0..hogs)
        .map(|i| SimTask { start_ns: rng.below(200_000), ..SimTask::cpu_bound(&format!("hog{i}"), 0) })
        .collect();
    tasks.push(SimTask { start_ns: rng.below(200_000), ..SimTask::periodic("sleeper", 0, run_ns.max(1), 1_000_000, slack_ns) });
    let report = simulate(config(online_cpus, hz, warmup_ns, window_ns, seed), &[], &tasks);
    report.tasks[hogs].latencies_ns.clone()
}
