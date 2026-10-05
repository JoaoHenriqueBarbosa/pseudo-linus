//! Criação de processos e threads, a entrada da thread do SO e o `execve` por dentro.
//!
//! - `fork` (`spawn_fn`) e `posix_spawn` (`spawn`) partem de [`fork_state`]: cópia de credenciais, cwd,
//!   umask, ambiente, rlimits, nice, disposições de sinal e da tabela de fds (os fds novos compartilham
//!   as descrições, com o FD_CLOEXEC de cada um). Depois [`apply_attrs`] aplica `ProcAttrs` na ordem: cwd,
//!   ações de fd, sinais, grupo e sessão.
//! - A thread do SO roda [`task_main`]: instala o processo na thread, roda o corpo sob `catch_unwind` e
//!   trata os payloads (`ExitUnwind`, `KillUnwind`, `ExecUnwind`, o [`GroupExitUnwind`] interno).

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use sysabi::{
    Errno, ExecUnwind, ExitUnwind, FdAction, KillUnwind, Mode, Pid, ProcAttrs, ProcessFn, ProcessGroup, Resource, Rlimit,
    Rusage, SigDisposition, Signal, ThreadFn, WaitStatus,
};
use vfs::{Caller, Cred, Loc, PinnedLoc, Start};

use crate::exec::{Image, comm_of};
use crate::fd::FdTable;
use crate::proc::{Entry, INIT_PID, PState, Proc, Rel, Task, finish_process};
use crate::sandbox::SbInner;
use crate::signal::SigState;

/// Payload interno: outra thread começou um `exit_group` (ou um `execve`) e esta thread desenrola.
pub(crate) struct GroupExitUnwind;

/// Corpo de um processo.
pub(crate) enum Body {
    Image(Image),
    Func(ProcessFn),
}

pub(crate) enum TaskBody {
    Main(Body),
    Thread(ThreadFn),
}

/// Estado inicial de um processo filho, ainda fora da tabela.
pub(crate) struct ChildSpec {
    pub cred: Arc<Cred>,
    pub cwd: Loc,
    pub root: Loc,
    pub umask: Mode,
    pub argv: Vec<Vec<u8>>,
    pub env: Vec<Vec<u8>>,
    pub comm: Vec<u8>,
    pub exe: Option<Loc>,
    pub rlimits: [Rlimit; 16],
    pub nice: i32,
    pub sig: SigState,
    pub fds: FdTable,
    pub group: ProcessGroup,
    pub new_session: bool,
    pub ppid: Pid,
    pub host_tracked: bool,
}

impl ChildSpec {
    /// Caller com o contexto do filho (pra resolver cwd e ações de fd).
    pub(crate) fn caller(&self, sb: &SbInner, pid_hint: Pid) -> Caller {
        Caller {
            cred: self.cred.clone(),
            root: self.root.clone(),
            cwd: self.cwd.clone(),
            umask: self.umask,
            now: sb.now(),
            pid: pid_hint,
            tid: pid_hint,
            fsize_limit: self.rlimits[Resource::Fsize as usize].cur,
        }
    }
}

/// Cópia do estado do processo de `parent` (o `copy_process` do fork).
pub(crate) fn fork_state(parent: &Task) -> ChildSpec {
    let p = &parent.proc;
    let st = p.st.lock();
    let fds = p.fds.lock().clone();
    let sig = p.sig.lock().forked();
    ChildSpec {
        cred: st.cred.clone(),
        cwd: st.cwd.loc().clone(),
        root: st.root.loc().clone(),
        umask: st.umask,
        argv: st.argv.clone(),
        env: st.env.clone(),
        comm: st.comm.clone(),
        exe: st.exe.as_ref().map(|e| e.loc().clone()),
        rlimits: st.rlimits,
        nice: st.nice,
        sig,
        fds,
        group: ProcessGroup::Inherit,
        new_session: false,
        ppid: p.pid,
        host_tracked: false,
    }
}

/// O filho herda a afinidade, a personality, a prioridade de E/S e a política de escalonamento do pai
/// (`copy_process`); `sched_fork` aplica o `reset_on_fork`: tempo real volta a `SCHED_OTHER` e nice
/// negativa vira 0. Chamado entre `insert_child` e `commit_exec`/`start_process`, quando a nice do filho
/// ainda não foi lida pelo escalonador.
pub(crate) fn inherit_tune(parent: &Task, child: &Arc<Task>) {
    let mut tune = parent.proc.tune.lock().clone();
    {
        let mut st = child.proc.st.lock();
        let (sched, nice) = tune.sched.fork(st.nice);
        tune.sched = sched;
        st.nice = nice;
    }
    *child.proc.tune.lock() = tune;
}

