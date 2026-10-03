//! Simulador de eventos discretos usando o mesmo [`Sched`] do sandbox sobre um [`ManualClock`].
//!
//! Modela o que o kernel faz com tarefas de usuário:
//!
//! - **Tick** periódico (`TICK_NSEC`) numa grade fixa com fase configurável, igual pra todas as CPUs
//!   (o padrão do kernel, sem `sched_skew_tick`), só nas CPUs ocupadas (NO_HZ). No tick roda
//!   [`Sched::tick`] (que também dispara o balanceamento periódico) e, se o corrente precisa ceder,
//!   [`Sched::schedule`] (é o retorno da interrupção pro modo usuário).
//! - **Timers de sono** como hrtimers com folga (`timer_slack_ns`): o timer dispara no vencimento
//!   duro (pedido + folga), ou antes, se um tick da CPU da tarefa cair entre o pedido e o vencimento
//!   duro. O wakeup chama [`Sched::try_to_wake_up`] e, se houve pedido de preempção, troca na hora.
//! - **Timers de banda** (fim de período e slack do `cpu.max`), dirigidos por
//!   [`Sched::next_timer_ns`] e [`Sched::run_timers`].
//! - **Rajadas de CPU**: uma tarefa periódica roda `run_ns` de CPU e volta a dormir; quando a rajada
//!   acaba, ela chama [`Sched::schedule`] bloqueando (o `nanosleep`).
//! - **Grupos** aninhados com `cpu.weight` e `cpu.max`, e **afinidade** por máscara.
//! - **Criação** de tarefa na CPU permitida com menos tarefas (aproxima o `find_idlest_cpu` do fork);
//!   o wakeup volta pra CPU de antes.
//!
//! A troca de contexto não custa tempo aqui. A latência de wakeup medida é o tempo entre o instante
//! pedido pro despertar (o vencimento suave) e o instante em que a tarefa volta a rodar, então inclui
//! a folga do timer, como uma medição feita em espaço de usuário no host.

use crate::clock::{Clock, ManualClock};
use crate::features::{Features, Tunables};
use crate::sched::{RqStats, Sched, SchedConfig, Topology};
use crate::{GroupId, TaskId};

/// Comportamento de uma tarefa simulada.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    /// Sempre pronta pra rodar (laço de CPU).
    CpuBound,
    /// Laço de rajada e sono: roda `run_ns` de CPU, dorme `sleep_ns` num timer com folga
    /// `timer_slack_ns` e repete.
    Periodic { run_ns: u64, sleep_ns: u64, timer_slack_ns: u64 },
}

/// Um grupo do cenário (um cgroup com o controlador `cpu`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimGroup {
    pub name: String,
    /// Índice do grupo pai no cenário; `None` é filho do raiz.
    pub parent: Option<usize>,
    /// `cpu.weight` (100 é o padrão).
    pub weight: u64,
    /// `cpu.max`: quota em ns (`None` = sem limite).
    pub quota_ns: Option<u64>,
    pub period_ns: u64,
    /// Fase do timer de período.
    pub period_offset_ns: u64,
}

impl SimGroup {
    /// Grupo com `cpu.weight` e sem limite de banda.
    pub fn new(name: &str, parent: Option<usize>, weight: u64) -> SimGroup {
        SimGroup { name: name.to_string(), parent, weight, quota_ns: None, period_ns: 100_000_000, period_offset_ns: 0 }
    }

    /// Mesmo grupo com `cpu.max`.
    pub fn with_quota(mut self, quota_ns: u64, period_ns: u64, offset_ns: u64) -> SimGroup {
        self.quota_ns = Some(quota_ns);
        self.period_ns = period_ns;
        self.period_offset_ns = offset_ns;
        self
    }
}

