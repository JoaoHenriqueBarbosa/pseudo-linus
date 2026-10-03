//! Modelo B: corrotinas corosensei presas a cada worker, sem migração.
//!
//! Cada worker é uma thread do SO com sua fila e suas corrotinas (que são `!Send` e nunca saem dali).
//! O balanceamento só acontece no spawn (round-robin). Acordar um processo de outro worker passa pela
//! caixa de entrada do dono e, se ele estiver ocioso, por `unpark`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::task::{Wake, Waker};
use std::thread::{self, JoinHandle, Thread};

use corosensei::stack::DefaultStack;
use corosensei::{Coroutine, CoroutineResult, Yielder};
use parking_lot::Mutex;

use crate::kernel::{Core, ExitStatus, Fd, File, Pid, ProcCommon, SIGKILL, SLICE, Tick, Timer, mark_attention, run_guarded};
use crate::sys::{ProcMain, Sys};

const NEW: u8 = 0;
const RUNNING: u8 = 1;
const NOTIFIED: u8 = 2;
const RUNNABLE: u8 = 3;
const BLOCKED: u8 = 4;
const DEAD: u8 = 5;

static NEXT_SHARED_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    /// "Processo corrente" guardado num thread-local do worker. Só é correto se o worker trocar o valor
    /// antes de cada `resume` (`ConfigB::tls_swap`); é o experimento do H10.
    static CURRENT_PID: Cell<Pid> = const { Cell::new(0) };
    /// Identidade do worker desta thread: (id do kernel, índice). Usado no caminho rápido de wake.
    static WORKER_ID: Cell<(u64, usize)> = const { Cell::new((0, usize::MAX)) };
    /// Processos acordados por código que roda no próprio worker dono: vão direto pra fila local, sem
    /// passar pela caixa de entrada com lock.
    static LOCAL_READY: RefCell<Vec<Arc<ProcB>>> = const { RefCell::new(Vec::new()) };
}

/// Pid do processo corrente segundo o thread-local do worker.
pub fn current_pid_tls() -> Pid {
    CURRENT_PID.with(Cell::get)
}

/// Grava o thread-local do worker (usado pelo teste ingênuo do H10, em que o próprio processo grava).
pub fn set_current_pid_tls(pid: Pid) {
    CURRENT_PID.with(|c| c.set(pid));
}

#[derive(Clone, Debug)]
pub struct ConfigB {
    pub workers: usize,
    pub stack_size: usize,
    pub timer: bool,
    /// O worker grava o pid corrente no thread-local antes de cada `resume`.
    pub tls_swap: bool,
}

pub enum Suspend {
    Yield,
    Block,
}

type Co = Coroutine<(), Suspend, ExitStatus, DefaultStack>;

struct Slot {
    co: Co,
    proc: Arc<ProcB>,
}

#[derive(Default)]
struct Inbox {
    spawns: Vec<(Arc<ProcB>, ProcMain)>,
    ready: Vec<Arc<ProcB>>,
}

struct WorkerB {
    inbox: Mutex<Inbox>,
    inbox_flag: AtomicBool,
    thread: OnceLock<Thread>,
    idle: AtomicBool,
    queued: AtomicUsize,
    running: Mutex<Option<Arc<ProcCommon>>>,
}

pub struct SharedB {
    pub core: Arc<Core>,
    id: u64,
    workers: Vec<WorkerB>,
    next: AtomicUsize,
    stack_size: usize,
    shutdown: AtomicBool,
    tls_swap: bool,
    pub stack_failures: AtomicU64,
}

pub struct ProcB {
    common: Arc<ProcCommon>,
    shared: Arc<SharedB>,
    worker: usize,
    state: AtomicU8,
    slot: AtomicUsize,
}

impl SharedB {
    fn push_inbox(&self, idx: usize, f: impl FnOnce(&mut Inbox)) {
        let w = &self.workers[idx];
        {
            let mut ib = w.inbox.lock();
            f(&mut ib);
            w.inbox_flag.store(true, Ordering::SeqCst);
        }
        if w.idle.load(Ordering::SeqCst)
            && let Some(t) = w.thread.get()
        {
            t.unpark();
        }
    }
}

impl Wake for ProcB {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        loop {
            match self.state.load(Ordering::Acquire) {
                RUNNING => {
                    if self
                        .state
                        .compare_exchange(RUNNING, NOTIFIED, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        return;
                    }
                }
                BLOCKED => {
                    if self
                        .state
                        .compare_exchange(BLOCKED, RUNNABLE, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                    {
                        let me = self.clone();
                        if WORKER_ID.with(Cell::get) == (self.shared.id, self.worker) {
                            LOCAL_READY.with(|l| l.borrow_mut().push(me));
                        } else {
                            self.shared.push_inbox(self.worker, move |ib| ib.ready.push(me));
                        }
                        return;
                    }
                }
                _ => return,
            }
        }
    }
}

