//! O lado do host: criar processo com stdin, stdout e stderr ligados a pontas de pipe que threads do
//! host leem e escrevem, esperar processo, e o `run` síncrono (atalho de spawn + leitura + wait).

use std::sync::Arc;
use std::time::{Duration, Instant};

use sysabi::{Errno, KillTarget, OFlags, Pid, ProcAttrs, Resource, Rusage, Signal, SpawnSpec, WaitStatus};
use vfs::{Caller, Cred, Opened, Start};

use crate::config::DEFAULT_RUN_TIMEOUT;
use crate::dev::Device;
use crate::exec;
use crate::fd::{FdTable, FileObj, Ofd};
use crate::park::Parker;
use crate::pipe::{Pipe, PipeEnd, Try, WriteError};
use crate::proc::{INIT_PID, default_rlimits};
use crate::sandbox::{Sandbox, SbInner};
use crate::signal::SigState;
use crate::spawn::{self, Body, ChildSpec};

/// O que ligar num fd padrão de um processo criado pelo host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StdioSpec {
    /// `/dev/null`.
    Null,
    /// Pipe com a outra ponta no host.
    Pipe,
}

/// stdin, stdout e stderr de um processo criado pelo host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostStdio {
    pub stdin: StdioSpec,
    pub stdout: StdioSpec,
    pub stderr: StdioSpec,
}

impl Default for HostStdio {
    fn default() -> Self {
        HostStdio { stdin: StdioSpec::Pipe, stdout: StdioSpec::Pipe, stderr: StdioSpec::Pipe }
    }
}

/// Resultado de uma leitura do host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOutcome {
    Data(usize),
    /// Todas as pontas de escrita fecharam e o pipe esvaziou.
    Eof,
    /// O prazo venceu sem dados.
    Timeout,
}

/// Ponta de leitura de um pipe, no host (stdout ou stderr de um processo).
pub struct HostReader {
    end: PipeEnd,
    parker: Arc<Parker>,
}

impl std::fmt::Debug for HostReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HostReader(pipe {})", self.end.pipe.ino)
    }
}

impl HostReader {
    /// Lê o que houver; espera até o prazo (`None` = sem prazo).
    pub fn read(&mut self, buf: &mut [u8], deadline: Option<Instant>) -> ReadOutcome {
        loop {
            match self.end.pipe.try_read(buf, false, &self.parker) {
                Try::Ready(Ok(0)) => return ReadOutcome::Eof,
                Try::Ready(Ok(n)) => return ReadOutcome::Data(n),
                Try::Ready(Err(_)) => return ReadOutcome::Eof,
                Try::Pending => {}
            }
            match deadline {
                Some(d) => {
                    if !self.parker.park_until(d) {
                        self.end.pipe.unregister(&self.parker);
                        if let Try::Ready(Ok(n)) = self.end.pipe.try_read(buf, true, &self.parker) {
                            return if n == 0 { ReadOutcome::Eof } else { ReadOutcome::Data(n) };
                        }
                        return ReadOutcome::Timeout;
                    }
                }
                None => self.parker.park(),
            }
        }
    }

    /// Lê sem esperar: `Some(0)` é EOF, `None` é "nada agora".
    pub fn try_read(&mut self, buf: &mut [u8]) -> Option<usize> {
        match self.end.pipe.try_read(buf, true, &self.parker) {
            Try::Ready(Ok(n)) => Some(n),
            _ => None,
        }
    }

    /// Lê até o EOF ou o prazo; devolve os bytes e se chegou ao EOF.
    pub fn read_to_end(&mut self, deadline: Option<Instant>) -> (Vec<u8>, bool) {
        let mut out = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match self.read(&mut buf, deadline) {
                ReadOutcome::Data(n) => out.extend_from_slice(&buf[..n]),
                ReadOutcome::Eof => return (out, true),
                ReadOutcome::Timeout => return (out, false),
            }
        }
    }
}

/// Ponta de escrita de um pipe, no host (stdin de um processo). Soltar fecha (EOF pro processo).
pub struct HostWriter {
    end: PipeEnd,
    parker: Arc<Parker>,
}

impl std::fmt::Debug for HostWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HostWriter(pipe {})", self.end.pipe.ino)
    }
}

