//! Processos e threads.
//!
//! - [`Proc`] é o processo (grupo de threads): fds, cwd, credenciais, ambiente, sinais, rlimits.
//! - [`Task`] é uma thread do processo e é o objeto [`sysabi::Syscalls`] instalado na thread do SO. A
//!   thread principal tem tid = pid.
//! - [`Table`] é a tabela de processos do sandbox (o `tasklist_lock`): parentesco, grupos, sessões,
//!   zumbis e relatórios de parada e continuação pro `wait4`. O pid 1 é um init virtual (sem thread) que
//!   adota órfãos e colhe os zumbis deles, menos os processos que o host acompanha.
//!
//! Término: `exit`, morte por sinal ou fim do `main` em qualquer thread é um `exit_group`: as outras
//! threads desenrolam no próximo ponto de checagem, e a última a sair faz o término do processo (fecha
//! fds, vira zumbi, avisa o pai).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};

use parking_lot::Mutex;
use sysabi::{Gid, Mode, Pid, Resource, Rlimit, Rusage, SigDisposition, Signal, Tid, Uid, WaitStatus, RLIM_INFINITY};
use vfs::{Cred, PinnedLoc};

use crate::cpu::CpuTask;
use crate::exec::Image;
use crate::fd::FdTable;
use crate::park::{Parker, Wake, WaitList};
use crate::sandbox::SbInner;
use crate::signal::SigState;

/// `pid_max` do Debian 13 real.
pub(crate) const PID_MAX: Pid = 4_194_304;
/// Onde a numeração recomeça depois de dar a volta (`RESERVED_PIDS`).
const RESERVED_PIDS: Pid = 300;
pub(crate) const INIT_PID: Pid = 1;

/// rlimits padrão (os do container Debian da bancada, `/proc/self/limits`).
pub(crate) fn default_rlimits(nofile: u64, fsize: u64) -> [Rlimit; 16] {
    let inf = Rlimit { cur: RLIM_INFINITY, max: RLIM_INFINITY };
    let mut r = [inf; 16];
    r[Resource::Fsize as usize] = Rlimit { cur: fsize, max: fsize };
    r[Resource::Stack as usize] = Rlimit { cur: 8 << 20, max: RLIM_INFINITY };
    r[Resource::Nofile as usize] = Rlimit { cur: nofile, max: nofile };
    r[Resource::Memlock as usize] = Rlimit { cur: 8 << 20, max: 8 << 20 };
    r[Resource::Sigpending as usize] = Rlimit { cur: 127_077, max: 127_077 };
    r[Resource::Msgqueue as usize] = Rlimit { cur: 819_200, max: 819_200 };
    r[Resource::Nice as usize] = Rlimit { cur: 0, max: 0 };
    r[Resource::Rtprio as usize] = Rlimit { cur: 0, max: 0 };
    r
}

/// Estado do processo protegido por uma trava.
pub(crate) struct PState {
    pub cred: Arc<Cred>,
    pub cwd: PinnedLoc,
    pub root: PinnedLoc,
    pub umask: Mode,
    pub argv: Vec<Vec<u8>>,
    pub env: Vec<Vec<u8>>,
    /// `comm` (até 15 bytes).
    pub comm: Vec<u8>,
    pub exe: Option<PinnedLoc>,
    pub rlimits: [Rlimit; 16],
    pub nice: i32,
    /// Imagem a rodar depois de um `execve` bem-sucedido (o unwind leva até a entrada do processo).
    pub pending_exec: Option<Image>,
}

impl PState {
    pub(crate) fn getenv(&self, name: &[u8]) -> Option<Vec<u8>> {
        self.env.iter().find_map(|kv| {
            let eq = kv.iter().position(|b| *b == b'=')?;
            (&kv[..eq] == name).then(|| kv[eq + 1..].to_vec())
        })
    }
}

/// Como o processo está terminando.
#[derive(Debug, Default)]
pub(crate) struct ExitState {
    /// Status do `exit_group` em andamento.
    pub group: Option<WaitStatus>,
    /// Um `execve` de outra thread pediu pra esta sair.
    pub exec_by: Option<Tid>,
}

/// Threads de um processo.
#[derive(Default)]
pub(crate) struct Threads {
    pub live: BTreeMap<Tid, Arc<Task>>,
    /// Terminadas e ainda não juntadas (`false`) ou já juntadas (`true`).
    pub done: BTreeMap<Tid, bool>,
    pub joiners: WaitList,
}

