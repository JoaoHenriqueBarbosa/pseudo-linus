//! Backend falso pra testar o host sem o kernel (feature `fake-backend` e testes unitários).
//!
//! Não é o pseudo-linus: é um dublê com o comportamento observável que o host precisa exercitar
//! (pipes com capacidade, processos em threads que obedecem a sinais, grupos e sessões, sistema de
//! arquivos em memória com symlinks, snapshot por cópia) e um punhado de programas de teste:
//!
//! | programa | faz |
//! |---|---|
//! | `echo`, `cat`, `true`, `false`, `env`, `pwd`, `printenv` | o óbvio |
//! | `sleep N` | dorme N segundos (fração aceita), acorda com sinal |
//! | `spin` | laço de CPU que só para com sinal |
//! | `yes` | escreve `y\n` até levar SIGPIPE ou sinal |
//! | `bigout N` | escreve N bytes `x` |
//! | `errout MSG` | escreve MSG no stderr |
//! | `exit N` | sai com N |
//! | `bgsleep N` | deixa um filho dormindo N segundos com o stdout herdado e sai |
//! | `crash` | aborta o processo do host (simula a queda de um worker) |
//! | `bash`/`sh -c SCRIPT` | um mini shell: comandos acima por linha ou `;`, mais `cd`, `export`, `.`, `&` e o laço de sessão do host |
//!
//! O worker só aceita este backend com `PL_ALLOW_FAKE_BACKEND=1`.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};
use sysabi::{DirEntry, Errno, KillTarget, Mode, Pid, ProcInfo, Signal, Stat, TimeSpec, WaitStatus, mode};

use crate::api::SandboxUsage;
use crate::backend::*;

const PIPE_CAP: usize = 64 * 1024;
const TICK: Duration = Duration::from_millis(2);

// ---------------------------------------------------------------- pipes

struct PipeState {
    buf: VecDeque<u8>,
    readers: usize,
    writers: usize,
}

struct Pipe {
    st: Mutex<PipeState>,
    cv: Condvar,
}

impl Pipe {
    fn new() -> Arc<Pipe> {
        Arc::new(Pipe { st: Mutex::new(PipeState { buf: VecDeque::new(), readers: 1, writers: 1 }), cv: Condvar::new() })
    }
}

struct PipeR(Arc<Pipe>);
struct PipeW(Arc<Pipe>);

impl Drop for PipeR {
    fn drop(&mut self) {
        self.0.st.lock().readers -= 1;
        self.0.cv.notify_all();
    }
}

impl Drop for PipeW {
    fn drop(&mut self) {
        self.0.st.lock().writers -= 1;
        self.0.cv.notify_all();
    }
}

impl PipeW {
    fn dup(&self) -> PipeW {
        self.0.st.lock().writers += 1;
        PipeW(self.0.clone())
    }
}

impl PipeR {
    fn read(&self, buf: &mut [u8], timeout: Duration) -> ReadOutcome {
        let deadline = Instant::now() + timeout;
        let mut st = self.0.st.lock();
        loop {
            if !st.buf.is_empty() {
                let n = buf.len().min(st.buf.len());
                for (i, b) in st.buf.drain(..n).enumerate() {
                    buf[i] = b;
                }
                self.0.cv.notify_all();
                return ReadOutcome::Data(n);
            }
            if st.writers == 0 {
                return ReadOutcome::Eof;
            }
            if self.0.cv.wait_until(&mut st, deadline).timed_out() {
                return ReadOutcome::TimedOut;
            }
        }
    }
}

impl PipeW {
    fn write(&self, data: &[u8], timeout: Duration) -> WriteOutcome {
        let deadline = Instant::now() + timeout;
        let mut st = self.0.st.lock();
        loop {
            if st.readers == 0 {
                return WriteOutcome::Closed;
            }
            let room = PIPE_CAP - st.buf.len();
            if room > 0 {
                let n = room.min(data.len());
                st.buf.extend(&data[..n]);
                self.0.cv.notify_all();
                return WriteOutcome::Wrote(n);
            }
            if self.0.cv.wait_until(&mut st, deadline).timed_out() {
                return WriteOutcome::TimedOut;
            }
        }
    }
}

impl HostReader for PipeR {
    fn read_timeout(&mut self, buf: &mut [u8], timeout: Duration) -> ReadOutcome {
        self.read(buf, timeout)
    }
}

impl HostWriter for PipeW {
    fn write_timeout(&mut self, buf: &[u8], timeout: Duration) -> WriteOutcome {
        self.write(buf, timeout)
    }
}

// ---------------------------------------------------------------- sistema de arquivos

#[derive(Clone, Debug)]
enum Kind {
    File(Vec<u8>),
    Dir,
    Symlink(Vec<u8>),
    Fifo,
}

#[derive(Clone, Debug)]
struct Node {
    kind: Kind,
    mode: Mode,
    uid: u32,
    gid: u32,
    atime: TimeSpec,
    mtime: TimeSpec,
    ino: u64,
}

#[derive(Clone, Debug, Default)]
struct Fs {
    nodes: BTreeMap<Vec<u8>, Node>,
    next_ino: u64,
}

fn split(p: &[u8]) -> Vec<Vec<u8>> {
    p.split(|b| *b == b'/').filter(|c| !c.is_empty()).map(<[u8]>::to_vec).collect()
}

fn join(stack: &[Vec<u8>]) -> Vec<u8> {
    if stack.is_empty() {
        return b"/".to_vec();
    }
    let mut v = Vec::new();
    for c in stack {
        v.push(b'/');
        v.extend_from_slice(c);
    }
    v
}

fn now_ts() -> TimeSpec {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    TimeSpec::from_duration_since_epoch(d)
}