/// Uma tarefa do cenário.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimTask {
    pub name: String,
    pub nice: i32,
    pub workload: Workload,
    /// Instante em que a tarefa é criada e acordada (`wake_up_new_task`).
    pub start_ns: u64,
    /// Fatia própria (`sched_attr.sched_runtime`), se houver.
    pub custom_slice_ns: Option<u64>,
    /// Índice do grupo no cenário; `None` é o raiz.
    pub group: Option<usize>,
    /// Máscara de CPUs permitidas; `None` é todas.
    pub cpus: Option<u64>,
}

impl SimTask {
    /// Tarefa CPU-bound criada em t = 0 no grupo raiz.
    pub fn cpu_bound(name: &str, nice: i32) -> SimTask {
        SimTask {
            name: name.to_string(),
            nice,
            workload: Workload::CpuBound,
            start_ns: 0,
            custom_slice_ns: None,
            group: None,
            cpus: None,
        }
    }

    /// Tarefa periódica criada em t = 0 no grupo raiz.
    pub fn periodic(name: &str, nice: i32, run_ns: u64, sleep_ns: u64, timer_slack_ns: u64) -> SimTask {
        SimTask { workload: Workload::Periodic { run_ns, sleep_ns, timer_slack_ns }, ..SimTask::cpu_bound(name, nice) }
    }

    /// A mesma tarefa no grupo dado.
    pub fn in_group(mut self, group: usize) -> SimTask {
        self.group = Some(group);
        self
    }
}

/// Parâmetros da simulação.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimConfig {
    pub sched: SchedConfig,
    /// Fim da simulação.
    pub duration_ns: u64,
    /// A grade de ticks é `tick_phase_ns + k * TICK_NSEC`.
    pub tick_phase_ns: u64,
    /// Início da janela de medição (CPU e latências contam a partir daqui).
    pub measure_from_ns: u64,
    /// Se `Some(p)`, amostra o tempo de CPU de cada grupo a cada `p` ns.
    pub sample_every_ns: Option<u64>,
}

impl SimConfig {
    /// Uma CPU, sem grupos, sem amostragem.
    pub fn single_cpu(tunables: Tunables, features: Features, duration_ns: u64, tick_phase_ns: u64, measure_from_ns: u64) -> SimConfig {
        SimConfig {
            sched: SchedConfig::single_cpu(tunables, features),
            duration_ns,
            tick_phase_ns,
            measure_from_ns,
            sample_every_ns: None,
        }
    }
}

/// Resultado de uma tarefa.
#[derive(Clone, Debug, PartialEq)]
pub struct SimTaskReport {
    pub name: String,
    pub nice: i32,
    pub group: Option<usize>,
    /// CPU recebida dentro da janela de medição.
    pub cpu_ns: u64,
    /// Fração da CPU total consumida pelas tarefas na janela.
    pub share: f64,
    /// Latências de wakeup na janela, em ordem de ocorrência.
    pub latencies_ns: Vec<u64>,
    pub wakeups: u64,
    pub migrations: u64,
}

/// Resultado de um grupo (inclui os subgrupos).
#[derive(Clone, Debug, PartialEq)]
pub struct SimGroupReport {
    pub name: String,
    pub cpu_ns: u64,
    /// Fração da CPU total consumida pelas tarefas na janela.
    pub share: f64,
    /// Deltas de `cpu.stat` na janela.
    pub nr_periods: u64,
    pub nr_throttled: u64,
    pub throttled_time_ns: u64,
}

/// Resultado da simulação.
#[derive(Clone, Debug, PartialEq)]
pub struct SimReport {
    pub window_ns: u64,
    pub tasks: Vec<SimTaskReport>,
    pub groups: Vec<SimGroupReport>,
    /// Contadores por CPU.
    pub stats: Vec<RqStats>,
    /// Amostras `(instante, tempo de CPU já contabilizado de cada grupo)`, se pedidas. Como o
    /// `usage_usec` do `cpu.stat`, o valor só anda quando o `update_curr` cobra quem roda.
    pub samples: Vec<(u64, Vec<u64>)>,
}