impl Tick for SharedB {
    fn tick(&self, _now_ns: u64) {
        for w in &self.workers {
            if let Some(p) = w.running.lock().as_ref() {
                mark_attention(p, &self.core);
            }
        }
    }
}

struct CtxB<'a> {
    proc: Arc<ProcB>,
    yielder: &'a Yielder<(), Suspend>,
    waker: Waker,
}

impl Sys for CtxB<'_> {
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
        self.yielder.suspend(Suspend::Block);
    }

    fn yield_now(&self) {
        let w = &self.proc.shared.workers[self.proc.worker];
        if w.queued.load(Ordering::Relaxed) == 0
            && !w.inbox_flag.load(Ordering::Relaxed)
            && LOCAL_READY.with(|l| l.borrow().is_empty())
        {
            return;
        }
        self.proc.shared.core.switches.fetch_add(1, Ordering::Relaxed);
        self.yielder.suspend(Suspend::Yield);
        self.proc.shared.core.probe_after_resume();
    }

    fn spawn_with(&self, files: Vec<(Fd, File)>, main: ProcMain) -> Pid {
        spawn(&self.proc.shared, files, main)
    }
}

fn create_coroutine(shared: &SharedB, proc: Arc<ProcB>, main: ProcMain) -> std::io::Result<Co> {
    let stack = DefaultStack::new(shared.stack_size)?;
    let waker = Waker::from(proc.clone());
    Ok(Coroutine::with_stack(stack, move |yielder: &Yielder<(), Suspend>, ()| {
        let ctx = CtxB { proc, yielder, waker };
        run_guarded(|| main(&ctx))
    }))
}

fn worker_main(shared: Arc<SharedB>, idx: usize) {
    let w = &shared.workers[idx];
    let _ = w.thread.set(thread::current());
    WORKER_ID.with(|c| c.set((shared.id, idx)));
    let mut slab: Vec<Option<Slot>> = Vec::new();
    let mut free: Vec<usize> = Vec::new();
    let mut live = 0usize;
    let mut runq: VecDeque<usize> = VecDeque::new();
    let mut spawns = Vec::new();
    let mut ready = Vec::new();
    let mut last_running: Pid = 0;
    loop {
        if w.inbox_flag.load(Ordering::SeqCst) {
            {
                let mut ib = w.inbox.lock();
                w.inbox_flag.store(false, Ordering::SeqCst);
                std::mem::swap(&mut ib.spawns, &mut spawns);
                std::mem::swap(&mut ib.ready, &mut ready);
            }
            for (p, main) in spawns.drain(..) {
                match create_coroutine(&shared, p.clone(), main) {
                    Ok(co) => {
                        let i = free.pop().unwrap_or_else(|| {
                            slab.push(None);
                            slab.len() - 1
                        });
                        p.slot.store(i, Ordering::Relaxed);
                        p.state.store(RUNNABLE, Ordering::Release);
                        slab[i] = Some(Slot { co, proc: p });
                        runq.push_back(i);
                        live += 1;
                    }
                    Err(e) => {
                        shared.stack_failures.fetch_add(1, Ordering::Relaxed);
                        p.state.store(DEAD, Ordering::Release);
                        shared.core.finish(&p.common, ExitStatus::Panicked(format!("falha ao criar pilha: {e}")));
                    }
                }
            }
            for p in ready.drain(..) {
                runq.push_back(p.slot.load(Ordering::Relaxed));
            }
        }
        LOCAL_READY.with(|l| {
            for p in l.borrow_mut().drain(..) {
                runq.push_back(p.slot.load(Ordering::Relaxed));
            }
        });
        let Some(i) = runq.pop_front() else {
            if shared.shutdown.load(Ordering::SeqCst) {
                // Processos que sobraram bloqueados: o Drop da corrotina suspensa faz force_unwind (os
                // Drops da pilha rodam) e o processo é dado como morto por SIGKILL.
                for slot in slab.drain(..).flatten() {
                    let proc = slot.proc.clone();
                    drop(slot);
                    shared.core.finish(&proc.common, ExitStatus::Signaled(SIGKILL));
                }
                break;
            }
            w.queued.store(0, Ordering::Relaxed);
            w.idle.store(true, Ordering::SeqCst);
            if w.inbox_flag.load(Ordering::SeqCst) || shared.shutdown.load(Ordering::SeqCst) {
                w.idle.store(false, Ordering::SeqCst);
                continue;
            }
            *w.running.lock() = None;
            last_running = 0;
            thread::park();
            w.idle.store(false, Ordering::SeqCst);
            continue;
        };
        w.queued.store(runq.len(), Ordering::Relaxed);
        let slot = slab[i].as_mut().expect("slot vivo");
        let pid = slot.proc.common.pid;
        slot.proc.state.store(RUNNING, Ordering::Release);
        if pid != last_running {
            *w.running.lock() = Some(slot.proc.common.clone());
            last_running = pid;
        }
        if shared.tls_swap {
            CURRENT_PID.with(|c| c.set(pid));
        }
        match slot.co.resume(()) {
            CoroutineResult::Yield(Suspend::Yield) => {
                slot.proc.state.store(RUNNABLE, Ordering::Release);
                runq.push_back(i);
            }
            CoroutineResult::Yield(Suspend::Block) => {
                if slot
                    .proc
                    .state
                    .compare_exchange(RUNNING, BLOCKED, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    // Acordado entre o poll e o suspend: volta pra fila.
                    slot.proc.state.store(RUNNABLE, Ordering::Release);
                    runq.push_back(i);
                }
            }
            CoroutineResult::Return(status) => {
                let slot = slab[i].take().expect("slot");
                free.push(i);
                live -= 1;
                slot.proc.state.store(DEAD, Ordering::Release);
                shared.core.finish(&slot.proc.common, status);
                drop(slot);
            }
        }
        w.queued.store(runq.len(), Ordering::Relaxed);
    }
    WORKER_ID.with(|c| c.set((0, usize::MAX)));
    let _ = live;
}

