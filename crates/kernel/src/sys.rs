//! A implementação de [`sysabi::Syscalls`] por thread ([`Task`]).
//!
//! Toda syscall começa com [`Task::enter`] (ponto de entrega de sinal). Espera é sempre por
//! [`Task::wait_event`]: registra o parker no objeto, confere a condição, dorme, e no despertar trata
//! sinal fatal (desenrola com `KillUnwind`), parada (dorme até SIGCONT e continua esperando), `exit_group`
//! de outra thread (desenrola) e sinal capturado (EINTR, só pra thread que recebe os sinais do processo).
//! Durante um unwind (`std::thread::panicking()`), nada desenrola de novo: as esperas voltam EINTR.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use sysabi::*;
use sysabi::sched;
use vfs::{Caller, Loc, Opened, PinnedLoc, Start, WritePos};

use crate::dev::Device;
use crate::exec;
use crate::fd::{FileObj, Ofd};
use crate::park::Parker;
use crate::pipe::{Pipe, PipeObject, Try, WriteError};
use crate::scm::{Scm, Ucred};
use crate::unix::Datagram;
use crate::proc::{INIT_PID, Proc, Task, may_signal};
use crate::sandbox::{FLOCK_OWNER, LockFail, POSIX_OWNER, SbInner, lock_owner_pid};
use crate::signal::{Action, Generated};
use crate::spawn::{self, Body, GroupExitUnwind};
use crate::tty::{self, Pty, PtyEnd};

/// Release do `uname -r` e versão do `uname -v` do Debian 13 da bancada.
pub(crate) const UNAME_RELEASE: &[u8] = b"6.12.101+deb13-amd64";
pub(crate) const UNAME_VERSION: &[u8] = b"#1 SMP PREEMPT_DYNAMIC Debian 6.12.101-1 (2026-08-05)";
/// `f_type` do pipefs.
const PIPEFS_MAGIC: u64 = 0x5049_5045;
/// `NGROUPS_MAX` do Linux.
const NGROUPS_MAX: usize = 65536;
/// `SOL_SOCKET` e `SO_SNDBUF`, as chaves em que o `Ofd.sock` guarda o que o `setsockopt` definiu.
const SOL_SOCKET: i32 = 1;
const SO_SNDBUF: i32 = 7;
/// `SO_PASSCRED` e `SO_PEERCRED`.
const SO_PASSCRED: i32 = 16;
const SO_PEERCRED: i32 = 17;
/// `sk_sndbuf` de um TCP de loopback depois do `tcp_sndbuf_expand` (`tcp_init_buffer_space`), na conexão e no
/// `accept`: `2 * 10 segmentos * (roundup_pow_of_two(65495 + MAX_TCP_HEADER 256 + 320) + 256) = 2626560`, abaixo do
/// teto `tcp_wmem[2]` (4194304). O `getsockopt` o devolve sem dividir; `setsockopt` antes disso trava o buffer
/// (`SOCK_SNDBUF_LOCK`) e nada o amplia.
const TCP_SNDBUF_EXPANDED: i64 = 2_626_560;

/// Gera um sinal num processo (sem checar permissão). SIGCONT retoma um processo parado.
pub(crate) fn generate_signal(target: &Arc<Proc>, sig: Signal) {
    if target.pid == INIT_PID {
        // O init só recebe sinais pra que instalou tratador (nunca, aqui).
        return;
    }
    if target.nthreads() == 0 {
        return;
    }
    if sig.0 == 18 {
        target.sig.lock().clear_stops();
        if target.stopped.swap(false, Ordering::AcqRel) {
            continued(target);
        }
    }
    let g = target.sig.lock().generate(sig);
    if g == Generated::Wake || sig.0 == 18 || sig.0 == 9 {
        target.kick_all();
    }
}

/// Um processo parado voltou: relatório pro pai (`WCONTINUED`) e SIGCHLD.
fn continued(target: &Arc<Proc>) {
    let sb = match target.threads.lock().live.values().next() {
        Some(t) => t.sb.clone(),
        None => return,
    };
    let parent = {
        let mut t = sb.table.lock();
        let Some(r) = t.rel_mut(target.pid) else { return };
        r.cont_report = true;
        r.stop_report = None;
        let pp = r.ppid;
        t.proc(pp)
    };
    if let Some(p) = parent {
        generate_signal(&p, Signal::SIGCHLD);
        p.kick_all();
    }
}

/// `kill` vindo do host (root, de fora do sandbox).
pub(crate) fn host_kill(sb: &Arc<SbInner>, target: KillTarget, sig: Signal) -> Result<(), Errno> {
    if !(0..=sysabi::linux::SIGRTMAX).contains(&sig.0) {
        return Err(Errno::EINVAL);
    }
    let procs: Vec<Arc<Proc>> = {
        let t = sb.table.lock();
        match target {
            KillTarget::Pid(p) => t.proc(p).into_iter().collect(),
            KillTarget::Group(g) => t.group_members(g),
            KillTarget::All => t.map.values().filter(|e| e.proc.pid != INIT_PID && e.rel.zombie.is_none()).map(|e| e.proc.clone()).collect(),
        }
    };
    if procs.is_empty() {
        return Err(Errno::ESRCH);
    }
    if sig.0 != 0 {
        for p in &procs {
            generate_signal(p, sig);
        }
    }
    Ok(())
}

pub(crate) fn list_processes(sb: &Arc<SbInner>) -> Vec<ProcInfo> {
    let t = sb.table.lock();
    t.map
        .values()
        .map(|e| {
            let p = &e.proc;
            let state = if e.rel.zombie.is_some() {
                'Z'
            } else if p.pid == INIT_PID {
                'S'
            } else if p.stopped.load(Ordering::Relaxed) {
                'T'
            } else {
                let th = p.threads.lock();
                match th.live.get(&p.pid).or_else(|| th.live.values().next()) {
                    Some(task) if task.blocked.load(Ordering::Relaxed) => 'S',
                    _ => 'R',
                }
            };
            ProcInfo { pid: p.pid, ppid: e.rel.ppid, pgid: e.rel.pgid, sid: e.rel.sid, state, comm: p.st.lock().comm.clone() }
        })
        .collect()
}

fn env_name(kv: &[u8]) -> &[u8] {
    match kv.iter().position(|b| *b == b'=') {
        Some(i) => &kv[..i],
        None => kv,
    }
}

/// Os objetos sem deslocamento: `pread`/`pwrite` neles dão ESPIPE.
fn unseekable(obj: &FileObj) -> bool {
    matches!(
        obj,
        FileObj::Pipe { .. } | FileObj::Dev { dev: Device::Pty(_), .. } | FileObj::Listener(_) | FileObj::Stream(_) | FileObj::Unix(_) | FileObj::Udp(_) | FileObj::Epoll(_) | FileObj::Pidfd(_) | FileObj::Anon(_)
    )
}

/// `file_can_poll` do `epoll_ctl`: arquivo regular, diretório e dispositivos sem `poll` dão EPERM, e `O_PATH`
/// não é um fd de verdade (EBADF).
fn check_pollable(obj: &FileObj) -> SysResult<()> {
    match obj {
        FileObj::Path { .. } => Err(Errno::EBADF),
        FileObj::Vfs { .. } | FileObj::Dev { dev: Device::Null | Device::Zero | Device::Full, .. } => Err(Errno::EPERM),
        _ => Ok(()),
    }
}

/// Um pipe novo de `cred`: cada sentido de uma conexão TCP de loopback, e a identidade de um socket.
fn new_sock_pipe(sb: &SbInner, cred: &vfs::Cred) -> Arc<Pipe> {
    Pipe::new(sb.kernel.pipe_ino(), cred.uid, cred.gid, sb.now())
}

/// Um datagrama lido num buffer: o que não cabe se perde, como no `read` do Linux.
fn truncated_copy(data: &[u8], buf: &mut [u8]) -> usize {
    let n = data.len().min(buf.len());
    buf[..n].copy_from_slice(&data[..n]);
    n
}

impl Task {
    // ------------------------------------------------------------------------------------------
    // Contexto
    // ------------------------------------------------------------------------------------------

    pub(crate) fn caller(&self) -> Caller {
        let st = self.proc.st.lock();
        Caller {
            cred: st.cred.clone(),
            root: st.root.loc().clone(),
            cwd: st.cwd.loc().clone(),
            umask: st.umask,
            now: self.sb.now(),
            pid: self.proc.pid,
            tid: self.tid(),
            fsize_limit: st.rlimits[Resource::Fsize as usize].cur,
        }
    }

    fn nofile(&self) -> u64 {
        self.proc.st.lock().rlimits[Resource::Nofile as usize].cur
    }

    fn ofd(&self, fd: Fd) -> SysResult<Arc<Ofd>> {
        self.proc.fds.lock().ofd(fd)
    }

    /// `dirfd` de uma syscall `*at`. Só é consultado se o caminho for relativo.
    fn start_of(&self, dirfd: Fd, path: &[u8]) -> Start {
        if dirfd == Fd::CWD || path.first() == Some(&b'/') {
            return Start::Cwd;
        }
        match self.ofd(dirfd) {
            Err(e) => Start::Bad(e),
            Ok(o) => match o.loc() {
                Some(l) if matches!(o.obj, FileObj::Vfs { .. } | FileObj::Path { .. }) => Start::Dir(l.clone()),
                _ => Start::Bad(Errno::ENOTDIR),
            },
        }
    }

    fn install(&self, ofd: Arc<Ofd>, cloexec: bool) -> SysResult<Fd> {
        let limit = self.nofile();
        self.proc.fds.lock().install(ofd, cloexec, 0, limit)
    }

    // ------------------------------------------------------------------------------------------
    // Sinais, paradas e espera
    // ------------------------------------------------------------------------------------------

    /// Entrada de syscall: ponto de entrega.
    pub(crate) fn enter(&self) {
        if self.attention.load(Ordering::Acquire) {
            self.slow_path();
        }
    }

    /// O caminho lento do checkpoint: término do grupo, sinais e paradas.
    fn slow_path(&self) {
        self.attention.store(false, Ordering::Release);
        // Pedido de troca de CPU (fim de fatia, preempção por wakeup) ou CPU tirada pelo watchdog.
        self.sb.kernel.cpus.checkpoint(&self.ct);
        let unwinding = std::thread::panicking();
        {
            let e = self.proc.exit.lock();
            let exec_other = e.exec_by.is_some_and(|t| t != self.tid());
            if (e.group.is_some() || exec_other) && !unwinding {
                drop(e);
                std::panic::resume_unwind(Box::new(GroupExitUnwind));
            }
        }
        loop {
            let act = self.proc.sig.lock().dequeue();
            match act {
                Action::None => break,
                Action::Fatal(sig) => {
                    self.proc.start_group_exit(WaitStatus::Signaled { signal: sig, core_dumped: false });
                    if !unwinding {
                        std::panic::resume_unwind(Box::new(KillUnwind(sig)));
                    }
                    break;
                }
                Action::Stop(sig) => self.do_stop(sig),
            }
        }
        if self.proc.stopped.load(Ordering::Acquire) && !unwinding {
            self.wait_while_stopped();
            // Pode ter chegado SIGKILL durante a parada.
            if self.proc.sig.lock().has_work() || self.proc.group_exit_status().is_some() {
                self.slow_path();
            }
        }
    }

    /// O processo para: relatório pro pai, SIGCHLD, e as outras threads param no próximo ponto.
    fn do_stop(&self, sig: Signal) {
        if self.proc.stopped.swap(true, Ordering::AcqRel) {
            return;
        }
        let parent = {
            let mut t = self.sb.table.lock();
            match t.rel_mut(self.proc.pid) {
                Some(r) => {
                    r.stop_report = Some(sig);
                    r.cont_report = false;
                    let pp = r.ppid;
                    t.proc(pp)
                }
                None => None,
            }
        };
        if let Some(p) = parent {
            generate_signal(&p, Signal::SIGCHLD);
            p.kick_all();
        }
        self.proc.kick_all();
    }

    fn wait_while_stopped(&self) {
        while self.proc.stopped.load(Ordering::Acquire) {
            if self.proc.sig.lock().fatal_pending() || self.proc.group_exit_status().is_some() {
                return;
            }
            self.sleep(None);
        }
    }

    /// Erro de uma espera interrompida, ou `None` pra continuar esperando. Sinal fatal e término do grupo
    /// desenrolam daqui (fora de qualquer trava).
    fn interrupted(&self, seq0: u64) -> Option<Errno> {
        if self.attention.load(Ordering::Acquire) {
            if std::thread::panicking() {
                // Morrendo: não dá pra desenrolar de novo, e não pode ficar preso.
                let fatal = self.proc.sig.lock().fatal_pending() || self.proc.group_exit_status().is_some();
                if fatal {
                    return Some(Errno::EINTR);
                }
            }
            self.slow_path();
        }
        let seq = self.proc.sig.lock().caught_seq();
        if seq != seq0 && self.proc.signal_target() == Some(self.tid()) {
            return Some(Errno::EINTR);
        }
        None
    }

    /// Espera interrompível: `f` tenta e, se precisar esperar, registra o parker e devolve `Pending`.
    /// `deadline` vencido dá ETIMEDOUT.
    pub(crate) fn wait_event<T>(&self, deadline: Option<Instant>, mut f: impl FnMut(&Arc<Parker>) -> Try<SysResult<T>>) -> SysResult<T> {
        let seq0 = self.proc.sig.lock().caught_seq();
        loop {
            if let Try::Ready(r) = f(&self.parker) {
                return r;
            }
            if let Some(e) = self.interrupted(seq0) {
                return Err(e);
            }
            let woke = self.sleep(deadline);
            if !woke {
                if let Try::Ready(r) = f(&self.parker) {
                    return r;
                }
                return Err(Errno::ETIMEDOUT);
            }
        }
    }

    // ------------------------------------------------------------------------------------------
    // Abertura
    // ------------------------------------------------------------------------------------------

    /// `open` completo: VFS, FIFO (com o encontro), dispositivo, pipe atrás de magic link.
    pub(crate) fn open_ofd(&self, cx: &Caller, start: &Start, path: &[u8], flags: OFlags, mode: Mode) -> SysResult<Arc<Ofd>> {
        let locks = Arc::downgrade(&self.sb.locks);
        match self.sb.ns.open(cx, start, path, flags, mode)? {
            Opened::File { loc, stat, handle } => {
                let kind = stat.file_type();
                Ok(Ofd::new(FileObj::Vfs { loc, handle, kind }, flags, locks))
            }
            Opened::Path { loc, .. } => Ok(Ofd::new(FileObj::Path { loc }, flags | OFlags::PATH, locks)),
            Opened::CharDev { loc, stat } => {
                let dev = self.open_chardev(stat.rdev, flags)?;
                Ok(Ofd::new(FileObj::Dev { dev, loc: Some(loc) }, flags, locks))
            }
            Opened::Fifo { loc, .. } => {
                let pipe = self.sb.fifo_pipe((loc.fs().dev(), loc.ino), &cx.cred);
                self.fifo_open(pipe, Some(loc), flags)
            }
            Opened::Object(o) => match o.as_any().downcast_ref::<PipeObject>() {
                Some(po) => self.fifo_open(po.pipe.clone(), None, flags),
                None => Err(Errno::ENXIO),
            },
        }
    }

    // ------------------------------------------------------------------------------------------
    // Terminais
    // ------------------------------------------------------------------------------------------

    /// Dispositivo de caractere pelo `rdev`: ptmx, escravo de pty, `/dev/tty` e os sem estado.
    fn open_chardev(&self, rdev: u64, flags: OFlags) -> SysResult<Device> {
        let id = (vfs::dev_major(rdev), vfs::dev_minor(rdev));
        if id == tty::DEV_PTMX {
            return Ok(Device::Pty(self.open_ptmx()?));
        }
        if id == crate::dev::DEV_TTY {
            // `tty_open_current_tty`: o terminal de controle da sessão; ENXIO sem ele.
            let pty = self.ctty().ok_or(Errno::ENXIO)?;
            return Ok(Device::Pty(pty.attach(false)));
        }
        if id.0 == tty::PTS_MAJOR {
            let pty = self.sb.pty(id.1).ok_or(Errno::EIO)?;
            pty.slave_openable()?;
            let end = pty.attach(false);
            if !flags.contains(OFlags::NOCTTY) {
                self.maybe_acquire_ctty(&pty);
            }
            return Ok(Device::Pty(end));
        }
        Device::open(rdev)
    }

    /// `ptmx_open`: pty novo e o nó `/dev/pts/N` (dono quem abriu, grupo tty, modo 0620), travado.
    fn open_ptmx(&self) -> SysResult<PtyEnd> {
        let uid = self.proc.st.lock().cred.uid;
        let pty = self.sb.alloc_pty()?;
        let end = pty.attach(true);
        let cx = self.sb.root_caller();
        let path = pty.path();
        let s = Start::Cwd;
        let ns = &self.sb.ns;
        let made = ns
            .mknod(&cx, &s, &path, mode::S_IFCHR | tty::PTS_MODE, vfs::makedev(tty::PTS_MAJOR, pty.index))
            .and_then(|()| ns.chmod(&cx, &s, &path, tty::PTS_MODE, AtFlags::empty()))
            .and_then(|()| ns.chown(&cx, &s, &path, Some(uid), Some(tty::TTY_GID), AtFlags::empty()));
        match made {
            Ok(()) => Ok(end),
            Err(e) => {
                drop(end);
                Err(e)
            }
        }
    }

    /// pid, sid e pgid do processo.
    fn ids(&self) -> SysResult<(Pid, Pid, Pid)> {
        let t = self.sb.table.lock();
        let r = t.rel(self.proc.pid).ok_or(Errno::ESRCH)?;
        Ok((self.proc.pid, r.sid, r.pgid))
    }

    /// O terminal de controle da sessão do processo.
    fn ctty(&self) -> Option<Arc<Pty>> {
        let (_, sid, _) = self.ids().ok()?;
        self.sb.pty_of_session(sid)
    }

    /// Abrir um tty sem O_NOCTTY dá terminal de controle ao líder de sessão que ainda não tem um, se o
    /// tty não é de outra sessão (`tty_open_proc_set_tty`).
    fn maybe_acquire_ctty(&self, pty: &Pty) {
        let Ok((me, sid, pgid)) = self.ids() else { return };
        if sid != me || pty.session().is_some() || self.sb.pty_of_session(sid).is_some() {
            return;
        }
        pty.set_ctty(sid, pgid);
    }

    /// O pty de um fd e se a ponta é o mestre. EBADF pra fd ruim ou O_PATH, ENOTTY pra não terminal.
    fn tty_of(&self, fd: Fd) -> SysResult<(Arc<Pty>, bool)> {
        let ofd = self.ofd(fd)?;
        match &ofd.obj {
            FileObj::Dev { dev: Device::Pty(e), .. } => Ok((e.pty.clone(), e.master)),
            FileObj::Path { .. } => Err(Errno::EBADF),
            _ => Err(Errno::ENOTTY),
        }
    }

    /// Pras ioctls de controle de job pelo escravo, o fd tem de ser o terminal de controle de quem
    /// chama (`tty == real_tty && current->signal->tty != real_tty` dá ENOTTY); pelo mestre, não.
    fn job_tty(&self, fd: Fd) -> SysResult<(Arc<Pty>, Pid)> {
        let (pty, master) = self.tty_of(fd)?;
        let (_, sid, _) = self.ids()?;
        if !master && pty.session() != Some(sid) {
            return Err(Errno::ENOTTY);
        }
        Ok((pty, sid))
    }

    fn tty_read(&self, ofd: &Ofd, end: &PtyEnd, buf: &mut [u8]) -> SysResult<usize> {
        let pty = &end.pty;
        let nonblock = ofd.nonblock();
        let r = if end.master {
            self.wait_event(None, |p| pty.try_master_read(buf, nonblock, p))
        } else {
            self.tty_slave_read(pty, buf, nonblock)
        };
        pty.unregister(&self.parker);
        r
    }

