//! E02: roda todas as medições e grava `testbench/results/e02-sched-eevdf.json`.
//!
//! `--quick` reduz casos e repetições (pra desenvolvimento); a rodada oficial é sem argumentos e leva
//! poucos minutos numa máquina quieta.

use std::time::{Duration, Instant};

use anyhow::Result;
use e02_sched_eevdf::host::{LatencyRun, ShareRun};
use e02_sched_eevdf::opbench::OpBench;
use e02_sched_eevdf::pickbench::PickBench;
use e02_sched_eevdf::stats::{LatencyQuantiles, latency_quantiles, summarize};
use e02_sched_eevdf::groups::{GroupsPlan, H41Data, QuotaComparison};
use e02_sched_eevdf::{cgroups, cpusel, groups, host, lockscale, opbench, pickbench, pickcheck, rbprop, simcmp};
use harness::{CandidateResult, ExperimentResult, Fit, Verdict};
use serde::Serialize;
use serde_json::json;

const SHARE_TOLERANCE: f64 = 0.05;
const LATENCY_MAGNITUDE: f64 = 10.0;
const OPS_RATIO_LIMIT: f64 = 2.0;
const LOCK_EFFICIENCY: f64 = 0.70;

struct Plan {
    prop_cases: u32,
    pick_cases: u32,
    reps: usize,
    share_window: Duration,
    share_warmup: Duration,
    latency_iters: usize,
    latency_warmup: usize,
    sim_seeds: u64,
    lock_duration: Duration,
    lock_reps: usize,
    groups: GroupsPlan,
}

impl Plan {
    fn new(quick: bool) -> Plan {
        if quick {
            Plan {
                prop_cases: 3_000,
                pick_cases: 500,
                reps: 1,
                share_window: Duration::from_millis(600),
                share_warmup: Duration::from_millis(150),
                latency_iters: 300,
                latency_warmup: 50,
                sim_seeds: 2,
                lock_duration: Duration::from_millis(60),
                lock_reps: 1,
                groups: GroupsPlan { reps: 1, warmup: Duration::from_millis(800), window: Duration::from_millis(1500), sim_seeds: 2 },
            }
        } else {
            Plan {
                prop_cases: 100_000,
                pick_cases: 20_000,
                reps: 3,
                share_window: Duration::from_secs(2),
                share_warmup: Duration::from_millis(300),
                latency_iters: 2_000,
                latency_warmup: 200,
                sim_seeds: 5,
                lock_duration: Duration::from_millis(250),
                lock_reps: 5,
                groups: GroupsPlan { reps: 3, warmup: Duration::from_secs(1), window: Duration::from_secs(3), sim_seeds: 4 },
            }
        }
    }
}

fn loadavg() -> String {
    std::fs::read_to_string("/proc/loadavg").map(|s| s.trim().to_string()).unwrap_or_default()
}

fn ratio_of_magnitude(a: f64, b: f64) -> f64 {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    if lo <= 0.0 { f64::INFINITY } else { hi / lo }
}

#[derive(Serialize)]
struct ShareScenario {
    nices: Vec<i32>,
    ideal: Vec<f64>,
    host_runs: Vec<ShareRun>,
    host_mean: Vec<f64>,
    host_stdev: Vec<f64>,
    sim_mean: Vec<f64>,
    sim_stdev: Vec<f64>,
    /// Maior diferença |host - simulador| entre as tarefas, em pontos percentuais.
    max_diff_pp: f64,
}