/// Um processo.
pub(crate) struct Proc {
    pub pid: Pid,
    pub st: Mutex<PState>,
    pub fds: Mutex<FdTable>,
    pub sig: Mutex<SigState>,
    pub threads: Mutex<Threads>,
    pub exit: Mutex<ExitState>,
    /// Parado por SIGSTOP/SIGTSTP/SIGTTIN/SIGTTOU.
    pub stopped: AtomicBool,
    /// Threads do host esperando este processo terminar.
    pub host_waiters: Mutex<WaitList>,
    /// Instante de criação, em ns do relógio monotônico do sandbox.
    pub start_ns: u64,
    /// CPU das threads que já terminaram, em ns.
    pub cpu_done_ns: AtomicU64,
    /// Criado por `fork` (`spawn_fn`) e sem `execve` desde então (`PF_FORKNOEXEC`).
    pub fork_noexec: AtomicBool,
    /// Pico do espaço de endereçamento e do residente, em kB (`VmPeak` e `VmHWM`).
    pub peak_size_kb: AtomicU64,
    pub peak_rss_kb: AtomicU64,
}

impl Proc {
    pub(crate) fn new(pid: Pid, st: PState, fds: FdTable, sig: SigState, start_ns: u64) -> Arc<Proc> {
        Arc::new(Proc {
            pid,
            st: Mutex::new(st),
            fds: Mutex::new(fds),
            sig: Mutex::new(sig),
            threads: Mutex::new(Threads::default()),
            exit: Mutex::new(ExitState::default()),
            stopped: AtomicBool::new(false),
            host_waiters: Mutex::new(WaitList::default()),
            start_ns,
            cpu_done_ns: AtomicU64::new(0),
            fork_noexec: AtomicBool::new(false),
            peak_size_kb: AtomicU64::new(0),
            peak_rss_kb: AtomicU64::new(0),
        })
    }

    /// Liga a atenção e acorda todas as threads (sinal, parada, término).
    pub(crate) fn kick_all(&self) {
        let tasks: Vec<Arc<Task>> = self.threads.lock().live.values().cloned().collect();
        for t in tasks {
            t.attention.store(true, Ordering::Release);
            t.parker.unpark();
        }
    }

    /// A thread que recebe os sinais dirigidos ao processo (EINTR): a principal, ou a de menor tid se
    /// ela já saiu.
    pub(crate) fn signal_target(&self) -> Option<Tid> {
        let th = self.threads.lock();
        if th.live.contains_key(&self.pid) {
            return Some(self.pid);
        }
        th.live.keys().next().copied()
    }

    pub(crate) fn group_exit_status(&self) -> Option<WaitStatus> {
        self.exit.lock().group
    }

    /// Começa um `exit_group` (o primeiro status vale) e acorda as outras threads.
    pub(crate) fn start_group_exit(&self, status: WaitStatus) {
        {
            let mut e = self.exit.lock();
            if e.group.is_none() {
                e.group = Some(status);
            }
        }
        self.kick_all();
    }

    pub(crate) fn nthreads(&self) -> usize {
        self.threads.lock().live.len()
    }
}

/// Uma thread de um processo: é o objeto `Syscalls` instalado na thread do SO.
pub(crate) struct Task {
    pub tid: AtomicI32,
    pub proc: Arc<Proc>,
    pub sb: Arc<SbInner>,
    pub parker: Arc<Parker>,
    /// Caminho lento do checkpoint: sinal, parada, término, pedido de troca de CPU.
    pub attention: Arc<AtomicBool>,
    /// Dormindo numa espera (estado S).
    pub blocked: AtomicBool,
    /// O lado escalonável (token de CPU).
    pub ct: Arc<CpuTask>,
}

impl Task {
    pub(crate) fn new(tid: Tid, proc: Arc<Proc>, sb: Arc<SbInner>) -> Arc<Task> {
        let parker = Parker::new();
        let attention = Arc::new(AtomicBool::new(false));
        let ct = CpuTask::new(attention.clone(), sb.cpu_acct.clone());
        Arc::new(Task { tid: AtomicI32::new(tid), proc, sb, parker, attention, blocked: AtomicBool::new(false), ct })
    }

    pub(crate) fn tid(&self) -> Tid {
        self.tid.load(Ordering::Relaxed)
    }