#[derive(Clone, Debug)]
struct TaskState {
    spec: SimTask,
    id: Option<TaskId>,
    remaining_run_ns: u64,
    timer_soft: Option<u64>,
    timer_hard: Option<u64>,
    /// Instante pedido do último despertar, até a tarefa voltar a rodar.
    wake_request: Option<u64>,
    latencies_ns: Vec<u64>,
    cpu_at_window_start: Option<u64>,
    migrations_at_window_start: u64,
}

/// Simulador passo a passo. [`simulate`] é o atalho pra rodar até o fim.
#[derive(Debug)]
pub struct Simulator {
    cfg: SimConfig,
    clock: ManualClock,
    sched: Sched<ManualClock>,
    groups: Vec<GroupId>,
    group_specs: Vec<SimGroup>,
    tasks: Vec<TaskState>,
    /// Índice da tarefa no cenário por `TaskId::index`.
    by_id: Vec<Option<usize>>,
    next_tick: u64,
    tick_ns: u64,
    window_started: bool,
    group_at_window_start: Vec<(u64, u64, u64, u64)>,
    samples: Vec<(u64, Vec<u64>)>,
    next_sample: Option<u64>,
}

impl Simulator {
    /// Monta o cenário em t = 0: cria os grupos (com peso e banda) e agenda as tarefas.
    pub fn new(cfg: SimConfig, groups: &[SimGroup], tasks: &[SimTask]) -> Simulator {
        let clock = ManualClock::new(0);
        let mut sched = Sched::new(clock.clone(), cfg.sched);
        let mut gids = Vec::with_capacity(groups.len());
        for (i, g) in groups.iter().enumerate() {
            let parent = match g.parent {
                Some(p) => {
                    assert!(p < i, "grupo pai precisa vir antes");
                    gids[p]
                }
                None => sched.root_group(),
            };
            let id = sched.create_group(parent);
            if g.weight != 100 {
                sched.set_group_weight(id, g.weight);
            }
            if let Some(q) = g.quota_ns {
                sched.set_group_period_offset(id, g.period_offset_ns);
                sched.set_group_bandwidth(id, Some(q), g.period_ns).expect("cpu.max válido");
            }
            gids.push(id);
        }
        let tick_ns = cfg.sched.tunables.tick_nsec;
        let states = tasks
            .iter()
            .map(|t| TaskState {
                spec: t.clone(),
                id: None,
                remaining_run_ns: 0,
                timer_soft: None,
                timer_hard: None,
                wake_request: None,
                latencies_ns: Vec::new(),
                cpu_at_window_start: None,
                migrations_at_window_start: 0,
            })
            .collect();
        let mut sim = Simulator {
            cfg,
            clock,
            sched,
            groups: gids,
            group_specs: groups.to_vec(),
            tasks: states,
            by_id: Vec::new(),
            next_tick: 0,
            tick_ns,
            window_started: false,
            group_at_window_start: vec![(0, 0, 0, 0); groups.len()],
            samples: Vec::new(),
            next_sample: cfg.sample_every_ns.map(|_| 0),
        };
        sim.next_tick = sim.tick_after(0, true);
        if cfg.measure_from_ns == 0 {
            sim.open_window();
        }
        sim
    }

    /// Instante atual.
    pub fn now(&self) -> u64 {
        self.clock.now_ns()
    }

    /// O escalonador, pra inspeção.
    pub fn sched(&self) -> &Sched<ManualClock> {
        &self.sched
    }

    /// Id da tarefa `i` do cenário, se já foi criada.
    pub fn task_id(&self, i: usize) -> Option<TaskId> {
        self.tasks[i].id
    }

    /// Id do grupo `i` do cenário.
    pub fn group_id(&self, i: usize) -> GroupId {
        self.groups[i]
    }

