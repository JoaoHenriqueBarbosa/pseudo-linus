//! H41 (divisão entre grupos), H42 (controle de banda) e a decisão de multi-CPU, no host com cgroups
//! v2 reais e no simulador.
//!
//! - **H41**: 1 laço contra 8 laços com pesos 100/100 e 100/300, e a hierarquia usuário > sandbox >
//!   processo, em 1 CPU e em 2 CPUs (um par SMT, que forma um domínio de balanceamento de 2 CPUs como
//!   numa VPS de 2 vCPUs). O simulador roda o modelo do kernel (uma runqueue por CPU, com balanceamento
//!   de domínio SMT). Critério: divisões a até 5 pontos percentuais.
//! - **H42**: quota de 20% e 50% (período de 100 ms), sozinho e disputando com um grupo sem limite,
//!   numa CPU. Critério: fração de CPU a até 2 pontos e o mesmo padrão roda/estrangula (fração de
//!   períodos estrangulados a até 0,15 e tempo médio até o estrangulamento a até 2 ms com a fase
//!   casada, 5 ms sem ela). O kernel sorteia a fase do timer de período em `init_cfs_bandwidth`, e a
//!   distância desse timer até a grade de ticks decide se a quota acaba no 5º ou no 6º tick (17 ou
//!   21 ms de execução com 20%). Sozinho, o laço roda desde o timer, então o primeiro degrau de uso
//!   depois de cada estrangulamento mede essa distância; cada rodada do host vira uma rodada do
//!   simulador com a mesma distância ([`SimOpts::tick_lead_ns`]). Disputando, o laço só volta num tick
//!   do outro grupo e a fase não é observável: o simulador sorteia a fase, como o kernel.
//! - **Multi-CPU**: nos cenários de 2 CPUs, a runqueue por CPU (com balanceamento) e a runqueue única
//!   são comparadas com o kernel; junto com o custo de trava medido no H11 com 2 workers, isso decide a
//!   organização pra VPS. Os cenários de 1 contra 8 também rodam no host em 2 núcleos diferentes
//!   (domínio MC, com os irmãos SMT ociosos), pra mostrar o efeito da topologia.

use std::time::Duration;

use anyhow::Result;
use sched::Topology;
use serde::Serialize;

use crate::cgroups::{CpuStat, Session};
use crate::lockscale::LockPoint;
use crate::scenario::{Pattern, Run, Scenario, SimOpts, analyze_pattern, group_samples, run_host, run_sim};
use crate::stats::summarize;

const MS: u64 = 1_000_000;

/// Repetições e janelas.
#[derive(Clone, Copy, Debug)]
pub struct GroupsPlan {
    pub reps: usize,
    pub warmup: Duration,
    pub window: Duration,
    pub sim_seeds: u64,
}

/// Divisão de um cenário no host e no simulador.
#[derive(Clone, Debug, Serialize)]
pub struct ShareComparison {
    pub scenario: String,
    pub cpus: Vec<usize>,
    pub groups: Vec<String>,
    pub ideal: Vec<f64>,
    pub host: Vec<f64>,
    pub host_stdev: Vec<f64>,
    pub host_runs: Vec<Vec<f64>>,
    /// Simulador no modelo do kernel (uma runqueue por CPU).
    pub sim_per_cpu: Vec<f64>,
    /// Simulador com runqueue única (só com 2 CPUs).
    pub sim_shared: Option<Vec<f64>>,
    /// Maior diferença host contra simulador por CPU, em pontos percentuais.
    pub diff_per_cpu_pp: f64,
    pub diff_shared_pp: Option<f64>,
    /// Maior distância do host até a divisão ideal pelos pesos.
    pub host_vs_ideal_pp: f64,
    /// Migrações por segundo das tarefas de cada grupo folha na janela (média das repetições).
    pub host_migrations_per_s: Vec<f64>,
    pub sim_per_cpu_migrations_per_s: Vec<f64>,
    /// Host, cenários de 1 laço contra N em 2 CPUs: fração do tempo em que `k` laços dos outros grupos
    /// estavam na fila da CPU do laço do primeiro grupo (média das repetições).
    pub host_colocated: Option<Vec<f64>>,
}