#[derive(Serialize)]
struct LatencyScenario {
    hogs: usize,
    slack_ns: u64,
    sleeper_cpu_per_iter_ns: f64,
    host: LatencyQuantiles,
    host_runs: Vec<LatencyQuantiles>,
    host_meta: Vec<LatencyRun>,
    sim: LatencyQuantiles,
    sim_runs: Vec<LatencyQuantiles>,
    ratio_p50: f64,
    ratio_p99: f64,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    // Modo hog: o processo que roda dentro de cada scope dos cenários de grupos.
    if args.get(1).map(String::as_str) == Some("hog") {
        let threads: usize = args.get(2).and_then(|s| s.parse().ok()).ok_or_else(|| anyhow::anyhow!("hog <threads> <cpus> <segundos>"))?;
        let cpus: Vec<usize> = args.get(3).map(|s| s.split(',').filter_map(|c| c.parse().ok()).collect()).unwrap_or_default();
        let seconds: f64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(1.0);
        return cgroups::hog_main(threads, &cpus, seconds);
    }
    let quick = args.iter().any(|a| a == "--quick");
    let plan = Plan::new(quick);
    let mut multicpu: Option<groups::MultiCpuDecision> = None;
    let t_start = Instant::now();
    let load_start = loadavg();
    let mut res = ExperimentResult::new("e02-sched-eevdf", "E02: árvore rubro-negra aumentada e EEVDF feitos à mão");

    // ---------------------------------------------------------------------------------------------
    // H12: correção e desempenho da árvore e do pick
    // ---------------------------------------------------------------------------------------------
    eprintln!("[H12] teste de propriedade da rbtree com {} sequências", plan.prop_cases);
    let prop = rbprop::run(plan.prop_cases);
    eprintln!("      passou: {} ({} operações, {:.1} s)", prop.passed, prop.operations, prop.elapsed_s);
    eprintln!("[H12] pick_eevdf contra força bruta com {} casos", plan.pick_cases);
    let pickc = pickcheck::run(plan.pick_cases);
    eprintln!("      passou: {} ({:.1} s)", pickc.passed, pickc.elapsed_s);

    eprintln!("[H12] benchmark rbtree contra BTreeMap");
    let sizes = [8usize, 64, 512, 4096, 32_768];
    let ops: Vec<OpBench> = sizes.iter().map(|&n| opbench::bench_size(n)).collect();
    for o in &ops {
        let r = o.ratios();
        eprintln!(
            "      n={:>6}: insert {:.2}x  remove(chave) {:.2}x  remove(handle) {:.2}x  leftmost {:.2}x  churn {:.2}x  churn+aug {:.2}x",
            r.n, r.insert, r.remove_key, r.remove_handle, r.leftmost, r.churn, r.churn_aug
        );
    }

    eprintln!("[H12] pick_eevdf aumentado contra varredura linear");
    let mut picks: Vec<PickBench> = Vec::new();
    for &n in &[8usize, 64, 512, 4096] {
        let states = pickbench::simulated_states(n, 24, 1000 + n as u64);
        picks.push(pickbench::bench(n, "simulated", &states));
        let states = pickbench::mixed_nice_states(n, 24, 2000 + n as u64);
        picks.push(pickbench::bench(n, "mixed_nice", &states));
        let states = pickbench::independent_states(n, 24, 3000 + n as u64);
        picks.push(pickbench::bench(n, "independent", &states));
    }
    for p in &picks {
        eprintln!(
            "      n={:>5} {:>9}: eevdf {:>8.1} ns  linear {:>9.1} ns  ganho {:>6.2}x  esquerda elegível {:.0}%  concordam {}",
            p.n,
            p.family,
            p.eevdf_ns,
            p.linear_ns,
            p.speedup,
            p.leftmost_eligible * 100.0,
            p.agree
        );
    }

