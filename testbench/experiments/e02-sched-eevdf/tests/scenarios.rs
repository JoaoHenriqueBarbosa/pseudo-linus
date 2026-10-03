//! Cenários de grupos (H41, H42): divisão ideal, análise do padrão de estrangulamento, simulador e
//! fumaça dos cgroups reais.

use std::time::Duration;

use e02_sched_eevdf::cgroups::Session;
use e02_sched_eevdf::scenario::{Scenario, SimOpts, analyze_pattern, group_samples, run_host, run_sim};
use sched::Topology;

const MS: u64 = 1_000_000;

#[test]
fn ideal_shares_follow_weights_per_level() {
    let close = |a: &[f64], b: &[f64]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-12);
    assert!(close(&Scenario::one_vs_eight(100).ideal_shares(), &[0.5, 0.5]));
    assert!(close(&Scenario::one_vs_eight(300).ideal_shares(), &[0.25, 0.75]));
    assert!(close(&Scenario::hierarchy().ideal_shares(), &[0.5, 0.5, 0.5, 0.125, 0.375]));
}

/// Amostras sintéticas no formato do `cpu.stat` de um laço com quota de 20 ms por 100 ms e fase de
/// 37 ms: o uso anda em degraus de 4 ms (os ticks) durante 20 ms e para até o período seguinte.
#[test]
fn pattern_of_synthetic_throttling() {
    let mut samples = Vec::new();
    let mut usage = 0u64;
    for ms in 0..3000u64 {
        let t = ms * MS;
        let in_period = (ms + 100 - 37) % 100;
        if in_period > 0 && in_period <= 20 && in_period % 4 == 0 {
            usage += 4 * MS;
        }
        samples.push((t, usage));
    }
    let p = analyze_pattern(&samples, 100 * MS);
    assert!(p.periods >= 28, "{p:?}");
    assert!((p.throttled_frac - 1.0).abs() < 1e-9, "{p:?}");
    assert!((p.median_usage_ms - 20.0).abs() < 4.1, "{p:?}");
    assert!((p.frac - 0.2).abs() < 0.01, "{p:?}");
    assert!(p.median_run_ms > 14.0 && p.median_run_ms < 22.0, "{p:?}");
}

#[test]
fn simulator_runs_quota_and_hierarchy_scenarios() {
    let opts = SimOpts { sample_ns: Some(MS), ..SimOpts::new(1, Topology::PerCpu, 500 * MS, 2_000 * MS, 1) };
    let r = run_sim(&Scenario::quota(20, false), &opts);
    assert!((r.cpu_frac[0] - 0.2).abs() < 0.01, "{:?}", r.cpu_frac);
    let p = analyze_pattern(&group_samples(&r, 0), 100 * MS);
    assert!(p.throttled_frac > 0.9, "{p:?}");
    let r = run_sim(&Scenario::hierarchy(), &SimOpts::new(1, Topology::PerCpu, 500 * MS, 2_000 * MS, 2));
    let ideal = Scenario::hierarchy().ideal_shares();
    for (s, i) in r.shares.iter().zip(&ideal) {
        assert!((s - i).abs() < 0.01, "{:?}", r.shares);
    }
}

/// A distância do timer de período até o tick seguinte decide em que tick a quota de 20 ms acaba: com
/// o tick logo antes do fim de cada 4 ms (3,9 ms), a quota acaba no 5º tick (3,9 + 4 * 4 = 19,9 ms,
/// e o 6º passa dos 20); com 1 ms, a dívida alterna e a execução varia entre 17 e 21 ms. O primeiro
/// degrau de uso reproduz a distância pedida.
#[test]
fn tick_lead_sets_where_the_quota_runs_out() {
    let pattern = |lead_ns: u64| {
        let opts = SimOpts { sample_ns: Some(MS), tick_lead_ns: Some(lead_ns), ..SimOpts::new(1, Topology::PerCpu, 500 * MS, 3_000 * MS, 3) };
        analyze_pattern(&group_samples(&run_sim(&Scenario::quota(20, false), &opts), 0), 100 * MS)
    };
    let late = pattern(3_900_000);
    assert!((late.first_step_ms - 3.9).abs() < 1e-6, "{late:?}");
    let early = pattern(1_000_000);
    assert!((early.first_step_ms - 1.0).abs() < 1e-6, "{early:?}");
    assert!((late.frac - 0.2).abs() < 0.005 && (early.frac - 0.2).abs() < 0.005, "{late:?} {early:?}");
    assert!(late.median_run_ms != early.median_run_ms || late.median_usage_ms != early.median_usage_ms, "{late:?} {early:?}");
}

/// Fumaça com cgroups v2 reais: sobe um cenário curto, confere a forma do resultado e que a limpeza
/// não deixou unidade. Sem o controlador `cpu` delegado ao usuário (máquina sem systemd de usuário),
/// o teste avisa e não roda.
#[test]
fn real_cgroups_smoke() {
    let mut session = match Session::with_exe(env!("CARGO_BIN_EXE_e02-sched-eevdf").into()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cgroups v2 indisponíveis, fumaça pulada: {e:#}");
            return;
        }
    };
    let choice = e02_sched_eevdf::cpusel::choose(Duration::from_millis(100)).expect("CPU");
    let run = run_host(&mut session, &Scenario::quota(50, true), &[choice.measure_cpu], Duration::from_millis(300), Duration::from_millis(400), None)
        .expect("cenário no host");
    assert!((run.shares.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    assert!(run.stats[0].nr_periods >= 3, "{:?}", run.stats);
    session.cleanup().expect("limpeza");
}
