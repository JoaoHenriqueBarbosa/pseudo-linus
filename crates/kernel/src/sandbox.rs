//! Um sandbox: namespace de montagens (tmpfs na raiz, tmpfs em `/dev`, procfs em `/proc`), tabela de
//! processos com o init virtual, relógio, a thread spawner (E06) e as tabelas de FIFOs e travas OFD.
//! [`Sandbox`] é a alça pública do host.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use sysabi::{
    AtFlags, DirEntry, Errno, FileLock, FileType, KillTarget, LockKind, Mode, OFlags, Pid, ProcInfo, Program,
    RenameFlags, Rusage, SetTime, Signal, Stat, TimeSpec, WaitStatus,
};
use vfs::procfs::Procfs;
use vfs::tmpfs::{Tmpfs, TmpfsLimits, TmpfsSnapshot};
use vfs::{Caller, Cred, MountFlags, Namespace, Opened, PinnedLoc, Start, WritePos};

use crate::config::{ClockMode, SandboxConfig, SpawnerInfo};
use crate::cpu::CpuAcct;
use crate::fd::FdTable;
use crate::image;
use crate::kernel::KernelInner;
use crate::loadavg::LoadAvg;
use crate::park::{Parker, WaitList};
use crate::pipe::Pipe;
use crate::proc::{INIT_PID, PState, Proc, Table, default_rlimits};
use crate::procinfo::SbProcProvider;
use crate::signal::SigState;
use crate::tty::Pty;

