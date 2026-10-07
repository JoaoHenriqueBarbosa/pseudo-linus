//! Testes de unidade da runqueue. Os valores esperados são feitos à mão a partir das fórmulas do
//! fair.c (comentados em cada teste), e o teste aleatório confere as invariantes depois de cada
//! operação.

use super::*;
use crate::clock::ManualClock;
use crate::timeline::INITIAL_ZERO_VRUNTIME as V0;
use crate::weight::scale_load_down;

const MS: u64 = 1_000_000;

fn rq16() -> (ManualClock, RunQueue<ManualClock>) {
    rq_with(Features::default())
}

fn rq_with(features: Features) -> (ManualClock, RunQueue<ManualClock>) {
    let clock = ManualClock::new(0);
    let rq = RunQueue::new(clock.clone(), Tunables::linux_6_12_101(16, 250), features);
    (clock, rq)
}

fn ok(rq: &RunQueue<ManualClock>) {
    if let Err(e) = rq.check_invariants() {
        panic!("invariante violada: {e}");
    }
}

/// Σ w_i * (V - v_i) sobre as entidades na fila, com `w_i = scale_load_down(peso)`.
/// Como V = v0 + piso(Σ w (v - v0) / W), a soma fica em (-W, 0].
fn weighted_lag_sum(rq: &RunQueue<ManualClock>) -> (i128, i128) {
    let v = rq.avg_vruntime_peek();
    let mut sum = 0i128;
    let mut total = 0i128;
    for (_, vr, w) in rq.queued_entities() {
        let w = i128::from(scale_load_down(w));
        sum += w * i128::from(v.wrapping_sub(vr) as i64);
        total += w;
    }
    (sum, total)
}

/// Tarefa nova numa runqueue vazia: V é o próprio `zero_vruntime` inicial, PLACE_LAG não se aplica
/// (`nr_running == 0`) e PLACE_DEADLINE_INITIAL dá meia fatia: 2,8 ms / 2 = 1,4 ms.
#[test]
fn first_task_starts_at_zero_vruntime_with_half_slice() {
    let (_clock, mut rq) = rq16();
    assert_eq!(rq.base_slice_ns(), 2_800_000);
    let a = rq.create_task(0);
    rq.wake_up_new_task(a);
    assert!(rq.need_resched(), "acordar com a CPU ociosa pede reescalonamento");
    assert_eq!(rq.schedule(false), Some(a));
    let ia = rq.task(a);
    assert_eq!(ia.vruntime, V0);
    assert_eq!(ia.deadline, V0.wrapping_add(1_400_000));
    // set_protect_slice guarda a deadline em vlag.
    assert_eq!(ia.vlag as u64, ia.deadline);
    ok(&rq);
}

/// A roda 1 ms sozinha (v = V0 + 1 ms). B nasce: V = vruntime de A (único na fila), lag 0, então
/// B.v = V0 + 1 ms e B.deadline = V0 + 1 ms + 1,4 ms. A segue protegida (RUN_TO_PARITY), sem preempção.
/// No tick de 4 ms: A.v = V0 + 4 ms passou da deadline (V0 + 1,4 ms), ganha deadline V0 + 4 + 2,8 ms
/// e pede reescalonamento. V = (4 + 1) / 2 = 2,5 ms: A não é elegível, B (v = 1 ms) é e vence.
#[test]
fn second_task_and_tick_preemption_by_hand() {
    let (clock, mut rq) = rq16();
    let a = rq.create_task(0);
    rq.wake_up_new_task(a);
    rq.schedule(false);

    clock.set(MS);
    let b = rq.create_task(0);
    rq.wake_up_new_task(b);
    assert!(!rq.need_resched(), "A está protegida pela fatia");
    let ib = rq.task(b);
    assert_eq!(rq.task(a).vruntime, V0.wrapping_add(MS));
    assert_eq!(ib.vruntime, V0.wrapping_add(MS));
    assert_eq!(ib.deadline, V0.wrapping_add(MS + 1_400_000));
    ok(&rq);

    clock.set(4 * MS);
    rq.tick();
    assert!(rq.need_resched());
    let ia = rq.task(a);
    assert_eq!(ia.vruntime, V0.wrapping_add(4 * MS));
    assert_eq!(ia.deadline, V0.wrapping_add(4 * MS + 2_800_000));
    assert_eq!(ia.sum_exec_runtime, 4 * MS);
    assert_eq!(rq.avg_vruntime_peek(), V0.wrapping_add(2_500_000));
    assert_eq!(rq.schedule(false), Some(b));
    assert!(!rq.need_resched());
    ok(&rq);
}

