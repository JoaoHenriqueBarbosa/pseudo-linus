//! O escalonador de várias CPUs com hierarquia de grupos: as estruturas do kernel (`struct rq`,
//! `struct cfs_rq`, `struct sched_entity`, `struct task_group`) e as operações que o core.c e o
//! syscalls.c fazem sobre a classe CFS.
//!
//! # Hierarquia
//!
//! Como no `CONFIG_FAIR_GROUP_SCHED`, cada grupo ([`GroupId`]) tem, por CPU, uma fila filha
//! (`tg->cfs_rq[cpu]`, o `my_q`) e uma entidade de grupo (`tg->se[cpu]`) que mora na fila do grupo pai.
//! O grupo raiz é o `root_task_group`: a fila dele em cada CPU é a `rq->cfs`. Uma tarefa entra na fila
//! do seu grupo na CPU dela; a entidade do grupo concorre com as irmãs pelo peso `calc_group_shares`
//! (shares do grupo vezes a fração da carga do grupo que está nesta CPU, medida pelo PELT). O pick
//! desce da raiz até uma tarefa; o `update_curr` do tick sobe cobrando cada nível. A hierarquia aceita
//! qualquer profundidade (usuário, sandbox, processo...).
//!
//! # Topologias
//!
//! - [`Topology::PerCpu`]: o modelo do kernel, uma runqueue por CPU, com entidades de grupo por CPU e
//!   balanceamento de carga entre elas ([`crate::balance`]).
//! - [`Topology::Shared`]: uma runqueue única, compartilhada por todas as CPUs. Cada grupo tem uma fila
//!   só, a entidade de grupo tem peso igual aos shares, e cada fila pode ter várias entidades rodando ao
//!   mesmo tempo (uma por CPU). Não existe no kernel; é a alternativa medida no experimento E02 pra uma
//!   VPS de poucas CPUs. Não aceita afinidade restrita.
//!
//! # Relógio
//!
//! Toda operação pública começa lendo o [`Clock`] injetado (`update_rq_clock`). O relógio do
//! escalonador (`sched_clock`, usado pelo PELT e pelos limites de taxa) é esse instante somado a
//! [`SCHED_CLOCK_BASE`], porque o kernel nunca vê o relógio em zero e o PELT usa
//! `last_update_time == 0` como marca de "ainda não anexado".

use rbtree::NodeId;

use crate::balance::{BalanceConfig, CpuBalance, Nohz};
use crate::bandwidth::{BandwidthError, CfsBandwidth};
use crate::clock::Clock;
use crate::features::{CUSTOM_SLICE_MAX_NS, CUSTOM_SLICE_MIN_NS, Features, Tunables};
use crate::pelt::SchedAvg;
use crate::timeline::{CurrView, Timeline};
use crate::weight::{LoadWeight, MAX_NICE, MIN_NICE, NICE_0_LOAD, scale_load_down, shares_from_cgroup_weight};
use crate::{EntityId, GroupId, TaskId};

/// Maior número de CPUs (a afinidade é uma máscara de 64 bits).
pub const MAX_CPUS: usize = 64;

/// Deslocamento do relógio do escalonador sobre o relógio injetado: 2^32 ns (uns 4,3 s de "uptime").
pub const SCHED_CLOCK_BASE: u64 = 1 << 32;

// Flags de `kernel/sched/sched.h`.
pub(crate) const DEQUEUE_SLEEP: u32 = 0x01;
pub(crate) const DEQUEUE_SAVE: u32 = 0x02;
pub(crate) const DEQUEUE_MOVE: u32 = 0x04;
pub(crate) const DEQUEUE_NOCLOCK: u32 = 0x08;
pub(crate) const DEQUEUE_SPECIAL: u32 = 0x10;
pub(crate) const DEQUEUE_DELAYED: u32 = 0x200;

pub(crate) const ENQUEUE_WAKEUP: u32 = 0x01;
pub(crate) const ENQUEUE_RESTORE: u32 = 0x02;
pub(crate) const ENQUEUE_MOVE: u32 = 0x04;
pub(crate) const ENQUEUE_NOCLOCK: u32 = 0x08;
pub(crate) const ENQUEUE_MIGRATED: u32 = 0x40;
pub(crate) const ENQUEUE_INITIAL: u32 = 0x80;
pub(crate) const ENQUEUE_DELAYED: u32 = 0x200;

// `wake_flags`.
pub(crate) const WF_FORK: u32 = 0x04;

// Flags do `update_load_avg`.
pub(crate) const UPDATE_TG: u32 = 0x1;
pub(crate) const SKIP_AGE_LOAD: u32 = 0x2;
pub(crate) const DO_ATTACH: u32 = 0x4;
pub(crate) const DO_DETACH: u32 = 0x8;

/// Organização das runqueues entre as CPUs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topology {
    /// Uma runqueue por CPU, como o kernel.
    PerCpu,
    /// Uma runqueue única pra todas as CPUs.
    Shared,
}

/// Configuração do escalonador.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchedConfig {
    pub cpus: usize,
    pub topology: Topology,
    pub tunables: Tunables,
    pub features: Features,
    pub balance: BalanceConfig,
}

impl SchedConfig {
    /// Uma CPU, sem balanceamento.
    pub fn single_cpu(tunables: Tunables, features: Features) -> SchedConfig {
        SchedConfig { cpus: 1, topology: Topology::PerCpu, tunables, features, balance: BalanceConfig::disabled() }
    }

    /// `cpus` CPUs com a topologia dada e o balanceamento padrão de um domínio desse tamanho.
    pub fn new(cpus: usize, topology: Topology, tunables: Tunables, features: Features) -> SchedConfig {
        let balance = if topology == Topology::PerCpu && cpus > 1 {
            BalanceConfig::mc_domain(cpus)
        } else {
            BalanceConfig::disabled()
        };
        SchedConfig { cpus, topology, tunables, features, balance }
    }
}

/// Índice de uma `cfs_rq`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct CfsId(pub(crate) u32);