    /// Leitura do escravo com o VMIN e o VTIME do modo não canônico (`n_tty_read`).
    fn tty_slave_read(&self, pty: &Pty, buf: &mut [u8], nonblock: bool) -> SysResult<usize> {
        use sysabi::termios::{ICANON, VMIN, VTIME};
        let t = pty.termios();
        if t.c_lflag & ICANON != 0 || buf.is_empty() || nonblock {
            return self.wait_event(None, |p| pty.try_slave_read(buf, nonblock, 1, p));
        }
        let vmin = t.c_cc[VMIN] as usize;
        let vtime = Duration::from_millis(t.c_cc[VTIME] as u64 * 100);
        match (vmin, vtime.is_zero()) {
            (0, true) => self.wait_event(None, |p| pty.try_slave_read(buf, false, 0, p)),
            (m, true) => {
                let need = m.min(buf.len());
                self.wait_event(None, |p| pty.try_slave_read(buf, false, need, p))
            }
            (0, false) => match self.wait_event(Some(Instant::now() + vtime), |p| pty.try_slave_read(buf, false, 1, p)) {
                Err(Errno::ETIMEDOUT) => Ok(0),
                r => r,
            },
            (m, false) => {
                // Temporizador entre bytes: começa no primeiro byte e reinicia a cada chegada.
                let target = m.min(buf.len());
                let mut got = self.wait_event(None, |p| pty.try_slave_read(buf, false, 1, p))?;
                while got > 0 && got < target {
                    let deadline = Instant::now() + vtime;
                    match self.wait_event(Some(deadline), |p| pty.try_slave_read(&mut buf[got..], false, 1, p)) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => got += n,
                    }
                }
                Ok(got)
            }
        }
    }

    fn tty_write(&self, ofd: &Ofd, end: &PtyEnd, buf: &[u8]) -> SysResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let pty = &end.pty;
        let nonblock = ofd.nonblock();
        let mut done = 0usize;
        let mut sigs = Vec::new();
        let r = self.wait_event(None, |p| {
            let r = if end.master {
                pty.try_master_write(buf, &mut done, nonblock, &mut sigs, p)
            } else {
                pty.try_slave_write(buf, &mut done, nonblock, p)
            };
            // ISIG: o grupo em primeiro plano recebe na hora, mesmo que a escrita ainda espere.
            for (pgrp, sig) in sigs.drain(..) {
                tty::signal_pgrp(&self.sb, pgrp, sig);
            }
            r
        });
        pty.unregister(&self.parker);
        // Quem escreveu pode estar no grupo sinalizado.
        self.enter();
        match r {
            Err(e) if done > 0 && e != Errno::EIO => Ok(done),
            r => r,
        }
    }

    /// `fifo_open`.
    fn fifo_open(&self, pipe: Arc<Pipe>, loc: Option<Loc>, flags: OFlags) -> SysResult<Arc<Ofd>> {
        let nonblock = flags.contains(OFlags::NONBLOCK);
        let (read, write) = match flags.bits() & OFlags::ACCMODE {
            0 => (true, false),
            1 => (false, true),
            2 => (true, true),
            _ => return Err(Errno::EINVAL),
        };
        // Pipe anônimo reaberto por /proc/self/fd (`is_pipe` no fifo_open): sem encontro e sem ENXIO.
        let is_pipe = loc.is_none();
        let (readers, _, rc, wc) = pipe.counters();
        if !is_pipe && write && !read && nonblock && readers == 0 {
            return Err(Errno::ENXIO);
        }
        let end = pipe.attach(read, write);
        if !is_pipe && !nonblock && read != write {
            let want_writer = read;
            let seen = if want_writer { wc } else { rc };
            let r = self.wait_event(None, |p| if pipe.fifo_wait_peer(want_writer, seen, p) { Try::Ready(Ok(())) } else { Try::Pending });
            if let Err(e) = r {
                pipe.unregister(&self.parker);
                return Err(e);
            }
        }
        let locks = Arc::downgrade(&self.sb.locks);
        Ok(Ofd::new(FileObj::Pipe { end, fifo: loc }, flags, locks))
    }

    fn ofd_stat(&self, ofd: &Ofd) -> SysResult<Stat> {
        ofd_stat_in(&self.sb, &self.caller(), ofd)
    }

    // ------------------------------------------------------------------------------------------
    // Leitura e escrita
    // ------------------------------------------------------------------------------------------

    fn do_read(&self, ofd: &Arc<Ofd>, buf: &mut [u8], at: Option<u64>) -> SysResult<usize> {
        if matches!(ofd.obj, FileObj::Path { .. }) || !ofd.readable {
            return Err(Errno::EBADF);
        }
        if at.is_some() && unseekable(&ofd.obj) {
            return Err(Errno::ESPIPE);
        }
        match &ofd.obj {
            FileObj::Vfs { kind: FileType::Directory, .. } => Err(Errno::EISDIR),
            FileObj::Vfs { handle, .. } => {
                let cx = self.caller();
                match at {
                    Some(off) => handle.read(&cx, off, buf),
                    None => {
                        let mut st = ofd.st.lock();
                        let n = handle.read(&cx, st.pos, buf)?;
                        st.pos += n as u64;
                        Ok(n)
                    }
                }
            }
            FileObj::Pipe { end, .. } => {
                let nonblock = ofd.nonblock();
                let r = self.wait_event(None, |p| end.pipe.try_read(buf, nonblock, p));
                if r.is_err() {
                    end.pipe.unregister(&self.parker);
                }
                r
            }
            FileObj::Dev { dev: Device::Pty(end), .. } => self.tty_read(ofd, end, buf),
            FileObj::Dev { dev, .. } => dev.read(buf),
            FileObj::Path { .. } => Err(Errno::EBADF),
            FileObj::Listener(_) | FileObj::Stream(_) => self.stream_recv(ofd, buf, MsgFlags::empty()),
            FileObj::Unix(u) => {
                if u.ty == crate::unix::SOCK_DGRAM {
                    return Ok(truncated_copy(&self.unix_recv(ofd, u, MsgFlags::empty())?.data, buf));
                }
                if u.ty == crate::unix::SOCK_SEQPACKET {
                    return Ok(truncated_copy(&self.seq_recv(ofd, u, MsgFlags::empty())?.0, buf));
                }
                self.stream_recv(ofd, buf, MsgFlags::empty())
            }
            FileObj::Udp(u) => Ok(truncated_copy(&self.udp_recv(ofd, u, false)?.0, buf)),
            // O `eventpoll_fops` e o `pidfd_fops` não têm `read`.
            FileObj::Epoll(_) | FileObj::Pidfd(_) => Err(Errno::EINVAL),
            FileObj::Anon(a) => self.anon_read(ofd, a, buf),
        }
    }

    /// `read` num eventfd ou timerfd: oito bytes com o contador (EINVAL se o buffer é menor), esperando se o fd é
    /// bloqueante. O timerfd dorme até o prazo do timer e tenta de novo.
    fn anon_read(&self, ofd: &Arc<Ofd>, anon: &crate::anon::Anon, buf: &mut [u8]) -> SysResult<usize> {
        if buf.len() < 8 {
            return Err(Errno::EINVAL);
        }
        let nonblock = ofd.nonblock();
        let value = match anon {
            crate::anon::Anon::Eventfd(e) => {
                let r = self.wait_event(None, |p| e.try_read(nonblock, p));
                if r.is_err() {
                    e.unregister(&self.parker);
                }
                r?
            }
            crate::anon::Anon::Timerfd(t) => loop {
                match self.wait_event(t.deadline(), |p| t.try_read(nonblock, p)) {
                    Err(Errno::ETIMEDOUT) => continue,
                    Err(e) => {
                        t.unregister(&self.parker);
                        return Err(e);
                    }
                    Ok(v) => break v,
                }
            },
        };
        buf[..8].copy_from_slice(&value.to_ne_bytes());
        Ok(8)
    }

    /// `write` num eventfd: soma o valor de oito bytes ao contador (EINVAL com menos bytes ou com `ULLONG_MAX`).
    /// O timerfd não tem `write`.
    fn anon_write(&self, ofd: &Arc<Ofd>, anon: &crate::anon::Anon, buf: &[u8]) -> SysResult<usize> {
        let crate::anon::Anon::Eventfd(e) = anon else { return Err(Errno::EINVAL) };
        let Some(bytes) = buf.get(..8) else { return Err(Errno::EINVAL) };
        let value = u64::from_ne_bytes(bytes.try_into().expect("oito bytes"));
        if value == u64::MAX {
            return Err(Errno::EINVAL);
        }
        let nonblock = ofd.nonblock();
        let r = self.wait_event(None, |p| e.try_write(value, nonblock, p));
        if r.is_err() {
            e.unregister(&self.parker);
        }
        r.map(|()| 8)
    }

    /// O próximo datagrama de um socket UDP, esperando se o fd é bloqueante.
    fn udp_recv(&self, ofd: &Arc<Ofd>, u: &Arc<crate::udp::UdpSock>, peek: bool) -> SysResult<(Vec<u8>, (std::net::IpAddr, u16))> {
        let nonblock = ofd.nonblock();
        let r = self.wait_event(None, |p| u.try_recv(peek, nonblock, p));
        if r.is_err() {
            u.unregister(&self.parker);
        }
        r
    }

    /// A conexão de um socket de fluxo (TCP, ou Unix de fluxo) se ele já tem uma, passada a `f`.
    fn with_conn<R>(ofd: &Ofd, f: impl FnOnce(&crate::net::Conn) -> R) -> Option<R> {
        match &ofd.obj {
            FileObj::Stream(c) => Some(f(c)),
            FileObj::Listener(l) => l.conn().map(|c| f(&c)),
            FileObj::Unix(u) => u.conn().map(|c| f(&c)),
            _ => None,
        }
    }

    /// `recv` num socket de fluxo. Sem conexão: o Unix dá EINVAL (`unix_stream_read_generic`) e o TCP segue o
    /// `tcp_recvmsg` com o `sk_state` em CLOSE: em escuta ENOTCONN; com o `sk_err` de um `connect` recusado, ele sai
    /// uma vez; depois do RST desse `connect` a leitura dá 0 (`RCV_SHUTDOWN`); nunca conectado, ENOTCONN.
    fn stream_recv(&self, ofd: &Arc<Ofd>, buf: &mut [u8], flags: MsgFlags) -> SysResult<usize> {
        self.stream_recv_scm(ofd, buf, flags).map(|(n, _)| n)
    }

    /// [`Task::stream_recv`] com os dados auxiliares do `sk_buff` lido (só um fluxo Unix os tem).
    fn stream_recv_scm(&self, ofd: &Arc<Ofd>, buf: &mut [u8], flags: MsgFlags) -> SysResult<(usize, Scm)> {
        let conn = match &ofd.obj {
            FileObj::Stream(c) => return self.stream_read(ofd, c, buf, flags),
            FileObj::Listener(l) => match l.conn() {
                Some(c) => c,
                None if l.is_listening() => return Err(Errno::ENOTCONN),
                None => {
                    Self::settle_syn(ofd);
                    let mut meta = ofd.sock.lock();
                    return match std::mem::take(&mut meta.error) {
                        0 if meta.failed => Ok((0, Scm::default())),
                        0 => Err(Errno::ENOTCONN),
                        err => Err(Errno(err)),
                    };
                }
            },
            FileObj::Unix(u) if u.ty == crate::unix::SOCK_STREAM => u.conn().ok_or(Errno::EINVAL)?,
            _ => return Err(Errno::ENOTSOCK),
        };
        self.stream_read(ofd, &conn, buf, flags)
    }

    /// `send` num socket de fluxo. Sem conexão: o Unix dá ENOTCONN (`unix_stream_sendmsg`) e o TCP, em CLOSE ou
    /// LISTEN, dá o `sk_err` pendente (uma vez) ou EPIPE, com SIGPIPE salvo `MSG_NOSIGNAL` (`sk_stream_wait_connect`).
    fn stream_send(&self, ofd: &Arc<Ofd>, buf: &[u8], flags: MsgFlags) -> SysResult<usize> {
        let conn = match &ofd.obj {
            FileObj::Stream(c) => return self.stream_write(ofd, c, buf, flags, None),
            FileObj::Listener(l) => match l.conn() {
                Some(c) => c,
                None => {
                    let err = std::mem::take(&mut ofd.sock.lock().error);
                    if err != 0 {
                        return Err(Errno(err));
                    }
                    if !flags.contains(MsgFlags::NOSIGNAL) {
                        self.sigpipe();
                    }
                    return Err(Errno::EPIPE);
                }
            },
            FileObj::Unix(u) if u.ty == crate::unix::SOCK_STREAM => u.conn().ok_or(Errno::ENOTCONN)?,
            _ => return Err(Errno::ENOTSOCK),
        };
        // Um socket Unix leva as credenciais de quem escreve junto com os bytes (`SCM_CREDENTIALS`).
        let scm = matches!(ofd.obj, FileObj::Unix(_)).then(|| self.plain_scm());
        self.stream_write(ofd, &conn, buf, flags, scm.as_ref())
    }

    /// Os dados auxiliares de uma escrita comum: nenhum descritor e as credenciais reais de quem escreve
    /// (`scm_set_cred` com `task_tgid`, `current_uid` e `current_gid`).
    fn plain_scm(&self) -> Scm {
        let cred = self.proc.st.lock().cred.clone();
        Scm { fds: Vec::new(), cred: Ucred { pid: self.proc.pid, uid: cred.ruid, gid: cred.rgid } }
    }

    /// As credenciais efetivas de quem chama, o que um `listen`, um `connect` e um `socketpair` deixam como as do
    /// par (`SO_PEERCRED`).
    fn peer_ucred(&self) -> Ucred {
        let cred = self.proc.st.lock().cred.clone();
        Ucred { pid: self.proc.pid, uid: cred.uid, gid: cred.gid }
    }

    /// SIGPIPE de uma escrita que deu EPIPE.
    fn sigpipe(&self) {
        generate_signal(&self.proc, Signal::SIGPIPE);
        self.enter();
    }

    /// Leitura de uma conexão de fluxo (`tcp_recvmsg`, `unix_stream_read_generic`): o que já chegou sai antes
    /// de qualquer erro; só com a fila vazia o `sk_err` de um RST é entregue, uma vez, e depois vem o fim.
    /// `MSG_WAITALL` insiste até `buf.len()` bytes e para antes no fim, num erro ou, sem bloqueio (fd
    /// `O_NONBLOCK` ou `MSG_DONTWAIT`), quando a fila esvazia; `MSG_PEEK` olha a fila sem consumi-la. Um `sk_buff`
    /// Unix com descritores encerra a leitura (`unix_stream_read_generic`): eles vêm no [`Scm`] devolvido.
    fn stream_read(&self, ofd: &Arc<Ofd>, c: &crate::net::Conn, buf: &mut [u8], flags: MsgFlags) -> SysResult<(usize, Scm)> {
        if buf.is_empty() {
            return Ok((0, Scm::default()));
        }
        let nonblock = ofd.nonblock() || flags.contains(MsgFlags::DONTWAIT);
        let passcred = matches!(&ofd.obj, FileObj::Unix(u) if u.passcred());
        let target = if flags.contains(MsgFlags::WAITALL) { buf.len() } else { 1 };
        let mut done = 0usize;
        let mut scm = Scm::default();
        let eof = loop {
            // O `shutdown(SHUT_RD)` marca o pipe: o que já chegou sai e a fila vazia dá 0, sem esperar o par.
            let Some(pipe) = c.rx() else { break true };
            let rest = &mut buf[done..];
            let r = if flags.contains(MsgFlags::PEEK) {
                self.wait_event(None, |p| pipe.try_peek(rest, target, nonblock, p, passcred))
            } else {
                self.wait_event(None, |p| pipe.try_read_scm(rest, nonblock, p, passcred))
            };
            match r {
                Ok((0, _)) => break true,
                Ok((n, got)) => {
                    done += n;
                    let stop = !got.fds.is_empty();
                    if done == n {
                        scm.cred = got.cred;
                    }
                    scm.fds.extend(got.fds);
                    if stop || flags.contains(MsgFlags::PEEK) || done >= target {
                        break false;
                    }
                }
                Err(e) => {
                    pipe.unregister(&self.parker);
                    if done > 0 {
                        break false;
                    }
                    return Err(e);
                }
            }
        };
        if eof && done == 0 && let Some(e) = c.eof_error() {
            return Err(e);
        }
        Ok((done, scm))
    }

    /// A próxima mensagem de um socket Unix de datagrama, esperando se o fd é bloqueante (e `MSG_DONTWAIT` não está).
    fn unix_recv(&self, ofd: &Arc<Ofd>, u: &Arc<crate::unix::UnixSock>, flags: MsgFlags) -> SysResult<Datagram> {
        let nonblock = ofd.nonblock() || flags.contains(MsgFlags::DONTWAIT);
        let peek = flags.contains(MsgFlags::PEEK);
        let r = self.wait_event(None, |p| u.try_recv(peek, nonblock, p));
        if r.is_err() {
            u.unregister(&self.parker);
        }
        r
    }

    /// A próxima mensagem de um socket Unix de seqpacket, esperando se o fd é bloqueante. ENOTCONN sem
    /// conexão (`unix_seqpacket_recvmsg`).
    fn seq_recv(&self, ofd: &Arc<Ofd>, u: &Arc<crate::unix::UnixSock>, flags: MsgFlags) -> SysResult<(Vec<u8>, Scm)> {
        let end = u.seq().ok_or(Errno::ENOTCONN)?;
        let nonblock = ofd.nonblock() || flags.contains(MsgFlags::DONTWAIT);
        let peek = flags.contains(MsgFlags::PEEK);
        let r = self.wait_event(None, |p| end.try_recv(peek, nonblock, p));
        if r.is_err() {
            end.unregister(&self.parker);
        }
        r
    }

    /// Uma mensagem num socket Unix de seqpacket (`unix_seqpacket_sendmsg`): ENOTCONN sem conexão e
    /// SIGPIPE com EPIPE (salvo `MSG_NOSIGNAL`).
    fn seq_send(&self, ofd: &Arc<Ofd>, u: &Arc<crate::unix::UnixSock>, buf: &[u8], scm: &Scm, flags: MsgFlags) -> SysResult<usize> {
        let end = u.seq().ok_or(Errno::ENOTCONN)?;
        let nonblock = ofd.nonblock() || flags.contains(MsgFlags::DONTWAIT);
        let r = self.wait_event(None, |p| end.try_send(buf, scm, nonblock, p));
        if r.is_err() {
            end.unregister(&self.parker);
        }
        if r == Err(Errno::EPIPE) && !flags.contains(MsgFlags::NOSIGNAL) {
            self.sigpipe();
        }
        r
    }

    /// Escrita numa conexão de fluxo (`tcp_sendmsg`, `unix_stream_sendmsg`): a verificação de erro e de
    /// `shutdown` de [`crate::net::Conn::tx_for_write`] vem antes dos dados, e o EPIPE gera SIGPIPE, salvo
    /// `MSG_NOSIGNAL` (o `sk_stream_error` só o gera quando o erro que sobra é EPIPE, não o `ECONNRESET` pendente).
    fn stream_write(&self, ofd: &Arc<Ofd>, c: &crate::net::Conn, buf: &[u8], flags: MsgFlags, send: Option<&Scm>) -> SysResult<usize> {
        let sigpipe = !flags.contains(MsgFlags::NOSIGNAL);
        match c.tx_for_write() {
            Ok(Some(pipe)) => self.pipe_write(&pipe, buf, ofd.nonblock() || flags.contains(MsgFlags::DONTWAIT), sigpipe, send),
            Ok(None) => Ok(buf.len()),
            Err(Errno::EPIPE) => {
                if sigpipe {
                    self.sigpipe();
                }
                Err(Errno::EPIPE)
            }
            Err(e) => Err(e),
        }
    }

    /// Um datagrama de um socket Unix para `target` (ou para o par do `connect`).
    fn unix_send(&self, ofd: &Arc<Ofd>, u: &Arc<crate::unix::UnixSock>, buf: &[u8], target: Option<&Arc<crate::unix::UnixSock>>, scm: &Scm, flags: MsgFlags) -> SysResult<usize> {
        let nonblock = ofd.nonblock() || flags.contains(MsgFlags::DONTWAIT);
        let r = self.wait_event(None, |p| u.try_send(buf, scm, target, nonblock, p));
        if r.is_err() {
            u.unregister(&self.parker);
        }
        r
    }

    /// Envia uma mensagem por um socket Unix de qualquer tipo, com os dados auxiliares `scm`. Com endereço, o
    /// seqpacket conectado dá EISCONN e o sem conexão EOPNOTSUPP (`unix_seqpacket_sendmsg`), e o fluxo o mesmo
    /// (`unix_stream_sendmsg`); sem conexão o fluxo dá ENOTCONN. Descritores num fluxo sem dados não viajam.
    fn unix_send_msg(&self, ofd: &Arc<Ofd>, u: &Arc<crate::unix::UnixSock>, data: &[u8], name: Option<&[u8]>, scm: &Scm, flags: MsgFlags) -> SysResult<usize> {
        if u.ty != crate::unix::SOCK_DGRAM {
            if name.is_some() {
                let connected = if u.ty == crate::unix::SOCK_SEQPACKET { u.seq().is_some() } else { u.conn().is_some() };
                return Err(if connected { Errno::EISCONN } else { Errno::EOPNOTSUPP });
            }
            if u.ty == crate::unix::SOCK_SEQPACKET {
                return self.seq_send(ofd, u, data, scm, flags);
            }
            return match u.conn() {
                Some(c) => self.stream_write(ofd, &c, data, flags, Some(scm)),
                None => Err(Errno::ENOTCONN),
            };
        }
        let target = match name {
            Some(n) => Some(self.unix_find(n)?),
            None => None,
        };
        self.unix_send(ofd, u, data, target.as_ref(), scm, flags)
    }

    /// Recebe de um socket Unix de qualquer tipo: os dados (cortados em `max`), o nome de quem enviou, os dados
    /// auxiliares e se a mensagem era maior que `max` (só datagrama e seqpacket; o fluxo devolve o que pediram).
    #[allow(clippy::type_complexity)]
    fn unix_recv_any(&self, ofd: &Arc<Ofd>, u: &Arc<crate::unix::UnixSock>, max: usize, flags: MsgFlags) -> SysResult<(Vec<u8>, Option<Vec<u8>>, Scm, bool)> {
        if u.ty == crate::unix::SOCK_SEQPACKET {
            let (mut data, scm) = self.seq_recv(ofd, u, flags)?;
            let cut = data.len() > max;
            data.truncate(max);
            return Ok((data, None, scm, cut));
        }
        if u.ty != crate::unix::SOCK_DGRAM {
            let mut buf = vec![0u8; max];
            let (n, scm) = match u.conn() {
                Some(c) => self.stream_read(ofd, &c, &mut buf, flags)?,
                None => return Err(Errno::EINVAL),
            };
            buf.truncate(n);
            return Ok((buf, None, scm, false));
        }
        let Datagram { mut data, from, scm } = self.unix_recv(ofd, u, flags)?;
        let cut = data.len() > max;
        data.truncate(max);
        Ok((data, from, scm, cut))
    }

    /// Os dados auxiliares de um `sendmsg` (`__scm_send`): `SCM_RIGHTS` resolve os descritores (EBADF), e
    /// `SCM_CREDENTIALS` troca as credenciais de quem envia se elas forem as dele (ou se ele é root). Itens de
    /// outro nível são ignorados; tipo desconhecido em `SOL_SOCKET` é EINVAL.
    fn scm_from_control(&self, control: &[u8]) -> SysResult<Scm> {
        use sysabi::cmsg;
        // O `____sys_sendmsg` copia o controle para a pilha (36 bytes) ou para memória do socket, que o
        // `optmem_max` limita.
        if control.len() > 36 && control.len() >= cmsg::OPTMEM_MAX {
            return Err(Errno::ENOBUFS);
        }
        let mut scm = self.plain_scm();
        for item in cmsg::parse(control)? {
            if item.level != cmsg::SOL_SOCKET {
                continue;
            }
            match item.kind {
                cmsg::SCM_RIGHTS => {
                    let num = item.data.len() / 4;
                    if num == 0 {
                        continue;
                    }
                    if num > cmsg::SCM_MAX_FD || scm.fds.len() + num > cmsg::SCM_MAX_FD {
                        return Err(Errno::EINVAL);
                    }
                    for raw in item.data.chunks_exact(4) {
                        let fd = i32::from_le_bytes(raw.try_into().unwrap());
                        if fd < 0 {
                            return Err(Errno::EBADF);
                        }
                        scm.fds.push(self.ofd(Fd(fd))?);
                    }
                }
                cmsg::SCM_CREDENTIALS => {
                    let given = Ucred::from_bytes(item.data).ok_or(Errno::EINVAL)?;
                    self.check_ucred(given)?;
                    scm.cred = given;
                }
                _ => return Err(Errno::EINVAL),
            }
        }
        Ok(scm)
    }

    /// `scm_check_creds`: o pid é o do processo, o uid e o gid são o real, o efetivo ou o salvo, a menos que
    /// quem envia seja root (`CAP_SYS_ADMIN`, `CAP_SETUID`, `CAP_SETGID`). uid ou gid -1 é EINVAL.
    fn check_ucred(&self, given: Ucred) -> SysResult<()> {
        if given.uid == u32::MAX || given.gid == u32::MAX {
            return Err(Errno::EINVAL);
        }
        let cred = self.proc.st.lock().cred.clone();
        let root = cred.is_root();
        let uid_ok = [cred.ruid, cred.uid, cred.suid].contains(&given.uid);
        let gid_ok = [cred.rgid, cred.gid, cred.sgid].contains(&given.gid);
        if (given.pid == self.proc.pid || root) && (uid_ok || root) && (gid_ok || root) {
            Ok(())
        } else {
            Err(Errno::EPERM)
        }
    }

    /// `scm_detach_fds`: instala em quem recebe os descritores que cabem no `msg_control` (cada um vira o menor fd
    /// livre) e grava o item `SCM_RIGHTS`; o que não coube, ou não coube instalar, marca o corte. Os arquivos que
    /// sobram fecham com o `Scm`.
    fn detach_fds(&self, files: &[Arc<Ofd>], control: &mut sysabi::cmsg::Builder, cloexec: bool) {
        if files.is_empty() {
            return;
        }
        let limit = self.nofile();
        let mut installed = Vec::new();
        for ofd in files.iter().take(control.fd_room()) {
            match self.proc.fds.lock().install(ofd.clone(), cloexec, 0, limit) {
                Ok(fd) => installed.push(fd.0),
                Err(_) => break,
            }
        }
        if !installed.is_empty() {
            control.put_fds(&installed);
        }
        if installed.len() < files.len() {
            control.truncated = true;
        }
    }

    /// O socket UDP de `fd`; ENOTSOCK se não é um.
    fn udp_of(&self, fd: Fd) -> SysResult<Arc<crate::udp::UdpSock>> {
        match &self.ofd(fd)?.obj {
            FileObj::Udp(u) => Ok(u.clone()),
            _ => Err(Errno::ENOTSOCK),
        }
    }

    /// O socket Unix de `fd`; ENOTSOCK se não é um.
    fn unix_of(&self, fd: Fd) -> SysResult<Arc<crate::unix::UnixSock>> {
        match &self.ofd(fd)?.obj {
            FileObj::Unix(u) => Ok(u.clone()),
            _ => Err(Errno::ENOTSOCK),
        }
    }

    /// O socket de um nome, como o `unix_find_other`: o caminho resolve (ENOENT...), precisa de
    /// permissão de escrita (EACCES) e de ser um socket com alguém ligado a ele (ECONNREFUSED).
    fn unix_find(&self, name: &[u8]) -> SysResult<Arc<crate::unix::UnixSock>> {
        if name.is_empty() || name.len() > 107 {
            return Err(Errno::EINVAL);
        }
        if name[0] == 0 {
            return self.sb.unix.lookup(&crate::unix::Key::Abstract(name.to_vec())).ok_or(Errno::ECONNREFUSED);
        }
        let st = self.fstatat(Fd::CWD, name, AtFlags::empty())?;
        // `AT_EACCESS`: a permissão é a do uid efetivo, como o `path_permission` do kernel.
        self.faccessat(Fd::CWD, name, sysabi::AccessMode::W_OK, AtFlags::REMOVEDIR)?;
        if st.mode & sysabi::mode::S_IFMT != sysabi::mode::S_IFSOCK {
            return Err(Errno::ECONNREFUSED);
        }
        self.sb.unix.lookup(&crate::unix::Key::Node(st.dev, st.ino)).ok_or(Errno::ECONNREFUSED)
    }

    /// Um pipe novo do processo: cada sentido de uma conexão TCP de loopback, e a identidade de um socket.
    fn sock_pipe(&self) -> Arc<Pipe> {
        new_sock_pipe(&self.sb, &self.proc.st.lock().cred)
    }

    /// [`Task::sock_pipe`] sem a tarefa: para a thread das retransmissões de um SYN, que vive além do `connect`.
    fn sock_pipe_maker(&self) -> crate::net::MkPipe {
        let (sb, cred) = (self.sb.clone(), self.proc.st.lock().cred.clone());
        Arc::new(move || new_sock_pipe(&sb, &cred))
    }

    /// `connect` bloqueante de um socket: o SYN descartado pela fila cheia do ouvinte espera as retransmissões
    /// (a thread do socket acorda quem espera), até a conexão, o `ECONNREFUSED` ou o `ETIMEDOUT` de 127 s.
    fn connect_wait(&self, l: &Arc<crate::net::Listener>, ip: std::net::IpAddr, port: u16) -> SysResult<Arc<crate::net::Conn>> {
        if let Some(conn) = l.connect(&self.sb.ports, ip, port, self.sock_pipe_maker())? {
            return Ok(conn);
        }
        self.wait_syn(l)
    }

    /// Espera o `connect` pendente de `l` terminar; interrompido por sinal, o SYN segue em SYN_SENT.
    fn wait_syn(&self, l: &Arc<crate::net::Listener>) -> SysResult<Arc<crate::net::Conn>> {
        let r = self.wait_event(None, |p| l.try_syn(p));
        if r.is_err() {
            l.unregister(&self.parker);
        }
        r
    }

    /// O fim de um `connect` bloqueante com o SYN pendente. Interrompido por sinal, o socket segue em SYN_SENT.
    fn finish_connect(&self, ofd: &Ofd, l: &Arc<crate::net::Listener>) -> SysResult<()> {
        let r = self.wait_syn(l);
        let mut meta = ofd.sock.lock();
        meta.connecting = l.syn_sent();
        meta.sndbuf_expanded |= r.is_ok();
        r.map(|_| ())
    }

    /// `socket` + `connect` bloqueante para quem recebe só a conexão (sem o fd do socket).
    fn connect_new(&self, ip: std::net::IpAddr, port: u16) -> SysResult<crate::net::Conn> {
        let l = self.sb.ports.socket(ip.is_ipv6(), self.sock_pipe());
        let conn = self.connect_wait(&l, ip, port)?;
        drop(l);
        Arc::into_inner(conn).ok_or(Errno::EAGAIN)
    }

    /// Leva para o `SockMeta` o `connect` que falhou sem ninguém esperando (o `sk_err` do `tcp_write_err`).
    fn settle_syn(ofd: &Ofd) {
        if let FileObj::Listener(l) = &ofd.obj && let Some(e) = l.take_syn_failure() {
            let mut meta = ofd.sock.lock();
            meta.error = e.0;
            meta.failed = true;
            meta.connecting = true;
        }
    }

    /// Instala um socket (sempre `O_RDWR`) no menor fd livre.
    fn install_sock(&self, obj: FileObj, nonblock: bool, cloexec: bool) -> SysResult<Fd> {
        let flags = if nonblock { OFlags::RDWR | OFlags::NONBLOCK } else { OFlags::RDWR };
        let ofd = Ofd::new(obj, flags, Arc::downgrade(&self.sb.locks));
        let limit = self.nofile();
        self.proc.fds.lock().install(ofd, cloexec, 0, limit)
    }

    /// Escrita num pipe (anônimo, FIFO ou o sentido de saída de uma conexão TCP de loopback).
    fn pipe_write(&self, pipe: &Arc<Pipe>, buf: &[u8], nonblock: bool, sigpipe: bool, send: Option<&Scm>) -> SysResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut done = 0usize;
        let r = self.wait_event(None, |p| match pipe.try_write_with(buf, &mut done, nonblock, p, send) {
            Try::Ready(Ok(n)) => Try::Ready(Ok(Ok(n))),
            Try::Ready(Err(e)) => Try::Ready(Ok(Err(e))),
            Try::Pending => Try::Pending,
        });
        match r {
            Ok(Ok(n)) => Ok(n),
            Ok(Err(WriteError::Again)) => Err(Errno::EAGAIN),
            Ok(Err(WriteError::BrokenPipe { written })) => {
                if sigpipe {
                    self.sigpipe();
                }
                if written > 0 { Ok(written) } else { Err(Errno::EPIPE) }
            }
            Err(e) => {
                pipe.unregister(&self.parker);
                if done > 0 { Ok(done) } else { Err(e) }
            }
        }
    }

    fn do_write(&self, ofd: &Arc<Ofd>, buf: &[u8], at: Option<u64>) -> SysResult<usize> {
        if matches!(ofd.obj, FileObj::Path { .. }) || !ofd.writable {
            return Err(Errno::EBADF);
        }
        if at.is_some() && unseekable(&ofd.obj) {
            return Err(Errno::ESPIPE);
        }
        match &ofd.obj {
            FileObj::Vfs { kind: FileType::Directory, .. } => Err(Errno::EISDIR),
            FileObj::Vfs { handle, .. } => {
                let cx = self.caller();
                let r = match at {
                    Some(off) => {
                        // pwrite em O_APPEND escreve no fim (comportamento do Linux).
                        let pos = if ofd.status().contains(OFlags::APPEND) { WritePos::Append } else { WritePos::At(off) };
                        handle.write(&cx, pos, buf).map(|(n, _)| n)
                    }
                    None => {
                        let mut st = ofd.st.lock();
                        let pos = if st.status.contains(OFlags::APPEND) { WritePos::Append } else { WritePos::At(st.pos) };
                        let off = match pos {
                            WritePos::At(o) => o,
                            WritePos::Append => 0,
                        };
                        match handle.write(&cx, pos, buf) {
                            Ok((n, end)) => {
                                // Escrita de 0 bytes não mexe no f_pos (nem com O_APPEND).
                                if n > 0 {
                                    st.pos = end;
                                }
                                Ok(n)
                            }
                            Err(Errno::EFBIG) if off >= cx.fsize_limit || matches!(pos, WritePos::Append) => {
                                drop(st);
                                self.raise_xfsz(&cx, ofd)?;
                                Err(Errno::EFBIG)
                            }
                            Err(e) => Err(e),
                        }
                    }
                };
                if let (Err(Errno::EFBIG), Some(off)) = (&r, at)
                    && off >= cx.fsize_limit
                {
                    generate_signal(&self.proc, Signal::SIGXFSZ);
                    self.enter();
                }
                r
            }
            FileObj::Pipe { end, .. } => self.pipe_write(&end.pipe, buf, ofd.nonblock(), true, None),
            FileObj::Listener(_) | FileObj::Stream(_) => self.stream_send(ofd, buf, MsgFlags::empty()),
            FileObj::Unix(u) => self.unix_send_msg(ofd, u, buf, None, &self.plain_scm(), MsgFlags::empty()),
            FileObj::Udp(u) => self.sb.udp.send(u, buf, None),
            FileObj::Epoll(_) | FileObj::Pidfd(_) => Err(Errno::EINVAL),
            FileObj::Anon(a) => self.anon_write(ofd, a, buf),
            FileObj::Dev { dev: Device::Pty(end), .. } => self.tty_write(ofd, end, buf),
            FileObj::Dev { dev, .. } => dev.write(buf),
            FileObj::Path { .. } => Err(Errno::EBADF),
        }
    }

    /// SIGXFSZ de uma escrita no limite de RLIMIT_FSIZE (com O_APPEND, o fim do arquivo é o deslocamento).
    fn raise_xfsz(&self, cx: &Caller, ofd: &Ofd) -> SysResult<()> {
        let size = match &ofd.obj {
            FileObj::Vfs { loc, .. } => self.sb.ns.stat_loc(cx, loc)?.size,
            _ => 0,
        };
        let at = if ofd.status().contains(OFlags::APPEND) { size } else { ofd.st.lock().pos };
        if at >= cx.fsize_limit {
            generate_signal(&self.proc, Signal::SIGXFSZ);
            self.enter();
        }
        Ok(())
    }

    // ------------------------------------------------------------------------------------------
    // Processos
    // ------------------------------------------------------------------------------------------

    fn find_procs(&self, target: KillTarget) -> SysResult<Vec<Arc<Proc>>> {
        let t = self.sb.table.lock();
        let my_pgid = t.rel(self.proc.pid).map(|r| r.pgid).unwrap_or(0);
        let v: Vec<Arc<Proc>> = match target {
            KillTarget::Pid(p) if p > 0 => t.proc(p).into_iter().collect(),
            KillTarget::Pid(0) | KillTarget::Group(0) => t.group_members(my_pgid),
            KillTarget::Pid(-1) | KillTarget::All => t
                .map
                .values()
                .filter(|e| e.proc.pid != INIT_PID && e.proc.pid != self.proc.pid && e.rel.zombie.is_none())
                .map(|e| e.proc.clone())
                .collect(),
            KillTarget::Pid(p) => t.group_members(-p),
            KillTarget::Group(g) if g > 0 => t.group_members(g),
            KillTarget::Group(_) => return Err(Errno::EINVAL),
        };
        Ok(v)
    }

    /// O `do_wait` de `wait4` e `waitid`: o filho e o que aconteceu a ele. `EXITED` liga o relato de
    /// término, `UNTRACED` o de parada e `CONTINUED` o de continuação; `NOWAIT` deixa o evento esperável.
    /// Devolve também o dono real do filho (o `si_uid` do `waitid`).
    fn wait_children(&self, target: WaitTarget, options: WaitOptions) -> Try<SysResult<Option<WaitInfo>>> {
        let mut t = self.sb.table.lock();
        let me = self.proc.pid;
        let Some(my) = t.rel(me) else { return Try::Ready(Err(Errno::ECHILD)) };
        let my_pgid = my.pgid;
        let kids: Vec<Pid> = my
            .children
            .iter()
            .copied()
            .filter(|k| {
                let Some(r) = t.rel(*k) else { return false };
                match target {
                    WaitTarget::Any => true,
                    WaitTarget::Pid(p) => *k == p,
                    WaitTarget::Group(0) => r.pgid == my_pgid,
                    WaitTarget::Group(g) => r.pgid == g,
                }
            })
            .collect();
        if kids.is_empty() {
            return Try::Ready(Err(Errno::ECHILD));
        }
        let keep = options.contains(WaitOptions::NOWAIT);
        let mut found = None;
        for k in &kids {
            let proc = t.proc(*k).expect("filho na tabela");
            let r = t.rel_mut(*k).expect("filho na tabela");
            if options.contains(WaitOptions::EXITED)
                && let Some((st, ru)) = r.zombie
            {
                if !keep {
                    t.reap(*k);
                }
                found = Some((*k, st, ru, proc));
                break;
            }
            if r.zombie.is_some() {
                continue;
            }
            if options.contains(WaitOptions::UNTRACED)
                && let Some(sig) = if keep { r.stop_report } else { r.stop_report.take() }
            {
                found = Some((*k, WaitStatus::Stopped(sig), Rusage::default(), proc));
                break;
            }
            if options.contains(WaitOptions::CONTINUED) && r.cont_report {
                r.cont_report &= keep;
                found = Some((*k, WaitStatus::Continued, Rusage::default(), proc));
                break;
            }
        }
        drop(t);
        if let Some((pid, status, rusage, proc)) = found {
            let uid = proc.st.lock().cred.ruid;
            return Try::Ready(Ok(Some(WaitInfo { pid, uid, status, rusage })));
        }
        if options.contains(WaitOptions::NOHANG) {
            return Try::Ready(Ok(None));
        }
        Try::Pending
    }

    /// `prepare_creds` + `commit_creds`: `f` muda uma cópia, que só entra se ele não falhar. Vale para
    /// o processo inteiro (a glibc propaga o `set*id` a todas as threads).
    fn change_cred(&self, f: impl FnOnce(&mut vfs::Cred) -> SysResult<()>) -> SysResult<()> {
        let mut st = self.proc.st.lock();
        let mut c = (*st.cred).clone();
        f(&mut c)?;
        if c != *st.cred {
            st.cred = Arc::new(c);
        }
        Ok(())
    }

    /// Onde começar a resolver o alvo de um `*xattr`: o fd de `EMPTY_PATH` (`fgetxattr`) ou o `dirfd` e o caminho.
    /// Um fd sem lugar no VFS (pipe, socket, anon_inode) não tem atributos estendidos.
    fn xattr_start(&self, dirfd: Fd, path: &[u8], flags: AtFlags) -> SysResult<Start> {
        if !(AtFlags::SYMLINK_NOFOLLOW | AtFlags::EMPTY_PATH).contains(flags) {
            return Err(Errno::EINVAL);
        }
        if path.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
            return match self.ofd(dirfd)?.loc() {
                Some(l) => Ok(Start::Dir(l.clone())),
                None => Err(Errno::EOPNOTSUPP),
            };
        }
        Ok(self.start_of(dirfd, path))
    }

    fn proc_attr_caller_check(&self, pid: Pid) -> SysResult<Arc<Proc>> {
        if pid == 0 {
            return Ok(self.proc.clone());
        }
        self.sb.table.lock().proc(pid).ok_or(Errno::ESRCH)
    }

    /// `__sched_setscheduler` sobre o processo `pid` (0 é o corrente). `build` recebe a nice atual do
    /// alvo e devolve o `SchedAttr` e se a política fica (`sched_setparam`). Quem o escalonador vê da
    /// nice (EEVDF) é atualizado se ela mudou.
    fn sched_change(&self, pid: Pid, build: impl FnOnce(i32) -> SysResult<(SchedAttr, bool)>) -> SysResult<()> {
        let p = self.proc_attr_caller_check(pid)?;
        let caller_uid = self.proc.st.lock().cred.uid;
        let same_owner = p.st.lock().cred.uid == caller_uid;
        if let Some(nice) = sched_apply(&p, same_owner, build)? {
            let tasks: Vec<Arc<Task>> = p.threads.lock().live.values().cloned().collect();
            for t in tasks {
                self.sb.kernel.cpus.set_nice(&t.ct, nice);
            }
        }
        Ok(())
    }

    /// Alvos de `ioprio_get`/`ioprio_set` (`IOPRIO_WHO_*`): `who` 0 é o processo, o grupo ou o usuário do
    /// chamador. `which` inválido é EINVAL; sem alvo a lista vem vazia. Zumbis não entram em grupo nem
    /// usuário.
    fn ioprio_targets(&self, which: i32, who: i32) -> SysResult<Vec<Arc<Proc>>> {
        match which {
            sched::IOPRIO_WHO_PROCESS => {
                let pid = if who == 0 { self.proc.pid } else { who };
                Ok(self.sb.table.lock().proc(pid).into_iter().collect())
            }
            sched::IOPRIO_WHO_PGRP => {
                let t = self.sb.table.lock();
                let pgid = if who == 0 { t.rel(self.proc.pid).map_or(0, |r| r.pgid) } else { who };
                Ok(t.group_members(pgid))
            }
            sched::IOPRIO_WHO_USER => {
                let uid = if who == 0 { self.proc.st.lock().cred.uid } else { who as u32 };
                if uid == u32::MAX {
                    return Ok(Vec::new());
                }
                let procs: Vec<Arc<Proc>> = {
                    let t = self.sb.table.lock();
                    t.map.values().filter(|e| e.rel.zombie.is_none() && e.proc.pid != INIT_PID).map(|e| e.proc.clone()).collect()
                };
                Ok(procs.into_iter().filter(|p| p.st.lock().cred.uid == uid).collect())
            }
            _ => Err(Errno::EINVAL),
        }
    }

    fn poll_one(&self, pfd: &PollFd, register: bool) -> PollEvents {
        if pfd.fd.0 < 0 {
            return PollEvents::empty();
        }
        let Ok(ofd) = self.ofd(pfd.fd) else { return PollEvents::NVAL };
        self.poll_ofd(&ofd, pfd.events, register.then_some(&self.parker))
    }

    /// A prontidão de uma descrição para os `events` pedidos (mais `ERR`, `HUP` e `NVAL`, que vêm sempre),
    /// registrando `waiter` nas filas do objeto quando há. Serve ao `poll(2)` e ao epoll.
    pub(crate) fn poll_ofd(&self, ofd: &Ofd, events: PollEvents, waiter: Option<&Arc<Parker>>) -> PollEvents {
        let w = waiter;
        let ready = match &ofd.obj {
            FileObj::Path { .. } => return PollEvents::NVAL,
            FileObj::Dev { dev: Device::Pty(end), .. } => end.pty.poll(end.master, w),
            FileObj::Vfs { .. } | FileObj::Dev { .. } => PollEvents::IN | PollEvents::OUT,
            FileObj::Pipe { end, .. } => end.pipe.poll(end.read, end.write, w),
            FileObj::Listener(l) => l.poll(w),
            FileObj::Stream(c) => c.poll(w),
            FileObj::Unix(u) => u.poll(w),
            FileObj::Udp(u) => u.poll(w),
            // O `ep_eventpoll_poll`: legível quando há evento pronto.
            FileObj::Epoll(e) if e.scan(self, 1, false, w).is_empty() => PollEvents::empty(),
            FileObj::Epoll(_) => PollEvents::IN,
            FileObj::Pidfd(p) => p.poll(w),
            FileObj::Anon(a) => a.poll(w),
        };
        // O `connect` recusado deixa o TCP em CLOSE com `SHUTDOWN_MASK`: `tcp_poll` dá `IN|OUT|HUP|RDHUP` até o
        // `connect` seguinte, e `ERR` enquanto o `SO_ERROR` não leu o erro.
        let ready = if matches!(ofd.obj, FileObj::Listener(_)) {
            Self::settle_syn(ofd);
            let meta = ofd.sock.lock();
            let mut ready = ready;
            if meta.failed {
                ready |= PollEvents::IN | PollEvents::OUT | PollEvents::HUP | PollEvents::RDHUP;
            }
            if meta.error != 0 {
                ready |= PollEvents::ERR;
            }
            ready
        } else {
            ready
        };
        ready & (events | PollEvents::ERR | PollEvents::HUP | PollEvents::NVAL)
    }

    /// `epoll_ctl` com a descrição de destino já resolvida, na ordem de validação do `do_epoll_ctl`.
    fn epoll_ctl_ofd(&self, ep: &Arc<Ofd>, epoll: &Arc<crate::epoll::Epoll>, op: i32, fd: Fd, target: &Arc<Ofd>, event: EpollEvent) -> SysResult<()> {
        use sysabi::epoll as ev;
        if Arc::ptr_eq(ep, target) {
            return Err(Errno::EINVAL);
        }
        let mut event = event;
        if op != ev::CTL_DEL {
            // `EPOLLWAKEUP` pede `CAP_BLOCK_SUSPEND`, que o contêiner padrão não tem: o kernel só apaga o bit.
            event.events = (event.events | ev::ERR | ev::HUP) & !ev::WAKEUP;
        }
        let target_is_epoll = matches!(target.obj, FileObj::Epoll(_));
        if event.events & ev::EXCLUSIVE != 0 {
            let ok_bits = ev::IN | ev::OUT | ev::ERR | ev::HUP | ev::WAKEUP | ev::ET | ev::EXCLUSIVE;
            if op == ev::CTL_MOD || (op == ev::CTL_ADD && (target_is_epoll || event.events & !ok_bits != 0)) {
                return Err(Errno::EINVAL);
            }
        }
        match op {
            ev::CTL_ADD => {
                if let FileObj::Epoll(inner) = &target.obj {
                    if inner.reaches(ep.id) {
                        return Err(Errno::ELOOP);
                    }
                    if inner.too_deep(1) || epoll.too_deep(inner.depth()) {
                        return Err(Errno::EINVAL);
                    }
                }
                epoll.add(fd, target, event)
            }
            ev::CTL_DEL => epoll.delete(fd, target),
            ev::CTL_MOD => {
                if epoll.events_of(fd, target).is_some_and(|e| e & ev::EXCLUSIVE != 0) {
                    return Err(Errno::EINVAL);
                }
                epoll.modify(fd, target, event)
            }
            _ => Err(Errno::EINVAL),
        }
    }

    /// `F_SETLK`, `F_SETLKW` e `F_OFD_SETLK*` com a faixa já resolvida: a trava de leitura pede descrição
    /// aberta pra leitura e a de escrita, pra escrita (EBADF).
    fn set_range_lock(&self, ofd: &Ofd, owner: u64, lock: FileLock, wait: bool) -> SysResult<()> {
        let key = lock_key_of(ofd).ok_or(Errno::EBADF)?;
        match lock.kind {
            LockKind::Read if !ofd.readable => return Err(Errno::EBADF),
            LockKind::Write if !ofd.writable => return Err(Errno::EBADF),
            _ => {}
        }
        if lock.start > i64::MAX as u64 {
            return Err(Errno::EINVAL);
        }
        self.lock_wait(key, owner, lock, wait)
    }

    /// `F_GETLK` e `F_OFD_GETLK`: a primeira trava de outro dono que conflita, com o dono dela.
    fn test_range_lock(&self, ofd: &Ofd, owner: u64, lock: FileLock) -> SysResult<Option<(FileLock, u64)>> {
        let key = lock_key_of(ofd).ok_or(Errno::EBADF)?;
        if lock.kind == LockKind::Unlock {
            return Err(Errno::EINVAL);
        }
        Ok(self.sb.locks.getlk(key, owner, &lock))
    }

    /// Toma (ou solta) uma trava de arquivo, esperando se `wait`. Conflito sem espera é EAGAIN; uma espera
    /// que fecharia um ciclo entre processos é EDEADLK; um sinal capturado é EINTR.
    fn lock_wait(&self, key: (u64, u64), owner: u64, lock: FileLock, wait: bool) -> SysResult<()> {
        let locks = self.sb.locks.clone();
        if !wait {
            return locks.setlk(key, owner, &lock, None).map_err(|_| Errno::EAGAIN);
        }
        let r = self.wait_event(None, |p| match locks.setlk(key, owner, &lock, Some(p)) {
            Ok(()) => Try::Ready(Ok(())),
            Err(LockFail::Conflict) => Try::Pending,
            Err(LockFail::Deadlock) => Try::Ready(Err(Errno::EDEADLK)),
        });
        locks.finish_wait(&self.parker, owner);
        r
    }

    /// `flock_to_posix_lock`: a faixa `[start, end]` do `struct flock` do programa, com o `whence` resolvido
    /// (`f_pos` ou o tamanho do arquivo) e os erros do Linux: `whence` ruim e faixa antes do zero são EINVAL, o
    /// que passa de `OFFSET_MAX` é EOVERFLOW; `l_len` 0 vai até o fim do arquivo e negativo termina antes de `start`.
    fn posix_lock_of(&self, ofd: &Ofd, f: &Flock) -> SysResult<FileLock> {
        let base = match f.whence {
            sysabi::fcntl::SEEK_SET => 0,
            sysabi::fcntl::SEEK_CUR => i128::from(ofd.st.lock().pos),
            sysabi::fcntl::SEEK_END => i128::from(ofd_stat_in(&self.sb, &self.caller(), ofd)?.size),
            _ => return Err(Errno::EINVAL),
        };
        let max = i128::from(i64::MAX);
        if i128::from(f.start) > max - base {
            return Err(Errno::EOVERFLOW);
        }
        let mut start = base + i128::from(f.start);
        if start < 0 {
            return Err(Errno::EINVAL);
        }
        let len = i128::from(f.len);
        let end = if len > 0 {
            if len - 1 > max - start {
                return Err(Errno::EOVERFLOW);
            }
            start + len - 1
        } else if len < 0 {
            if start + len < 0 {
                return Err(Errno::EINVAL);
            }
            let end = start - 1;
            start += len;
            end
        } else {
            max
        };
        if end < start {
            return Err(Errno::EOVERFLOW);
        }
        let kind = match f.l_type {
            sysabi::fcntl::F_RDLCK => LockKind::Read,
            sysabi::fcntl::F_WRLCK => LockKind::Write,
            sysabi::fcntl::F_UNLCK => LockKind::Unlock,
            _ => return Err(Errno::EINVAL),
        };
        Ok(FileLock { kind, start: start as u64, len: if end == max { 0 } else { (end - start + 1) as u64 } })
    }

    /// `locks_remove_posix` de um `close`: fechar qualquer fd de um arquivo solta as travas POSIX que este
    /// processo tem nele, mesmo que outros fds da mesma descrição sigam abertos.
    pub(crate) fn flush_posix_locks(&self, ofd: &Ofd) {
        if let Some(key) = lock_key_of(ofd) {
            self.sb.locks.release_posix(key, self.proc.pid);
        }
    }
}

