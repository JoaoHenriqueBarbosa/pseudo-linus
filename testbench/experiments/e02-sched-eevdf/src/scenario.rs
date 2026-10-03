//! Cenários de grupos (H41, H42 e a decisão de multi-CPU), definidos uma vez e rodados no kernel do
//! host (cgroups v2 reais, [`crate::cgroups`]) e no simulador do crate `sched`.
//!
//! Um cenário é uma árvore de grupos. Grupo com threads é folha (um scope no host); grupo sem threads é
//! intermediário (uma slice no host). Toda thread é um laço de CPU nice 0, fixado no conjunto de CPUs
//! do cenário.
//!
//! O padrão de estrangulamento ([`analyze_pattern`]) sai de amostras do tempo de CPU acumulado do grupo
//! a cada 1 ms (o `usage_usec` do `cpu.stat` no host; o tempo já contabilizado no simulador, que anda nos
//! mesmos pontos: no kernel o uso do cgroup só é atualizado quando o `update_curr` cobra quem roda, ou
//! seja, de 4 em 4 ms num laço de CPU), com o mesmo algoritmo pros dois lados: trechos de pelo menos
//! 12 ms sem progresso são
//! estrangulamento (a alternância normal do escalonador para um grupo por no máximo uma fatia mais um
//! tick, menos que isso); o fim desses trechos marca o começo do período, cuja fase é estimada pela
//! média circular; e cada período é medido em uso, se teve estrangulamento e quanto tempo rodou antes
//! dele.

use std::time::{Duration, Instant};

use anyhow::Result;
use sched::{BalanceConfig, Features, SchedConfig, SimConfig, SimGroup, SimTask, Topology, Tunables, simulate};
use serde::Serialize;

use crate::cgroups::{CpuStat, Hog, HogSpec, Session, read_usage_usec};
use crate::rng::SplitMix;

const MS: u64 = 1_000_000;

/// Um grupo do cenário.
#[derive(Clone, Debug, Serialize)]
pub struct GroupSpec {
    pub name: String,
    pub parent: Option<usize>,
    /// `cpu.weight`.
    pub weight: u64,
    /// `cpu.max` em porcentagem de uma CPU, com período de 100 ms.
    pub quota_pct: Option<u32>,
    /// Laços de CPU (0 pra grupo intermediário).
    pub threads: usize,
}

/// Um cenário.
#[derive(Clone, Debug, Serialize)]
pub struct Scenario {
    pub id: String,
    pub groups: Vec<GroupSpec>,
}

fn g(name: &str, parent: Option<usize>, weight: u64, quota_pct: Option<u32>, threads: usize) -> GroupSpec {
    GroupSpec { name: name.to_string(), parent, weight, quota_pct, threads }
}

impl Scenario {
    /// Grupo A com 1 laço contra grupo B com 8 laços, pesos 100 e `wb`.
    pub fn one_vs_eight(wb: u64) -> Scenario {
        Scenario { id: format!("1v8-w100-{wb}"), groups: vec![g("A", None, 100, None, 1), g("B", None, wb, None, 8)] }
    }

    /// Usuário > sandbox > processo: u1 (100) > s (100) com 1 laço; u2 (100) > s1 (100) com 4 laços
    /// e s2 (300) com 2 laços.
    pub fn hierarchy() -> Scenario {
        Scenario {
            id: "hier-u-s-p".to_string(),
            groups: vec![
                g("u1", None, 100, None, 0),
                g("u1s", Some(0), 100, None, 1),
                g("u2", None, 100, None, 0),
                g("u2s1", Some(2), 100, None, 4),
                g("u2s2", Some(2), 300, None, 2),
            ],
        }
    }

    /// Grupo Q com quota de `pct`% (1 laço), sozinho ou disputando com O (1 laço, sem limite).
    pub fn quota(pct: u32, competing: bool) -> Scenario {
        let mut groups = vec![g("Q", None, 100, Some(pct), 1)];
        if competing {
            groups.push(g("O", None, 100, None, 1));
        }
        Scenario { id: format!("quota{pct}{}", if competing { "-vs-O" } else { "-alone" }), groups }
    }