/// Uma entidade que está rodando numa CPU, com o instante desde o qual o tempo dela não foi cobrado.
/// No kernel é o `cfs_rq->curr` (com `se->exec_start`); aqui a fila guarda um por CPU, porque no modo
/// de runqueue única uma entidade de grupo pode estar no caminho de várias CPUs ao mesmo tempo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CurrSlot {
    pub cpu: usize,
    pub ent: EntityId,
    pub exec_start: u64,
}

/// `struct cfs_rq`.
#[derive(Clone, Debug)]
pub(crate) struct CfsRq {
    /// CPU dona da fila; `None` no modo de runqueue única.
    pub cpu: Option<usize>,
    pub tg: GroupId,
    /// `tg->se[cpu]`: a entidade de grupo cuja fila é esta (`None` na raiz).
    pub my_se: Option<EntityId>,
    pub timeline: Timeline,
    pub curr: Vec<CurrSlot>,
    pub next: Option<EntityId>,
    pub nr_running: u32,
    pub h_nr_queued: u32,
    pub h_nr_runnable: u32,
    pub h_nr_delayed: u32,
    /// `cfs_rq->load.weight`.
    pub load: u64,
    pub avg: SchedAvg,
    pub tg_load_avg_contrib: u64,
    pub last_update_tg_load_avg: u64,
    // Controle de banda.
    pub runtime_enabled: bool,
    pub runtime_remaining: i64,
    pub throttled: bool,
    pub throttle_count: u32,
    pub throttled_clock: u64,
    pub throttled_clock_pelt: u64,
    pub throttled_clock_pelt_time: u64,
}

impl CfsRq {
    pub(crate) fn new(cpu: Option<usize>, tg: GroupId, my_se: Option<EntityId>) -> CfsRq {
        CfsRq {
            cpu,
            tg,
            my_se,
            timeline: Timeline::new(),
            curr: Vec::with_capacity(1),
            next: None,
            nr_running: 0,
            h_nr_queued: 0,
            h_nr_runnable: 0,
            h_nr_delayed: 0,
            load: 0,
            avg: SchedAvg::default(),
            tg_load_avg_contrib: 0,
            last_update_tg_load_avg: 0,
            runtime_enabled: false,
            runtime_remaining: 0,
            throttled: false,
            throttle_count: 0,
            throttled_clock: 0,
            throttled_clock_pelt: 0,
            throttled_clock_pelt_time: 0,
        }
    }
}

/// Os campos da `task_struct` que a classe usa.
#[derive(Clone, Debug)]
pub(crate) struct TaskData {
    pub nice: i32,
    pub group: GroupId,
    /// `task_cpu(p)`.
    pub cpu: usize,
    /// `p->cpus_ptr` como máscara.
    pub affinity: u64,
    /// `task_on_rq_queued(p)`.
    pub queued: bool,
    /// `TASK_ON_RQ_MIGRATING` durante uma migração.
    pub migrating: bool,
    /// `p->__state != TASK_RUNNING`.
    pub sleeping: bool,
    /// `TASK_NEW`.
    pub new: bool,
    pub nr_wakeups: u64,
    pub nr_switches_in: u64,
    pub nr_migrations: u64,
}

/// O que a entidade é.
#[derive(Clone, Debug)]
pub(crate) enum Kind {
    Task(TaskData),
    /// Entidade de grupo; `my_q` é a fila do grupo nesta CPU.
    Group { my_q: CfsId },
}

/// `struct sched_entity`.
#[derive(Clone, Debug)]
pub(crate) struct Entity {
    pub kind: Kind,
    /// `se->cfs_rq`: a fila onde a entidade é enfileirada.
    pub cfs_rq: CfsId,
    pub parent: Option<EntityId>,
    pub depth: u32,
    pub load: LoadWeight,
    pub node: Option<NodeId>,
    pub deadline: u64,
    pub vruntime: u64,
    /// `se->vlag`; enquanto roda, guarda a deadline do pick (`set_protect_slice`).
    pub vlag: i64,
    pub slice: u64,
    pub on_rq: bool,
    pub sched_delayed: bool,
    pub rel_deadline: bool,
    pub custom_slice: bool,
    pub exec_start: u64,
    pub sum_exec_runtime: u64,
    pub prev_sum_exec_runtime: u64,
    pub avg: SchedAvg,
}

/// `struct task_group`.
#[derive(Clone, Debug)]
pub(crate) struct TaskGroup {
    pub parent: Option<GroupId>,
    pub children: Vec<GroupId>,
    /// `tg->shares` (escalado).
    pub shares: u64,
    pub cgroup_weight: u64,
    /// `tg->cfs_rq[]`: uma fila por CPU (ou uma só no modo de runqueue única).
    pub cfs_rq: Vec<CfsId>,
    /// `tg->se[]` (`None` em todas as posições pro grupo raiz).
    pub se: Vec<Option<EntityId>>,
    /// `tg->load_avg`.
    pub load_avg: i64,
    pub bw: CfsBandwidth,
}

/// `struct rq`, a parte que não é a `cfs_rq`.
#[derive(Clone, Debug)]
pub(crate) struct Cpu {
    /// `rq->cfs` (no modo de runqueue única, a mesma pra todas as CPUs).
    pub root: CfsId,
    /// `rq->curr`; `None` é a tarefa idle.
    pub curr: Option<TaskId>,
    /// `TIF_NEED_RESCHED` do corrente.
    pub need_resched: bool,
    /// `rq->nr_running` (no modo de runqueue única, só a posição 0 é usada).
    pub nr_running: u32,
    pub stats: RqStats,
    pub balance: CpuBalance,
}

/// Contadores de uma CPU.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RqStats {
    pub nr_switches: u64,
    pub nr_ticks: u64,
    pub nr_wakeups: u64,
    /// Quantas vezes um wakeup pediu preempção do corrente.
    pub nr_wakeup_preemptions: u64,
    /// Quantas entidades atrasadas foram tiradas da fila no pick.
    pub nr_delayed_dequeues: u64,
    /// Tarefas puxadas pra esta CPU pelo balanceamento.
    pub nr_pulled: u64,
    /// Rodadas de balanceamento que não moveram nada.
    pub nr_balance_failed: u64,
}