impl Fs {
    fn new() -> Fs {
        let mut fs = Fs { nodes: BTreeMap::new(), next_ino: 1 };
        fs.put(b"/".to_vec(), Kind::Dir, 0o755);
        for (d, m) in [
            ("/bin", 0o755),
            ("/usr", 0o755),
            ("/usr/bin", 0o755),
            ("/etc", 0o755),
            ("/tmp", 0o1777),
            ("/root", 0o700),
            ("/work", 0o755),
            ("/run", 0o755),
            ("/dev", 0o755),
        ] {
            fs.put(d.as_bytes().to_vec(), Kind::Dir, m);
        }
        for (name, _) in PROGRAMS {
            fs.put(format!("/usr/bin/{name}").into_bytes(), Kind::File(b"#!fake\n".to_vec()), 0o755);
            fs.put(format!("/bin/{name}").into_bytes(), Kind::File(b"#!fake\n".to_vec()), 0o755);
        }
        fs
    }

    fn put(&mut self, path: Vec<u8>, kind: Kind, perm: Mode) {
        let t = now_ts();
        let ino = self.next_ino;
        self.next_ino += 1;
        self.nodes.insert(path, Node { kind, mode: perm, uid: 0, gid: 0, atime: t, mtime: t, ino });
    }

    /// Resolve symlinks (intermediários sempre; o último se `follow`). Devolve o caminho canônico, que
    /// pode não existir (só o último componente).
    fn resolve(&self, path: &[u8], follow: bool) -> Result<Vec<u8>, Errno> {
        if !path.starts_with(b"/") {
            return Err(Errno::EINVAL);
        }
        let mut stack: Vec<Vec<u8>> = Vec::new();
        let mut todo: VecDeque<Vec<u8>> = split(path).into();
        let mut links = 0;
        while let Some(c) = todo.pop_front() {
            if c == b"." {
                continue;
            }
            if c == b".." {
                stack.pop();
                continue;
            }
            stack.push(c);
            let cur = join(&stack);
            let last = todo.is_empty();
            match self.nodes.get(&cur).map(|n| &n.kind) {
                None if last => return Ok(cur),
                None => return Err(Errno::ENOENT),
                Some(Kind::Symlink(t)) if !last || follow => {
                    links += 1;
                    if links > 40 {
                        return Err(Errno::ELOOP);
                    }
                    stack.pop();
                    if t.starts_with(b"/") {
                        stack.clear();
                    }
                    for comp in split(t).into_iter().rev() {
                        todo.push_front(comp);
                    }
                }
                Some(Kind::Dir) => {}
                Some(_) if !last => return Err(Errno::ENOTDIR),
                Some(_) => {}
            }
        }
        Ok(join(&stack))
    }

    fn get(&self, path: &[u8], follow: bool) -> Result<(Vec<u8>, &Node), Errno> {
        let p = self.resolve(path, follow)?;
        let n = self.nodes.get(&p).ok_or(Errno::ENOENT)?;
        Ok((p, n))
    }

    fn parent_ok(&self, canon: &[u8]) -> Result<(), Errno> {
        let comps = split(canon);
        if comps.is_empty() {
            return Err(Errno::EEXIST);
        }
        let parent = join(&comps[..comps.len() - 1]);
        match self.nodes.get(&parent).map(|n| &n.kind) {
            Some(Kind::Dir) => Ok(()),
            Some(_) => Err(Errno::ENOTDIR),
            None => Err(Errno::ENOENT),
        }
    }

    fn children(&self, dir: &[u8]) -> Vec<(Vec<u8>, &Node)> {
        let mut prefix = dir.to_vec();
        if !prefix.ends_with(b"/") {
            prefix.push(b'/');
        }
        self.nodes
            .range(prefix.clone()..)
            .take_while(|(k, _)| k.starts_with(&prefix))
            .filter(|(k, _)| !k[prefix.len()..].contains(&b'/') && k.len() > prefix.len())
            .map(|(k, n)| (k[prefix.len()..].to_vec(), n))
            .collect()
    }

    fn stat_of(n: &Node) -> Stat {
        let (ty, size) = match &n.kind {
            Kind::File(d) => (mode::S_IFREG, d.len() as u64),
            Kind::Dir => (mode::S_IFDIR, 4096),
            Kind::Symlink(t) => (mode::S_IFLNK, t.len() as u64),
            Kind::Fifo => (mode::S_IFIFO, 0),
        };
        Stat {
            dev: 1,
            ino: n.ino,
            mode: ty | n.mode,
            nlink: 1,
            uid: n.uid,
            gid: n.gid,
            size,
            blksize: 4096,
            blocks: size.div_ceil(512),
            atime: n.atime,
            mtime: n.mtime,
            ctime: n.mtime,
            ..Default::default()
        }
    }

    fn bytes(&self) -> u64 {
        self.nodes.values().map(|n| if let Kind::File(d) = &n.kind { d.len() as u64 } else { 0 }).sum()
    }
}

// ---------------------------------------------------------------- processos

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PState {
    Running,
    Exited(WaitStatus),
}

struct ProcEntry {
    pid: Pid,
    ppid: Pid,
    pgid: Pid,
    sid: Pid,
    comm: Vec<u8>,
    state: PState,
    signal: Arc<AtomicI32>,
}

struct SbInner {
    fs: Mutex<Fs>,
    procs: Mutex<BTreeMap<Pid, ProcEntry>>,
    exited: Condvar,
    next_pid: Mutex<Pid>,
    destroyed: AtomicBool,
    max_procs: u32,
    fs_limit: u64,
}