fn mean_cols(rows: &[Vec<f64>]) -> (Vec<f64>, Vec<f64>) {
    let n = rows.first().map(Vec::len).unwrap_or(0);
    let col = |i: usize| -> Vec<f64> { rows.iter().map(|r| r[i]).collect() };
    ((0..n).map(|i| summarize(&col(i)).mean).collect(), (0..n).map(|i| summarize(&col(i)).stdev).collect())
}

fn max_diff_pp(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y).abs() * 100.0).fold(0.0, f64::max)
}

fn round1(v: &[f64]) -> Vec<f64> {
    v.iter().map(|x| (x * 1000.0).round() / 10.0).collect()
}

/// Migrações por segundo de cada grupo numa rodada.
fn migrations_per_s(r: &Run) -> Vec<f64> {
    r.migrations.iter().map(|&m| m as f64 * 1e9 / r.window_ns.max(1) as f64).collect()
}

fn compare_shares(session: &mut Session, plan: &GroupsPlan, sc: &Scenario, cpus: &[usize], with_sim: bool) -> Result<ShareComparison> {
    let mut host_runs = Vec::new();
    let mut host_mig = Vec::new();
    let mut host_coloc: Vec<Vec<f64>> = Vec::new();
    for _ in 0..plan.reps {
        let r = run_host(session, sc, cpus, plan.warmup, plan.window, None)?;
        host_mig.push(migrations_per_s(&r));
        if let Some(c) = r.colocated {
            host_coloc.push(c);
        }
        host_runs.push(r.shares);
    }
    let host_colocated = (!host_coloc.is_empty()).then(|| {
        let n = host_coloc.iter().map(Vec::len).max().unwrap_or(0);
        let padded: Vec<Vec<f64>> = host_coloc.iter().map(|c| (0..n).map(|k| c.get(k).copied().unwrap_or(0.0)).collect()).collect();
        mean_cols(&padded).0
    });
    let (host, host_stdev) = mean_cols(&host_runs);
    let warmup = plan.warmup.as_nanos() as u64;
    let window = plan.window.as_nanos() as u64;
    let sim_runs = |topo: Topology| -> Vec<Run> {
        (0..plan.sim_seeds).map(|s| run_sim(sc, &SimOpts::new(cpus.len(), topo, warmup, window, 500 + s))).collect()
    };
    let shares = |runs: &[Run]| mean_cols(&runs.iter().map(|r| r.shares.clone()).collect::<Vec<_>>()).0;
    let per_cpu_runs = if with_sim { sim_runs(Topology::PerCpu) } else { Vec::new() };
    let sim_per_cpu = if with_sim { shares(&per_cpu_runs) } else { Vec::new() };
    let sim_per_cpu_migrations_per_s = mean_cols(&per_cpu_runs.iter().map(migrations_per_s).collect::<Vec<_>>()).0;
    let sim_shared = (with_sim && cpus.len() > 1).then(|| shares(&sim_runs(Topology::Shared)));
    let ideal = sc.ideal_shares();
    let cmp = ShareComparison {
        scenario: sc.id.clone(),
        cpus: cpus.to_vec(),
        groups: sc.groups.iter().map(|g| g.name.clone()).collect(),
        diff_per_cpu_pp: if with_sim { max_diff_pp(&host, &sim_per_cpu) } else { f64::NAN },
        diff_shared_pp: sim_shared.as_ref().map(|s| max_diff_pp(&host, s)),
        host_vs_ideal_pp: max_diff_pp(&host, &ideal),
        ideal,
        host,
        host_stdev,
        host_runs,
        sim_per_cpu,
        sim_shared,
        host_migrations_per_s: mean_cols(&host_mig).0,
        sim_per_cpu_migrations_per_s,
        host_colocated,
    };
    let mig = |v: &[f64]| v.iter().map(|x| format!("{x:.1}")).collect::<Vec<_>>().join("/");
    eprintln!(
        "      {} em {:?}: host {:?} (ideal {:?}) simulador por CPU {:?}{} | diferença {:.2} pp | migrações/s host {} simulador {}{}",
        cmp.scenario,
        cmp.cpus,
        round1(&cmp.host),
        round1(&cmp.ideal),
        round1(&cmp.sim_per_cpu),
        cmp.sim_shared.as_ref().map(|s| format!(" runqueue única {:?}", round1(s))).unwrap_or_default(),
        cmp.diff_per_cpu_pp,
        mig(&cmp.host_migrations_per_s),
        mig(&cmp.sim_per_cpu_migrations_per_s),
        cmp.host_colocated.as_ref().map(|c| format!(" | laços dos outros na CPU do primeiro (k=0,1,..) {:?}", round1(c))).unwrap_or_default()
    );
    Ok(cmp)
}