/// Aplica os atributos de `posix_spawn`/`spawn_fn` no filho: cwd, ações de fd (na ordem), sinais.
pub(crate) fn apply_attrs(opener: &Task, spec: &mut ChildSpec, attrs: &ProcAttrs) -> Result<(), Errno> {
    let sb = &opener.sb;
    if let Some(env) = &attrs.env {
        spec.env = env.clone();
    }
    if let Some(cwd) = &attrs.cwd {
        let cx = spec.caller(sb, 0);
        spec.cwd = sb.ns.chdir_target(&cx, &Start::Cwd, cwd)?;
    }
    let limit = spec.rlimits[Resource::Nofile as usize].cur;
    for act in &attrs.fd_actions {
        match act {
            FdAction::Dup2 { from, to } => {
                let ofd = spec.fds.ofd(*from)?;
                if to.0 < 0 || to.0 as u64 >= limit {
                    return Err(Errno::EBADF);
                }
                if from == to {
                    spec.fds.get_mut(*to)?.cloexec = false;
                } else {
                    let old = spec.fds.put(*to, ofd, false);
                    drop(old);
                }
            }
            FdAction::Close(fd) => {
                let old = spec.fds.remove(*fd);
                drop(old);
            }
            FdAction::Open { fd, path, flags, mode } => {
                if fd.0 < 0 || fd.0 as u64 >= limit {
                    return Err(Errno::EBADF);
                }
                let cx = spec.caller(sb, 0);
                let ofd = opener.open_ofd(&cx, &Start::Cwd, path, *flags, *mode)?;
                let old = spec.fds.put(*fd, ofd, flags.contains(sysabi::OFlags::CLOEXEC));
                drop(old);
            }
        }
    }
    for s in &attrs.reset_signals {
        if s.is_valid() && !s.is_uncatchable() {
            spec.sig.set_disposition(*s, SigDisposition::Default);
        }
    }
    for s in &attrs.ignore_signals {
        if s.is_valid() && !s.is_uncatchable() {
            spec.sig.set_disposition(*s, SigDisposition::Ignore);
        }
    }
    spec.group = attrs.group;
    spec.new_session = attrs.new_session;
    Ok(())
}

/// Põe o filho na tabela (pid, grupo, sessão, limites). Ainda sem thread.
pub(crate) fn insert_child(sb: &Arc<SbInner>, spec: ChildSpec) -> Result<Arc<Task>, Errno> {
    let mut t = sb.table.lock();
    if sb.destroyed.load(Ordering::Acquire) {
        return Err(Errno::EAGAIN);
    }
    if let Some(max) = sb.cfg.limits.max_procs
        && t.live >= max
    {
        return Err(Errno::EAGAIN);
    }
    let nproc = spec.rlimits[Resource::Nproc as usize].cur;
    if !spec.cred.is_root() && nproc != sysabi::RLIM_INFINITY {
        let uid = spec.cred.uid;
        let n = t.map.values().filter(|e| e.rel.zombie.is_none() && e.proc.pid != INIT_PID && e.proc.st.lock().cred.uid == uid).count();
        if n as u64 >= nproc {
            return Err(Errno::EAGAIN);
        }
    }
    let (ppgid, psid) = t.rel(spec.ppid).map(|r| (r.pgid, r.sid)).unwrap_or((INIT_PID, INIT_PID));
    if let ProcessGroup::Join(g) = spec.group
        && !spec.new_session
    {
        let ok = t.map.values().any(|e| e.rel.pgid == g && e.rel.sid == psid && e.rel.zombie.is_none());
        if !ok {
            return Err(Errno::EPERM);
        }
    }
    let pid = t.alloc_pid().ok_or(Errno::EAGAIN)?;
    let (pgid, sid) = if spec.new_session {
        (pid, pid)
    } else {
        match spec.group {
            ProcessGroup::Inherit => (ppgid, psid),
            ProcessGroup::New => (pid, psid),
            ProcessGroup::Join(g) => (g, psid),
        }
    };
    let st = PState {
        cred: spec.cred,
        cwd: PinnedLoc::new(spec.cwd),
        root: PinnedLoc::new(spec.root),
        umask: spec.umask,
        argv: spec.argv,
        env: spec.env,
        comm: spec.comm,
        exe: spec.exe.map(PinnedLoc::new),
        rlimits: spec.rlimits,
        nice: spec.nice,
        pending_exec: None,
    };
    let proc = Proc::new(pid, st, spec.fds, spec.sig, sb.mono_ns());
    let task = Task::new(pid, proc.clone(), sb.clone());
    proc.threads.lock().live.insert(pid, task.clone());
    t.map.insert(
        pid,
        Entry {
            proc,
            rel: Rel {
                ppid: spec.ppid,
                pgid,
                sid,
                children: Default::default(),
                zombie: None,
                stop_report: None,
                cont_report: false,
                host_tracked: spec.host_tracked,
                children_rusage: Rusage::default(),
            },
        },
    );
    if let Some(parent) = t.rel_mut(spec.ppid) {
        parent.children.insert(pid);
    }
    t.live += 1;
    t.forks += 1;
    Ok(task)
}

