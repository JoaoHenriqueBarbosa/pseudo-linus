//! Balanceamento de carga entre CPUs no modelo de uma runqueue por CPU.
//!
//! É uma redução do `sched_balance_rq` do fair.c (6.12.101) pra um domínio só, em que cada grupo de
//! balanceamento é uma CPU (o caso de uma VPS de poucas vCPUs, ou de um par SMT). O que foi portado:
//!
//! - **gatilho**: no tick, quando passou o intervalo do domínio (`get_sd_balance_interval`:
//!   `balance_interval` ms, vezes `busy_factor` se a CPU está ocupada, menos um jiffy, limitado a HZ/10),
//!   e quando a CPU fica sem tarefa (`sched_balance_newidle`);
//! - **classificação** de cada CPU por número de tarefas: ociosa (`group_has_spare`), uma tarefa
//!   (`group_fully_busy`) ou mais de uma (`group_overloaded`). Com tarefas CPU-bound a utilização é
//!   sempre a capacidade, então a classificação por `util`/`runnable` do kernel dá o mesmo resultado;
//! - **escolha da origem** (`sched_balance_find_src_group`): a CPU de tipo mais alto e, no empate entre
//!   sobrecarregadas, a de maior carga (`cpu_load` = carga PELT da raiz); sem balanceamento se a local é
//!   de tipo maior, se está acima da média, ou se a origem não passa de `imbalance_pct`;
//! - **desequilíbrio** (`calculate_imbalance`): CPU ociosa puxa metade da diferença de tarefas
//!   (`migrate_task`); senão, carga (`migrate_load`) com `min(origem - média, média - local)`;
//! - **escolha das tarefas** (`detach_tasks`): da menos recente pra mais recente (a cauda do
//!   `cfs_tasks`), pulando quem roda, quem não pode ir pra esta CPU e quem está quente (rodou há menos
//!   de `migration_cost`, ou é o "next" da fila) enquanto `nr_balance_failed <= cache_nice_tries`; com
//!   `migrate_load`, pula quem tem `task_h_load >> nr_balance_failed` maior que o desequilíbrio que
//!   falta;
//! - **falha e reset**: rodada sem migração incrementa `nr_balance_failed`; com sucesso zera; o
//!   intervalo volta ao mínimo quando havia desequilíbrio e dobra (até o máximo) quando estava
//!   balanceado; com `migrate_task` e falhas acima de `cache_nice_tries + 2`, o balanceamento ativo leva
//!   a tarefa menos recente mesmo quente;
//! - **carga bloqueada** (`sched_balance_update_blocked_averages`): antes de cada balanceamento
//!   periódico e de cada newidle, a carga das filas que esvaziaram decai e chega no `tg->load_avg`;
//! - **NOHZ** (`nohz_balancer_kick` + `_nohz_idle_balance`): o tick de uma CPU ocupada atualiza a
//!   carga bloqueada das CPUs ociosas a cada 32 ms e, com esta CPU sobrecarregada ou mais de uma CPU
//!   ocupada, faz as CPUs ociosas cujo intervalo venceu puxarem trabalho. O kernel faz isso numa CPU
//!   ociosa acordada por IPI; aqui é feito na hora, no tick.
//!
//! Fica de fora: domínios em vários níveis, capacidade reduzida por IRQ e RT, NUMA, EAS, assimetria;
//! no newidle, o corte por `avg_idle` e o sorteio do NI_RANDOM (o `rd->overloaded` é aproximado por
//! "alguma CPU tem 2 tarefas ou mais agora"); e a escolha de CPU no wakeup (`select_task_rq_fair`): a
//! tarefa acorda na CPU em que dormiu, e só o balanceamento a move.

use crate::TaskId;
use crate::clock::Clock;
use crate::sched::{Kind, Sched};