    /// Divisão ideal pelos pesos (cada nível divide o do pai pelos pesos dos irmãos com laços), sem
    /// quota e supondo CPUs suficientes.
    pub fn ideal_shares(&self) -> Vec<f64> {
        let n = self.groups.len();
        let mut share = vec![0.0; n];
        let active = |i: usize| self.subtree_threads(i) > 0;
        for i in 0..n {
            let siblings: Vec<usize> = (0..n).filter(|&j| self.groups[j].parent == self.groups[i].parent && active(j)).collect();
            if !active(i) {
                continue;
            }
            let wsum: u64 = siblings.iter().map(|&j| self.groups[j].weight).sum();
            let parent_share = self.groups[i].parent.map(|p| share[p]).unwrap_or(1.0);
            share[i] = parent_share * self.groups[i].weight as f64 / wsum as f64;
        }
        share
    }

    fn subtree_threads(&self, i: usize) -> usize {
        self.groups[i].threads + (0..self.groups.len()).filter(|&j| self.groups[j].parent == Some(i)).map(|j| self.subtree_threads(j)).sum::<usize>()
    }

    /// Índices dos descendentes (inclusive o próprio).
    fn subtree(&self, i: usize) -> Vec<usize> {
        let mut out = vec![i];
        let mut k = 0;
        while k < out.len() {
            let x = out[k];
            out.extend((0..self.groups.len()).filter(|&j| self.groups[j].parent == Some(x)));
            k += 1;
        }
        out
    }

    /// Soma por grupo (inclui subgrupos) a partir do uso das folhas.
    pub fn aggregate(&self, leaf_usage: &[u64]) -> Vec<u64> {
        (0..self.groups.len()).map(|i| self.subtree(i).iter().map(|&j| leaf_usage[j]).sum()).collect()
    }
}

/// Resultado de uma rodada (host ou simulador).
#[derive(Clone, Debug, Serialize)]
pub struct Run {
    /// Fração do tempo de CPU de todas as folhas que coube a cada grupo.
    pub shares: Vec<f64>,
    /// Tempo de CPU de cada grupo dividido pela janela (1,0 = uma CPU inteira).
    pub cpu_frac: Vec<f64>,
    pub window_ns: u64,
    /// `cpu.stat` na janela, por grupo folha (zeros nos intermediários).
    pub stats: Vec<CpuStat>,
    /// Migrações das tarefas de cada grupo folha na janela (zero nos intermediários).
    pub migrations: Vec<u64>,
    /// Só no host, com 2 CPUs ou mais e o primeiro grupo com um laço só: fração das amostras (a cada
    /// 10 ms) em que `k` laços dos outros grupos estavam na fila da CPU desse laço, por `k`.
    pub colocated: Option<Vec<f64>>,
    #[serde(skip)]
    pub samples: Vec<(u64, Vec<u64>)>,
}

fn shares_of(usage: &[u64], scenario: &Scenario) -> Vec<f64> {
    let total: u64 = scenario.groups.iter().zip(usage).filter(|(gs, _)| gs.threads > 0).map(|(_, &u)| u).sum();
    scenario.aggregate(usage).iter().map(|&u| u as f64 / total.max(1) as f64).collect()
}