impl HostWriter {
    /// Escreve tudo, esperando espaço até o prazo. EPIPE se o processo fechou a leitura; ETIMEDOUT com o
    /// que já foi escrito em `written`.
    pub fn write_all(&mut self, data: &[u8], deadline: Option<Instant>) -> Result<(), (Errno, usize)> {
        let mut done = 0usize;
        while done < data.len() {
            let chunk = &data[done..data.len().min(done + 64 * 1024)];
            let mut d = 0usize;
            loop {
                match self.end.pipe.try_write(chunk, &mut d, false, &self.parker) {
                    Try::Ready(Ok(_)) => break,
                    Try::Ready(Err(WriteError::BrokenPipe { written })) => return Err((Errno::EPIPE, done + written)),
                    Try::Ready(Err(WriteError::Again)) => {}
                    Try::Pending => {}
                }
                match deadline {
                    Some(dl) => {
                        if !self.parker.park_until(dl) {
                            self.end.pipe.unregister(&self.parker);
                            return Err((Errno::ETIMEDOUT, done + d));
                        }
                    }
                    None => self.parker.park(),
                }
            }
            done += chunk.len();
        }
        Ok(())
    }

    /// Escreve o que couber agora, sem esperar.
    pub fn try_write(&mut self, data: &[u8]) -> Result<usize, Errno> {
        let mut d = 0usize;
        match self.end.pipe.try_write(data, &mut d, true, &self.parker) {
            Try::Ready(Ok(n)) => Ok(n),
            Try::Ready(Err(WriteError::BrokenPipe { .. })) => Err(Errno::EPIPE),
            Try::Ready(Err(WriteError::Again)) | Try::Pending => Ok(d),
        }
    }
}

/// Processo criado pelo host.
#[derive(Debug)]
pub struct Spawned {
    pub pid: Pid,
    pub stdin: Option<HostWriter>,
    pub stdout: Option<HostReader>,
    pub stderr: Option<HostReader>,
}

/// Pedido de execução síncrona.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunRequest {
    /// argv; `argv[0]` sem barra é procurado no PATH do ambiente, como o `execvp`.
    pub argv: Vec<Vec<u8>>,
    /// Ambiente completo (`NAME=valor`); `None` = o padrão do sandbox.
    pub env: Option<Vec<Vec<u8>>>,
    /// cwd; `None` = o padrão do sandbox.
    pub cwd: Option<Vec<u8>>,
    pub stdin: Vec<u8>,
    /// Prazo de parede; vencido, o kernel manda SIGKILL na sessão inteira.
    pub timeout: Option<Duration>,
}

/// Resultado de [`Sandbox::run`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: WaitStatus,
    pub timed_out: bool,
    pub rusage: Rusage,
}

fn host_pipe(sb: &SbInner) -> Arc<Pipe> {
    Pipe::new(sb.kernel.pipe_ino(), 0, 0, sb.now())
}

fn dev_null(sb: &SbInner, cx: &Caller, flags: OFlags) -> Result<Arc<Ofd>, Errno> {
    let locks = Arc::downgrade(&sb.locks);
    match sb.ns.open(cx, &Start::Cwd, b"/dev/null", flags, 0)? {
        Opened::CharDev { loc, stat } => Ok(Ofd::new(FileObj::Dev { dev: Device::open(stat.rdev)?, loc: Some(loc) }, flags, locks)),
        _ => Err(Errno::ENXIO),
    }
}

/// Procura `name` no PATH do ambiente, como o `execvp`.
fn which(sb: &SbInner, cx: &Caller, name: &[u8], env: &[Vec<u8>]) -> Result<Vec<u8>, Errno> {
    if name.contains(&b'/') {
        return Ok(name.to_vec());
    }
    if name.is_empty() {
        return Err(Errno::ENOENT);
    }
    let path = env
        .iter()
        .find_map(|kv| kv.strip_prefix(b"PATH="))
        .map(|p| p.to_vec())
        .unwrap_or_else(|| b"/usr/local/bin:/usr/bin:/bin".to_vec());
    let mut eacces = false;
    for dir in path.split(|b| *b == b':') {
        let dir = if dir.is_empty() { b".".as_slice() } else { dir };
        let cand = [dir, b"/", name].concat();
        match sb.ns.exec_open(cx, &Start::Cwd, &cand) {
            Ok(_) => return Ok(cand),
            Err(Errno::EACCES) => eacces = true,
            Err(_) => {}
        }
    }
    Err(if eacces { Errno::EACCES } else { Errno::ENOENT })
}

