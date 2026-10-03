//! Controle de banda do CFS (`CONFIG_CFS_BANDWIDTH`, o `cpu.max` do cgroup v2), como no fair.c da
//! 6.12.101.
//!
//! Cada grupo com quota tem um pool (`cfs_bandwidth`) reabastecido com `quota` a cada `period` por um
//! timer. Cada fila do grupo (uma por CPU) pega runtime do pool em fatias de
//! `sched_cfs_bandwidth_slice` (5 ms) e vai gastando no `update_curr`. Quando a fila fica sem runtime e
//! o pool está vazio, ela pede reescalonamento; no `put_prev_entity` (ou no pick, ou ao enfileirar) a
//! fila é estrangulada: a entidade do grupo sai da fila do pai, as tarefas continuam enfileiradas na
//! fila estrangulada e o relógio do PELT dela para. No próximo período o timer reabastece o pool e
//! distribui runtime pras filas estranguladas, na ordem em que foram estranguladas, que voltam pra
//! hierarquia (`unthrottle_cfs_rq`). Como o tick é o único ponto em que uma tarefa CPU-bound é cobrada,
//! o grupo pode passar da quota em até um tick; essa dívida (runtime negativo) é paga no período
//! seguinte.
//!
//! O timer de período começa numa fase escolhida ([`crate::Sched::set_group_period_offset`]; o kernel
//! sorteia) e fica parado enquanto o grupo não consome nada (`cfs_b->idle`). Os timers são dirigidos
//! de fora: [`crate::Sched::next_timer_ns`] diz quando vence o próximo e [`crate::Sched::run_timers`]
//! roda os vencidos.

use std::fmt;

use crate::clock::Clock;
use crate::sched::{CfsId, DEQUEUE_DELAYED, DEQUEUE_SLEEP, DEQUEUE_SPECIAL, ENQUEUE_WAKEUP, Sched, UPDATE_TG};
use crate::{EntityId, GroupId};

/// Menor quota e menor período aceitos (`min_cfs_quota_period`): 1 ms.
pub const MIN_CFS_QUOTA_PERIOD_NS: u64 = 1_000_000;

/// Maior período aceito (`max_cfs_quota_period`): 1 s.
pub const MAX_CFS_QUOTA_PERIOD_NS: u64 = 1_000_000_000;

/// Período padrão (`default_cfs_period`): 100 ms.
pub const DEFAULT_CFS_PERIOD_NS: u64 = 100_000_000;

/// Runtime que uma fila guarda ao devolver sobra (`min_cfs_rq_runtime`): 1 ms.
const MIN_CFS_RQ_RUNTIME_NS: i64 = 1_000_000;

/// Distância mínima até o fim do período pra redistribuir sobra (`min_bandwidth_expiration`): 2 ms.
const MIN_BANDWIDTH_EXPIRATION_NS: u64 = 2_000_000;

/// Espera pra juntar sobra antes de distribuir (`cfs_bandwidth_slack_period`): 5 ms.
const CFS_BANDWIDTH_SLACK_PERIOD_NS: u64 = 5_000_000;

/// Quota ou período fora dos limites do kernel (`-EINVAL`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BandwidthError {
    QuotaTooSmall,
    PeriodOutOfRange,
    RootGroup,
}

impl fmt::Display for BandwidthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BandwidthError::QuotaTooSmall => write!(f, "quota menor que 1 ms"),
            BandwidthError::PeriodOutOfRange => write!(f, "período fora de [1 ms, 1 s]"),
            BandwidthError::RootGroup => write!(f, "o grupo raiz não tem limite de banda"),
        }
    }
}

impl std::error::Error for BandwidthError {}

/// `struct cfs_bandwidth`.
#[derive(Clone, Debug)]
pub(crate) struct CfsBandwidth {
    pub period: u64,
    /// `None` é `RUNTIME_INF`.
    pub quota: Option<u64>,
    pub burst: u64,
    /// Runtime no pool.
    pub runtime: u64,
    pub runtime_snap: u64,
    pub hierarchical_quota: Option<u64>,
    pub idle: bool,
    pub period_active: bool,
    /// Próximo vencimento do timer de período (relógio injetado).
    pub timer_expires: u64,
    pub slack_started: bool,
    pub slack_expires: Option<u64>,
    /// `throttled_cfs_rq`, na ordem de estrangulamento.
    pub throttled: Vec<CfsId>,
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub nr_burst: u64,
    pub throttled_time: u64,
    pub burst_time: u64,
}