    let correct = prop.passed && pickc.passed && picks.iter().all(|p| p.agree);
    let ratios: Vec<_> = ops.iter().map(OpBench::ratios).collect();
    let slow_ops: Vec<String> = ratios
        .iter()
        .flat_map(|r| {
            [("insert", r.insert), ("remove", r.remove_key), ("leftmost", r.leftmost)]
                .into_iter()
                .filter(|(_, x)| *x > OPS_RATIO_LIMIT)
                .map(move |(name, x)| format!("{name} n={} ({x:.2}x)", r.n))
        })
        .collect();
    let slow_picks: Vec<String> = picks
        .iter()
        .filter(|p| p.n >= 64 && p.speedup <= 1.0)
        .map(|p| format!("{} n={} ({:.2}x)", p.family, p.n, p.speedup))
        .collect();
    let worst = |f: fn(&e02_sched_eevdf::opbench::Ratios) -> f64| ratios.iter().map(f).fold(0.0f64, f64::max);
    let h12_verdict = if !correct {
        Verdict::Refuted
    } else if slow_ops.is_empty() && slow_picks.is_empty() {
        Verdict::Confirmed
    } else {
        Verdict::Partial
    };
    let pick_at = |n: usize, fam: &str| picks.iter().find(|p| p.n == n && p.family == fam).map(|p| p.speedup).unwrap_or(f64::NAN);
    let h12_summary = format!(
        "{} sequências de proptest e {} casos de pick contra força bruta {}; pior razão contra BTreeMap: insert {:.2}x, remove por chave {:.2}x (por handle, que é o uso do escalonador, {:.2}x), leftmost {:.2}x{}; pick aumentado {:.1}x mais rápido que o linear em n=64 e {:.0}x em n=4096 no pior caso (deadline independente do vruntime, que força a descida da árvore){}.",
        prop.cases,
        pickc.cases,
        if correct { "sem falha" } else { "COM FALHA" },
        worst(|r| r.insert),
        worst(|r| r.remove_key),
        worst(|r| r.remove_handle),
        worst(|r| r.leftmost),
        if slow_ops.is_empty() { String::new() } else { format!(" (acima de 2x: {})", slow_ops.join(", ")) },
        pick_at(64, "independent"),
        pick_at(4096, "independent"),
        if slow_picks.is_empty() { String::new() } else { format!("; sem ganho em {}", slow_picks.join(", ")) },
    );
    res.hypothesis(
        "H12",
        h12_verdict,
        h12_summary,
        json!({ "proptest": prop, "pick_check": pickc, "ops_ns": ops, "ratios": ratios, "pick": picks }),
    );

    // ---------------------------------------------------------------------------------------------
    // H11: trava global contra runqueue por worker
    // ---------------------------------------------------------------------------------------------
    eprintln!("[H11] vazão de pick+put, trava global contra trava por runqueue");
    let workers = [1usize, 2, 4, 8, 16];
    let thinks = [0u64, 1_000, 10_000, 100_000];
    let lock = lockscale::run(&workers, &thinks, plan.lock_duration, plan.lock_reps);
    for p in &lock {
        eprintln!(
            "      trabalho {:>6} ns, {:>2} workers: global {:>11.0} op/s  por runqueue {:>11.0} op/s  eficiência {:.2}",
            p.think_ns, p.workers, p.global_ops_per_s, p.per_worker_ops_per_s, p.global_efficiency
        );
    }
    let passes = |think: u64| {
        lock.iter().filter(|p| p.think_ns == think && p.workers > 1 && p.workers <= 8).all(|p| p.global_efficiency >= LOCK_EFFICIENCY)
    };
    let eff = |think: u64, w: usize| lock.iter().find(|p| p.think_ns == think && p.workers == w).map(|p| p.global_efficiency).unwrap_or(f64::NAN);
    let threshold = thinks.iter().copied().find(|&t| passes(t));
    let single_op_ns = lock.iter().find(|p| p.think_ns == 0 && p.workers == 1).map(|p| 1e9 / p.per_worker_ops_per_s).unwrap_or(f64::NAN);
    let (h11_verdict, h11_summary) = match threshold {
        Some(0) => (
            Verdict::Confirmed,
            format!(
                "Mesmo com operações coladas (pick+put de {single_op_ns:.0} ns), a trava global mantém {:.0}% da vazão da trava por runqueue com 8 workers.",
                eff(0, 8) * 100.0
            ),
        ),
        Some(t) => (
            Verdict::Partial,
            format!(
                "Com operações coladas (pick+put de {single_op_ns:.0} ns) a trava global não escala: {:.0}% da vazão ideal com 2 workers, {:.0}% com 8. Só passa de 70% até 8 workers quando há pelo menos {} µs de trabalho entre operações ({:.0}% com 8 workers).",
                eff(0, 2) * 100.0,
                eff(0, 8) * 100.0,
                t / 1000,
                eff(t, 8) * 100.0
            ),
        ),
        None => (
            Verdict::Refuted,
            format!(
                "A trava global fica abaixo de 70% da vazão ideal com até 8 workers em todos os tempos de trabalho testados (com 100 µs entre operações: {:.0}% com 8 workers).",
                eff(100_000, 8) * 100.0
            ),
        ),
    };
    res.hypothesis("H11", h11_verdict, h11_summary, json!({ "points": lock, "op": "tick que preempta: avanço de 3 ms, tick, schedule (put + pick_eevdf + set_next) com 8 tarefas por runqueue" }));