/// PLACE_LAG: com A (corrente) e B na fila, W = 1024 + 1024. Uma tarefa C nice 0 que dormiu com
/// vlag = 1 ms entra com lag inflado 1 ms * (2048 + 1024) / 2048 = 1,5 ms, ou seja v = V - 1,5 ms.
/// O V novo cai 0,5 ms e o lag efetivo volta a ser exatamente 1 ms.
#[test]
fn place_lag_inflates_and_preserves_lag() {
    let (clock, mut rq) = rq16();
    let a = rq.create_task(0);
    rq.wake_up_new_task(a);
    rq.schedule(false);
    clock.set(MS);
    let b = rq.create_task(0);
    rq.wake_up_new_task(b);
    // A e B com v = V0 + 1 ms; V = V0 + 1 ms.
    let c = rq.create_task(0);
    {
        let d = rq.s.td_mut(c);
        d.new = false;
        d.sleeping = true;
        rq.s.e_mut(c.entity()).vlag = MS as i64;
    }
    let v_before = rq.avg_vruntime_peek();
    assert_eq!(v_before, V0.wrapping_add(MS));
    rq.try_to_wake_up(c);
    let ic = rq.task(c);
    assert_eq!(ic.vruntime, v_before.wrapping_sub(1_500_000));
    assert_eq!(ic.deadline, ic.vruntime.wrapping_add(2_800_000));
    let v_after = rq.avg_vruntime_peek();
    assert_eq!(v_after, V0.wrapping_add(500_000));
    assert_eq!(v_after.wrapping_sub(ic.vruntime), MS);
    // Com V' = V0 + 0,5 ms, A (v = V0 + 1 ms) deixou de ser elegível e perde a proteção na disputa.
    // Na árvore, C (deadline V0 + 2,3 ms) vem antes de B (V0 + 2,4 ms) e é elegível: o pick é C, e
    // o wakeup pede preempção.
    assert_eq!(rq.peek_pick(), Some(c.entity()));
    assert!(rq.need_resched());
    ok(&rq);
}

/// `entity_lag` limita o lag a `calc_delta_fair(max(2 * slice, TICK_NSEC))`.
/// nice 0, fatia 2,8 ms: limite 5,6 ms. nice 19: 5,6 ms * 1024 / 15 pelo ponto fixo = 382293333.
/// nice 5: 17117612. Com 1 CPU (fatia 0,7 ms) o limite é o tick: 4 ms.
#[test]
fn entity_lag_is_clamped() {
    let (_clock, mut rq) = rq16();
    let a = rq.create_task(0);
    let z = rq.create_task(19);
    let f = rq.create_task(5);
    let base = 1_000_000_000u64;
    let (a, z, f) = (a.entity(), z.entity(), f.entity());
    // Tarefa nova tem vruntime 0: lag de 1 s, limitado a 5,6 ms.
    assert_eq!(rq.s.entity_lag(base, a), 5_600_000);
    rq.s.e_mut(a).vruntime = base;
    assert_eq!(rq.s.entity_lag(base + 10 * MS, a), 5_600_000);
    assert_eq!(rq.s.entity_lag(base - 10 * MS, a), -5_600_000);
    assert_eq!(rq.s.entity_lag(base + 3 * MS, a), 3_000_000);
    rq.s.e_mut(z).vruntime = base;
    assert_eq!(rq.s.entity_lag(base + 1_000 * MS, z), 382_293_333);
    rq.s.e_mut(f).vruntime = base;
    assert_eq!(rq.s.entity_lag(base - 100 * MS, f), -17_117_612);

    let clock = ManualClock::new(0);
    let mut rq1 = RunQueue::new(clock, Tunables::linux_6_12_101(1, 250), Features::default());
    let t = rq1.create_task(0).entity();
    rq1.s.e_mut(t).vruntime = base;
    assert_eq!(rq1.s.entity_lag(base + 10 * MS, t), 4_000_000);
}