    /// Primeiro ponto da grade de ticks depois de `t` (ou em `t`, se `inclusive`).
    fn tick_after(&self, t: u64, inclusive: bool) -> u64 {
        let phase = self.cfg.tick_phase_ns % self.tick_ns;
        if t < phase {
            return phase;
        }
        let k = (t - phase) / self.tick_ns;
        let at = phase + k * self.tick_ns;
        if inclusive && at == t { at } else { at + self.tick_ns }
    }

    fn index_of(&self, t: TaskId) -> Option<usize> {
        self.by_id.get(t.index() as usize).copied().flatten()
    }

    fn running_index(&self, cpu: usize) -> Option<usize> {
        self.sched.current(cpu).and_then(|t| self.index_of(t))
    }

    fn any_busy(&self) -> bool {
        (0..self.sched.nr_cpus()).any(|c| self.sched.current(c).is_some())
    }

    /// Instante do próximo evento.
    fn next_event_time(&self) -> u64 {
        let mut t = u64::MAX;
        if self.any_busy() {
            t = t.min(self.next_tick);
        }
        for cpu in 0..self.sched.nr_cpus() {
            if let Some(i) = self.running_index(cpu)
                && let Workload::Periodic { .. } = self.tasks[i].spec.workload
            {
                t = t.min(self.now() + self.tasks[i].remaining_run_ns);
            }
        }
        for s in &self.tasks {
            if s.id.is_none() {
                t = t.min(s.spec.start_ns);
            }
            if let Some(h) = s.timer_hard {
                t = t.min(h);
            }
        }
        if let Some(b) = self.sched.next_timer_ns() {
            t = t.min(b);
        }
        if let Some(s) = self.next_sample {
            t = t.min(s);
        }
        t
    }

    fn open_window(&mut self) {
        self.window_started = true;
        for i in 0..self.tasks.len() {
            let (cpu, mig) = match self.tasks[i].id {
                Some(id) => (self.sched.task_runtime_now(id), self.sched.task(id).nr_migrations),
                None => (0, 0),
            };
            self.tasks[i].cpu_at_window_start = Some(cpu);
            self.tasks[i].migrations_at_window_start = mig;
        }
        for (i, &g) in self.groups.iter().enumerate() {
            let info = self.sched.group(g);
            self.group_at_window_start[i] =
                (self.sched.group_runtime_now(g), info.nr_periods, info.nr_throttled, info.throttled_time_ns);
        }
    }

    /// Avança o tempo até `t`, gastando a rajada de quem roda.
    fn advance_to(&mut self, t: u64) {
        let now = self.now();
        if t <= now {
            return;
        }
        if !self.window_started && self.cfg.measure_from_ns > now && self.cfg.measure_from_ns < t {
            self.advance_to(self.cfg.measure_from_ns);
        }
        let now = self.now();
        let dt = t - now;
        for cpu in 0..self.sched.nr_cpus() {
            if let Some(i) = self.running_index(cpu)
                && let Workload::Periodic { .. } = self.tasks[i].spec.workload
            {
                self.tasks[i].remaining_run_ns = self.tasks[i].remaining_run_ns.saturating_sub(dt);
            }
        }
        self.clock.set(t);
        if !self.window_started && self.now() >= self.cfg.measure_from_ns {
            self.open_window();
        }
    }

    fn wake(&mut self, i: usize) {
        let soft = self.tasks[i].timer_soft.take().expect("timer armado");
        self.tasks[i].timer_hard = None;
        if let Workload::Periodic { run_ns, .. } = self.tasks[i].spec.workload {
            self.tasks[i].remaining_run_ns = run_ns;
        }
        self.tasks[i].wake_request = Some(soft);
        let id = self.tasks[i].id.expect("tarefa criada");
        self.sched.try_to_wake_up(id);
    }

    /// Depois de um `schedule`, registra a latência de quem voltou a rodar.
    fn note_switch(&mut self, cpu: usize) {
        let Some(i) = self.running_index(cpu) else { return };
        if let Some(req) = self.tasks[i].wake_request.take()
            && self.window_started
            && req >= self.cfg.measure_from_ns
        {
            let lat = self.now() - req;
            self.tasks[i].latencies_ns.push(lat);
        }
    }