thread_local! {
    /// Sandbox a que a thread corrente pertence (0 = thread do host).
    static THREAD_SANDBOX: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

pub(crate) fn thread_sandbox() -> u64 {
    THREAD_SANDBOX.with(|c| c.get())
}

type Job = (String, usize, Box<dyn FnOnce() + Send>, mpsc::SyncSender<Result<(), Errno>>);

/// A thread spawner do sandbox: criada uma vez, recebe o hook do host (Landlock e seccomp) e cria as
/// threads dos processos que o host inicia. Threads criadas por um processo do sandbox nascem direto da
/// thread dele, que já descende da spawner e herda as restrições.
struct Spawner {
    tx: Mutex<Option<mpsc::Sender<Job>>>,
}

impl Spawner {
    fn start(id: u64, hook: Option<crate::config::SpawnerHook>, info: SpawnerInfo) -> Result<Spawner, String> {
        let (tx, rx) = mpsc::channel::<Job>();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);
        std::thread::Builder::new()
            .name(format!("pl-spawner-{id}"))
            .spawn(move || {
                if let Some(h) = hook
                    && let Err(e) = h(&info)
                {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
                THREAD_SANDBOX.with(|c| c.set(id));
                let _ = ready_tx.send(Ok(()));
                for (name, stack, f, reply) in rx {
                    let r = std::thread::Builder::new().name(name).stack_size(stack).spawn(f);
                    let _ = reply.send(r.map(|_| ()).map_err(|_| Errno::EAGAIN));
                }
            })
            .map_err(|e| format!("thread spawner: {e}"))?;
        ready_rx.recv().map_err(|_| "a thread spawner morreu antes de ficar pronta".to_string())??;
        Ok(Spawner { tx: Mutex::new(Some(tx)) })
    }

    fn stop(&self) {
        self.tx.lock().take();
    }
}

/// Travas OFD (`F_OFD_SETLK`) de um sandbox, por inode.
#[derive(Default)]
pub(crate) struct LockTable {
    inner: Mutex<LockInner>,
}

#[derive(Default)]
struct LockInner {
    map: HashMap<(u64, u64), Vec<LockRec>>,
    waiters: WaitList,
}

#[derive(Clone, Copy, Debug)]
struct LockRec {
    owner: u64,
    write: bool,
    start: u64,
    /// Exclusivo; `u64::MAX` = até o infinito.
    end: u64,
}

fn lock_range(l: &FileLock) -> (u64, u64) {
    let end = if l.len == 0 { u64::MAX } else { l.start.saturating_add(l.len) };
    (l.start, end)
}

impl LockTable {
    /// Primeira trava de outro dono que conflita.
    pub(crate) fn getlk(&self, key: (u64, u64), owner: u64, l: &FileLock) -> Option<FileLock> {
        let (s, e) = lock_range(l);
        let want_write = l.kind == LockKind::Write;
        let g = self.inner.lock();
        let recs = g.map.get(&key)?;
        recs.iter().find(|r| r.owner != owner && r.start < e && s < r.end && (r.write || want_write)).map(|r| FileLock {
            kind: if r.write { LockKind::Write } else { LockKind::Read },
            start: r.start,
            len: if r.end == u64::MAX { 0 } else { r.end - r.start },
        })
    }

    /// Aplica (ou solta) a trava; `Err(())` se conflita. Registra o parker pra esperar quando pedido.
    pub(crate) fn setlk(&self, key: (u64, u64), owner: u64, l: &FileLock, waiter: Option<&Arc<Parker>>) -> Result<(), ()> {
        let (s, e) = lock_range(l);
        let mut g = self.inner.lock();
        if l.kind != LockKind::Unlock {
            let want_write = l.kind == LockKind::Write;
            let conflict = g
                .map
                .get(&key)
                .is_some_and(|recs| recs.iter().any(|r| r.owner != owner && r.start < e && s < r.end && (r.write || want_write)));
            if conflict {
                if let Some(w) = waiter {
                    g.waiters.register(w);
                }
                return Err(());
            }
        }
        let recs = g.map.entry(key).or_default();
        // Recorta as travas do próprio dono na faixa.
        let mut out = Vec::with_capacity(recs.len() + 2);
        for r in recs.drain(..) {
            if r.owner != owner || r.end <= s || e <= r.start {
                out.push(r);
                continue;
            }
            if r.start < s {
                out.push(LockRec { end: s, ..r });
            }
            if e < r.end {
                out.push(LockRec { start: e, ..r });
            }
        }
        if l.kind != LockKind::Unlock {
            out.push(LockRec { owner, write: l.kind == LockKind::Write, start: s, end: e });
        }
        // Junta faixas adjacentes do mesmo dono e tipo.
        out.sort_by_key(|r| (r.owner, r.write, r.start));
        let mut merged: Vec<LockRec> = Vec::with_capacity(out.len());
        for r in out {
            if let Some(last) = merged.last_mut()
                && last.owner == r.owner
                && last.write == r.write
                && last.end >= r.start
            {
                last.end = last.end.max(r.end);
                continue;
            }
            merged.push(r);
        }
        let empty = merged.is_empty();
        *recs = merged;
        if empty {
            g.map.remove(&key);
        }
        let w = g.waiters.take();
        drop(g);
        w.run();
        Ok(())
    }

    pub(crate) fn unregister(&self, waiter: &Arc<Parker>) {
        self.inner.lock().waiters.unregister(waiter);
    }

    /// Solta tudo de um dono (último close da descrição).
    pub(crate) fn release_owner(&self, owner: u64) {
        let mut g = self.inner.lock();
        let mut changed = false;
        g.map.retain(|_, recs| {
            let before = recs.len();
            recs.retain(|r| r.owner != owner);
            changed |= recs.len() != before;
            !recs.is_empty()
        });
        if changed {
            let w = g.waiters.take();
            drop(g);
            w.run();
        }
    }
}

struct ClockState {
    mode: ClockMode,
    set_at: Instant,
}

pub(crate) struct SbInner {
    pub id: u64,
    pub kernel: Arc<KernelInner>,
    pub ns: Arc<Namespace>,
    pub rootfs: Arc<Tmpfs>,
    pub devfs: Arc<Tmpfs>,
    pub workfs: Arc<Tmpfs>,
    programs: HashMap<Vec<u8>, Program>,
    /// Build-id do ELF de cada programa embutido -> caminho na tabela (ver `exec::builtin_file`).
    builtin_ids: HashMap<[u8; 20], Vec<u8>>,
    pub table: Mutex<Table>,
    pub hostname: Mutex<Vec<u8>>,
    /// `domainname` do UTS (`/proc/sys/kernel/domainname`); o Linux começa com `(none)`.
    pub domainname: Mutex<Vec<u8>>,
    clock: Mutex<ClockState>,
    pub boot: Instant,
    pub cfg: SandboxConfig,
    spawner: Spawner,
    pub fifos: Mutex<HashMap<(u64, u64), Weak<Pipe>>>,
    pub locks: Arc<LockTable>,
    pub destroyed: AtomicBool,
    /// Grupo de CPU do sandbox (filho do grupo do usuário).
    pub cpu_group: sched::GroupId,
    /// CPU das threads que já terminaram, em ns.
    pub cpu_done_ns: std::sync::atomic::AtomicU64,
    /// Tempos de CPU por CPU virtual e trocas de contexto, pro `/proc/stat` do sandbox.
    pub cpu_acct: Arc<CpuAcct>,
    /// Médias de carga do sandbox (`/proc/loadavg`).
    pub load: Arc<LoadAvg>,
    /// Threads do host esperando algum processo terminar (destroy).
    exit_waiters: Mutex<WaitList>,
    /// Pseudoterminais vivos, pelo número (o `/dev/pts/N`). O número só volta a ficar livre quando
    /// todas as pontas do par fecharam.
    pub ptys: Mutex<BTreeMap<u32, Weak<Pty>>>,
    /// Portas TCP em escuta no loopback.
    pub ports: Arc<crate::net::Ports>,
    /// Sockets do domínio Unix e os nomes ligados a eles.
    pub unix: Arc<crate::unix::UnixTable>,
}

impl SbInner {
    /// Relógio de parede do sandbox.
    pub(crate) fn now(&self) -> TimeSpec {
        let c = self.clock.lock();
        match c.mode {
            ClockMode::Host => {
                let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
                TimeSpec::from_duration_since_epoch(d)
            }
            ClockMode::Fixed(t) => t,
            ClockMode::StartAt(t) => {
                let e = c.set_at.elapsed();
                let total = t.nsec as u128 + e.subsec_nanos() as u128;
                TimeSpec { sec: t.sec + e.as_secs() as i64 + (total / 1_000_000_000) as i64, nsec: (total % 1_000_000_000) as u32 }
            }
        }
    }

    /// Relógio monotônico do sandbox (desde o boot dele), em ns.
    pub(crate) fn mono_ns(&self) -> u64 {
        u64::try_from(self.boot.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    pub(crate) fn program(&self, path: &[u8]) -> Option<Program> {
        self.programs.get(path).copied()
    }

    /// O programa embutido que um ELF com esse build-id roda. O `true` real tem o build-id do
    /// Debian e roda o `/usr/bin/true` da tabela.
    pub(crate) fn builtin_path(&self, id: &[u8; 20]) -> Option<Vec<u8>> {
        if crate::exec::is_real_true_id(id) {
            return Some(b"/usr/bin/true".to_vec());
        }
        self.builtin_ids.get(id).cloned()
    }

    /// Caller de root com cwd em `/` (operações diretas do host e montagem da imagem).
    pub(crate) fn root_caller(&self) -> Caller {
        let mut cx = vfs::ops::kernel_caller(self.ns.root());
        cx.now = self.now();
        cx
    }

    /// Cria a thread do SO de um processo: direto, se quem pede já é thread deste sandbox (herda as
    /// restrições); senão pela spawner.
    pub(crate) fn spawn_os_thread(&self, name: String, f: Box<dyn FnOnce() + Send>) -> Result<(), Errno> {
        let id = self.id;
        let stack = self.kernel.config.stack_size;
        let wrapped: Box<dyn FnOnce() + Send> = Box::new(move || {
            THREAD_SANDBOX.with(|c| c.set(id));
            f();
        });
        if thread_sandbox() == id {
            return std::thread::Builder::new().name(name).stack_size(stack).spawn(wrapped).map(|_| ()).map_err(|_| Errno::EAGAIN);
        }
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        {
            let g = self.spawner.tx.lock();
            let tx = g.as_ref().ok_or(Errno::EAGAIN)?;
            tx.send((name, stack, wrapped, reply_tx)).map_err(|_| Errno::EAGAIN)?;
        }
        reply_rx.recv().map_err(|_| Errno::EAGAIN)?
    }

    /// Avisa quem espera que um processo terminou.
    pub(crate) fn proc_exited(&self) {
        self.exit_waiters.lock().take().run();
    }

    /// Pipe por trás de uma FIFO (um por inode enquanto houver ponta aberta).
    pub(crate) fn fifo_pipe(&self, key: (u64, u64), cred: &Cred) -> Arc<Pipe> {
        let mut g = self.fifos.lock();
        if let Some(p) = g.get(&key).and_then(Weak::upgrade) {
            return p;
        }
        g.retain(|_, w| w.strong_count() > 0);
        let p = Pipe::new(self.kernel.pipe_ino(), cred.uid, cred.gid, self.now());
        g.insert(key, Arc::downgrade(&p));
        p
    }

    /// Pty novo com o menor número livre (`devpts_new_index`); ENOSPC passado o máximo.
    pub(crate) fn alloc_pty(self: &Arc<Self>) -> Result<Arc<Pty>, Errno> {
        let mut g = self.ptys.lock();
        g.retain(|_, w| w.strong_count() > 0);
        let idx = (0..crate::tty::PTY_MAX).find(|i| !g.contains_key(i)).ok_or(Errno::ENOSPC)?;
        let p = Pty::new(idx, Arc::downgrade(self));
        g.insert(idx, Arc::downgrade(&p));
        Ok(p)
    }

    pub(crate) fn pty(&self, idx: u32) -> Option<Arc<Pty>> {
        self.ptys.lock().get(&idx).and_then(Weak::upgrade)
    }

    /// O terminal de controle da sessão `sid`.
    pub(crate) fn pty_of_session(&self, sid: Pid) -> Option<Arc<Pty>> {
        let ptys: Vec<Arc<Pty>> = self.ptys.lock().values().filter_map(Weak::upgrade).collect();
        ptys.into_iter().find(|p| p.master_open() && p.session() == Some(sid))
    }
}

/// Retrato do sistema de arquivos de um sandbox.
#[derive(Clone, Debug)]
pub struct Snapshot {
    root: TmpfsSnapshot,
    dev: TmpfsSnapshot,
    work: TmpfsSnapshot,
}

/// Uso de recursos de um sandbox.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// Processos vivos (sem o init e sem zumbis).
    pub procs: u32,
    /// Bytes de conteúdo no tmpfs da raiz e do `/dev`.
    pub fs_bytes: u64,
    pub fs_inodes: u64,
    /// CPU já consumida pelos processos que terminaram e pelos vivos (contabilidade disponível).
    pub cpu_ns: u64,
}

/// Uma entrada da árvore devolvida por [`SandboxFs::tree`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeEntry {
    File { data: Vec<u8>, mode: Mode },
    Dir { mode: Mode },
    Symlink { target: Vec<u8> },
    Other { mode: Mode },
}

/// Entrada de `readdir` do host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsEntry {
    pub name: Vec<u8>,
    pub ino: u64,
    pub kind: FileType,
}

