//! `pick_eevdf` aumentado contra varredura linear (H12).
//!
//! Duas famílias de estados pra cada n:
//!
//! - **simulados**: n tarefas CPU-bound com nice entre -5 e 5 rodando no simulador do crate `sched`
//!   (fatia 2,8 ms, tick 4 ms) até misturar; os estados são retratos da runqueue em instantes
//!   aleatórios, com o corrente sem proteção de fatia (é o pick do tick que preempta). Aqui o mais à
//!   esquerda costuma ser elegível e o `pick_eevdf` sai pelo atalho O(1).
//! - **nices misturados**: estados sorteados com nices de -20 a 19 na forma que o EEVDF produz (ver
//!   [`mixed_nice_states`]). Na prática o mais à esquerda também é elegível: as tarefas leves com lag
//!   positivo ficam na ponta esquerda tanto em vruntime quanto em deadline.
//! - **independentes**: deadline sorteada sem relação com o vruntime (ver [`independent_states`]), o pior
//!   caso, em que o pick desce a árvore guiado pelo `min_vruntime` em cerca de metade dos estados.
//!
//! A linha de base é a runqueue ingênua: um vetor contíguo com as mesmas somas incrementais de V
//! (então a elegibilidade custa o mesmo) e uma varredura que pega a menor deadline entre os elegíveis.
//! As duas escolhas são conferidas uma contra a outra em todo estado.

use std::hint::black_box;
use std::time::Instant;

use sched::timeline::{CurrView, QueuedEntity, Timeline, deadline_before};
use sched::weight::{LoadWeight, calc_delta_fair, scale_load_down};
use sched::{EntityId, Features, SimConfig, SimTask, Simulator, Tunables};
use serde::Serialize;

use crate::rng::SplitMix;

/// Um estado: a linha do tempo, o corrente e o vetor da linha de base.
#[derive(Clone, Debug)]
pub struct PickState {
    pub timeline: Timeline,
    pub curr: Option<CurrView>,
    pub nr_running: u32,
    pub linear: LinearRq,
}

/// Runqueue ingênua: vetor de (vruntime, deadline, tarefa) e as somas de V.
#[derive(Clone, Debug)]
pub struct LinearRq {
    zero_vruntime: u64,
    avg: i64,
    load: i64,
    ents: Vec<(u64, u64, EntityId)>,
}

impl LinearRq {
    fn from_timeline(tl: &Timeline) -> LinearRq {
        LinearRq {
            zero_vruntime: tl.zero_vruntime(),
            avg: tl.avg_vruntime_sum(),
            load: tl.avg_load() as i64,
            ents: tl.tree().iter().map(|(_, d, e)| (e.vruntime, d.0, e.entity)).collect(),
        }
    }

    /// Mesma regra do `pick_eevdf`, por varredura O(n).
    pub fn pick(&self, nr_running: u32, curr: Option<&CurrView>, run_to_parity: bool) -> Option<EntityId> {
        if nr_running == 1 {
            return curr.map(|c| c.entity).or_else(|| self.ents.first().map(|e| e.2));
        }
        let key = |v: u64| v.wrapping_sub(self.zero_vruntime) as i64;
        let (mut avg, mut load) = (self.avg, self.load);
        if let Some(c) = curr {
            let w = scale_load_down(c.weight) as i64;
            avg = avg.wrapping_add(key(c.vruntime).wrapping_mul(w));
            load += w;
        }
        let eligible = |v: u64| avg >= key(v).wrapping_mul(load);
        let curr_e = curr.filter(|c| eligible(c.vruntime));
        if run_to_parity && let Some(c) = curr_e && c.protected {
            return Some(c.entity);
        }
        let mut best: Option<(u64, EntityId)> = None;
        for &(v, d, t) in &self.ents {
            if eligible(v) && best.is_none_or(|b| deadline_before(d, b.0)) {
                best = Some((d, t));
            }
        }
        match (best, curr_e) {
            (None, c) => c.map(|c| c.entity),
            (Some((bd, _)), Some(c)) if deadline_before(c.deadline, bd) => Some(c.entity),
            (Some((_, t)), _) => Some(t),
        }
    }
}

impl PickState {
    /// `pick_eevdf` da linha do tempo, com o corrente do estado.
    pub fn pick_eevdf(&self) -> Option<EntityId> {
        self.timeline.pick_eevdf(self.nr_running, self.curr.as_slice(), self.curr.as_ref(), &[], true)
    }

    /// Força bruta da linha do tempo (varredura em ordem).
    pub fn pick_tree_linear(&self) -> Option<EntityId> {
        self.timeline.pick_linear(self.nr_running, self.curr.as_slice(), self.curr.as_ref(), &[], true)
    }
}

/// Estados simulados com n tarefas.
pub fn simulated_states(n: usize, count: usize, seed: u64) -> Vec<PickState> {
    let mut rng = SplitMix::new(seed);
    let tasks: Vec<SimTask> = (0..n).map(|i| SimTask::cpu_bound(&format!("t{i}"), rng.range_i64(-5, 5) as i32)).collect();
    let slice = 2_800_000u64;
    let warmup = (n as u64 * slice * 2).max(1_000_000_000);
    let cfg = SimConfig::single_cpu(Tunables::linux_6_12_101(16, 250), Features::default(), u64::MAX / 4, rng.below(4_000_000), 0);
    let mut sim = Simulator::new(cfg, &[], &tasks);
    while sim.now() < warmup {
        sim.step();
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        for _ in 0..1 + rng.below(40) {
            sim.step();
        }
        let s = sim.sched();
        let timeline = s.root_timeline(0).clone();
        let curr = s.curr_view(0).map(|c| CurrView { protected: false, ..c });
        let linear = LinearRq::from_timeline(&timeline);
        out.push(PickState { timeline, curr, nr_running: s.root_nr_running(0), linear });
    }
    out
}

