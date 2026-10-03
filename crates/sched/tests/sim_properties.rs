//! Propriedades de longo prazo do EEVDF, verificadas no simulador com a configuração do host de teste
//! (16 CPUs online, fatia 2,8 ms, HZ=250), numa CPU e sem grupos.
//!
//! - Divisão de CPU proporcional ao peso entre tarefas CPU-bound.
//! - Lag limitado: em todo instante, o lag em tempo real `w_i * (V - v_i) / 1024` de cada entidade
//!   na fila fica dentro de `fatia + tick`. A conta: quem roda só perde a CPU quando um tick nota que a
//!   deadline passou, então recebe no máximo `fatia + tick` de serviço de uma vez (lag negativo); e
//!   quem espera é elegível e, com deadline no máximo uma fatia à frente, é escolhido antes que o
//!   atraso passe de uma rodada desse tamanho (lag positivo).
//! - Latência de wakeup limitada: um processo que dorme 1 ms em laço espera no máximo
//!   `folga + fatia + tick` por CPU com um laço de CPU competindo (a proteção de fatia do concorrente
//!   só cai no tick seguinte ao fim da fatia).

use sched::weight::{SCHED_PRIO_TO_WEIGHT, nice_to_index};
use sched::{Features, SimConfig, SimTask, Simulator, Tunables, percentile, simulate};

const MS: u64 = 1_000_000;

fn config(duration_ns: u64, phase: u64) -> SimConfig {
    SimConfig::single_cpu(Tunables::linux_6_12_101(16, 250), Features::default(), duration_ns, phase, 0)
}

fn weight(nice: i32) -> f64 {
    f64::from(SCHED_PRIO_TO_WEIGHT[nice_to_index(nice)])
}

#[test]
fn cpu_share_is_proportional_to_weight() {
    let scenarios: &[&[i32]] = &[&[0, 0, 0], &[0, 5], &[0, 3, 6, 9], &[0, 19], &[-5, 0, 5], &[0, 1, 2, 3, 4, 5, 6, 7]];
    for nices in scenarios {
        let tasks: Vec<SimTask> = nices.iter().enumerate().map(|(i, &n)| SimTask::cpu_bound(&format!("t{i}"), n)).collect();
        let report = simulate(config(20_000 * MS, 1_234_567), &[], &tasks);
        let total_w: f64 = nices.iter().map(|&n| weight(n)).sum();
        for (t, &n) in report.tasks.iter().zip(nices.iter()) {
            let ideal = weight(n) / total_w;
            assert!(
                (t.share - ideal).abs() < 0.005,
                "nices {nices:?}: {} (nice {n}) teve {:.4} da CPU, ideal {ideal:.4}",
                t.name,
                t.share
            );
        }
    }
}

#[test]
fn lag_stays_within_slice_plus_tick() {
    let bound = 2_800_000i128 + 4_000_000;
    let scenarios: Vec<Vec<SimTask>> = vec![
        (0..4).map(|i| SimTask::cpu_bound(&format!("h{i}"), 0)).collect(),
        vec![SimTask::cpu_bound("a", 0), SimTask::cpu_bound("b", 5), SimTask::cpu_bound("c", 10), SimTask::cpu_bound("d", -5)],
        vec![
            SimTask::cpu_bound("a", 0),
            SimTask::cpu_bound("b", 0),
            SimTask::periodic("s", 0, 20_000, MS, 50_000),
            SimTask::periodic("t", 3, 500_000, 2 * MS, 50_000),
        ],
    ];
    for (k, tasks) in scenarios.iter().enumerate() {
        for phase in [0, 1_111_111, 3_999_999] {
            let mut sim = Simulator::new(config(3_000 * MS, phase), &[], tasks);
            let mut worst = 0i128;
            while sim.step() {
                worst = worst.max(sim.sched().max_abs_real_lag());
                if let Err(e) = sim.sched().check_invariants() {
                    panic!("cenário {k}: {e}");
                }
            }
            assert!(worst <= bound, "cenário {k}, fase {phase}: lag de {worst} ns passou de {bound}");
        }
    }
}

#[test]
fn wakeup_latency_is_bounded_with_one_hog() {
    let slack = 50_000u64;
    for phase in [0, 777_777, 2_500_000] {
        let tasks = vec![SimTask::cpu_bound("hog", 0), SimTask::periodic("sleeper", 0, 5_000, MS, slack)];
        let report = simulate(config(5_000 * MS, phase), &[], &tasks);
        let lat = &report.tasks[1].latencies_ns;
        assert!(lat.len() > 1000, "poucos despertares: {}", lat.len());
        let max = *lat.iter().max().expect("não vazio");
        let bound = slack + 2_800_000 + 4_000_000;
        assert!(max <= bound, "fase {phase}: latência máxima {max} passou de {bound}");
        // A maior parte dos despertares preempta na hora (lag positivo do sleeper).
        let p50 = percentile(lat, 50.0).expect("não vazio");
        assert!(p50 <= slack, "fase {phase}: mediana {p50}");
    }
}

#[test]
fn sleeper_alone_wakes_at_timer_expiry() {
    let tasks = vec![SimTask::periodic("sleeper", 0, 5_000, MS, 50_000)];
    let report = simulate(config(1_000 * MS, 0), &[], &tasks);
    let lat = &report.tasks[0].latencies_ns;
    assert!(lat.iter().all(|&l| l == 50_000), "sem concorrência a latência é a folga do timer");
}