/// Como [`SandboxFs::write`] abre o arquivo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteMode {
    /// Cria se não existe e trunca (`>`).
    Truncate,
    /// Cria se não existe e escreve no fim (`>>`).
    Append,
    /// Só cria; EEXIST se já existe.
    CreateNew,
    /// Arquivo existente, escrevendo a partir do deslocamento dado.
    At(u64),
}

/// Um sandbox.
pub struct Sandbox {
    pub(crate) inner: Arc<SbInner>,
}

impl std::fmt::Debug for Sandbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sandbox({})", self.inner.id)
    }
}

/// Por que um sandbox não pôde ser criado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreateError {
    Errno(Errno),
    /// O hook da thread spawner falhou (Landlock ou seccomp do host).
    Spawner(String),
}

impl From<Errno> for CreateError {
    fn from(e: Errno) -> Self {
        CreateError::Errno(e)
    }
}

impl std::fmt::Display for CreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreateError::Errno(e) => write!(f, "{}", e.message()),
            CreateError::Spawner(s) => write!(f, "spawner: {s}"),
        }
    }
}

impl std::error::Error for CreateError {}

impl Sandbox {
    pub(crate) fn create(kernel: Arc<KernelInner>, mut cfg: SandboxConfig, from: Option<&Snapshot>) -> Result<Sandbox, CreateError> {
        let id = kernel.sandbox_id();
        let boot = Instant::now();
        let clock = Mutex::new(ClockState { mode: cfg.clock, set_at: boot });
        let wall = ClockState { mode: cfg.clock, set_at: boot };
        let now = {
            let tmp = Mutex::new(wall);
            let c = tmp.lock();
            match c.mode {
                ClockMode::Host => TimeSpec::from_duration_since_epoch(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()),
                ClockMode::Fixed(t) | ClockMode::StartAt(t) => t,
            }
        };
        let root_limits = TmpfsLimits {
            max_blocks: cfg.limits.fs_bytes.map(|b| b.div_ceil(4096)),
            max_inodes: cfg.limits.fs_inodes,
        };
        let dev_limits = TmpfsLimits { max_blocks: Some(65536 / 4), max_inodes: None };
        let (rootfs, devfs, workfs) = match from {
            Some(s) => (
                Tmpfs::from_snapshot(kernel.anon_dev(), &s.root, root_limits),
                Tmpfs::from_snapshot(kernel.anon_dev(), &s.dev, dev_limits),
                Tmpfs::from_snapshot(kernel.anon_dev(), &s.work, root_limits),
            ),
            None => (
                Tmpfs::new(kernel.anon_dev(), 0o755, now, root_limits),
                Tmpfs::new(kernel.anon_dev(), 0o755, now, dev_limits),
                Tmpfs::new(kernel.anon_dev(), 0o755, now, root_limits),
            ),
        };
        let mut root_opts = String::new();
        if let Some(b) = cfg.limits.fs_bytes {
            root_opts.push_str(&format!("size={}k,", b.div_ceil(1024)));
        }
        root_opts.push_str("inode64");
        let ns = Namespace::new(rootfs.clone(), MountFlags::RELATIME, "tmpfs", &root_opts);
        let programs: HashMap<Vec<u8>, Program> = cfg.programs.iter().map(|p| (p.path().into_bytes(), *p)).collect();
        let mut cx = vfs::ops::kernel_caller(ns.root());
        cx.now = now;
        if from.is_none() {
            image::build_root(&ns, &cx, &cfg.programs, &cfg.hostname)?;
        }
        let w = vfs::namei::Walker::new(&ns, &cx);
        let root = ns.root();
        let dev_dir = w.lookup_child(&root, b"dev")?;
        ns.mount(&dev_dir, devfs.clone(), MountFlags::NOSUID, "tmpfs", "size=65536k,mode=755,inode64")?;
        if from.is_none() {
            image::build_dev(&ns, &cx)?;
        }
        // O oráculo roda com `--tmpfs /work:exec`: o diretório de trabalho é outro sistema de
        // arquivos, e o git, por exemplo, para a busca do repositório na fronteira.
        let work_dir = w.lookup_child(&root, b"work")?;
        ns.mount(&work_dir, workfs.clone(), MountFlags::NOSUID | MountFlags::NODEV | MountFlags::RELATIME, "tmpfs", "inode64")?;
        let provider = Arc::new(SbProcProvider { sb: OnceLock::new() });
        let boot_ts = now;
        let procfs = Procfs::new(kernel.anon_dev(), provider.clone(), boot_ts);
        procfs.set_namespace(&ns);
        let proc_dir = vfs::namei::Walker::new(&ns, &cx).lookup_child(&root, b"proc")?;
        ns.mount(&proc_dir, procfs.clone(), MountFlags::NOSUID | MountFlags::NODEV | MountFlags::NOEXEC | MountFlags::RELATIME, "proc", "")?;
        let ncpus = kernel.cpus.ncpus();
        let cpu_group = kernel.cpus.create_group(
            cfg.user_group.as_ref().map(|u| u.id()),
            crate::cpu::GroupLimits { weight: cfg.cpu_weight, max: cfg.cpu_max },
        )?;
        let spawner = Spawner::start(id, kernel.config.spawner_hook.clone(), SpawnerInfo { sandbox_id: id, host_mounts: Vec::new() })
            .map_err(CreateError::Spawner)?;
        let init = Proc::new(
            INIT_PID,
            PState {
                cred: Arc::new(Cred::root()),
                cwd: PinnedLoc::new(ns.root()),
                root: PinnedLoc::new(ns.root()),
                umask: 0o022,
                argv: vec![b"/sbin/init".to_vec()],
                env: Vec::new(),
                comm: b"init".to_vec(),
                exe: None,
                rlimits: default_rlimits(cfg.limits.nofile, cfg.limits.fsize),
                nice: 0,
                pending_exec: None,
            },
            FdTable::default(),
            SigState::new(),
            0,
        );
        cfg.programs = Vec::new();
        let hostname = cfg.hostname.clone().into_bytes();
        let inner = Arc::new(SbInner {
            id,
            kernel,
            ns,
            rootfs,
            devfs,
            workfs,
            builtin_ids: programs.keys().map(|p| (crate::exec::build_id(p), p.clone())).collect(),
            programs,
            table: Mutex::new(Table::new(init)),
            hostname: Mutex::new(hostname),
            domainname: Mutex::new(b"(none)".to_vec()),
            clock,
            boot,
            cfg,
            spawner,
            fifos: Mutex::new(HashMap::new()),
            locks: Arc::new(LockTable::default()),
            destroyed: AtomicBool::new(false),
            cpu_group,
            cpu_done_ns: std::sync::atomic::AtomicU64::new(0),
            cpu_acct: CpuAcct::new(ncpus),
            load: LoadAvg::new(),
            exit_waiters: Mutex::new(WaitList::default()),
            ptys: Mutex::new(BTreeMap::new()),
            ports: Arc::default(),
            unix: Arc::default(),
        });
        let _ = provider.sb.set(Arc::downgrade(&inner));
        inner.load.bind(&inner);
        inner.kernel.cpus.add_sampler(&inner.load);
        Ok(Sandbox { inner })
    }