/// Cria um processo a partir do host: filho do init, com o stdio pedido. Com `leader`, numa sessão nova
/// (líder de sessão e de grupo); sem, herda o grupo e a sessão do init, como o processo de um
/// `docker exec` sem terminal, que herda os do `runc init` e não é líder de grupo (o `setsid` do
/// util-linux, por exemplo, só faz fork quando é líder).
pub(crate) fn host_spawn(sb: &Arc<SbInner>, spec: SpawnSpec, stdio: HostStdio, leader: bool) -> Result<Spawned, Errno> {
    let cfg = &sb.cfg;
    let cred = Arc::new(Cred::new(cfg.uid, cfg.gid, vec![cfg.gid]));
    let mut cx = sb.root_caller();
    cx.cred = cred.clone();
    cx.umask = 0o022;
    let env = spec.attrs.env.clone().unwrap_or_else(|| cfg.env.clone());
    let cwd_path = spec.attrs.cwd.clone().unwrap_or_else(|| cfg.cwd.clone());
    let cwd = sb.ns.chdir_target(&cx, &Start::Cwd, &cwd_path)?;
    cx.cwd = cwd.clone();
    let mut fds = FdTable::default();
    let locks = Arc::downgrade(&sb.locks);
    let mut host_in = None;
    let mut host_out = None;
    let mut host_err = None;
    let limit = cfg.limits.nofile;
    match stdio.stdin {
        StdioSpec::Null => {
            fds.put(sysabi::Fd(0), dev_null(sb, &cx, OFlags::RDONLY)?, false);
        }
        StdioSpec::Pipe => {
            let p = host_pipe(sb);
            fds.put(sysabi::Fd(0), Ofd::new(FileObj::Pipe { end: p.attach(true, false), fifo: None }, OFlags::RDONLY, locks.clone()), false);
            host_in = Some(HostWriter { end: p.attach(false, true), parker: Parker::new() });
        }
    }
    for (fd, sp, slot) in [(1, stdio.stdout, &mut host_out), (2, stdio.stderr, &mut host_err)] {
        match sp {
            StdioSpec::Null => {
                fds.put(sysabi::Fd(fd), dev_null(sb, &cx, OFlags::WRONLY)?, false);
            }
            StdioSpec::Pipe => {
                let p = host_pipe(sb);
                fds.put(sysabi::Fd(fd), Ofd::new(FileObj::Pipe { end: p.attach(false, true), fifo: None }, OFlags::WRONLY, locks.clone()), false);
                *slot = Some(HostReader { end: p.attach(true, false), parker: Parker::new() });
            }
        }
    }
    let _ = limit;
    let mut child = ChildSpec {
        cred,
        cwd,
        root: sb.ns.root.root(),
        umask: 0o022,
        argv: spec.argv.clone(),
        env: env.clone(),
        comm: exec::comm_of(&spec.path),
        exe: None,
        rlimits: default_rlimits(cfg.limits.nofile, cfg.limits.fsize),
        nice: 0,
        sig: SigState::new(),
        fds,
        group: if leader { spec.attrs.group } else { sysabi::ProcessGroup::Inherit },
        new_session: leader,
        ppid: INIT_PID,
        host_tracked: true,
    };
    // Ações de fd do pedido (dup2, close, open), sinais.
    host_apply(sb, &mut child, &spec.attrs)?;
    let ccx = child.caller(sb, 0);
    let img = exec::load(sb, &ccx, &spec.path, spec.argv.clone(), child.env.clone())?;
    let task = spawn::insert_child(sb, child)?;
    let pid = task.proc.pid;
    spawn::commit_exec(&task, &img, false);
    spawn::start_process(sb, task, Body::Image(img))?;
    Ok(Spawned { pid, stdin: host_in, stdout: host_out, stderr: host_err })
}