/// Parâmetros do domínio de balanceamento (`struct sched_domain`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BalanceConfig {
    pub enabled: bool,
    /// Puxar trabalho quando a CPU fica ociosa.
    pub newidle: bool,
    /// `sd->min_interval` em ms (o kernel usa o número de CPUs do domínio).
    pub min_interval_ms: u64,
    /// `sd->max_interval` em ms.
    pub max_interval_ms: u64,
    /// `sd->busy_factor`.
    pub busy_factor: u64,
    /// `sd->imbalance_pct`.
    pub imbalance_pct: u64,
    /// `sd->cache_nice_tries`.
    pub cache_nice_tries: u32,
    /// `SD_SHARE_CPUCAPACITY` (irmãos SMT): tarefas nunca estão quentes.
    pub share_cpucapacity: bool,
    /// `sysctl_sched_nr_migrate`.
    pub nr_migrate: u32,
    /// `HZ`, pra converter intervalos em jiffies.
    pub hz: u64,
}

impl BalanceConfig {
    /// Sem balanceamento.
    pub const fn disabled() -> BalanceConfig {
        BalanceConfig {
            enabled: false,
            newidle: false,
            min_interval_ms: 1,
            max_interval_ms: 2,
            busy_factor: 16,
            imbalance_pct: 117,
            cache_nice_tries: 1,
            share_cpucapacity: false,
            nr_migrate: 32,
            hz: 250,
        }
    }

    /// Domínio MC (CPUs que dividem o último nível de cache): `imbalance_pct` 117, `cache_nice_tries`
    /// 1, intervalo de `cpus` ms (o dobro no máximo), `busy_factor` 16.
    pub const fn mc_domain(cpus: usize) -> BalanceConfig {
        BalanceConfig {
            enabled: true,
            newidle: true,
            min_interval_ms: cpus as u64,
            max_interval_ms: 2 * cpus as u64,
            busy_factor: 16,
            imbalance_pct: 117,
            cache_nice_tries: 1,
            share_cpucapacity: false,
            nr_migrate: 32,
            hz: 250,
        }
    }

    /// Domínio SMT (irmãos de um núcleo): `imbalance_pct` 110, `cache_nice_tries` 0, sem tarefa quente.
    pub const fn smt_domain(cpus: usize) -> BalanceConfig {
        BalanceConfig {
            enabled: true,
            newidle: true,
            min_interval_ms: cpus as u64,
            max_interval_ms: 2 * cpus as u64,
            busy_factor: 16,
            imbalance_pct: 110,
            cache_nice_tries: 0,
            share_cpucapacity: true,
            nr_migrate: 32,
            hz: 250,
        }
    }
}

/// Estado de balanceamento de uma CPU no domínio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CpuBalance {
    pub last_balance: u64,
    /// `sd->balance_interval` em ms.
    pub interval_ms: u64,
    pub nr_balance_failed: u32,
    /// `rq->has_blocked_load`: a CPU entrou em idle com carga bloqueada ainda por decair.
    pub has_blocked_load: bool,
    /// `rq->last_blocked_load_update_tick`, em jiffies.
    pub last_blocked_update_jiffy: u64,
}

impl CpuBalance {
    pub(crate) fn new(cfg: &BalanceConfig, now: u64) -> CpuBalance {
        CpuBalance {
            last_balance: now,
            interval_ms: cfg.min_interval_ms,
            nr_balance_failed: 0,
            has_blocked_load: false,
            last_blocked_update_jiffy: 0,
        }
    }
}

/// O estado global do NOHZ (`struct nohz`) que importa aqui.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Nohz {
    /// `nohz.has_blocked`: alguma CPU ociosa pode ter carga bloqueada.
    pub has_blocked: bool,
    /// `nohz.next_blocked` em ns: próxima atualização da carga bloqueada das CPUs ociosas.
    pub next_blocked: u64,
}

/// `LOAD_AVG_PERIOD` em ms: cadência da atualização da carga bloqueada das CPUs ociosas.
const LOAD_AVG_PERIOD_MS: u64 = 32;

/// `enum group_type`, só os três casos que importam aqui.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum GroupType {
    HasSpare,
    FullyBusy,
    Overloaded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Migration {
    Load,
    Task,
}

impl<C: Clock> Sched<C> {
    fn jiffy_ns(&self) -> u64 {
        1_000_000_000 / self.cfg.balance.hz
    }

    /// `get_sd_balance_interval` em ns.
    fn balance_interval_ns(&self, cpu: usize, busy: bool) -> u64 {
        let b = &self.cfg.balance;
        let mut ms = self.cpus[cpu].balance.interval_ms;
        if busy {
            ms *= b.busy_factor;
        }
        let mut jiffies = (ms * b.hz).div_ceil(1000);
        if busy {
            jiffies = jiffies.saturating_sub(1);
        }
        let max = (b.hz / 10).max(1);
        jiffies.clamp(1, max) * self.jiffy_ns()
    }

