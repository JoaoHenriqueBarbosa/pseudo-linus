//! `std::process` do pseudo-processo: término, processos filhos (`Command`) e `exec`.
//!
//! - [`exit`] não pode ser `std::process::exit` (mataria o host com todos os pseudo-processos):
//!   desenrola a pilha com o `ExitUnwind` do `sysabi`, e o kernel termina o processo com o código.
//! - [`Command`] tem a API do std e cria o filho com `sysabi::Syscalls::spawn` (o `posix_spawn` do
//!   pseudo-linus). A busca no `PATH` é a do `execvp` da glibc: tenta cada diretório, pula ENOENT e
//!   ENOTDIR, lembra de EACCES, e arquivo executável sem `#!` (ENOEXEC) roda com `/bin/sh`.
//! - Antes de criar filho e antes de `exec`, o stdout com buffer do processo é descarregado, pra
//!   que a saída do pai não fique atrás da do filho (é o que os programas GNU fazem com `fflush`).

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

pub use std::process::ExitCode;

use sysabi::{
    AccessMode, AtFlags, Errno, Fd, FdAction, KillTarget, OFlags, Pid, ProcAttrs, ProcessGroup, Signal, SpawnSpec,
    WaitOptions, WaitStatus, WaitTarget,
};

use crate::errno::{cvt, from_errno};
use crate::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, IntoRawFd, OwnedFd, RawFd};
use crate::proc;

pub use crate::proc::ExitRequest;

/// `exit(3)` do pseudo-processo: descarrega o stdout e termina com `code`.
pub fn exit(code: i32) -> ! {
    let _ = crate::io::flush_stdout();
    sysabi::sys::exit(code)
}

/// `abort(3)`: SIGABRT no próprio processo.
pub fn abort() -> ! {
    let sys = proc::sys();
    let _ = sys.kill(KillTarget::Pid(sys.getpid()), Signal::SIGABRT);
    sysabi::sys::exit(134)
}

/// `getpid(2)`.
pub fn id() -> u32 {
    proc::sys().getpid() as u32
}

/// PATH padrão da glibc (`confstr(_CS_PATH)`) quando o processo não tem `PATH`.
pub const DEFAULT_PATH: &[u8] = b"/bin:/usr/bin";

/// Procura `name` no PATH como o `execvp`, sem executar: o primeiro candidato que existe e é
/// executável. `path` é o PATH a usar (`None` usa o do processo).
pub fn resolve_in_path(name: &[u8], path: Option<&[u8]>) -> Option<Vec<u8>> {
    if name.contains(&b'/') {
        return Some(name.to_vec());
    }
    let sys = proc::sys();
    let own = sys.getenv(b"PATH");
    let path = path.or(own.as_deref()).unwrap_or(DEFAULT_PATH);
    for cand in path_candidates(name, path) {
        if let Ok(st) = sys.fstatat(Fd::CWD, &cand, AtFlags::empty())
            && st.file_type() == sysabi::FileType::Regular
            && sys.faccessat(Fd::CWD, &cand, AccessMode::X_OK, AtFlags::empty()).is_ok()
        {
            return Some(cand);
        }
    }
    None
}

fn path_candidates(name: &[u8], path: &[u8]) -> Vec<Vec<u8>> {
    path.split(|b| *b == b':')
        .map(|dir| {
            let mut c = if dir.is_empty() { Vec::new() } else { dir.to_vec() };
            if !c.is_empty() && !c.ends_with(b"/") {
                c.push(b'/');
            }
            c.extend_from_slice(name);
            c
        })
        .collect()
}

/// Erros com que o `execvp` passa pro próximo diretório do PATH.
fn try_next(e: Errno) -> bool {
    matches!(e, Errno::ENOENT | Errno::ENOTDIR | Errno::ESTALE | Errno::ENODEV | Errno::ETIMEDOUT)
}

// ---------------------------------------------------------------------------------------------
// Stdio

enum StdioKind {
    Inherit,
    Null,
    Piped,
    Fd(OwnedFd),
}

/// Destino de um fluxo padrão do filho.
pub struct Stdio(StdioKind);