/// Desfaz um `insert_child` cuja thread não pôde ser criada.
fn rollback_child(sb: &Arc<SbInner>, task: &Arc<Task>) {
    let proc = task.proc.clone();
    proc.threads.lock().live.clear();
    let fds = proc.fds.lock().take_all();
    drop(fds);
    let mut t = sb.table.lock();
    if let Some(e) = t.map.remove(&proc.pid)
        && let Some(parent) = t.rel_mut(e.rel.ppid)
    {
        parent.children.remove(&proc.pid);
    }
    t.live = t.live.saturating_sub(1);
}

/// Cria a thread do SO de um processo novo.
pub(crate) fn start_process(sb: &Arc<SbInner>, task: Arc<Task>, body: Body) -> Result<(), Errno> {
    if matches!(body, Body::Func(_)) {
        // `fork` sem `execve`: o processo roda uma função do pai (`PF_FORKNOEXEC`).
        task.proc.fork_noexec.store(true, Ordering::Relaxed);
    }
    let name = format!("pl{}-{}", sb.id, task.proc.pid);
    let t2 = task.clone();
    match sb.spawn_os_thread(name, Box::new(move || task_main(t2, TaskBody::Main(body)))) {
        Ok(()) => Ok(()),
        Err(e) => {
            rollback_child(sb, &task);
            Err(e)
        }
    }
}

/// Cria uma thread nova no processo de `cur`.
pub(crate) fn start_thread(cur: &Task, body: ThreadFn) -> Result<Pid, Errno> {
    let sb = cur.sb.clone();
    let proc = cur.proc.clone();
    let tid = {
        let mut t = sb.table.lock();
        let tid = t.alloc_pid().ok_or(Errno::EAGAIN)?;
        t.tids.insert(tid);
        t.forks += 1;
        tid
    };
    let task = Task::new(tid, proc.clone(), sb.clone());
    proc.threads.lock().live.insert(tid, task.clone());
    // O espaço de endereçamento cresce com a pilha da thread nova: registra o pico.
    let _ = crate::procmem::snapshot(&proc);
    let name = format!("pl{}-{}-{}", sb.id, proc.pid, tid);
    let t2 = task.clone();
    if let Err(e) = sb.spawn_os_thread(name, Box::new(move || task_main(t2, TaskBody::Thread(body)))) {
        proc.threads.lock().live.remove(&tid);
        sb.table.lock().tids.remove(&tid);
        return Err(e);
    }
    Ok(tid)
}

enum Outcome {
    /// Uma thread secundária terminou normalmente.
    ThreadDone,
    Exit(i32),
    Killed(Signal),
    /// Outra thread terminou o processo (ou fez execve).
    GroupExit,
    Panic(String),
}

fn run_image(img: Image) -> i32 {
    let argv0 = img.argv.first().cloned().unwrap_or_default();
    let mut ctx = sysabi::Ctx::current(&argv0);
    let args = sysabi::ctx::to_os_args(&img.argv);
    (img.program.main)(&mut ctx, &args)
}

fn panic_message(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic sem mensagem".to_string()
    }
}

fn run_task(task: &Arc<Task>, body: TaskBody) -> Outcome {
    let mut next = Some(body);
    loop {
        let body = next.take().expect("corpo presente");
        let r = catch_unwind(AssertUnwindSafe(move || match body {
            TaskBody::Main(Body::Image(img)) => Outcome::Exit(run_image(img)),
            TaskBody::Main(Body::Func(f)) => Outcome::Exit(f()),
            TaskBody::Thread(f) => {
                f();
                Outcome::ThreadDone
            }
        }));
        match r {
            Ok(o) => return o,
            Err(p) => {
                if p.is::<ExecUnwind>() {
                    let img = task.proc.st.lock().pending_exec.take();
                    match img {
                        Some(img) => {
                            commit_exec(task, &img, true);
                            next = Some(TaskBody::Main(Body::Image(img)));
                            continue;
                        }
                        None => return Outcome::GroupExit,
                    }
                }
                if let Some(e) = p.downcast_ref::<ExitUnwind>() {
                    return Outcome::Exit(e.0);
                }
                if let Some(k) = p.downcast_ref::<KillUnwind>() {
                    return Outcome::Killed(k.0);
                }
                if p.is::<GroupExitUnwind>() {
                    return Outcome::GroupExit;
                }
                return Outcome::Panic(panic_message(&*p));
            }
        }
    }
}