/// Roda o cenário no host. `sample`: amostra o uso de cada folha a cada tanto (H42).
pub fn run_host(session: &mut Session, sc: &Scenario, cpus: &[usize], warmup: Duration, window: Duration, sample: Option<Duration>) -> Result<Run> {
    let root = session.root_slice();
    let scen_slice = session.child_slice(&root, &sc.id.replace('-', "_"));
    let mut slice_of: Vec<String> = Vec::new();
    for (i, gs) in sc.groups.iter().enumerate() {
        let parent_slice = match gs.parent {
            Some(p) => slice_of[p].clone(),
            None => scen_slice.clone(),
        };
        if gs.threads == 0 {
            let s = session.child_slice(&parent_slice, &format!("g{i}"));
            session.set_slice_weight(&s, gs.weight)?;
            slice_of.push(s);
        } else {
            slice_of.push(parent_slice);
        }
    }
    let seconds = (warmup + window).as_secs_f64() + 1.5;
    let mut hogs: Vec<Option<Hog>> = Vec::new();
    for (i, gs) in sc.groups.iter().enumerate() {
        if gs.threads == 0 {
            hogs.push(None);
            continue;
        }
        let name = format!("{}{}", gs.name.to_lowercase(), i);
        let spec = HogSpec {
            name: &name,
            slice: &slice_of[i],
            weight: gs.weight,
            quota_pct: gs.quota_pct,
            threads: gs.threads,
            cpus,
            seconds,
        };
        hogs.push(Some(session.spawn_hog(&spec)?));
    }
    std::thread::sleep(warmup);
    let read_all = |hogs: &[Option<Hog>]| -> Result<Vec<CpuStat>> {
        hogs.iter().map(|h| h.as_ref().map(|h| h.cpu_stat()).unwrap_or(Ok(CpuStat::default()))).collect()
    };
    let migrations_of = |hogs: &[Option<Hog>]| -> Result<Vec<u64>> { hogs.iter().map(|h| h.as_ref().map(Hog::migrations).unwrap_or(Ok(0))).collect() };
    let mig_before = migrations_of(&hogs)?;
    let before = read_all(&hogs)?;
    let t0 = Instant::now();
    let mut samples = Vec::new();
    let mut colocated = None;
    if let Some(every) = sample {
        let paths: Vec<Option<std::path::PathBuf>> = hogs.iter().map(|h| h.as_ref().map(Hog::stat_path)).collect();
        let mut k = 0u32;
        loop {
            let at = t0 + every * k;
            let now = Instant::now();
            if at > now {
                std::thread::sleep(at - now);
            }
            let t = t0.elapsed();
            if t >= window {
                break;
            }
            let mut v = Vec::with_capacity(paths.len());
            for p in &paths {
                v.push(match p {
                    Some(p) => read_usage_usec(p)? * 1000,
                    None => 0,
                });
            }
            samples.push((t.as_nanos() as u64, v));
            k += 1;
        }
    } else if cpus.len() > 1 && sc.groups.first().is_some_and(|g| g.threads == 1) && sc.groups.len() > 1 {
        // Quantos laços dos outros grupos dividem a fila com o laço do primeiro grupo.
        let mut hist = vec![0u64; 1];
        let mut k = 0u32;
        loop {
            let at = t0 + Duration::from_millis(10) * k;
            let now = Instant::now();
            if at > now {
                std::thread::sleep(at - now);
            }
            if t0.elapsed() >= window {
                break;
            }
            k += 1;
            let Some(Ok(first)) = hogs[0].as_ref().map(Hog::thread_cpus) else { continue };
            let Some(&cpu0) = first.first() else { continue };
            let mut n = 0usize;
            for h in hogs[1..].iter().flatten() {
                n += h.thread_cpus()?.iter().filter(|&&c| c == cpu0).count();
            }
            if hist.len() <= n {
                hist.resize(n + 1, 0);
            }
            hist[n] += 1;
        }
        let total: u64 = hist.iter().sum();
        colocated = Some(hist.iter().map(|&c| c as f64 / total.max(1) as f64).collect());
    } else {
        std::thread::sleep(window);
    }
    let after = read_all(&hogs)?;
    let wall = t0.elapsed().as_nanos() as u64;
    let mig_after = migrations_of(&hogs)?;
    let migrations = mig_after.iter().zip(&mig_before).map(|(a, b)| a.saturating_sub(*b)).collect();
    for h in hogs.into_iter().flatten() {
        h.wait()?;
    }
    let stats: Vec<CpuStat> = before
        .iter()
        .zip(&after)
        .map(|(b, a)| CpuStat {
            usage_usec: a.usage_usec - b.usage_usec,
            nr_periods: a.nr_periods - b.nr_periods,
            nr_throttled: a.nr_throttled - b.nr_throttled,
            throttled_usec: a.throttled_usec - b.throttled_usec,
        })
        .collect();
    let usage: Vec<u64> = stats.iter().map(|s| s.usage_usec * 1000).collect();
    let agg = sc.aggregate(&usage);
    // Os intermediários somam as amostras das folhas.
    let samples = samples.into_iter().map(|(t, v)| (t, sc.aggregate(&v))).collect();
    Ok(Run {
        shares: shares_of(&usage, sc),
        cpu_frac: agg.iter().map(|&u| u as f64 / wall as f64).collect(),
        window_ns: wall,
        stats,
        migrations,
        colocated,
        samples,
    })
}