impl SbInner {
    fn alive(&self) -> BResult<()> {
        if self.destroyed.load(Ordering::Acquire) { Err(BackendError::Gone) } else { Ok(()) }
    }

    fn running(&self) -> u32 {
        self.procs.lock().values().filter(|p| p.state == PState::Running).count() as u32
    }
}

/// O ambiente de um programa falso.
pub struct Proc {
    pub argv: Vec<Vec<u8>>,
    pub env: Vec<Vec<u8>>,
    pub cwd: Vec<u8>,
    pid: Pid,
    pgid: Pid,
    sid: Pid,
    stdin: Option<PipeR>,
    stdout: PipeW,
    stderr: PipeW,
    sb: Arc<SbInner>,
    signal: Arc<AtomicI32>,
}

/// O processo foi morto por sinal (o programa desenrola com `?`).
pub struct Killed(pub Signal);

impl Proc {
    fn check(&self) -> Result<(), Killed> {
        match self.signal.load(Ordering::Acquire) {
            0 => Ok(()),
            s => Err(Killed(Signal(s))),
        }
    }

    fn write_to(&self, w: &PipeW, mut data: &[u8]) -> Result<(), Killed> {
        while !data.is_empty() {
            self.check()?;
            match w.write(data, TICK) {
                WriteOutcome::Wrote(n) => data = &data[n..],
                WriteOutcome::Closed => {
                    self.signal.store(Signal::SIGPIPE.0, Ordering::Release);
                    return Err(Killed(Signal::SIGPIPE));
                }
                WriteOutcome::TimedOut => {}
            }
        }
        Ok(())
    }

    pub fn out(&self, data: &[u8]) -> Result<(), Killed> {
        self.write_to(&self.stdout, data)
    }

    pub fn err(&self, data: &[u8]) -> Result<(), Killed> {
        self.write_to(&self.stderr, data)
    }

    /// Lê o stdin até o fim.
    pub fn read_all(&self) -> Result<Vec<u8>, Killed> {
        let mut v = Vec::new();
        let Some(r) = &self.stdin else { return Ok(v) };
        let mut buf = [0u8; 8192];
        loop {
            self.check()?;
            match r.read(&mut buf, TICK) {
                ReadOutcome::Data(n) => v.extend_from_slice(&buf[..n]),
                ReadOutcome::Eof => return Ok(v),
                ReadOutcome::TimedOut => {}
            }
        }
    }

    /// Lê uma linha do stdin (sem o `\n`); `None` no fim.
    pub fn read_line(&self) -> Result<Option<Vec<u8>>, Killed> {
        let mut v = Vec::new();
        let Some(r) = &self.stdin else { return Ok(None) };
        let mut b = [0u8; 1];
        loop {
            self.check()?;
            match r.read(&mut b, TICK) {
                ReadOutcome::Data(_) if b[0] == b'\n' => return Ok(Some(v)),
                ReadOutcome::Data(_) => v.push(b[0]),
                ReadOutcome::Eof => return Ok(if v.is_empty() { None } else { Some(v) }),
                ReadOutcome::TimedOut => {}
            }
        }
    }

    pub fn sleep(&self, d: Duration) -> Result<(), Killed> {
        let until = Instant::now() + d;
        while Instant::now() < until {
            self.check()?;
            std::thread::sleep(TICK.min(until.saturating_duration_since(Instant::now())));
        }
        Ok(())
    }

    pub fn getenv(&self, k: &[u8]) -> Option<Vec<u8>> {
        self.env.iter().rev().find_map(|e| {
            let (n, v) = e.split_at(e.iter().position(|b| *b == b'=')?);
            (n == k).then(|| v[1..].to_vec())
        })
    }

    pub fn setenv(&mut self, k: &[u8], v: &[u8]) {
        self.env.retain(|e| !(e.starts_with(k) && e.get(k.len()) == Some(&b'=')));
        let mut e = k.to_vec();
        e.push(b'=');
        e.extend_from_slice(v);
        self.env.push(e);
    }

    fn abs(&self, p: &[u8]) -> Vec<u8> {
        join_path(&self.cwd, p)
    }

    pub fn read_file(&self, p: &[u8]) -> Result<Vec<u8>, Errno> {
        let fs = self.sb.fs.lock();
        match &fs.get(&self.abs(p), true)?.1.kind {
            Kind::File(d) => Ok(d.clone()),
            Kind::Dir => Err(Errno::EISDIR),
            _ => Err(Errno::EINVAL),
        }
    }

    pub fn write_file(&self, p: &[u8], data: &[u8]) -> Result<(), Errno> {
        let mut fs = self.sb.fs.lock();
        let canon = fs.resolve(&self.abs(p), true)?;
        fs.parent_ok(&canon)?;
        fs.put(canon, Kind::File(data.to_vec()), 0o644);
        Ok(())
    }

    fn chdir(&mut self, p: &[u8]) -> Result<(), Errno> {
        let fs = self.sb.fs.lock();
        let (canon, n) = fs.get(&self.abs(p), true)?;
        if !matches!(n.kind, Kind::Dir) {
            return Err(Errno::ENOTDIR);
        }
        drop(fs);
        self.cwd = canon;
        Ok(())
    }

    /// Filho em segundo plano no mesmo grupo e sessão, com stdout e stderr herdados.
    fn spawn_child(&self, argv: Vec<Vec<u8>>) -> Result<Pid, Errno> {
        if self.sb.running() >= self.sb.max_procs {
            return Err(Errno::EAGAIN);
        }
        let prog = lookup(&argv[0]).ok_or(Errno::ENOENT)?;
        let pid = new_pid(&self.sb);
        let signal = Arc::new(AtomicI32::new(0));
        let child = Proc {
            argv: argv.clone(),
            env: self.env.clone(),
            cwd: self.cwd.clone(),
            pid,
            pgid: self.pgid,
            sid: self.sid,
            stdin: None,
            stdout: self.stdout.dup(),
            stderr: self.stderr.dup(),
            sb: self.sb.clone(),
            signal: signal.clone(),
        };
        register(&self.sb, pid, self.pid, self.pgid, self.sid, &argv[0], signal);
        start(child, prog);
        Ok(pid)
    }
}