impl Stdio {
    pub fn inherit() -> Stdio {
        Stdio(StdioKind::Inherit)
    }
    pub fn null() -> Stdio {
        Stdio(StdioKind::Null)
    }
    pub fn piped() -> Stdio {
        Stdio(StdioKind::Piped)
    }
}

impl fmt::Debug for Stdio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            StdioKind::Inherit => f.write_str("Stdio::inherit"),
            StdioKind::Null => f.write_str("Stdio::null"),
            StdioKind::Piped => f.write_str("Stdio::piped"),
            StdioKind::Fd(fd) => write!(f, "Stdio::fd({})", fd.as_raw_fd()),
        }
    }
}

impl From<OwnedFd> for Stdio {
    fn from(fd: OwnedFd) -> Stdio {
        Stdio(StdioKind::Fd(fd))
    }
}

impl From<crate::fs::File> for Stdio {
    fn from(f: crate::fs::File) -> Stdio {
        Stdio(StdioKind::Fd(OwnedFd::from(f)))
    }
}

impl From<ChildStdin> for Stdio {
    fn from(c: ChildStdin) -> Stdio {
        Stdio(StdioKind::Fd(c.0))
    }
}

impl From<ChildStdout> for Stdio {
    fn from(c: ChildStdout) -> Stdio {
        Stdio(StdioKind::Fd(c.0))
    }
}

impl From<ChildStderr> for Stdio {
    fn from(c: ChildStderr) -> Stdio {
        Stdio(StdioKind::Fd(c.0))
    }
}

/// `Stdio::from(io::stdout())` etc.: o filho recebe uma cópia do fd do pai.
impl From<crate::io::Stdout> for Stdio {
    fn from(_: crate::io::Stdout) -> Stdio {
        let _ = crate::io::flush_stdout();
        BorrowedFd::borrow_raw(1).try_clone_to_owned().map(Stdio::from).unwrap_or_else(|_| Stdio::inherit())
    }
}

impl From<crate::io::Stderr> for Stdio {
    fn from(_: crate::io::Stderr) -> Stdio {
        BorrowedFd::borrow_raw(2).try_clone_to_owned().map(Stdio::from).unwrap_or_else(|_| Stdio::inherit())
    }
}

// ---------------------------------------------------------------------------------------------
// Command

/// `std::process::Command`.
pub struct Command {
    program: OsString,
    arg0: Option<OsString>,
    args: Vec<OsString>,
    env_clear: bool,
    /// Alterações no ambiente, na ordem em que foram feitas (`None` remove).
    env: BTreeMap<OsString, Option<OsString>>,
    env_order: Vec<OsString>,
    cwd: Option<OsString>,
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
    stderr: Option<Stdio>,
    pgroup: Option<Pid>,
}

impl fmt::Debug for Command {
    /// Mesmo formato do `Debug` do std: `"prog" "arg1" "arg2"`, com o ambiente alterado na frente.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(dir) = &self.cwd {
            write!(f, "cd {dir:?} && ")?;
        }
        if self.env_clear {
            f.write_str("env -i ")?;
        }
        for k in &self.env_order {
            if let Some(Some(v)) = self.env.get(k) {
                write!(f, "{}={v:?} ", k.to_string_lossy())?;
            }
        }
        write!(f, "{:?}", self.arg0.as_ref().unwrap_or(&self.program))?;
        for a in &self.args {
            write!(f, " {a:?}")?;
        }
        Ok(())
    }
}

impl Command {
    pub fn new<S: AsRef<OsStr>>(program: S) -> Command {
        Command {
            program: program.as_ref().to_os_string(),
            arg0: None,
            args: Vec::new(),
            env_clear: false,
            env: BTreeMap::new(),
            env_order: Vec::new(),
            cwd: None,
            stdin: None,
            stdout: None,
            stderr: None,
            pgroup: None,
        }
    }

    pub fn arg<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Command {
        self.args.push(arg.as_ref().to_os_string());
        self
    }

