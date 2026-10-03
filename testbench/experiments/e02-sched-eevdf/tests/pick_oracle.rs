//! H12: `pick_eevdf` aumentado contra força bruta com aritmética exata, e os benchmarks de pick
//! conferindo que as duas escolhas batem em todo estado.

use e02_sched_eevdf::{pickbench, pickcheck};

#[test]
fn pick_eevdf_matches_exact_brute_force() {
    let report = pickcheck::run(20_000);
    assert!(report.passed, "{}", report.failure.unwrap_or_default());
}

#[test]
fn benchmark_states_agree() {
    for n in [8usize, 64, 512] {
        for (family, states) in [
            ("simulated", pickbench::simulated_states(n, 6, n as u64)),
            ("mixed_nice", pickbench::mixed_nice_states(n, 6, n as u64)),
            ("independent", pickbench::independent_states(n, 6, n as u64)),
        ] {
            for s in &states {
                s.timeline.check_invariants().expect("linha do tempo coerente");
                let fast = s.pick_eevdf();
                let slow = s.linear.pick(s.nr_running, s.curr.as_ref(), true);
                let tree_linear = s.pick_tree_linear();
                assert_eq!(fast, slow, "família {family}, n = {n}");
                assert_eq!(fast, tree_linear, "família {family}, n = {n}");
            }
        }
    }
}