    // ---------------------------------------------------------------------------------------------
    // H13: diferencial contra o kernel do host
    // ---------------------------------------------------------------------------------------------
    let online = cpusel::online_cpus();
    let (hz, hz_from_config) = simcmp::host_hz();
    let tunables = sched::Tunables::linux_6_12_101(online, hz);
    eprintln!(
        "[H13] host: {online} CPUs online, HZ={hz}{}, fatia {:.2} ms",
        if hz_from_config { "" } else { " (padrão, /boot/config ilegível)" },
        tunables.base_slice_ns() as f64 / 1e6
    );
    let h13 = run_h13(&plan, online, hz);
    match h13 {
        Ok((verdict, summary, evidence, shares_ok)) => {
            res.hypothesis("H13", verdict, summary, evidence);
            res.candidates.push(CandidateResult {
                name: "sched (nosso, crates/sched)".to_string(),
                version: "0.1.0".to_string(),
                role: "scheduler".to_string(),
                category: Some("a".to_string()),
                conformance: None,
                fit: if correct && shares_ok { Fit::Fits } else { Fit::FitsWithWork },
                notes: "EEVDF fiel ao fair.c da 6.12.101, sobre relógio injetável; mesmo código no simulador e no sandbox.".to_string(),
                metrics: json!({ "base_slice_ns": tunables.base_slice_ns(), "tick_nsec": tunables.tick_nsec }),
            });
        }
        Err(e) => {
            eprintln!("      falhou: {e:#}");
            res.hypothesis("H13", Verdict::Inconclusive, format!("Medição no host falhou: {e:#}"), json!({}));
        }
    }

    // ---------------------------------------------------------------------------------------------
    // H41, H42 e multi-CPU: grupos e banda com cgroups v2 reais
    // ---------------------------------------------------------------------------------------------
    eprintln!("[H41/H42] grupos e banda com cgroups v2 (systemd-run --user --scope)");
    match run_groups(&plan) {
        Ok((cpus, h41, h42)) => {
            record_h41(&mut res, &h41, &cpus);
            record_h42(&mut res, &h42);
            let decision = groups::decide(&h41, &lock);
            eprintln!("      decisão multi-CPU: {} ({})", decision.chosen, decision.reason);
            res.notes.push(format!("Multi-CPU (2 vCPUs): {}. {}", decision.chosen, decision.reason));
            multicpu = Some(decision);
        }
        Err(e) => {
            eprintln!("      falhou: {e:#}");
            for id in ["H41", "H42"] {
                res.hypothesis(id, Verdict::Inconclusive, format!("Medição com cgroups falhou: {e:#}"), json!({}));
            }
        }
    }

    res.candidates.insert(
        0,
        CandidateResult {
            name: "rbtree (nosso, crates/rbtree)".to_string(),
            version: "0.1.0".to_string(),
            role: "rbtree".to_string(),
            category: Some("a".to_string()),
            conformance: None,
            fit: if correct { Fit::Fits } else { Fit::DoesNotFit },
            notes: "Arena Vec<Node> com índices u32, tradução do lib/rbtree.c com augmentação genérica e cache do mais à esquerda.".to_string(),
            metrics: json!({ "ratios_vs_btreemap": ratios }),
        },
    );
    res.candidates.insert(
        1,
        CandidateResult {
            name: "std::collections::BTreeMap".to_string(),
            version: harness::HostInfo::collect().rustc,
            role: "rbtree".to_string(),
            category: Some("a".to_string()),
            conformance: None,
            fit: Fit::Reference,
            notes: "Linha de base de desempenho; não tem augmentação nem handle estável, então não serve pro pick O(log n).".to_string(),
            metrics: json!({}),
        },
    );