/// DELAY_DEQUEUE: A roda de 0 a 2 ms; B nasce em 1 ms com v = V0 + 1 ms. Em 2 ms, A dorme com
/// v = V0 + 2 ms e V = V0 + 1,5 ms: não é elegível, então fica na árvore marcada como atrasada (continua
/// contando em nr_running e rq->nr_running). B roda. No tick de 4 ms, B.v = V0 + 3 ms passa da deadline
/// (V0 + 2,4 ms) e cede; V = (2 + 3) / 2 = 2,5 ms, A (deadline V0 + 1,4 ms) é elegível e é escolhida, mas
/// como está atrasada sai da fila de vez: lag = 0,5 ms > 0 vira 0 (DELAY_ZERO). B volta a rodar.
#[test]
fn delayed_dequeue_by_hand() {
    let (clock, mut rq) = rq16();
    let a = rq.create_task(0);
    rq.wake_up_new_task(a);
    rq.schedule(false);
    clock.set(MS);
    let b = rq.create_task(0);
    rq.wake_up_new_task(b);
    clock.set(2 * MS);
    assert_eq!(rq.schedule(true), Some(b));
    let ia = rq.task(a);
    assert!(ia.sched_delayed && ia.on_rq && ia.queued && ia.sleeping);
    assert_eq!(rq.nr_running(), 2);
    assert_eq!(rq.nr_delayed(), 1);
    assert_eq!(rq.rq_nr_running(), 2);
    ok(&rq);

    clock.set(4 * MS);
    rq.tick();
    assert!(rq.need_resched());
    assert_eq!(rq.schedule(false), Some(b));
    let ia = rq.task(a);
    assert!(!ia.sched_delayed && !ia.on_rq && !ia.queued && ia.sleeping);
    assert_eq!(ia.vlag, 0);
    assert_eq!(rq.stats().nr_delayed_dequeues, 1);
    assert_eq!(rq.nr_running(), 1);
    assert_eq!(rq.rq_nr_running(), 1);
    ok(&rq);
}

/// Atrasada que acorda antes de ser escolhida (`ttwu_runnable` + `requeue_delayed_entity`). Em 2,5 ms
/// o V usa o vruntime velho de B (o requeue não chama update_curr): (2 + 1) / 2 = 1,5 ms, lag de A =
/// -0,5 ms, negativo, então A fica onde está e só perde a marca. B continua protegida.
#[test]
fn delayed_task_woken_before_pick() {
    let (clock, mut rq) = rq16();
    let a = rq.create_task(0);
    rq.wake_up_new_task(a);
    rq.schedule(false);
    clock.set(MS);
    let b = rq.create_task(0);
    rq.wake_up_new_task(b);
    clock.set(2 * MS);
    rq.schedule(true);
    let v_a = rq.task(a).vruntime;
    clock.set(2_500_000);
    assert!(rq.try_to_wake_up(a));
    let ia = rq.task(a);
    assert!(!ia.sched_delayed && ia.on_rq && ia.queued && !ia.sleeping);
    assert_eq!(ia.vruntime, v_a);
    assert_eq!(ia.vlag, -500_000);
    assert_eq!(rq.nr_delayed(), 0);
    assert!(!rq.need_resched());
    ok(&rq);
}

/// Sem DELAY_DEQUEUE, quem dorme sai da fila na hora, mesmo sem ser elegível, com o lag negativo
/// guardado: V = 1,5 ms, v = 2 ms, vlag = -0,5 ms.
#[test]
fn without_delay_dequeue_sleep_dequeues_at_once() {
    let features = Features { delay_dequeue: false, ..Features::default() };
    let (clock, mut rq) = rq_with(features);
    let a = rq.create_task(0);
    rq.wake_up_new_task(a);
    rq.schedule(false);
    clock.set(MS);
    let b = rq.create_task(0);
    rq.wake_up_new_task(b);
    clock.set(2 * MS);
    rq.schedule(true);
    let ia = rq.task(a);
    assert!(!ia.on_rq && !ia.queued && !ia.sched_delayed);
    assert_eq!(ia.vlag, -500_000);
    ok(&rq);
}

/// RUN_TO_PARITY e PREEMPT_SHORT. A roda protegida; em 0,5 ms nasce B.
/// - B com fatia normal: entra com v = V, A segue elegível e protegida, sem preempção.
/// - B com fatia própria de 0,1 ms: `do_preempt_short` (fatia menor, elegível, deadline V + 0,05 ms
///   antes da de A em V0 + 1,4 ms) cancela a proteção e o pick passa a ser B: preempção.
/// - Mesma B sem PREEMPT_SHORT: A segue protegida.
#[test]
fn run_to_parity_and_preempt_short() {
    for (custom, features, expect) in [
        (None, Features::default(), false),
        (Some(100_000), Features::default(), true),
        (Some(100_000), Features { preempt_short: false, ..Features::default() }, false),
    ] {
        let (clock, mut rq) = rq_with(features);
        let a = rq.create_task(0);
        rq.wake_up_new_task(a);
        rq.schedule(false);
        clock.set(500_000);
        let b = rq.create_task(0);
        rq.set_custom_slice(b, custom);
        rq.wake_up_new_task(b);
        assert_eq!(rq.need_resched(), expect, "fatia {custom:?}, features {features:?}");
        if expect {
            assert!(!rq.s.protect_slice(a.entity()), "a proteção de A foi cancelada");
            assert_eq!(rq.schedule(false), Some(b));
        }
        ok(&rq);
    }
}