fn spawn(shared: &Arc<SharedB>, files: Vec<(Fd, File)>, main: ProcMain) -> Pid {
    let common = shared.core.new_proc(files);
    let pid = common.pid;
    let idx = shared.next.fetch_add(1, Ordering::Relaxed) % shared.workers.len();
    let proc = Arc::new(ProcB {
        common: common.clone(),
        shared: shared.clone(),
        worker: idx,
        state: AtomicU8::new(NEW),
        slot: AtomicUsize::new(usize::MAX),
    });
    common.set_waker(Waker::from(proc.clone()));
    shared.push_inbox(idx, move |ib| ib.spawns.push((proc, main)));
    pid
}

pub struct KernelB {
    shared: Arc<SharedB>,
    handles: Vec<JoinHandle<()>>,
    timer: Option<Timer>,
}

impl KernelB {
    pub fn new(cfg: ConfigB) -> KernelB {
        let core = Arc::new(Core::new());
        let workers = (0..cfg.workers)
            .map(|_| WorkerB {
                inbox: Mutex::new(Inbox::default()),
                inbox_flag: AtomicBool::new(false),
                thread: OnceLock::new(),
                idle: AtomicBool::new(false),
                queued: AtomicUsize::new(0),
                running: Mutex::new(None),
            })
            .collect();
        let shared = Arc::new(SharedB {
            core: core.clone(),
            id: NEXT_SHARED_ID.fetch_add(1, Ordering::Relaxed),
            workers,
            next: AtomicUsize::new(0),
            stack_size: cfg.stack_size,
            shutdown: AtomicBool::new(false),
            tls_swap: cfg.tls_swap,
            stack_failures: AtomicU64::new(0),
        });
        let handles = (0..cfg.workers)
            .map(|i| {
                let s = shared.clone();
                thread::Builder::new()
                    .name(format!("e01-b-worker-{i}"))
                    .spawn(move || worker_main(s, i))
                    .expect("worker")
            })
            .collect();
        // Espera os workers publicarem o handle de thread (pra `unpark` funcionar desde o início).
        while shared.workers.iter().any(|w| w.thread.get().is_none()) {
            thread::yield_now();
        }
        let timer = cfg.timer.then(|| Timer::start(core, shared.clone(), SLICE));
        KernelB { shared, handles, timer }
    }

    pub fn shared(&self) -> &Arc<SharedB> {
        &self.shared
    }
}

impl Drop for KernelB {
    fn drop(&mut self) {
        self.timer.take();
        self.shared.shutdown.store(true, Ordering::SeqCst);
        for w in &self.shared.workers {
            if let Some(t) = w.thread.get() {
                t.unpark();
            }
        }
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

impl crate::SyncKernel for KernelB {
    fn core(&self) -> &Arc<Core> {
        &self.shared.core
    }

    fn spawn(&self, files: Vec<(Fd, File)>, main: ProcMain) -> Pid {
        spawn(&self.shared, files, main)
    }
}
