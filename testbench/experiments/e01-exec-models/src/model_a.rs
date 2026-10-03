//! Modelo A: uma thread do SO por pseudo-processo, com N tokens de CPU virtual.
//!
//! Só a thread que segura um token roda; as outras ficam em `park()`. Ceder ou bloquear é: devolver o
//! token (entregando direto ao próximo da fila, se houver), `unpark` desse próximo e `park` até receber
//! um token de novo. Quem leva cada thread pra um núcleo físico é o escalonador do host.
//!
//! O watchdog (opcional) roda no timer: se um processo ignora `attention` por mais de 3 ms de relógio e
//! consome mais de 2 ms de CPU nesse intervalo (lido de `/proc/self/task/<tid>/schedstat`), a thread
//! dele é rebaixada pra nice 19.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::task::{Wake, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

use crate::kernel::{Core, ExitStatus, Fd, File, Pid, ProcCommon, SLICE, Tick, Timer, mark_attention, run_guarded};
use crate::sys::{ProcMain, Sys};

const NEW: u8 = 0;
const RUNNING: u8 = 1;
const RUNNABLE: u8 = 2;
const BLOCKED: u8 = 3;
const DEAD: u8 = 4;
const NO_CPU: usize = usize::MAX;

/// Limiares do watchdog.
const WATCHDOG_WALL: Duration = Duration::from_millis(3);
const WATCHDOG_CPU: Duration = Duration::from_millis(2);

#[derive(Clone, Debug)]
pub struct ConfigA {
    pub ncpus: usize,
    pub stack_size: usize,
    pub timer: bool,
    /// Espera ativa antes do `park` ao esperar um token (zero = só `park`). Com espera ativa, um handoff
    /// rápido não passa por futex, ao custo de CPU do host gasto girando.
    pub spin: Duration,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct WatchdogStats {
    pub reniced: u64,
    /// Do timer marcar o processo até o rebaixamento, em ns.
    pub detect_ns: Vec<u64>,
    pub restore_attempts: u64,
    pub restore_ok: u64,
    pub restore_errors: Vec<String>,
}

struct SchedA {
    runq: VecDeque<Arc<ProcA>>,
    free: Vec<usize>,
    running: Vec<Option<Arc<ProcA>>>,
}

pub struct SharedA {
    pub core: Arc<Core>,
    sched: Mutex<SchedA>,
    stack_size: usize,
    spin: Duration,
    watchdog: AtomicBool,
    pub watchdog_stats: Mutex<WatchdogStats>,
    pub spawn_failures: AtomicU64,
}

pub struct ProcA {
    common: Arc<ProcCommon>,
    shared: Arc<SharedA>,
    thread: OnceLock<Thread>,
    cpu: AtomicUsize,
    state: AtomicU8,
    wake_pending: AtomicBool,
    reniced: AtomicBool,
    restore_tried: AtomicBool,
    suspect_cpu_ns: AtomicU64,
}

impl ProcA {
    fn unpark(&self) {
        if let Some(t) = self.thread.get() {
            t.unpark();
        }
    }

    fn wait_for_grant(&self) {
        let spin = self.shared.spin;
        if !spin.is_zero() && self.cpu.load(Ordering::Acquire) == NO_CPU {
            let start = Instant::now();
            let mut i = 0u32;
            while self.cpu.load(Ordering::Acquire) == NO_CPU {
                std::hint::spin_loop();
                i = i.wrapping_add(1);
                if i.is_multiple_of(64) && start.elapsed() >= spin {
                    break;
                }
            }
        }
        while self.cpu.load(Ordering::Acquire) == NO_CPU {
            thread::park();
        }
    }
}

impl SharedA {
    /// Torna `p` executável: entrega um token livre ou coloca na fila. Devolve quem precisa de `unpark`.
    fn make_runnable_locked(s: &mut SchedA, p: &Arc<ProcA>) -> Option<Arc<ProcA>> {
        if let Some(cpu) = s.free.pop() {
            p.state.store(RUNNING, Ordering::Relaxed);
            p.cpu.store(cpu, Ordering::Release);
            s.running[cpu] = Some(p.clone());
            Some(p.clone())
        } else {
            p.state.store(RUNNABLE, Ordering::Relaxed);
            s.runq.push_back(p.clone());
            None
        }
    }

    /// Devolve o token `cpu`: passa direto pro primeiro da fila ou marca como livre.
    fn release_cpu_locked(s: &mut SchedA, cpu: usize) -> Option<Arc<ProcA>> {
        if let Some(next) = s.runq.pop_front() {
            next.state.store(RUNNING, Ordering::Relaxed);
            next.cpu.store(cpu, Ordering::Release);
            s.running[cpu] = Some(next.clone());
            Some(next)
        } else {
            s.running[cpu] = None;
            s.free.push(cpu);
            None
        }
    }

