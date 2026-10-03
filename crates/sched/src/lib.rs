//! Escalonador EEVDF (Earliest Eligible Virtual Deadline First) feito à mão, fiel ao
//! `kernel/sched/fair.c` do Linux **6.12.101** (o kernel do Debian 13 usado como referência), com
//! hierarquia de grupos (`CONFIG_FAIR_GROUP_SCHED`), controle de banda (`CONFIG_CFS_BANDWIDTH`, o
//! `cpu.max`) e várias CPUs.
//!
//! # Peças
//!
//! - [`weight`]: tabelas `sched_prio_to_weight`/`sched_prio_to_wmult`, `scale_load`,
//!   `__calc_delta`/`calc_delta_fair` com o mesmo ponto fixo do kernel, `div_s64` com divisor de 32 bits,
//!   e o mapeamento do `cpu.weight` do cgroup v2 pra shares.
//! - [`features`]: as features do `features.h` que mudam decisões ([`Features`], padrão = 6.12.101) e
//!   os parâmetros derivados de sysctl, CPUs e `HZ` ([`Tunables`]): fatia base
//!   `0,70 ms * (1 + ilog2(min(ncpus, 8)))`, `TICK_NSEC`, fatia de banda de 5 ms.
//! - [`clock`]: o relógio injetável ([`Clock`], [`ManualClock`], [`MonotonicClock`]).
//! - [`timeline`]: a árvore rubro-negra aumentada (crate `rbtree`) ordenada pela deadline virtual com
//!   comparação circular, augmentação `min_vruntime`/`min_slice`, a conta do V relativa a
//!   `zero_vruntime`, a elegibilidade sem divisão e o `pick_eevdf` em O(log n).
//! - [`pelt`]: o PELT (carga média com meia-vida de 32 ms) que alimenta o peso das entidades de grupo.
//! - [`Sched`] (módulos `sched`, `fair`, `bandwidth`, `balance`): o escalonador de várias CPUs com
//!   grupos aninhados, controle de banda e balanceamento; [`RunQueue`] é o atalho de uma CPU só, sem
//!   grupos.
//! - [`sim`]: simulador de eventos discretos (tick, timers com folga, timers de banda, rajadas de CPU
//!   e sono, várias CPUs) rodando o mesmo [`Sched`].
//!
//! # Fidelidade
//!
//! Toda conta usa os tipos e arredondamentos do kernel: vruntime e deadline em `u64` com comparação
//! `(s64)(a - b)`, somas com sinal que dão a volta, piso explícito no V, inverso de 32 bits no
//! `__calc_delta`. Observações sobre o design v2, confirmadas no código da 6.12.101:
//!
//! - A árvore compara **só a deadline**; empates vão pra direita, ou seja, ficam na ordem de chegada.
//!   Não há desempate por id.
//! - `div_s64` recebe o divisor como `s32`; `avg_load` é `u64` no `cfs_rq`, convertido pra `long` nas
//!   contas.
//!
//! As aproximações (PELT só de carga e sem propagação, balanceamento de um domínio só) estão
//! documentadas em `fair` e [`balance`]. O experimento E02 da bancada
//! (`testbench/experiments/e02-sched-eevdf`) compara este código com o kernel do host, com e sem
//! cgroups.

pub mod balance;
pub mod bandwidth;
pub mod clock;
mod fair;
pub mod features;
#[cfg(test)]
mod group_tests;
mod invariants;
pub mod pelt;
pub mod rq;
mod sched;
pub mod sim;
pub mod timeline;
pub mod weight;

pub use balance::BalanceConfig;
pub use bandwidth::BandwidthError;
pub use clock::{Clock, ManualClock, MonotonicClock};
pub use features::{Features, TunableScaling, Tunables};
pub use rq::RunQueue;
pub use sched::{GroupInfo, MAX_CPUS, RqStats, SCHED_CLOCK_BASE, Sched, SchedConfig, TaskInfo, Topology};
pub use sim::{
    SimConfig, SimGroup, SimGroupReport, SimReport, SimTask, SimTaskReport, Simulator, Workload, percentile, simulate,
};
pub use timeline::{CurrView, EevdfAugment, EevdfSummary, QueuedEntity, Timeline, VDeadline};
pub use weight::LoadWeight;

/// Identificador de uma entidade de escalonamento (tarefa ou entidade de grupo) num [`Sched`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(u32);

impl EntityId {
    /// Monta um id a partir do índice (útil pra montar estados da [`Timeline`] à mão).
    pub const fn from_index(index: u32) -> EntityId {
        EntityId(index)
    }

    /// Índice do id.
    pub const fn index(self) -> u32 {
        self.0
    }
}

/// Identificador de tarefa: o índice da entidade dela.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TaskId(u32);

impl TaskId {
    /// Monta um id a partir do índice.
    pub const fn from_index(index: u32) -> TaskId {
        TaskId(index)
    }

    /// Índice do id.
    pub const fn index(self) -> u32 {
        self.0
    }

    /// A entidade da tarefa (`&p->se`).
    pub const fn entity(self) -> EntityId {
        EntityId(self.0)
    }
}

/// Identificador de grupo (`struct task_group`, um cgroup com o controlador `cpu`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupId(u32);

impl GroupId {
    /// O `root_task_group`.
    pub const ROOT: GroupId = GroupId(0);

    /// Monta um id a partir do índice.
    pub const fn from_index(index: u32) -> GroupId {
        GroupId(index)
    }

    /// Índice do id.
    pub const fn index(self) -> u32 {
        self.0
    }
}