    /// O que o tick de uma CPU ocupada dispara (`sched_balance_trigger`): a softirq de balanceamento
    /// quando vence o intervalo do domínio (`sched_balance_softirq`: carga bloqueada e depois
    /// `sched_balance_domains`) e o chute do NOHZ pras CPUs ociosas (`nohz_balancer_kick`).
    pub(crate) fn periodic_balance(&mut self, cpu: usize) {
        if self.shared() || self.cfg.cpus < 2 {
            return;
        }
        let busy = self.cpus[cpu].curr.is_some();
        let interval = self.balance_interval_ns(cpu, busy);
        if self.now >= self.cpus[cpu].balance.last_balance + interval {
            self.update_blocked_averages(cpu);
            self.load_balance(cpu, !busy, false);
            self.cpus[cpu].balance.last_balance = self.now;
        }
        if busy {
            self.nohz_balancer_kick(cpu);
        }
    }

    /// CPU sem tarefa e sem nada na fila (`idle_cpu`).
    fn cpu_idle(&self, cpu: usize) -> bool {
        self.cpus[cpu].curr.is_none() && self.rq_nr_running(cpu) == 0
    }

    /// `nohz_balance_enter_idle`: a CPU parou o tick em idle; supõe carga bloqueada e liga a
    /// atualização periódica da carga das CPUs ociosas.
    pub(crate) fn nohz_balance_enter_idle(&mut self, cpu: usize) {
        if self.shared() || self.cfg.cpus < 2 || !self.cfg.balance.enabled {
            return;
        }
        self.cpus[cpu].balance.has_blocked_load = true;
        self.nohz.has_blocked = true;
    }

    /// `nohz_balancer_kick` + `_nohz_idle_balance`, feitos na hora pela CPU ocupada (no kernel, um IPI
    /// acorda uma CPU ociosa que faz o trabalho em nome das outras). Com alguma CPU ociosa:
    ///
    /// - `NOHZ_STATS_KICK` a cada `LOAD_AVG_PERIOD` (32 ms) enquanto houver carga bloqueada: decai a carga
    ///   bloqueada de cada CPU ociosa (`update_nohz_stats`);
    /// - `NOHZ_BALANCE_KICK` quando esta CPU tem 2 tarefas ou mais, ou quando há mais de uma CPU ocupada
    ///   no domínio (`nr_busy_cpus` do LLC): cada CPU ociosa cujo intervalo venceu balanceia como ociosa
    ///   (`sched_balance_domains(rq, CPU_IDLE)`) e puxa trabalho.
    fn nohz_balancer_kick(&mut self, this: usize) {
        let idle: Vec<usize> = (1..=self.cfg.cpus).map(|k| (this + k) % self.cfg.cpus).filter(|&c| self.cpu_idle(c)).collect();
        if idle.is_empty() {
            return;
        }
        let stats = self.nohz.has_blocked && self.now > self.nohz.next_blocked;
        let nohz_next_balance = idle
            .iter()
            .map(|&c| self.cpus[c].balance.last_balance + self.balance_interval_ns(c, false))
            .min()
            .unwrap_or(u64::MAX);
        let nr_busy = (0..self.cfg.cpus).filter(|&c| self.cpus[c].curr.is_some()).count();
        let balance = self.now >= nohz_next_balance && (self.rq_nr_running(this) >= 2 || nr_busy > 1);
        if !stats && !balance {
            return;
        }
        if stats {
            self.nohz.has_blocked = false;
        }
        let jiffy = self.jiffy_ns();
        let mut has_blocked = false;
        for c in idle {
            if !self.cpu_idle(c) {
                continue;
            }
            if stats && self.cpus[c].balance.has_blocked_load {
                // update_nohz_stats: no máximo uma vez por jiffy.
                has_blocked |= if self.now / jiffy > self.cpus[c].balance.last_blocked_update_jiffy {
                    self.update_blocked_averages(c)
                } else {
                    true
                };
            }
            if balance && self.now >= self.cpus[c].balance.last_balance + self.balance_interval_ns(c, false) {
                self.load_balance(c, true, false);
                self.cpus[c].balance.last_balance = self.now;
            }
        }
        if stats {
            self.nohz.next_blocked = self.now + LOAD_AVG_PERIOD_MS * 1_000_000;
        }
        if has_blocked {
            self.nohz.has_blocked = true;
        }
    }

