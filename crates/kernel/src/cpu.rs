//! CPUs virtuais: o EEVDF do `crates/sched` decide quem roda; cada CPU virtual é um token, e só a thread
//! que segura um token executa código de pseudo-processo.
//!
//! - Uma thread nova pede entrada ([`Cpus::start`]: `create_task` + `wake_up_new_task`) e espera o token.
//! - Quem vai dormir devolve o token ([`Cpus::sleep`]: `schedule(cpu, true)`) e entrega ao próximo.
//! - Quem acordou de um evento pede o token de volta ([`Cpus::wake`]: `try_to_wake_up`) e espera.
//! - A thread de tick chama `tick` em cada CPU ocupada (HZ=250, como o Debian), roda os timers de banda e,
//!   quando o escalonador pede troca (`need_resched`), liga a atenção do corrente: ele cede no próximo
//!   ponto de checagem ([`Cpus::checkpoint`]). CPU ociosa com trabalho na fila pega o próximo na hora.
//! - Watchdog: corrente que não atende um pedido de troca por mais de [`WATCHDOG_NS`] é tirado da CPU
//!   à força (a CPU vai pro próximo) e a thread do host dele cai pra nice 19 (E01, H07). Ele continua
//!   rodando no host até chegar num ponto de checagem, onde espera o token como qualquer um.
//!
//! O escalonador inteiro fica atrás de uma trava (o kernel do Linux tem uma por runqueue). Com a
//! topologia de runqueue única (padrão) a trava é a mesma de qualquer jeito; a vazão medida no E02 com
//! essa escolha está no `crates/sched`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::Mutex;
use sched::{Features, GroupId, MonotonicClock, Sched, SchedConfig, TaskId, Topology, Tunables};
use sysabi::Errno;

use crate::park::Parker;

/// HZ do kernel do Debian 13 (CONFIG_HZ_250).
pub(crate) const HZ: u64 = 250;
/// Tempo sem atender um pedido de troca até o watchdog agir.
pub(crate) const WATCHDOG_NS: u64 = 8_000_000;

/// O lado "escalonável" de uma thread de pseudo-processo.
#[derive(Debug)]
pub(crate) struct CpuTask {
    id: Mutex<Option<TaskId>>,
    /// CPU concedida + 1; 0 = sem CPU.
    granted: AtomicUsize,
    /// O escalonador pediu que ceda a CPU.
    pub resched: AtomicBool,
    resched_since: AtomicU64,
    /// Fora da fila (dormindo).
    sleeping: AtomicBool,
    /// A thread do host já foi rebaixada pra nice 19.
    reniced: AtomicBool,
    pub host_tid: AtomicI32,
    pub attention: Arc<AtomicBool>,
    /// Só pra esperar o token. É separado do parker de eventos da thread: uma concessão de CPU nunca
    /// consome o aviso de um evento (senão a thread que conferiu a condição, perdeu a CPU no checkpoint
    /// e esperou o token engoliria o aviso e dormiria pra sempre).
    grant: Parker,
}

impl CpuTask {
    pub(crate) fn new(attention: Arc<AtomicBool>) -> Arc<CpuTask> {
        Arc::new(CpuTask {
            id: Mutex::new(None),
            granted: AtomicUsize::new(0),
            resched: AtomicBool::new(false),
            resched_since: AtomicU64::new(0),
            sleeping: AtomicBool::new(false),
            reniced: AtomicBool::new(false),
            host_tid: AtomicI32::new(0),
            attention,
            grant: Parker::default(),
        })
    }

    fn id(&self) -> Option<TaskId> {
        *self.id.lock()
    }

    fn cpu(&self) -> Option<usize> {
        match self.granted.load(Ordering::Acquire) {
            0 => None,
            n => Some(n - 1),
        }
    }

    /// Espera receber uma CPU.
    fn wait_grant(&self) {
        while self.granted.load(Ordering::Acquire) == 0 {
            self.grant.park();
        }
    }
}

struct Inner {
    s: Sched<MonotonicClock>,
    tasks: HashMap<TaskId, Arc<CpuTask>>,
    running: Vec<Option<Arc<CpuTask>>>,
    /// Grupos devolvidos, por pai, pra reaproveitar (o `sched` não remove grupos).
    free_groups: HashMap<GroupId, Vec<GroupId>>,
    /// Instante (relógio do escalonador) de quando cada CPU ficou com o corrente atual.
    clock: MonotonicClock,
}