/// Configuração do simulador pra `cpus` CPUs na topologia dada, com o domínio de balanceamento de um
/// par SMT quando há duas CPUs numa runqueue por CPU (é o par que o diferencial usa no host).
pub fn sim_sched_config(cpus: usize, topology: Topology) -> SchedConfig {
    let mut sc = SchedConfig::new(cpus, topology, Tunables::linux_6_12_101(16, 250), Features::default());
    if topology == Topology::PerCpu && cpus > 1 {
        sc.balance = BalanceConfig::smt_domain(cpus);
    }
    sc
}

/// Como rodar um cenário no simulador.
#[derive(Clone, Copy, Debug)]
pub struct SimOpts {
    pub cpus: usize,
    pub topology: Topology,
    pub warmup_ns: u64,
    pub window_ns: u64,
    pub seed: u64,
    /// Amostra o uso de cada grupo a cada tanto (H42).
    pub sample_ns: Option<u64>,
    /// Distância do começo de cada período do primeiro grupo com quota até o tick seguinte. `None`
    /// sorteia a grade de ticks e a fase do timer de período, como o kernel (que sorteia a fase em
    /// `init_cfs_bandwidth`). Como 100 ms é múltiplo do tick de 4 ms, essa distância é a mesma em todo
    /// período e decide em que tick a quota acaba.
    pub tick_lead_ns: Option<u64>,
}

impl SimOpts {
    pub fn new(cpus: usize, topology: Topology, warmup_ns: u64, window_ns: u64, seed: u64) -> SimOpts {
        SimOpts { cpus, topology, warmup_ns, window_ns, seed, sample_ns: None, tick_lead_ns: None }
    }
}