    pub fn args<I, S>(&mut self, args: I) -> &mut Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        for a in args {
            self.arg(a);
        }
        self
    }

    fn touch_env(&mut self, key: &OsStr, value: Option<OsString>) {
        let key = key.to_os_string();
        if !self.env.contains_key(&key) {
            self.env_order.push(key.clone());
        }
        self.env.insert(key, value);
    }

    pub fn env<K: AsRef<OsStr>, V: AsRef<OsStr>>(&mut self, key: K, val: V) -> &mut Command {
        self.touch_env(key.as_ref(), Some(val.as_ref().to_os_string()));
        self
    }

    pub fn envs<I, K, V>(&mut self, vars: I) -> &mut Command
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
        for (k, v) in vars {
            self.env(k, v);
        }
        self
    }

    pub fn env_remove<K: AsRef<OsStr>>(&mut self, key: K) -> &mut Command {
        self.touch_env(key.as_ref(), None);
        self
    }

    pub fn env_clear(&mut self) -> &mut Command {
        self.env_clear = true;
        self.env.clear();
        self.env_order.clear();
        self
    }

    pub fn current_dir<P: AsRef<Path>>(&mut self, dir: P) -> &mut Command {
        self.cwd = Some(dir.as_ref().as_os_str().to_os_string());
        self
    }

    pub fn stdin<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Command {
        self.stdin = Some(cfg.into());
        self
    }

    pub fn stdout<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Command {
        self.stdout = Some(cfg.into());
        self
    }

    pub fn stderr<T: Into<Stdio>>(&mut self, cfg: T) -> &mut Command {
        self.stderr = Some(cfg.into());
        self
    }

    pub fn get_program(&self) -> &OsStr {
        &self.program
    }

    pub fn get_args(&self) -> std::slice::Iter<'_, OsString> {
        self.args.iter()
    }

    pub fn get_envs(&self) -> impl Iterator<Item = (&OsStr, Option<&OsStr>)> {
        self.env_order.iter().map(|k| (k.as_os_str(), self.env.get(k).and_then(|v| v.as_deref())))
    }

    pub fn get_current_dir(&self) -> Option<&Path> {
        self.cwd.as_deref().map(Path::new)
    }

    fn env_modified(&self) -> bool {
        self.env_clear || !self.env.is_empty()
    }

    /// Ambiente completo do filho, na ordem do pai, com as alterações aplicadas no lugar e as
    /// variáveis novas no fim.
    fn child_env(&self) -> Option<Vec<Vec<u8>>> {
        if !self.env_modified() {
            return None;
        }
        let mut out: Vec<Vec<u8>> = Vec::new();
        let mut done: Vec<&OsString> = Vec::new();
        if !self.env_clear {
            for kv in proc::sys().environ() {
                let name_end = kv.iter().skip(1).position(|b| *b == b'=').map(|p| p + 1).unwrap_or(kv.len());
                let name = OsStr::from_bytes(&kv[..name_end]);
                match self.env.get_key_value(name) {
                    Some((k, Some(v))) => {
                        let mut e = k.as_bytes().to_vec();
                        e.push(b'=');
                        e.extend_from_slice(v.as_bytes());
                        out.push(e);
                        done.push(k);
                    }
                    Some((k, None)) => done.push(k),
                    None => out.push(kv),
                }
            }
        }
        for k in &self.env_order {
            if done.contains(&k) {
                continue;
            }
            if let Some(Some(v)) = self.env.get(k) {
                let mut e = k.as_bytes().to_vec();
                e.push(b'=');
                e.extend_from_slice(v.as_bytes());
                out.push(e);
            }
        }
        Some(out)
    }

    /// PATH usado na busca do programa: o do ambiente do filho se ele foi definido, senão o do pai.
    fn search_path(&self) -> Vec<u8> {
        if let Some(Some(p)) = self.env.get(OsStr::new("PATH")) {
            return p.as_bytes().to_vec();
        }
        if self.env_clear || matches!(self.env.get(OsStr::new("PATH")), Some(None)) {
            return DEFAULT_PATH.to_vec();
        }
        proc::sys().getenv(b"PATH").unwrap_or_else(|| DEFAULT_PATH.to_vec())
    }

    fn candidates(&self) -> Vec<Vec<u8>> {
        let name = self.program.as_bytes();
        if name.contains(&b'/') {
            return vec![name.to_vec()];
        }
        if name.is_empty() {
            return Vec::new();
        }
        path_candidates(name, &self.search_path())
    }

    fn argv(&self) -> Vec<Vec<u8>> {
        let mut argv = vec![self.arg0.as_ref().unwrap_or(&self.program).as_bytes().to_vec()];
        argv.extend(self.args.iter().map(|a| a.as_bytes().to_vec()));
        argv
    }

    pub fn spawn(&mut self) -> io::Result<Child> {
        self.spawn_with(StdioKind::Inherit, StdioKind::Inherit)
    }

    pub fn status(&mut self) -> io::Result<ExitStatus> {
        self.spawn()?.wait()
    }

    pub fn output(&mut self) -> io::Result<Output> {
        let child = self.spawn_with(StdioKind::Null, StdioKind::Piped)?;
        child.wait_with_output()
    }

    fn spawn_with(&mut self, default_in: StdioKind, default_out: StdioKind) -> io::Result<Child> {
        let _ = crate::io::flush_stdout();
        let sys = proc::sys();
        let mut actions = Vec::new();
        // fds do lado do filho que o pai fecha depois do spawn.
        let mut child_ends: Vec<OwnedFd> = Vec::new();
        let mut parent_ends: [Option<OwnedFd>; 3] = [None, None, None];
        let default_err = match default_out {
            StdioKind::Piped => StdioKind::Piped,
            _ => StdioKind::Inherit,
        };
        let cfgs = [
            self.stdin.take().map(|s| s.0).unwrap_or(default_in),
            self.stdout.take().map(|s| s.0).unwrap_or(default_out),
            self.stderr.take().map(|s| s.0).unwrap_or(default_err),
        ];
        for (n, cfg) in cfgs.into_iter().enumerate() {
            let target = Fd(n as i32);
            match cfg {
                StdioKind::Inherit => {}
                StdioKind::Null => actions.push(FdAction::Open {
                    fd: target,
                    path: b"/dev/null".to_vec(),
                    flags: if n == 0 { OFlags::RDONLY } else { OFlags::WRONLY },
                    mode: 0,
                }),
                StdioKind::Piped => {
                    let (r, w) = cvt(sys.pipe2(OFlags::CLOEXEC))?;
                    let (r, w) = (OwnedFd::from_raw_fd(r.0), OwnedFd::from_raw_fd(w.0));
                    let (mine, theirs) = if n == 0 { (w, r) } else { (r, w) };
                    actions.push(FdAction::Dup2 { from: theirs.raw(), to: target });
                    child_ends.push(theirs);
                    parent_ends[n] = Some(mine);
                }
                StdioKind::Fd(fd) => {
                    actions.push(FdAction::Dup2 { from: fd.raw(), to: target });
                    child_ends.push(fd);
                }
            }
        }
        let attrs = ProcAttrs {
            env: self.child_env(),
            cwd: self.cwd.as_ref().map(|c| c.as_bytes().to_vec()),
            fd_actions: actions,
            group: match self.pgroup {
                None => ProcessGroup::Inherit,
                Some(0) => ProcessGroup::New,
                Some(g) => ProcessGroup::Join(g),
            },
            ..ProcAttrs::default()
        };
        let argv = self.argv();
        let mut saw_eacces = false;
        let mut result: Option<io::Result<Pid>> = None;
        for path in self.candidates() {
            let spec = SpawnSpec { path: path.clone(), argv: argv.clone(), attrs: attrs.clone() };
            match sys.spawn(spec) {
                Ok(pid) => {
                    result = Some(Ok(pid));
                    break;
                }
                Err(Errno::ENOEXEC) => {
                    // Como o execvp: arquivo executável sem `#!` roda com o shell.
                    let mut sh_argv = vec![b"/bin/sh".to_vec(), path.clone()];
                    sh_argv.extend(argv.iter().skip(1).cloned());
                    let spec = SpawnSpec { path: b"/bin/sh".to_vec(), argv: sh_argv, attrs: attrs.clone() };
                    result = Some(cvt(sys.spawn(spec)));
                    break;
                }
                Err(Errno::EACCES) => saw_eacces = true,
                Err(e) if try_next(e) => {}
                Err(e) => {
                    result = Some(Err(from_errno(e)));
                    break;
                }
            }
        }
        drop(child_ends);
        let pid = match result {
            Some(r) => r?,
            None => return Err(from_errno(if saw_eacces { Errno::EACCES } else { Errno::ENOENT })),
        };
        let [i, o, e] = parent_ends;
        Ok(Child {
            pid,
            status: None,
            stdin: i.map(ChildStdin),
            stdout: o.map(ChildStdout),
            stderr: e.map(ChildStderr),
        })
    }

    /// `CommandExt::exec`: troca o programa do processo corrente. Só volta em caso de erro.
    fn do_exec(&mut self) -> io::Error {
        let _ = crate::io::flush_stdout();
        let sys = proc::sys();
        for (n, cfg) in [self.stdin.take(), self.stdout.take(), self.stderr.take()].into_iter().enumerate() {
            let target = Fd(n as i32);
            let r = match cfg.map(|s| s.0) {
                None | Some(StdioKind::Inherit) | Some(StdioKind::Piped) => Ok(()),
                Some(StdioKind::Null) => {
                    let flags = if n == 0 { OFlags::RDONLY } else { OFlags::WRONLY };
                    sys.openat(Fd::CWD, b"/dev/null", flags, 0).and_then(|fd| {
                        let r = if fd == target { Ok(target) } else { sys.dup3(fd, target, false) };
                        if fd != target {
                            let _ = sys.close(fd);
                        }
                        r.map(|_| ())
                    })
                }
                Some(StdioKind::Fd(fd)) => {
                    if fd.raw() == target {
                        let r = sys.set_cloexec(target, false);
                        std::mem::forget(fd);
                        r
                    } else {
                        sys.dup3(fd.raw(), target, false).map(|_| ())
                    }
                }
            };
            if let Err(e) = r {
                return from_errno(e);
            }
        }
        if let Some(dir) = &self.cwd
            && let Err(e) = sys.chdir(dir.as_bytes())
        {
            return from_errno(e);
        }
        if let Some(g) = self.pgroup
            && let Err(e) = sys.setpgid(0, g)
        {
            return from_errno(e);
        }
        let env = self.child_env();
        let argv = self.argv();
        let mut saw_eacces = false;
        for path in self.candidates() {
            match sys.execve(&path, &argv, env.as_deref()) {
                Errno::ENOEXEC => {
                    let mut sh_argv = vec![b"/bin/sh".to_vec(), path.clone()];
                    sh_argv.extend(argv.iter().skip(1).cloned());
                    return from_errno(sys.execve(b"/bin/sh", &sh_argv, env.as_deref()));
                }
                Errno::EACCES => saw_eacces = true,
                e if try_next(e) => {}
                e => return from_errno(e),
            }
        }
        from_errno(if saw_eacces { Errno::EACCES } else { Errno::ENOENT })
    }
}