type Main = fn(&mut Proc) -> Result<i32, Killed>;

const PROGRAMS: &[(&str, Main)] = &[
    ("echo", p_echo),
    ("cat", p_cat),
    ("true", |_| Ok(0)),
    ("false", |_| Ok(1)),
    ("env", p_env),
    ("pwd", p_pwd),
    ("printenv", p_printenv),
    ("sleep", p_sleep),
    ("spin", p_spin),
    ("yes", p_yes),
    ("bigout", p_bigout),
    ("errout", p_errout),
    ("exit", p_exit),
    ("bgsleep", p_bgsleep),
    ("crash", p_crash),
    ("bash", p_sh),
    ("sh", p_sh),
];

fn lookup(path: &[u8]) -> Option<Main> {
    let base = path.rsplit(|b| *b == b'/').next().unwrap_or(path);
    PROGRAMS.iter().find(|(n, _)| n.as_bytes() == base).map(|(_, m)| *m)
}

fn arg_str(a: &[u8]) -> String {
    String::from_utf8_lossy(a).into_owned()
}

fn p_echo(p: &mut Proc) -> Result<i32, Killed> {
    let mut line = p.argv[1..].join(&b' ');
    line.push(b'\n');
    p.out(&line)?;
    Ok(0)
}

fn p_cat(p: &mut Proc) -> Result<i32, Killed> {
    if p.argv.len() == 1 {
        let data = p.read_all()?;
        p.out(&data)?;
        return Ok(0);
    }
    let mut rc = 0;
    let files = p.argv[1..].to_owned();
    for f in &files {
        match p.read_file(f) {
            Ok(d) => p.out(&d)?,
            Err(e) => {
                p.err(format!("cat: {}: {}\n", arg_str(f), e.message()).as_bytes())?;
                rc = 1;
            }
        }
    }
    Ok(rc)
}

fn p_env(p: &mut Proc) -> Result<i32, Killed> {
    let mut v = Vec::new();
    for e in &p.env {
        v.extend_from_slice(e);
        v.push(b'\n');
    }
    p.out(&v)?;
    Ok(0)
}

fn p_pwd(p: &mut Proc) -> Result<i32, Killed> {
    let mut v = p.cwd.clone();
    v.push(b'\n');
    p.out(&v)?;
    Ok(0)
}

fn p_printenv(p: &mut Proc) -> Result<i32, Killed> {
    let Some(k) = p.argv.get(1).cloned() else { return p_env(p) };
    match p.getenv(&k) {
        Some(mut v) => {
            v.push(b'\n');
            p.out(&v)?;
            Ok(0)
        }
        None => Ok(1),
    }
}

fn p_sleep(p: &mut Proc) -> Result<i32, Killed> {
    let secs: f64 = p.argv.get(1).and_then(|a| arg_str(a).parse().ok()).unwrap_or(1.0);
    p.sleep(Duration::from_secs_f64(secs))?;
    Ok(0)
}

fn p_spin(p: &mut Proc) -> Result<i32, Killed> {
    let mut x: u64 = 0;
    loop {
        for _ in 0..10_000 {
            x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        }
        std::hint::black_box(x);
        p.check()?;
    }
}

fn p_yes(p: &mut Proc) -> Result<i32, Killed> {
    let chunk = b"y\n".repeat(4096);
    loop {
        p.out(&chunk)?;
    }
}

fn p_bigout(p: &mut Proc) -> Result<i32, Killed> {
    let mut n: u64 = p.argv.get(1).and_then(|a| arg_str(a).parse().ok()).unwrap_or(0);
    let chunk = vec![b'x'; 65536];
    while n > 0 {
        let k = n.min(chunk.len() as u64) as usize;
        p.out(&chunk[..k])?;
        n -= k as u64;
    }
    Ok(0)
}

fn p_errout(p: &mut Proc) -> Result<i32, Killed> {
    let mut line = p.argv[1..].join(&b' ');
    line.push(b'\n');
    p.err(&line)?;
    Ok(0)
}

fn p_exit(p: &mut Proc) -> Result<i32, Killed> {
    Ok(p.argv.get(1).and_then(|a| arg_str(a).parse().ok()).unwrap_or(0))
}

fn p_bgsleep(p: &mut Proc) -> Result<i32, Killed> {
    let secs = p.argv.get(1).cloned().unwrap_or_else(|| b"5".to_vec());
    let _ = p.spawn_child(vec![b"sleep".to_vec(), secs]);
    Ok(0)
}

fn p_crash(_p: &mut Proc) -> Result<i32, Killed> {
    // Simula um stack overflow que escapou do stacker: o processo do host inteiro cai.
    std::process::abort()
}