impl CfsBandwidth {
    /// `init_cfs_bandwidth`: sem quota, período de 100 ms, timer na fase 0.
    pub(crate) fn new(_now: u64) -> CfsBandwidth {
        CfsBandwidth {
            period: DEFAULT_CFS_PERIOD_NS,
            quota: None,
            burst: 0,
            runtime: 0,
            runtime_snap: 0,
            hierarchical_quota: None,
            idle: false,
            period_active: false,
            timer_expires: 0,
            slack_started: false,
            slack_expires: None,
            throttled: Vec::new(),
            nr_periods: 0,
            nr_throttled: 0,
            nr_burst: 0,
            throttled_time: 0,
            burst_time: 0,
        }
    }
}

/// `hrtimer_forward`: avança o vencimento em múltiplos de `interval` até passar de `now`. Devolve
/// quantos intervalos andou (0 se ainda não venceu).
fn hrtimer_forward(expires: &mut u64, now: u64, interval: u64) -> u64 {
    let delta = now as i64 - *expires as i64;
    if delta < 0 {
        return 0;
    }
    let mut orun = 1u64;
    let delta = delta as u64;
    if delta >= interval {
        orun = delta / interval;
        *expires += interval * orun;
        if *expires > now {
            return orun;
        }
        orun += 1;
    }
    *expires += interval;
    orun
}

impl<C: Clock> Sched<C> {
    /// `cfs_rq_throttled`.
    pub(crate) fn cfs_rq_throttled(&self, cfs: CfsId) -> bool {
        self.c(cfs).throttled
    }

    /// `throttled_hierarchy`: a fila ou algum ancestral está estrangulado.
    pub(crate) fn throttled_hierarchy(&self, cfs: CfsId) -> bool {
        self.c(cfs).throttle_count > 0
    }

    /// `tg_set_cfs_bandwidth`.
    pub(crate) fn tg_set_cfs_bandwidth(&mut self, g: GroupId, quota: Option<u64>, period: u64, burst: u64) -> Result<(), BandwidthError> {
        if g == GroupId::ROOT {
            return Err(BandwidthError::RootGroup);
        }
        if quota.is_some_and(|q| q < MIN_CFS_QUOTA_PERIOD_NS) {
            return Err(BandwidthError::QuotaTooSmall);
        }
        if !(MIN_CFS_QUOTA_PERIOD_NS..=MAX_CFS_QUOTA_PERIOD_NS).contains(&period) {
            return Err(BandwidthError::PeriodOutOfRange);
        }
        // tg_cfs_schedulable_down no cgroup v2: a quota hierárquica é a menor do caminho.
        let parent_hq = self.g(g).parent.and_then(|p| self.g(p).bw.hierarchical_quota);
        let hq = match (quota, parent_hq) {
            (None, p) => p,
            (Some(q), None) => Some(q),
            (Some(q), Some(p)) => Some(q.min(p)),
        };
        let enabled = quota.is_some();
        {
            let bw = &mut self.g_mut(g).bw;
            bw.period = period;
            bw.quota = quota;
            bw.burst = burst;
            bw.hierarchical_quota = hq;
        }
        self.refill_cfs_bandwidth_runtime(g);
        if enabled {
            self.start_cfs_bandwidth(g);
        }
        let cfs_ids = self.g(g).cfs_rq.clone();
        for cfs in cfs_ids {
            let c = self.c_mut(cfs);
            c.runtime_enabled = enabled;
            c.runtime_remaining = 0;
            if c.throttled {
                self.unthrottle_cfs_rq(cfs);
            }
        }
        Ok(())
    }

    /// `__refill_cfs_bandwidth_runtime`.
    fn refill_cfs_bandwidth_runtime(&mut self, g: GroupId) {
        let bw = &mut self.g_mut(g).bw;
        let Some(quota) = bw.quota else { return };
        bw.runtime += quota;
        let runtime = bw.runtime_snap as i64 - bw.runtime as i64;
        if runtime > 0 {
            bw.burst_time += runtime as u64;
            bw.nr_burst += 1;
        }
        bw.runtime = bw.runtime.min(quota + bw.burst);
        bw.runtime_snap = bw.runtime;
    }