/// `std::os::unix::process::CommandExt`.
pub trait CommandExt {
    /// Troca o programa do processo corrente (mesmo pid). Só volta em caso de erro.
    fn exec(&mut self) -> io::Error;
    fn arg0<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Command;
    /// `0` cria grupo novo com o pid do filho.
    fn process_group(&mut self, pgroup: i32) -> &mut Command;
}

impl CommandExt for Command {
    fn exec(&mut self) -> io::Error {
        self.do_exec()
    }
    fn arg0<S: AsRef<OsStr>>(&mut self, arg: S) -> &mut Command {
        self.arg0 = Some(arg.as_ref().to_os_string());
        self
    }
    fn process_group(&mut self, pgroup: i32) -> &mut Command {
        self.pgroup = Some(pgroup);
        self
    }
}

// ---------------------------------------------------------------------------------------------
// Child

#[derive(Debug)]
pub struct ChildStdin(OwnedFd);
#[derive(Debug)]
pub struct ChildStdout(OwnedFd);
#[derive(Debug)]
pub struct ChildStderr(OwnedFd);

impl Write for ChildStdin {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let sys = proc::sys();
        loop {
            match sys.write(self.0.raw(), buf) {
                Err(Errno::EINTR) => continue,
                r => return cvt(r),
            }
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn read_child(fd: &OwnedFd, buf: &mut [u8]) -> io::Result<usize> {
    let sys = proc::sys();
    loop {
        match sys.read(fd.raw(), buf) {
            Err(Errno::EINTR) => continue,
            r => return cvt(r),
        }
    }
}

impl Read for ChildStdout {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        read_child(&self.0, buf)
    }
}

impl Read for ChildStderr {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        read_child(&self.0, buf)
    }
}

