//! Hierarquia de grupos, controle de banda e várias CPUs: sequências aleatórias de todas as operações
//! públicas com as invariantes conferidas depois de cada uma, e propriedades de divisão de CPU no
//! simulador.

use sched::{
    BalanceConfig, Features, GroupId, ManualClock, Sched, SchedConfig, SimConfig, SimGroup, SimTask, TaskId, Topology,
    Tunables, simulate,
};

const MS: u64 = 1_000_000;

/// Gerador SplitMix64.
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn check(s: &Sched<ManualClock>, ctx: &str) {
    if let Err(e) = s.check_invariants() {
        panic!("{ctx}: {e}");
    }
    for (sum, w) in s.weighted_lag_sums() {
        assert!(sum <= 0 && (w == 0 || sum > -w), "{ctx}: Σ w (V - v) = {sum} fora de (-{w}, 0]");
    }
    for cpu in 0..s.nr_cpus() {
        assert_eq!(s.peek_pick(cpu), s.peek_pick_linear(cpu), "{ctx}: pick aumentado diverge do linear na CPU {cpu}");
    }
}

fn random_run(seed: u64, cpus: usize, topology: Topology, smt: bool, ops: usize) -> (u64, u64) {
    let mut rng = SplitMix(seed);
    let clock = ManualClock::new(0);
    let mut cfg = SchedConfig::new(cpus, topology, Tunables::linux_6_12_101(16, 250), Features::default());
    if topology == Topology::PerCpu && cpus > 1 && smt {
        cfg.balance = BalanceConfig::smt_domain(cpus);
    }
    let mut s = Sched::new(clock.clone(), cfg);
    let mut groups = vec![GroupId::ROOT];
    for _ in 0..1 + rng.below(6) {
        let parent = groups[rng.below(groups.len() as u64) as usize];
        let g = s.create_group(parent);
        if rng.below(2) == 0 {
            s.set_group_weight(g, 1 + rng.below(1000));
        }
        if rng.below(3) == 0 {
            s.set_group_period_offset(g, rng.below(100 * MS));
            let period = (10 + rng.below(90)) * MS;
            let quota = (1 + rng.below(60)) * MS;
            s.set_group_bandwidth(g, Some(quota), period).expect("cpu.max");
        }
        groups.push(g);
    }
    let mut now = 0u64;
    let mut throttles = 0u64;
    let mut migrations = 0u64;
    for step in 0..ops {
        now += rng.below(3 * MS);
        clock.set(now);
        while s.next_timer_ns().is_some_and(|t| t <= now) {
            s.run_timers();
        }
        let cpu = rng.below(cpus as u64) as usize;
        let tasks: Vec<TaskId> = s.task_ids().collect();
        match rng.below(20) {
            0..=2 => {
                if tasks.len() < 30 {
                    let g = groups[rng.below(groups.len() as u64) as usize];
                    let t = s.create_task(rng.below(40) as i32 - 20, g, cpu);
                    if topology == Topology::PerCpu && cpus > 1 && rng.below(3) == 0 {
                        let mask = (1u64 << cpu) | (rng.next() & ((1u64 << cpus) - 1));
                        s.set_affinity(t, mask);
                    }
                    if rng.below(4) == 0 {
                        s.set_custom_slice(t, Some(rng.below(5 * MS)));
                    }
                    s.wake_up_new_task(t);
                }
            }
            3..=8 => s.tick(cpu),
            9 | 10 => {
                if s.current(cpu).is_some() {
                    s.schedule(cpu, true);
                }
            }
            11 | 12 => {
                let sleeping: Vec<TaskId> = tasks.iter().copied().filter(|&t| s.task(t).sleeping).collect();
                if !sleeping.is_empty() {
                    s.try_to_wake_up(sleeping[rng.below(sleeping.len() as u64) as usize]);
                }
            }
            13 => {
                if !tasks.is_empty() {
                    let t = tasks[rng.below(tasks.len() as u64) as usize];
                    s.set_user_nice(t, rng.below(40) as i32 - 20);
                }
            }
            14 => {
                if !tasks.is_empty() {
                    let t = tasks[rng.below(tasks.len() as u64) as usize];
                    let v = if rng.below(2) == 0 { None } else { Some(rng.below(3 * MS)) };
                    s.set_custom_slice(t, v);
                }
            }
            15 => {
                if s.current(cpu).is_some() {
                    s.yield_current(cpu);
                }
            }
            16 => {
                if s.current(cpu).is_some() && rng.below(3) == 0 {
                    s.exit_current(cpu);
                }
            }
            17 if groups.len() > 1 => {
                let g = groups[1 + rng.below(groups.len() as u64 - 1) as usize];
                s.set_group_weight(g, 1 + rng.below(10_000));
            }
            18 if groups.len() > 1 => {
                let g = groups[1 + rng.below(groups.len() as u64 - 1) as usize];
                let q = if rng.below(3) == 0 { None } else { Some((1 + rng.below(40)) * MS) };
                s.set_group_bandwidth(g, q, (5 + rng.below(95)) * MS).expect("cpu.max");
            }
            _ => {}
        }
        for _ in 0..4 {
            for c in 0..cpus {
                if s.need_resched(c) {
                    s.schedule(c, false);
                }
            }
        }
        check(&s, &format!("semente {seed}, {cpus} CPUs {topology:?}, passo {step}"));
        throttles = groups.iter().skip(1).map(|&g| s.group(g).nr_throttled).sum::<u64>().max(throttles);
        migrations = s.task_ids().map(|t| s.task(t).nr_migrations).sum::<u64>().max(migrations);
    }
    (throttles, migrations)
}