/// Resultado do H41 e dos dados de topologia.
#[derive(Clone, Debug, Serialize)]
pub struct H41Data {
    pub one_cpu: Vec<ShareComparison>,
    pub two_cpus_smt: Vec<ShareComparison>,
    /// Só host: 1 contra 8 em dois núcleos diferentes.
    pub two_cores_host: Vec<ShareComparison>,
}

/// Roda o H41 (e os cenários de topologia).
pub fn run_h41(session: &mut Session, plan: &GroupsPlan, cpu: usize, smt_sibling: usize, other_core: usize) -> Result<H41Data> {
    let scenarios = [Scenario::one_vs_eight(100), Scenario::one_vs_eight(300), Scenario::hierarchy()];
    let mut one_cpu = Vec::new();
    let mut two = Vec::new();
    for sc in &scenarios {
        one_cpu.push(compare_shares(session, plan, sc, &[cpu], true)?);
    }
    for sc in &scenarios {
        two.push(compare_shares(session, plan, sc, &[cpu, smt_sibling], true)?);
    }
    let mut cores = Vec::new();
    for sc in &scenarios[..2] {
        cores.push(compare_shares(session, plan, sc, &[cpu, other_core], false)?);
    }
    Ok(H41Data { one_cpu, two_cpus_smt: two, two_cores_host: cores })
}

/// Comparação de um cenário de quota.
#[derive(Clone, Debug, Serialize)]
pub struct QuotaComparison {
    pub scenario: String,
    pub quota_pct: u32,
    pub competing: bool,
    pub host_frac: f64,
    pub host_frac_runs: Vec<f64>,
    pub sim_frac: f64,
    pub host_other_frac: Option<f64>,
    pub sim_other_frac: Option<f64>,
    pub host_pattern: Vec<Pattern>,
    pub sim_pattern: Vec<Pattern>,
    pub host_cpu_stat: Vec<CpuStat>,
    pub sim_cpu_stat: Vec<CpuStat>,
    /// Se cada rodada do simulador usou a distância timer-tick medida na rodada do host
    /// correspondente (os cenários sozinhos); senão, o simulador sorteia a fase.
    pub phase_matched: bool,
    pub frac_diff_pp: f64,
    /// Maior diferença do tempo médio até o estrangulamento (por rodada com a fase casada, na
    /// média das rodadas sem ela), em ms.
    pub run_diff_ms: f64,
    pub pattern_same: bool,
}

fn mean(v: &[f64]) -> f64 {
    let f: Vec<f64> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if f.is_empty() { f64::NAN } else { f.iter().sum::<f64>() / f.len() as f64 }
}