    pub fn id(&self) -> u64 {
        self.inner.id
    }

    /// Troca o relógio de parede (o `faketime` de um caso).
    pub fn set_clock(&self, mode: ClockMode) {
        *self.inner.clock.lock() = ClockState { mode, set_at: Instant::now() };
    }

    /// Hora atual do sandbox.
    pub fn now(&self) -> TimeSpec {
        self.inner.now()
    }

    /// Acesso direto ao sistema de arquivos, como root.
    pub fn fs(&self) -> SandboxFs<'_> {
        SandboxFs { sb: &self.inner }
    }

    /// Retrato O(1) do sistema de arquivos (raiz e `/dev`).
    pub fn snapshot(&self) -> Snapshot {
        Snapshot { root: self.inner.rootfs.snapshot(), dev: self.inner.devfs.snapshot(), work: self.inner.workfs.snapshot() }
    }

    /// Volta o sistema de arquivos ao retrato. Processos vivos continuam: um fd cujo inode não existe no
    /// retrato passa a dar ESTALE.
    pub fn restore(&self, snap: &Snapshot) {
        self.inner.rootfs.restore(&snap.root);
        self.inner.devfs.restore(&snap.dev);
        self.inner.workfs.restore(&snap.work);
    }

    /// Uso de recursos.
    pub fn usage(&self) -> Usage {
        let (rb, ri) = self.inner.rootfs.usage();
        let (db, di) = self.inner.devfs.usage();
        let (wb, wi) = self.inner.workfs.usage();
        let (db, di) = (db + wb, di + wi);
        let t = self.inner.table.lock();
        let procs = t.live;
        drop(t);
        let cpu = self.inner.cpu_done_ns.load(Ordering::Relaxed) + self.inner.kernel.cpus.group_runtime(self.inner.cpu_group);
        Usage { procs, fs_bytes: rb + db, fs_inodes: ri + di, cpu_ns: cpu }
    }

    /// Processos visíveis (o init incluso).
    pub fn processes(&self) -> Vec<ProcInfo> {
        crate::sys::list_processes(&self.inner)
    }

    /// Detalhe de um processo pro host: info, argv e CPU.
    pub fn process(&self, pid: Pid) -> Option<HostProcInfo> {
        let info = self.processes().into_iter().find(|p| p.pid == pid)?;
        let proc = self.inner.table.lock().proc(pid)?;
        let argv = proc.st.lock().argv.clone();
        Some(HostProcInfo { info, argv, cpu_ns: proc.cpu_done_ns.load(Ordering::Relaxed), threads: proc.nthreads() })
    }

    /// Manda um sinal como root de fora do sandbox (`KillTarget::All` = todos menos o init).
    pub fn kill(&self, target: KillTarget, sig: Signal) -> Result<(), Errno> {
        crate::sys::host_kill(&self.inner, target, sig)
    }

    /// SIGKILL em todos os processos.
    pub fn kill_all(&self) {
        let _ = crate::sys::host_kill(&self.inner, KillTarget::All, Signal::SIGKILL);
    }

    /// Espera um processo criado pelo host terminar e colhe o zumbi. `None` se o prazo venceu.
    pub fn wait(&self, pid: Pid, deadline: Option<Instant>) -> Result<Option<(WaitStatus, Rusage)>, Errno> {
        crate::hostio::host_wait(&self.inner, pid, deadline)
    }

    /// Destrói o sandbox: SIGKILL em tudo, espera as threads saírem (até 2 s) e para a spawner. O `Drop`
    /// faz o mesmo.
    pub fn destroy(self) {
        drop(self);
    }

    fn shutdown(&self) {
        if self.inner.destroyed.swap(true, Ordering::AcqRel) {
            return;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let me = Parker::new();
        loop {
            let _ = crate::sys::host_kill(&self.inner, KillTarget::All, Signal::SIGKILL);
            let any_threads = {
                let t = self.inner.table.lock();
                t.map.values().any(|e| e.proc.nthreads() > 0)
            };
            if !any_threads {
                break;
            }
            self.inner.exit_waiters.lock().register(&me);
            if !me.park_until(deadline.min(Instant::now() + Duration::from_millis(50))) && Instant::now() >= deadline {
                break;
            }
        }
        // Zumbis e o init: solta a tabela (quebra os ciclos Task -> SbInner).
        let procs: Vec<Arc<Proc>> = {
            let mut t = self.inner.table.lock();
            let keep: Vec<Pid> = t.map.iter().filter(|(_, e)| e.proc.nthreads() > 0).map(|(p, _)| *p).collect();
            let all: BTreeMap<Pid, crate::proc::Entry> = std::mem::take(&mut t.map);
            let mut out = Vec::new();
            for (p, e) in all {
                if keep.contains(&p) {
                    t.map.insert(p, e);
                } else {
                    out.push(e.proc);
                }
            }
            out
        };
        let stuck = !self.inner.table.lock().map.is_empty();
        for p in procs {
            let fds = p.fds.lock().take_all();
            drop(fds);
        }
        self.inner.spawner.stop();
        // Grupo de CPU vazio volta pra ser reaproveitado; com thread presa ele fica (vazado de propósito).
        if !stuck {
            self.inner.kernel.cpus.release_group(self.inner.cpu_group);
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Detalhe de um processo pro host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostProcInfo {
    pub info: ProcInfo,
    pub argv: Vec<Vec<u8>>,
    /// CPU das threads que já terminaram.
    pub cpu_ns: u64,
    pub threads: usize,
}

/// Operações diretas no sistema de arquivos do sandbox, como root, com cwd em `/`. Caminhos relativos
/// resolvem a partir de `/`. Os erros são os do Linux.
pub struct SandboxFs<'a> {
    sb: &'a Arc<SbInner>,
}

impl SandboxFs<'_> {
    fn cx(&self) -> Caller {
        self.sb.root_caller()
    }

    fn ns(&self) -> &Namespace {
        &self.sb.ns
    }

    pub fn stat(&self, path: &[u8]) -> Result<Stat, Errno> {
        Ok(self.ns().stat(&self.cx(), &Start::Cwd, path, AtFlags::empty())?.stat())
    }

    pub fn lstat(&self, path: &[u8]) -> Result<Stat, Errno> {
        Ok(self.ns().stat(&self.cx(), &Start::Cwd, path, AtFlags::SYMLINK_NOFOLLOW)?.stat())
    }

    /// Lê `len` bytes a partir de `offset` (menos no fim do arquivo).
    pub fn read(&self, path: &[u8], offset: u64, len: usize) -> Result<Vec<u8>, Errno> {
        let cx = self.cx();
        match self.ns().open(&cx, &Start::Cwd, path, OFlags::RDONLY, 0)? {
            Opened::File { handle, stat, .. } => {
                if vfs::is_dir(stat.mode) {
                    return Err(Errno::EISDIR);
                }
                let mut out = Vec::new();
                out.try_reserve(len.min(stat.size as usize)).map_err(|_| Errno::ENOMEM)?;
                let mut buf = vec![0u8; 64 * 1024];
                let mut off = offset;
                while out.len() < len {
                    let want = (len - out.len()).min(buf.len());
                    let n = handle.read(&cx, off, &mut buf[..want])?;
                    if n == 0 {
                        break;
                    }
                    out.extend_from_slice(&buf[..n]);
                    off += n as u64;
                }
                Ok(out)
            }
            _ => Err(Errno::EINVAL),
        }
    }

    pub fn read_file(&self, path: &[u8]) -> Result<Vec<u8>, Errno> {
        self.read(path, 0, usize::MAX)
    }

    /// Escreve com o modo de abertura dado; `mode` vale na criação (sem umask) e é aplicado com chmod
    /// quando o arquivo é criado.
    pub fn write(&self, path: &[u8], data: &[u8], how: WriteMode, mode: Mode) -> Result<(), Errno> {
        let cx = self.cx();
        let flags = match how {
            WriteMode::Truncate => OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC,
            WriteMode::Append => OFlags::WRONLY | OFlags::CREAT | OFlags::APPEND,
            WriteMode::CreateNew => OFlags::WRONLY | OFlags::CREAT | OFlags::EXCL,
            WriteMode::At(_) => OFlags::WRONLY,
        };
        let existed = self.lstat(path).is_ok();
        match self.ns().open(&cx, &Start::Cwd, path, flags, mode & 0o7777)? {
            Opened::File { handle, .. } => {
                let mut done = 0usize;
                while done < data.len() {
                    let pos = match how {
                        WriteMode::Append => WritePos::Append,
                        WriteMode::At(o) => WritePos::At(o + done as u64),
                        _ => WritePos::At(done as u64),
                    };
                    let (n, _) = handle.write(&cx, pos, &data[done..])?;
                    done += n;
                }
            }
            _ => return Err(Errno::EINVAL),
        }
        if !existed {
            self.ns().chmod(&cx, &Start::Cwd, path, mode & 0o7777, AtFlags::empty())?;
        }
        Ok(())
    }

    /// Cria ou substitui um arquivo com o conteúdo e o modo exatos.
    pub fn write_file(&self, path: &[u8], data: &[u8], mode: Mode) -> Result<(), Errno> {
        self.write(path, data, WriteMode::Truncate, mode)?;
        self.ns().chmod(&self.cx(), &Start::Cwd, path, mode & 0o7777, AtFlags::empty())
    }

    pub fn readdir(&self, path: &[u8]) -> Result<Vec<FsEntry>, Errno> {
        let cx = self.cx();
        match self.ns().open(&cx, &Start::Cwd, path, OFlags::RDONLY | OFlags::DIRECTORY, 0)? {
            Opened::File { handle, .. } => {
                let mut out = Vec::new();
                let mut cookie = 0;
                loop {
                    let (batch, next): (Vec<DirEntry>, u64) = handle.readdir(&cx, cookie, 1024)?;
                    if batch.is_empty() {
                        break;
                    }
                    cookie = next;
                    out.extend(batch.into_iter().filter(|e| e.name != b"." && e.name != b"..").map(|e| FsEntry {
                        name: e.name,
                        ino: e.ino,
                        kind: e.kind,
                    }));
                }
                Ok(out)
            }
            _ => Err(Errno::ENOTDIR),
        }
    }

    pub fn readlink(&self, path: &[u8]) -> Result<Vec<u8>, Errno> {
        self.ns().readlink(&self.cx(), &Start::Cwd, path)
    }

    /// `mkdir` com o modo exato (sem umask).
    pub fn mkdir(&self, path: &[u8], mode: Mode) -> Result<(), Errno> {
        let cx = self.cx();
        self.ns().mkdir(&cx, &Start::Cwd, path, mode)?;
        self.ns().chmod(&cx, &Start::Cwd, path, mode & 0o7777, AtFlags::empty())
    }

    /// `mkdir -p`: cria os que faltam com `mode`; os que existem ficam como estão.
    pub fn mkdir_all(&self, path: &[u8], mode: Mode) -> Result<(), Errno> {
        let mut acc = Vec::new();
        if path.first() == Some(&b'/') {
            acc.push(b'/');
        }
        for comp in path.split(|b| *b == b'/').filter(|c| !c.is_empty()) {
            if !acc.is_empty() && acc.last() != Some(&b'/') {
                acc.push(b'/');
            }
            acc.extend_from_slice(comp);
            match self.stat(&acc) {
                Ok(st) if vfs::is_dir(st.mode) => continue,
                Ok(_) => return Err(Errno::ENOTDIR),
                Err(Errno::ENOENT) => self.mkdir(&acc, mode)?,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    pub fn unlink(&self, path: &[u8]) -> Result<(), Errno> {
        self.ns().unlink(&self.cx(), &Start::Cwd, path, AtFlags::empty())
    }

    pub fn rmdir(&self, path: &[u8]) -> Result<(), Errno> {
        self.ns().unlink(&self.cx(), &Start::Cwd, path, AtFlags::REMOVEDIR)
    }

    /// `rm -rf` (não segue symlinks; ENOENT se não existe).
    pub fn remove_all(&self, path: &[u8]) -> Result<(), Errno> {
        let st = self.lstat(path)?;
        if vfs::is_dir(st.mode) {
            for e in self.readdir(path)? {
                let mut child = path.to_vec();
                if child.last() != Some(&b'/') {
                    child.push(b'/');
                }
                child.extend_from_slice(&e.name);
                self.remove_all(&child)?;
            }
            self.rmdir(path)
        } else {
            self.unlink(path)
        }
    }

    pub fn symlink(&self, target: &[u8], path: &[u8]) -> Result<(), Errno> {
        self.ns().symlink(&self.cx(), target, &Start::Cwd, path)
    }

    /// `link(2)`: hardlink de `existing` em `new` (sem seguir symlink, como o `link` do Linux).
    pub fn link(&self, existing: &[u8], new: &[u8]) -> Result<(), Errno> {
        self.ns().link(&self.cx(), &Start::Cwd, existing, &Start::Cwd, new, AtFlags::empty())
    }

    pub fn rename(&self, from: &[u8], to: &[u8]) -> Result<(), Errno> {
        self.ns().rename(&self.cx(), &Start::Cwd, from, &Start::Cwd, to, RenameFlags::empty())
    }

    pub fn chmod(&self, path: &[u8], mode: Mode) -> Result<(), Errno> {
        self.ns().chmod(&self.cx(), &Start::Cwd, path, mode, AtFlags::empty())
    }

    pub fn chown(&self, path: &[u8], uid: Option<u32>, gid: Option<u32>, nofollow: bool) -> Result<(), Errno> {
        let f = if nofollow { AtFlags::SYMLINK_NOFOLLOW } else { AtFlags::empty() };
        self.ns().chown(&self.cx(), &Start::Cwd, path, uid, gid, f)
    }

    /// `utimensat` (com `nofollow`, no próprio symlink).
    pub fn utimens(&self, path: &[u8], atime: SetTime, mtime: SetTime, nofollow: bool) -> Result<(), Errno> {
        let f = if nofollow { AtFlags::SYMLINK_NOFOLLOW } else { AtFlags::empty() };
        self.ns().utimens(&self.cx(), &Start::Cwd, path, atime, mtime, f)
    }

    /// Atalho: atime e mtime no mesmo instante, sem seguir symlink.
    pub fn set_mtime(&self, path: &[u8], t: TimeSpec) -> Result<(), Errno> {
        self.utimens(path, SetTime::At(t), SetTime::At(t), true)
    }

    /// Retrato recursivo de um diretório: caminhos relativos a `root`, em ordem de nome.
    pub fn tree(&self, root: &[u8]) -> Result<Vec<(Vec<u8>, TreeEntry)>, Errno> {
        let mut out = Vec::new();
        self.walk(root, &[], &mut out)?;
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    fn walk(&self, dir: &[u8], prefix: &[u8], out: &mut Vec<(Vec<u8>, TreeEntry)>) -> Result<(), Errno> {
        for e in self.readdir(dir)? {
            let mut abs = dir.to_vec();
            if abs.last() != Some(&b'/') {
                abs.push(b'/');
            }
            abs.extend_from_slice(&e.name);
            let rel = if prefix.is_empty() { e.name.clone() } else { [prefix, b"/", &e.name].concat() };
            let st = self.lstat(&abs)?;
            let entry = match st.mode & sysabi::mode::S_IFMT {
                sysabi::mode::S_IFREG => TreeEntry::File { data: self.read_file(&abs)?, mode: st.mode & 0o7777 },
                sysabi::mode::S_IFDIR => {
                    out.push((rel.clone(), TreeEntry::Dir { mode: st.mode & 0o7777 }));
                    self.walk(&abs, &rel, out)?;
                    continue;
                }
                sysabi::mode::S_IFLNK => TreeEntry::Symlink { target: self.readlink(&abs)? },
                _ => TreeEntry::Other { mode: st.mode },
            };
            out.push((rel, entry));
        }
        Ok(())
    }
}