/// O que dá pra ver de uma tarefa de fora.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskInfo {
    pub nice: i32,
    pub group: GroupId,
    pub cpu: usize,
    /// `se->load.weight` (escalado).
    pub weight: u64,
    pub vruntime: u64,
    pub deadline: u64,
    pub vlag: i64,
    pub slice: u64,
    pub custom_slice: bool,
    /// `se->on_rq`.
    pub on_rq: bool,
    pub sched_delayed: bool,
    /// `task_on_rq_queued(p)`.
    pub queued: bool,
    pub sleeping: bool,
    pub running: bool,
    pub sum_exec_runtime: u64,
    pub nr_wakeups: u64,
    pub nr_switches_in: u64,
    pub nr_migrations: u64,
    /// `se->avg.load_avg`.
    pub load_avg: u64,
}

/// O que dá pra ver de um grupo de fora.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupInfo {
    pub parent: Option<GroupId>,
    pub cgroup_weight: u64,
    pub shares: u64,
    /// Peso atual da entidade do grupo em cada CPU (`tg->se[cpu]->load.weight`).
    pub se_weight: Vec<u64>,
    /// `tg->load_avg`.
    pub load_avg: i64,
    /// Filas estranguladas agora, por CPU.
    pub throttled: Vec<bool>,
    pub quota_ns: Option<u64>,
    pub period_ns: u64,
    /// `cpu.stat`: `nr_periods`, `nr_throttled`, `throttled_usec` (aqui em ns).
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub throttled_time_ns: u64,
    /// Runtime ainda no pool do grupo neste período.
    pub pool_runtime_ns: u64,
}

/// Visão das entidades que estão rodando numa fila, sem alocar no caso comum (uma só).
#[derive(Clone, Debug, Default)]
pub(crate) struct Running {
    one: Option<CurrView>,
    many: Vec<CurrView>,
}

impl Running {
    pub(crate) fn as_slice(&self) -> &[CurrView] {
        if self.many.is_empty() { self.one.as_slice() } else { &self.many }
    }

    /// Junta uma visão sem repetir entidade.
    pub(crate) fn push(&mut self, v: CurrView) {
        if self.many.is_empty() && self.one.is_none() {
            self.one = Some(v);
        } else {
            if let Some(o) = self.one.take() {
                self.many.push(o);
            }
            if !self.many.iter().any(|x| x.entity == v.entity) {
                self.many.push(v);
            }
        }
    }
}

/// Modo de bloqueio do `__schedule`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Block {
    /// Preempção ou chamada voluntária com a tarefa ainda pronta.
    None,
    /// `TASK_INTERRUPTIBLE`: pode virar dequeue atrasado.
    Sleep,
    /// `TASK_DEAD`: estado especial, sai da fila na hora.
    Dead,
}

/// O escalonador inteiro: todas as CPUs, filas, entidades e grupos.
#[derive(Debug)]
pub struct Sched<C: Clock> {
    pub(crate) clock: C,
    pub(crate) cfg: SchedConfig,
    /// `sysctl_sched_base_slice`.
    pub(crate) base_slice: u64,
    /// `rq_clock_task` (o mesmo pra todas as CPUs: o relógio é global).
    pub(crate) now: u64,
    pub(crate) cpus: Vec<Cpu>,
    pub(crate) cfs: Vec<CfsRq>,
    pub(crate) ents: Vec<Option<Entity>>,
    pub(crate) free_ents: Vec<u32>,
    pub(crate) groups: Vec<Option<TaskGroup>>,
    pub(crate) nohz: Nohz,
}

impl<C: Clock> Sched<C> {
    /// Escalonador vazio, só com o grupo raiz.
    pub fn new(clock: C, cfg: SchedConfig) -> Sched<C> {
        assert!(cfg.cpus >= 1 && cfg.cpus <= MAX_CPUS, "de 1 a {MAX_CPUS} CPUs");
        let now = clock.now_ns();
        let mut s = Sched {
            clock,
            cfg,
            base_slice: cfg.tunables.base_slice_ns(),
            now,
            cpus: Vec::with_capacity(cfg.cpus),
            cfs: Vec::new(),
            ents: Vec::new(),
            free_ents: Vec::new(),
            groups: Vec::new(),
            nohz: Nohz { has_blocked: false, next_blocked: now },
        };
        let root = GroupId::ROOT;
        let roots: Vec<CfsId> = match cfg.topology {
            Topology::PerCpu => (0..cfg.cpus).map(|c| s.alloc_cfs(CfsRq::new(Some(c), root, None))).collect(),
            Topology::Shared => vec![s.alloc_cfs(CfsRq::new(None, root, None))],
        };
        for c in 0..cfg.cpus {
            let r = if cfg.topology == Topology::Shared { roots[0] } else { roots[c] };
            s.cpus.push(Cpu {
                root: r,
                curr: None,
                need_resched: false,
                nr_running: 0,
                stats: RqStats::default(),
                balance: CpuBalance::new(&cfg.balance, now),
            });
        }
        s.groups.push(Some(TaskGroup {
            parent: None,
            children: Vec::new(),
            shares: NICE_0_LOAD,
            cgroup_weight: 100,
            se: vec![None; roots.len()],
            cfs_rq: roots,
            load_avg: 0,
            bw: CfsBandwidth::new(now),
        }));
        s
    }

    // ---------------------------------------------------------------------------------------------
    // Consultas
    // ---------------------------------------------------------------------------------------------

    /// O relógio.
    pub fn clock(&self) -> &C {
        &self.clock
    }

    /// Configuração.
    pub fn config(&self) -> &SchedConfig {
        &self.cfg
    }

    /// Número de CPUs.
    pub fn nr_cpus(&self) -> usize {
        self.cfg.cpus
    }

    /// `sysctl_sched_base_slice` em ns.
    pub fn base_slice_ns(&self) -> u64 {
        self.base_slice
    }

    /// `rq_clock_task` da última atualização.
    pub fn clock_task(&self) -> u64 {
        self.now
    }

    /// O grupo raiz.
    pub fn root_group(&self) -> GroupId {
        GroupId::ROOT
    }

