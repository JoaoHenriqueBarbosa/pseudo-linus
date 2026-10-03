//! Testes de unidade da hierarquia e da banda, com valores feitos à mão a partir das fórmulas do
//! fair.c (comentados em cada teste).

use crate::balance::BalanceConfig;
use crate::clock::ManualClock;
use crate::features::{Features, Tunables};
use crate::sched::{Sched, SchedConfig, Topology};
use crate::weight::NICE_0_LOAD;
use crate::{GroupId, TaskId};

const MS: u64 = 1_000_000;

fn sched(cpus: usize) -> (ManualClock, Sched<ManualClock>) {
    let clock = ManualClock::new(0);
    let mut cfg = SchedConfig::new(cpus, Topology::PerCpu, Tunables::linux_6_12_101(16, 250), Features::default());
    cfg.balance = BalanceConfig::disabled();
    (clock.clone(), Sched::new(clock, cfg))
}

fn ok<C: crate::Clock>(s: &Sched<C>) {
    if let Err(e) = s.check_invariants() {
        panic!("invariante violada: {e}");
    }
}

/// Numa CPU, o peso da entidade de grupo é exatamente o shares do grupo.
///
/// `cpu.weight` 300 → `sched_weight_from_cgroup` = 3072 → shares = 3072 << 10 = 3145728. A tarefa
/// nova entra com `load_avg` 1024 (`init_entity_runnable_average`), é anexada à fila do grupo, que vai
/// a `load_avg` 1024, e o `update_tg_load_avg` leva `tg->load_avg` a 1024. No `calc_group_shares`:
/// load = max(1024, 1024), tg_weight = 1024 - 1024 + 1024, shares = 3145728 * 1024 / 1024.
#[test]
fn group_entity_weight_is_shares_on_one_cpu() {
    let (_clock, mut s) = sched(1);
    let g = s.create_group(GroupId::ROOT);
    s.set_group_weight(g, 300);
    assert_eq!(s.group(g).shares, 3072 << 10);
    let t = s.create_task(0, g, 0);
    s.wake_up_new_task(t);
    assert_eq!(s.group(g).se_weight, vec![3_145_728]);
    assert_eq!(s.group(g).load_avg, 1024);
    s.schedule(0, false);
    assert_eq!(s.current(0), Some(t));
    ok(&s);
}

/// Duas CPUs, grupo com 3 tarefas na CPU 0 e 1 na CPU 1: o peso se divide pela carga de cada CPU.
///
/// Depois que as contribuições assentam (`tg->load_avg` = 3072 + 1024 = 4096):
/// CPU 0: 1048576 * 3072 / (4096 - 3072 + 3072) = 786432; CPU 1: 1048576 * 1024 / 4096 = 262144.
///
/// Logo depois da criação o retrato é outro, como no kernel: as três tarefas da CPU 0 entram com a
/// CPU 1 vazia, então a entidade da CPU 0 fica com o peso cheio (1048576) e só é recalculada num evento
/// da CPU 0; a da CPU 1 vê `tg->load_avg` = 2048 (a contribuição da CPU 0 subiu só uma vez, por causa do
/// limite de uma atualização por milissegundo) e calcula 1048576 * 1024 / (2048 - 1024 + 1024) = 524288.
/// Os ticks seguintes corrigem as duas.
#[test]
fn group_weight_splits_by_per_cpu_load() {
    let (clock, mut s) = sched(2);
    let g = s.create_group(GroupId::ROOT);
    let on0: Vec<TaskId> = (0..3).map(|_| s.create_task(0, g, 0)).collect();
    let on1 = s.create_task(0, g, 1);
    for &t in &on0 {
        s.wake_up_new_task(t);
    }
    s.wake_up_new_task(on1);
    assert_eq!(s.group(g).load_avg, 2048);
    assert_eq!(s.group(g).se_weight, vec![NICE_0_LOAD, 524_288]);
    for c in 0..2 {
        s.schedule(c, false);
    }
    for k in 1..=10 {
        clock.set(k * 4 * MS);
        for c in 0..2 {
            s.tick(c);
            if s.need_resched(c) {
                s.schedule(c, false);
            }
        }
        ok(&s);
    }
    let w = s.group(g).se_weight;
    let near = |a: u64, b: u64| (a as f64 / b as f64 - 1.0).abs() < 0.01;
    assert!(near(w[0], 786_432) && near(w[1], 262_144), "pesos {w:?}");
}