    /// `start_cfs_bandwidth`: liga o timer de período, no próximo ponto da grade.
    pub(crate) fn start_cfs_bandwidth(&mut self, g: GroupId) {
        let now = self.now;
        let bw = &mut self.g_mut(g).bw;
        if bw.period_active {
            return;
        }
        bw.period_active = true;
        let period = bw.period;
        hrtimer_forward(&mut bw.timer_expires, now, period);
    }

    /// `__assign_cfs_rq_runtime`: tira do pool até completar `target` na fila. Devolve se a fila ficou
    /// com runtime positivo.
    fn assign_cfs_rq_runtime_target(&mut self, cfs: CfsId, target: u64) -> bool {
        let g = self.c(cfs).tg;
        let min_amount = (target as i64 - self.c(cfs).runtime_remaining) as u64;
        let mut amount = 0u64;
        if self.g(g).bw.quota.is_none() {
            amount = min_amount;
        } else {
            self.start_cfs_bandwidth(g);
            let bw = &mut self.g_mut(g).bw;
            if bw.runtime > 0 {
                amount = bw.runtime.min(min_amount);
                bw.runtime -= amount;
                bw.idle = false;
            }
        }
        let c = self.c_mut(cfs);
        c.runtime_remaining += amount as i64;
        c.runtime_remaining > 0
    }

    /// `__account_cfs_rq_runtime`: desconta o tempo; se acabou e não dá pra pegar mais, pede
    /// reescalonamento de quem roda na fila (o estrangulamento acontece no `put_prev_entity`).
    fn account_cfs_rq_runtime_inner(&mut self, cfs: CfsId, delta_exec: u64) {
        self.c_mut(cfs).runtime_remaining -= delta_exec as i64;
        if self.c(cfs).runtime_remaining > 0 {
            return;
        }
        if self.c(cfs).throttled {
            return;
        }
        let slice = self.cfg.tunables.cfs_bandwidth_slice_ns;
        if !self.assign_cfs_rq_runtime_target(cfs, slice) && self.has_curr(cfs) {
            let cpus: Vec<usize> = self.c(cfs).curr.iter().map(|s| s.cpu).collect();
            for cpu in cpus {
                self.resched_cpu(cpu);
            }
        }
    }

    /// `account_cfs_rq_runtime`.
    pub(crate) fn account_cfs_rq_runtime(&mut self, cfs: CfsId, delta_exec: u64) {
        if !self.c(cfs).runtime_enabled {
            return;
        }
        self.account_cfs_rq_runtime_inner(cfs, delta_exec);
    }

    /// Grupos da subárvore de `g`, em pré-ordem (`walk_tg_tree_from`).
    fn subtree(&self, g: GroupId) -> Vec<GroupId> {
        let mut out = vec![g];
        let mut i = 0;
        while i < out.len() {
            let children = self.g(out[i]).children.clone();
            out.extend(children);
            i += 1;
        }
        out
    }

    /// `tg_throttle_down` em toda a subárvore: conta o estrangulamento e para o relógio do PELT.
    fn throttle_down_subtree(&mut self, g: GroupId, slot: usize) {
        let pelt = self.sched_clock();
        for x in self.subtree(g) {
            let cfs = self.g(x).cfs_rq[slot];
            let c = self.c_mut(cfs);
            if c.throttle_count == 0 {
                c.throttled_clock_pelt = pelt;
            }
            c.throttle_count += 1;
        }
    }

    /// `tg_unthrottle_up` em toda a subárvore: desconta e retoma o relógio do PELT.
    fn unthrottle_up_subtree(&mut self, g: GroupId, slot: usize) {
        let pelt = self.sched_clock();
        for x in self.subtree(g) {
            let cfs = self.g(x).cfs_rq[slot];
            let c = self.c_mut(cfs);
            c.throttle_count -= 1;
            if c.throttle_count == 0 {
                c.throttled_clock_pelt_time += pelt - c.throttled_clock_pelt;
            }
        }
    }

    /// Posição da fila nos vetores por CPU do grupo dela.
    fn cfs_slot(&self, cfs: CfsId) -> usize {
        self.c(cfs).cpu.unwrap_or(0)
    }