/// `ProcAttrs` num processo criado pelo host (sem processo pai pra esperar FIFO: abrir FIFO aqui não
/// bloqueia).
fn host_apply(sb: &Arc<SbInner>, spec: &mut ChildSpec, attrs: &ProcAttrs) -> Result<(), Errno> {
    let limit = spec.rlimits[Resource::Nofile as usize].cur;
    for act in &attrs.fd_actions {
        match act {
            sysabi::FdAction::Dup2 { from, to } => {
                let ofd = spec.fds.ofd(*from)?;
                if to.0 < 0 || to.0 as u64 >= limit {
                    return Err(Errno::EBADF);
                }
                let old = spec.fds.put(*to, ofd, false);
                drop(old);
            }
            sysabi::FdAction::Close(fd) => {
                let old = spec.fds.remove(*fd);
                drop(old);
            }
            sysabi::FdAction::CloseFrom(from) => {
                if from.0 < 0 {
                    return Err(Errno::EBADF);
                }
                let closed: Vec<_> = spec.fds.fds().into_iter().filter(|fd| fd.0 >= from.0).filter_map(|fd| spec.fds.remove(fd)).collect();
                drop(closed);
            }
            sysabi::FdAction::Open { fd, path, flags, mode } => {
                let cx = spec.caller(sb, 0);
                let locks = Arc::downgrade(&sb.locks);
                let ofd = match sb.ns.open(&cx, &Start::Cwd, path, *flags, *mode)? {
                    Opened::File { loc, stat, handle } => Ofd::new(FileObj::Vfs { loc, handle, kind: stat.file_type() }, *flags, locks),
                    Opened::Path { loc, .. } => Ofd::new(FileObj::Path { loc }, *flags, locks),
                    Opened::CharDev { loc, stat } => Ofd::new(FileObj::Dev { dev: Device::open(stat.rdev)?, loc: Some(loc) }, *flags, locks),
                    Opened::Fifo { .. } | Opened::Object(_) => return Err(Errno::ENXIO),
                };
                let old = spec.fds.put(*fd, ofd, flags.contains(OFlags::CLOEXEC));
                drop(old);
            }
        }
    }
    for s in &attrs.reset_signals {
        if s.is_valid() && !s.is_uncatchable() {
            spec.sig.set_disposition(*s, sysabi::SigDisposition::Default);
        }
    }
    for s in &attrs.ignore_signals {
        if s.is_valid() && !s.is_uncatchable() {
            spec.sig.set_disposition(*s, sysabi::SigDisposition::Ignore);
        }
    }
    Ok(())
}

/// Espera um processo criado pelo host e colhe o zumbi.
pub(crate) fn host_wait(sb: &Arc<SbInner>, pid: Pid, deadline: Option<Instant>) -> Result<Option<(WaitStatus, Rusage)>, Errno> {
    let parker = Parker::new();
    loop {
        {
            let mut t = sb.table.lock();
            let r = t.rel(pid).ok_or(Errno::ECHILD)?;
            if !r.host_tracked {
                return Err(Errno::ECHILD);
            }
            if let Some(z) = r.zombie.clone() {
                t.reap(pid);
                return Ok(Some(z));
            }
            let proc = t.proc(pid).ok_or(Errno::ECHILD)?;
            proc.host_waiters.lock().register(&parker);
        }
        match deadline {
            Some(d) => {
                if !parker.park_until(d) {
                    let mut t = sb.table.lock();
                    if let Some(z) = t.rel(pid).and_then(|r| r.zombie.clone()) {
                        t.reap(pid);
                        return Ok(Some(z));
                    }
                    return Ok(None);
                }
            }
            None => parker.park(),
        }
    }
}

fn exited(sb: &SbInner, pid: Pid) -> bool {
    sb.table.lock().rel(pid).is_none_or(|r| r.zombie.is_some())
}

impl Sandbox {
    /// Cria um processo a partir do host (filho do init, sessão e grupo novos) com o stdio pedido.
    pub fn spawn(&self, spec: SpawnSpec, stdio: HostStdio) -> Result<Spawned, Errno> {
        host_spawn(&self.inner, spec, stdio, true)
    }

