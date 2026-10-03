//! Peças comuns aos três modelos: tabela de processos, sinais, descritores, término, timer de fatia e a
//! sonda de latência de troca.
//!
//! Os modelos só diferem em como um processo espera (`block`), como cede a CPU virtual (`yield_now`) e
//! como é acordado (o `Waker` do processo). Pipe, sinais, `wait` e a tabela de descritores são os mesmos,
//! pra que a comparação meça o mecanismo de execução e não implementações diferentes do resto.

use std::any::Any;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::task::{Poll, Waker};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

use crate::pipe::{Pipe, PipeReader, PipeWriter};

pub type Pid = u32;
pub type Fd = usize;

pub const SIGABRT: i32 = 6;
pub const SIGKILL: i32 = 9;
pub const SIGPIPE: i32 = 13;
pub const SIGTERM: i32 = 15;

/// Como um processo terminou, do ponto de vista do pai.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExitStatus {
    Exited(i32),
    /// Morto por sinal (ação default), via unwind com [`KillUnwind`].
    Signaled(i32),
    /// Panic de verdade dentro do processo: o pai vê como término anormal (equivale a SIGABRT).
    Panicked(String),
}

impl ExitStatus {
    pub fn is_abnormal(&self) -> bool {
        !matches!(self, ExitStatus::Exited(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Errno {
    Badf,
    Pipe,
    Srch,
    Child,
}

pub type SysResult<T> = Result<T, Errno>;

/// Payload do unwind de término forçado. Lançado com `resume_unwind` (não passa pelo panic hook) e
/// capturado por `catch_unwind` na entrada do processo.
pub struct KillUnwind(pub i32);

/// Payload do `exit(código)` chamado de qualquer profundidade: desenrola a pilha (rodando os Drops) até a
/// entrada do processo.
pub struct ExitUnwind(pub i32);

/// `exit(2)` do pseudo-processo.
pub fn exit_process(code: i32) -> ! {
    resume_unwind(Box::new(ExitUnwind(code)))
}

/// Objeto aberto num descritor. O dono (`File`) conta leitores e escritores do pipe; o `Drop` dele é o
/// `close` implícito que roda quando o processo termina.
pub enum File {
    Read(PipeReader),
    Write(PipeWriter),
    /// Escoadouro que só conta bytes (o `/dev/null` da bancada); ler dele dá EOF.
    Sink(Arc<AtomicU64>),
}

impl Clone for File {
    fn clone(&self) -> File {
        match self {
            File::Read(r) => File::Read(r.clone()),
            File::Write(w) => File::Write(w.clone()),
            File::Sink(c) => File::Sink(c.clone()),
        }
    }
}

/// Referência barata a um objeto aberto, usada durante uma chamada sem segurar a tabela de descritores.
pub enum FileRef {
    Read(Arc<Pipe>),
    Write(Arc<Pipe>),
    Sink(Arc<AtomicU64>),
}

struct ExitState {
    status: Option<ExitStatus>,
    wakers: Vec<Waker>,
}

/// Estado de um processo comum aos três modelos.
pub struct ProcCommon {
    pub pid: Pid,
    /// Ligado pelo timer no fim da fatia e por sinal pendente. O caminho rápido do checkpoint só lê isto.
    pub attention: AtomicBool,
    /// Instante (ns desde a época do kernel) em que o timer ligou `attention` pela última vez.
    pub attention_set_ns: AtomicU64,
    pending: AtomicU32,
    fds: Mutex<Vec<Option<File>>>,
    exit: Mutex<ExitState>,
    exit_cv: Condvar,
    waker: Mutex<Option<Waker>>,
    /// tid da thread do host que executa o processo (modelo A); 0 nos outros.
    pub host_tid: AtomicI32,
}

impl ProcCommon {
    fn new(pid: Pid, files: Vec<(Fd, File)>) -> ProcCommon {
        let mut table: Vec<Option<File>> = Vec::new();
        for (fd, file) in files {
            if table.len() <= fd {
                table.resize_with(fd + 1, || None);
            }
            table[fd] = Some(file);
        }
        ProcCommon {
            pid,
            attention: AtomicBool::new(false),
            attention_set_ns: AtomicU64::new(0),
            pending: AtomicU32::new(0),
            fds: Mutex::new(table),
            exit: Mutex::new(ExitState { status: None, wakers: Vec::new() }),
            exit_cv: Condvar::new(),
            waker: Mutex::new(None),
            host_tid: AtomicI32::new(0),
        }
    }

    /// Instala o waker do processo (usado por `kill` pra tirá-lo de um bloqueio).
    pub fn set_waker(&self, waker: Waker) {
        *self.waker.lock() = Some(waker);
    }

    fn wake(&self) {
        let waker = self.waker.lock().clone();
        if let Some(w) = waker {
            w.wake();
        }
    }

    /// Marca um sinal pendente, liga `attention` e acorda o processo se estiver bloqueado.
    pub fn raise(&self, sig: i32) {
        self.pending.fetch_or(1 << sig, Ordering::AcqRel);
        self.attention.store(true, Ordering::Release);
        self.wake();
    }

    /// Sinal fatal pendente, se houver. Todos os sinais da bancada têm ação default de término, e SIGKILL
    /// tem prioridade.
    pub fn fatal_signal(&self) -> Option<i32> {
        let p = self.pending.load(Ordering::Acquire);
        if p == 0 {
            None
        } else if p & (1 << SIGKILL) != 0 {
            Some(SIGKILL)
        } else {
            Some(p.trailing_zeros() as i32)
        }
    }

    /// Ponto de entrega de sinal: se houver sinal fatal pendente, desenrola a pilha do processo.
    #[inline]
    pub fn check_signals(&self) {
        if let Some(sig) = self.fatal_signal() {
            resume_unwind(Box::new(KillUnwind(sig)));
        }
    }

    pub fn file_ref(&self, fd: Fd) -> SysResult<FileRef> {
        let fds = self.fds.lock();
        match fds.get(fd).and_then(Option::as_ref) {
            Some(File::Read(r)) => Ok(FileRef::Read(r.pipe().clone())),
            Some(File::Write(w)) => Ok(FileRef::Write(w.pipe().clone())),
            Some(File::Sink(c)) => Ok(FileRef::Sink(c.clone())),
            None => Err(Errno::Badf),
        }
    }

    pub fn dup_file(&self, fd: Fd) -> SysResult<File> {
        let fds = self.fds.lock();
        fds.get(fd).and_then(Option::as_ref).cloned().ok_or(Errno::Badf)
    }

    pub fn install(&self, file: File) -> Fd {
        let mut fds = self.fds.lock();
        if let Some(i) = fds.iter().position(Option::is_none) {
            fds[i] = Some(file);
            i
        } else {
            fds.push(Some(file));
            fds.len() - 1
        }
    }

    pub fn close(&self, fd: Fd) -> SysResult<()> {
        // O Drop do File roda fora do lock da tabela (ele pode acordar outros processos).
        let file = {
            let mut fds = self.fds.lock();
            fds.get_mut(fd).and_then(Option::take)
        };
        match file {
            Some(f) => {
                drop(f);
                Ok(())
            }
            None => Err(Errno::Badf),
        }
    }

    pub fn exit_status(&self) -> Option<ExitStatus> {
        self.exit.lock().status.clone()
    }
}

/// Sonda de latência: do timer ligar `attention` até o processo entrar no caminho lento (preempção) e até
/// o próximo processo estar rodando (handoff).
#[derive(Default)]
pub struct SwitchProbe {
    pub enabled: AtomicBool,
    handoff_from_ns: AtomicU64,
    pub preempt_ns: Mutex<Vec<u64>>,
    pub handoff_ns: Mutex<Vec<u64>>,
}

/// O núcleo compartilhado: tabela de processos e relógio.
pub struct Core {
    procs: Mutex<HashMap<Pid, Arc<ProcCommon>>>,
    next_pid: AtomicU32,
    pub epoch: Instant,
    pub probe: SwitchProbe,
    /// Trocas de processo efetivas (cessão com outro processo pronto), contadas pelos modelos.
    pub switches: AtomicU64,
}

impl Default for Core {
    fn default() -> Self {
        Core::new()
    }
}

impl Core {
    pub fn new() -> Core {
        Core {
            procs: Mutex::new(HashMap::new()),
            next_pid: AtomicU32::new(1),
            epoch: Instant::now(),
            probe: SwitchProbe::default(),
            switches: AtomicU64::new(0),
        }
    }

    #[inline]
    pub fn now_ns(&self) -> u64 {
        self.epoch.elapsed().as_nanos() as u64
    }

    pub fn new_proc(&self, files: Vec<(Fd, File)>) -> Arc<ProcCommon> {
        let pid = self.next_pid.fetch_add(1, Ordering::Relaxed);
        let proc = Arc::new(ProcCommon::new(pid, files));
        self.procs.lock().insert(pid, proc.clone());
        proc
    }

    pub fn lookup(&self, pid: Pid) -> Option<Arc<ProcCommon>> {
        self.procs.lock().get(&pid).cloned()
    }

    pub fn live_count(&self) -> usize {
        self.procs.lock().values().filter(|p| p.exit_status().is_none()).count()
    }

    pub fn kill(&self, pid: Pid, sig: i32) -> SysResult<()> {
        let proc = self.lookup(pid).ok_or(Errno::Srch)?;
        if proc.exit_status().is_some() {
            return Ok(());
        }
        proc.raise(sig);
        Ok(())
    }

    /// Término: fecha todos os descritores (o Drop acorda quem espera no pipe), publica o status e acorda
    /// quem espera em `wait`.
    pub fn finish(&self, proc: &ProcCommon, status: ExitStatus) {
        let files = std::mem::take(&mut *proc.fds.lock());
        drop(files);
        *proc.waker.lock() = None;
        let wakers = {
            let mut st = proc.exit.lock();
            st.status = Some(status);
            std::mem::take(&mut st.wakers)
        };
        proc.exit_cv.notify_all();
        for w in wakers {
            w.wake();
        }
    }

    /// `wait` do lado do host: bloqueia a thread do host até o processo terminar, e colhe o zumbi.
    pub fn wait_host(&self, pid: Pid) -> SysResult<ExitStatus> {
        let proc = self.lookup(pid).ok_or(Errno::Child)?;
        let status = {
            let mut st = proc.exit.lock();
            while st.status.is_none() {
                proc.exit_cv.wait(&mut st);
            }
            st.status.clone().expect("status")
        };
        self.procs.lock().remove(&pid);
        Ok(status)
    }

    /// Igual a `wait_host`, com prazo. `None` se o prazo venceu.
    pub fn wait_host_timeout(&self, pid: Pid, timeout: Duration) -> SysResult<Option<ExitStatus>> {
        let proc = self.lookup(pid).ok_or(Errno::Child)?;
        let deadline = Instant::now() + timeout;
        let status = {
            let mut st = proc.exit.lock();
            while st.status.is_none() {
                if proc.exit_cv.wait_until(&mut st, deadline).timed_out() {
                    break;
                }
            }
            st.status.clone()
        };
        if status.is_some() {
            self.procs.lock().remove(&pid);
        }
        Ok(status)
    }

    /// `wait` do lado de um processo: pronto se o filho terminou; senão registra o waker.
    pub fn poll_wait(&self, pid: Pid, waker: &Waker) -> Poll<SysResult<ExitStatus>> {
        let Some(proc) = self.lookup(pid) else { return Poll::Ready(Err(Errno::Child)) };
        let mut st = proc.exit.lock();
        if let Some(status) = st.status.clone() {
            drop(st);
            self.procs.lock().remove(&pid);
            return Poll::Ready(Ok(status));
        }
        crate::pipe::register(&mut st.wakers, waker);
        Poll::Pending
    }

    /// Caminho lento do checkpoint, comum aos modelos: apaga `attention`, entrega sinal fatal (unwind) e
    /// registra a latência de preempção. Devolve o instante em que o timer marcou o processo.
    pub fn ack_attention(&self, proc: &ProcCommon) -> u64 {
        proc.attention.swap(false, Ordering::AcqRel);
        let set_ns = proc.attention_set_ns.swap(0, Ordering::AcqRel);
        proc.check_signals();
        if set_ns != 0 && self.probe.enabled.load(Ordering::Relaxed) {
            let now = self.now_ns();
            self.probe.preempt_ns.lock().push(now.saturating_sub(set_ns));
        }
        set_ns
    }

    /// Chamado logo antes de ceder a CPU virtual por fim de fatia.
    #[inline]
    pub fn probe_before_yield(&self, set_ns: u64) {
        if set_ns != 0 && self.probe.enabled.load(Ordering::Relaxed) {
            self.probe.handoff_from_ns.store(set_ns, Ordering::Release);
        }
    }

    /// Chamado por um processo que acabou de voltar a rodar depois de ceder.
    #[inline]
    pub fn probe_after_resume(&self) {
        if self.probe.enabled.load(Ordering::Relaxed) {
            let from = self.probe.handoff_from_ns.swap(0, Ordering::AcqRel);
            if from != 0 {
                let now = self.now_ns();
                self.probe.handoff_ns.lock().push(now.saturating_sub(from));
            }
        }
    }
}

/// Liga `attention` de um processo que está rodando (chamado pelo timer). O instante é lido aqui, logo
/// antes de ligar o flag, pra que a latência medida não inclua o trabalho do timer antes disso.
#[inline]
pub fn mark_attention(proc: &ProcCommon, core: &Core) {
    if !proc.attention.load(Ordering::Relaxed) {
        proc.attention_set_ns.store(core.now_ns().max(1), Ordering::Relaxed);
        proc.attention.store(true, Ordering::Release);
    }
}

/// Executa o corpo de um processo capturando término por sinal e panic.
pub fn run_guarded<F: FnOnce() -> i32>(f: F) -> ExitStatus {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(code) => ExitStatus::Exited(code),
        Err(payload) => status_from_payload(payload),
    }
}

pub fn status_from_payload(payload: Box<dyn Any + Send>) -> ExitStatus {
    if let Some(k) = payload.downcast_ref::<KillUnwind>() {
        return ExitStatus::Signaled(k.0);
    }
    if let Some(e) = payload.downcast_ref::<ExitUnwind>() {
        return ExitStatus::Exited(e.0);
    }
    let msg = if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "payload de panic desconhecido".to_string()
    };
    ExitStatus::Panicked(msg)
}

/// O que o timer faz a cada fatia: cada modelo marca os processos que estão rodando nas CPUs virtuais.
pub trait Tick: Send + Sync {
    fn tick(&self, now_ns: u64);
}

/// Thread de timer que chama `tick` a cada `period`.
pub struct Timer {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Timer {
    pub fn start(core: Arc<Core>, target: Arc<dyn Tick>, period: Duration) -> Timer {
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let handle = std::thread::Builder::new()
            .name("e01-timer".into())
            .spawn(move || {
                while !stop2.load(Ordering::Relaxed) {
                    std::thread::sleep(period);
                    target.tick(core.now_ns());
                }
            })
            .expect("thread do timer");
        Timer { stop, handle: Some(handle) }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Fatia do timer da bancada.
pub const SLICE: Duration = Duration::from_millis(1);

/// Cria um pipe e devolve as duas pontas como `File`.
pub fn new_pipe() -> (File, File) {
    let (r, w) = Pipe::new_pair();
    (File::Read(r), File::Write(w))
}