/// Roda o cenário no simulador.
pub fn run_sim(sc: &Scenario, opts: &SimOpts) -> Run {
    let SimOpts { cpus, topology, warmup_ns, window_ns, seed, sample_ns, tick_lead_ns } = *opts;
    let mut rng = SplitMix::new(seed);
    let offsets: Vec<Option<u64>> = sc.groups.iter().map(|gs| gs.quota_pct.map(|_| rng.below(100 * MS))).collect();
    let groups: Vec<SimGroup> = sc
        .groups
        .iter()
        .zip(&offsets)
        .map(|(gs, off)| {
            let base = SimGroup::new(&gs.name, gs.parent, gs.weight);
            match (gs.quota_pct, off) {
                (Some(p), Some(off)) => base.with_quota(u64::from(p) * MS, 100 * MS, *off),
                _ => base,
            }
        })
        .collect();
    let mask = (1u64 << cpus) - 1;
    let mut tasks = Vec::new();
    for (i, gs) in sc.groups.iter().enumerate() {
        for k in 0..gs.threads {
            let mut t = SimTask::cpu_bound(&format!("{}-{k}", gs.name), 0).in_group(i);
            t.start_ns = rng.below(2 * MS);
            t.cpus = Some(mask);
            tasks.push(t);
        }
    }
    let sched = sim_sched_config(cpus, topology);
    let tick = sched.tunables.tick_nsec;
    let tick_phase_ns = match (tick_lead_ns, offsets.iter().flatten().next()) {
        (Some(lead), Some(off)) => (off + lead) % tick,
        _ => rng.below(tick),
    };
    let cfg = SimConfig { sched, duration_ns: warmup_ns + window_ns, tick_phase_ns, measure_from_ns: warmup_ns, sample_every_ns: sample_ns };
    let r = simulate(cfg, &groups, &tasks);
    let usage: Vec<u64> = sc
        .groups
        .iter()
        .enumerate()
        .map(|(i, gs)| if gs.threads > 0 { r.tasks.iter().filter(|t| t.group == Some(i)).map(|t| t.cpu_ns).sum() } else { 0 })
        .collect();
    let agg = sc.aggregate(&usage);
    let stats = sc
        .groups
        .iter()
        .zip(&r.groups)
        .map(|(gs, gr)| {
            if gs.threads > 0 {
                CpuStat {
                    usage_usec: gr.cpu_ns / 1000,
                    nr_periods: gr.nr_periods,
                    nr_throttled: gr.nr_throttled,
                    throttled_usec: gr.throttled_time_ns / 1000,
                }
            } else {
                CpuStat::default()
            }
        })
        .collect();
    let samples = r
        .samples
        .iter()
        .filter(|(t, _)| *t >= warmup_ns)
        .map(|(t, v)| (*t - warmup_ns, v.clone()))
        .collect();
    let migrations = (0..sc.groups.len()).map(|i| r.tasks.iter().filter(|t| t.group == Some(i)).map(|t| t.migrations).sum()).collect();
    Run {
        shares: shares_of(&usage, sc),
        cpu_frac: agg.iter().map(|&u| u as f64 / r.window_ns as f64).collect(),
        window_ns: r.window_ns,
        stats,
        migrations,
        colocated: None,
        samples,
    }
}

/// Padrão de estrangulamento de um grupo.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Pattern {
    /// Tempo de CPU sobre a janela.
    pub frac: f64,
    pub periods: usize,
    /// Fração dos períodos com estrangulamento.
    pub throttled_frac: f64,
    /// Mediana, nos períodos estrangulados, do tempo do começo do período até o estrangulamento (ms).
    pub median_run_ms: f64,
    /// Média do mesmo tempo (ms). É a medida comparada: com dívida, a execução alterna entre dois ticks
    /// vizinhos (por exemplo 48 e 52 ms com quota de 50 ms), e a mediana de uma distribuição com duas
    /// modas quase iguais pula de uma pra outra por um período a mais ou a menos.
    pub mean_run_ms: f64,
    /// Mediana do uso por período (ms).
    pub median_usage_ms: f64,
    /// Fase estimada do período (ms).
    pub phase_ms: f64,
    /// Mediana do primeiro degrau de uso depois de cada estrangulamento (ms). Com a CPU ociosa no
    /// desestrangulamento, o laço roda desde o timer de período e o primeiro `update_curr` é o tick
    /// seguinte, então o degrau é a distância do timer até a grade de ticks (menos a latência de
    /// acordar a CPU).
    pub first_step_ms: f64,
}

fn median(v: &mut [f64]) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

/// Uso interpolado no instante `t`.
fn usage_at(samples: &[(u64, u64)], t: u64) -> f64 {
    match samples.binary_search_by_key(&t, |s| s.0) {
        Ok(i) => samples[i].1 as f64,
        Err(0) => samples[0].1 as f64,
        Err(i) if i >= samples.len() => samples[samples.len() - 1].1 as f64,
        Err(i) => {
            let (t0, u0) = samples[i - 1];
            let (t1, u1) = samples[i];
            u0 as f64 + (u1 as f64 - u0 as f64) * (t - t0) as f64 / (t1 - t0) as f64
        }
    }
}