/// Estados sorteados com n entidades na árvore e nices de -20 a 19, na forma que o EEVDF produz:
/// vruntime dentro do limite de lag da entidade (`calc_delta_fair(2 * fatia)` em volta da base) e
/// deadline em algum ponto da fatia virtual dela (`v + calc_delta_fair(fatia) * u`, u em (0, 1]).
pub fn mixed_nice_states(n: usize, count: usize, seed: u64) -> Vec<PickState> {
    let mut rng = SplitMix::new(seed);
    (0..count)
        .map(|_| {
            let base = rng.next_u64();
            let mut tl = Timeline::with_zero_vruntime(base);
            for i in 0..n {
                let nice = rng.range_i64(-20, 19) as i32;
                let lw = LoadWeight::from_nice(nice);
                let lag = calc_delta_fair(5_600_000, &lw) as i64;
                let vslice = calc_delta_fair(2_800_000, &lw);
                let v = base.wrapping_add(rng.range_i64(-lag, lag) as u64);
                let d = v.wrapping_add(vslice * (1 + rng.below(1000)) / 1000);
                tl.enqueue(QueuedEntity { entity: EntityId::from_index(i as u32), vruntime: v, slice: 2_800_000, weight: lw.weight }, d);
            }
            let linear = LinearRq::from_timeline(&tl);
            PickState { timeline: tl, curr: None, nr_running: n as u32, linear }
        })
        .collect()
}

/// Estados adversários: deadline sorteada independente do vruntime (±60 ms contra ±50 ms), então o mais
/// à esquerda é elegível só em cerca de metade dos estados e, quando não é, o pick precisa descer a
/// árvore guiado pelo `min_vruntime`. É o pior caso do `pick_eevdf`.
pub fn independent_states(n: usize, count: usize, seed: u64) -> Vec<PickState> {
    let mut rng = SplitMix::new(seed);
    (0..count)
        .map(|_| {
            let base = rng.next_u64();
            let mut tl = Timeline::with_zero_vruntime(base);
            for i in 0..n {
                let lw = LoadWeight::from_nice(rng.range_i64(-20, 19) as i32);
                let v = base.wrapping_add(rng.range_i64(-50_000_000, 50_000_000) as u64);
                let d = base.wrapping_add(rng.range_i64(-60_000_000, 60_000_000) as u64);
                tl.enqueue(QueuedEntity { entity: EntityId::from_index(i as u32), vruntime: v, slice: 2_800_000, weight: lw.weight }, d);
            }
            let linear = LinearRq::from_timeline(&tl);
            PickState { timeline: tl, curr: None, nr_running: n as u32, linear }
        })
        .collect()
}

/// Resultado de uma família num tamanho.
#[derive(Clone, Debug, Serialize)]
pub struct PickBench {
    pub n: usize,
    pub family: String,
    pub states: usize,
    pub eevdf_ns: f64,
    pub linear_ns: f64,
    /// `linear_ns / eevdf_ns` (acima de 1, o aumentado é mais rápido).
    pub speedup: f64,
    /// Fração dos estados em que o mais à esquerda já é elegível (atalho O(1)).
    pub leftmost_eligible: f64,
    /// As duas escolhas bateram em todos os estados.
    pub agree: bool,
}

/// Mede uma família de estados.
pub fn bench(n: usize, family: &str, states: &[PickState]) -> PickBench {
    let agree = states.iter().all(|s| s.pick_eevdf() == s.linear.pick(s.nr_running, s.curr.as_ref(), true));
    let leftmost_eligible = states
        .iter()
        .filter(|s| {
            s.timeline
                .tree()
                .first()
                .is_some_and(|f| s.timeline.vruntime_eligible(s.curr.as_slice(), s.timeline.entity(f).vruntime))
        })
        .count() as f64
        / states.len() as f64;
    let per_state = ((20_000_000 / n.max(1)) / states.len()).clamp(20, 20_000);
    let picks = (per_state * states.len()) as f64;

    let mut eevdf = Vec::new();
    let mut linear = Vec::new();
    for _ in 0..5 {
        let t0 = Instant::now();
        for s in states {
            for _ in 0..per_state {
                black_box(black_box(s).pick_eevdf());
            }
        }
        eevdf.push(t0.elapsed().as_nanos() as f64 / picks);
        let t0 = Instant::now();
        for s in states {
            for _ in 0..per_state {
                black_box(black_box(&s.linear).pick(s.nr_running, s.curr.as_ref(), true));
            }
        }
        linear.push(t0.elapsed().as_nanos() as f64 / picks);
    }
    // Mínimo de repetições intercaladas: a menos perturbada por outros processos.
    let eevdf_ns = eevdf.iter().copied().fold(f64::INFINITY, f64::min);
    let linear_ns = linear.iter().copied().fold(f64::INFINITY, f64::min);
    PickBench {
        n,
        family: family.to_string(),
        states: states.len(),
        eevdf_ns,
        linear_ns,
        speedup: linear_ns / eevdf_ns,
        leftmost_eligible,
        agree,
    }
}
