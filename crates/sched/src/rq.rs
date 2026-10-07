//! [`RunQueue`]: o atalho de uma CPU só, com todas as tarefas no grupo raiz, sobre o [`Sched`].
//!
//! É a forma mais simples de usar o escalonador (e a que os experimentos H11 a H13 usam). As operações
//! correspondem às do kernel:
//!
//! | Aqui | Kernel |
//! |---|---|
//! | [`RunQueue::create_task`] | `sched_fork` + `set_load_weight` |
//! | [`Sched::wake_up_new_task`] | `wake_up_new_task` (`activate_task(ENQUEUE_INITIAL)` + `wakeup_preempt(WF_FORK)`) |
//! | [`Sched::try_to_wake_up`] | `try_to_wake_up` (`ttwu_runnable` ou `ttwu_do_activate`) |
//! | [`RunQueue::schedule`] | `__schedule` (`try_to_block_task` + `pick_next_task` + troca) |
//! | [`RunQueue::tick`] | `sched_tick` + `task_tick_fair` + `entity_tick` |
//! | [`Sched::set_user_nice`] | `set_user_nice` + `reweight_task_fair` + `prio_changed_fair` |
//! | [`Sched::set_custom_slice`] | `sched_setattr` com `sched_runtime` (`__sched_setscheduler`) |
//! | [`RunQueue::yield_current`] | `do_sched_yield` (`yield_task_fair` + `schedule`) |
//! | [`RunQueue::exit_current`] | `do_task_dead` (bloqueio com `DEQUEUE_SPECIAL`) |

use crate::clock::Clock;
use crate::features::{Features, Tunables};
use crate::sched::{RqStats, Sched, SchedConfig};
use crate::timeline::{CurrView, Timeline};
use crate::{EntityId, GroupId, TaskId};

/// Uma CPU sem grupos.
#[derive(Debug)]
pub struct RunQueue<C: Clock> {
    pub(crate) s: Sched<C>,
}

/// As operações que não dependem da CPU (relógio, tarefas, despertar, nice, fatia) são as do
/// [`Sched`] direto; as de baixo fixam a CPU 0.
impl<C: Clock> std::ops::Deref for RunQueue<C> {
    type Target = Sched<C>;

    fn deref(&self) -> &Sched<C> {
        &self.s
    }
}

impl<C: Clock> std::ops::DerefMut for RunQueue<C> {
    fn deref_mut(&mut self) -> &mut Sched<C> {
        &mut self.s
    }
}

impl<C: Clock> RunQueue<C> {
    /// Runqueue vazia.
    pub fn new(clock: C, tunables: Tunables, features: Features) -> RunQueue<C> {
        RunQueue { s: Sched::new(clock, SchedConfig::single_cpu(tunables, features)) }
    }

    /// O escalonador por baixo.
    pub fn sched(&self) -> &Sched<C> {
        &self.s
    }

    /// Features em uso.
    pub fn features(&self) -> Features {
        self.s.config().features
    }

    /// Parâmetros em uso.
    pub fn tunables(&self) -> Tunables {
        self.s.config().tunables
    }

    /// Tarefa rodando; `None` é idle.
    pub fn current(&self) -> Option<TaskId> {
        self.s.current(0)
    }

    /// Diz se o corrente precisa ceder a CPU no próximo ponto de preempção.
    pub fn need_resched(&self) -> bool {
        self.s.need_resched(0)
    }

    /// `cfs_rq->nr_running`.
    pub fn nr_running(&self) -> u32 {
        self.s.root_nr_running(0)
    }

    /// `rq->nr_running`.
    pub fn rq_nr_running(&self) -> u32 {
        self.s.rq_nr_running(0)
    }

    /// `cfs_rq->h_nr_delayed`.
    pub fn nr_delayed(&self) -> u32 {
        self.s.root_nr_delayed(0)
    }

    /// `cfs_rq->load.weight`.
    pub fn load_weight(&self) -> u64 {
        self.s.root_load_weight(0)
    }

    /// Contadores.
    pub fn stats(&self) -> RqStats {
        self.s.stats(0)
    }

    /// A linha do tempo (árvore e somas).
    pub fn timeline(&self) -> &Timeline {
        self.s.root_timeline(0)
    }

    /// V atual, sem mexer no estado.
    pub fn avg_vruntime_peek(&self) -> u64 {
        self.s.avg_vruntime_peek(0)
    }

    /// Visão do corrente.
    pub fn curr_view(&self) -> Option<CurrView> {
        self.s.curr_view(0)
    }

    /// O que o `pick_eevdf` escolheria agora.
    pub fn peek_pick(&self) -> Option<EntityId> {
        self.s.peek_pick(0)
    }

    /// A mesma escolha por força bruta.
    pub fn peek_pick_linear(&self) -> Option<EntityId> {
        self.s.peek_pick_linear(0)
    }

    /// Entidades na fila, com `(entidade, vruntime, peso escalado)`.
    pub fn queued_entities(&self) -> Vec<(EntityId, u64, u64)> {
        self.s.queued_entities(0)
    }

    /// `sched_fork` no grupo raiz.
    pub fn create_task(&mut self, nice: i32) -> TaskId {
        self.s.create_task(nice, GroupId::ROOT, 0)
    }

    /// `__schedule`.
    pub fn schedule(&mut self, prev_blocks: bool) -> Option<TaskId> {
        self.s.schedule(0, prev_blocks)
    }

    /// `do_task_dead`.
    pub fn exit_current(&mut self) -> Option<TaskId> {
        self.s.exit_current(0)
    }

    /// `sched_tick`.
    pub fn tick(&mut self) {
        self.s.tick(0);
    }

    /// `sched_yield`.
    pub fn yield_current(&mut self) -> Option<TaskId> {
        self.s.yield_current(0)
    }
}

#[cfg(test)]
mod tests;