/// Analisa amostras `(instante, tempo de CPU acumulado)` de um grupo com período `period_ns`.
pub fn analyze_pattern(samples: &[(u64, u64)], period_ns: u64) -> Pattern {
    let min_stall = 12 * MS;
    if samples.len() < 3 {
        return Pattern {
            frac: f64::NAN,
            periods: 0,
            throttled_frac: f64::NAN,
            median_run_ms: f64::NAN,
            mean_run_ms: f64::NAN,
            median_usage_ms: f64::NAN,
            phase_ms: f64::NAN,
            first_step_ms: f64::NAN,
        };
    }
    let (t_first, u_first) = samples[0];
    let (t_last, u_last) = samples[samples.len() - 1];
    let frac = (u_last - u_first) as f64 / (t_last - t_first) as f64;

    // Trechos sem progresso, e o degrau de uso que encerra cada um.
    let mut stalls: Vec<(u64, u64)> = Vec::new();
    let mut steps: Vec<f64> = Vec::new();
    let mut start: Option<u64> = None;
    for w in samples.windows(2) {
        let (t0, u0) = w[0];
        let (t1, u1) = w[1];
        let rate = (u1 - u0) as f64 / (t1 - t0).max(1) as f64;
        if rate < 0.05 {
            start.get_or_insert(t0);
        } else if let Some(s) = start.take()
            && t0 - s >= min_stall
        {
            stalls.push((s, t0));
            steps.push((u1 - u0) as f64 / MS as f64);
        }
    }
    if let Some(s) = start
        && t_last - s >= min_stall
    {
        stalls.push((s, t_last));
    }

    // Fase: média circular dos fins de estrangulamento (que caem no começo do período).
    let ends: Vec<f64> = stalls.iter().filter(|s| s.1 < t_last).map(|s| (s.1 % period_ns) as f64 / period_ns as f64).collect();
    let phase = if ends.is_empty() {
        0.0
    } else {
        let (sx, sy) = ends.iter().fold((0.0, 0.0), |(x, y), f| {
            let a = f * std::f64::consts::TAU;
            (x + a.cos(), y + a.sin())
        });
        let mut a = sy.atan2(sx) / std::f64::consts::TAU;
        if a < 0.0 {
            a += 1.0;
        }
        a * period_ns as f64
    };
    let phase_ns = phase as u64;

    let first_k = t_first.saturating_sub(phase_ns).div_ceil(period_ns);
    let mut usage = Vec::new();
    let mut runs = Vec::new();
    let mut throttled = 0usize;
    let mut periods = 0usize;
    let mut k = first_k;
    loop {
        let a = phase_ns + k * period_ns;
        let b = a + period_ns;
        if b > t_last {
            break;
        }
        if a >= t_first {
            periods += 1;
            usage.push((usage_at(samples, b) - usage_at(samples, a)) / MS as f64);
            // Estrangulamento que começa dentro do período (tolerância de 2 ms pro começo).
            if let Some(s) = stalls.iter().find(|s| s.0 + 2 * MS >= a && s.0 < b) {
                throttled += 1;
                runs.push((s.0.saturating_sub(a)) as f64 / MS as f64);
            }
        }
        k += 1;
    }
    Pattern {
        frac,
        periods,
        throttled_frac: if periods == 0 { f64::NAN } else { throttled as f64 / periods as f64 },
        mean_run_ms: if runs.is_empty() { f64::NAN } else { runs.iter().sum::<f64>() / runs.len() as f64 },
        median_run_ms: median(&mut runs),
        median_usage_ms: median(&mut usage),
        phase_ms: phase / MS as f64,
        first_step_ms: median(&mut steps),
    }
}

/// Amostras de um grupo a partir das amostras de todos.
pub fn group_samples(run: &Run, group: usize) -> Vec<(u64, u64)> {
    run.samples.iter().map(|(t, v)| (*t, v[group])).collect()
}