    fn schedule(&mut self, cpu: usize, prev_blocks: bool) {
        self.sched.schedule(cpu, prev_blocks);
        self.note_switch(cpu);
    }

    /// Atende todos os pedidos de reescalonamento pendentes (uma troca pode gerar outra, por exemplo
    /// uma migração do balanceamento que preempta a CPU de destino).
    fn resched_pending(&mut self) {
        for _ in 0..8 {
            let mut any = false;
            for cpu in 0..self.sched.nr_cpus() {
                if self.sched.need_resched(cpu) {
                    self.schedule(cpu, false);
                    any = true;
                }
            }
            if !any {
                break;
            }
        }
    }

    /// CPU de criação: a permitida com menos tarefas.
    fn placement_cpu(&self, mask: u64) -> usize {
        if self.sched.config().topology == Topology::Shared {
            return mask.trailing_zeros() as usize;
        }
        (0..self.sched.nr_cpus())
            .filter(|&c| mask & (1 << c) != 0)
            .min_by_key(|&c| (self.sched.rq_nr_running(c), c))
            .expect("máscara sem CPU")
    }

    /// Processa todos os eventos do próximo instante. Devolve `false` quando chegou ao fim.
    pub fn step(&mut self) -> bool {
        let end = self.cfg.duration_ns;
        let t = self.next_event_time();
        if t > end {
            self.advance_to(end);
            return false;
        }
        let n = self.sched.nr_cpus();
        let busy_before: Vec<bool> = (0..n).map(|c| self.sched.current(c).is_some()).collect();
        self.advance_to(t);

        // 1. Fim de rajada: a tarefa corrente dorme.
        for cpu in 0..n {
            if let Some(i) = self.running_index(cpu)
                && let Workload::Periodic { sleep_ns, timer_slack_ns, .. } = self.tasks[i].spec.workload
                && self.tasks[i].remaining_run_ns == 0
            {
                let soft = t + sleep_ns;
                self.tasks[i].timer_soft = Some(soft);
                self.tasks[i].timer_hard = Some(soft + timer_slack_ns);
                self.schedule(cpu, true);
            }
        }

        // 2. Criação de tarefas.
        for i in 0..self.tasks.len() {
            if self.tasks[i].id.is_none() && self.tasks[i].spec.start_ns == t {
                let all = if n == 64 { u64::MAX } else { (1u64 << n) - 1 };
                let mask = self.tasks[i].spec.cpus.unwrap_or(all) & all;
                let cpu = self.placement_cpu(mask);
                let group = self.tasks[i].spec.group.map(|g| self.groups[g]).unwrap_or(GroupId::ROOT);
                let id = self.sched.create_task(self.tasks[i].spec.nice, group, cpu);
                if mask != all {
                    self.sched.set_affinity(id, mask);
                }
                if let Some(s) = self.tasks[i].spec.custom_slice_ns {
                    self.sched.set_custom_slice(id, Some(s));
                }
                self.tasks[i].id = Some(id);
                let idx = id.index() as usize;
                if self.by_id.len() <= idx {
                    self.by_id.resize(idx + 1, None);
                }
                self.by_id[idx] = Some(i);
                if let Workload::Periodic { run_ns, .. } = self.tasks[i].spec.workload {
                    self.tasks[i].remaining_run_ns = run_ns;
                }
                if self.window_started {
                    self.tasks[i].cpu_at_window_start = Some(0);
                    self.tasks[i].migrations_at_window_start = 0;
                }
                self.sched.wake_up_new_task(id);
                self.resched_pending();
            }
        }

        // 3. Timers de sono vencidos (vencimento duro).
        for i in 0..self.tasks.len() {
            if self.tasks[i].timer_hard == Some(t) {
                self.wake(i);
                self.resched_pending();
            }
        }

        // 4. Timers de banda.
        if self.sched.next_timer_ns().is_some_and(|b| b <= t) {
            self.sched.run_timers();
            self.resched_pending();
        }

        // 5. Tick nas CPUs ocupadas (NO_HZ), processando também os hrtimers cujo vencimento suave já
        //    passou na CPU.
        if t == self.next_tick {
            for (cpu, &was_busy) in busy_before.iter().enumerate() {
                if was_busy || self.sched.current(cpu).is_some() {
                    self.sched.tick(cpu);
                    for i in 0..self.tasks.len() {
                        if self.tasks[i].timer_soft.is_some_and(|s| s <= t)
                            && self.tasks[i].id.is_some_and(|id| self.sched.task(id).cpu == cpu)
                        {
                            self.wake(i);
                        }
                    }
                }
            }
            self.resched_pending();
            self.next_tick = self.tick_after(t, false);
        } else if self.next_tick < t {
            self.next_tick = self.tick_after(t, false);
        }

        // 6. Amostra.
        if let (Some(s), Some(every)) = (self.next_sample, self.cfg.sample_every_ns)
            && s == t
        {
            let v = self.groups.iter().map(|&g| self.sched.group_runtime_accounted(g)).collect();
            self.samples.push((t, v));
            self.next_sample = Some(t + every);
        }
        true
    }