    res.metrics = json!({
        "elapsed_s": t_start.elapsed().as_secs_f64(),
        "loadavg_start": load_start,
        "loadavg_end": loadavg(),
        "quick": quick,
        "multicpu_decision": multicpu,
    });
    res.notes.push(format!(
        "Carga do host no início ({load_start}) e no fim ({}); medições de tempo valem pra máquina quieta, o runner refaz tudo em sequência.",
        loadavg()
    ));
    res.notes.push(
        "A árvore do kernel ordena só pela deadline (comparação circular), com empates na ordem de chegada; o desempate por id do design v2 não existe no fair.c da 6.12.101.".to_string(),
    );
    let path = res.write()?;
    eprintln!("gravado {} em {:.0} s", path.display(), t_start.elapsed().as_secs_f64());
    Ok(())
}

/// CPUs usadas nos cenários de grupos.
#[derive(Clone, Debug, Serialize)]
struct GroupCpus {
    measure: usize,
    smt_sibling: usize,
    other_core: usize,
    helper: usize,
}

/// Abre a sessão de cgroups, escolhe as CPUs, roda H41 e H42 e limpa tudo.
fn run_groups(plan: &Plan) -> Result<(GroupCpus, H41Data, Vec<QuotaComparison>)> {
    let all_cpus = rustix::thread::sched_getaffinity(None)?;
    let choice = cpusel::choose(Duration::from_millis(500))?;
    let (sibling, other) = cpusel::pairs(&choice);
    let other = other.ok_or_else(|| anyhow::anyhow!("sem CPU livre em outro núcleo"))?;
    let sibling = if sibling == choice.measure_cpu { other } else { sibling };
    let cpus = GroupCpus { measure: choice.measure_cpu, smt_sibling: sibling, other_core: other, helper: choice.helper_cpu };
    eprintln!(
        "      CPU medida {}, irmã SMT {}, outro núcleo {}, coordenação na CPU {}",
        cpus.measure, cpus.smt_sibling, cpus.other_core, cpus.helper
    );
    let mut session = cgroups::Session::new()?;
    cpusel::pin_current_thread(choice.helper_cpu)?;
    // Amostragem de 1 ms sem a folga de 50 µs do timer.
    let _ = rustix::thread::set_current_timer_slack(std::num::NonZeroU64::new(1));
    let outcome = (|| -> Result<(H41Data, Vec<QuotaComparison>)> {
        eprintln!("[H41] divisão entre grupos");
        let h41 = groups::run_h41(&mut session, &plan.groups, cpus.measure, cpus.smt_sibling, cpus.other_core)?;
        eprintln!("[H42] controle de banda");
        let h42 = groups::run_h42(&mut session, &plan.groups, cpus.measure)?;
        Ok((h41, h42))
    })();
    let _ = rustix::thread::set_current_timer_slack(None);
    let cleanup = session.cleanup();
    cpusel::unpin_current_thread(&all_cpus)?;
    let (h41, h42) = outcome?;
    cleanup?;
    Ok((cpus, h41, h42))
}