    /// Dorme até um evento (ou o prazo): devolve a CPU antes e pede de volta depois. `false` = prazo.
    pub(crate) fn sleep(&self, deadline: Option<std::time::Instant>) -> bool {
        let cpus = &self.sb.kernel.cpus;
        cpus.sleep(&self.ct);
        self.blocked.store(true, Ordering::Relaxed);
        let woke = match deadline {
            Some(d) => self.parker.park_until(d),
            None => {
                self.parker.park();
                true
            }
        };
        self.blocked.store(false, Ordering::Relaxed);
        cpus.wake(&self.ct);
        woke
    }

    /// Tempo de CPU desta thread até agora, em ns.
    pub(crate) fn cpu_ns(&self) -> u64 {
        self.sb.kernel.cpus.runtime(&self.ct)
    }
}

impl Proc {
    /// CPU do processo: threads que terminaram mais as vivas.
    pub(crate) fn cpu_ns(&self) -> u64 {
        let live: Vec<Arc<Task>> = self.threads.lock().live.values().cloned().collect();
        self.cpu_done_ns.load(Ordering::Relaxed) + live.iter().map(|t| t.cpu_ns()).sum::<u64>()
    }
}

/// Parentesco e estado de um processo na tabela.
#[derive(Debug, Clone)]
pub(crate) struct Rel {
    pub ppid: Pid,
    pub pgid: Pid,
    pub sid: Pid,
    pub children: BTreeSet<Pid>,
    pub zombie: Option<(WaitStatus, Rusage)>,
    /// Parada ainda não relatada ao pai (`WUNTRACED`).
    pub stop_report: Option<Signal>,
    /// Continuação ainda não relatada (`WCONTINUED`).
    pub cont_report: bool,
    /// O host espera este processo (o init não colhe).
    pub host_tracked: bool,
    /// Uso de CPU dos filhos já colhidos (`RUSAGE_CHILDREN`).
    pub children_rusage: Rusage,
}

pub(crate) struct Entry {
    pub proc: Arc<Proc>,
    pub rel: Rel,
}

/// A tabela de processos de um sandbox.
pub(crate) struct Table {
    pub map: BTreeMap<Pid, Entry>,
    /// tids de threads secundárias vivas (dividem o espaço de pids).
    pub tids: BTreeSet<Tid>,
    next_pid: Pid,
    /// A numeração já deu a volta em `pid_max`.
    wrapped: bool,
    /// Processos vivos, sem contar o init e os zumbis.
    pub live: u32,
    /// Processos e threads criados desde o boot do sandbox (`total_forks`).
    pub forks: u64,
}

impl Table {
    pub(crate) fn new(init: Arc<Proc>) -> Table {
        let mut map = BTreeMap::new();
        map.insert(
            INIT_PID,
            Entry {
                proc: init,
                rel: Rel {
                    ppid: 0,
                    pgid: INIT_PID,
                    sid: INIT_PID,
                    children: BTreeSet::new(),
                    zombie: None,
                    stop_report: None,
                    cont_report: false,
                    host_tracked: false,
                    children_rusage: Rusage::default(),
                },
            },
        );
        Table { map, tids: BTreeSet::new(), next_pid: INIT_PID + 1, wrapped: false, live: 0, forks: 0 }
    }

    /// Último pid alocado (o `last_pid` do namespace de pids), 1 se só o init existiu.
    pub(crate) fn last_pid(&self) -> Pid {
        self.next_pid - 1
    }

    /// Próximo pid livre (`alloc_pid`): sobe até `pid_max`, dá a volta pra 300, pula pids em uso como
    /// processo, thread, grupo ou sessão.
    pub(crate) fn alloc_pid(&mut self) -> Option<Pid> {
        if !self.wrapped {
            // Antes da primeira volta nenhum pid acima de next_pid foi usado.
            if self.next_pid < PID_MAX {
                let p = self.next_pid;
                self.next_pid += 1;
                return Some(p);
            }
            self.wrapped = true;
            self.next_pid = RESERVED_PIDS;
        }
        let mut used: BTreeSet<Pid> = self.tids.clone();
        for (p, e) in &self.map {
            used.insert(*p);
            used.insert(e.rel.pgid);
            used.insert(e.rel.sid);
        }
        let start = self.next_pid;
        let mut p = start;
        loop {
            if p >= PID_MAX {
                p = RESERVED_PIDS;
            }
            if !used.contains(&p) {
                self.next_pid = p + 1;
                return Some(p);
            }
            p += 1;
            if p == start {
                return None;
            }
        }
    }

