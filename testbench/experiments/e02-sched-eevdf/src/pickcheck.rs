//! `pick_eevdf` contra força bruta com aritmética exata (H12).
//!
//! Cada caso monta uma [`Timeline`] com até 300 entidades de pesos variados, vruntimes espalhados em
//! volta de uma base aleatória (inclusive perto da volta do u64), deadlines em três formas (ver
//! `entity_strategy`, com empates), remoções no meio, e às vezes um corrente fora da árvore (protegido
//! ou não). A escolha por
//! força bruta não usa nada da árvore nem das somas incrementais: recalcula a elegibilidade em `i128`
//! (`(v_i - b) * W <= Σ (v_j - b) * w_j`) e pega a menor deadline entre os elegíveis, com empate pra
//! quem foi inserido antes, aplicando as mesmas regras de corrente e RUN_TO_PARITY do kernel.

use std::time::Instant;

use proptest::prelude::*;
use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestError, TestRng, TestRunner};
use rbtree::NodeId;
use sched::EntityId;
use sched::timeline::{CurrView, QueuedEntity, Timeline, deadline_before};
use sched::weight::{LoadWeight, calc_delta_fair, scale_load_down};
use serde::Serialize;

/// Entidade gerada: nice, deslocamento do vruntime e da deadline em relação à base.
#[derive(Clone, Copy, Debug)]
pub struct GenEntity {
    pub nice: i32,
    pub v_off: i64,
    pub d_off: i64,
}

/// Um caso gerado.
#[derive(Clone, Debug)]
pub struct PickCase {
    pub base: u64,
    pub entities: Vec<GenEntity>,
    /// Índices (módulo o tamanho atual) removidos depois de inserir tudo.
    pub removals: Vec<usize>,
    pub curr: Option<(GenEntity, bool)>,
    pub run_to_parity: bool,
}

/// Três formas de entidade, misturadas no mesmo caso:
/// - deadline logo depois do vruntime (0 a 6 ms, ou 0 a 4 ns pra forçar empates);
/// - deadline independente do vruntime (o mais à esquerda costuma não ser elegível);
/// - forma do EEVDF: vruntime dentro do limite de lag do peso e deadline dentro da fatia virtual.
fn entity_strategy() -> impl Strategy<Value = GenEntity> {
    let near = (-20i32..=19, -50_000_000i64..=50_000_000, prop_oneof![3 => 0i64..=6_000_000, 1 => 0i64..=4])
        .prop_map(|(nice, v_off, d)| GenEntity { nice, v_off, d_off: v_off + d });
    let independent = (-20i32..=19, -50_000_000i64..=50_000_000, -60_000_000i64..=60_000_000)
        .prop_map(|(nice, v_off, d_off)| GenEntity { nice, v_off, d_off });
    let eevdf = (-20i32..=19, -1.0f64..=1.0, 0.001f64..=1.0).prop_map(|(nice, lag_frac, slice_frac)| {
        let lw = LoadWeight::from_nice(nice);
        let lag = calc_delta_fair(5_600_000, &lw) as f64;
        let vslice = calc_delta_fair(2_800_000, &lw) as f64;
        let v_off = (lag * lag_frac) as i64;
        GenEntity { nice, v_off, d_off: v_off + (vslice * slice_frac) as i64 }
    });
    prop_oneof![near, independent, eevdf]
}

/// Estratégia dos casos.
pub fn case_strategy() -> impl Strategy<Value = PickCase> {
    (
        prop_oneof![any::<u64>(), Just(u64::MAX - 20_000_000), Just(0u64)],
        proptest::collection::vec(entity_strategy(), 1..300),
        proptest::collection::vec(any::<usize>(), 0..40),
        proptest::option::of((entity_strategy(), any::<bool>())),
        any::<bool>(),
    )
        .prop_map(|(base, entities, removals, curr, run_to_parity)| PickCase { base, entities, removals, curr, run_to_parity })
}