    fn watch(&self, p: &ProcA, now_ns: u64) {
        if !p.common.attention.load(Ordering::Relaxed) || p.reniced.load(Ordering::Relaxed) {
            return;
        }
        let set = p.common.attention_set_ns.load(Ordering::Relaxed);
        if set == 0 || now_ns.saturating_sub(set) < WATCHDOG_WALL.as_nanos() as u64 {
            return;
        }
        let tid = p.common.host_tid.load(Ordering::Relaxed);
        let Some(cpu_ns) = thread_cpu_ns(tid) else { return };
        let suspect = p.suspect_cpu_ns.load(Ordering::Relaxed);
        if suspect == 0 {
            p.suspect_cpu_ns.store(cpu_ns.max(1), Ordering::Relaxed);
            return;
        }
        if cpu_ns.saturating_sub(suspect) >= WATCHDOG_CPU.as_nanos() as u64 {
            let target = rustix::process::Pid::from_raw(tid);
            if target.is_some() && rustix::process::setpriority_process(target, 19).is_ok() {
                p.reniced.store(true, Ordering::Relaxed);
                let mut st = self.watchdog_stats.lock();
                st.reniced += 1;
                st.detect_ns.push(now_ns.saturating_sub(set));
            }
        }
    }
}

/// Tempo de CPU de uma thread do próprio processo host, em ns (primeiro campo de schedstat).
pub fn thread_cpu_ns(tid: i32) -> Option<u64> {
    let text = std::fs::read_to_string(format!("/proc/self/task/{tid}/schedstat")).ok()?;
    text.split_whitespace().next()?.parse().ok()
}

impl Tick for SharedA {
    fn tick(&self, now_ns: u64) {
        let running: Vec<Arc<ProcA>> = {
            let s = self.sched.lock();
            s.running.iter().flatten().cloned().collect()
        };
        let watchdog = self.watchdog.load(Ordering::Relaxed);
        for p in running {
            mark_attention(&p.common, &self.core);
            if watchdog {
                self.watch(&p, now_ns);
            }
        }
    }
}

impl Wake for ProcA {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let grant = {
            let mut s = self.shared.sched.lock();
            if self.state.load(Ordering::Relaxed) == BLOCKED {
                SharedA::make_runnable_locked(&mut s, self)
            } else {
                self.wake_pending.store(true, Ordering::Relaxed);
                None
            }
        };
        if let Some(p) = grant {
            p.unpark();
        }
    }
}

struct CtxA {
    proc: Arc<ProcA>,
    waker: Waker,
}

impl Sys for CtxA {
    fn proc(&self) -> &Arc<ProcCommon> {
        &self.proc.common
    }

    fn core(&self) -> &Core {
        &self.proc.shared.core
    }

    fn waker(&self) -> &Waker {
        &self.waker
    }

    fn block(&self) {
        let next = {
            let mut s = self.proc.shared.sched.lock();
            if self.proc.wake_pending.swap(false, Ordering::Relaxed) {
                return;
            }
            self.proc.state.store(BLOCKED, Ordering::Relaxed);
            let cpu = self.proc.cpu.swap(NO_CPU, Ordering::Relaxed);
            SharedA::release_cpu_locked(&mut s, cpu)
        };
        if let Some(n) = next {
            n.unpark();
        }
        self.proc.wait_for_grant();
    }

    fn yield_now(&self) {
        let next = {
            let mut s = self.proc.shared.sched.lock();
            let Some(next) = s.runq.pop_front() else { return };
            let cpu = self.proc.cpu.swap(NO_CPU, Ordering::Relaxed);
            next.state.store(RUNNING, Ordering::Relaxed);
            next.cpu.store(cpu, Ordering::Release);
            s.running[cpu] = Some(next.clone());
            self.proc.state.store(RUNNABLE, Ordering::Relaxed);
            s.runq.push_back(self.proc.clone());
            next
        };
        self.proc.shared.core.switches.fetch_add(1, Ordering::Relaxed);
        next.unpark();
        self.proc.wait_for_grant();
        self.proc.shared.core.probe_after_resume();
    }

    fn spawn_with(&self, files: Vec<(Fd, File)>, main: ProcMain) -> Pid {
        spawn(&self.proc.shared, files, main)
    }