/// Entrada da thread do SO de uma thread de processo.
pub(crate) fn task_main(task: Arc<Task>, body: TaskBody) {
    let cpus = task.sb.kernel.cpus.clone();
    task.ct.host_tid.store(rustix::thread::gettid().as_raw_nonzero().get(), Ordering::Relaxed);
    // Espera um token de CPU antes de rodar qualquer coisa do processo.
    let nice = task.proc.st.lock().nice;
    cpus.start(&task.ct, nice, task.sb.cpu_group);
    sysabi::sys::install(task.clone());
    let outcome = run_task(&task, body);
    let proc = task.proc.clone();
    let sb = task.sb.clone();
    match outcome {
        Outcome::ThreadDone | Outcome::GroupExit => {}
        Outcome::Exit(code) => proc.start_group_exit(WaitStatus::Exited(code & 0xff)),
        Outcome::Killed(sig) => proc.start_group_exit(WaitStatus::Signaled { signal: sig, core_dumped: false }),
        Outcome::Panic(_msg) => proc.start_group_exit(WaitStatus::Signaled { signal: Signal::SIGABRT, core_dumped: false }),
    }
    let cpu = cpus.runtime(&task.ct);
    proc.cpu_done_ns.fetch_add(cpu, Ordering::Relaxed);
    sb.cpu_done_ns.fetch_add(cpu, Ordering::Relaxed);
    let tid = task.tid();
    let last = {
        let mut th = proc.threads.lock();
        th.live.remove(&tid);
        if tid != proc.pid {
            th.done.insert(tid, false);
        }
        let w = th.joiners.take();
        let last = th.live.is_empty();
        drop(th);
        w.run();
        last
    };
    if tid != proc.pid {
        sb.table.lock().tids.remove(&tid);
    }
    if last {
        let status = proc.group_exit_status().unwrap_or(WaitStatus::Exited(0));
        let cpu_total = proc.cpu_done_ns.load(Ordering::Relaxed);
        let ru = Rusage { utime: std::time::Duration::from_nanos(cpu_total), stime: std::time::Duration::ZERO, maxrss_kib: 0 };
        finish_process(&sb, &proc, status, ru);
    }
    sysabi::sys::uninstall();
    cpus.exit(&task.ct);
}

/// Troca o programa do processo (`begin_new_exec`): mata as outras threads (`de_thread`), fecha os fds
/// com FD_CLOEXEC, volta capturados ao padrão, troca argv, ambiente, `comm` e `exe`.
pub(crate) fn commit_exec(task: &Arc<Task>, img: &Image, live: bool) {
    let proc = &task.proc;
    if live && proc.nthreads() > 1 {
        proc.exit.lock().exec_by = Some(task.tid());
        proc.kick_all();
        loop {
            {
                let mut th = proc.threads.lock();
                if th.live.len() <= 1 {
                    break;
                }
                th.joiners.register(&task.parker);
            }
            task.sleep(None);
        }
        proc.exit.lock().exec_by = None;
        let old = task.tid();
        if old != proc.pid {
            let mut th = proc.threads.lock();
            if let Some(t) = th.live.remove(&old) {
                th.live.insert(proc.pid, t);
            }
            drop(th);
            task.tid.store(proc.pid, Ordering::Relaxed);
            task.sb.table.lock().tids.remove(&old);
        }
    }
    let closed = proc.fds.lock().take_cloexec();
    drop(closed);
    proc.sig.lock().exec_reset();
    let mut st = proc.st.lock();
    st.argv = img.argv.clone();
    st.env = img.env.clone();
    st.comm = comm_of(&img.filename);
    st.exe = Some(PinnedLoc::new(img.exe.clone()));
    st.pending_exec = None;
    drop(st);
    // A personality atravessa o exec. Um exec que troca as credenciais (setuid, setgid) apagaria os bits de
    // `PER_CLEAR_ON_SETID`, mas o sandbox não tem exec setuid: o exec nunca é "secure".
    {
        let mut tune = proc.tune.lock();
        tune.personality = sysabi::sched::personality::after_exec(tune.personality, false);
    }
    // Imagem nova, `mm` novo: o pico de memória recomeça e o processo deixa de ser um fork puro.
    proc.fork_noexec.store(false, Ordering::Relaxed);
    proc.peak_size_kb.store(0, Ordering::Relaxed);
    proc.peak_rss_kb.store(0, Ordering::Relaxed);
}