    /// `sched_balance_newidle`: CPU sem tarefa tenta puxar, se alguma CPU tem mais de uma tarefa
    /// (`rd->overloaded`), depois de atualizar a própria carga bloqueada. Devolve se puxou alguma.
    pub(crate) fn newidle_balance(&mut self, cpu: usize) -> bool {
        if self.shared() || self.cfg.cpus < 2 {
            return false;
        }
        if !(0..self.cfg.cpus).any(|c| self.rq_nr_running(c) >= 2) {
            return false;
        }
        self.update_blocked_averages(cpu);
        self.load_balance(cpu, true, true) > 0
    }

    fn cpu_load(&self, cpu: usize) -> u64 {
        self.c(self.cpus[cpu].root).avg.load_avg
    }

    fn group_type(&self, cpu: usize) -> GroupType {
        match self.c(self.cpus[cpu].root).h_nr_queued {
            0 => GroupType::HasSpare,
            1 => GroupType::FullyBusy,
            _ => GroupType::Overloaded,
        }
    }

    /// Diz se a tarefa está "quente" na CPU de origem (`task_hot`).
    fn task_hot(&self, p: TaskId, dst: usize) -> bool {
        if self.cfg.balance.share_cpucapacity {
            return false;
        }
        let se = p.entity();
        let cfs = self.cfs_rq_of(se);
        if self.cfg.features.cache_hot_buddy && self.rq_nr_running(dst) > 0 && self.c(cfs).next == Some(se) {
            return true;
        }
        let delta = self.now as i64 - self.e(se).exec_start as i64;
        delta < self.cfg.tunables.migration_cost_ns as i64
    }