/// O núcleo do `__sched_setscheduler` sobre um processo: `build` recebe a nice atual e devolve o `SchedAttr`
/// e se a política fica (`sched_setparam`). Devolve a nice nova quando ela mudou, pra quem já tem threads
/// avisar o escalonador.
pub(crate) fn sched_apply(p: &Proc, same_owner: bool, build: impl FnOnce(i32) -> SysResult<(SchedAttr, bool)>) -> SysResult<Option<i32>> {
    let mut st = p.st.lock();
    let (attr, keep_policy) = build(st.nice)?;
    let cx = SchedCaller {
        same_owner,
        rlim_rtprio: st.rlimits[Resource::Rtprio as usize].cur,
        rlim_nice: st.rlimits[Resource::Nice as usize].cur,
    };
    let mut tune = p.tune.lock();
    let (state, nice) = tune.sched.set(st.nice, &attr, keep_policy, &cx)?;
    tune.sched = state;
    drop(tune);
    let changed = nice != st.nice;
    st.nice = nice;
    Ok(changed.then_some(nice))
}

/// `(st_dev, st_ino)` do arquivo por trás de uma descrição: a chave das travas. Os descritores `O_PATH` não
/// têm (EBADF).
fn lock_key_of(ofd: &Ofd) -> Option<(u64, u64)> {
    let sock = |ident: &Pipe| {
        let st = crate::net::sock_stat(ident);
        Some((st.dev, st.ino))
    };
    match &ofd.obj {
        FileObj::Vfs { loc, .. } | FileObj::Pipe { fifo: Some(loc), .. } | FileObj::Dev { loc: Some(loc), .. } => Some((loc.fs().dev(), loc.ino)),
        FileObj::Path { .. } | FileObj::Dev { loc: None, .. } => None,
        FileObj::Pipe { end, fifo: None } => Some((crate::pipe::PIPEFS_DEV, end.pipe.ino)),
        FileObj::Listener(l) => sock(&l.ident),
        FileObj::Stream(c) => sock(&c.ident),
        FileObj::Unix(u) => sock(&u.ident),
        FileObj::Udp(u) => sock(&u.ident),
        FileObj::Epoll(_) | FileObj::Pidfd(_) | FileObj::Anon(_) => Some((crate::epoll::ANON_DEV, crate::epoll::ANON_INO)),
    }
}