/// As CPUs virtuais de um kernel.
pub(crate) struct Cpus {
    inner: Mutex<Inner>,
    ncpus: usize,
    stop: AtomicBool,
}

impl std::fmt::Debug for Cpus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cpus({})", self.ncpus)
    }
}

/// Peso e limite de banda de um grupo (`cpu.weight`, `cpu.max`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GroupLimits {
    pub weight: u64,
    pub max: Option<(u64, u64)>,
}

const DEFAULT_PERIOD_NS: u64 = 100_000_000;

impl Cpus {
    pub(crate) fn new(ncpus: usize, topology: Topology) -> Arc<Cpus> {
        let clock = MonotonicClock::new();
        let tun = Tunables::linux_6_12_101(ncpus as u32, HZ);
        let cfg = SchedConfig::new(ncpus, topology, tun, Features::default());
        let s = Sched::new(clock, cfg);
        let cpus = Arc::new(Cpus {
            inner: Mutex::new(Inner { s, tasks: HashMap::new(), running: vec![None; ncpus], free_groups: HashMap::new(), clock }),
            ncpus,
            stop: AtomicBool::new(false),
        });
        let weak = Arc::downgrade(&cpus);
        let tick = Duration::from_nanos(tun.tick_nsec);
        // Thread de tick do kernel: nasce aqui, da thread do host que criou o kernel (nunca da spawner).
        let _ = std::thread::Builder::new().name("pl-tick".into()).spawn(move || tick_loop(weak, tick));
        cpus
    }

    pub(crate) fn ncpus(&self) -> usize {
        self.ncpus
    }

    // ---- grupos ----

    pub(crate) fn create_group(&self, parent: Option<GroupId>, lim: GroupLimits) -> Result<GroupId, Errno> {
        let mut g = self.inner.lock();
        let parent = parent.unwrap_or(GroupId::ROOT);
        let id = match g.free_groups.get_mut(&parent).and_then(Vec::pop) {
            Some(id) => id,
            None => g.s.create_group(parent),
        };
        if let Err(e) = apply_limits(&mut g.s, id, lim) {
            g.free_groups.entry(parent).or_default().push(id);
            return Err(e);
        }
        Ok(id)
    }

    pub(crate) fn set_group(&self, id: GroupId, lim: GroupLimits) -> Result<(), Errno> {
        let mut g = self.inner.lock();
        apply_limits(&mut g.s, id, lim)
    }

    /// Devolve um grupo sem tarefas pra ser reaproveitado.
    pub(crate) fn release_group(&self, id: GroupId) {
        let mut g = self.inner.lock();
        let parent = g.s.group(id).parent.unwrap_or(GroupId::ROOT);
        let _ = apply_limits(&mut g.s, id, GroupLimits { weight: 100, max: None });
        g.free_groups.entry(parent).or_default().push(id);
    }

    /// Tempo de CPU somado das tarefas de um grupo e dos subgrupos.
    pub(crate) fn group_runtime(&self, id: GroupId) -> u64 {
        self.inner.lock().s.group_runtime_now(id)
    }

    // ---- tarefas ----

    /// Entrada de uma thread nova: cria a tarefa, acorda e espera a CPU. Chamada pela própria thread.
    pub(crate) fn start(&self, ct: &Arc<CpuTask>, nice: i32, group: GroupId) {
        {
            let mut g = self.inner.lock();
            let cpu = least_loaded(&g.s);
            let id = g.s.create_task(nice.clamp(-20, 19), group, cpu);
            *ct.id.lock() = Some(id);
            g.tasks.insert(id, ct.clone());
            g.s.wake_up_new_task(id);
            kick(&mut g);
        }
        ct.wait_grant();
    }

    /// Vai dormir: devolve a CPU e entrega ao próximo. Uma tarefa que o watchdog tirou da CPU ainda está
    /// na fila como pronta; ela espera voltar a ser corrente pra então sair da fila dormindo.
    pub(crate) fn sleep(&self, ct: &Arc<CpuTask>) {
        loop {
            ct.wait_grant();
            let mut g = self.inner.lock();
            let Some(cpu) = ct.cpu() else { continue };
            if g.s.current(cpu) != ct.id() {
                ct.granted.store(0, Ordering::Release);
                continue;
            }
            ct.sleeping.store(true, Ordering::Release);
            ct.resched.store(false, Ordering::Relaxed);
            let next = g.s.schedule(cpu, true);
            switch(&mut g, cpu, next);
            kick(&mut g);
            return;
        }
    }

