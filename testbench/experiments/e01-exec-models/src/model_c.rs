//! Modelo C: processo = future, executor próprio com N workers (sem tokio).
//!
//! Fila global com contagem de workers dormindo, mais um slot LIFO por worker (o truque do tokio pra
//! ping-pong: quem é acordado de dentro de um poll roda em seguida no mesmo worker, com limite de 16
//! seguidas pra não matar a fila global de fome). A migração entre workers é sound porque todo future é
//! `Send`. Kill: o executor vê o sinal antes do poll e descarta o future (os Drops rodam). Panic e
//! término por sinal dentro do poll são capturados com `catch_unwind`.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, JoinHandle};

use parking_lot::{Condvar, Mutex};

use crate::kernel::{
    Core, Errno, ExitStatus, Fd, File, FileRef, Pid, ProcCommon, SIGPIPE, SLICE, SysResult, Tick, Timer,
    mark_attention, new_pipe, status_from_payload,
};

pub type BoxFut = Pin<Box<dyn Future<Output = i32> + Send + 'static>>;

const LIFO_LIMIT: u32 = 16;

static NEXT_EXEC_ID: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static EXEC_ID: Cell<u64> = const { Cell::new(0) };
    static LIFO: RefCell<Option<Arc<TaskC>>> = const { RefCell::new(None) };
    static YIELD_REQ: Cell<bool> = const { Cell::new(false) };
    /// Task-local do processo corrente: o executor grava antes de cada poll e apaga depois.
    static CURRENT_PID: Cell<Pid> = const { Cell::new(0) };
    static WORKER_IDX: Cell<usize> = const { Cell::new(usize::MAX) };
}

/// Pid do processo corrente (task-local mantido pelo executor).
pub fn current_pid() -> Pid {
    CURRENT_PID.with(Cell::get)
}

/// Índice do worker que está executando o poll corrente.
pub fn current_worker() -> usize {
    WORKER_IDX.with(Cell::get)
}

#[derive(Clone, Debug)]
pub struct ConfigC {
    pub workers: usize,
    pub timer: bool,
}

struct QueueC {
    q: VecDeque<Arc<TaskC>>,
    sleepers: usize,
}

pub struct ExecC {
    pub core: Arc<Core>,
    id: u64,
    queue: Mutex<QueueC>,
    cv: Condvar,
    queued: AtomicUsize,
    live: AtomicUsize,
    shutdown: AtomicBool,
    running: Vec<Mutex<Option<Arc<ProcCommon>>>>,
}

pub struct TaskC {
    proc: Arc<ProcCommon>,
    fut: Mutex<Option<BoxFut>>,
    scheduled: AtomicBool,
    exec: Arc<ExecC>,
}

impl Wake for TaskC {
    fn wake(self: Arc<Self>) {
        if !self.scheduled.swap(true, Ordering::AcqRel) {
            let exec = self.exec.clone();
            exec.schedule(self);
        }
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if !self.scheduled.swap(true, Ordering::AcqRel) {
            self.exec.schedule(self.clone());
        }
    }
}

impl ExecC {
    fn schedule(&self, t: Arc<TaskC>) {
        if EXEC_ID.with(Cell::get) == self.id {
            let displaced = LIFO.with(|l| l.borrow_mut().replace(t));
            if let Some(d) = displaced {
                self.push_global(d);
            }
        } else {
            self.push_global(t);
        }
    }

    fn push_global(&self, t: Arc<TaskC>) {
        let mut q = self.queue.lock();
        q.q.push_back(t);
        self.queued.fetch_add(1, Ordering::Relaxed);
        if q.sleepers > 0 {
            self.cv.notify_one();
        }
    }

    fn pop_global(&self, idx: usize) -> Option<Arc<TaskC>> {
        let mut q = self.queue.lock();
        loop {
            if let Some(t) = q.q.pop_front() {
                self.queued.fetch_sub(1, Ordering::Relaxed);
                return Some(t);
            }
            if self.shutdown.load(Ordering::SeqCst) {
                return None;
            }
            *self.running[idx].lock() = None;
            q.sleepers += 1;
            self.cv.wait(&mut q);
            q.sleepers -= 1;
        }
    }

    fn has_other_work(&self) -> bool {
        self.queued.load(Ordering::Relaxed) > 0 || LIFO.with(|l| l.borrow().is_some())
    }

    fn complete(&self, task: &TaskC, status: ExitStatus) {
        self.core.finish(&task.proc, status);
        self.live.fetch_sub(1, Ordering::AcqRel);
    }