    /// Tarefa rodando na CPU (`rq->curr`); `None` é idle.
    pub fn current(&self, cpu: usize) -> Option<TaskId> {
        self.cpus[cpu].curr
    }

    /// Diz se o corrente da CPU precisa ceder no próximo ponto de preempção.
    pub fn need_resched(&self, cpu: usize) -> bool {
        self.cpus[cpu].need_resched
    }

    /// Contadores da CPU.
    pub fn stats(&self, cpu: usize) -> RqStats {
        self.cpus[cpu].stats
    }

    /// `rq->nr_running` da CPU (no modo de runqueue única, o total).
    pub fn rq_nr_running(&self, cpu: usize) -> u32 {
        self.cpus[self.rq_slot_of_cpu(cpu)].nr_running
    }

    /// Diz se o id é de uma tarefa viva.
    pub fn contains(&self, t: TaskId) -> bool {
        matches!(self.ents.get(t.index() as usize), Some(Some(Entity { kind: Kind::Task(_), .. })))
    }

    /// Ids das tarefas vivas.
    pub fn task_ids(&self) -> impl Iterator<Item = TaskId> + '_ {
        self.ents.iter().enumerate().filter_map(|(i, e)| match e {
            Some(Entity { kind: Kind::Task(_), .. }) => Some(TaskId::from_index(i as u32)),
            _ => None,
        })
    }

    /// Ids dos grupos vivos (o raiz incluso).
    pub fn group_ids(&self) -> impl Iterator<Item = GroupId> + '_ {
        self.groups.iter().enumerate().filter(|(_, g)| g.is_some()).map(|(i, _)| GroupId::from_index(i as u32))
    }

    /// Estado de uma tarefa.
    pub fn task(&self, t: TaskId) -> TaskInfo {
        let e = self.e(t.entity());
        let d = self.td(t);
        TaskInfo {
            nice: d.nice,
            group: d.group,
            cpu: d.cpu,
            weight: e.load.weight,
            vruntime: e.vruntime,
            deadline: e.deadline,
            vlag: e.vlag,
            slice: e.slice,
            custom_slice: e.custom_slice,
            on_rq: e.on_rq,
            sched_delayed: e.sched_delayed,
            queued: d.queued,
            sleeping: d.sleeping,
            running: self.cpus.iter().any(|c| c.curr == Some(t)),
            sum_exec_runtime: e.sum_exec_runtime,
            nr_wakeups: d.nr_wakeups,
            nr_switches_in: d.nr_switches_in,
            nr_migrations: d.nr_migrations,
            load_avg: e.avg.load_avg,
        }
    }

    /// Estado de um grupo.
    pub fn group(&self, g: GroupId) -> GroupInfo {
        let tg = self.g(g);
        GroupInfo {
            parent: tg.parent,
            cgroup_weight: tg.cgroup_weight,
            shares: tg.shares,
            se_weight: tg.se.iter().map(|s| s.map(|s| self.e(s).load.weight).unwrap_or(0)).collect(),
            load_avg: tg.load_avg,
            throttled: tg.cfs_rq.iter().map(|&c| self.c(c).throttled).collect(),
            quota_ns: tg.bw.quota,
            period_ns: tg.bw.period,
            nr_periods: tg.bw.nr_periods,
            nr_throttled: tg.bw.nr_throttled,
            throttled_time_ns: tg.bw.throttled_time,
            pool_runtime_ns: tg.bw.runtime,
        }
    }

    /// Tempo de CPU da tarefa, contando o trecho em andamento até o relógio atual (como o
    /// `task_sched_runtime` que alimenta o `/proc/<pid>/schedstat`).
    pub fn task_runtime_now(&self, t: TaskId) -> u64 {
        let e = self.e(t.entity());
        let now = self.clock.now_ns().max(self.now);
        for c in &self.cfs[e.cfs_rq.0 as usize].curr {
            if c.ent == t.entity() {
                let delta = now.wrapping_sub(c.exec_start) as i64;
                if delta > 0 {
                    return e.sum_exec_runtime + delta as u64;
                }
            }
        }
        e.sum_exec_runtime
    }

    /// Tempo de CPU somado das tarefas de um grupo e dos subgrupos, até o relógio atual.
    pub fn group_runtime_now(&self, g: GroupId) -> u64 {
        self.group_tasks(g).map(|t| self.task_runtime_now(t)).sum()
    }

    /// Tempo de CPU já contabilizado das tarefas de um grupo e dos subgrupos: o `usage_usec` do
    /// `cpu.stat` (em ns), que no kernel só anda quando o `update_curr` cobra o corrente (no tick, numa
    /// tarefa CPU-bound).
    pub fn group_runtime_accounted(&self, g: GroupId) -> u64 {
        self.group_tasks(g).map(|t| self.e(t.entity()).sum_exec_runtime).sum()
    }

    /// Tarefas do grupo e dos subgrupos.
    fn group_tasks(&self, g: GroupId) -> impl Iterator<Item = TaskId> + '_ {
        self.task_ids().filter(move |&t| {
            let mut grp = Some(self.td(t).group);
            while let Some(x) = grp {
                if x == g {
                    return true;
                }
                grp = self.g(x).parent;
            }
            false
        })
    }

    // ---------------------------------------------------------------------------------------------
    // Grupos (cgroup v2: cpu.weight e cpu.max)
    // ---------------------------------------------------------------------------------------------

    /// `sched_create_group` + `online_fair_sched_group`: grupo novo dentro de `parent`, com peso padrão
    /// (`cpu.weight` 100) e sem limite de banda.
    pub fn create_group(&mut self, parent: GroupId) -> GroupId {
        self.update_rq_clock();
        let gid = GroupId::from_index(self.groups.len() as u32);
        let parent_cfs = self.g(parent).cfs_rq.clone();
        let parent_se = self.g(parent).se.clone();
        let n = parent_cfs.len();
        let mut cfs_ids = Vec::with_capacity(n);
        let mut se_ids = Vec::with_capacity(n);
        let parent_bw_quota = self.g(parent).bw.hierarchical_quota;
        for i in 0..n {
            let cpu = self.c(parent_cfs[i]).cpu;
            let my_q = self.alloc_cfs(CfsRq::new(cpu, gid, None));
            let depth = parent_se[i].map(|p| self.e(p).depth + 1).unwrap_or(0);
            let se = self.alloc_ent(Entity {
                kind: Kind::Group { my_q },
                cfs_rq: parent_cfs[i],
                parent: parent_se[i],
                depth,
                load: LoadWeight::with_weight(NICE_0_LOAD),
                node: None,
                deadline: 0,
                vruntime: 0,
                vlag: 0,
                slice: self.base_slice,
                on_rq: false,
                sched_delayed: false,
                rel_deadline: false,
                custom_slice: false,
                exec_start: 0,
                sum_exec_runtime: 0,
                prev_sum_exec_runtime: 0,
                avg: SchedAvg::default(),
            });
            self.c_mut(my_q).my_se = Some(se);
            cfs_ids.push(my_q);
            se_ids.push(Some(se));
        }
        let mut bw = CfsBandwidth::new(self.now);
        bw.hierarchical_quota = parent_bw_quota;
        self.groups.push(Some(TaskGroup {
            parent: Some(parent),
            children: Vec::new(),
            shares: NICE_0_LOAD,
            cgroup_weight: 100,
            cfs_rq: cfs_ids.clone(),
            se: se_ids.clone(),
            load_avg: 0,
            bw,
        }));
        self.g_mut(parent).children.push(gid);
        // online_fair_sched_group: anexa a entidade de grupo (carga zero) e herda o estado de
        // estrangulamento do pai (sync_throttle).
        for (i, se) in se_ids.iter().enumerate() {
            self.attach_entity_cfs_rq(se.expect("entidade de grupo"));
            self.sync_throttle(gid, i);
        }
        gid
    }

    /// `cpu.weight` do grupo (1 a 10000; 100 = 1024 de shares), como `cpu_weight_write_u64` +
    /// `sched_group_set_shares`.
    pub fn set_group_weight(&mut self, g: GroupId, cgroup_weight: u64) {
        assert!(g != GroupId::ROOT, "o peso do grupo raiz não muda");
        self.update_rq_clock();
        let shares = shares_from_cgroup_weight(cgroup_weight);
        self.g_mut(g).cgroup_weight = cgroup_weight;
        if self.g(g).shares == shares {
            return;
        }
        self.g_mut(g).shares = shares;
        let ses = self.g(g).se.clone();
        for se in ses.into_iter().flatten() {
            // Propaga pela hierarquia (for_each_sched_entity).
            let mut s = Some(se);
            while let Some(x) = s {
                let cfs = self.e(x).cfs_rq;
                self.update_load_avg(cfs, x, UPDATE_TG);
                self.update_cfs_group(x);
                s = self.e(x).parent;
            }
        }
    }

    /// `cpu.max`: quota e período em ns (`None` = sem limite), como `tg_set_cfs_bandwidth`.
    pub fn set_group_bandwidth(&mut self, g: GroupId, quota_ns: Option<u64>, period_ns: u64) -> Result<(), BandwidthError> {
        self.update_rq_clock();
        self.tg_set_cfs_bandwidth(g, quota_ns, period_ns, 0)
    }

    /// Fase do timer de período do grupo: os períodos começam em `offset + k * período`. O kernel
    /// sorteia esse deslocamento na criação do grupo (`init_cfs_bandwidth`).
    pub fn set_group_period_offset(&mut self, g: GroupId, offset_ns: u64) {
        let bw = &mut self.g_mut(g).bw;
        bw.timer_expires = offset_ns % bw.period.max(1);
    }

    // ---------------------------------------------------------------------------------------------
    // Tarefas (o lado core.c)
    // ---------------------------------------------------------------------------------------------

    /// `sched_fork`: cria a tarefa em `TASK_NEW` no grupo dado, na CPU dada, fora da fila, com vruntime
    /// e lag zerados, fatia base e PELT com carga cheia (`init_entity_runnable_average`).
    pub fn create_task(&mut self, nice: i32, group: GroupId, cpu: usize) -> TaskId {
        assert!((MIN_NICE..=MAX_NICE).contains(&nice), "nice {nice} fora de [-20, 19]");
        assert!(cpu < self.cfg.cpus, "CPU {cpu} não existe");
        let lw = LoadWeight::from_nice(nice);
        let slot = self.group_slot(cpu);
        let cfs_rq = self.g(group).cfs_rq[slot];
        let parent = self.g(group).se[slot];
        let depth = parent.map(|p| self.e(p).depth + 1).unwrap_or(0);
        let all = if self.cfg.cpus == 64 { u64::MAX } else { (1u64 << self.cfg.cpus) - 1 };
        let id = self.alloc_ent(Entity {
            kind: Kind::Task(TaskData {
                nice,
                group,
                cpu,
                affinity: all,
                queued: false,
                migrating: false,
                sleeping: false,
                new: true,
                nr_wakeups: 0,
                nr_switches_in: 0,
                nr_migrations: 0,
            }),
            cfs_rq,
            parent,
            depth,
            load: lw,
            node: None,
            deadline: 0,
            vruntime: 0,
            vlag: 0,
            slice: self.base_slice,
            on_rq: false,
            sched_delayed: false,
            rel_deadline: false,
            custom_slice: false,
            exec_start: 0,
            sum_exec_runtime: 0,
            prev_sum_exec_runtime: 0,
            avg: SchedAvg { load_avg: scale_load_down(lw.weight), ..SchedAvg::default() },
        });
        TaskId::from_index(id.index())
    }

    /// `sched_setaffinity` antes de a tarefa entrar na fila (o balanceamento respeita a máscara).
    pub fn set_affinity(&mut self, t: TaskId, mask: u64) {
        assert!(self.cfg.topology == Topology::PerCpu || mask.count_ones() as usize >= self.cfg.cpus, "a runqueue única não aceita afinidade restrita");
        let cpu = self.td(t).cpu;
        assert!(mask & (1 << cpu) != 0, "a máscara precisa incluir a CPU atual da tarefa");
        self.td_mut(t).affinity = mask;
    }

    /// `wake_up_new_task`: primeira entrada na fila, com meia fatia (PLACE_DEADLINE_INITIAL), seguida
    /// do teste de preempção com `WF_FORK`.
    pub fn wake_up_new_task(&mut self, p: TaskId) {
        assert!(self.td(p).new, "{p:?} já foi acordada");
        self.update_rq_clock();
        let d = self.td_mut(p);
        d.new = false;
        d.sleeping = false;
        self.activate_task(p, ENQUEUE_NOCLOCK | ENQUEUE_INITIAL);
        self.wakeup_preempt(p, WF_FORK);
    }

    /// `try_to_wake_up`: acorda uma tarefa que dorme, na CPU onde ela estava (sem balanceamento no
    /// wakeup). Se ela ainda está na fila (dequeue atrasado), segue o `ttwu_runnable`
    /// (`ENQUEUE_DELAYED`); senão, `ttwu_do_activate` com `ENQUEUE_WAKEUP`. Em ambos os casos testa se
    /// ela preempta. Devolve `false` se a tarefa não dormia.
    pub fn try_to_wake_up(&mut self, p: TaskId) -> bool {
        if !self.td(p).sleeping {
            return false;
        }
        self.update_rq_clock();
        let d = self.td_mut(p);
        d.sleeping = false;
        d.nr_wakeups += 1;
        let cpu = d.cpu;
        self.cpus[cpu].stats.nr_wakeups += 1;
        if self.td(p).queued {
            if self.e(p.entity()).sched_delayed {
                self.enqueue_task_fair(p, ENQUEUE_NOCLOCK | ENQUEUE_DELAYED);
            }
            if !self.task_on_cpu(p) {
                self.wakeup_preempt(p, 0);
            }
        } else {
            self.activate_task(p, ENQUEUE_WAKEUP | ENQUEUE_NOCLOCK);
            self.wakeup_preempt(p, 0);
        }
        true
    }

    /// `__schedule` na CPU. Se `prev_blocks`, o corrente vai dormir (`TASK_INTERRUPTIBLE`) e pode ficar
    /// na fila como atrasado (DELAY_DEQUEUE); senão é preempção ou cessão voluntária. Escolhe o
    /// próximo, faz a troca e limpa o pedido de reescalonamento. Devolve quem roda agora.
    pub fn schedule(&mut self, cpu: usize, prev_blocks: bool) -> Option<TaskId> {
        self.do_schedule(cpu, if prev_blocks { Block::Sleep } else { Block::None })
    }

    /// `do_task_dead`: o corrente da CPU termina e a posição dele é liberada. Devolve quem roda agora.
    pub fn exit_current(&mut self, cpu: usize) -> Option<TaskId> {
        let prev = self.cpus[cpu].curr.expect("exit_current sem tarefa rodando");
        let next = self.do_schedule(cpu, Block::Dead);
        self.ents[prev.index() as usize] = None;
        self.free_ents.push(prev.index());
        next
    }

    /// `sched_tick` + `task_tick_fair` na CPU, seguido do balanceamento periódico se for a hora
    /// (`sched_balance_trigger`).
    pub fn tick(&mut self, cpu: usize) {
        self.update_rq_clock();
        self.cpus[cpu].stats.nr_ticks += 1;
        if let Some(curr) = self.cpus[cpu].curr {
            self.task_tick_fair(curr);
        }
        if self.cfg.balance.enabled {
            self.periodic_balance(cpu);
        }
    }

    /// `set_user_nice`: tira da fila (`DEQUEUE_SAVE`), troca o peso escalando o lag
    /// (`reweight_entity`), devolve à fila (`ENQUEUE_RESTORE`, com a deadline relativa preservada) e
    /// testa preempção (`prio_changed_fair`).
    pub fn set_user_nice(&mut self, p: TaskId, nice: i32) {
        if self.td(p).nice == nice || !(MIN_NICE..=MAX_NICE).contains(&nice) {
            return;
        }
        self.update_rq_clock();
        let queued = self.td(p).queued;
        let running_cpu = self.running_cpu(p);
        if queued {
            self.dequeue_task_fair(p, DEQUEUE_SAVE | DEQUEUE_NOCLOCK);
        }
        if let Some(cpu) = running_cpu {
            self.put_prev_task_fair(cpu, p);
        }
        let old_nice = self.td(p).nice;
        self.td_mut(p).nice = nice;
        self.reweight_task_fair(p, LoadWeight::from_nice(nice));
        if queued {
            self.enqueue_task_fair(p, ENQUEUE_RESTORE | ENQUEUE_NOCLOCK);
        }
        if let Some(cpu) = running_cpu {
            self.set_next_task_fair(cpu, p);
        }
        self.prio_changed_fair(p, old_nice);
    }

    /// `sched_setattr` com `sched_runtime`: fatia própria entre 0,1 ms e 100 ms (`Some`), ou volta à
    /// fatia base (`None`). Segue o `__sched_setscheduler`: tira da fila, troca os parâmetros
    /// (`__setscheduler_params`, que também refaz o peso), devolve à fila. Como a prioridade não muda,
    /// não há `prio_changed`.
    pub fn set_custom_slice(&mut self, p: TaskId, runtime_ns: Option<u64>) {
        self.update_rq_clock();
        let queued = self.td(p).queued;
        let running_cpu = self.running_cpu(p);
        if queued {
            self.dequeue_task_fair(p, DEQUEUE_SAVE | DEQUEUE_MOVE | DEQUEUE_NOCLOCK);
        }
        if let Some(cpu) = running_cpu {
            self.put_prev_task_fair(cpu, p);
        }
        let base = self.base_slice;
        let e = self.e_mut(p.entity());
        match runtime_ns {
            Some(r) => {
                e.custom_slice = true;
                e.slice = r.clamp(CUSTOM_SLICE_MIN_NS, CUSTOM_SLICE_MAX_NS);
            }
            None => {
                e.custom_slice = false;
                e.slice = base;
            }
        }
        let lw = LoadWeight::from_nice(self.td(p).nice);
        self.reweight_task_fair(p, lw);
        if queued {
            self.enqueue_task_fair(p, ENQUEUE_RESTORE | ENQUEUE_MOVE | ENQUEUE_NOCLOCK);
        }
        if let Some(cpu) = running_cpu {
            self.set_next_task_fair(cpu, p);
        }
    }

    /// `sched_yield` na CPU: `yield_task_fair` (se elegível, o corrente pula pra própria deadline e
    /// ganha uma nova) seguido de `schedule()`.
    pub fn yield_current(&mut self, cpu: usize) -> Option<TaskId> {
        self.update_rq_clock();
        if let Some(c) = self.cpus[cpu].curr
            && self.rq_nr_running(cpu) != 1
        {
            let se = c.entity();
            let cfs = self.e(se).cfs_rq;
            self.clear_buddies(cfs, se);
            self.update_curr(cfs);
            if self.entity_eligible(cfs, se) {
                let e = self.e_mut(se);
                e.vruntime = e.deadline;
                self.update_deadline(cfs, se);
            }
        }
        self.do_schedule(cpu, Block::None)
    }

    /// Instante (no relógio injetado) do próximo timer de banda a vencer: fim de período ou slack.
    pub fn next_timer_ns(&self) -> Option<u64> {
        let mut best: Option<u64> = None;
        for g in self.groups.iter().flatten() {
            if g.bw.period_active {
                best = Some(best.map_or(g.bw.timer_expires, |b| b.min(g.bw.timer_expires)));
            }
            if let Some(s) = g.bw.slack_expires {
                best = Some(best.map_or(s, |b| b.min(s)));
            }
        }
        best
    }

    /// Roda os timers de banda vencidos até o relógio atual (`sched_cfs_period_timer` e
    /// `sched_cfs_slack_timer`). Quem dirige o escalonador chama isto quando o relógio passa de
    /// [`Sched::next_timer_ns`]; depois, deve reescalonar as CPUs com pedido pendente.
    pub fn run_timers(&mut self) {
        self.update_rq_clock();
        let ids: Vec<GroupId> = self.group_ids().collect();
        for g in ids {
            if self.g(g).bw.slack_expires.is_some_and(|s| s <= self.now) {
                self.g_mut(g).bw.slack_expires = None;
                self.do_sched_cfs_slack_timer(g);
            }
            if self.g(g).bw.period_active && self.g(g).bw.timer_expires <= self.now {
                self.sched_cfs_period_timer(g);
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // core.c por dentro
    // ---------------------------------------------------------------------------------------------

    /// `update_rq_clock`: o relógio só anda pra frente.
    pub(crate) fn update_rq_clock(&mut self) {
        let now = self.clock.now_ns();
        if now > self.now {
            self.now = now;
        }
    }

    /// `sched_clock_cpu`: o relógio do escalonador (PELT, limites de taxa).
    pub(crate) fn sched_clock(&self) -> u64 {
        self.now + SCHED_CLOCK_BASE
    }

    pub(crate) fn shared(&self) -> bool {
        self.cfg.topology == Topology::Shared
    }

    /// Posição nos vetores por CPU de um grupo (`tg->cfs_rq[cpu]`).
    pub(crate) fn group_slot(&self, cpu: usize) -> usize {
        if self.shared() { 0 } else { cpu }
    }

    /// Posição do contador `rq->nr_running` usado por uma CPU.
    pub(crate) fn rq_slot_of_cpu(&self, cpu: usize) -> usize {
        if self.shared() { 0 } else { cpu }
    }

    /// Posição do `rq->nr_running` da fila.
    pub(crate) fn rq_slot_of_cfs(&self, cfs: CfsId) -> usize {
        self.c(cfs).cpu.unwrap_or(0)
    }

    pub(crate) fn resched_cpu(&mut self, cpu: usize) {
        self.cpus[cpu].need_resched = true;
    }

    /// CPU em que a tarefa está rodando, se estiver.
    pub(crate) fn running_cpu(&self, p: TaskId) -> Option<usize> {
        self.cpus.iter().position(|c| c.curr == Some(p))
    }

    /// `task_on_cpu`.
    pub(crate) fn task_on_cpu(&self, p: TaskId) -> bool {
        self.running_cpu(p).is_some()
    }

    fn alloc_cfs(&mut self, c: CfsRq) -> CfsId {
        let id = CfsId(u32::try_from(self.cfs.len()).expect("filas demais"));
        self.cfs.push(c);
        id
    }

    fn alloc_ent(&mut self, e: Entity) -> EntityId {
        match self.free_ents.pop() {
            Some(i) => {
                self.ents[i as usize] = Some(e);
                EntityId::from_index(i)
            }
            None => {
                let i = u32::try_from(self.ents.len()).expect("entidades demais");
                self.ents.push(Some(e));
                EntityId::from_index(i)
            }
        }
    }

    pub(crate) fn e(&self, id: EntityId) -> &Entity {
        match self.ents.get(id.index() as usize) {
            Some(Some(e)) => e,
            _ => panic!("{id:?} não existe"),
        }
    }

    pub(crate) fn e_mut(&mut self, id: EntityId) -> &mut Entity {
        match self.ents.get_mut(id.index() as usize) {
            Some(Some(e)) => e,
            _ => panic!("{id:?} não existe"),
        }
    }

    pub(crate) fn c(&self, id: CfsId) -> &CfsRq {
        &self.cfs[id.0 as usize]
    }

    pub(crate) fn c_mut(&mut self, id: CfsId) -> &mut CfsRq {
        &mut self.cfs[id.0 as usize]
    }

    pub(crate) fn g(&self, id: GroupId) -> &TaskGroup {
        match self.groups.get(id.index() as usize) {
            Some(Some(g)) => g,
            _ => panic!("{id:?} não existe"),
        }
    }

    pub(crate) fn g_mut(&mut self, id: GroupId) -> &mut TaskGroup {
        match self.groups.get_mut(id.index() as usize) {
            Some(Some(g)) => g,
            _ => panic!("{id:?} não existe"),
        }
    }

    pub(crate) fn td(&self, t: TaskId) -> &TaskData {
        match &self.e(t.entity()).kind {
            Kind::Task(d) => d,
            Kind::Group { .. } => panic!("{t:?} não é tarefa"),
        }
    }

    pub(crate) fn td_mut(&mut self, t: TaskId) -> &mut TaskData {
        match &mut self.e_mut(t.entity()).kind {
            Kind::Task(d) => d,
            Kind::Group { .. } => panic!("{t:?} não é tarefa"),
        }
    }

    pub(crate) fn do_schedule(&mut self, cpu: usize, block: Block) -> Option<TaskId> {
        self.update_rq_clock();
        let prev = self.cpus[cpu].curr;
        if let Some(p) = prev {
            match block {
                Block::None => {}
                Block::Sleep => {
                    self.td_mut(p).sleeping = true;
                    self.block_task(p, DEQUEUE_NOCLOCK);
                }
                Block::Dead => {
                    self.td_mut(p).sleeping = true;
                    self.block_task(p, DEQUEUE_NOCLOCK | DEQUEUE_SPECIAL);
                }
            }
        }
        let next = self.pick_next_task_fair(cpu, prev);
        self.cpus[cpu].need_resched = false;
        if next != prev {
            self.cpus[cpu].stats.nr_switches += 1;
            self.cpus[cpu].curr = next;
            if let Some(n) = next {
                let d = self.td_mut(n);
                d.nr_switches_in += 1;
                d.cpu = cpu;
            }
            if next.is_none() {
                // A CPU vai pra idle e para o tick (NO_HZ).
                self.nohz_balance_enter_idle(cpu);
            }
        }
        next
    }

    /// `block_task`: se o dequeue terminou (não ficou atrasado), a tarefa sai da runqueue.
    fn block_task(&mut self, p: TaskId, flags: u32) {
        if self.dequeue_task_fair(p, DEQUEUE_SLEEP | flags) {
            self.finish_block_task(p);
        }
    }

    /// `__block_task`: `p->on_rq = 0`.
    pub(crate) fn finish_block_task(&mut self, p: TaskId) {
        self.td_mut(p).queued = false;
    }

    /// `activate_task`: enfileira e marca `p->on_rq = TASK_ON_RQ_QUEUED`.
    pub(crate) fn activate_task(&mut self, p: TaskId, mut flags: u32) {
        if self.td(p).migrating {
            flags |= ENQUEUE_MIGRATED;
        }
        self.enqueue_task_fair(p, flags);
        let d = self.td_mut(p);
        d.queued = true;
        d.migrating = false;
    }

    /// `deactivate_task` + `set_task_cpu` + `activate_task` + `wakeup_preempt`: migra uma tarefa pronta
    /// (não rodando) pra outra CPU, como o `detach_task`/`attach_task` do balanceamento.
    pub(crate) fn move_queued_task(&mut self, p: TaskId, dst: usize) {
        debug_assert!(!self.shared());
        debug_assert!(self.td(p).queued && !self.task_on_cpu(p));
        self.td_mut(p).migrating = true;
        self.dequeue_task_fair(p, DEQUEUE_NOCLOCK);
        // set_task_cpu: migrate_task_rq_fair (avg.last_update_time = 0) e set_task_rq.
        let group = self.td(p).group;
        let se = p.entity();
        let cfs_rq = self.g(group).cfs_rq[dst];
        let parent = self.g(group).se[dst];
        let depth = parent.map(|x| self.e(x).depth + 1).unwrap_or(0);
        let e = self.e_mut(se);
        e.avg.last_update_time = 0;
        e.cfs_rq = cfs_rq;
        e.parent = parent;
        e.depth = depth;
        let d = self.td_mut(p);
        d.cpu = dst;
        d.nr_migrations += 1;
        self.activate_task(p, ENQUEUE_NOCLOCK);
        self.wakeup_preempt(p, 0);
    }

    /// `wakeup_preempt`. Com idle rodando, qualquer tarefa CFS preempta; senão a classe decide. No modo
    /// de runqueue única, a tarefa pode ir pra qualquer CPU: uma CPU ociosa é acordada se houver; senão
    /// cada CPU passa pelo teste até uma ceder.
    pub(crate) fn wakeup_preempt(&mut self, p: TaskId, wake_flags: u32) {
        if self.shared() {
            if let Some(idle) = (0..self.cfg.cpus).find(|&c| self.cpus[c].curr.is_none() && !self.cpus[c].need_resched) {
                self.resched_cpu(idle);
                return;
            }
            if (0..self.cfg.cpus).any(|c| self.cpus[c].curr.is_none()) {
                return;
            }
            for cpu in 0..self.cfg.cpus {
                let curr = self.cpus[cpu].curr.expect("todas ocupadas");
                let before = self.cpus[cpu].need_resched;
                self.check_preempt_wakeup_fair(cpu, curr, p, wake_flags);
                if !before && self.cpus[cpu].need_resched {
                    return;
                }
            }
            return;
        }
        let cpu = self.td(p).cpu;
        match self.cpus[cpu].curr {
            None => self.resched_cpu(cpu),
            Some(c) => self.check_preempt_wakeup_fair(cpu, c, p, wake_flags),
        }
    }

    /// `prio_changed_fair`.
    fn prio_changed_fair(&mut self, p: TaskId, old_nice: i32) {
        if !self.td(p).queued {
            return;
        }
        let root = self.cpus[self.td(p).cpu].root;
        if self.c(root).nr_running == 1 {
            return;
        }
        if let Some(cpu) = self.running_cpu(p) {
            if self.td(p).nice > old_nice {
                self.resched_cpu(cpu);
            }
        } else {
            self.wakeup_preempt(p, 0);
        }
    }

    /// `reweight_task_fair`: `reweight_entity` e depois o inverso da tabela.
    fn reweight_task_fair(&mut self, p: TaskId, lw: LoadWeight) {
        let se = p.entity();
        let cfs = self.e(se).cfs_rq;
        self.reweight_entity(cfs, se, lw.weight);
        self.e_mut(se).load.inv_weight = lw.inv_weight;
    }
}