/// Fatia própria é limitada a [0,1 ms, 100 ms] e volta à base com `None`.
#[test]
fn custom_slice_is_clamped() {
    let (_clock, mut rq) = rq16();
    let a = rq.create_task(0);
    rq.set_custom_slice(a, Some(1));
    assert_eq!(rq.task(a).slice, 100_000);
    rq.set_custom_slice(a, Some(u64::MAX));
    assert_eq!(rq.task(a).slice, 100 * MS);
    rq.set_custom_slice(a, None);
    assert_eq!(rq.task(a).slice, 2_800_000);
    assert!(!rq.task(a).custom_slice);
}

/// Renice de uma tarefa na fila preserva o lag ponderado (`w * (V - v)`) e a deadline relativa.
#[test]
fn renice_preserves_weighted_lag_and_relative_deadline() {
    let (clock, mut rq) = rq16();
    let ids: Vec<TaskId> = (0..3).map(|_| rq.create_task(0)).collect();
    for &t in &ids {
        rq.wake_up_new_task(t);
    }
    rq.schedule(false);
    // Alguns ticks pra espalhar os vruntimes.
    for k in 1..=7 {
        clock.set(k * 4 * MS);
        rq.tick();
        if rq.need_resched() {
            rq.schedule(false);
        }
    }
    let curr = rq.current().expect("alguém roda");
    let x = *ids.iter().find(|&&t| t != curr).expect("outra tarefa");
    let before = rq.task(x);
    let v_before = rq.avg_vruntime_peek();
    let w_before = i128::from(scale_load_down(before.weight));
    let lag_before = w_before * i128::from(v_before.wrapping_sub(before.vruntime) as i64);

    rq.set_user_nice(x, 5);
    ok(&rq);
    let after = rq.task(x);
    let v_after = rq.avg_vruntime_peek();
    let w_after = i128::from(scale_load_down(after.weight));
    let lag_after = w_after * i128::from(v_after.wrapping_sub(after.vruntime) as i64);
    assert_eq!(after.weight, 335 << 10);
    // Duas divisões truncadas (reweight e place) e o piso de V: erro de poucas unidades de peso.
    let tol = 4 * (w_before + w_after);
    assert!((lag_before - lag_after).abs() <= tol, "lag ponderado {lag_before} virou {lag_after}");
    assert_eq!(
        after.deadline.wrapping_sub(after.vruntime),
        before.deadline.wrapping_sub(before.vruntime),
        "deadline relativa preservada"
    );
}

/// Renice do corrente pra nice maior pede reescalonamento (`prio_changed_fair`); pra nice menor, não.
#[test]
fn renice_of_current() {
    let (clock, mut rq) = rq16();
    let a = rq.create_task(0);
    let b = rq.create_task(0);
    rq.wake_up_new_task(a);
    rq.wake_up_new_task(b);
    assert_eq!(rq.schedule(false), Some(a));
    clock.set(MS);
    rq.set_user_nice(a, 3);
    assert!(rq.need_resched());
    assert_eq!(rq.current(), Some(a));
    rq.schedule(false);
    ok(&rq);
    let c = rq.current().expect("alguém roda");
    let nice = rq.task(c).nice;
    rq.set_user_nice(c, nice - 1);
    assert!(!rq.need_resched());
    ok(&rq);
}

/// `yield_task_fair`: corrente elegível pula pra própria deadline e ganha outra; não elegível, nada
/// muda além do reescalonamento.
///
/// A roda 1 ms; B nasce em 1 ms com v = V = V0 + 1 ms. A tem v = V, é elegível, e cede: v vai pra
/// deadline (V0 + 1,4 ms), que é renovada pra V0 + 4,2 ms. V = 1,2 ms, A deixa de ser elegível e B roda.
/// Depois, B roda 0,1 ms e cede: v = V0 + 1,1 ms e V = (1,4 + 1,1) / 2 = 1,25 ms, então B é elegível e
/// também pula pra deadline dela; A volta a rodar.
#[test]
fn yield_moves_vruntime_to_deadline() {
    let (clock, mut rq) = rq16();
    let a = rq.create_task(0);
    rq.wake_up_new_task(a);
    rq.schedule(false);
    clock.set(MS);
    let b = rq.create_task(0);
    rq.wake_up_new_task(b);
    assert_eq!(rq.yield_current(), Some(b));
    let ia = rq.task(a);
    assert_eq!(ia.vruntime, V0.wrapping_add(1_400_000));
    assert_eq!(ia.deadline, V0.wrapping_add(4_200_000));
    ok(&rq);

    clock.set(MS + 100_000);
    let before = rq.task(b);
    assert_eq!(rq.yield_current(), Some(a));
    let ib = rq.task(b);
    assert_eq!(ib.vruntime, before.deadline);
    assert_eq!(ib.deadline, before.deadline.wrapping_add(2_800_000));
    ok(&rq);

    // Sozinha na fila, yield não mexe em nada.
    let (_clock, mut solo) = rq16();
    let s = solo.create_task(0);
    solo.wake_up_new_task(s);
    solo.schedule(false);
    let before = solo.task(s);
    assert_eq!(solo.yield_current(), Some(s));
    assert_eq!(solo.task(s).vruntime, before.vruntime);
}