    /// `throttle_cfs_rq`: tira a entidade do grupo (e os ancestrais que esvaziarem) da hierarquia. As
    /// tarefas ficam na fila estrangulada. Devolve `false` se, na última hora, apareceu runtime.
    pub(crate) fn throttle_cfs_rq(&mut self, cfs: CfsId) -> bool {
        let g = self.c(cfs).tg;
        if self.assign_cfs_rq_runtime_target(cfs, 1) {
            return false;
        }
        self.g_mut(g).bw.throttled.push(cfs);

        let slot = self.cfs_slot(cfs);
        self.throttle_down_subtree(g, slot);

        let queued_delta = self.c(cfs).h_nr_queued;
        let runnable_delta = self.c(cfs).h_nr_runnable;
        let delayed_delta = self.c(cfs).h_nr_delayed;
        let mut se: Option<EntityId> = self.c(cfs).my_se;
        let mut reached_root = true;
        let mut stopped = false;
        while let Some(s) = se {
            let q = self.cfs_rq_of(s);
            if !self.e(s).on_rq {
                stopped = true;
                break;
            }
            let mut flags = DEQUEUE_SLEEP | DEQUEUE_SPECIAL;
            if self.e(s).sched_delayed {
                flags |= DEQUEUE_DELAYED;
            }
            self.dequeue_entity(q, s, flags);
            let c = self.c_mut(q);
            c.h_nr_queued -= queued_delta;
            c.h_nr_runnable -= runnable_delta;
            c.h_nr_delayed -= delayed_delta;
            if self.c(q).load != 0 {
                se = self.parent_entity(s);
                break;
            }
            se = self.parent_entity(s);
        }
        if !stopped {
            while let Some(s) = se {
                let q = self.cfs_rq_of(s);
                if !self.e(s).on_rq {
                    stopped = true;
                    reached_root = false;
                    break;
                }
                self.update_load_avg(q, s, 0);
                let c = self.c_mut(q);
                c.h_nr_queued -= queued_delta;
                c.h_nr_runnable -= runnable_delta;
                c.h_nr_delayed -= delayed_delta;
                se = self.parent_entity(s);
            }
            if !stopped && reached_root {
                let rq = self.rq_slot_of_cfs(cfs);
                self.cpus[rq].nr_running -= queued_delta;
            }
        }
        let now = self.now;
        let c = self.c_mut(cfs);
        c.throttled = true;
        if c.nr_running != 0 {
            c.throttled_clock = now;
        }
        true
    }

    /// `unthrottle_cfs_rq`: devolve a fila à hierarquia, enfileirando de novo a entidade do grupo e os
    /// ancestrais que tinham saído.
    pub(crate) fn unthrottle_cfs_rq(&mut self, cfs: CfsId) {
        let g = self.c(cfs).tg;
        self.update_rq_clock();
        self.c_mut(cfs).throttled = false;
        let now = self.now;
        if self.c(cfs).throttled_clock != 0 {
            let dt = now - self.c(cfs).throttled_clock;
            self.g_mut(g).bw.throttled_time += dt;
            self.c_mut(cfs).throttled_clock = 0;
        }
        self.g_mut(g).bw.throttled.retain(|&x| x != cfs);
        let slot = self.cfs_slot(cfs);
        self.unthrottle_up_subtree(g, slot);

        if self.c(cfs).load != 0 {
            let queued_delta = self.c(cfs).h_nr_queued;
            let runnable_delta = self.c(cfs).h_nr_runnable;
            let delayed_delta = self.c(cfs).h_nr_delayed;
            let mut se = self.c(cfs).my_se;
            let mut stopped = false;
            while let Some(s) = se {
                let q = self.cfs_rq_of(s);
                if self.e(s).sched_delayed {
                    self.dequeue_entity(q, s, DEQUEUE_SLEEP | DEQUEUE_DELAYED);
                } else if self.e(s).on_rq {
                    break;
                }
                self.enqueue_entity(q, s, ENQUEUE_WAKEUP);
                let c = self.c_mut(q);
                c.h_nr_queued += queued_delta;
                c.h_nr_runnable += runnable_delta;
                c.h_nr_delayed += delayed_delta;
                if self.cfs_rq_throttled(q) {
                    stopped = true;
                    break;
                }
                se = self.parent_entity(s);
            }
            if !stopped {
                while let Some(s) = se {
                    let q = self.cfs_rq_of(s);
                    self.update_load_avg(q, s, UPDATE_TG);
                    let c = self.c_mut(q);
                    c.h_nr_queued += queued_delta;
                    c.h_nr_runnable += runnable_delta;
                    c.h_nr_delayed += delayed_delta;
                    if self.cfs_rq_throttled(q) {
                        stopped = true;
                        break;
                    }
                    se = self.parent_entity(s);
                }
                if !stopped {
                    let rq = self.rq_slot_of_cfs(cfs);
                    self.cpus[rq].nr_running += queued_delta;
                }
            }
        }
        // Acorda CPU ociosa que agora tem o que rodar.
        for cpu in 0..self.cfg.cpus {
            let root = self.cpus[cpu].root;
            if self.cpus[cpu].curr.is_none() && self.c(root).nr_running > 0 && (self.shared() || self.c(cfs).cpu == Some(cpu)) {
                self.resched_cpu(cpu);
            }
        }
    }

