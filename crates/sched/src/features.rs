//! Features do escalonador (`kernel/sched/features.h`) e parâmetros ajustáveis (`sysctl_sched_*`).

/// Nanossegundos por milissegundo.
pub const NSEC_PER_MSEC: u64 = 1_000_000;

/// Nanossegundos por segundo.
pub const NSEC_PER_SEC: u64 = 1_000_000_000;

/// `normalized_sysctl_sched_base_slice` na 6.12.101: 0,70 ms (era 0,75 ms até a 6.12.30).
pub const NORMALIZED_BASE_SLICE_NS: u64 = 700_000;

/// Faixa aceita pra fatia própria de uma tarefa (`sched_attr.sched_runtime`): de 0,1 ms a 100 ms.
pub const CUSTOM_SLICE_MIN_NS: u64 = NSEC_PER_MSEC / 10;
pub const CUSTOM_SLICE_MAX_NS: u64 = NSEC_PER_MSEC * 100;

/// `TICK_NSEC` pra um dado `HZ`: `(NSEC_PER_SEC + HZ/2) / HZ`. Com HZ=250, 4 ms.
pub const fn tick_nsec(hz: u64) -> u64 {
    (NSEC_PER_SEC + hz / 2) / hz
}

/// As features do fair.c que mudam decisões do EEVDF numa runqueue. O `Default` é o da 6.12.101.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Features {
    /// PLACE_LAG: preserva o lag entre dormir e acordar, escalado por `(W + w) / W`.
    pub place_lag: bool,
    /// PLACE_DEADLINE_INITIAL: tarefa nova começa com meia fatia.
    pub place_deadline_initial: bool,
    /// PLACE_REL_DEADLINE: preserva a deadline relativa quando a tarefa sai e volta sem dormir.
    pub place_rel_deadline: bool,
    /// RUN_TO_PARITY: quem foi escolhido não é preemptado até gastar a fatia (`protect_slice`).
    pub run_to_parity: bool,
    /// PREEMPT_SHORT: quem acorda com fatia menor pode cancelar a proteção do corrente.
    pub preempt_short: bool,
    /// NEXT_BUDDY: quem acorda vira o "next" preferido (desligado por padrão).
    pub next_buddy: bool,
    /// PICK_BUDDY: o pick respeita o "next" quando ele é elegível.
    pub pick_buddy: bool,
    /// DELAY_DEQUEUE: quem dorme sem ser elegível fica na árvore até pagar o lag negativo.
    pub delay_dequeue: bool,
    /// DELAY_ZERO: lag positivo de quem sai atrasado (ou acorda atrasado) vira zero.
    pub delay_zero: bool,
    /// WAKEUP_PREEMPTION: quem acorda pode preemptar o corrente.
    pub wakeup_preemption: bool,
    /// CACHE_HOT_BUDDY: o "next" de uma fila conta como quente pro balanceamento.
    pub cache_hot_buddy: bool,
    /// LB_MIN: o balanceamento ignora tarefas de carga menor que 16 enquanto não falhou.
    pub lb_min: bool,
}

impl Default for Features {
    fn default() -> Self {
        Features::LINUX_6_12_101
    }
}

impl Features {
    /// Valores de `kernel/sched/features.h` na 6.12.101.
    pub const LINUX_6_12_101: Features = Features {
        place_lag: true,
        place_deadline_initial: true,
        place_rel_deadline: true,
        run_to_parity: true,
        preempt_short: true,
        next_buddy: false,
        pick_buddy: true,
        delay_dequeue: true,
        delay_zero: true,
        wakeup_preemption: true,
        cache_hot_buddy: true,
        lb_min: false,
    };
}

/// `sysctl_sched_tunable_scaling`: como a fatia base cresce com o número de CPUs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunableScaling {
    /// Fator 1.
    None,
    /// Fator `1 + ilog2(min(ncpus, 8))` (padrão).
    Log,
    /// Fator `min(ncpus, 8)`.
    Linear,
}

