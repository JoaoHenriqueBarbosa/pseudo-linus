//! Fumaça das partes de medição: rodam, devolvem números coerentes e não travam. Os números em si são
//! resultado do experimento, não condição de teste.

use std::time::Duration;

use e02_sched_eevdf::{cpusel, host, lockscale, opbench, simcmp};

#[test]
fn op_bench_runs() {
    let o = opbench::bench_size(64);
    for x in [o.insert_rb, o.insert_bt, o.remove_rb_handle, o.remove_bt, o.leftmost_rb, o.churn_rb_aug] {
        assert!(x.is_finite() && x > 0.0);
    }
}

#[test]
fn lock_scaling_runs() {
    let points = lockscale::run(&[1, 2], &[0], Duration::from_millis(20), 1);
    assert_eq!(points.len(), 2);
    assert!(points.iter().all(|p| p.global_ops_per_s > 0.0 && p.per_worker_ops_per_s > 0.0));
}

#[test]
fn sim_scenarios_match_ideal_shares() {
    // nice 0 contra nice 5: 1024 / 1359 = 75,35%.
    let s = simcmp::shares(16, 250, &[0, 5], 300_000_000, 4_000_000_000, 1);
    assert!((s[0] - 0.7535).abs() < 0.01, "{s:?}");
    let lat = simcmp::latencies(16, 250, 1, 5_000, 50_000, 100_000_000, 1_000_000_000, 1);
    assert!(lat.len() > 500);
}

/// Mede de verdade no host, com janelas curtas: confere só a forma do resultado.
#[test]
fn host_measurements_run() {
    let choice = cpusel::choose(Duration::from_millis(100)).expect("escolher CPU");
    let s = host::measure_shares(choice.measure_cpu, &[0, 5], Duration::from_millis(50), Duration::from_millis(200)).expect("divisão");
    assert_eq!(s.shares.len(), 2);
    assert!((s.shares.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    assert!(s.cpu_ns.iter().all(|&c| c > 0));
    let l = host::measure_latency(choice.measure_cpu, 1, None, 10, 50).expect("latência");
    assert_eq!(l.samples_ns.len(), 50);
    assert!(l.timer_slack_ns > 0);
}