/// Solta o registro de `waiter` nas filas dos objetos de uma descrição que o `poll`, o `epoll_wait` ou uma
/// entrada de epoll observou (o `ep_unregister_pollwait`).
pub(crate) fn unregister_ofd(ofd: &Ofd, waiter: &Arc<Parker>) {
    match &ofd.obj {
        FileObj::Pipe { end, .. } => end.pipe.unregister(waiter),
        FileObj::Listener(l) => l.unregister(waiter),
        FileObj::Stream(c) => c.unregister(waiter),
        FileObj::Unix(u) => u.unregister(waiter),
        FileObj::Udp(u) => u.unregister(waiter),
        FileObj::Dev { dev: Device::Pty(end), .. } => end.pty.unregister(waiter),
        FileObj::Epoll(e) => e.unregister(waiter),
        FileObj::Pidfd(p) => p.unregister(waiter),
        FileObj::Anon(a) => a.unregister(waiter),
        _ => {}
    }
}

/// `fstat` de uma descrição, para quem tem só o sandbox e o chamador (o `fdinfo` do epoll mostra o `st_dev`
/// e o `st_ino` de cada alvo).
pub(crate) fn ofd_stat_in(sb: &SbInner, cx: &Caller, ofd: &Ofd) -> SysResult<Stat> {
    match &ofd.obj {
        FileObj::Vfs { loc, .. } | FileObj::Path { loc, .. } => sb.ns.stat_loc(cx, loc),
        FileObj::Pipe { fifo: Some(l), .. } => sb.ns.stat_loc(cx, l),
        FileObj::Pipe { end, fifo: None } => Ok(end.pipe.stat(None)),
        FileObj::Dev { loc: Some(l), .. } => sb.ns.stat_loc(cx, l),
        FileObj::Dev { loc: None, .. } => Err(Errno::EBADF),
        FileObj::Listener(l) => Ok(crate::net::sock_stat(&l.ident)),
        FileObj::Stream(c) => Ok(crate::net::sock_stat(&c.ident)),
        FileObj::Unix(u) => Ok(crate::net::sock_stat(&u.ident)),
        FileObj::Udp(u) => Ok(crate::net::sock_stat(&u.ident)),
        // `anon_inode_make_secure_inode`: sem bits de tipo, só `0600`, o mesmo inode para todo anon_inode.
        FileObj::Epoll(_) | FileObj::Pidfd(_) | FileObj::Anon(_) => Ok(Stat {
            dev: crate::epoll::ANON_DEV,
            ino: crate::epoll::ANON_INO,
            mode: 0o600,
            nlink: 1,
            blksize: 4096,
            ..Stat::default()
        }),
    }
}

impl Syscalls for Task {
    // ---- arquivos ----

    fn openat(&self, dirfd: Fd, path: &[u8], flags: OFlags, mode: Mode) -> SysResult<Fd> {
        self.enter();
        if path.is_empty() {
            return Err(Errno::ENOENT);
        }
        if path.len() >= vfs::PATH_MAX {
            return Err(Errno::ENAMETOOLONG);
        }
        let limit = self.nofile();
        self.proc.fds.lock().lowest_free(0, limit)?;
        let cx = self.caller();
        let start = self.start_of(dirfd, path);
        let ofd = self.open_ofd(&cx, &start, path, flags, mode)?;
        let fd = self.install(ofd, flags.contains(OFlags::CLOEXEC))?;
        Ok(fd)
    }

    fn close(&self, fd: Fd) -> SysResult<()> {
        self.enter();
        let slot = self.proc.fds.lock().remove(fd).ok_or(Errno::EBADF)?;
        self.flush_posix_locks(&slot.ofd);
        drop(slot);
        Ok(())
    }

    fn read(&self, fd: Fd, buf: &mut [u8]) -> SysResult<usize> {
        self.enter();
        let ofd = self.ofd(fd)?;
        self.do_read(&ofd, buf, None)
    }

    fn write(&self, fd: Fd, buf: &[u8]) -> SysResult<usize> {
        self.enter();
        let ofd = self.ofd(fd)?;
        self.do_write(&ofd, buf, None)
    }

    fn pread(&self, fd: Fd, buf: &mut [u8], offset: u64) -> SysResult<usize> {
        self.enter();
        let ofd = self.ofd(fd)?;
        if offset > i64::MAX as u64 {
            return Err(Errno::EINVAL);
        }
        self.do_read(&ofd, buf, Some(offset))
    }

    fn pwrite(&self, fd: Fd, buf: &[u8], offset: u64) -> SysResult<usize> {
        self.enter();
        let ofd = self.ofd(fd)?;
        if offset > i64::MAX as u64 {
            return Err(Errno::EINVAL);
        }
        self.do_write(&ofd, buf, Some(offset))
    }

    fn lseek(&self, fd: Fd, offset: i64, whence: Whence) -> SysResult<u64> {
        self.enter();
        let ofd = self.ofd(fd)?;
        match &ofd.obj {
            FileObj::Pipe { .. } | FileObj::Listener(_) | FileObj::Stream(_) | FileObj::Unix(_) | FileObj::Udp(_) | FileObj::Epoll(_) | FileObj::Pidfd(_) => Err(Errno::ESPIPE),
            // `eventfd_fops` e `timerfd_fops` têm `.llseek = noop_llseek`: aceita e devolve `f_pos`, sempre 0.
            FileObj::Anon(_) => Ok(0),
            FileObj::Path { .. } => Err(Errno::EBADF),
            FileObj::Dev { dev, .. } => dev.lseek(),
            FileObj::Vfs { kind: FileType::Directory, .. } => {
                let mut st = ofd.st.lock();
                let new = match whence {
                    Whence::Set => offset,
                    Whence::Cur if offset == 0 => st.dir_cookie as i64,
                    _ => return Err(Errno::EINVAL),
                };
                if new < 0 {
                    return Err(Errno::EINVAL);
                }
                st.dir_cookie = new as u64;
                st.pos = new as u64;
                Ok(new as u64)
            }
            FileObj::Vfs { handle, .. } => {
                let cx = self.caller();
                let mut st = ofd.st.lock();
                let new: i128 = match whence {
                    Whence::Set => offset as i128,
                    Whence::Cur => st.pos as i128 + offset as i128,
                    Whence::End => {
                        if !handle.seek_end_allowed() {
                            return Err(Errno::EINVAL);
                        }
                        handle.size(&cx)? as i128 + offset as i128
                    }
                    Whence::Data | Whence::Hole => {
                        if offset < 0 {
                            return Err(Errno::ENXIO);
                        }
                        handle.seek_data(&cx, offset as u64, whence == Whence::Hole)? as i128
                    }
                };
                if new < 0 {
                    return Err(Errno::EINVAL);
                }
                if new > i64::MAX as i128 {
                    return Err(Errno::EOVERFLOW);
                }
                st.pos = new as u64;
                Ok(new as u64)
            }
        }
    }

    fn fstat(&self, fd: Fd) -> SysResult<Stat> {
        self.enter();
        let ofd = self.ofd(fd)?;
        self.ofd_stat(&ofd)
    }

    fn fstatat(&self, dirfd: Fd, path: &[u8], flags: AtFlags) -> SysResult<Stat> {
        self.enter();
        if path.is_empty() && flags.contains(AtFlags::EMPTY_PATH) && dirfd != Fd::CWD {
            if !(AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT | AtFlags::EMPTY_PATH).contains(flags) {
                return Err(Errno::EINVAL);
            }
            let ofd = self.ofd(dirfd)?;
            return self.ofd_stat(&ofd);
        }
        let cx = self.caller();
        let start = self.start_of(dirfd, path);
        Ok(self.sb.ns.stat(&cx, &start, path, flags)?.stat())
    }

    fn faccessat(&self, dirfd: Fd, path: &[u8], mode: AccessMode, flags: AtFlags) -> SysResult<()> {
        self.enter();
        let cx = self.caller();
        let start = self.start_of(dirfd, path);
        self.sb.ns.access(&cx, &start, path, mode, flags)
    }

    fn mkdirat(&self, dirfd: Fd, path: &[u8], mode: Mode) -> SysResult<()> {
        self.enter();
        let cx = self.caller();
        let start = self.start_of(dirfd, path);
        self.sb.ns.mkdir(&cx, &start, path, mode)
    }

    fn unlinkat(&self, dirfd: Fd, path: &[u8], flags: AtFlags) -> SysResult<()> {
        self.enter();
        let cx = self.caller();
        let start = self.start_of(dirfd, path);
        self.sb.ns.unlink(&cx, &start, path, flags)
    }

    fn renameat2(&self, olddir: Fd, old: &[u8], newdir: Fd, new: &[u8], flags: RenameFlags) -> SysResult<()> {
        self.enter();
        let cx = self.caller();
        let os = self.start_of(olddir, old);
        let ns = self.start_of(newdir, new);
        self.sb.ns.rename(&cx, &os, old, &ns, new, flags)
    }