    fn run_task(&self, task: &Arc<TaskC>, idx: usize) {
        let mut slot = task.fut.lock();
        task.scheduled.store(false, Ordering::Release);
        let Some(fut) = slot.as_mut() else { return };
        if let Some(sig) = task.proc.fatal_signal() {
            let f = slot.take();
            drop(slot);
            drop(f);
            self.complete(task, ExitStatus::Signaled(sig));
            return;
        }
        {
            let mut r = self.running[idx].lock();
            if r.as_ref().map(|p| p.pid) != Some(task.proc.pid) {
                *r = Some(task.proc.clone());
            }
        }
        CURRENT_PID.with(|c| c.set(task.proc.pid));
        let waker = Waker::from(task.clone());
        let mut cx = Context::from_waker(&waker);
        let res = catch_unwind(AssertUnwindSafe(|| fut.as_mut().poll(&mut cx)));
        CURRENT_PID.with(|c| c.set(0));
        match res {
            Ok(Poll::Pending) => {
                drop(slot);
                if YIELD_REQ.with(|y| y.replace(false)) && !task.scheduled.swap(true, Ordering::AcqRel) {
                    self.push_global(task.clone());
                }
            }
            Ok(Poll::Ready(code)) => {
                let f = slot.take();
                drop(slot);
                drop(f);
                self.complete(task, ExitStatus::Exited(code));
            }
            Err(payload) => {
                YIELD_REQ.with(|y| y.set(false));
                let f = slot.take();
                drop(slot);
                drop(f);
                self.complete(task, status_from_payload(payload));
            }
        }
    }
}

impl Tick for ExecC {
    fn tick(&self, _now_ns: u64) {
        for r in &self.running {
            if let Some(p) = r.lock().as_ref() {
                mark_attention(p, &self.core);
            }
        }
    }
}

fn worker_main(exec: Arc<ExecC>, idx: usize) {
    EXEC_ID.with(|c| c.set(exec.id));
    WORKER_IDX.with(|c| c.set(idx));
    let mut streak = 0u32;
    loop {
        let mut next = LIFO.with(|l| l.borrow_mut().take());
        if next.is_some() {
            streak += 1;
            if streak > LIFO_LIMIT {
                exec.push_global(next.take().expect("lifo"));
                streak = 0;
            }
        } else {
            streak = 0;
        }
        let task = match next {
            Some(t) => t,
            None => match exec.pop_global(idx) {
                Some(t) => t,
                None => break,
            },
        };
        exec.run_task(&task, idx);
    }
    EXEC_ID.with(|c| c.set(0));
}

/// Future que devolve `Pending` uma vez e pede ao executor pra recolocar a tarefa no fim da fila global.
struct YieldOnce(bool);

impl Future for YieldOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            YIELD_REQ.with(|y| y.set(true));
            Poll::Pending
        }
    }
}

/// API de processo do modelo C: as mesmas chamadas dos modelos síncronos, em versão `async`.
#[derive(Clone)]
pub struct CtxC {
    proc: Arc<ProcCommon>,
    exec: Arc<ExecC>,
}

impl CtxC {
    pub fn pid(&self) -> Pid {
        self.proc.pid
    }

    pub fn proc(&self) -> &Arc<ProcCommon> {
        &self.proc
    }

    pub fn core(&self) -> &Core {
        &self.exec.core
    }

    #[inline(always)]
    pub fn need_checkpoint(&self) -> bool {
        self.proc.attention.load(Ordering::Relaxed)
    }

    #[inline(always)]
    pub async fn checkpoint(&self) {
        if self.need_checkpoint() {
            self.checkpoint_slow().await;
        }
    }

    pub async fn checkpoint_slow(&self) {
        let set_ns = self.exec.core.ack_attention(&self.proc);
        if self.exec.has_other_work() {
            self.exec.core.probe_before_yield(set_ns);
            self.exec.core.switches.fetch_add(1, Ordering::Relaxed);
            YieldOnce(false).await;
            self.exec.core.probe_after_resume();
        }
    }

    pub async fn yield_now(&self) {
        if self.exec.has_other_work() {
            self.exec.core.switches.fetch_add(1, Ordering::Relaxed);
            YieldOnce(false).await;
            self.exec.core.probe_after_resume();
        }
    }

    pub async fn read(&self, fd: Fd, buf: &mut [u8]) -> SysResult<usize> {
        self.proc.check_signals();
        match self.proc.file_ref(fd)? {
            FileRef::Read(pipe) => {
                let n = poll_fn(|cx| {
                    self.proc.check_signals();
                    pipe.poll_read(buf, cx.waker())
                })
                .await;
                Ok(n)
            }
            FileRef::Sink(_) => Ok(0),
            FileRef::Write(_) => Err(Errno::Badf),
        }
    }

    pub async fn write(&self, fd: Fd, data: &[u8]) -> SysResult<usize> {
        self.proc.check_signals();
        match self.proc.file_ref(fd)? {
            FileRef::Write(pipe) => {
                let mut off = 0;
                while off < data.len() {
                    let r = poll_fn(|cx| {
                        self.proc.check_signals();
                        pipe.poll_write(&data[off..], cx.waker())
                    })
                    .await;
                    match r {
                        Ok(n) => off += n,
                        Err(_) => {
                            self.proc.raise(SIGPIPE);
                            self.proc.check_signals();
                            return Err(Errno::Pipe);
                        }
                    }
                }
                Ok(off)
            }
            FileRef::Sink(c) => {
                c.fetch_add(data.len() as u64, Ordering::Relaxed);
                Ok(data.len())
            }
            FileRef::Read(_) => Err(Errno::Badf),
        }
    }