    /// Acordou de um evento: volta pra fila e espera a CPU.
    pub(crate) fn wake(&self, ct: &Arc<CpuTask>) {
        {
            let mut g = self.inner.lock();
            if ct.sleeping.swap(false, Ordering::AcqRel)
                && let Some(id) = ct.id()
            {
                g.s.try_to_wake_up(id);
                kick(&mut g);
            }
        }
        ct.wait_grant();
    }

    /// Ponto de checagem: cede a CPU se o escalonador pediu; espera se o watchdog tirou a CPU.
    pub(crate) fn checkpoint(&self, ct: &Arc<CpuTask>) {
        if ct.resched.swap(false, Ordering::AcqRel) {
            let mut g = self.inner.lock();
            if let Some(cpu) = ct.cpu()
                && g.s.current(cpu) == ct.id()
                && g.s.need_resched(cpu)
            {
                let next = g.s.schedule(cpu, false);
                switch(&mut g, cpu, next);
                kick(&mut g);
            }
        }
        if ct.granted.load(Ordering::Acquire) == 0 {
            ct.wait_grant();
        }
    }

    /// `sched_yield`.
    pub(crate) fn yield_now(&self, ct: &Arc<CpuTask>) {
        {
            let mut g = self.inner.lock();
            let Some(cpu) = ct.cpu() else { return };
            if g.s.current(cpu) != ct.id() {
                return;
            }
            let next = g.s.yield_current(cpu);
            switch(&mut g, cpu, next);
            kick(&mut g);
        }
        ct.wait_grant();
    }

    /// Fim da thread: sai da CPU e devolve o tempo de CPU total dela.
    pub(crate) fn exit(&self, ct: &Arc<CpuTask>) -> u64 {
        ct.wait_grant();
        let mut g = self.inner.lock();
        let Some(id) = ct.id() else { return 0 };
        let rt = g.s.task_runtime_now(id);
        let Some(cpu) = ct.cpu() else { return rt };
        if g.s.current(cpu) != Some(id) {
            return rt;
        }
        let next = g.s.exit_current(cpu);
        g.tasks.remove(&id);
        g.running[cpu] = None;
        ct.granted.store(0, Ordering::Release);
        switch(&mut g, cpu, next);
        kick(&mut g);
        rt
    }

    pub(crate) fn set_nice(&self, ct: &Arc<CpuTask>, nice: i32) {
        let mut g = self.inner.lock();
        if let Some(id) = ct.id() {
            g.s.set_user_nice(id, nice.clamp(-20, 19));
            kick(&mut g);
        }
    }

    /// Tempo de CPU da tarefa até agora, em ns.
    pub(crate) fn runtime(&self, ct: &CpuTask) -> u64 {
        let g = self.inner.lock();
        ct.id().map(|id| g.s.task_runtime_now(id)).unwrap_or(0)
    }

    /// CPU em que a tarefa está (ou esteve por último).
    pub(crate) fn task_cpu(&self, ct: &CpuTask) -> usize {
        let g = self.inner.lock();
        ct.id().map(|id| g.s.task(id).cpu).unwrap_or(0)
    }

    /// Tarefas prontas ou rodando, por CPU (pro loadavg e o `/proc/stat`).
    pub(crate) fn nr_running(&self) -> u32 {
        let g = self.inner.lock();
        (0..self.ncpus).map(|c| g.s.rq_nr_running(c)).max().unwrap_or(0)
    }

    /// Estado legível das CPUs e tarefas (diagnóstico).
    pub(crate) fn debug_state(&self) -> String {
        use std::fmt::Write as _;
        let g = self.inner.lock();
        let mut out = String::new();
        for cpu in 0..self.ncpus {
            let _ = writeln!(
                out,
                "cpu{cpu}: sched.curr={:?} need_resched={} token={:?}",
                g.s.current(cpu),
                g.s.need_resched(cpu),
                g.running[cpu].as_ref().and_then(|c| c.id())
            );
        }
        let mut ids: Vec<&TaskId> = g.tasks.keys().collect();
        ids.sort();
        for id in ids {
            let ct = &g.tasks[id];
            let t = g.s.task(*id);
            let _ = writeln!(
                out,
                "{id:?}: granted={} resched={} sleeping={} | sched: queued={} sleeping={} running={} delayed={} on_rq={} cpu={}",
                ct.granted.load(Ordering::Relaxed),
                ct.resched.load(Ordering::Relaxed),
                ct.sleeping.load(Ordering::Relaxed),
                t.queued,
                t.sleeping,
                t.running,
                t.sched_delayed,
                t.on_rq,
                t.cpu
            );
        }
        out
    }