    /// Roda até o fim: escreve o stdin, junta stdout e stderr, espera o processo. No timeout manda SIGKILL
    /// na sessão do processo. Volta quando o processo principal termina (saída de processo em segundo
    /// plano que ainda segure o pipe depois disso não é esperada).
    pub fn run(&self, req: RunRequest) -> Result<RunOutput, Errno> {
        let sb = &self.inner;
        let env = req.env.clone().unwrap_or_else(|| sb.cfg.env.clone());
        let mut cx = sb.root_caller();
        let cwd_path = req.cwd.clone().unwrap_or_else(|| sb.cfg.cwd.clone());
        cx.cwd = sb.ns.chdir_target(&cx, &Start::Cwd, &cwd_path)?;
        let argv0 = req.argv.first().cloned().ok_or(Errno::EINVAL)?;
        let path = which(sb, &cx, &argv0, &env)?;
        let spec = SpawnSpec {
            path,
            argv: req.argv.clone(),
            attrs: ProcAttrs { env: Some(env), cwd: Some(cwd_path), ..ProcAttrs::default() },
        };
        // Igual ao oráculo (`docker exec`): o processo do caso não é líder de grupo nem de sessão.
        let mut sp = host_spawn(sb, spec, HostStdio::default(), false)?;
        let pid = sp.pid;
        let deadline = Instant::now() + req.timeout.unwrap_or(DEFAULT_RUN_TIMEOUT);
        let parker = Parker::new();
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut stdin_off = 0usize;
        if req.stdin.is_empty() {
            sp.stdin = None;
        }
        let mut timed_out = false;
        let mut out_eof = false;
        let mut err_eof = false;
        loop {
            // stdin
            if let Some(w) = sp.stdin.as_mut() {
                match w.try_write(&req.stdin[stdin_off..]) {
                    Ok(n) => stdin_off += n,
                    Err(_) => stdin_off = req.stdin.len(),
                }
                if stdin_off >= req.stdin.len() {
                    sp.stdin = None;
                } else {
                    w.end.pipe.poll(false, true, Some(&parker));
                }
            }
            // stdout e stderr
            for (r, sink, eof) in [(&mut sp.stdout, &mut out, &mut out_eof), (&mut sp.stderr, &mut err, &mut err_eof)] {
                if let Some(rd) = r.as_mut() {
                    loop {
                        match rd.try_read(&mut buf) {
                            Some(0) => {
                                *eof = true;
                                break;
                            }
                            Some(n) => sink.extend_from_slice(&buf[..n]),
                            None => break,
                        }
                    }
                    if !*eof {
                        rd.end.pipe.poll(true, false, Some(&parker));
                    }
                }
            }
            let done = exited(sb, pid);
            if done || (out_eof && err_eof && exited(sb, pid)) {
                break;
            }
            if let Some(p) = sb.table.lock().proc(pid) {
                p.host_waiters.lock().register(&parker);
            }
            if exited(sb, pid) {
                continue;
            }
            if !parker.park_until(deadline) && !timed_out && Instant::now() >= deadline {
                timed_out = true;
                let session: Vec<Pid> = {
                    let t = sb.table.lock();
                    let sid = t.rel(pid).map(|r| r.sid).unwrap_or(pid);
                    if sid != INIT_PID {
                        t.map.iter().filter(|(_, e)| e.rel.sid == sid).map(|(p, _)| *p).collect()
                    } else {
                        // Sessão do init (processo de `run`, que não é líder): só o processo e os
                        // descendentes, nunca o init nem o que mais rodar no sandbox.
                        let mut out = vec![pid];
                        let mut i = 0;
                        while i < out.len() {
                            if let Some(r) = t.rel(out[i]) {
                                out.extend(r.children.iter().copied());
                            }
                            i += 1;
                        }
                        out
                    }
                };
                for p in session {
                    let _ = crate::sys::host_kill(sb, KillTarget::Pid(p), Signal::SIGKILL);
                }
            }
        }
        // O que ficou no buffer depois do fim.
        for (r, sink) in [(&mut sp.stdout, &mut out), (&mut sp.stderr, &mut err)] {
            if let Some(rd) = r.as_mut() {
                while let Some(n) = rd.try_read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    sink.extend_from_slice(&buf[..n]);
                }
            }
        }
        let (status, rusage) = host_wait(sb, pid, None)?.ok_or(Errno::ECHILD)?;
        Ok(RunOutput { stdout: out, stderr: err, status, timed_out, rusage })
    }
}