fn record_h41(res: &mut ExperimentResult, h41: &H41Data, cpus: &GroupCpus) {
    let one_ok = h41.one_cpu.iter().all(|c| c.diff_per_cpu_pp <= SHARE_TOLERANCE * 100.0);
    let two_ok = h41.two_cpus_smt.iter().all(|c| c.diff_per_cpu_pp <= SHARE_TOLERANCE * 100.0);
    let worst = |v: &[groups::ShareComparison]| v.iter().map(|c| c.diff_per_cpu_pp).fold(0.0, f64::max);
    let a_share = |v: &[groups::ShareComparison], id: &str| {
        v.iter().find(|c| c.scenario == id).map(|c| (c.host[0] * 100.0, c.sim_per_cpu[0] * 100.0)).unwrap_or((f64::NAN, f64::NAN))
    };
    let (h1, s1) = a_share(&h41.one_cpu, "1v8-w100-100");
    let (h2, s2) = a_share(&h41.two_cpus_smt, "1v8-w100-100");
    let (h3, s3) = a_share(&h41.two_cpus_smt, "1v8-w100-300");
    let cores = h41.two_cores_host.first().map(|c| c.host[0] * 100.0).unwrap_or(f64::NAN);
    let verdict = match (one_ok, two_ok) {
        (true, true) => Verdict::Confirmed,
        (true, false) => Verdict::Partial,
        _ => Verdict::Refuted,
    };
    let summary = format!(
        "Maior diferença host contra simulador: {:.2} pp em 1 CPU e {:.2} pp em 2 CPUs (par SMT), tolerância 5 pp. Grupo de 1 laço contra grupo de 8 com pesos iguais: {h1:.1}% no host e {s1:.1}% no simulador em 1 CPU, {h2:.1}% e {s2:.1}% em 2 CPUs; com pesos 100/300 em 2 CPUs, {h3:.1}% e {s3:.1}%. Em dois núcleos diferentes (domínio MC com os irmãos SMT ociosos) o host dá {cores:.1}%, longe dos 50% do peso.",
        worst(&h41.one_cpu),
        worst(&h41.two_cpus_smt)
    );
    res.hypothesis("H41", verdict, summary, json!({ "cpus": cpus, "data": h41, "criterion_pp": SHARE_TOLERANCE * 100.0 }));
}

/// Média dos valores finitos (NaN se não houver).
fn h42_mean(v: impl Iterator<Item = f64>) -> f64 {
    let f: Vec<f64> = v.filter(|x| x.is_finite()).collect();
    if f.is_empty() { f64::NAN } else { f.iter().sum::<f64>() / f.len() as f64 }
}

fn record_h42(res: &mut ExperimentResult, h42: &[QuotaComparison]) {
    let frac_ok = h42.iter().all(|c| c.frac_diff_pp <= 2.0);
    let pattern_ok = h42.iter().all(|c| c.pattern_same);
    let verdict = match (frac_ok, pattern_ok) {
        (true, true) => Verdict::Confirmed,
        (true, false) => Verdict::Partial,
        _ => Verdict::Refuted,
    };
    let parts: Vec<String> = h42
        .iter()
        .map(|c| {
            format!(
                "{}% {}: {:.1}% no host, {:.1}% no simulador",
                c.quota_pct,
                if c.competing { "disputando" } else { "sozinho" },
                c.host_frac * 100.0,
                c.sim_frac * 100.0
            )
        })
        .collect();
    let runs: Vec<String> = h42
        .iter()
        .map(|c| {
            let h = h42_mean(c.host_pattern.iter().map(|p| p.mean_run_ms));
            let s = h42_mean(c.sim_pattern.iter().map(|p| p.mean_run_ms));
            if h.is_nan() && s.is_nan() {
                format!("{} sem estrangulamento longo nos dois", c.scenario)
            } else if h.is_nan() || s.is_nan() {
                let ht = h42_mean(c.host_pattern.iter().map(|p| p.throttled_frac));
                let st = h42_mean(c.sim_pattern.iter().map(|p| p.throttled_frac));
                format!("{} com estrangulamento longo em {:.0}% dos períodos no host e {:.0}% no simulador", c.scenario, ht * 100.0, st * 100.0)
            } else {
                format!("{} roda {h:.1} contra {s:.1} ms ({})", c.scenario, if c.phase_matched { "fase casada" } else { "fase sorteada" })
            }
        })
        .collect();
    let bad: Vec<&str> = h42.iter().filter(|c| !c.pattern_same).map(|c| c.scenario.as_str()).collect();
    let summary = format!(
        "Fração de CPU do grupo com cpu.max: {} (tolerância 2 pp). Padrão roda/estrangula por período {}: {}.",
        parts.join("; "),
        if bad.is_empty() { "igual nos quatro cenários".to_string() } else { format!("diferente em {}", bad.join(", ")) },
        runs.join("; ")
    );
    res.hypothesis("H42", verdict, summary, json!({ "scenarios": h42, "criterion_pp": 2.0 }));
}