    fn tick(&self) {
        let mut g = self.inner.lock();
        for cpu in 0..self.ncpus {
            if g.s.current(cpu).is_some() {
                g.s.tick(cpu);
            }
        }
        let now = sched::Clock::now_ns(&g.clock);
        if g.s.next_timer_ns().is_some_and(|t| t <= now) {
            g.s.run_timers();
        }
        kick(&mut g);
        watchdog(&mut g, now);
    }
}

impl Drop for Cpus {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn apply_limits(s: &mut Sched<MonotonicClock>, id: GroupId, lim: GroupLimits) -> Result<(), Errno> {
    if !(1..=10_000).contains(&lim.weight) {
        return Err(Errno::EINVAL);
    }
    s.set_group_weight(id, lim.weight);
    let (quota, period) = match lim.max {
        Some((q, p)) => (Some(q), p),
        None => (None, DEFAULT_PERIOD_NS),
    };
    s.set_group_bandwidth(id, quota, period).map_err(|_| Errno::EINVAL)
}

fn least_loaded(s: &Sched<MonotonicClock>) -> usize {
    if s.config().topology == Topology::Shared {
        return 0;
    }
    (0..s.nr_cpus()).min_by_key(|&c| (s.rq_nr_running(c), c)).unwrap_or(0)
}

/// Troca o corrente de uma CPU: quem sai perde o token, quem entra ganha e é acordado.
fn switch(g: &mut Inner, cpu: usize, next: Option<TaskId>) {
    let prev = g.running[cpu].take();
    let next_ct = next.and_then(|id| g.tasks.get(&id).cloned());
    if let Some(p) = &prev
        && !next_ct.as_ref().is_some_and(|n| Arc::ptr_eq(n, p))
    {
        p.granted.store(0, Ordering::Release);
    }
    if let Some(n) = &next_ct {
        n.resched.store(false, Ordering::Relaxed);
        n.granted.store(cpu + 1, Ordering::Release);
        n.grant.unpark();
    }
    g.running[cpu] = next_ct;
}

/// Atende os pedidos de troca: CPU ociosa pega o próximo da fila; CPU ocupada recebe o aviso no corrente.
fn kick(g: &mut Inner) {
    for _ in 0..4 {
        let mut again = false;
        for cpu in 0..g.running.len() {
            if !g.s.need_resched(cpu) {
                continue;
            }
            match g.s.current(cpu) {
                None => {
                    let next = g.s.schedule(cpu, false);
                    switch(g, cpu, next);
                    again |= next.is_some();
                }
                Some(id) => {
                    if let Some(ct) = g.tasks.get(&id)
                        && !ct.resched.swap(true, Ordering::AcqRel)
                    {
                        let now = sched::Clock::now_ns(&g.clock);
                        ct.resched_since.store(now.max(1), Ordering::Relaxed);
                        // O corrente está rodando (nunca dormindo num evento): basta a atenção.
                        ct.attention.store(true, Ordering::Release);
                    }
                }
            }
        }
        if !again {
            break;
        }
    }
}

/// Corrente que ignora o pedido de troca há mais de WATCHDOG_NS: sai da CPU à força, nice 19 no host.
fn watchdog(g: &mut Inner, now: u64) {
    for cpu in 0..g.running.len() {
        let Some(ct) = g.running[cpu].clone() else { continue };
        if !ct.resched.load(Ordering::Acquire) {
            continue;
        }
        let since = ct.resched_since.load(Ordering::Relaxed);
        if since == 0 || now.saturating_sub(since) < WATCHDOG_NS {
            continue;
        }
        if !ct.reniced.swap(true, Ordering::AcqRel) {
            let tid = ct.host_tid.load(Ordering::Relaxed);
            if let Some(pid) = rustix::process::Pid::from_raw(tid) {
                let _ = rustix::process::setpriority_process(Some(pid), 19);
            }
        }
        let next = g.s.schedule(cpu, false);
        if next.is_some_and(|n| Some(n) != ct.id()) {
            switch(g, cpu, next);
        }
    }
}

fn tick_loop(cpus: Weak<Cpus>, tick: Duration) {
    loop {
        std::thread::sleep(tick);
        let Some(c) = cpus.upgrade() else { return };
        if c.stop.load(Ordering::Relaxed) {
            return;
        }
        c.tick();
    }
}