    /// `distribute_cfs_runtime`: dá runtime às filas estranguladas, na ordem, até o pool acabar.
    /// Devolve se ainda sobrou fila estrangulada.
    fn distribute_cfs_runtime(&mut self, g: GroupId) -> bool {
        let mut remaining = 1u64;
        let mut throttled = false;
        let mut to_unthrottle = Vec::new();
        let list = self.g(g).bw.throttled.clone();
        for cfs in list {
            if remaining == 0 {
                throttled = true;
                break;
            }
            if !self.cfs_rq_throttled(cfs) {
                continue;
            }
            let mut runtime = (-self.c(cfs).runtime_remaining + 1) as u64;
            let bw = &mut self.g_mut(g).bw;
            if runtime > bw.runtime {
                runtime = bw.runtime;
            }
            bw.runtime -= runtime;
            remaining = bw.runtime;
            self.c_mut(cfs).runtime_remaining += runtime as i64;
            if self.c(cfs).runtime_remaining > 0 {
                to_unthrottle.push(cfs);
            } else {
                throttled = true;
            }
        }
        for cfs in to_unthrottle {
            if self.cfs_rq_throttled(cfs) {
                self.unthrottle_cfs_rq(cfs);
            }
        }
        throttled
    }

    /// `do_sched_cfs_period_timer`: reabastece o pool e distribui. Devolve `true` se o timer pode
    /// parar (grupo ocioso e nada estrangulado).
    fn do_sched_cfs_period_timer(&mut self, g: GroupId, overrun: u64) -> bool {
        if self.g(g).bw.quota.is_none() {
            return true;
        }
        let mut throttled = !self.g(g).bw.throttled.is_empty();
        self.g_mut(g).bw.nr_periods += overrun;
        self.refill_cfs_bandwidth_runtime(g);
        if self.g(g).bw.idle && !throttled {
            return true;
        }
        if !throttled {
            self.g_mut(g).bw.idle = true;
            return false;
        }
        self.g_mut(g).bw.nr_throttled += overrun;
        while throttled && self.g(g).bw.runtime > 0 {
            throttled = self.distribute_cfs_runtime(g);
        }
        self.g_mut(g).bw.idle = false;
        false
    }

    /// `sched_cfs_period_timer`.
    pub(crate) fn sched_cfs_period_timer(&mut self, g: GroupId) {
        let mut idle = false;
        loop {
            let now = self.now;
            let bw = &mut self.g_mut(g).bw;
            let period = bw.period;
            let overrun = hrtimer_forward(&mut bw.timer_expires, now, period);
            if overrun == 0 {
                break;
            }
            idle = self.do_sched_cfs_period_timer(g, overrun);
        }
        if idle {
            self.g_mut(g).bw.period_active = false;
        }
    }

    /// `runtime_refresh_within`: o reabastecimento está a menos de `min_expire`?
    fn runtime_refresh_within(&self, g: GroupId, min_expire: u64) -> bool {
        let remaining = self.g(g).bw.timer_expires as i64 - self.now as i64;
        remaining < min_expire as i64
    }