/// Saída libera a posição e passa a CPU; a CPU ociosa volta a pedir reescalonamento quando alguém nasce.
#[test]
fn exit_and_idle() {
    let (clock, mut rq) = rq16();
    let a = rq.create_task(0);
    rq.wake_up_new_task(a);
    rq.schedule(false);
    clock.set(MS);
    assert_eq!(rq.exit_current(), None);
    assert!(!rq.contains(a));
    assert_eq!(rq.nr_running(), 0);
    ok(&rq);
    let b = rq.create_task(0);
    assert_eq!(b.index(), a.index(), "posição reaproveitada");
    rq.wake_up_new_task(b);
    assert!(rq.need_resched());
    assert_eq!(rq.schedule(false), Some(b));
    ok(&rq);
}

/// Tarefa já pronta não é "acordada" de novo.
#[test]
fn waking_a_runnable_task_is_a_no_op() {
    let (_clock, mut rq) = rq16();
    let a = rq.create_task(0);
    rq.wake_up_new_task(a);
    assert!(!rq.try_to_wake_up(a));
}

/// Gerador SplitMix64 pros testes aleatórios.
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

/// Sequências aleatórias de todas as operações públicas. Depois de cada uma: invariantes internas, o
/// `pick_eevdf` igual ao pick por força bruta, e Σ w (V - v) em (-W, 0].
#[test]
fn random_operations_keep_invariants() {
    for seed in 0..60u64 {
        let mut rng = SplitMix(seed);
        let features = if seed % 4 == 3 {
            Features { delay_dequeue: false, run_to_parity: seed % 8 == 3, ..Features::default() }
        } else {
            Features::default()
        };
        let (clock, mut rq) = rq_with(features);
        let mut now = 0u64;
        for _ in 0..1500 {
            now += rng.below(3 * MS);
            clock.set(now);
            match rng.below(16) {
                0 | 1 => {
                    if rq.task_ids().count() < 40 {
                        let t = rq.create_task(rng.below(40) as i32 - 20);
                        if rng.below(4) == 0 {
                            rq.set_custom_slice(t, Some(rng.below(5 * MS)));
                        }
                        rq.wake_up_new_task(t);
                    }
                }
                2..=5 => rq.tick(),
                6 | 7 if rq.current().is_some() => {
                    rq.schedule(true);
                }
                8..=10 => {
                    let sleeping: Vec<TaskId> = rq.task_ids().filter(|&t| rq.task(t).sleeping).collect();
                    if !sleeping.is_empty() {
                        let t = sleeping[rng.below(sleeping.len() as u64) as usize];
                        rq.try_to_wake_up(t);
                    }
                }
                11 => {
                    let all: Vec<TaskId> = rq.task_ids().collect();
                    if !all.is_empty() {
                        let t = all[rng.below(all.len() as u64) as usize];
                        rq.set_user_nice(t, rng.below(40) as i32 - 20);
                    }
                }
                12 => {
                    let all: Vec<TaskId> = rq.task_ids().collect();
                    if !all.is_empty() {
                        let t = all[rng.below(all.len() as u64) as usize];
                        let s = if rng.below(2) == 0 { None } else { Some(rng.below(3 * MS)) };
                        rq.set_custom_slice(t, s);
                    }
                }
                13 if rq.current().is_some() => {
                    rq.yield_current();
                }
                14 if rq.current().is_some() && rng.below(3) == 0 => {
                    rq.exit_current();
                }
                _ => {}
            }
            if rq.need_resched() {
                rq.schedule(false);
            }
            ok(&rq);
            assert_eq!(rq.peek_pick(), rq.peek_pick_linear(), "pick aumentado diverge do linear");
            let (sum, w) = weighted_lag_sum(&rq);
            assert!(sum <= 0 && (w == 0 || sum > -w), "Σ w (V - v) = {sum} fora de (-{w}, 0]");
        }
    }
}