/// Roda o H42.
pub fn run_h42(session: &mut Session, plan: &GroupsPlan, cpu: usize) -> Result<Vec<QuotaComparison>> {
    let mut out = Vec::new();
    for (pct, competing) in [(20, false), (50, false), (20, true), (50, true)] {
        let sc = Scenario::quota(pct, competing);
        let mut host_runs: Vec<Run> = Vec::new();
        for _ in 0..plan.reps {
            host_runs.push(run_host(session, &sc, &[cpu], plan.warmup, plan.window, Some(Duration::from_millis(1)))?);
        }
        let warmup = plan.warmup.as_nanos() as u64;
        let window = plan.window.as_nanos() as u64;
        let host_pattern: Vec<Pattern> = host_runs.iter().map(|r| analyze_pattern(&group_samples(r, 0), 100 * MS)).collect();
        let leads: Vec<Option<u64>> =
            host_pattern.iter().map(|p| (!competing && p.first_step_ms.is_finite()).then_some((p.first_step_ms * MS as f64) as u64)).collect();
        let phase_matched = !leads.is_empty() && leads.iter().all(Option::is_some);
        let opts = |seed: u64, lead: Option<u64>| SimOpts {
            sample_ns: Some(MS),
            tick_lead_ns: lead,
            ..SimOpts::new(1, Topology::PerCpu, warmup, window, seed)
        };
        let sim_runs: Vec<Run> = if phase_matched {
            leads.iter().enumerate().map(|(r, &lead)| run_sim(&sc, &opts(900 + r as u64, lead))).collect()
        } else {
            (0..plan.sim_seeds).map(|s| run_sim(&sc, &opts(900 + s, None))).collect()
        };
        let sim_pattern: Vec<Pattern> = sim_runs.iter().map(|r| analyze_pattern(&group_samples(r, 0), 100 * MS)).collect();
        let host_frac_runs: Vec<f64> = host_runs.iter().map(|r| r.cpu_frac[0]).collect();
        let host_frac = mean(&host_frac_runs);
        let sim_frac = mean(&sim_runs.iter().map(|r| r.cpu_frac[0]).collect::<Vec<_>>());
        let other = |runs: &[Run]| competing.then(|| mean(&runs.iter().map(|r| r.cpu_frac[1]).collect::<Vec<_>>()));
        let ht = mean(&host_pattern.iter().map(|p| p.throttled_frac).collect::<Vec<_>>());
        let st = mean(&sim_pattern.iter().map(|p| p.throttled_frac).collect::<Vec<_>>());
        let hr = mean(&host_pattern.iter().map(|p| p.mean_run_ms).collect::<Vec<_>>());
        let sr = mean(&sim_pattern.iter().map(|p| p.mean_run_ms).collect::<Vec<_>>());
        // Tempo médio até o estrangulamento: com a fase casada, rodada a rodada; sem ela, a média das
        // rodadas.
        let run_diff_ms = if phase_matched {
            host_pattern
                .iter()
                .zip(&sim_pattern)
                .map(|(h, s)| if h.mean_run_ms.is_nan() && s.mean_run_ms.is_nan() { 0.0 } else { (h.mean_run_ms - s.mean_run_ms).abs() })
                .fold(0.0, |a: f64, d| if d.is_nan() { f64::INFINITY } else { a.max(d) })
        } else if hr.is_nan() && sr.is_nan() {
            0.0
        } else {
            (hr - sr).abs()
        };
        let run_tol = if phase_matched { 2.0 } else { 5.0 };
        let run_same = run_diff_ms <= run_tol || (ht < 0.2 && st < 0.2);
        let pattern_same = (ht - st).abs() <= 0.15 && run_same;
        let steps: Vec<String> = host_pattern.iter().map(|p| format!("{:.2}", p.first_step_ms)).collect();
        let cmp = QuotaComparison {
            scenario: sc.id.clone(),
            quota_pct: pct,
            competing,
            host_frac,
            host_frac_runs,
            sim_frac,
            host_other_frac: other(&host_runs),
            sim_other_frac: other(&sim_runs),
            host_pattern,
            sim_pattern,
            host_cpu_stat: host_runs.iter().map(|r| r.stats[0]).collect(),
            sim_cpu_stat: sim_runs.iter().map(|r| r.stats[0]).collect(),
            phase_matched,
            frac_diff_pp: (host_frac - sim_frac).abs() * 100.0,
            run_diff_ms,
            pattern_same,
        };
        eprintln!(
            "      {}: fração host {:.2}% simulador {:.2}% | períodos estrangulados {:.2} contra {:.2} | roda {:.1} ms contra {:.1} ms antes de estrangular (diferença {:.1} ms, {}; primeiro degrau no host [{}] ms)",
            cmp.scenario,
            host_frac * 100.0,
            sim_frac * 100.0,
            ht,
            st,
            hr,
            sr,
            run_diff_ms,
            if phase_matched { "fase casada" } else { "fase sorteada" },
            steps.join(", ")
        );
        out.push(cmp);
    }
    Ok(out)
}