    pub(crate) fn rel(&self, pid: Pid) -> Option<&Rel> {
        self.map.get(&pid).map(|e| &e.rel)
    }

    pub(crate) fn rel_mut(&mut self, pid: Pid) -> Option<&mut Rel> {
        self.map.get_mut(&pid).map(|e| &mut e.rel)
    }

    pub(crate) fn proc(&self, pid: Pid) -> Option<Arc<Proc>> {
        self.map.get(&pid).map(|e| e.proc.clone())
    }

    /// Remove um zumbi (colhido).
    pub(crate) fn reap(&mut self, pid: Pid) -> Option<(WaitStatus, Rusage)> {
        let e = self.map.remove(&pid)?;
        if let Some(parent) = self.map.get_mut(&e.rel.ppid) {
            parent.rel.children.remove(&pid);
            if let Some((_, ru)) = &e.rel.zombie {
                parent.rel.children_rusage.utime += ru.utime + e.rel.children_rusage.utime;
                parent.rel.children_rusage.stime += ru.stime + e.rel.children_rusage.stime;
                parent.rel.children_rusage.maxrss_kib = parent.rel.children_rusage.maxrss_kib.max(ru.maxrss_kib);
            }
        }
        e.rel.zombie
    }

    /// Processos (não zumbis) de um grupo.
    pub(crate) fn group_members(&self, pgid: Pid) -> Vec<Arc<Proc>> {
        self.map.values().filter(|e| e.rel.pgid == pgid && e.rel.zombie.is_none() && e.proc.pid != INIT_PID).map(|e| e.proc.clone()).collect()
    }
}

/// Uid e gid de quem manda um sinal, pra checar permissão (`kill_ok_by_cred`).
pub(crate) fn may_signal(sender: &Cred, target_uid: Uid, _target_gid: Gid) -> bool {
    sender.is_root() || sender.uid == target_uid
}

/// Termina o processo depois que a última thread saiu: fecha fds, vira zumbi, reparenta os filhos pro
/// init e avisa o pai.
pub(crate) fn finish_process(sb: &Arc<SbInner>, proc: &Arc<Proc>, status: WaitStatus, rusage: Rusage) {
    // Fecha os fds fora de qualquer trava (fechar pode acordar outros processos).
    let fds = proc.fds.lock().take_all();
    drop(fds);
    let exe = proc.st.lock().exe.take();
    drop(exe);
    let mut wake = Wake::none();
    let mut sigchld_to: Option<Arc<Proc>> = None;
    {
        let mut t = sb.table.lock();
        let pid = proc.pid;
        let kids: Vec<Pid> = match t.rel_mut(pid) {
            Some(r) => std::mem::take(&mut r.children).into_iter().collect(),
            None => Vec::new(),
        };
        for k in kids {
            let reap_now = {
                let Some(r) = t.rel_mut(k) else { continue };
                r.ppid = INIT_PID;
                r.zombie.is_some() && !r.host_tracked
            };
            if let Some(init) = t.rel_mut(INIT_PID) {
                init.children.insert(k);
            }
            if reap_now {
                t.reap(k);
            }
        }
        t.live = t.live.saturating_sub(1);
        let (ppid, host_tracked) = match t.rel_mut(pid) {
            Some(r) => {
                r.zombie = Some((status, rusage.clone()));
                r.stop_report = None;
                r.cont_report = false;
                (r.ppid, r.host_tracked)
            }
            None => (INIT_PID, false),
        };
        if ppid == INIT_PID {
            if !host_tracked {
                t.reap(pid);
            }
        } else if let Some(parent) = t.proc(ppid) {
            let ignores = parent.sig.lock().disposition(Signal::SIGCHLD) == SigDisposition::Ignore;
            if ignores {
                // SIGCHLD ignorado explicitamente: o filho não vira zumbi.
                t.reap(pid);
            }
            sigchld_to = Some(parent);
        }
    }
    wake.merge(proc.host_waiters.lock().take());
    wake.run();
    if let Some(parent) = sigchld_to {
        crate::sys::generate_signal(&parent, Signal::SIGCHLD);
        parent.kick_all();
    }
    sb.proc_exited();
}