    /// Tarefas na fila da CPU que poderiam migrar, da menos recente pra mais recente.
    fn migration_candidates(&self, src: usize) -> Vec<TaskId> {
        let mut v: Vec<(u64, TaskId)> = self
            .ents
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                Some(e) => match &e.kind {
                    Kind::Task(d) if d.cpu == src && d.queued && e.on_rq && !e.sched_delayed => {
                        Some((e.exec_start, TaskId::from_index(i as u32)))
                    }
                    _ => None,
                },
                None => None,
            })
            .filter(|(_, t)| self.cpus[src].curr != Some(*t))
            .collect();
        v.sort_unstable();
        v.into_iter().map(|(_, t)| t).collect()
    }

    /// Uma rodada de `sched_balance_rq` pra `this` (destino). Devolve quantas tarefas migrou.
    fn load_balance(&mut self, this: usize, idle: bool, newly_idle: bool) -> u32 {
        let b = self.cfg.balance;
        let local_type = self.group_type(this);
        let local_load = self.cpu_load(this);

        // sched_balance_find_src_group
        let mut busiest: Option<usize> = None;
        for cpu in 0..self.cfg.cpus {
            if cpu == this {
                continue;
            }
            let t = self.group_type(cpu);
            if t == GroupType::HasSpare {
                continue;
            }
            busiest = match busiest {
                None => Some(cpu),
                Some(bc) => {
                    let bt = self.group_type(bc);
                    if t > bt || (t == bt && self.cpu_load(cpu) > self.cpu_load(bc)) { Some(cpu) } else { Some(bc) }
                }
            };
        }
        let total: u64 = (0..self.cfg.cpus).map(|c| self.cpu_load(c)).sum();
        let avg = total / self.cfg.cpus as u64;

        let mut balanced = true;
        let mut migration = Migration::Load;
        let mut imbalance: i64 = 0;
        if let Some(bc) = busiest {
            let busiest_type = self.group_type(bc);
            let busiest_load = self.cpu_load(bc);
            let busiest_nr = self.c(self.cpus[bc].root).h_nr_queued;
            let local_nr = self.c(self.cpus[this].root).h_nr_queued;
            if local_type > busiest_type || busiest_type == GroupType::FullyBusy && local_type != GroupType::HasSpare {
                balanced = true;
            } else if local_type == GroupType::HasSpare {
                // Uma CPU de cada lado: puxa metade da diferença de tarefas.
                migration = Migration::Task;
                imbalance = (i64::from(busiest_nr) - i64::from(local_nr)) >> 1;
                balanced = imbalance <= 0;
            } else if busiest_type == GroupType::Overloaded {
                if local_load >= busiest_load
                    || local_load >= avg
                    || 100 * busiest_load <= b.imbalance_pct * local_load
                {
                    balanced = true;
                } else {
                    imbalance = ((busiest_load - avg).min(avg - local_load)) as i64;
                    balanced = imbalance <= 0;
                }
            }
            if !balanced {
                let moved = self.detach_and_attach(bc, this, migration, imbalance, idle);
                if moved == 0 {
                    self.cpus[this].stats.nr_balance_failed += 1;
                    if !newly_idle {
                        self.cpus[this].balance.nr_balance_failed += 1;
                    }
                    // Balanceamento ativo (imbalanced_active_balance): só pra migrate_task.
                    let failed = self.cpus[this].balance.nr_balance_failed;
                    if migration == Migration::Task && failed > b.cache_nice_tries + 2 {
                        let active = self.active_balance(bc, this);
                        self.cpus[this].balance.nr_balance_failed = b.cache_nice_tries + 1;
                        if active > 0 {
                            self.cpus[this].balance.interval_ms = b.min_interval_ms;
                            return active;
                        }
                    }
                } else {
                    self.cpus[this].balance.nr_balance_failed = 0;
                }
                self.cpus[this].balance.interval_ms = b.min_interval_ms;
                return moved;
            }
        }
        // out_balanced
        self.cpus[this].balance.nr_balance_failed = 0;
        if !newly_idle && self.cpus[this].balance.interval_ms < b.max_interval_ms {
            self.cpus[this].balance.interval_ms *= 2;
        }
        0
    }

    /// `detach_tasks` + `attach_tasks`.
    fn detach_and_attach(&mut self, src: usize, dst: usize, migration: Migration, mut imbalance: i64, idle: bool) -> u32 {
        let b = self.cfg.balance;
        if self.c(self.cpus[src].root).h_nr_queued <= 1 || imbalance <= 0 {
            return 0;
        }
        let failed = self.cpus[dst].balance.nr_balance_failed;
        let loop_max = b.nr_migrate.min(self.rq_nr_running(src)) as usize;
        let mut moved = 0;
        for (i, p) in self.migration_candidates(src).into_iter().enumerate() {
            if i >= loop_max {
                break;
            }
            if idle && self.rq_nr_running(src) <= 1 {
                break;
            }
            if self.td(p).affinity & (1 << dst) == 0 {
                continue;
            }
            if self.throttled_hierarchy(self.cfs_rq_of(p.entity())) {
                continue;
            }
            let hot = self.task_hot(p, dst);
            if hot && failed <= b.cache_nice_tries {
                continue;
            }
            match migration {
                Migration::Load => {
                    let load = self.task_h_load(p).max(1);
                    if self.cfg.features.lb_min && load < 16 && failed == 0 {
                        continue;
                    }
                    if (load >> failed.min(63)) as i64 > imbalance {
                        continue;
                    }
                    imbalance -= load as i64;
                }
                Migration::Task => imbalance -= 1,
            }
            self.move_queued_task(p, dst);
            self.cpus[dst].stats.nr_pulled += 1;
            moved += 1;
            // Com preempção, uma CPU que acabou de ficar ociosa puxa uma tarefa só.
            if idle {
                break;
            }
            if imbalance <= 0 {
                break;
            }
        }
        moved
    }

    /// `active_load_balance_cpu_stop`: leva a tarefa menos recente da origem, mesmo quente.
    fn active_balance(&mut self, src: usize, dst: usize) -> u32 {
        if self.rq_nr_running(src) <= 1 {
            return 0;
        }
        for p in self.migration_candidates(src) {
            if self.td(p).affinity & (1 << dst) == 0 || self.throttled_hierarchy(self.cfs_rq_of(p.entity())) {
                continue;
            }
            self.move_queued_task(p, dst);
            self.cpus[dst].stats.nr_pulled += 1;
            return 1;
        }
        0
    }
}