/// Monta a linha do tempo e devolve (escolha do `pick_eevdf`, escolha da força bruta, n).
pub fn evaluate(case: &PickCase) -> (Option<EntityId>, Option<EntityId>, usize) {
    let mut tl = Timeline::with_zero_vruntime(case.base);
    // (task, vruntime, deadline, peso escalado, node), na ordem de inserção.
    let mut live: Vec<(EntityId, u64, u64, u64, NodeId)> = Vec::new();
    for (i, g) in case.entities.iter().enumerate() {
        let task = EntityId::from_index(i as u32);
        let v = case.base.wrapping_add(g.v_off as u64);
        let d = case.base.wrapping_add(g.d_off as u64);
        let w = LoadWeight::from_nice(g.nice).weight;
        let node = tl.enqueue(QueuedEntity { entity: task, vruntime: v, slice: 2_800_000, weight: w }, d);
        live.push((task, v, d, w, node));
    }
    for r in &case.removals {
        if live.len() <= 1 {
            break;
        }
        let (_, _, _, _, node) = live.remove(r % live.len());
        tl.dequeue(node);
    }
    let curr = case.curr.map(|(g, protected)| {
        let v = case.base.wrapping_add(g.v_off as u64);
        let d = case.base.wrapping_add(g.d_off as u64);
        CurrView {
            entity: EntityId::from_index(1_000_000),
            vruntime: v,
            deadline: d,
            weight: LoadWeight::from_nice(g.nice).weight,
            slice: 2_800_000,
            protected,
        }
    });
    let nr_running = (live.len() + usize::from(curr.is_some())) as u32;
    let fast = tl.pick_eevdf(nr_running, curr.as_slice(), curr.as_ref(), &[], case.run_to_parity);
    let brute = brute_force(case.base, &live, curr.as_ref(), case.run_to_parity);
    (fast, brute, live.len())
}

fn brute_force(base: u64, live: &[(EntityId, u64, u64, u64, NodeId)], curr: Option<&CurrView>, rtp: bool) -> Option<EntityId> {
    if live.len() + usize::from(curr.is_some()) == 1 {
        // nr_running == 1: o corrente se existir, senão o único da árvore, sem testar elegibilidade.
        return curr.map(|c| c.entity).or_else(|| live.first().map(|e| e.0));
    }
    let key = |v: u64| i128::from(v.wrapping_sub(base) as i64);
    let mut total_w = 0i128;
    let mut weighted = 0i128;
    for e in live {
        let w = i128::from(scale_load_down(e.3));
        total_w += w;
        weighted += key(e.1) * w;
    }
    if let Some(c) = curr {
        let w = i128::from(scale_load_down(c.weight));
        total_w += w;
        weighted += key(c.vruntime) * w;
    }
    let eligible = |v: u64| key(v) * total_w <= weighted;
    let curr_e = curr.filter(|c| eligible(c.vruntime));
    if rtp && let Some(c) = curr_e && c.protected {
        return Some(c.entity);
    }
    let mut best: Option<&(EntityId, u64, u64, u64, NodeId)> = None;
    for e in live {
        if eligible(e.1) && best.is_none_or(|b| deadline_before(e.2, b.2)) {
            best = Some(e);
        }
    }
    match (best, curr_e) {
        (None, c) => c.map(|c| c.entity),
        (Some(b), Some(c)) if deadline_before(c.deadline, b.2) => Some(c.entity),
        (Some(b), _) => Some(b.0),
    }
}

/// Resultado do teste.
#[derive(Clone, Debug, Serialize)]
pub struct PickCheckReport {
    pub cases: u32,
    pub passed: bool,
    pub failure: Option<String>,
    pub elapsed_s: f64,
}

/// Roda `cases` casos com semente fixa.
pub fn run(cases: u32) -> PickCheckReport {
    let config = Config { cases, failure_persistence: None, ..Config::default() };
    let rng = TestRng::deterministic_rng(RngAlgorithm::ChaCha);
    let mut runner = TestRunner::new_with_rng(config, rng);
    let t0 = Instant::now();
    let result = runner.run(&case_strategy(), |case| {
        let (fast, brute, n) = evaluate(&case);
        if fast == brute {
            Ok(())
        } else {
            Err(TestCaseError::fail(format!("n = {n}: pick_eevdf {fast:?}, força bruta {brute:?}")))
        }
    });
    let failure = match result {
        Ok(()) => None,
        Err(TestError::Fail(reason, input)) => Some(format!("{reason}; entrada mínima: {input:?}")),
        Err(TestError::Abort(reason)) => Some(format!("abortado: {reason}")),
    };
    PickCheckReport { cases, passed: failure.is_none(), failure, elapsed_s: t0.elapsed().as_secs_f64() }
}