macro_rules! child_fd {
    ($t:ty) => {
        impl AsRawFd for $t {
            fn as_raw_fd(&self) -> RawFd {
                self.0.as_raw_fd()
            }
        }
        impl AsFd for $t {
            fn as_fd(&self) -> BorrowedFd<'_> {
                self.0.as_fd()
            }
        }
        impl IntoRawFd for $t {
            fn into_raw_fd(self) -> RawFd {
                self.0.into_raw_fd()
            }
        }
        impl From<$t> for OwnedFd {
            fn from(c: $t) -> OwnedFd {
                c.0
            }
        }
    };
}

child_fd!(ChildStdin);
child_fd!(ChildStdout);
child_fd!(ChildStderr);

/// Um processo filho.
#[derive(Debug)]
pub struct Child {
    pid: Pid,
    status: Option<ExitStatus>,
    pub stdin: Option<ChildStdin>,
    pub stdout: Option<ChildStdout>,
    pub stderr: Option<ChildStderr>,
}

impl Child {
    pub fn id(&self) -> u32 {
        self.pid as u32
    }

    /// SIGKILL no filho (se ele ainda não foi colhido).
    pub fn kill(&mut self) -> io::Result<()> {
        if self.status.is_some() {
            return Ok(());
        }
        cvt(proc::sys().kill(KillTarget::Pid(self.pid), Signal::SIGKILL))
    }