/// Carga bloqueada: o grupo tem uma tarefa em cada CPU; a da CPU 0 dorme e a CPU 0 fica ociosa. A
/// fila do grupo na CPU 0 guarda a carga de quando a tarefa rodava (cerca de 1024) e, sem decair, o
/// `tg->load_avg` ficaria perto de 2048 e a entidade do grupo na CPU 1 com metade do shares. O tick da
/// CPU 1 atualiza a carga bloqueada da CPU ociosa a cada 32 ms (`NOHZ_STATS_KICK`); com meia-vida de
/// 32 ms, em 400 ms sobram 1024 / 2^12,5, menos de 1, e a entidade na CPU 1 volta ao shares inteiro.
#[test]
fn blocked_load_of_idle_cpu_decays_into_tg_load() {
    let clock = ManualClock::new(0);
    let cfg = SchedConfig::new(2, Topology::PerCpu, Tunables::linux_6_12_101(16, 250), Features::default());
    let mut s = Sched::new(clock.clone(), cfg);
    let g = s.create_group(GroupId::ROOT);
    let t0 = s.create_task(0, g, 0);
    let t1 = s.create_task(0, g, 1);
    s.wake_up_new_task(t0);
    s.wake_up_new_task(t1);
    for c in 0..2 {
        s.schedule(c, false);
    }
    let mut now = 0;
    let mut run_until = |s: &mut Sched<ManualClock>, until: u64, cpus: &[usize]| {
        while now < until {
            now += 4 * MS;
            clock.set(now);
            for &c in cpus {
                s.tick(c);
                if s.need_resched(c) {
                    s.schedule(c, false);
                }
            }
            ok(s);
        }
    };
    run_until(&mut s, 200 * MS, &[0, 1]);
    let before = s.group(g).load_avg;
    assert!((1900..=2048).contains(&before), "tg->load_avg com as duas rodando: {before}");
    assert_eq!(s.schedule(0, true), None, "a tarefa da CPU 0 dorme e a CPU fica ociosa");
    run_until(&mut s, 600 * MS, &[1]);
    let after = s.group(g).load_avg;
    assert!((1000..=1030).contains(&after), "tg->load_avg depois de 400 ms: {after}");
    let w = s.group(g).se_weight[1];
    assert!(w >= NICE_0_LOAD * 99 / 100, "entidade do grupo na CPU 1: {w}");
}

/// Ciclo de banda com quota de 10 ms por 100 ms e tick de 4 ms, à mão.
///
/// t = 0: a fila pega 5 ms do pool (fica com 5 ms no pool). Tick de 4 ms: sobra 1 ms. Tick de 8 ms:
/// -3 ms, pega mais 5 ms (o pool esvazia), fica com 2 ms. Tick de 12 ms: -2 ms e o pool está vazio:
/// pede reescalonamento, a fila é estrangulada no pick e a CPU fica ociosa. O grupo usou 12 ms do
/// período (2 ms de dívida). Em 100 ms o timer reabastece 10 ms e dá 2 ms + 1 ns pra fila, que volta.
#[test]
fn bandwidth_cycle_by_hand() {
    let (clock, mut s) = sched(1);
    let g = s.create_group(GroupId::ROOT);
    s.set_group_bandwidth(g, Some(10 * MS), 100 * MS).expect("cpu.max");
    assert_eq!(s.next_timer_ns(), Some(100 * MS));
    let t = s.create_task(0, g, 0);
    s.wake_up_new_task(t);
    s.schedule(0, false);
    assert_eq!(s.group(g).pool_runtime_ns, 5 * MS);
    for k in 1..=3 {
        clock.set(k * 4 * MS);
        s.tick(0);
        if s.need_resched(0) {
            s.schedule(0, false);
        }
        ok(&s);
    }
    assert_eq!(s.current(0), None, "estrangulado em 12 ms");
    assert_eq!(s.group(g).throttled, vec![true]);
    assert_eq!(s.task(t).sum_exec_runtime, 12 * MS);
    assert_eq!(s.group(g).pool_runtime_ns, 0);

    clock.set(100 * MS);
    s.run_timers();
    assert_eq!(s.group(g).throttled, vec![false]);
    assert_eq!(s.group(g).pool_runtime_ns, 8 * MS - 1);
    assert_eq!(s.group(g).nr_periods, 1);
    assert_eq!(s.group(g).nr_throttled, 1);
    assert_eq!(s.group(g).throttled_time_ns, 88 * MS);
    assert!(s.need_resched(0));
    assert_eq!(s.schedule(0, false), Some(t));
    ok(&s);
}

/// Quota menor que 1 ms ou período fora de [1 ms, 1 s] são recusados; o raiz não aceita limite.
#[test]
fn bandwidth_limits_are_validated() {
    let (_clock, mut s) = sched(1);
    let g = s.create_group(GroupId::ROOT);
    assert!(s.set_group_bandwidth(g, Some(500_000), 100 * MS).is_err());
    assert!(s.set_group_bandwidth(g, Some(10 * MS), 2_000 * MS).is_err());
    assert!(s.set_group_bandwidth(GroupId::ROOT, Some(10 * MS), 100 * MS).is_err());
    assert!(s.set_group_bandwidth(g, None, 100 * MS).is_ok());
}

/// Hierarquia de três níveis numa CPU: a entidade do grupo do meio tem o peso dos shares dele, e a
/// tarefa do grupo de baixo é escolhida descendo da raiz.
#[test]
fn nested_groups_pick_down_the_hierarchy() {
    let (clock, mut s) = sched(1);
    let user = s.create_group(GroupId::ROOT);
    let sandbox = s.create_group(user);
    s.set_group_weight(sandbox, 50);
    let t = s.create_task(0, sandbox, 0);
    s.wake_up_new_task(t);
    let other = s.create_task(0, GroupId::ROOT, 0);
    s.wake_up_new_task(other);
    s.schedule(0, false);
    assert_eq!(s.group(user).se_weight, vec![NICE_0_LOAD]);
    assert_eq!(s.group(sandbox).se_weight, vec![512 << 10]);
    for k in 1..=500 {
        clock.set(k * 4 * MS);
        s.tick(0);
        if s.need_resched(0) {
            s.schedule(0, false);
        }
    }
    ok(&s);
    // O grupo do usuário disputa com a tarefa do raiz por peso 1024 contra 1024.
    let a = s.task_runtime_now(t) as f64;
    let b = s.task_runtime_now(other) as f64;
    assert!((a / (a + b) - 0.5).abs() < 0.01, "{a} contra {b}");
}