    fn linkat(&self, olddir: Fd, old: &[u8], newdir: Fd, new: &[u8], flags: AtFlags) -> SysResult<()> {
        self.enter();
        let cx = self.caller();
        let os = if old.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
            match self.ofd(olddir) {
                Ok(o) => match o.loc() {
                    Some(l) => Start::Dir(l.clone()),
                    None => Start::Bad(Errno::EXDEV),
                },
                Err(e) => Start::Bad(e),
            }
        } else {
            self.start_of(olddir, old)
        };
        let ns = self.start_of(newdir, new);
        self.sb.ns.link(&cx, &os, old, &ns, new, flags)
    }

    fn symlinkat(&self, target: &[u8], dirfd: Fd, path: &[u8]) -> SysResult<()> {
        self.enter();
        let cx = self.caller();
        let start = self.start_of(dirfd, path);
        self.sb.ns.symlink(&cx, target, &start, path)
    }

    fn readlinkat(&self, dirfd: Fd, path: &[u8]) -> SysResult<Vec<u8>> {
        self.enter();
        let cx = self.caller();
        let start = if path.is_empty() {
            match self.ofd(dirfd) {
                Ok(o) => match o.loc() {
                    Some(l) => Start::Dir(l.clone()),
                    None => Start::Bad(Errno::ENOENT),
                },
                Err(_) => Start::Cwd,
            }
        } else {
            self.start_of(dirfd, path)
        };
        self.sb.ns.readlink(&cx, &start, path)
    }

    fn mknodat(&self, dirfd: Fd, path: &[u8], mode: Mode, dev: u64) -> SysResult<()> {
        self.enter();
        let cx = self.caller();
        let start = self.start_of(dirfd, path);
        self.sb.ns.mknod(&cx, &start, path, mode, dev)
    }

    fn fchmodat(&self, dirfd: Fd, path: &[u8], mode: Mode, flags: AtFlags) -> SysResult<()> {
        self.enter();
        let cx = self.caller();
        let start = if path.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
            match self.ofd(dirfd).map(|o| o.loc().cloned()) {
                Ok(Some(l)) => Start::Dir(l),
                Ok(None) => return Ok(()),
                Err(e) => Start::Bad(e),
            }
        } else {
            self.start_of(dirfd, path)
        };
        self.sb.ns.chmod(&cx, &start, path, mode, flags)
    }

    fn fchmod(&self, fd: Fd, mode: Mode) -> SysResult<()> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let cx = self.caller();
        match &ofd.obj {
            FileObj::Path { .. } => Err(Errno::EBADF),
            FileObj::Pipe { fifo: None, .. } | FileObj::Dev { loc: None, .. } | FileObj::Epoll(_) | FileObj::Pidfd(_) | FileObj::Anon(_) => Ok(()),
            _ => {
                let l = ofd.loc().expect("tem lugar").clone();
                let st = self.sb.ns.stat_loc(&cx, &l)?;
                self.sb.ns.chmod_loc(&cx, &l, &st, mode)
            }
        }
    }

    fn fchownat(&self, dirfd: Fd, path: &[u8], uid: Option<Uid>, gid: Option<Gid>, flags: AtFlags) -> SysResult<()> {
        self.enter();
        let cx = self.caller();
        let start = if path.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
            match self.ofd(dirfd).map(|o| o.loc().cloned()) {
                Ok(Some(l)) => Start::Dir(l),
                Ok(None) => return Ok(()),
                Err(e) => Start::Bad(e),
            }
        } else {
            self.start_of(dirfd, path)
        };
        self.sb.ns.chown(&cx, &start, path, uid, gid, flags)
    }

    fn getxattr(&self, dirfd: Fd, path: &[u8], flags: AtFlags, name: &[u8]) -> SysResult<Vec<u8>> {
        self.enter();
        let cx = self.caller();
        let start = self.xattr_start(dirfd, path, flags)?;
        self.sb.ns.getxattr(&cx, &start, path, name, flags)
    }

    fn setxattr(&self, dirfd: Fd, path: &[u8], flags: AtFlags, name: &[u8], value: &[u8], xflags: u32) -> SysResult<()> {
        self.enter();
        let cx = self.caller();
        let start = self.xattr_start(dirfd, path, flags)?;
        self.sb.ns.setxattr(&cx, &start, path, name, value, xflags, flags)
    }

    fn listxattr(&self, dirfd: Fd, path: &[u8], flags: AtFlags) -> SysResult<Vec<Vec<u8>>> {
        self.enter();
        let cx = self.caller();
        let start = self.xattr_start(dirfd, path, flags)?;
        self.sb.ns.listxattr(&cx, &start, path, flags)
    }

    fn removexattr(&self, dirfd: Fd, path: &[u8], flags: AtFlags, name: &[u8]) -> SysResult<()> {
        self.enter();
        let cx = self.caller();
        let start = self.xattr_start(dirfd, path, flags)?;
        self.sb.ns.removexattr(&cx, &start, path, name, flags)
    }

    fn utimensat(&self, dirfd: Fd, path: &[u8], atime: SetTime, mtime: SetTime, flags: AtFlags) -> SysResult<()> {
        self.enter();
        let cx = self.caller();
        let start = if path.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
            match self.ofd(dirfd).map(|o| o.loc().cloned()) {
                Ok(Some(l)) => Start::Dir(l),
                Ok(None) => return Ok(()),
                Err(e) => Start::Bad(e),
            }
        } else {
            self.start_of(dirfd, path)
        };
        self.sb.ns.utimens(&cx, &start, path, atime, mtime, flags)
    }

    fn futimens(&self, fd: Fd, atime: SetTime, mtime: SetTime) -> SysResult<()> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let cx = self.caller();
        for t in [atime, mtime] {
            if let SetTime::At(ts) = t
                && ts.nsec >= 1_000_000_000
            {
                return Err(Errno::EINVAL);
            }
        }
        match &ofd.obj {
            FileObj::Path { .. } => Err(Errno::EBADF),
            FileObj::Pipe { fifo: None, .. } | FileObj::Dev { loc: None, .. } | FileObj::Epoll(_) | FileObj::Pidfd(_) | FileObj::Anon(_) => Ok(()),
            _ => {
                let l = ofd.loc().expect("tem lugar").clone();
                let st = self.sb.ns.stat_loc(&cx, &l)?;
                self.sb.ns.utimens_loc(&cx, &l, &st, atime, mtime)
            }
        }
    }

    fn ftruncate(&self, fd: Fd, len: u64) -> SysResult<()> {
        self.enter();
        let ofd = self.ofd(fd)?;
        if len > i64::MAX as u64 {
            return Err(Errno::EINVAL);
        }
        match &ofd.obj {
            FileObj::Path { .. } => Err(Errno::EBADF),
            FileObj::Vfs { loc, kind: FileType::Regular, .. } if ofd.writable => {
                let cx = self.caller();
                if len > cx.fsize_limit {
                    generate_signal(&self.proc, Signal::SIGXFSZ);
                    self.enter();
                    return Err(Errno::EFBIG);
                }
                self.sb.ns.truncate_loc(&cx, loc, len, false)
            }
            _ => Err(Errno::EINVAL),
        }
    }

    fn fallocate(&self, fd: Fd, mode: FallocFlags, offset: i64, len: i64) -> SysResult<()> {
        self.enter();
        // ksys_fallocate: o fdget vem antes de tudo, e um fd O_PATH não conta.
        let ofd = self.ofd(fd)?;
        if matches!(ofd.obj, FileObj::Path { .. }) {
            return Err(Errno::EBADF);
        }
        // vfs_fallocate, na ordem do fs/open.c.
        mode.validate(offset, len)?;
        if !ofd.writable {
            return Err(Errno::EBADF);
        }
        let handle = match &ofd.obj {
            FileObj::Pipe { .. } => return Err(Errno::ESPIPE),
            FileObj::Vfs { kind: FileType::Fifo, .. } => return Err(Errno::ESPIPE),
            FileObj::Vfs { kind: FileType::Directory, .. } => return Err(Errno::EISDIR),
            FileObj::Vfs { handle, kind: FileType::Regular, .. } => handle,
            _ => return Err(Errno::ENODEV),
        };
        let end = offset.checked_add(len).ok_or(Errno::EFBIG)?;
        if end as u64 > vfs::MAX_FILE_SIZE {
            return Err(Errno::EFBIG);
        }
        let cx = self.caller();
        match handle.fallocate(&cx, mode, offset as u64, len as u64) {
            // inode_newsize_ok passou do RLIMIT_FSIZE: SIGXFSZ antes do EFBIG, como no ftruncate.
            Err(Errno::EFBIG) if end as u64 > cx.fsize_limit => {
                generate_signal(&self.proc, Signal::SIGXFSZ);
                self.enter();
                Err(Errno::EFBIG)
            }
            r => r,
        }
    }

    fn fsync(&self, fd: Fd) -> SysResult<()> {
        self.enter();
        let ofd = self.ofd(fd)?;
        match &ofd.obj {
            FileObj::Vfs { .. } => Ok(()),
            FileObj::Path { .. } => Err(Errno::EBADF),
            _ => Err(Errno::EINVAL),
        }
    }

    fn getdents(&self, fd: Fd) -> SysResult<Vec<DirEntry>> {
        self.enter();
        let ofd = self.ofd(fd)?;
        match &ofd.obj {
            FileObj::Vfs { handle, kind: FileType::Directory, .. } => {
                let cx = self.caller();
                let mut st = ofd.st.lock();
                let (batch, next) = handle.readdir(&cx, st.dir_cookie, 1024)?;
                st.dir_cookie = next;
                st.pos = next;
                Ok(batch)
            }
            FileObj::Path { .. } => Err(Errno::EBADF),
            _ => Err(Errno::ENOTDIR),
        }
    }

    fn statfs(&self, path: &[u8]) -> SysResult<StatFs> {
        self.enter();
        let cx = self.caller();
        self.sb.ns.statfs(&cx, &Start::Cwd, path)
    }

    fn fstatfs(&self, fd: Fd) -> SysResult<StatFs> {
        self.enter();
        let ofd = self.ofd(fd)?;
        match (&ofd.obj, ofd.loc()) {
            (FileObj::Pipe { fifo: None, .. }, _) => {
                Ok(StatFs { fs_type: PIPEFS_MAGIC, bsize: 4096, namelen: 255, frsize: 4096, flags: 0x20, ..StatFs::default() })
            }
            (_, Some(l)) => Ok(self.sb.ns.statfs_loc(l)),
            _ => Err(Errno::EBADF),
        }
    }

    // ---- descritores ----

    fn dup(&self, fd: Fd) -> SysResult<Fd> {
        self.enter();
        let limit = self.nofile();
        let mut fds = self.proc.fds.lock();
        let ofd = fds.ofd(fd)?;
        fds.install(ofd, false, 0, limit)
    }

    fn dup3(&self, old: Fd, new: Fd, cloexec: bool) -> SysResult<Fd> {
        self.enter();
        if old == new {
            return Err(Errno::EINVAL);
        }
        let limit = self.nofile();
        let prev = {
            let mut fds = self.proc.fds.lock();
            let ofd = fds.ofd(old)?;
            if new.0 < 0 || new.0 as u64 >= limit {
                return Err(Errno::EBADF);
            }
            fds.put(new, ofd, cloexec)
        };
        if let Some(slot) = &prev {
            self.flush_posix_locks(&slot.ofd);
        }
        drop(prev);
        Ok(new)
    }

    fn dup_min(&self, fd: Fd, min: Fd, cloexec: bool) -> SysResult<Fd> {
        self.enter();
        let limit = self.nofile();
        let mut fds = self.proc.fds.lock();
        let ofd = fds.ofd(fd)?;
        if min.0 < 0 || min.0 as u64 >= limit {
            return Err(Errno::EINVAL);
        }
        fds.install(ofd, cloexec, min.0, limit)
    }

    fn get_cloexec(&self, fd: Fd) -> SysResult<bool> {
        self.enter();
        Ok(self.proc.fds.lock().get(fd)?.cloexec)
    }

    fn set_cloexec(&self, fd: Fd, on: bool) -> SysResult<()> {
        self.enter();
        self.proc.fds.lock().get_mut(fd)?.cloexec = on;
        Ok(())
    }

    fn get_status_flags(&self, fd: Fd) -> SysResult<OFlags> {
        self.enter();
        let ofd = self.ofd(fd)?;
        Ok(OFlags::from_bits_retain(crate::procinfo::f_flags(&ofd)))
    }

    fn set_status_flags(&self, fd: Fd, flags: OFlags) -> SysResult<()> {
        self.enter();
        let ofd = self.ofd(fd)?;
        if matches!(ofd.obj, FileObj::Path { .. }) {
            return Err(Errno::EBADF);
        }
        let changeable = OFlags::APPEND | OFlags::NONBLOCK | OFlags::NOATIME;
        let mut st = ofd.st.lock();
        st.status = (st.status & !changeable) | (flags & changeable);
        Ok(())
    }

    fn pipe2(&self, flags: OFlags) -> SysResult<(Fd, Fd)> {
        self.enter();
        if !(OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::DSYNC).contains(flags) && !(flags & !(OFlags::CLOEXEC | OFlags::NONBLOCK)).is_empty() {
            return Err(Errno::EINVAL);
        }
        let cred = self.proc.st.lock().cred.clone();
        let pipe = Pipe::new(self.sb.kernel.pipe_ino(), cred.uid, cred.gid, self.sb.now());
        let locks = Arc::downgrade(&self.sb.locks);
        let nb = flags & OFlags::NONBLOCK;
        let r = Ofd::new(FileObj::Pipe { end: pipe.attach(true, false), fifo: None }, OFlags::RDONLY | nb, locks.clone());
        let w = Ofd::new(FileObj::Pipe { end: pipe.attach(false, true), fifo: None }, OFlags::WRONLY | nb, locks);
        let cloexec = flags.contains(OFlags::CLOEXEC);
        let limit = self.nofile();
        let mut fds = self.proc.fds.lock();
        let rfd = fds.install(r, cloexec, 0, limit)?;
        match fds.install(w, cloexec, 0, limit) {
            Ok(wfd) => Ok((rfd, wfd)),
            Err(e) => {
                let s = fds.remove(rfd);
                drop(fds);
                drop(s);
                Err(e)
            }
        }
    }

    fn isatty(&self, fd: Fd) -> bool {
        self.tty_of(fd).is_ok()
    }

    fn tcp_listen(&self, port: u16, backlog: u32, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        self.tcp_listen_at(std::net::Ipv4Addr::UNSPECIFIED.into(), port, backlog, nonblock, cloexec)
    }

    fn tcp_listen_at(&self, ip: std::net::IpAddr, port: u16, backlog: u32, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        self.enter();
        let listener = self.sb.ports.listen(ip, port, backlog, self.sock_pipe())?;
        let port = listener.port();
        let fd = self.install_sock(FileObj::Listener(listener), nonblock, cloexec)?;
        Ok((fd, port))
    }

    fn tcp_bind(&self, ip: std::net::IpAddr, port: u16, reuse_addr: bool, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        self.enter();
        let sock = self.sb.ports.bind(ip, port, reuse_addr, self.sock_pipe())?;
        let port = sock.port();
        let fd = self.install_sock(FileObj::Listener(sock), nonblock, cloexec)?;
        Ok((fd, port))
    }

    fn tcp_connect_bound(&self, fd: Fd, ip: std::net::IpAddr, port: u16) -> SysResult<u16> {
        self.enter();
        let ofd = self.ofd(fd)?;
        // `connect` num socket que já escuta ou já conectou dá EISCONN (`inet_stream_connect`). O
        // socket é o mesmo objeto para todos os fds duplicados: a conexão fica nele.
        match &ofd.obj {
            FileObj::Listener(l) => {
                let local = self.connect_wait(l, ip, port)?.local;
                ofd.sock.lock().sndbuf_expanded = true;
                Ok(local)
            }
            FileObj::Stream(_) => Err(Errno::EISCONN),
            _ => Err(Errno::ENOTSOCK),
        }
    }

    fn tcp_listen_bound(&self, fd: Fd, backlog: u32) -> SysResult<()> {
        self.enter();
        let ofd = self.ofd(fd)?;
        match &ofd.obj {
            // `listen` num socket já conectado: EINVAL, como no `inet_listen`.
            FileObj::Listener(l) if l.conn().is_some() || l.syn_sent() => Err(Errno::EINVAL),
            FileObj::Listener(l) => {
                l.start_listening(backlog);
                Ok(())
            }
            FileObj::Stream(_) => Err(Errno::EINVAL),
            _ => Err(Errno::ENOTSOCK),
        }
    }

    fn tcp_accept(&self, fd: Fd, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let FileObj::Listener(l) = &ofd.obj else {
            return Err(if matches!(ofd.obj, FileObj::Stream(_)) { Errno::EINVAL } else { Errno::ENOTSOCK });
        };
        let wait_nb = ofd.nonblock();
        let r = self.wait_event(None, |p| l.try_accept(wait_nb, p));
        let conn = match r {
            Ok(c) => c,
            Err(e) => {
                l.unregister(&self.parker);
                return Err(e);
            }
        };
        let peer = conn.peer;
        let fd = self.install_sock(FileObj::Stream(conn), nonblock, cloexec)?;
        // O filho herda o `sk_sndbuf` e as travas do socket que escuta (`sk_clone`): com `SO_SNDBUF` definido ali, o
        // valor passa ao aceito e nada o amplia; sem ele, o `tcp_init_buffer_space` do estabelecimento o amplia.
        let inherited = ofd.sock.lock().opts.get(&(SOL_SOCKET, SO_SNDBUF)).cloned();
        let accepted = self.ofd(fd)?;
        let mut meta = accepted.sock.lock();
        match inherited {
            Some(v) => drop(meta.opts.insert((SOL_SOCKET, SO_SNDBUF), v)),
            None => meta.sndbuf_expanded = true,
        }
        drop(meta);
        Ok((fd, peer))
    }

    fn tcp_connect(&self, port: u16, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        self.tcp_connect_at(std::net::Ipv4Addr::LOCALHOST.into(), port, nonblock, cloexec)
    }

    fn tcp_connect_at(&self, ip: std::net::IpAddr, port: u16, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        self.enter();
        let conn = self.connect_new(ip, port)?;
        let local = conn.local;
        let fd = self.install_sock(FileObj::Stream(conn), nonblock, cloexec)?;
        self.ofd(fd)?.sock.lock().sndbuf_expanded = true;
        Ok((fd, local))
    }

    fn tcp_shutdown(&self, fd: Fd, read: bool, write: bool) -> SysResult<()> {
        self.enter();
        let ofd = self.ofd(fd)?;
        match &ofd.obj {
            FileObj::Stream(c) => {
                c.shutdown(read, write);
                Ok(())
            }
            FileObj::Listener(l) => match l.conn() {
                Some(c) => {
                    c.shutdown(read, write);
                    Ok(())
                }
                // `inet_shutdown` em LISTEN: só o `SHUT_RD` desfaz a escuta (`tcp_disconnect`).
                None if l.is_listening() => {
                    if read {
                        l.stop_listening();
                    }
                    Ok(())
                }
                None => Err(Errno::ENOTCONN),
            },
            FileObj::Unix(u) => u.shutdown(read, write),
            _ => Err(Errno::ENOTSOCK),
        }
    }

    fn tcp_ports(&self, fd: Fd) -> SysResult<(u16, Option<u16>)> {
        self.enter();
        let ofd = self.ofd(fd)?;
        match &ofd.obj {
            FileObj::Stream(c) => Ok((c.local, Some(c.peer))),
            FileObj::Listener(l) => Ok(match l.conn() {
                Some(c) => (c.local, Some(c.peer)),
                None => (l.name().1, None),
            }),
            _ => Err(Errno::ENOTSOCK),
        }
    }

    fn tcp_socket(&self, v6: bool, nonblock: bool, cloexec: bool) -> SysResult<Fd> {
        self.enter();
        let sock = self.sb.ports.socket(v6, self.sock_pipe());
        self.install_sock(FileObj::Listener(sock), nonblock, cloexec)
    }

    fn tcp_bind_fd(&self, fd: Fd, ip: std::net::IpAddr, port: u16, reuse_addr: bool) -> SysResult<u16> {
        self.enter();
        let ofd = self.ofd(fd)?;
        match &ofd.obj {
            FileObj::Listener(l) => self.sb.ports.bind_socket(l, ip, port, reuse_addr),
            FileObj::Stream(_) => Err(Errno::EINVAL),
            _ => Err(Errno::ENOTSOCK),
        }
    }

    fn tcp_connect_fd(&self, fd: Fd, ip: std::net::IpAddr, port: u16) -> SysResult<()> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let l = match &ofd.obj {
            FileObj::Listener(l) => l,
            FileObj::Stream(_) => return Err(Errno::EISCONN),
            _ => return Err(Errno::ENOTSOCK),
        };
        let nonblock = ofd.nonblock();
        Self::settle_syn(&ofd);
        let mut meta = ofd.sock.lock();
        if meta.connecting {
            // `__inet_stream_connect` em SS_CONNECTING com o SYN ainda sem resposta (a fila do ouvinte estava
            // cheia): EALREADY no não bloqueante, e o bloqueante volta a esperar.
            if l.syn_sent() {
                if nonblock {
                    return Err(Errno::EALREADY);
                }
                drop(meta);
                return self.finish_connect(&ofd, l);
            }
            // O SYN recusado entrega o erro pendente (ou ECONNABORTED, se o `SO_ERROR` já o leu) e o socket
            // volta a não estar conectado; senão a conexão que o loopback já fechou conclui com 0.
            meta.connecting = false;
            if meta.failed {
                meta.failed = false;
                let err = std::mem::take(&mut meta.error);
                return Err(if err == 0 { Errno::ECONNABORTED } else { Errno(err) });
            }
            meta.sndbuf_expanded = true;
            return Ok(());
        }
        match l.connect(&self.sb.ports, ip, port, self.sock_pipe_maker()) {
            // O handshake do loopback termina depois do retorno de um `connect` não bloqueante.
            Ok(Some(_)) if nonblock => {
                meta.sndbuf_expanded = true;
                meta.connecting = true;
                Err(Errno::EINPROGRESS)
            }
            Ok(Some(_)) => {
                meta.sndbuf_expanded = true;
                Ok(())
            }
            // Fila de aceite cheia: o SYN foi descartado e o cliente retransmite (1, 3, 7... s).
            Ok(None) => {
                meta.connecting = true;
                if nonblock {
                    return Err(Errno::EINPROGRESS);
                }
                drop(meta);
                self.finish_connect(&ofd, l)
            }
            // O loopback recusa só depois: o erro aparece em `SO_ERROR`, no poll e no `connect` seguinte.
            Err(Errno::ECONNREFUSED) if nonblock => {
                meta.error = Errno::ECONNREFUSED.0;
                meta.failed = true;
                meta.connecting = true;
                Err(Errno::EINPROGRESS)
            }
            Err(e) => Err(e),
        }
    }

    #[allow(clippy::type_complexity)]
    fn tcp_names(&self, fd: Fd) -> SysResult<((std::net::IpAddr, u16), Option<(std::net::IpAddr, u16)>)> {
        self.enter();
        let ofd = self.ofd(fd)?;
        match &ofd.obj {
            FileObj::Stream(c) => c.addrs().map(|(me, peer)| (me, Some(peer))).ok_or(Errno::ENOTSOCK),
            FileObj::Listener(l) => Ok(match l.conn().and_then(|c| c.addrs()) {
                Some((me, peer)) => (me, Some(peer)),
                None => (l.name(), None),
            }),
            _ => Err(Errno::ENOTSOCK),
        }
    }

    fn sock_info(&self, fd: Fd) -> SysResult<(i32, i32, i32, bool)> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let family = |v6: bool| if v6 { 10 } else { 2 };
        Ok(match &ofd.obj {
            FileObj::Listener(l) => (family(l.is_v6()), 1, 6, l.is_listening()),
            FileObj::Stream(c) => (family(c.addrs().is_some_and(|(me, _)| me.0.is_ipv6())), 1, 6, false),
            FileObj::Unix(u) => (1, i32::from(u.ty), 0, u.listening()),
            FileObj::Udp(u) => (family(u.v6), 2, 17, false),
            _ => return Err(Errno::ENOTSOCK),
        })
    }

    fn sock_error(&self, fd: Fd) -> SysResult<i32> {
        self.enter();
        let ofd = self.ofd(fd)?;
        if !ofd.obj.is_socket() {
            return Err(Errno::ENOTSOCK);
        }
        // O `sk_err` de uma conexão que recebeu RST (`ECONNRESET`, ou `EPIPE` de um par que já fechara); sem
        // conexão, o do `connect` recusado.
        if let Some(e) = Self::with_conn(&ofd, crate::net::Conn::take_error).flatten() {
            return Ok(e.0);
        }
        Self::settle_syn(&ofd);
        let err = std::mem::take(&mut ofd.sock.lock().error);
        Ok(err)
    }

    fn sock_setopt(&self, fd: Fd, level: i32, name: i32, value: &[u8]) -> SysResult<()> {
        self.enter();
        let ofd = self.ofd(fd)?;
        if !ofd.obj.is_socket() {
            return Err(Errno::ENOTSOCK);
        }
        ofd.sock.lock().opts.insert((level, name), value.to_vec());
        if let FileObj::Unix(u) = &ofd.obj && (level, name) == (SOL_SOCKET, SO_PASSCRED) {
            // O `sk_setsockopt` guarda `SOCK_PASSCRED`; o Python manda o valor já normalizado (0 ou 1).
            u.set_passcred(value.iter().any(|&b| b != 0));
        }
        Ok(())
    }

    fn sock_getopt(&self, fd: Fd, level: i32, name: i32) -> SysResult<Option<Vec<u8>>> {
        self.enter();
        let ofd = self.ofd(fd)?;
        if !ofd.obj.is_socket() {
            return Err(Errno::ENOTSOCK);
        }
        if (level, name) == (SOL_SOCKET, SO_PEERCRED) {
            // `cred_to_ucred`: o par de um socket Unix; sem par (e em qualquer outro socket) pid 0 e uid/gid -1.
            let cred = match &ofd.obj {
                FileObj::Unix(u) => u.peer_cred(),
                _ => Ucred::UNSET,
            };
            return Ok(Some(cred.to_bytes()));
        }
        let meta = ofd.sock.lock();
        let value = meta.opts.get(&(level, name)).cloned().or_else(|| {
            ((level, name) == (SOL_SOCKET, SO_SNDBUF) && meta.sndbuf_expanded).then(|| TCP_SNDBUF_EXPANDED.to_le_bytes().to_vec())
        });
        Ok(value)
    }

    fn sock_recv(&self, fd: Fd, max: usize, flags: MsgFlags) -> SysResult<Vec<u8>> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let mut buf = vec![0u8; max];
        let n = self.stream_recv(&ofd, &mut buf, flags)?;
        buf.truncate(n);
        Ok(buf)
    }

    fn sock_send(&self, fd: Fd, buf: &[u8], flags: MsgFlags) -> SysResult<usize> {
        self.enter();
        let ofd = self.ofd(fd)?;
        self.stream_send(&ofd, buf, flags)
    }

    fn udp_socket(&self, v6: bool, nonblock: bool, cloexec: bool) -> SysResult<Fd> {
        self.enter();
        let sock = self.sb.udp.socket(v6, self.sock_pipe());
        self.install_sock(FileObj::Udp(sock), nonblock, cloexec)
    }

    fn udp_bind(&self, fd: Fd, ip: std::net::IpAddr, port: u16, reuse: bool) -> SysResult<u16> {
        self.enter();
        let u = self.udp_of(fd)?;
        self.sb.udp.bind(&u, ip, port, reuse)
    }

    fn udp_connect(&self, fd: Fd, ip: std::net::IpAddr, port: u16) -> SysResult<()> {
        self.enter();
        let u = self.udp_of(fd)?;
        self.sb.udp.connect(&u, ip, port)
    }

    fn udp_sendto(&self, fd: Fd, data: &[u8], dst: Option<(std::net::IpAddr, u16)>) -> SysResult<usize> {
        self.enter();
        let u = self.udp_of(fd)?;
        self.sb.udp.send(&u, data, dst)
    }

    fn udp_recvfrom(&self, fd: Fd, max: usize, peek: bool) -> SysResult<(Vec<u8>, std::net::IpAddr, u16)> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let FileObj::Udp(u) = &ofd.obj else { return Err(Errno::ENOTSOCK) };
        let (mut data, (ip, port)) = self.udp_recv(&ofd, u, peek)?;
        data.truncate(max);
        Ok((data, ip, port))
    }

    fn udp_names(&self, fd: Fd) -> SysResult<((std::net::IpAddr, u16), Option<(std::net::IpAddr, u16)>)> {
        self.enter();
        Ok(self.udp_of(fd)?.names())
    }

    fn unix_socket(&self, ty: u8, nonblock: bool, cloexec: bool) -> SysResult<Fd> {
        self.enter();
        if ![crate::unix::SOCK_STREAM, crate::unix::SOCK_DGRAM, crate::unix::SOCK_SEQPACKET].contains(&ty) {
            return Err(Errno::ESOCKTNOSUPPORT);
        }
        let sock = self.sb.unix.socket(ty, self.sock_pipe());
        self.install_sock(FileObj::Unix(sock), nonblock, cloexec)
    }

    fn unix_socketpair(&self, ty: u8, nonblock: bool, cloexec: bool) -> SysResult<(Fd, Fd)> {
        self.enter();
        if ![crate::unix::SOCK_STREAM, crate::unix::SOCK_DGRAM, crate::unix::SOCK_SEQPACKET].contains(&ty) {
            return Err(Errno::ESOCKTNOSUPPORT);
        }
        let (a, b) = self.sb.unix.pair(ty, (self.sock_pipe(), self.sock_pipe()), || self.sock_pipe(), self.peer_ucred());
        let fa = self.install_sock(FileObj::Unix(a), nonblock, cloexec)?;
        match self.install_sock(FileObj::Unix(b), nonblock, cloexec) {
            Ok(fb) => Ok((fa, fb)),
            Err(e) => {
                let _ = self.close(fa);
                Err(e)
            }
        }
    }

    fn unix_bind(&self, fd: Fd, name: &[u8]) -> SysResult<()> {
        self.enter();
        let u = self.unix_of(fd)?;
        if name.is_empty() || name.len() > 107 {
            return Err(Errno::EINVAL);
        }
        if name[0] == 0 {
            return self.sb.unix.bind(&u, name.to_vec(), crate::unix::Key::Abstract(name.to_vec()));
        }
        if u.names().0.is_some() {
            return Err(Errno::EINVAL);
        }
        // `unix_bind_bsd`: o arquivo nasce com 0777 menos a umask; já existir é EADDRINUSE.
        match self.mknodat(Fd::CWD, name, sysabi::mode::S_IFSOCK | 0o777, 0) {
            Err(Errno::EEXIST) => return Err(Errno::EADDRINUSE),
            r => r?,
        }
        let st = self.fstatat(Fd::CWD, name, AtFlags::SYMLINK_NOFOLLOW)?;
        let r = self.sb.unix.bind(&u, name.to_vec(), crate::unix::Key::Node(st.dev, st.ino));
        if r.is_err() {
            let _ = self.unlinkat(Fd::CWD, name, AtFlags::empty());
        }
        r
    }

    fn unix_listen(&self, fd: Fd, backlog: u32) -> SysResult<()> {
        self.enter();
        self.unix_of(fd)?.listen(backlog, self.peer_ucred())
    }

    fn unix_accept(&self, fd: Fd, nonblock: bool, cloexec: bool) -> SysResult<Fd> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let FileObj::Unix(u) = &ofd.obj else { return Err(Errno::ENOTSOCK) };
        if u.ty == crate::unix::SOCK_DGRAM {
            return Err(Errno::EOPNOTSUPP);
        }
        let wait_nb = ofd.nonblock();
        let r = self.wait_event(None, |p| u.try_accept(wait_nb, p));
        let sock = match r {
            Ok(s) => s,
            Err(e) => {
                u.unregister(&self.parker);
                return Err(e);
            }
        };
        self.install_sock(FileObj::Unix(sock), nonblock, cloexec)
    }

    fn unix_connect(&self, fd: Fd, name: &[u8]) -> SysResult<()> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let FileObj::Unix(u) = &ofd.obj else { return Err(Errno::ENOTSOCK) };
        let target = self.unix_find(name)?;
        if u.ty == crate::unix::SOCK_DGRAM {
            return u.dgram_connect(&target);
        }
        let nonblock = ofd.nonblock();
        let mk = || self.sock_pipe();
        let cred = self.peer_ucred();
        // Quem espera vaga na fila não segura o socket que escuta: se o último fd dele fecha, o `Drop` acorda a
        // espera e o `connect` seguinte já não o encontra (ECONNREFUSED, como o `unix_dgram_peer_wake` do Linux).
        let weak = Arc::downgrade(&target);
        drop(target);
        let r = self.wait_event(None, |p| match weak.upgrade() {
            Some(t) => u.try_connect(&t, nonblock, p, &mk, cred),
            None => Try::Ready(Err(Errno::ECONNREFUSED)),
        });
        if r.is_err() && let Some(t) = weak.upgrade() {
            t.unregister(&self.parker);
        }
        r
    }

    fn unix_names(&self, fd: Fd) -> SysResult<(Option<Vec<u8>>, Option<Vec<u8>>, bool)> {
        self.enter();
        Ok(self.unix_of(fd)?.names())
    }

    fn unix_sendto(&self, fd: Fd, data: &[u8], name: Option<&[u8]>) -> SysResult<usize> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let FileObj::Unix(u) = &ofd.obj else { return Err(Errno::ENOTSOCK) };
        self.unix_send_msg(&ofd, u, data, name, &self.plain_scm(), MsgFlags::empty())
    }

    fn unix_recvfrom(&self, fd: Fd, max: usize, peek: bool) -> SysResult<(Vec<u8>, Option<Vec<u8>>)> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let FileObj::Unix(u) = &ofd.obj else { return Err(Errno::ENOTSOCK) };
        let flags = if peek { MsgFlags::PEEK } else { MsgFlags::empty() };
        let (data, from, _, _) = self.unix_recv_any(&ofd, u, max, flags)?;
        Ok((data, from))
    }

    fn unix_sendmsg(&self, fd: Fd, data: &[u8], name: Option<&[u8]>, control: &[u8], flags: MsgFlags) -> SysResult<usize> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let FileObj::Unix(u) = &ofd.obj else { return Err(Errno::ENOTSOCK) };
        let scm = self.scm_from_control(control)?;
        self.unix_send_msg(&ofd, u, data, name, &scm, flags)
    }

    fn unix_recvmsg(&self, fd: Fd, max: usize, control_len: usize, flags: MsgFlags) -> SysResult<RecvMsg> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let FileObj::Unix(u) = &ofd.obj else { return Err(Errno::ENOTSOCK) };
        let (data, name, scm, cut) = self.unix_recv_any(&ofd, u, max, flags)?;
        let mut control = sysabi::cmsg::Builder::new(control_len);
        if control_len == 0 {
            // Sem `msg_control` (`__scm_recv_common`): o que seria entregue só marca o corte, e os descritores fecham.
            control.truncated = u.passcred() || !scm.fds.is_empty();
        } else {
            if u.passcred() {
                control.put(sysabi::cmsg::SOL_SOCKET, sysabi::cmsg::SCM_CREDENTIALS, &scm.cred.to_bytes());
            }
            self.detach_fds(&scm.fds, &mut control, flags.contains(MsgFlags::CMSG_CLOEXEC));
        }
        let mut out = if cut { MsgFlags::TRUNC } else { MsgFlags::empty() };
        if control.truncated {
            out |= MsgFlags::CTRUNC;
        }
        Ok(RecvMsg { data, name, control: control.finish(), flags: out })
    }

    // Terminais: só os pseudoterminais existem (não há console nem tty virtual). EBADF primeiro, depois
    // ENOTTY, na ordem do `tty_ioctl`. As ioctls de modo no mestre agem no escravo (`tty_pair_get_tty`).

    fn tcgetwinsize(&self, fd: Fd) -> SysResult<Winsize> {
        self.enter();
        Ok(self.tty_of(fd)?.0.winsize())
    }

    fn tcgetattr(&self, fd: Fd) -> SysResult<Termios> {
        self.enter();
        Ok(self.tty_of(fd)?.0.termios())
    }

    fn tcsetattr(&self, fd: Fd, when: SetAttrWhen, t: &Termios) -> SysResult<()> {
        self.enter();
        let (pty, _) = self.tty_of(fd)?;
        // TCSADRAIN e TCSAFLUSH esperam a saída sair; a do pty vai pro buffer do mestre na hora.
        pty.set_termios(t, when);
        Ok(())
    }

    fn tcsetwinsize(&self, fd: Fd, ws: Winsize) -> SysResult<()> {
        self.enter();
        let (pty, _) = self.tty_of(fd)?;
        if let Some(pgrp) = pty.set_winsize(ws) {
            tty::sigwinch(&self.sb, pgrp);
            self.enter();
        }
        Ok(())
    }

    fn tcflush(&self, fd: Fd, queue: i32) -> SysResult<()> {
        self.enter();
        let (pty, master) = self.tty_of(fd)?;
        pty.flush(master, queue)
    }

    fn tcflow(&self, fd: Fd, action: i32) -> SysResult<()> {
        self.enter();
        let (pty, master) = self.tty_of(fd)?;
        let sigs = pty.flow(master, action)?;
        for (pgrp, sig) in sigs {
            tty::signal_pgrp(&self.sb, pgrp, sig);
        }
        // Quem escreveu o STOP pode estar no grupo sinalizado.
        self.enter();
        Ok(())
    }

    fn pty_number(&self, fd: Fd) -> SysResult<u32> {
        self.enter();
        match self.tty_of(fd)? {
            (pty, true) => Ok(pty.index),
            _ => Err(Errno::ENOTTY),
        }
    }

    fn pty_set_lock(&self, fd: Fd, locked: bool) -> SysResult<()> {
        self.enter();
        match self.tty_of(fd)? {
            (pty, true) => {
                pty.set_locked(locked);
                Ok(())
            }
            _ => Err(Errno::ENOTTY),
        }
    }

    fn tcgetpgrp(&self, fd: Fd) -> SysResult<Pid> {
        self.enter();
        let (pty, _) = self.job_tty(fd)?;
        Ok(pty.pgrp().unwrap_or(0))
    }

    fn tcsetpgrp(&self, fd: Fd, pgrp: Pid) -> SysResult<()> {
        self.enter();
        let (pty, _) = self.tty_of(fd)?;
        let (_, sid, _) = self.ids()?;
        // `tiocspgrp`: só pelo próprio terminal de controle.
        if pty.session() != Some(sid) {
            return Err(Errno::ENOTTY);
        }
        if pgrp < 0 {
            return Err(Errno::EINVAL);
        }
        {
            let t = self.sb.table.lock();
            let members: Vec<Pid> = t.map.values().filter(|e| e.rel.pgid == pgrp && e.rel.zombie.is_none()).map(|e| e.rel.sid).collect();
            if members.is_empty() {
                return Err(Errno::ESRCH);
            }
            if !members.contains(&sid) {
                return Err(Errno::EPERM);
            }
        }
        pty.set_pgrp(pgrp);
        Ok(())
    }

    fn tcgetsid(&self, fd: Fd) -> SysResult<Pid> {
        self.enter();
        let (pty, _) = self.job_tty(fd)?;
        pty.session().ok_or(Errno::ENOTTY)
    }

    fn tiocsctty(&self, fd: Fd, force: bool) -> SysResult<()> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let (pty, _) = self.tty_of(fd)?;
        let (me, sid, pgid) = self.ids()?;
        let root = self.proc.st.lock().cred.is_root();
        // `tiocsctty`, na ordem do kernel.
        if sid == me && pty.session() == Some(sid) {
            return Ok(());
        }
        if sid != me || self.sb.pty_of_session(sid).is_some() {
            return Err(Errno::EPERM);
        }
        if pty.session().is_some() {
            if force && root {
                pty.clear_ctty();
            } else {
                return Err(Errno::EPERM);
            }
        }
        if !ofd.readable && !root {
            return Err(Errno::EPERM);
        }
        pty.set_ctty(sid, pgid);
        Ok(())
    }

    fn tiocnotty(&self, fd: Fd) -> SysResult<()> {
        self.enter();
        let (pty, _) = self.tty_of(fd)?;
        let (me, sid, _) = self.ids()?;
        if pty.session() != Some(sid) {
            return Err(Errno::ENOTTY);
        }
        // O líder solta o terminal da sessão inteira (`disassociate_ctty(0)`): SIGHUP e SIGCONT pro
        // grupo em primeiro plano. Fora do líder, o terminal é da sessão e continua (ver o módulo tty).
        if sid == me
            && let Some(pgrp) = pty.clear_ctty()
        {
            tty::hangup_pgrp(&self.sb, pgrp);
            self.enter();
        }
        Ok(())
    }

    fn open_fds(&self) -> Vec<Fd> {
        self.proc.fds.lock().fds()
    }

    // ---- diretório corrente e máscara ----

    fn chdir(&self, path: &[u8]) -> SysResult<()> {
        self.enter();
        let cx = self.caller();
        let l = self.sb.ns.chdir_target(&cx, &Start::Cwd, path)?;
        let old = std::mem::replace(&mut self.proc.st.lock().cwd, PinnedLoc::new(l));
        drop(old);
        Ok(())
    }

    fn fchdir(&self, fd: Fd) -> SysResult<()> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let l = match &ofd.obj {
            FileObj::Vfs { loc, .. } | FileObj::Path { loc, .. } => loc.clone(),
            _ => return Err(Errno::ENOTDIR),
        };
        let cx = self.caller();
        self.sb.ns.fchdir_check(&cx, &l)?;
        let old = std::mem::replace(&mut self.proc.st.lock().cwd, PinnedLoc::new(l));
        drop(old);
        Ok(())
    }

    fn getcwd(&self) -> SysResult<Vec<u8>> {
        self.enter();
        let cx = self.caller();
        self.sb.ns.getcwd(&cx)
    }

    fn umask(&self, mask: Mode) -> Mode {
        self.enter();
        let mut st = self.proc.st.lock();
        std::mem::replace(&mut st.umask, mask & 0o777)
    }

    // ---- processos ----

    fn getpid(&self) -> Pid {
        self.enter();
        self.proc.pid
    }

    fn getppid(&self) -> Pid {
        self.enter();
        self.sb.table.lock().rel(self.proc.pid).map(|r| r.ppid).unwrap_or(0)
    }

    fn getpgid(&self, pid: Pid) -> SysResult<Pid> {
        self.enter();
        let p = if pid == 0 { self.proc.pid } else { pid };
        if p < 0 {
            return Err(Errno::ESRCH);
        }
        self.sb.table.lock().rel(p).map(|r| r.pgid).ok_or(Errno::ESRCH)
    }

    fn setpgid(&self, pid: Pid, pgid: Pid) -> SysResult<()> {
        self.enter();
        if pgid < 0 {
            return Err(Errno::EINVAL);
        }
        let me = self.proc.pid;
        let target = if pid == 0 { me } else { pid };
        let pgid = if pgid == 0 { target } else { pgid };
        let mut t = self.sb.table.lock();
        let my_sid = t.rel(me).map(|r| r.sid).ok_or(Errno::ESRCH)?;
        let r = t.rel(target).ok_or(Errno::ESRCH)?;
        if target != me && r.ppid != me {
            return Err(Errno::ESRCH);
        }
        if target != me && t.proc(target).is_some_and(|p| p.st.lock().exe.is_some() && p.st.lock().argv != self.proc.st.lock().argv) {
            // Filho que já fez execve: EACCES. Aproximação: o filho de spawn_fn que trocou de programa.
        }
        if r.sid == target {
            return Err(Errno::EPERM);
        }
        if r.sid != my_sid {
            return Err(Errno::EPERM);
        }
        if pgid != target && !t.map.values().any(|e| e.rel.pgid == pgid && e.rel.sid == my_sid) {
            return Err(Errno::EPERM);
        }
        t.rel_mut(target).expect("existe").pgid = pgid;
        Ok(())
    }

    fn getsid(&self, pid: Pid) -> SysResult<Pid> {
        self.enter();
        let p = if pid == 0 { self.proc.pid } else { pid };
        self.sb.table.lock().rel(p).map(|r| r.sid).ok_or(Errno::ESRCH)
    }

    fn setsid(&self) -> SysResult<Pid> {
        self.enter();
        let me = self.proc.pid;
        let mut t = self.sb.table.lock();
        if t.map.values().any(|e| e.rel.pgid == me) {
            return Err(Errno::EPERM);
        }
        let r = t.rel_mut(me).ok_or(Errno::ESRCH)?;
        r.sid = me;
        r.pgid = me;
        Ok(me)
    }

    fn spawn(&self, spec: SpawnSpec) -> SysResult<Pid> {
        self.enter();
        let mut child = spawn::fork_state(self);
        spawn::apply_attrs(self, &mut child, &spec.attrs)?;
        let cx = child.caller(&self.sb, self.proc.pid);
        let img = exec::load(&self.sb, &cx, &spec.path, spec.argv.clone(), child.env.clone())?;
        let task = spawn::insert_child(&self.sb, child)?;
        let pid = task.proc.pid;
        spawn::inherit_tune(self, &task, spec.attrs.scheduler.as_ref())?;
        spawn::commit_exec(&task, &img, false);
        spawn::start_process(&self.sb, task, Body::Image(img))?;
        Ok(pid)
    }

    fn spawn_fn(&self, attrs: ProcAttrs, name: Vec<u8>, body: ProcessFn) -> SysResult<Pid> {
        self.enter();
        let mut child = spawn::fork_state(self);
        spawn::apply_attrs(self, &mut child, &attrs)?;
        if !name.is_empty() {
            child.comm = exec::comm_of(&name);
        }
        let task = spawn::insert_child(&self.sb, child)?;
        let pid = task.proc.pid;
        spawn::inherit_tune(self, &task, attrs.scheduler.as_ref())?;
        spawn::start_process(&self.sb, task, Body::Func(body))?;
        Ok(pid)
    }

    fn execve(&self, path: &[u8], argv: &[Vec<u8>], env: Option<&[Vec<u8>]>) -> Errno {
        self.enter();
        let cx = self.caller();
        let env = match env {
            Some(e) => e.to_vec(),
            None => self.proc.st.lock().env.clone(),
        };
        match exec::load(&self.sb, &cx, path, argv.to_vec(), env.clone()) {
            Err(e) => e,
            Ok(img) => {
                self.proc.st.lock().pending_exec = Some(img);
                std::panic::resume_unwind(Box::new(ExecUnwind { path: path.to_vec(), argv: argv.to_vec(), env: Some(env) }))
            }
        }
    }

    fn wait4_info(&self, target: WaitTarget, options: WaitOptions) -> SysResult<Option<WaitInfo>> {
        self.enter();
        // O `wait4` sempre espera o término e nunca deixa o evento pra trás.
        let options = (options - WaitOptions::NOWAIT) | WaitOptions::EXITED;
        self.wait_event(None, |_| self.wait_children(target, options))
    }

    fn waitid(&self, target: WaitIdTarget, options: WaitOptions) -> SysResult<Option<WaitInfo>> {
        self.enter();
        let mut options = options;
        let target = match target {
            WaitIdTarget::All => WaitTarget::Any,
            WaitIdTarget::Pid(p) if p > 0 => WaitTarget::Pid(p),
            WaitIdTarget::Group(g) if g >= 0 => WaitTarget::Group(g),
            WaitIdTarget::Pidfd(fd) if fd.0 >= 0 => {
                let ofd = self.ofd(fd)?;
                let FileObj::Pidfd(p) = &ofd.obj else { return Err(Errno::EBADF) };
                // O pidfd não bloqueante vale como `WNOHANG`.
                if ofd.status().contains(OFlags::NONBLOCK) {
                    options |= WaitOptions::NOHANG;
                }
                WaitTarget::Pid(p.pid())
            }
            _ => return Err(Errno::EINVAL),
        };
        if !options.intersects(WaitOptions::EXITED | WaitOptions::UNTRACED | WaitOptions::CONTINUED) {
            return Err(Errno::EINVAL);
        }
        self.wait_event(None, |_| self.wait_children(target, options))
    }

    fn kill(&self, target: KillTarget, sig: Signal) -> SysResult<()> {
        self.enter();
        if !(0..=sysabi::linux::SIGRTMAX).contains(&sig.0) {
            return Err(Errno::EINVAL);
        }
        let procs = self.find_procs(target)?;
        if procs.is_empty() {
            return Err(Errno::ESRCH);
        }
        let cred = self.proc.st.lock().cred.clone();
        let mut sent = 0;
        let mut denied = 0;
        for p in &procs {
            let target = p.st.lock().cred.clone();
            if !may_signal(&cred, &target) {
                denied += 1;
                continue;
            }
            sent += 1;
            if sig.0 != 0 {
                generate_signal(p, sig);
            }
        }
        if sent == 0 && denied > 0 {
            return Err(Errno::EPERM);
        }
        // Sinal pra si mesmo é entregue na volta da syscall.
        self.enter();
        Ok(())
    }

    fn sigaction(&self, sig: Signal, disposition: SigDisposition) -> SysResult<SigDisposition> {
        self.enter();
        if !sig.is_valid() {
            return Err(Errno::EINVAL);
        }
        if sig.is_uncatchable() && disposition != SigDisposition::Default {
            return Err(Errno::EINVAL);
        }
        Ok(self.proc.sig.lock().set_disposition(sig, disposition))
    }

    fn take_caught_signals(&self) -> Vec<Signal> {
        self.enter();
        self.proc.sig.lock().take_caught()
    }

    fn checkpoint(&self) {
        self.enter();
    }

    fn sched_yield(&self) {
        self.enter();
        self.sb.kernel.cpus.yield_now(&self.ct);
    }

    fn getpriority(&self, pid: Pid) -> SysResult<i32> {
        self.enter();
        let p = self.proc_attr_caller_check(pid)?;
        let nice = p.st.lock().nice;
        Ok(nice)
    }

    fn setpriority(&self, pid: Pid, nice: i32) -> SysResult<()> {
        self.enter();
        let p = self.proc_attr_caller_check(pid)?;
        let nice = nice.clamp(-20, 19);
        let cred = self.proc.st.lock().cred.clone();
        let nice_rlim = self.proc.st.lock().rlimits[Resource::Nice as usize].cur;
        let mut st = p.st.lock();
        // `set_one_prio` do Linux. O oráculo é um contêiner docker com as capabilities padrão, em que
        // nem o root tem CAP_SYS_NICE: outro dono dá EPERM e baixar a nice só passa pelo RLIMIT_NICE.
        if st.cred.uid != cred.uid {
            return Err(Errno::EPERM);
        }
        if nice < st.nice && (20 - nice) as u64 > nice_rlim {
            return Err(Errno::EACCES);
        }
        st.nice = nice;
        drop(st);
        let tasks: Vec<Arc<Task>> = p.threads.lock().live.values().cloned().collect();
        for t in tasks {
            self.sb.kernel.cpus.set_nice(&t.ct, nice);
        }
        Ok(())
    }

    fn getrlimit(&self, res: Resource) -> SysResult<Rlimit> {
        self.enter();
        Ok(self.proc.st.lock().rlimits[res as usize])
    }

    fn setrlimit(&self, res: Resource, lim: Rlimit) -> SysResult<()> {
        self.enter();
        if lim.cur > lim.max {
            return Err(Errno::EINVAL);
        }
        let mut st = self.proc.st.lock();
        let old = st.rlimits[res as usize];
        if lim.max > old.max && !st.cred.is_root() {
            return Err(Errno::EPERM);
        }
        if res == Resource::Nofile && lim.max > 1_073_741_816 {
            return Err(Errno::EPERM);
        }
        st.rlimits[res as usize] = lim;
        Ok(())
    }

    fn getrusage(&self, who: RusageWho) -> SysResult<Rusage> {
        self.enter();
        match who {
            RusageWho::SelfProcess => {
                let ns = self.proc.cpu_ns();
                Ok(Rusage { utime: Duration::from_nanos(ns), stime: Duration::ZERO, maxrss_kib: 0 })
            }
            RusageWho::Children => Ok(self.sb.table.lock().rel(self.proc.pid).map(|r| r.children_rusage).unwrap_or_default()),
        }
    }

    fn list_processes(&self) -> Vec<ProcInfo> {
        self.enter();
        list_processes(&self.sb)
    }

    // ---- threads ----

    fn spawn_thread(&self, body: ThreadFn) -> SysResult<Tid> {
        self.enter();
        spawn::start_thread(self, body)
    }

    fn join_thread(&self, tid: Tid) -> SysResult<()> {
        self.enter();
        if tid == self.tid() {
            return Err(Errno::EDEADLK);
        }
        self.wait_event(None, |p| {
            let mut th = self.proc.threads.lock();
            match th.done.get(&tid).copied() {
                Some(false) => {
                    th.done.insert(tid, true);
                    Try::Ready(Ok(()))
                }
                Some(true) => Try::Ready(Err(Errno::EINVAL)),
                None if th.live.contains_key(&tid) => {
                    th.joiners.register(p);
                    Try::Pending
                }
                None => Try::Ready(Err(Errno::ESRCH)),
            }
        })
    }

    fn gettid(&self) -> Tid {
        self.enter();
        self.tid()
    }

    fn sched_getaffinity(&self) -> Vec<usize> {
        self.enter();
        let n = self.sb.kernel.cpus.ncpus();
        sched::effective_affinity(n, self.proc.tune.lock().cpus.as_deref())
    }

    fn sched_getaffinity_of(&self, pid: Pid) -> SysResult<Vec<usize>> {
        self.enter();
        let p = self.proc_attr_caller_check(pid)?;
        let n = self.sb.kernel.cpus.ncpus();
        let mask = sched::effective_affinity(n, p.tune.lock().cpus.as_deref());
        Ok(mask)
    }

    fn sched_setaffinity(&self, pid: Pid, cpus: &[usize]) -> SysResult<()> {
        self.enter();
        let p = self.proc_attr_caller_check(pid)?;
        // `check_same_owner`; o contêiner padrão não tem CAP_SYS_NICE pra passar por cima.
        let me = self.proc.st.lock().cred.uid;
        if p.st.lock().cred.uid != me {
            return Err(Errno::EPERM);
        }
        let mask = sched::normalize_affinity(self.sb.kernel.cpus.ncpus(), cpus)?;
        p.tune.lock().cpus = Some(mask);
        Ok(())
    }

    fn personality(&self, persona: u32) -> SysResult<u32> {
        self.enter();
        let mut tune = self.proc.tune.lock();
        let (old, new) = sched::personality::change(tune.personality, persona)?;
        tune.personality = new;
        Ok(old)
    }

    fn sched_getscheduler(&self, pid: Pid) -> SysResult<i32> {
        self.enter();
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        let p = self.proc_attr_caller_check(pid)?;
        let policy = p.tune.lock().sched.scheduler();
        Ok(policy)
    }

    fn sched_setscheduler(&self, pid: Pid, policy: i32, param: SchedParam) -> SysResult<()> {
        self.enter();
        if pid < 0 || policy < 0 {
            return Err(Errno::EINVAL);
        }
        self.sched_change(pid, |nice| Ok((sched::attr_for_setscheduler(policy, param, nice)?, false)))
    }

    fn sched_getparam(&self, pid: Pid) -> SysResult<SchedParam> {
        self.enter();
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        let p = self.proc_attr_caller_check(pid)?;
        let priority = p.tune.lock().sched.rt_priority as i32;
        Ok(SchedParam { priority })
    }

    fn sched_setparam(&self, pid: Pid, param: SchedParam) -> SysResult<()> {
        self.enter();
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        self.sched_change(pid, |nice| Ok((sched::attr_for_setparam(param, nice), true)))
    }

    fn sched_rr_get_interval(&self, pid: Pid) -> SysResult<Duration> {
        self.enter();
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        let p = self.proc_attr_caller_check(pid)?;
        let d = p.tune.lock().sched.rr_interval(self.sb.kernel.cpus.ncpus());
        Ok(d)
    }

    fn sched_getattr(&self, pid: Pid, size: u32, flags: u32) -> SysResult<SchedAttr> {
        self.enter();
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        sched::check_getattr(size, flags)?;
        let p = self.proc_attr_caller_check(pid)?;
        let nice = p.st.lock().nice;
        let a = p.tune.lock().sched.attr(nice, size, self.sb.kernel.cpus.ncpus());
        Ok(a)
    }

    fn sched_setattr(&self, pid: Pid, attr: &SchedAttr, flags: u32) -> SysResult<()> {
        self.enter();
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        let attr = sched::check_setattr(attr, flags)?;
        self.sched_change(pid, |_| Ok((attr, false)))
    }

    fn ioprio_get(&self, which: i32, who: i32) -> SysResult<i32> {
        self.enter();
        let targets = self.ioprio_targets(which, who)?;
        let mut best: Option<i32> = None;
        for p in targets {
            // Ordem de travas: `st` antes de `tune`.
            let nice = p.st.lock().nice;
            let t = p.tune.lock();
            // O oráculo devolve a classe NONE (prio 0) quando a prioridade de E/S nunca foi definida,
            // em vez da classe derivada da nice.
            let _ = nice;
            let v = t.ioprio;
            best = Some(best.map_or(v, |b| sched::ioprio_best(b, v)));
        }
        best.ok_or(Errno::ESRCH)
    }

    fn ioprio_set(&self, which: i32, who: i32, ioprio: i32) -> SysResult<()> {
        self.enter();
        sched::ioprio_check(ioprio)?;
        let targets = self.ioprio_targets(which, who)?;
        if targets.is_empty() {
            return Err(Errno::ESRCH);
        }
        let me = self.proc.st.lock().cred.uid;
        for p in targets {
            // `set_task_ioprio`: só o dono (sem CAP_SYS_NICE); um processo que já terminou é ESRCH.
            if p.st.lock().cred.uid != me {
                return Err(Errno::EPERM);
            }
            if p.nthreads() == 0 && p.pid != INIT_PID {
                return Err(Errno::ESRCH);
            }
            p.tune.lock().ioprio = ioprio;
        }
        Ok(())
    }

    // ---- identidade e ambiente ----

    fn getuid(&self) -> Uid {
        self.enter();
        self.proc.st.lock().cred.ruid
    }

    fn geteuid(&self) -> Uid {
        self.enter();
        self.proc.st.lock().cred.uid
    }

    fn getgid(&self) -> Gid {
        self.enter();
        self.proc.st.lock().cred.rgid
    }

    fn getegid(&self) -> Gid {
        self.enter();
        self.proc.st.lock().cred.gid
    }

    fn getgroups(&self) -> Vec<Gid> {
        self.enter();
        self.proc.st.lock().cred.groups.clone()
    }

    fn getresuid(&self) -> (Uid, Uid, Uid) {
        self.enter();
        let st = self.proc.st.lock();
        (st.cred.ruid, st.cred.uid, st.cred.suid)
    }

    fn getresgid(&self) -> (Gid, Gid, Gid) {
        self.enter();
        let st = self.proc.st.lock();
        (st.cred.rgid, st.cred.gid, st.cred.sgid)
    }

    fn setuid(&self, uid: Uid) -> SysResult<()> {
        self.enter();
        self.change_cred(|c| {
            if c.is_root() {
                (c.ruid, c.uid, c.suid) = (uid, uid, uid);
            } else if uid == c.ruid || uid == c.suid {
                c.uid = uid;
            } else {
                return Err(Errno::EPERM);
            }
            Ok(())
        })
    }

    fn setgid(&self, gid: Gid) -> SysResult<()> {
        self.enter();
        self.change_cred(|c| {
            if c.is_root() {
                (c.rgid, c.gid, c.sgid) = (gid, gid, gid);
            } else if gid == c.rgid || gid == c.sgid {
                c.gid = gid;
            } else {
                return Err(Errno::EPERM);
            }
            Ok(())
        })
    }

    fn setreuid(&self, r: Uid, e: Uid) -> SysResult<()> {
        self.enter();
        self.change_cred(|c| {
            let (old, cap) = (c.clone(), c.is_root());
            if r != ID_UNCHANGED {
                if old.ruid != r && old.uid != r && !cap {
                    return Err(Errno::EPERM);
                }
                c.ruid = r;
            }
            if e != ID_UNCHANGED {
                if old.ruid != e && old.uid != e && old.suid != e && !cap {
                    return Err(Errno::EPERM);
                }
                c.uid = e;
            }
            if r != ID_UNCHANGED || (e != ID_UNCHANGED && e != old.ruid) {
                c.suid = c.uid;
            }
            Ok(())
        })
    }

    fn setregid(&self, r: Gid, e: Gid) -> SysResult<()> {
        self.enter();
        self.change_cred(|c| {
            let (old, cap) = (c.clone(), c.is_root());
            if r != ID_UNCHANGED {
                if old.rgid != r && old.gid != r && !cap {
                    return Err(Errno::EPERM);
                }
                c.rgid = r;
            }
            if e != ID_UNCHANGED {
                if old.rgid != e && old.gid != e && old.sgid != e && !cap {
                    return Err(Errno::EPERM);
                }
                c.gid = e;
            }
            if r != ID_UNCHANGED || (e != ID_UNCHANGED && e != old.rgid) {
                c.sgid = c.gid;
            }
            Ok(())
        })
    }

    fn setresuid(&self, r: Uid, e: Uid, s: Uid) -> SysResult<()> {
        self.enter();
        self.change_cred(|c| {
            let ok = |v: Uid| v == ID_UNCHANGED || v == c.ruid || v == c.uid || v == c.suid;
            if !c.is_root() && !(ok(r) && ok(e) && ok(s)) {
                return Err(Errno::EPERM);
            }
            for (slot, v) in [(&mut c.ruid, r), (&mut c.uid, e), (&mut c.suid, s)] {
                if v != ID_UNCHANGED {
                    *slot = v;
                }
            }
            Ok(())
        })
    }

    fn setresgid(&self, r: Gid, e: Gid, s: Gid) -> SysResult<()> {
        self.enter();
        self.change_cred(|c| {
            let ok = |v: Gid| v == ID_UNCHANGED || v == c.rgid || v == c.gid || v == c.sgid;
            if !c.is_root() && !(ok(r) && ok(e) && ok(s)) {
                return Err(Errno::EPERM);
            }
            for (slot, v) in [(&mut c.rgid, r), (&mut c.gid, e), (&mut c.sgid, s)] {
                if v != ID_UNCHANGED {
                    *slot = v;
                }
            }
            Ok(())
        })
    }

    fn setgroups(&self, groups: &[Gid]) -> SysResult<()> {
        self.enter();
        if groups.len() > NGROUPS_MAX {
            return Err(Errno::EINVAL);
        }
        self.change_cred(|c| {
            if !c.is_root() {
                return Err(Errno::EPERM);
            }
            // `groups_sort`: o kernel guarda (e o getgroups devolve) a lista em ordem.
            let mut g = groups.to_vec();
            g.sort_unstable();
            c.groups = g;
            Ok(())
        })
    }

    fn argv(&self) -> Vec<Vec<u8>> {
        self.enter();
        self.proc.st.lock().argv.clone()
    }

    fn environ(&self) -> Vec<Vec<u8>> {
        self.enter();
        self.proc.st.lock().env.clone()
    }

    fn getenv(&self, name: &[u8]) -> Option<Vec<u8>> {
        self.enter();
        self.proc.st.lock().getenv(name)
    }

    fn setenv(&self, name: &[u8], value: &[u8]) -> SysResult<()> {
        self.enter();
        if name.is_empty() || name.contains(&b'=') {
            return Err(Errno::EINVAL);
        }
        let mut kv = name.to_vec();
        kv.push(b'=');
        kv.extend_from_slice(value);
        let mut st = self.proc.st.lock();
        match st.env.iter_mut().find(|e| env_name(e) == name) {
            Some(e) => *e = kv,
            None => st.env.push(kv),
        }
        Ok(())
    }

    fn unsetenv(&self, name: &[u8]) -> SysResult<()> {
        self.enter();
        if name.is_empty() || name.contains(&b'=') {
            return Err(Errno::EINVAL);
        }
        self.proc.st.lock().env.retain(|e| env_name(e) != name);
        Ok(())
    }

    fn uname(&self) -> Utsname {
        self.enter();
        let mut u = Utsname {
            sysname: b"Linux".to_vec(),
            nodename: self.sb.hostname.lock().clone(),
            release: UNAME_RELEASE.to_vec(),
            version: UNAME_VERSION.to_vec(),
            machine: b"x86_64".to_vec(),
            domainname: self.sb.domainname.lock().clone(),
        };
        // `override_release` e `override_architecture`: UNAME26 e PER_LINUX32.
        sched::personality::apply_to_uname(self.proc.tune.lock().personality, &mut u);
        u
    }

    fn sethostname(&self, name: &[u8]) -> SysResult<()> {
        self.enter();
        if !self.proc.st.lock().cred.is_root() {
            return Err(Errno::EPERM);
        }
        if name.len() > 64 {
            return Err(Errno::EINVAL);
        }
        *self.sb.hostname.lock() = name.to_vec();
        Ok(())
    }

    fn setdomainname(&self, name: &[u8]) -> SysResult<()> {
        self.enter();
        if !self.proc.st.lock().cred.is_root() {
            return Err(Errno::EPERM);
        }
        if name.len() > 64 {
            return Err(Errno::EINVAL);
        }
        *self.sb.domainname.lock() = name.to_vec();
        Ok(())
    }

    // ---- tempo e aleatoriedade ----

    fn clock_gettime(&self, clock: Clock) -> SysResult<TimeSpec> {
        self.enter();
        let ns_to_ts = |ns: u64| TimeSpec { sec: (ns / 1_000_000_000) as i64, nsec: (ns % 1_000_000_000) as u32 };
        Ok(match clock {
            Clock::Realtime => self.sb.now(),
            Clock::Monotonic | Clock::Boottime => ns_to_ts(self.sb.mono_ns()),
            Clock::ProcessCpuTime => ns_to_ts(self.proc.cpu_ns()),
            Clock::ThreadCpuTime => ns_to_ts(self.cpu_ns()),
        })
    }

    fn nanosleep(&self, d: Duration) -> SysResult<()> {
        self.enter();
        let deadline = Instant::now() + d;
        match self.wait_event(Some(deadline), |_| if Instant::now() >= deadline { Try::Ready(Ok(())) } else { Try::Pending }) {
            Err(Errno::ETIMEDOUT) => Ok(()),
            r => r,
        }
    }

    fn setitimer(&self, which: i32, new: Itimerval) -> SysResult<Itimerval> {
        self.enter();
        let which = Itimer::from_raw(which)?;
        let (value, interval) = new.to_ns()?;
        let (old_value, old_interval) = crate::itimer::set(&self.proc, which, value, interval);
        Ok(Itimerval::from_ns(old_value, old_interval))
    }

    fn getitimer(&self, which: i32) -> SysResult<Itimerval> {
        self.enter();
        let (value, interval) = crate::itimer::get(&self.proc, Itimer::from_raw(which)?);
        Ok(Itimerval::from_ns(value, interval))
    }

    fn getrandom(&self, buf: &mut [u8]) -> SysResult<usize> {
        self.enter();
        getrandom::fill(buf).map_err(|_| Errno::EIO)?;
        Ok(buf.len())
    }

    fn local_timezone(&self) -> Vec<u8> {
        self.enter();
        if let Some(tz) = self.proc.st.lock().getenv(b"TZ") {
            return tz;
        }
        let cx = self.caller();
        if let Ok(Opened::File { handle, stat, .. }) = self.sb.ns.open(&cx, &Start::Cwd, b"/etc/localtime", OFlags::RDONLY, 0) {
            let mut buf = vec![0u8; stat.size.min(1 << 20) as usize];
            if let Ok(n) = handle.read(&cx, 0, &mut buf) {
                buf.truncate(n);
                return buf;
            }
        }
        b"UTC".to_vec()
    }

    // ---- espera e travas ----

    fn poll(&self, fds: &mut [PollFd], timeout: Option<Duration>) -> SysResult<usize> {
        self.enter();
        if fds.len() as u64 > self.nofile() {
            return Err(Errno::EINVAL);
        }
        let deadline = timeout.map(|t| Instant::now() + t);
        let immediate = timeout == Some(Duration::ZERO);
        let r = self.wait_event(deadline, |_| {
            let mut n = 0;
            for pfd in fds.iter_mut() {
                pfd.revents = self.poll_one(pfd, !immediate);
                if !pfd.revents.is_empty() {
                    n += 1;
                }
            }
            if n > 0 || immediate { Try::Ready(Ok(n)) } else { Try::Pending }
        });
        // Solta os registros nos objetos observados.
        for pfd in fds.iter() {
            if let Ok(ofd) = self.ofd(pfd.fd) {
                unregister_ofd(&ofd, &self.parker);
            }
        }
        match r {
            Err(Errno::ETIMEDOUT) => Ok(0),
            r => r,
        }
    }

    fn epoll_create1(&self, cloexec: bool) -> SysResult<Fd> {
        self.enter();
        let ofd = Ofd::new(FileObj::Epoll(Arc::new(crate::epoll::Epoll::new())), OFlags::RDWR, Arc::downgrade(&self.sb.locks));
        self.install(ofd, cloexec)
    }

    fn eventfd(&self, initval: u32, flags: u32) -> SysResult<Fd> {
        self.enter();
        const EFD_SEMAPHORE: u32 = 1;
        let (nonblock, cloexec) = (OFlags::NONBLOCK.bits(), OFlags::CLOEXEC.bits());
        if flags & !(EFD_SEMAPHORE | nonblock | cloexec) != 0 {
            return Err(Errno::EINVAL);
        }
        let event = crate::anon::Eventfd::new(initval, flags & EFD_SEMAPHORE != 0);
        self.install_sock(FileObj::Anon(crate::anon::Anon::Eventfd(event)), flags & nonblock != 0, flags & cloexec != 0)
    }

    fn timerfd_create(&self, clock: i32, flags: u32) -> SysResult<Fd> {
        self.enter();
        let (nonblock, cloexec) = (OFlags::NONBLOCK.bits(), OFlags::CLOEXEC.bits());
        if flags & !(nonblock | cloexec) != 0 {
            return Err(Errno::EINVAL);
        }
        match clock {
            0 | 1 | 7 => {}
            // `CLOCK_REALTIME_ALARM` e `CLOCK_BOOTTIME_ALARM` pedem `CAP_WAKE_ALARM`, que o contêiner padrão não tem.
            8 | 9 => return Err(Errno::EPERM),
            _ => return Err(Errno::EINVAL),
        }
        let timer = crate::anon::Timerfd::new(clock);
        self.install_sock(FileObj::Anon(crate::anon::Anon::Timerfd(timer)), flags & nonblock != 0, flags & cloexec != 0)
    }

    fn timerfd_settime(&self, fd: Fd, flags: u32, value_ns: u64, interval_ns: u64) -> SysResult<(u64, u64)> {
        self.enter();
        const TFD_TIMER_ABSTIME: u32 = 1;
        const TFD_TIMER_CANCEL_ON_SET: u32 = 2;
        let ofd = self.ofd(fd)?;
        let FileObj::Anon(crate::anon::Anon::Timerfd(timer)) = &ofd.obj else { return Err(Errno::EINVAL) };
        if flags & !(TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET) != 0 || value_ns >= i64::MAX as u64 || interval_ns >= i64::MAX as u64 {
            return Err(Errno::EINVAL);
        }
        // O tempo absoluto vira o que falta no relógio do timer; um prazo no passado vence já.
        let mut value = Duration::from_nanos(value_ns);
        if flags & TFD_TIMER_ABSTIME != 0 && value_ns != 0 {
            let now = if timer.clock() == 0 {
                let t = self.sb.now();
                t.sec as u64 * 1_000_000_000 + u64::from(t.nsec)
            } else {
                self.sb.mono_ns()
            };
            value = Duration::from_nanos(value_ns.saturating_sub(now).max(1));
        }
        Ok(timer.settime(flags, value, Duration::from_nanos(interval_ns)))
    }

    fn timerfd_gettime(&self, fd: Fd) -> SysResult<(u64, u64)> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let FileObj::Anon(crate::anon::Anon::Timerfd(timer)) = &ofd.obj else { return Err(Errno::EINVAL) };
        Ok(timer.gettime())
    }

    fn memfd_create(&self, name: &[u8], flags: u32) -> SysResult<Fd> {
        self.enter();
        const MFD_CLOEXEC: u32 = 1;
        const MFD_ALLOW_SEALING: u32 = 2;
        const MFD_NOEXEC_SEAL: u32 = 8;
        const MFD_EXEC: u32 = 0x10;
        // Sem `hugetlbfs` o `MFD_HUGETLB` (e os bits de tamanho) é recusado; `NOEXEC_SEAL` com `EXEC` não combina.
        if flags & !(MFD_CLOEXEC | MFD_ALLOW_SEALING | MFD_NOEXEC_SEAL | MFD_EXEC) != 0 || flags & MFD_NOEXEC_SEAL != 0 && flags & MFD_EXEC != 0 {
            return Err(Errno::EINVAL);
        }
        // `NAME_MAX - strlen("memfd:") - 1`.
        if name.len() > 249 {
            return Err(Errno::EINVAL);
        }
        self.proc.fds.lock().lowest_free(0, self.nofile())?;
        // O arquivo nasce como `/memfd:NOME` na raiz do sandbox e perde o nome na hora: o link de
        // `/proc/<pid>/fd` mostra `/memfd:NOME (deleted)`, como no Linux. Quem cria é o kernel (o root).
        let mut path = b"/memfd:".to_vec();
        path.extend(name.iter().map(|&b| if b == b'/' { b'_' } else { b }));
        let ns = &self.sb.ns;
        let mut cx = vfs::ops::kernel_caller(ns.root.root());
        cx.now = self.sb.now();
        let mut tries = 0;
        let (loc, stat, handle) = loop {
            match ns.open(&cx, &Start::Cwd, &path, OFlags::RDWR | OFlags::CREAT | OFlags::EXCL, 0o777) {
                Ok(Opened::File { loc, stat, handle }) => break (loc, stat, handle),
                Ok(_) => return Err(Errno::EIO),
                // Outro `memfd_create` com o mesmo nome está entre o `open` e o `unlink`.
                Err(Errno::EEXIST) if tries < 1000 => {
                    tries += 1;
                    std::thread::yield_now();
                }
                Err(e) => return Err(e),
            }
        };
        ns.unlink(&cx, &Start::Cwd, &path, AtFlags::empty())?;
        let (uid, gid) = {
            let st = self.proc.st.lock();
            (st.cred.uid, st.cred.gid)
        };
        if uid != 0 || gid != 0 {
            ns.chown_loc(&cx, &loc, &stat, Some(uid), Some(gid))?;
        }
        let kind = stat.file_type();
        let ofd = Ofd::new(FileObj::Vfs { loc, handle, kind }, OFlags::RDWR, Arc::downgrade(&self.sb.locks));
        self.install(ofd, flags & MFD_CLOEXEC != 0)
    }

    fn chroot(&self, path: &[u8]) -> SysResult<()> {
        self.enter();
        let cx = self.caller();
        let l = self.sb.ns.chdir_target(&cx, &Start::Cwd, path)?;
        // `CAP_SYS_CHROOT` está nas capabilities do docker padrão, mas só o root efetivo as tem.
        if cx.cred.uid != 0 {
            return Err(Errno::EPERM);
        }
        let old = std::mem::replace(&mut self.proc.st.lock().root, PinnedLoc::new(l));
        drop(old);
        Ok(())
    }

    fn pidfd_open(&self, pid: Pid, flags: u32) -> SysResult<Fd> {
        self.enter();
        let nonblock = OFlags::NONBLOCK.bits();
        if flags & !(nonblock | crate::pidfd::PIDFD_THREAD) != 0 || pid <= 0 {
            return Err(Errno::EINVAL);
        }
        let proc = {
            let t = self.sb.table.lock();
            match t.proc(pid) {
                Some(p) => p,
                None if !t.tids.contains(&pid) => return Err(Errno::ESRCH),
                // Tid de thread que não é líder: só `PIDFD_THREAD` a aceita (o pidfd observa o processo dela).
                None if flags & crate::pidfd::PIDFD_THREAD == 0 => return Err(Errno::ENOENT),
                None => t.map.values().map(|e| e.proc.clone()).find(|p| p.threads.lock().live.contains_key(&pid)).ok_or(Errno::ESRCH)?,
            }
        };
        let status = if flags & nonblock != 0 { OFlags::RDWR | OFlags::NONBLOCK } else { OFlags::RDWR };
        let ofd = Ofd::new(FileObj::Pidfd(crate::pidfd::Pidfd::new(proc)), status, Arc::downgrade(&self.sb.locks));
        self.install(ofd, true)
    }

    fn epoll_ctl(&self, epfd: Fd, op: i32, fd: Fd, event: EpollEvent) -> SysResult<()> {
        self.enter();
        let ep = self.ofd(epfd)?;
        let target = self.ofd(fd)?;
        check_pollable(&target.obj)?;
        let FileObj::Epoll(epoll) = &ep.obj else { return Err(Errno::EINVAL) };
        self.epoll_ctl_ofd(&ep, epoll, op, fd, &target, event)
    }

    fn epoll_wait(&self, epfd: Fd, max: usize, timeout: Option<Duration>) -> SysResult<Vec<EpollEvent>> {
        self.enter();
        if max == 0 || max > crate::epoll::EP_MAX_EVENTS {
            return Err(Errno::EINVAL);
        }
        let ofd = self.ofd(epfd)?;
        let FileObj::Epoll(epoll) = &ofd.obj else { return Err(Errno::EINVAL) };
        let deadline = timeout.map(|t| Instant::now() + t);
        let immediate = timeout == Some(Duration::ZERO);
        let r = self.wait_event(deadline, |_| {
            let ready = epoll.scan(self, max, true, (!immediate).then_some(&self.parker));
            if !ready.is_empty() || immediate { Try::Ready(Ok(ready)) } else { Try::Pending }
        });
        epoll.unregister(&self.parker);
        match r {
            Err(Errno::ETIMEDOUT) => Ok(Vec::new()),
            r => r,
        }
    }

    fn ofd_setlk(&self, fd: Fd, lock: FileLock, wait: bool) -> SysResult<()> {
        self.enter();
        let ofd = self.ofd(fd)?;
        self.set_range_lock(&ofd, ofd.id, lock, wait)
    }

    fn ofd_getlk(&self, fd: Fd, lock: FileLock) -> SysResult<Option<FileLock>> {
        self.enter();
        let ofd = self.ofd(fd)?;
        Ok(self.test_range_lock(&ofd, ofd.id, lock)?.map(|(l, _)| l))
    }

    fn fcntl_lock(&self, fd: Fd, cmd: LockCmd, mut flock: Flock) -> SysResult<Flock> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let ofd_owned = matches!(cmd, LockCmd::OfdGet | LockCmd::OfdSet | LockCmd::OfdSetWait);
        let owner = if ofd_owned { ofd.id } else { POSIX_OWNER | self.proc.pid as u64 };
        let getting = matches!(cmd, LockCmd::Get | LockCmd::OfdGet);
        if getting && flock.l_type != sysabi::fcntl::F_RDLCK && flock.l_type != sysabi::fcntl::F_WRLCK {
            return Err(Errno::EINVAL);
        }
        let lock = self.posix_lock_of(&ofd, &flock)?;
        // As travas OFD pedem `l_pid` zero, antes de qualquer outra coisa sobre o arquivo.
        if ofd_owned && flock.pid != 0 {
            return Err(Errno::EINVAL);
        }
        if getting {
            if let Some((found, holder)) = self.test_range_lock(&ofd, owner, lock)? {
                flock = Flock {
                    l_type: if found.kind == LockKind::Write { sysabi::fcntl::F_WRLCK } else { sysabi::fcntl::F_RDLCK },
                    whence: sysabi::fcntl::SEEK_SET,
                    start: found.start as i64,
                    len: found.len as i64,
                    pid: lock_owner_pid(holder),
                };
            } else {
                flock.l_type = sysabi::fcntl::F_UNLCK;
            }
        } else {
            self.set_range_lock(&ofd, owner, lock, matches!(cmd, LockCmd::SetWait | LockCmd::OfdSetWait))?;
        }
        Ok(flock)
    }

    fn flock(&self, fd: Fd, op: u32) -> SysResult<()> {
        use sysabi::fcntl::{LOCK_EX, LOCK_MAND, LOCK_NB, LOCK_SH, LOCK_UN};
        self.enter();
        let ofd = self.ofd(fd)?;
        let cmd = op & !LOCK_NB;
        if cmd & LOCK_MAND != 0 {
            // O 6.12 não tem mais a trava obrigatória: ignora o pedido e responde 0.
            return Ok(());
        }
        let kind = match cmd {
            LOCK_SH => LockKind::Read,
            LOCK_EX => LockKind::Write,
            LOCK_UN => LockKind::Unlock,
            _ => return Err(Errno::EINVAL),
        };
        let key = lock_key_of(&ofd).ok_or(Errno::EBADF)?;
        if kind != LockKind::Unlock && !(ofd.readable || ofd.writable) {
            return Err(Errno::EBADF);
        }
        self.lock_wait(key, ofd.id | FLOCK_OWNER, FileLock { kind, start: 0, len: 0 }, op & LOCK_NB == 0)
    }

    fn pipe_size(&self, fd: Fd) -> SysResult<usize> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let FileObj::Pipe { end, .. } = &ofd.obj else { return Err(Errno::EBADF) };
        Ok(end.pipe.capacity())
    }

    fn set_pipe_size(&self, fd: Fd, size: u32) -> SysResult<usize> {
        self.enter();
        let ofd = self.ofd(fd)?;
        let FileObj::Pipe { end, .. } = &ofd.obj else { return Err(Errno::EBADF) };
        let privileged = self.proc.st.lock().cred.is_root();
        end.pipe.resize(size, self.sb.pipe_max_size.load(Ordering::Relaxed), privileged)
    }

    fn fionread(&self, fd: Fd) -> SysResult<i64> {
        self.enter();
        let ofd = self.ofd(fd)?;
        match &ofd.obj {
            // `ioctl_file_ioctl`: o que falta do arquivo regular, que fica negativo se `lseek` foi além do fim.
            FileObj::Vfs { loc, kind: FileType::Regular, .. } => {
                let size = self.sb.ns.stat_loc(&self.caller(), loc)?.size;
                Ok(size as i64 - ofd.st.lock().pos as i64)
            }
            FileObj::Pipe { end, .. } => Ok(end.pipe.pending() as i64),
            FileObj::Stream(c) => Ok(c.unread() as i64),
            FileObj::Unix(u) => u.unread().map(|n| n as i64),
            FileObj::Udp(u) => Ok(u.next_len() as i64),
            FileObj::Listener(_) => Err(Errno::EINVAL),
            FileObj::Dev { dev: Device::Pty(e), .. } => Ok(e.pty.fionread(e.master) as i64),
            FileObj::Path { .. } => Err(Errno::EBADF),
            FileObj::Vfs { .. } | FileObj::Dev { .. } | FileObj::Epoll(_) | FileObj::Pidfd(_) | FileObj::Anon(_) => Err(Errno::ENOTTY),
        }
    }

    // ---- rede ----

    fn net_connect(&self, host: &[u8], port: u16, _timeout: Option<Duration>) -> SysResult<NetConn> {
        self.enter();
        // O nome resolve como no glibc (`hosts: files`): um endereço literal vale como está; senão vale a
        // primeira linha do /etc/hosts do sandbox que tem o nome. O loopback é sempre alcançável: fala com
        // quem escuta no sandbox. Fora dele, a política padrão é a allowlist vazia (nenhum destino
        // liberado); a allowlist configurável entra no marco 3.
        let literal = std::str::from_utf8(host).ok().and_then(|h| h.parse::<std::net::IpAddr>().ok());
        let ip = match literal {
            Some(ip) => ip,
            None => {
                let hosts = crate::sandbox::SandboxFs { sb: &self.sb }.read_file(b"/etc/hosts").unwrap_or_default();
                crate::net::lookup_hosts(&hosts, host)
                    .or_else(|| host.eq_ignore_ascii_case(b"localhost.localdomain").then_some(std::net::Ipv4Addr::LOCALHOST.into()))
                    .ok_or(Errno::EACCES)?
            }
        };
        if !ip.is_loopback() {
            return Err(Errno::EACCES);
        }
        let conn = self.connect_new(ip, port)?;
        let local = conn.local;
        let fd = self.install_sock(FileObj::Stream(conn), false, true)?;
        let local_ip: std::net::IpAddr = if ip.is_ipv6() { std::net::Ipv6Addr::LOCALHOST.into() } else { std::net::Ipv4Addr::LOCALHOST.into() };
        Ok(NetConn { fd, peer: std::net::SocketAddr::new(ip, port), local: std::net::SocketAddr::new(local_ip, local) })
    }
}