    fn on_attention_ack(&self) {
        self.proc.suspect_cpu_ns.store(0, Ordering::Relaxed);
        if self.proc.reniced.load(Ordering::Relaxed) && !self.proc.restore_tried.swap(true, Ordering::Relaxed) {
            // Tenta voltar pra nice 0. Sem CAP_SYS_NICE e com RLIMIT_NICE = 0 isso falha (EACCES).
            let r = rustix::process::setpriority_process(None, 0);
            let mut st = self.proc.shared.watchdog_stats.lock();
            st.restore_attempts += 1;
            match r {
                Ok(()) => st.restore_ok += 1,
                Err(e) => {
                    let name = match e.raw_os_error() {
                        13 => "EACCES".to_string(),
                        1 => "EPERM".to_string(),
                        n => format!("errno {n}"),
                    };
                    st.restore_errors.push(name);
                }
            }
        }
    }
}

fn thread_main(proc: Arc<ProcA>, main: ProcMain) {
    let _ = proc.thread.set(thread::current());
    proc.common
        .host_tid
        .store(rustix::thread::gettid().as_raw_nonzero().get(), Ordering::Relaxed);
    let shared = proc.shared.clone();
    {
        let mut s = shared.sched.lock();
        let _ = SharedA::make_runnable_locked(&mut s, &proc);
    }
    proc.wait_for_grant();
    let ctx = CtxA { proc: proc.clone(), waker: Waker::from(proc.clone()) };
    let status = run_guarded(|| main(&ctx));
    drop(ctx);
    shared.core.finish(&proc.common, status);
    let next = {
        let mut s = shared.sched.lock();
        proc.state.store(DEAD, Ordering::Relaxed);
        let cpu = proc.cpu.swap(NO_CPU, Ordering::Relaxed);
        SharedA::release_cpu_locked(&mut s, cpu)
    };
    if let Some(n) = next {
        n.unpark();
    }
}

fn spawn(shared: &Arc<SharedA>, files: Vec<(Fd, File)>, main: ProcMain) -> Pid {
    let common = shared.core.new_proc(files);
    let pid = common.pid;
    let proc = Arc::new(ProcA {
        common: common.clone(),
        shared: shared.clone(),
        thread: OnceLock::new(),
        cpu: AtomicUsize::new(NO_CPU),
        state: AtomicU8::new(NEW),
        wake_pending: AtomicBool::new(false),
        reniced: AtomicBool::new(false),
        restore_tried: AtomicBool::new(false),
        suspect_cpu_ns: AtomicU64::new(0),
    });
    common.set_waker(Waker::from(proc.clone()));
    let p2 = proc.clone();
    let r = thread::Builder::new().stack_size(shared.stack_size).spawn(move || thread_main(p2, main));
    if let Err(e) = r {
        shared.spawn_failures.fetch_add(1, Ordering::Relaxed);
        shared.core.finish(&common, ExitStatus::Panicked(format!("falha ao criar thread: {e}")));
    }
    pid
}

pub struct KernelA {
    shared: Arc<SharedA>,
    _timer: Option<Timer>,
}

impl KernelA {
    pub fn new(cfg: ConfigA) -> KernelA {
        let core = Arc::new(Core::new());
        let shared = Arc::new(SharedA {
            core: core.clone(),
            sched: Mutex::new(SchedA {
                runq: VecDeque::new(),
                free: (0..cfg.ncpus).rev().collect(),
                running: vec![None; cfg.ncpus],
            }),
            stack_size: cfg.stack_size,
            spin: cfg.spin,
            watchdog: AtomicBool::new(false),
            watchdog_stats: Mutex::new(WatchdogStats::default()),
            spawn_failures: AtomicU64::new(0),
        });
        let timer = cfg.timer.then(|| Timer::start(core, shared.clone(), SLICE));
        KernelA { shared, _timer: timer }
    }

    pub fn shared(&self) -> &Arc<SharedA> {
        &self.shared
    }

    pub fn set_watchdog(&self, on: bool) {
        self.shared.watchdog.store(on, Ordering::Relaxed);
    }

    /// tid da thread do host de um processo (0 se ainda não começou).
    pub fn host_tid(&self, pid: Pid) -> i32 {
        self.shared.core.lookup(pid).map(|p| p.host_tid.load(Ordering::Relaxed)).unwrap_or(0)
    }
}

impl crate::SyncKernel for KernelA {
    fn core(&self) -> &Arc<Core> {
        &self.shared.core
    }

    fn spawn(&self, files: Vec<(Fd, File)>, main: ProcMain) -> Pid {
        spawn(&self.shared, files, main)
    }
}