/// Divide uma linha em palavras, com aspas simples e duplas simples (sem expansão além de `$VAR`).
fn words(p: &Proc, line: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cur = Vec::new();
    let mut has = false;
    let mut i = 0;
    let expand = |cur: &mut Vec<u8>, line: &[u8], i: &mut usize| {
        let start = *i + 1;
        let mut j = start;
        if line.get(j) == Some(&b'?') {
            *i = j + 1;
            cur.extend_from_slice(p.getenv(b"?").unwrap_or_default().as_slice());
            return;
        }
        while j < line.len() && (line[j].is_ascii_alphanumeric() || line[j] == b'_') {
            j += 1;
        }
        if j == start {
            cur.push(b'$');
            *i = start;
            return;
        }
        cur.extend_from_slice(&p.getenv(&line[start..j]).unwrap_or_default());
        *i = j;
    };
    while i < line.len() {
        let c = line[i];
        match c {
            b' ' | b'\t' => {
                if has {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
                i += 1;
            }
            b'\'' => {
                has = true;
                i += 1;
                while i < line.len() && line[i] != b'\'' {
                    cur.push(line[i]);
                    i += 1;
                }
                i += 1;
            }
            b'"' => {
                has = true;
                i += 1;
                while i < line.len() && line[i] != b'"' {
                    if line[i] == b'$' {
                        expand(&mut cur, line, &mut i);
                    } else {
                        cur.push(line[i]);
                        i += 1;
                    }
                }
                i += 1;
            }
            b'$' => {
                has = true;
                expand(&mut cur, line, &mut i);
            }
            _ => {
                has = true;
                cur.push(c);
                i += 1;
            }
        }
    }
    if has {
        out.push(cur);
    }
    out
}

/// Roda um script do mini shell. `Ok(Some(code))` = `exit`.
fn run_script(p: &mut Proc, script: &[u8], last: &mut i32) -> Result<Option<i32>, Killed> {
    for raw in script.split(|b| *b == b'\n' || *b == b';') {
        let line = raw.trim_ascii();
        if line.is_empty() || line.starts_with(b"#") {
            continue;
        }
        let (line, background) = match line.strip_suffix(b"&") {
            Some(l) => (l.trim_ascii(), true),
            None => (line, false),
        };
        let w = words(p, line);
        if w.is_empty() {
            continue;
        }
        let rc = match w[0].as_slice() {
            b"cd" => {
                let target = w.get(1).cloned().or_else(|| p.getenv(b"HOME")).unwrap_or_else(|| b"/".to_vec());
                match p.chdir(&target) {
                    Ok(()) => {
                        let cwd = p.cwd.clone();
                        p.setenv(b"PWD", &cwd);
                        0
                    }
                    Err(e) => {
                        p.err(format!("bash: cd: {}: {}\n", arg_str(&target), e.message()).as_bytes())?;
                        1
                    }
                }
            }
            b"export" => {
                for a in &w[1..] {
                    if let Some(eq) = a.iter().position(|b| *b == b'=') {
                        let (k, v) = (a[..eq].to_vec(), a[eq + 1..].to_vec());
                        p.setenv(&k, &v);
                    }
                }
                0
            }
            b"exit" => return Ok(Some(w.get(1).and_then(|a| arg_str(a).parse().ok()).unwrap_or(*last))),
            b"." | b"source" => match w.get(1).map(|f| p.read_file(f)) {
                Some(Ok(text)) => {
                    if let Some(code) = run_script(p, &text, last)? {
                        return Ok(Some(code));
                    }
                    *last
                }
                Some(Err(e)) => {
                    p.err(format!("bash: {}: {}\n", arg_str(&w[1]), e.message()).as_bytes())?;
                    1
                }
                None => 2,
            },
            _ => {
                let Some(prog) = lookup(&w[0]) else {
                    p.err(format!("bash: {}: command not found\n", arg_str(&w[0])).as_bytes())?;
                    *last = 127;
                    continue;
                };
                if background {
                    let _ = p.spawn_child(w.clone());
                    0
                } else {
                    let saved = std::mem::replace(&mut p.argv, w.clone());
                    let r = prog(p);
                    p.argv = saved;
                    r?
                }
            }
        };
        *last = rc;
        let s = rc.to_string().into_bytes();
        p.setenv(b"?", &s);
    }
    Ok(None)
}

fn p_sh(p: &mut Proc) -> Result<i32, Killed> {
    let script = match (p.argv.get(1).map(Vec::as_slice), p.argv.get(2)) {
        (Some(b"-c"), Some(s)) => s.clone(),
        (Some(file), _) if !file.starts_with(b"-") => match p.read_file(file) {
            Ok(s) => s,
            Err(e) => {
                p.err(format!("bash: {}: {}\n", arg_str(file), e.message()).as_bytes())?;
                return Ok(127);
            }
        },
        _ => {
            // Script pelo stdin: a primeira linha decide se é o laço de sessão do host, que manda
            // o laço numa linha só e depois os ids pelo mesmo pipe.
            let Some(first) = p.read_line()? else { return Ok(0) };
            if first.starts_with(b"__osh_dir='") {
                first
            } else {
                let mut s = first;
                s.push(b'\n');
                s.extend(p.read_all()?);
                s
            }
        }
    };
    if let Some(rest) = script.strip_prefix(b"__osh_dir='") {
        let end = rest.iter().position(|b| *b == b'\'').unwrap_or(rest.len());
        return session_loop(p, rest[..end].to_vec());
    }
    let mut last = 0;
    Ok(run_script(p, &script, &mut last)?.unwrap_or(last))
}

/// O laço de sessão do host ([`crate::session`]), feito à mão.
fn session_loop(p: &mut Proc, dir: Vec<u8>) -> Result<i32, Killed> {
    let mut last = 0;
    while let Some(id) = p.read_line()? {
        let id = arg_str(&id);
        let base = format!("{}/{id}", arg_str(&dir));
        let script = p.read_file(format!("{base}.sh").as_bytes()).unwrap_or_default();
        let input = p.read_file(format!("{base}.in").as_bytes()).unwrap_or_default();
        let saved = p.stdin.take();
        let pipe = Pipe::new();
        let (r, w) = (PipeR(pipe.clone()), PipeW(pipe));
        w.write(&input, Duration::from_secs(1));
        drop(w);
        p.stdin = Some(r);
        let res = run_script(p, &script, &mut last);
        p.stdin = saved;
        if let Some(code) = res? {
            return Ok(code);
        }
        let mut state = p.cwd.clone();
        state.push(0);
        for e in &p.env {
            if !e.starts_with(b"?=") {
                state.extend_from_slice(e);
                state.push(0);
            }
        }
        let _ = p.write_file(format!("{base}.state").as_bytes(), &state);
        p.out(format!("\x1eOSH-END {id} {last}\x1e\n").as_bytes())?;
        p.err(format!("\x1eOSH-END {id}\x1e\n").as_bytes())?;
    }
    Ok(last)
}

fn new_pid(sb: &SbInner) -> Pid {
    let mut n = sb.next_pid.lock();
    *n += 1;
    *n
}

fn register(sb: &SbInner, pid: Pid, ppid: Pid, pgid: Pid, sid: Pid, argv0: &[u8], signal: Arc<AtomicI32>) {
    let comm = argv0.rsplit(|b| *b == b'/').next().unwrap_or(argv0).to_vec();
    sb.procs.lock().insert(pid, ProcEntry { pid, ppid, pgid, sid, comm, state: PState::Running, signal });
}

fn start(mut proc: Proc, main: Main) {
    let sb = proc.sb.clone();
    let pid = proc.pid;
    let child = proc.pgid != pid;
    std::thread::Builder::new()
        .name(format!("fake-{pid}"))
        .spawn(move || {
            let status = match main(&mut proc) {
                Ok(code) => WaitStatus::Exited(code & 0xff),
                Err(Killed(sig)) => WaitStatus::Signaled { signal: sig, core_dumped: false },
            };
            drop(proc);
            let mut procs = sb.procs.lock();
            if child {
                procs.remove(&pid);
            } else if let Some(e) = procs.get_mut(&pid) {
                e.state = PState::Exited(status);
            }
            drop(procs);
            sb.exited.notify_all();
        })
        .expect("thread do processo falso");
}

struct Waiter {
    sb: Arc<SbInner>,
    pid: Pid,
}

impl ExitWaiter for Waiter {
    fn wait_timeout(&mut self, timeout: Duration) -> Option<ExitInfo> {
        let deadline = Instant::now() + timeout;
        let mut procs = self.sb.procs.lock();
        loop {
            match procs.get(&self.pid).map(|e| e.state) {
                Some(PState::Exited(status)) => {
                    procs.remove(&self.pid);
                    return Some(ExitInfo { status, cpu_ns: 0 });
                }
                None => return Some(ExitInfo { status: WaitStatus::Signaled { signal: Signal::SIGKILL, core_dumped: false }, cpu_ns: 0 }),
                Some(PState::Running) => {}
            }
            if self.sb.exited.wait_until(&mut procs, deadline).timed_out() {
                return None;
            }
        }
    }
}

/// Uma sandbox falsa.
pub struct FakeSandbox {
    inner: Arc<SbInner>,
}

impl FakeSandbox {
    fn fs_op<R>(&self, f: impl FnOnce(&mut Fs) -> Result<R, Errno>, ctx: &[u8]) -> BResult<R> {
        self.inner.alive()?;
        let mut fs = self.inner.fs.lock();
        f(&mut fs).map_err(|e| BackendError::os(e, arg_str(ctx)))
    }
}

impl Sandbox for FakeSandbox {
    fn spawn(&self, req: SpawnRequest) -> BResult<Spawned> {
        self.inner.alive()?;
        if self.inner.running() >= self.inner.max_procs {
            return Err(BackendError::Limit(format!("limite de processos da sandbox ({})", self.inner.max_procs)));
        }
        let main = {
            let fs = self.inner.fs.lock();
            let (_, n) = fs.get(&req.path, true).map_err(|e| BackendError::os(e, arg_str(&req.path)))?;
            if !matches!(n.kind, Kind::File(_)) || n.mode & 0o111 == 0 {
                return Err(BackendError::os(Errno::EACCES, arg_str(&req.path)));
            }
            lookup(&req.path).ok_or_else(|| BackendError::os(Errno::ENOEXEC, arg_str(&req.path)))?
        };
        {
            let fs = self.inner.fs.lock();
            match fs.get(&req.cwd, true) {
                Ok((_, n)) if matches!(n.kind, Kind::Dir) => {}
                Ok(_) => return Err(BackendError::os(Errno::ENOTDIR, arg_str(&req.cwd))),
                Err(e) => return Err(BackendError::os(e, arg_str(&req.cwd))),
            }
        }
        let pid = new_pid(&self.inner);
        let (pin, pout, perr) = (Pipe::new(), Pipe::new(), Pipe::new());
        let signal = Arc::new(AtomicI32::new(0));
        let proc = Proc {
            argv: req.argv.clone(),
            env: req.env,
            cwd: req.cwd,
            pid,
            pgid: pid,
            sid: pid,
            stdin: Some(PipeR(pin.clone())),
            stdout: PipeW(pout.clone()),
            stderr: PipeW(perr.clone()),
            sb: self.inner.clone(),
            signal: signal.clone(),
        };
        register(&self.inner, pid, 1, pid, pid, req.argv.first().map(Vec::as_slice).unwrap_or(b"?"), signal);
        start(proc, main);
        Ok(Spawned {
            pid,
            stdin: Box::new(PipeW(pin)),
            stdout: Box::new(PipeR(pout)),
            stderr: Box::new(PipeR(perr)),
            exit: Box::new(Waiter { sb: self.inner.clone(), pid }),
        })
    }

    fn kill(&self, target: KillTarget, sig: Signal) -> BResult<()> {
        self.inner.alive()?;
        let procs = self.inner.procs.lock();
        let mut hit = false;
        for e in procs.values().filter(|e| e.state == PState::Running) {
            let m = match target {
                KillTarget::Pid(p) => e.pid == p,
                KillTarget::Group(g) => e.pgid == g,
                KillTarget::All => true,
            };
            if m {
                hit = true;
                if sig.0 != 0 {
                    e.signal.store(sig.0, Ordering::Release);
                }
            }
        }
        if hit { Ok(()) } else { Err(BackendError::os(Errno::ESRCH, "")) }
    }

    fn processes(&self) -> Vec<ProcInfo> {
        self.inner
            .procs
            .lock()
            .values()
            .map(|e| ProcInfo {
                pid: e.pid,
                ppid: e.ppid,
                pgid: e.pgid,
                sid: e.sid,
                state: if e.state == PState::Running { 'S' } else { 'Z' },
                comm: e.comm.clone(),
            })
            .collect()
    }

    fn stat(&self, path: &[u8], follow: bool) -> BResult<Stat> {
        self.fs_op(|fs| fs.get(path, follow).map(|(_, n)| Fs::stat_of(n)), path)
    }

    fn read_dir(&self, path: &[u8]) -> BResult<Vec<DirEntry>> {
        self.fs_op(
            |fs| {
                let (canon, n) = fs.get(path, true)?;
                if !matches!(n.kind, Kind::Dir) {
                    return Err(Errno::ENOTDIR);
                }
                Ok(fs
                    .children(&canon)
                    .into_iter()
                    .map(|(name, n)| DirEntry { ino: n.ino, kind: Fs::stat_of(n).file_type(), name })
                    .collect())
            },
            path,
        )
    }

    fn read_file(&self, path: &[u8], offset: u64, max: usize) -> BResult<Vec<u8>> {
        self.fs_op(
            |fs| match &fs.get(path, true)?.1.kind {
                Kind::File(d) => {
                    let start = (offset as usize).min(d.len());
                    Ok(d[start..(start + max).min(d.len())].to_vec())
                }
                Kind::Dir => Err(Errno::EISDIR),
                _ => Err(Errno::EINVAL),
            },
            path,
        )
    }

    fn write_file(&self, path: &[u8], data: &[u8], opts: WriteOpts) -> BResult<()> {
        let limit = self.inner.fs_limit;
        self.fs_op(
            |fs| {
                let canon = fs.resolve(path, true)?;
                let existing = fs.nodes.get(&canon).map(|n| n.kind.clone());
                let old_len = match &existing {
                    Some(Kind::File(d)) => d.len() as u64,
                    _ => 0,
                };
                let new_len = if opts.append { old_len + data.len() as u64 } else { data.len() as u64 };
                if fs.bytes() - old_len + new_len > limit {
                    return Err(Errno::ENOSPC);
                }
                match existing {
                    Some(_) if opts.exclusive => Err(Errno::EEXIST),
                    Some(Kind::Dir) => Err(Errno::EISDIR),
                    Some(Kind::File(_)) => {
                        let n = fs.nodes.get_mut(&canon).expect("existe");
                        if let Kind::File(d) = &mut n.kind {
                            if !opts.append {
                                d.clear();
                            }
                            d.extend_from_slice(data);
                        }
                        n.mtime = now_ts();
                        Ok(())
                    }
                    Some(_) => Err(Errno::EINVAL),
                    None => {
                        fs.parent_ok(&canon)?;
                        fs.put(canon, Kind::File(data.to_vec()), opts.mode & 0o7777);
                        Ok(())
                    }
                }
            },
            path,
        )
    }

    fn mkdir(&self, path: &[u8], m: Mode) -> BResult<()> {
        self.fs_op(
            |fs| {
                let canon = fs.resolve(path, false)?;
                if fs.nodes.contains_key(&canon) {
                    return Err(Errno::EEXIST);
                }
                fs.parent_ok(&canon)?;
                fs.put(canon, Kind::Dir, m & 0o7777);
                Ok(())
            },
            path,
        )
    }

    fn unlink(&self, path: &[u8]) -> BResult<()> {
        self.fs_op(
            |fs| {
                let canon = fs.resolve(path, false)?;
                match fs.nodes.get(&canon).map(|n| &n.kind) {
                    None => Err(Errno::ENOENT),
                    Some(Kind::Dir) => Err(Errno::EISDIR),
                    Some(_) => {
                        fs.nodes.remove(&canon);
                        Ok(())
                    }
                }
            },
            path,
        )
    }

    fn rmdir(&self, path: &[u8]) -> BResult<()> {
        self.fs_op(
            |fs| {
                let canon = fs.resolve(path, false)?;
                match fs.nodes.get(&canon).map(|n| &n.kind) {
                    None => Err(Errno::ENOENT),
                    Some(Kind::Dir) if canon == b"/" => Err(Errno::EBUSY),
                    Some(Kind::Dir) if !fs.children(&canon).is_empty() => Err(Errno::ENOTEMPTY),
                    Some(Kind::Dir) => {
                        fs.nodes.remove(&canon);
                        Ok(())
                    }
                    Some(_) => Err(Errno::ENOTDIR),
                }
            },
            path,
        )
    }

    fn symlink(&self, target: &[u8], path: &[u8]) -> BResult<()> {
        self.fs_op(
            |fs| {
                let canon = fs.resolve(path, false)?;
                if fs.nodes.contains_key(&canon) {
                    return Err(Errno::EEXIST);
                }
                fs.parent_ok(&canon)?;
                fs.put(canon, Kind::Symlink(target.to_vec()), 0o777);
                Ok(())
            },
            path,
        )
    }

    fn readlink(&self, path: &[u8]) -> BResult<Vec<u8>> {
        self.fs_op(
            |fs| match &fs.get(path, false)?.1.kind {
                Kind::Symlink(t) => Ok(t.clone()),
                _ => Err(Errno::EINVAL),
            },
            path,
        )
    }

    fn link(&self, existing: &[u8], new: &[u8]) -> BResult<()> {
        self.fs_op(
            |fs| {
                let (_, n) = fs.get(existing, false)?;
                let n = n.clone();
                let canon = fs.resolve(new, false)?;
                if fs.nodes.contains_key(&canon) {
                    return Err(Errno::EEXIST);
                }
                fs.parent_ok(&canon)?;
                fs.nodes.insert(canon, n);
                Ok(())
            },
            new,
        )
    }

    fn mknod(&self, path: &[u8], m: Mode, _dev: u64) -> BResult<()> {
        if m & mode::S_IFMT != mode::S_IFIFO {
            return Err(BackendError::os(Errno::EPERM, arg_str(path)));
        }
        self.fs_op(
            |fs| {
                let canon = fs.resolve(path, false)?;
                if fs.nodes.contains_key(&canon) {
                    return Err(Errno::EEXIST);
                }
                fs.parent_ok(&canon)?;
                fs.put(canon, Kind::Fifo, m & 0o7777);
                Ok(())
            },
            path,
        )
    }

    fn chmod(&self, path: &[u8], m: Mode) -> BResult<()> {
        self.fs_op(
            |fs| {
                let canon = fs.resolve(path, true)?;
                let n = fs.nodes.get_mut(&canon).ok_or(Errno::ENOENT)?;
                n.mode = m & 0o7777;
                Ok(())
            },
            path,
        )
    }

    fn chown(&self, path: &[u8], uid: u32, gid: u32, follow: bool) -> BResult<()> {
        self.fs_op(
            |fs| {
                let canon = fs.resolve(path, follow)?;
                let n = fs.nodes.get_mut(&canon).ok_or(Errno::ENOENT)?;
                n.uid = uid;
                n.gid = gid;
                Ok(())
            },
            path,
        )
    }

    fn set_times(&self, path: &[u8], atime: TimeSpec, mtime: TimeSpec, follow: bool) -> BResult<()> {
        self.fs_op(
            |fs| {
                let canon = fs.resolve(path, follow)?;
                let n = fs.nodes.get_mut(&canon).ok_or(Errno::ENOENT)?;
                n.atime = atime;
                n.mtime = mtime;
                Ok(())
            },
            path,
        )
    }

    fn snapshot(&self) -> BResult<SnapshotToken> {
        self.inner.alive()?;
        Ok(Arc::new(self.inner.fs.lock().clone()))
    }

    fn restore(&self, snap: &SnapshotToken) -> BResult<()> {
        self.inner.alive()?;
        let fs = snap.downcast_ref::<Fs>().ok_or_else(|| BackendError::Internal("snapshot de outro backend".into()))?;
        *self.inner.fs.lock() = fs.clone();
        Ok(())
    }

    fn usage(&self) -> SandboxUsage {
        SandboxUsage { procs: self.inner.running(), mem_bytes: 0, fs_bytes: self.inner.fs.lock().bytes(), cpu_ns: 0 }
    }

    fn destroy(&self) {
        self.inner.destroyed.store(true, Ordering::Release);
        for e in self.inner.procs.lock().values() {
            e.signal.store(Signal::SIGKILL.0, Ordering::Release);
        }
    }
}

/// O backend falso.
#[derive(Default)]
pub struct FakeBackend {
    users: Mutex<BTreeMap<String, UserSched>>,
}

impl FakeBackend {

    pub fn user(&self, name: &str) -> Option<UserSched> {
        self.users.lock().get(name).cloned()
    }
}

impl Backend for FakeBackend {
    fn info(&self) -> BackendInfo {
        BackendInfo { name: "fake".into(), cpus: 1, programs: PROGRAMS.len(), isolation: "nenhum (backend falso)".into() }
    }

    fn ensure_user(&self, user: &UserSched) -> BResult<()> {
        self.users.lock().insert(user.user.clone(), user.clone());
        Ok(())
    }

    fn create_sandbox(&self, _id: &str, _user: &str, spec: &SandboxSpec) -> BResult<Arc<dyn Sandbox>> {
        if spec.image != "default" {
            return Err(BackendError::Internal(format!("imagem desconhecida: {}", spec.image)));
        }
        Ok(Arc::new(FakeSandbox {
            inner: Arc::new(SbInner {
                fs: Mutex::new(Fs::new()),
                procs: Mutex::new(BTreeMap::new()),
                exited: Condvar::new(),
                next_pid: Mutex::new(1),
                destroyed: AtomicBool::new(false),
                max_procs: spec.limits.max_procs,
                fs_limit: spec.limits.fs_bytes,
            }),
        }))
    }
}

#[cfg(test)]
pub(crate) fn test_sandbox() -> Arc<dyn Sandbox> {
    use crate::api::SandboxLimits;
    FakeBackend::default()
        .create_sandbox(
            "sb_test",
            "tester",
            &SandboxSpec {
                image: "default".into(),
                hostname: "pl".into(),
                limits: SandboxLimits {
                    mem_bytes: 1 << 28,
                    max_procs: 16,
                    fs_bytes: 1 << 24,
                    nofile: 1024,
                    cpu_weight: 100,
                    cpu_max: None,
                },
            },
        )
        .expect("sandbox falsa")
}