    /// Roda até o fim.
    pub fn run(&mut self) {
        while self.step() {}
    }

    /// Relatório da janela de medição.
    pub fn report(&self) -> SimReport {
        let window_ns = self.now().saturating_sub(self.cfg.measure_from_ns);
        let cpu: Vec<u64> = self
            .tasks
            .iter()
            .map(|s| match s.id {
                Some(id) => self.sched.task_runtime_now(id).saturating_sub(s.cpu_at_window_start.unwrap_or(0)),
                None => 0,
            })
            .collect();
        let total: u64 = cpu.iter().sum();
        let share = |c: u64| if total == 0 { 0.0 } else { c as f64 / total as f64 };
        let tasks = self
            .tasks
            .iter()
            .zip(&cpu)
            .map(|(s, &c)| SimTaskReport {
                name: s.spec.name.clone(),
                nice: s.spec.nice,
                group: s.spec.group,
                cpu_ns: c,
                share: share(c),
                latencies_ns: s.latencies_ns.clone(),
                wakeups: s.id.map(|id| self.sched.task(id).nr_wakeups).unwrap_or(0),
                migrations: s.id.map(|id| self.sched.task(id).nr_migrations - s.migrations_at_window_start).unwrap_or(0),
            })
            .collect();
        let groups = self
            .groups
            .iter()
            .enumerate()
            .map(|(i, &g)| {
                let info = self.sched.group(g);
                let (c0, p0, t0, tt0) = self.group_at_window_start[i];
                let c = self.sched.group_runtime_now(g) - c0;
                SimGroupReport {
                    name: self.group_specs[i].name.clone(),
                    cpu_ns: c,
                    share: share(c),
                    nr_periods: info.nr_periods - p0,
                    nr_throttled: info.nr_throttled - t0,
                    throttled_time_ns: info.throttled_time_ns - tt0,
                }
            })
            .collect();
        let stats = (0..self.sched.nr_cpus()).map(|c| self.sched.stats(c)).collect();
        SimReport { window_ns, tasks, groups, stats, samples: self.samples.clone() }
    }
}

/// Roda o cenário até `duration_ns` e devolve o relatório.
pub fn simulate(cfg: SimConfig, groups: &[SimGroup], tasks: &[SimTask]) -> SimReport {
    let mut sim = Simulator::new(cfg, groups, tasks);
    sim.run();
    sim.report()
}

/// Percentil `p` (0 a 100) pelo método do vizinho mais próximo, sobre uma cópia ordenada.
pub fn percentile(values: &[u64], p: f64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_unstable();
    let rank = ((p / 100.0) * v.len() as f64).ceil() as usize;
    Some(v[rank.clamp(1, v.len()) - 1])
}