/// A decisão de multi-CPU.
#[derive(Clone, Debug, Serialize)]
pub struct MultiCpuDecision {
    pub chosen: String,
    pub reason: String,
    pub per_cpu_max_diff_pp: f64,
    pub shared_max_diff_pp: f64,
    pub per_cpu_vs_ideal_pp: f64,
    pub shared_vs_ideal_pp: f64,
    pub host_smt_vs_ideal_pp: f64,
    pub host_cores_vs_ideal_pp: f64,
    /// Eficiência da trava global com 2 workers no H11, por tempo de trabalho entre operações.
    pub global_lock_efficiency_2_workers: Vec<(u64, f64)>,
}

/// Decide entre runqueue por vCPU com balanceamento e runqueue única travada.
pub fn decide(h41: &H41Data, lock: &[LockPoint]) -> MultiCpuDecision {
    let two = &h41.two_cpus_smt;
    let per_cpu_max = two.iter().map(|c| c.diff_per_cpu_pp).fold(0.0, f64::max);
    let shared_max = two.iter().filter_map(|c| c.diff_shared_pp).fold(0.0, f64::max);
    let vs_ideal = |f: &dyn Fn(&ShareComparison) -> Vec<f64>| two.iter().map(|c| max_diff_pp(&f(c), &c.ideal)).fold(0.0, f64::max);
    let per_cpu_vs_ideal = vs_ideal(&|c| c.sim_per_cpu.clone());
    let shared_vs_ideal = vs_ideal(&|c| c.sim_shared.clone().unwrap_or_default());
    let host_smt_vs_ideal = two.iter().map(|c| c.host_vs_ideal_pp).fold(0.0, f64::max);
    let host_cores_vs_ideal = h41.two_cores_host.iter().map(|c| c.host_vs_ideal_pp).fold(0.0, f64::max);
    let eff: Vec<(u64, f64)> = lock.iter().filter(|p| p.workers == 2).map(|p| (p.think_ns, p.global_efficiency)).collect();
    let eff_at = |t: u64| eff.iter().find(|e| e.0 == t).map(|e| e.1).unwrap_or(f64::NAN);
    let (chosen, reason) = if per_cpu_max <= 5.0 && per_cpu_max <= shared_max + 1.0 {
        (
            "runqueue por vCPU com balanceamento".to_string(),
            format!(
                "Reproduz o kernel em 2 CPUs a até {per_cpu_max:.1} pp (a runqueue única fica a {shared_max:.1} pp, então a fidelidade não separa as duas); o que decide é que ela é o mesmo modelo do Linux (mesmo calc_group_shares, PELT e balanceamento, o que o sandbox promete imitar), aceita afinidade (sched_setaffinity) e não divide trava entre os workers: com 2 workers, a trava global do H11 rende {:.0}% sem trabalho entre operações e {:.0}% com 1 µs.",
                eff_at(0) * 100.0,
                eff_at(1_000) * 100.0
            ),
        )
    } else if shared_max <= 5.0 {
        (
            "runqueue única travada".to_string(),
            format!(
                "A runqueue única fica a {shared_max:.1} pp do kernel em 2 CPUs e a por vCPU a {per_cpu_max:.1} pp; com 2 workers a trava global rende {:.0}% sem trabalho entre operações e {:.0}% com 1 µs.",
                eff_at(0) * 100.0,
                eff_at(1_000) * 100.0
            ),
        )
    } else {
        (
            "runqueue por vCPU com balanceamento".to_string(),
            format!(
                "Nenhuma das duas fica a 5 pp do kernel em 2 CPUs (por vCPU {per_cpu_max:.1} pp, única {shared_max:.1} pp); a por vCPU é o modelo do kernel e não divide trava."
            ),
        )
    };
    MultiCpuDecision {
        chosen,
        reason,
        per_cpu_max_diff_pp: per_cpu_max,
        shared_max_diff_pp: shared_max,
        per_cpu_vs_ideal_pp: per_cpu_vs_ideal,
        shared_vs_ideal_pp: shared_vs_ideal,
        host_smt_vs_ideal_pp: host_smt_vs_ideal,
        host_cores_vs_ideal_pp: host_cores_vs_ideal,
        global_lock_efficiency_2_workers: eff,
    }
}