    /// Espera o filho terminar. Como o std, fecha o stdin do filho antes.
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        drop(self.stdin.take());
        if let Some(st) = self.status {
            return Ok(st);
        }
        let sys = proc::sys();
        loop {
            match sys.wait4(WaitTarget::Pid(self.pid), WaitOptions::empty()) {
                Ok(Some((_, st))) => {
                    if matches!(st, WaitStatus::Stopped(_) | WaitStatus::Continued) {
                        continue;
                    }
                    let st = ExitStatus(st);
                    self.status = Some(st);
                    return Ok(st);
                }
                // Sem NOHANG o kernel bloqueia; `None` só aparece num kernel síncrono de teste.
                Ok(None) => sys.sched_yield(),
                Err(Errno::EINTR) => {}
                Err(e) => return Err(from_errno(e)),
            }
        }
    }

    /// `waitpid(..., WNOHANG)`.
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        if let Some(st) = self.status {
            return Ok(Some(st));
        }
        match proc::sys().wait4(WaitTarget::Pid(self.pid), WaitOptions::NOHANG) {
            Ok(Some((_, st))) if !matches!(st, WaitStatus::Stopped(_) | WaitStatus::Continued) => {
                let st = ExitStatus(st);
                self.status = Some(st);
                Ok(Some(st))
            }
            Ok(_) => Ok(None),
            Err(e) => Err(from_errno(e)),
        }
    }

    /// Lê stdout e stderr do filho até o fim (stderr numa thread, pra que um filho que enche o pipe
    /// de stderr não trave a leitura do stdout) e espera ele terminar.
    pub fn wait_with_output(mut self) -> io::Result<Output> {
        drop(self.stdin.take());
        let err_thread = self.stderr.take().map(|mut e| {
            crate::thread::spawn(move || {
                let mut buf = Vec::new();
                e.read_to_end(&mut buf).map(|_| buf)
            })
        });
        let mut stdout = Vec::new();
        if let Some(mut o) = self.stdout.take() {
            o.read_to_end(&mut stdout)?;
        }
        let stderr = match err_thread {
            Some(t) => t.join().map_err(|_| io::Error::other("thread do stderr do filho terminou com panic"))??,
            None => Vec::new(),
        };
        let status = self.wait()?;
        Ok(Output { status, stdout, stderr })
    }
}