#[test]
fn random_operations_one_cpu() {
    let mut throttles = 0;
    for seed in 0..40 {
        throttles += random_run(seed, 1, Topology::PerCpu, false, 1200).0;
    }
    assert!(throttles > 0, "as sequências precisam exercitar o estrangulamento");
}

#[test]
fn random_operations_per_cpu_with_balancing() {
    let (mut throttles, mut migrations) = (0, 0);
    for seed in 0..40 {
        let cpus = 2 + (seed % 3) as usize;
        let (t, m) = random_run(1000 + seed, cpus, Topology::PerCpu, seed % 2 == 0, 1200);
        throttles += t;
        migrations += m;
    }
    assert!(throttles > 0 && migrations > 0, "estrangulamentos {throttles}, migrações {migrations}");
}

#[test]
fn random_operations_shared_runqueue() {
    let mut throttles = 0;
    for seed in 0..40 {
        let cpus = 2 + (seed % 3) as usize;
        throttles += random_run(2000 + seed, cpus, Topology::Shared, false, 1200).0;
    }
    assert!(throttles > 0);
}

fn sim_config(cpus: usize, topology: Topology) -> SimConfig {
    let mut sc = SchedConfig::new(cpus, topology, Tunables::linux_6_12_101(16, 250), Features::default());
    if topology == Topology::PerCpu && cpus > 1 {
        sc.balance = BalanceConfig::smt_domain(cpus);
    }
    SimConfig { sched: sc, duration_ns: 6_000 * MS, tick_phase_ns: 1_234_567, measure_from_ns: 1_000 * MS, sample_every_ns: None }
}

fn one_vs_eight(weight_b: u64) -> (Vec<SimGroup>, Vec<SimTask>) {
    let groups = vec![SimGroup::new("A", None, 100), SimGroup::new("B", None, weight_b)];
    let mut tasks = vec![SimTask::cpu_bound("a", 0).in_group(0)];
    for i in 0..8 {
        tasks.push(SimTask::cpu_bound(&format!("b{i}"), 0).in_group(1));
    }
    (groups, tasks)
}

/// Numa CPU e na runqueue única, a divisão entre grupos é a dos pesos, independente de quantas tarefas
/// cada grupo tem. Com uma runqueue por CPU, o balanceamento chega perto disso.
#[test]
fn group_share_follows_weight_not_task_count() {
    for (cpus, topology, tol) in [(1, Topology::PerCpu, 0.005), (2, Topology::Shared, 0.01), (2, Topology::PerCpu, 0.05)] {
        for (wb, ideal) in [(100, 0.5), (300, 0.25)] {
            let (g, t) = one_vs_eight(wb);
            let r = simulate(sim_config(cpus, topology), &g, &t);
            let a = r.groups[0].share;
            assert!((a - ideal).abs() <= tol, "{cpus} CPUs {topology:?}, pesos 100/{wb}: A teve {a:.4}, ideal {ideal}");
        }
    }
}

/// Hierarquia usuário > sandbox > processo: dois usuários de peso igual dividem meio a meio; dentro
/// do segundo, sandboxes de peso 100 e 300 dividem 1 pra 3, com 4 e 2 tarefas.
#[test]
fn nested_hierarchy_divides_by_weight_at_each_level() {
    let groups = vec![
        SimGroup::new("u1", None, 100),
        SimGroup::new("u1s", Some(0), 100),
        SimGroup::new("u2", None, 100),
        SimGroup::new("u2s1", Some(2), 100),
        SimGroup::new("u2s2", Some(2), 300),
    ];
    let mut tasks = vec![SimTask::cpu_bound("a", 0).in_group(1)];
    tasks.extend((0..4).map(|i| SimTask::cpu_bound(&format!("s1-{i}"), 0).in_group(3)));
    tasks.extend((0..2).map(|i| SimTask::cpu_bound(&format!("s2-{i}"), 0).in_group(4)));
    for (cpus, topology, tol) in [(1, Topology::PerCpu, 0.005), (2, Topology::Shared, 0.01), (2, Topology::PerCpu, 0.05)] {
        let r = simulate(sim_config(cpus, topology), &groups, &tasks);
        let share = |i: usize| r.groups[i].share;
        for (i, ideal) in [(0, 0.5), (2, 0.5), (3, 0.125), (4, 0.375)] {
            assert!((share(i) - ideal).abs() <= tol, "{cpus} CPUs {topology:?}: {} teve {:.4}, ideal {ideal}", r.groups[i].name, share(i));
        }
    }
}

/// `cpu.max` limita o grupo à quota, sozinho ou disputando, e o padrão é rodar no começo do período
/// e ficar estrangulado no resto.
#[test]
fn quota_limits_cpu_fraction() {
    for (quota_ms, competing) in [(20, false), (50, false), (20, true), (50, true)] {
        let mut groups = vec![SimGroup::new("Q", None, 100).with_quota(quota_ms * MS, 100 * MS, 37 * MS)];
        let mut tasks = vec![SimTask::cpu_bound("q", 0).in_group(0)];
        if competing {
            groups.push(SimGroup::new("O", None, 100));
            tasks.push(SimTask::cpu_bound("o", 0).in_group(1));
        }
        let mut cfg = sim_config(1, Topology::PerCpu);
        cfg.sample_every_ns = Some(MS);
        let r = simulate(cfg, &groups, &tasks);
        let frac = r.groups[0].cpu_ns as f64 / r.window_ns as f64;
        let ideal = (quota_ms as f64 / 100.0).min(if competing { 0.5 } else { 1.0 });
        assert!((frac - ideal).abs() < 0.01, "quota {quota_ms}%, disputando {competing}: {frac:.4}");
        if !competing || quota_ms < 50 {
            assert!(r.groups[0].nr_throttled + 1 >= r.groups[0].nr_periods, "estrangulado em todo período");
        }
    }
}