/// H13: mede no host, roda no simulador e compara. Devolve veredito, resumo, evidência e se a divisão
/// de CPU bateu.
fn run_h13(plan: &Plan, online: u32, hz: u64) -> Result<(Verdict, String, serde_json::Value, bool)> {
    let all_cpus = rustix::thread::sched_getaffinity(None)?;
    let choice = cpusel::choose(Duration::from_millis(500))?;
    eprintln!("      CPU medida {}, coordenação na CPU {}", choice.measure_cpu, choice.helper_cpu);
    cpusel::pin_current_thread(choice.helper_cpu)?;
    let outcome = measure_and_compare(plan, online, hz, choice.measure_cpu);
    cpusel::unpin_current_thread(&all_cpus)?;
    let (shares, latencies) = outcome?;

    let shares_ok = shares.iter().all(|s| s.max_diff_pp <= SHARE_TOLERANCE * 100.0);
    let contended: Vec<&LatencyScenario> = latencies.iter().filter(|l| l.hogs > 0 && l.slack_ns >= 1_000).collect();
    let latency_ok = contended.iter().all(|l| l.ratio_p50 <= LATENCY_MAGNITUDE && l.ratio_p99 <= LATENCY_MAGNITUDE);
    let verdict = match (shares_ok, latency_ok) {
        (true, true) => Verdict::Confirmed,
        (false, false) => Verdict::Refuted,
        _ => Verdict::Partial,
    };
    let worst_share = shares.iter().map(|s| s.max_diff_pp).fold(0.0f64, f64::max);
    let lat_text: Vec<String> = latencies
        .iter()
        .map(|l| {
            format!(
                "{} laço(s), folga {}: host p50/p99 {:.0}/{:.0} µs, simulador {:.0}/{:.0} µs",
                l.hogs,
                if l.slack_ns >= 1000 { format!("{} µs", l.slack_ns / 1000) } else { format!("{} ns", l.slack_ns) },
                l.host.p50_us,
                l.host.p99_us,
                l.sim.p50_us,
                l.sim.p99_us
            )
        })
        .collect();
    let summary = format!(
        "Divisão de CPU: maior diferença host contra simulador de {worst_share:.2} pp em {} cenários de nice (tolerância 5 pp). Latência de wakeup: {}.",
        shares.len(),
        lat_text.join("; ")
    );
    let evidence = json!({
        "cpu_choice": choice,
        "online_cpus": online,
        "hz": hz,
        "base_slice_ns": sched::Tunables::linux_6_12_101(online, hz).base_slice_ns(),
        "shares": shares,
        "latency": latencies,
        "criteria": { "share_tolerance_pp": SHARE_TOLERANCE * 100.0, "latency_magnitude_ratio": LATENCY_MAGNITUDE },
    });
    Ok((verdict, summary, evidence, shares_ok))
}