    /// `start_cfs_slack_bandwidth`.
    fn start_cfs_slack_bandwidth(&mut self, g: GroupId) {
        let min_left = CFS_BANDWIDTH_SLACK_PERIOD_NS + MIN_BANDWIDTH_EXPIRATION_NS;
        if self.runtime_refresh_within(g, min_left) {
            return;
        }
        if self.g(g).bw.slack_started {
            return;
        }
        let now = self.now;
        let bw = &mut self.g_mut(g).bw;
        bw.slack_started = true;
        bw.slack_expires = Some(now + CFS_BANDWIDTH_SLACK_PERIOD_NS);
    }

    /// `return_cfs_rq_runtime`: fila que esvaziou devolve o que passa de 1 ms ao pool.
    pub(crate) fn return_cfs_rq_runtime(&mut self, cfs: CfsId) {
        if !self.c(cfs).runtime_enabled || self.c(cfs).nr_running != 0 {
            return;
        }
        let slack = self.c(cfs).runtime_remaining - MIN_CFS_RQ_RUNTIME_NS;
        if slack <= 0 {
            return;
        }
        let g = self.c(cfs).tg;
        let slice = self.cfg.tunables.cfs_bandwidth_slice_ns;
        if self.g(g).bw.quota.is_some() {
            self.g_mut(g).bw.runtime += slack as u64;
            if self.g(g).bw.runtime > slice && !self.g(g).bw.throttled.is_empty() {
                self.start_cfs_slack_bandwidth(g);
            }
        }
        self.c_mut(cfs).runtime_remaining -= slack;
    }

    /// `do_sched_cfs_slack_timer`.
    pub(crate) fn do_sched_cfs_slack_timer(&mut self, g: GroupId) {
        self.g_mut(g).bw.slack_started = false;
        if self.runtime_refresh_within(g, MIN_BANDWIDTH_EXPIRATION_NS) {
            return;
        }
        let slice = self.cfg.tunables.cfs_bandwidth_slice_ns;
        if self.g(g).bw.quota.is_none() || self.g(g).bw.runtime <= slice {
            return;
        }
        self.distribute_cfs_runtime(g);
    }

    /// `check_enqueue_throttle`: grupo que acorda sem runtime é estrangulado já na entrada.
    pub(crate) fn check_enqueue_throttle(&mut self, cfs: CfsId) {
        if !self.c(cfs).runtime_enabled || self.has_curr(cfs) {
            return;
        }
        if self.cfs_rq_throttled(cfs) {
            return;
        }
        self.account_cfs_rq_runtime(cfs, 0);
        if self.c(cfs).runtime_remaining <= 0 {
            self.throttle_cfs_rq(cfs);
        }
    }

    /// `sync_throttle`: fila nova herda a contagem de estrangulamento do pai.
    pub(crate) fn sync_throttle(&mut self, g: GroupId, slot: usize) {
        let Some(parent) = self.g(g).parent else { return };
        let cfs = self.g(g).cfs_rq[slot];
        let pcfs = self.g(parent).cfs_rq[slot];
        let count = self.c(pcfs).throttle_count;
        let pelt = self.sched_clock();
        let c = self.c_mut(cfs);
        c.throttle_count = count;
        c.throttled_clock_pelt = pelt;
    }

    /// `check_cfs_rq_runtime`: estrangula a fila que ficou sem runtime. Devolve se está estrangulada.
    pub(crate) fn check_cfs_rq_runtime(&mut self, cfs: CfsId) -> bool {
        if !self.c(cfs).runtime_enabled || self.c(cfs).runtime_remaining > 0 {
            return false;
        }
        if self.cfs_rq_throttled(cfs) {
            return true;
        }
        self.throttle_cfs_rq(cfs)
    }
}

#[cfg(test)]
mod tests {
    use super::hrtimer_forward;

    #[test]
    fn forward_like_hrtimer() {
        let mut e = 100;
        assert_eq!(hrtimer_forward(&mut e, 50, 100), 0);
        assert_eq!(e, 100);
        assert_eq!(hrtimer_forward(&mut e, 100, 100), 1);
        assert_eq!(e, 200);
        assert_eq!(hrtimer_forward(&mut e, 450, 100), 3);
        assert_eq!(e, 500);
        assert_eq!(hrtimer_forward(&mut e, 700, 100), 3);
        assert_eq!(e, 800);
    }
}