/// Parâmetros que o kernel deriva de sysctl, do número de CPUs e de `HZ`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tunables {
    /// `normalized_sysctl_sched_base_slice` em ns.
    pub normalized_base_slice_ns: u64,
    /// `sysctl_sched_tunable_scaling`.
    pub scaling: TunableScaling,
    /// `num_online_cpus()` usado no `get_update_sysctl_factor`.
    pub online_cpus: u32,
    /// `TICK_NSEC`.
    pub tick_nsec: u64,
    /// `sysctl_sched_cfs_bandwidth_slice` em ns: quanto runtime uma fila pega do grupo de cada vez.
    pub cfs_bandwidth_slice_ns: u64,
    /// `sysctl_sched_migration_cost` em ns: tarefa que rodou há menos que isso está "quente" pro
    /// balanceamento.
    pub migration_cost_ns: u64,
}

/// `sysctl_sched_cfs_bandwidth_slice` padrão: 5 ms.
pub const CFS_BANDWIDTH_SLICE_NS: u64 = 5 * NSEC_PER_MSEC;

/// `sysctl_sched_migration_cost` padrão: 0,5 ms.
pub const MIGRATION_COST_NS: u64 = 500_000;

impl Tunables {
    /// Valores da 6.12.101 pra uma máquina com `online_cpus` CPUs e um dado `HZ`.
    pub const fn linux_6_12_101(online_cpus: u32, hz: u64) -> Tunables {
        Tunables {
            normalized_base_slice_ns: NORMALIZED_BASE_SLICE_NS,
            scaling: TunableScaling::Log,
            online_cpus,
            tick_nsec: tick_nsec(hz),
            cfs_bandwidth_slice_ns: CFS_BANDWIDTH_SLICE_NS,
            migration_cost_ns: MIGRATION_COST_NS,
        }
    }

    /// `get_update_sysctl_factor()`.
    pub fn factor(&self) -> u64 {
        let cpus = self.online_cpus.clamp(1, 8);
        match self.scaling {
            TunableScaling::None => 1,
            TunableScaling::Linear => u64::from(cpus),
            TunableScaling::Log => 1 + u64::from(cpus.ilog2()),
        }
    }

    /// `sysctl_sched_base_slice` = fator * fatia normalizada. No host de teste (16 CPUs): 2,8 ms.
    pub fn base_slice_ns(&self) -> u64 {
        self.factor() * self.normalized_base_slice_ns
    }
}

impl Default for Tunables {
    /// Uma CPU e HZ=250: fatia de 0,7 ms e tick de 4 ms.
    fn default() -> Self {
        Tunables::linux_6_12_101(1, 250)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_slice_by_cpu_count() {
        let slice = |cpus| Tunables::linux_6_12_101(cpus, 250).base_slice_ns();
        assert_eq!(slice(1), 700_000);
        assert_eq!(slice(2), 1_400_000);
        assert_eq!(slice(3), 1_400_000);
        assert_eq!(slice(4), 2_100_000);
        assert_eq!(slice(8), 2_800_000);
        assert_eq!(slice(16), 2_800_000);
        assert_eq!(slice(128), 2_800_000);
    }

    #[test]
    fn tick_lengths() {
        assert_eq!(tick_nsec(250), 4_000_000);
        assert_eq!(tick_nsec(1000), 1_000_000);
        assert_eq!(tick_nsec(300), 3_333_333);
    }

    #[test]
    fn default_features_match_features_h() {
        let f = Features::default();
        assert!(f.place_lag && f.place_deadline_initial && f.place_rel_deadline);
        assert!(f.run_to_parity && f.preempt_short && f.pick_buddy);
        assert!(f.delay_dequeue && f.delay_zero && f.wakeup_preemption);
        assert!(f.cache_hot_buddy && !f.lb_min);
        assert!(!f.next_buddy);
    }
}