/// Status de término de um filho.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitStatus(WaitStatus);

impl ExitStatus {
    pub fn from_wait_status(st: WaitStatus) -> ExitStatus {
        ExitStatus(st)
    }
    pub fn wait_status(&self) -> WaitStatus {
        self.0
    }
    pub fn success(&self) -> bool {
        self.0 == WaitStatus::Exited(0)
    }
    pub fn code(&self) -> Option<i32> {
        match self.0 {
            WaitStatus::Exited(c) => Some(c),
            _ => None,
        }
    }
    pub fn exit_ok(&self) -> Result<(), ExitStatus> {
        if self.success() { Ok(()) } else { Err(*self) }
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            WaitStatus::Exited(c) => write!(f, "exit status: {c}"),
            WaitStatus::Signaled { signal, core_dumped } => {
                let name = signal.name().map(|n| format!(" (SIG{n})")).unwrap_or_default();
                write!(f, "signal: {}{name}{}", signal.0, if core_dumped { " (core dumped)" } else { "" })
            }
            WaitStatus::Stopped(s) => write!(f, "stopped (not terminated) by signal: {}", s.0),
            WaitStatus::Continued => f.write_str("continued (WIFCONTINUED)"),
        }
    }
}

/// `std::os::unix::process::ExitStatusExt`.
pub trait ExitStatusExt {
    fn from_raw(raw: i32) -> Self;
    fn signal(&self) -> Option<i32>;
    fn core_dumped(&self) -> bool;
    fn stopped_signal(&self) -> Option<i32>;
    fn continued(&self) -> bool;
    fn into_raw(self) -> i32;
}

impl ExitStatusExt for ExitStatus {
    /// Codificação do `wait(2)` do Linux.
    fn from_raw(raw: i32) -> Self {
        let sig = raw & 0x7f;
        ExitStatus(if raw == 0xffff {
            WaitStatus::Continued
        } else if sig == 0 {
            WaitStatus::Exited((raw >> 8) & 0xff)
        } else if sig == 0x7f {
            WaitStatus::Stopped(Signal((raw >> 8) & 0xff))
        } else {
            WaitStatus::Signaled { signal: Signal(sig), core_dumped: raw & 0x80 != 0 }
        })
    }
    fn signal(&self) -> Option<i32> {
        match self.0 {
            WaitStatus::Signaled { signal, .. } => Some(signal.0),
            _ => None,
        }
    }
    fn core_dumped(&self) -> bool {
        matches!(self.0, WaitStatus::Signaled { core_dumped: true, .. })
    }
    fn stopped_signal(&self) -> Option<i32> {
        match self.0 {
            WaitStatus::Stopped(s) => Some(s.0),
            _ => None,
        }
    }
    fn continued(&self) -> bool {
        self.0 == WaitStatus::Continued
    }
    fn into_raw(self) -> i32 {
        match self.0 {
            WaitStatus::Exited(c) => (c & 0xff) << 8,
            WaitStatus::Signaled { signal, core_dumped } => signal.0 | if core_dumped { 0x80 } else { 0 },
            WaitStatus::Stopped(s) => 0x7f | (s.0 << 8),
            WaitStatus::Continued => 0xffff,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Lê a máscara de criação de arquivos sem mudá-la (o `umask(2)` só sabe trocar, então troca e
/// volta; a máscara é do pseudo-processo, não há corrida com outros processos).
pub fn get_umask() -> u32 {
    let sys = proc::sys();
    let old = sys.umask(0o022);
    sys.umask(old);
    old
}

/// `umask(2)`: troca e devolve a anterior.
pub fn set_umask(mask: u32) -> u32 {
    proc::sys().umask(mask)
}

/// Bytes de um `OsString` (atalho pros portes).
pub fn os_bytes(s: &OsStr) -> Vec<u8> {
    s.as_bytes().to_vec()
}

/// Constrói um `OsString` a partir de bytes.
pub fn os_from_bytes(b: Vec<u8>) -> OsString {
    OsString::from_vec(b)
}