    pub fn close(&self, fd: Fd) -> SysResult<()> {
        self.proc.close(fd)
    }

    pub fn pipe(&self) -> (Fd, Fd) {
        let (r, w) = new_pipe();
        (self.proc.install(r), self.proc.install(w))
    }

    pub async fn wait(&self, pid: Pid) -> SysResult<ExitStatus> {
        self.proc.check_signals();
        poll_fn(|cx| {
            self.proc.check_signals();
            self.exec.core.poll_wait(pid, cx.waker())
        })
        .await
    }

    pub fn kill(&self, pid: Pid, sig: i32) -> SysResult<()> {
        self.exec.core.kill(pid, sig)
    }

    /// Termina o processo de qualquer profundidade (dentro do poll); o executor descarta o future.
    pub fn exit(&self, code: i32) -> ! {
        crate::kernel::exit_process(code)
    }

    pub fn spawn<F, Fut>(&self, inherit: &[(Fd, Fd)], f: F) -> SysResult<Pid>
    where
        F: FnOnce(CtxC) -> Fut,
        Fut: Future<Output = i32> + Send + 'static,
    {
        let mut files = Vec::with_capacity(inherit.len());
        for &(child_fd, parent_fd) in inherit {
            files.push((child_fd, self.proc.dup_file(parent_fd)?));
        }
        Ok(spawn(&self.exec, files, f))
    }
}

fn spawn<F, Fut>(exec: &Arc<ExecC>, files: Vec<(Fd, File)>, f: F) -> Pid
where
    F: FnOnce(CtxC) -> Fut,
    Fut: Future<Output = i32> + Send + 'static,
{
    let common = exec.core.new_proc(files);
    let pid = common.pid;
    let ctx = CtxC { proc: common.clone(), exec: exec.clone() };
    let fut: BoxFut = Box::pin(f(ctx));
    let task = Arc::new(TaskC {
        proc: common.clone(),
        fut: Mutex::new(Some(fut)),
        scheduled: AtomicBool::new(true),
        exec: exec.clone(),
    });
    common.set_waker(Waker::from(task.clone()));
    exec.live.fetch_add(1, Ordering::AcqRel);
    exec.push_global(task);
    pid
}

pub struct KernelC {
    exec: Arc<ExecC>,
    handles: Vec<JoinHandle<()>>,
    timer: Option<Timer>,
}

impl KernelC {
    pub fn new(cfg: ConfigC) -> KernelC {
        let core = Arc::new(Core::new());
        let exec = Arc::new(ExecC {
            core: core.clone(),
            id: NEXT_EXEC_ID.fetch_add(1, Ordering::Relaxed),
            queue: Mutex::new(QueueC { q: VecDeque::new(), sleepers: 0 }),
            cv: Condvar::new(),
            queued: AtomicUsize::new(0),
            live: AtomicUsize::new(0),
            shutdown: AtomicBool::new(false),
            running: (0..cfg.workers).map(|_| Mutex::new(None)).collect(),
        });
        let handles = (0..cfg.workers)
            .map(|i| {
                let e = exec.clone();
                thread::Builder::new()
                    .name(format!("e01-c-worker-{i}"))
                    .spawn(move || worker_main(e, i))
                    .expect("worker")
            })
            .collect();
        let timer = cfg.timer.then(|| Timer::start(core, exec.clone(), SLICE));
        KernelC { exec, handles, timer }
    }

    pub fn core(&self) -> &Arc<Core> {
        &self.exec.core
    }

    pub fn spawn<F, Fut>(&self, files: Vec<(Fd, File)>, f: F) -> Pid
    where
        F: FnOnce(CtxC) -> Fut,
        Fut: Future<Output = i32> + Send + 'static,
    {
        spawn(&self.exec, files, f)
    }

    pub fn wait(&self, pid: Pid) -> ExitStatus {
        self.exec.core.wait_host(pid).expect("wait")
    }

    pub fn live(&self) -> usize {
        self.exec.live.load(Ordering::Acquire)
    }
}

impl Drop for KernelC {
    fn drop(&mut self) {
        self.timer.take();
        self.exec.shutdown.store(true, Ordering::SeqCst);
        {
            let _q = self.exec.queue.lock();
            self.exec.cv.notify_all();
        }
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
        // Tarefas que sobraram (bloqueadas pra sempre): descarta os futures pra quebrar ciclos de Arc.
        let leftover: Vec<Arc<TaskC>> = self.exec.queue.lock().q.drain(..).collect();
        for t in leftover {
            let f = t.fut.lock().take();
            drop(f);
        }
    }
}