fn measure_and_compare(plan: &Plan, online: u32, hz: u64, cpu: usize) -> Result<(Vec<ShareScenario>, Vec<LatencyScenario>)> {
    let share_cases: [&[i32]; 5] = [&[0, 0], &[0, 5], &[0, 3, 6, 9], &[0, 19], &[0, 0, 0, 0]];
    let mut shares = Vec::new();
    for nices in share_cases {
        let mut runs = Vec::new();
        for _ in 0..plan.reps {
            runs.push(host::measure_shares(cpu, nices, plan.share_warmup, plan.share_window)?);
        }
        let window = runs.iter().map(|r| r.window_ns).sum::<u64>() / runs.len() as u64;
        let warmup = plan.share_warmup.as_nanos() as u64;
        let sims: Vec<Vec<f64>> = (0..plan.sim_seeds).map(|s| simcmp::shares(online, hz, nices, warmup, window, 77 + s)).collect();
        let total_w: f64 = nices.iter().map(|&n| f64::from(sched::weight::SCHED_PRIO_TO_WEIGHT[sched::weight::nice_to_index(n)])).sum();
        let ideal: Vec<f64> =
            nices.iter().map(|&n| f64::from(sched::weight::SCHED_PRIO_TO_WEIGHT[sched::weight::nice_to_index(n)]) / total_w).collect();
        let col = |rows: &[Vec<f64>], i: usize| -> Vec<f64> { rows.iter().map(|r| r[i]).collect() };
        let host_rows: Vec<Vec<f64>> = runs.iter().map(|r| r.shares.clone()).collect();
        let host_mean: Vec<f64> = (0..nices.len()).map(|i| summarize(&col(&host_rows, i)).mean).collect();
        let host_stdev: Vec<f64> = (0..nices.len()).map(|i| summarize(&col(&host_rows, i)).stdev).collect();
        let sim_mean: Vec<f64> = (0..nices.len()).map(|i| summarize(&col(&sims, i)).mean).collect();
        let sim_stdev: Vec<f64> = (0..nices.len()).map(|i| summarize(&col(&sims, i)).stdev).collect();
        let max_diff_pp = host_mean.iter().zip(&sim_mean).map(|(h, s)| (h - s).abs() * 100.0).fold(0.0f64, f64::max);
        eprintln!(
            "      nices {:?}: host {:?} simulador {:?} ideal {:?} (diferença máx. {:.2} pp, CPU nossa {:.0}%)",
            nices,
            host_mean.iter().map(|x| (x * 1000.0).round() / 10.0).collect::<Vec<_>>(),
            sim_mean.iter().map(|x| (x * 1000.0).round() / 10.0).collect::<Vec<_>>(),
            ideal.iter().map(|x| (x * 1000.0).round() / 10.0).collect::<Vec<_>>(),
            max_diff_pp,
            runs.iter().map(|r| r.ours_fraction).sum::<f64>() / runs.len() as f64 * 100.0
        );
        shares.push(ShareScenario { nices: nices.to_vec(), ideal, host_runs: runs, host_mean, host_stdev, sim_mean, sim_stdev, max_diff_pp });
    }

    let latency_cases: [(usize, Option<u64>); 5] = [(0, None), (1, None), (3, None), (1, Some(1)), (3, Some(1))];
    let mut latencies = Vec::new();
    for (hogs, slack) in latency_cases {
        let mut runs = Vec::new();
        for _ in 0..plan.reps {
            runs.push(host::measure_latency(cpu, hogs, slack, plan.latency_warmup, plan.latency_iters)?);
        }
        let slack_ns = runs[0].timer_slack_ns;
        let run_ns = e02_sched_eevdf::stats::median(&runs.iter().map(|r| r.sleeper_cpu_per_iter_ns).collect::<Vec<_>>()) as u64;
        let window = runs.iter().map(|r| r.window_ns).sum::<u64>() / runs.len() as u64;
        let warmup = 200_000_000;
        let host_all: Vec<u64> = runs.iter().flat_map(|r| r.samples_ns.iter().copied()).collect();
        let sim_runs: Vec<Vec<u64>> =
            (0..plan.sim_seeds).map(|s| simcmp::latencies(online, hz, hogs, run_ns, slack_ns, warmup, window, 99 + s)).collect();
        let sim_all: Vec<u64> = sim_runs.iter().flatten().copied().collect();
        let host_q = latency_quantiles(&host_all);
        let sim_q = latency_quantiles(&sim_all);
        let ratio_p50 = ratio_of_magnitude(host_q.p50_us, sim_q.p50_us);
        let ratio_p99 = ratio_of_magnitude(host_q.p99_us, sim_q.p99_us);
        eprintln!(
            "      {hogs} laço(s), folga {slack_ns} ns: host p50 {:.1} p99 {:.1} máx {:.0} µs | simulador p50 {:.1} p99 {:.1} máx {:.0} µs | CPU/volta {run_ns} ns",
            host_q.p50_us, host_q.p99_us, host_q.max_us, sim_q.p50_us, sim_q.p99_us, sim_q.max_us
        );
        latencies.push(LatencyScenario {
            hogs,
            slack_ns,
            sleeper_cpu_per_iter_ns: run_ns as f64,
            host: host_q,
            host_runs: runs.iter().map(|r| latency_quantiles(&r.samples_ns)).collect(),
            host_meta: runs,
            sim: sim_q,
            sim_runs: sim_runs.iter().map(|r| latency_quantiles(r)).collect(),
            ratio_p50,
            ratio_p99,
        });
    }
    Ok((shares, latencies))
}
