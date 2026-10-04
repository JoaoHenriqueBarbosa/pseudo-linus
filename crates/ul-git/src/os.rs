//! Tudo que o git faz com o sistema passa por aqui, sobre `sysabi`: arquivos, diretórios, lockfiles,
//! ambiente, relógio e os fluxos padrão.
//!
//! O stdout tem buffer de 4096 bytes e só desce no fim (ou quando enche), como o stdio da glibc num
//! pipe; o stderr é sem buffer. Isso reproduz a ordem que o git real produz quando os dois vão pro
//! mesmo lugar (`2>&1`).

use std::cell::RefCell;
use std::sync::Arc;

use sysabi::{AtFlags, Clock, DirEntry, Errno, Fd, FileType, Mode, OFlags, RenameFlags, SetTime, Stat, Syscalls, sys};

/// Tamanho do buffer do stdout (o `st_blksize` de um pipe).
const STDOUT_BUF: usize = 4096;

thread_local! {
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

pub fn sysc() -> Arc<dyn Syscalls> {
    sys::current()
}

// ---- fluxos padrão ----------------------------------------------------------------------------

/// Acrescenta ao stdout (com buffer).
pub fn out(data: &[u8]) {
    let full = OUT.with(|o| {
        let mut o = o.borrow_mut();
        o.extend_from_slice(data);
        if o.len() >= STDOUT_BUF {
            let n = o.len() / STDOUT_BUF * STDOUT_BUF;
            Some(o.drain(..n).collect::<Vec<u8>>())
        } else {
            None
        }
    });
    if let Some(chunk) = full {
        let _ = sys::write_all(Fd::STDOUT, &chunk);
    }
}

pub fn outs(s: &str) {
    out(s.as_bytes());
}

/// Esvazia o buffer do stdout (o `fflush(stdout)` do git).
pub fn flush_out() {
    let data = OUT.with(|o| std::mem::take(&mut *o.borrow_mut()));
    if !data.is_empty() {
        let _ = sys::write_all(Fd::STDOUT, &data);
    }
}

/// Troca o buffer do stdout (o testkit roda processos filhos na mesma thread).
pub fn swap_out(buf: Vec<u8>) -> Vec<u8> {
    OUT.with(|o| std::mem::replace(&mut *o.borrow_mut(), buf))
}

/// Escreve direto no stderr.
pub fn err_bytes(data: &[u8]) {
    let _ = sys::write_all(Fd::STDERR, data);
}

/// `prefix` + `msg` + `\n` no stderr, com caracteres de controle trocados por `?` como o `vreportf`.
pub fn err_line(prefix: &str, msg: &str) {
    let mut line = String::with_capacity(prefix.len() + msg.len() + 1);
    line.push_str(prefix);
    for c in msg.chars() {
        if c.is_ascii_control() && c != '\t' && c != '\n' {
            line.push('?');
        } else {
            line.push(c);
        }
    }
    line.push('\n');
    err_bytes(line.as_bytes());
}

pub fn errs(s: &str) {
    err_bytes(s.as_bytes());
}

pub fn stdin_all() -> Vec<u8> {
    sys::read_to_end(Fd::STDIN).unwrap_or_default()
}

// ---- ambiente, relógio, identidade ------------------------------------------------------------

pub fn getenv(name: &str) -> Option<Vec<u8>> {
    // Fora de um pseudo-processo (testes de unidade) não há ambiente.
    sys::try_current()?.getenv(name.as_bytes())
}

pub fn getenv_str(name: &str) -> Option<String> {
    getenv(name).map(|v| String::from_utf8_lossy(&v).into_owned())
}

pub fn setenv(name: &str, value: &[u8]) {
    let _ = sysc().setenv(name.as_bytes(), value);
}

pub fn unsetenv(name: &str) {
    let _ = sysc().unsetenv(name.as_bytes());
}

pub fn environ() -> Vec<Vec<u8>> {
    sysc().environ()
}

/// Segundos desde a época, pelo relógio do sandbox.
pub fn now() -> i64 {
    match sys::try_current() {
        Some(s) => s.clock_gettime(Clock::Realtime).map(|t| t.sec).unwrap_or(0),
        None => 0,
    }
}

pub fn hostname() -> Vec<u8> {
    sysc().uname().nodename
}

pub fn getuid() -> u32 {
    sysc().getuid()
}

pub fn getpid() -> i32 {
    sysc().getpid()
}

pub fn umask() -> Mode {
    let s = sysc();
    let m = s.umask(0o022);
    s.umask(m);
    m
}

// ---- caminhos ---------------------------------------------------------------------------------

/// `a/b`, sem barra dupla.
pub fn join(a: &[u8], b: &[u8]) -> Vec<u8> {
    if a.is_empty() {
        return b.to_vec();
    }
    if b.is_empty() {
        return a.to_vec();
    }
    let mut p = a.to_vec();
    if !p.ends_with(b"/") {
        p.push(b'/');
    }
    p.extend_from_slice(b.strip_prefix(b"/").unwrap_or(b));
    p
}

pub fn getcwd() -> Result<Vec<u8>, Errno> {
    sysc().getcwd()
}

pub fn chdir(path: &[u8]) -> Result<(), Errno> {
    sysc().chdir(path)
}

/// Diretório pai (`/a/b` -> `/a`, `/a` -> `/`, `a` -> ``).
pub fn dirname(p: &[u8]) -> &[u8] {
    match p.iter().rposition(|b| *b == b'/') {
        Some(0) => b"/",
        Some(i) => &p[..i],
        None => b"",
    }
}

pub fn basename(p: &[u8]) -> &[u8] {
    let p = p.strip_suffix(b"/").unwrap_or(p);
    match p.iter().rposition(|b| *b == b'/') {
        Some(i) => &p[i + 1..],
        None => p,
    }
}

/// Normaliza um caminho absoluto sem tocar no FS: tira `.`, resolve `..` e barras repetidas.
pub fn normalize_abs(p: &[u8]) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = Vec::new();
    for c in p.split(|b| *b == b'/') {
        match c {
            b"" | b"." => {}
            b".." => {
                parts.pop();
            }
            _ => parts.push(c),
        }
    }
    let mut out = Vec::new();
    for c in parts {
        out.push(b'/');
        out.extend_from_slice(c);
    }
    if out.is_empty() {
        out.push(b'/');
    }
    out
}

/// Caminho absoluto a partir do cwd (sem resolver symlinks).
pub fn absolute(p: &[u8]) -> Vec<u8> {
    if p.starts_with(b"/") {
        return normalize_abs(p);
    }
    let cwd = getcwd().unwrap_or_else(|_| b"/".to_vec());
    normalize_abs(&join(&cwd, p))
}

/// Caminho real (symlinks resolvidos), via `chdir` + `getcwd` no diretório; pra arquivo, o
/// diretório pai é resolvido e o nome acrescentado.
pub fn realpath(p: &[u8]) -> Result<Vec<u8>, Errno> {
    let s = sysc();
    let cwd = s.getcwd()?;
    let st = s.fstatat(Fd::CWD, p, AtFlags::empty())?;
    let r = if st.file_type() == FileType::Directory {
        s.chdir(p).and_then(|_| s.getcwd())
    } else {
        let dir = dirname(p);
        let dir = if dir.is_empty() { b".".as_slice() } else { dir };
        s.chdir(dir).and_then(|_| s.getcwd()).map(|d| join(&d, basename(p)))
    };
    let _ = s.chdir(&cwd);
    r
}

// ---- arquivos ---------------------------------------------------------------------------------

pub fn lstat(p: &[u8]) -> Result<Stat, Errno> {
    sysc().fstatat(Fd::CWD, p, AtFlags::SYMLINK_NOFOLLOW)
}

pub fn stat(p: &[u8]) -> Result<Stat, Errno> {
    sysc().fstatat(Fd::CWD, p, AtFlags::empty())
}

pub fn exists(p: &[u8]) -> bool {
    lstat(p).is_ok()
}

pub fn is_dir(p: &[u8]) -> bool {
    stat(p).map(|s| s.file_type() == FileType::Directory).unwrap_or(false)
}

pub fn is_file(p: &[u8]) -> bool {
    stat(p).map(|s| s.file_type() == FileType::Regular).unwrap_or(false)
}

pub fn read(p: &[u8]) -> Result<Vec<u8>, Errno> {
    sys::read_file(p)
}

/// Lê um arquivo; `None` se ele não existe (ENOENT/ENOTDIR).
pub fn read_opt(p: &[u8]) -> Result<Option<Vec<u8>>, Errno> {
    match sys::read_file(p) {
        Ok(d) => Ok(Some(d)),
        Err(Errno::ENOENT | Errno::ENOTDIR) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Cria ou trunca e escreve.
pub fn write(p: &[u8], data: &[u8], mode: Mode) -> Result<(), Errno> {
    let s = sysc();
    let fd = s.openat(Fd::CWD, p, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC, mode)?;
    let r = write_fd(&*s, fd, data);
    let _ = s.close(fd);
    r
}

/// Acrescenta no fim (cria se não existir).
pub fn append(p: &[u8], data: &[u8], mode: Mode) -> Result<(), Errno> {
    let s = sysc();
    let fd = s.openat(Fd::CWD, p, OFlags::WRONLY | OFlags::CREAT | OFlags::APPEND | OFlags::CLOEXEC, mode)?;
    let r = write_fd(&*s, fd, data);
    let _ = s.close(fd);
    r
}

fn write_fd(s: &dyn Syscalls, fd: Fd, mut data: &[u8]) -> Result<(), Errno> {
    while !data.is_empty() {
        match s.write(fd, data) {
            Ok(0) => return Err(Errno::EIO),
            Ok(n) => data = &data[n..],
            Err(Errno::EINTR) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Cria um arquivo novo (`O_EXCL`).
pub fn create_new(p: &[u8], data: &[u8], mode: Mode) -> Result<(), Errno> {
    let s = sysc();
    let fd = s.openat(Fd::CWD, p, OFlags::WRONLY | OFlags::CREAT | OFlags::EXCL | OFlags::CLOEXEC, mode)?;
    let r = write_fd(&*s, fd, data);
    let _ = s.close(fd);
    r
}

pub fn unlink(p: &[u8]) -> Result<(), Errno> {
    sysc().unlinkat(Fd::CWD, p, AtFlags::empty())
}

pub fn rmdir(p: &[u8]) -> Result<(), Errno> {
    sysc().unlinkat(Fd::CWD, p, AtFlags::REMOVEDIR)
}

pub fn rename(a: &[u8], b: &[u8]) -> Result<(), Errno> {
    sysc().renameat2(Fd::CWD, a, Fd::CWD, b, RenameFlags::empty())
}

pub fn readlink(p: &[u8]) -> Result<Vec<u8>, Errno> {
    sysc().readlinkat(Fd::CWD, p)
}

pub fn symlink(target: &[u8], p: &[u8]) -> Result<(), Errno> {
    sysc().symlinkat(target, Fd::CWD, p)
}

pub fn chmod(p: &[u8], mode: Mode) -> Result<(), Errno> {
    sysc().fchmodat(Fd::CWD, p, mode, AtFlags::empty())
}

pub fn set_mtime(p: &[u8], sec: i64) -> Result<(), Errno> {
    let t = sysabi::TimeSpec { sec, nsec: 0 };
    sysc().utimensat(Fd::CWD, p, SetTime::At(t), SetTime::At(t), AtFlags::empty())
}

pub fn mkdir(p: &[u8], mode: Mode) -> Result<(), Errno> {
    sysc().mkdirat(Fd::CWD, p, mode)
}

/// `mkdir -p`.
pub fn mkdir_p(p: &[u8], mode: Mode) -> Result<(), Errno> {
    match mkdir(p, mode) {
        Ok(()) => return Ok(()),
        Err(Errno::EEXIST) => {
            return if is_dir(p) { Ok(()) } else { Err(Errno::EEXIST) };
        }
        Err(Errno::ENOENT) => {}
        Err(e) => return Err(e),
    }
    let parent = dirname(p);
    if !parent.is_empty() && parent != p {
        mkdir_p(parent, mode)?;
    }
    match mkdir(p, mode) {
        Ok(()) | Err(Errno::EEXIST) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Garante os diretórios que levam até o arquivo `p`.
pub fn mkdir_parents(p: &[u8]) -> Result<(), Errno> {
    let parent = dirname(p);
    if parent.is_empty() || parent == b"/" {
        return Ok(());
    }
    mkdir_p(parent, 0o777)
}

/// Lista um diretório (sem `.` e `..`), na ordem do FS.
pub fn read_dir(p: &[u8]) -> Result<Vec<DirEntry>, Errno> {
    sys::read_dir(p)
}

/// Remove diretórios vazios subindo a partir de `dir` até (sem incluir) `stop`.
pub fn remove_empty_parents(dir: &[u8], stop: &[u8]) {
    let mut d = dir.to_vec();
    while d.len() > stop.len() && d.starts_with(stop) {
        if rmdir(&d).is_err() {
            break;
        }
        d = dirname(&d).to_vec();
    }
}

/// `rm -rf`.
pub fn remove_tree(p: &[u8]) -> Result<(), Errno> {
    let st = match lstat(p) {
        Ok(s) => s,
        Err(Errno::ENOENT) => return Ok(()),
        Err(e) => return Err(e),
    };
    if st.file_type() == FileType::Directory {
        for e in read_dir(p)? {
            remove_tree(&join(p, &e.name))?;
        }
        rmdir(p)
    } else {
        unlink(p)
    }
}

// ---- lockfile ---------------------------------------------------------------------------------

/// `<arquivo>.lock` criado com `O_EXCL`; `commit` renomeia por cima do arquivo, e o `Drop` sem
/// commit apaga o lock.
pub struct LockFile {
    path: Vec<u8>,
    lock: Vec<u8>,
    fd: Option<Fd>,
}

impl LockFile {
    pub fn acquire(path: &[u8]) -> Result<LockFile, Errno> {
        let mut lock = path.to_vec();
        lock.extend_from_slice(b".lock");
        let s = sysc();
        let fd = s.openat(Fd::CWD, &lock, OFlags::RDWR | OFlags::CREAT | OFlags::EXCL | OFlags::CLOEXEC, 0o666)?;
        Ok(LockFile { path: path.to_vec(), lock, fd: Some(fd) })
    }

    pub fn path(&self) -> &[u8] {
        &self.path
    }

    pub fn lock_path(&self) -> &[u8] {
        &self.lock
    }

    pub fn write(&mut self, data: &[u8]) -> Result<(), Errno> {
        let fd = self.fd.ok_or(Errno::EBADF)?;
        write_fd(&*sysc(), fd, data)
    }

    pub fn commit(mut self) -> Result<(), Errno> {
        if let Some(fd) = self.fd.take() {
            let _ = sysc().close(fd);
        }
        let r = rename(&self.lock, &self.path);
        if r.is_err() {
            let _ = unlink(&self.lock);
        }
        self.lock.clear();
        r
    }

    /// Fecha e apaga o lock sem mexer no arquivo.
    pub fn rollback(mut self) {
        self.release();
    }

    fn release(&mut self) {
        if let Some(fd) = self.fd.take() {
            let _ = sysc().close(fd);
        }
        if !self.lock.is_empty() {
            let _ = unlink(&self.lock);
            self.lock.clear();
        }
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        self.release();
    }
}

/// A mensagem do git quando o lock já existe (ou não pôde ser criado).
pub fn lock_error_message(path: &[u8], e: Errno) -> String {
    let abs = absolute(path);
    let mut msg = format!("Unable to create '{}.lock': {}", String::from_utf8_lossy(&abs), e.message());
    if e == Errno::EEXIST {
        msg.push_str(
            ".\n\nAnother git process seems to be running in this repository, e.g.\n\
             an editor opened by 'git commit'. Please make sure all processes\n\
             are terminated then try again. If it still fails, a git process\n\
             may have crashed in this repository earlier:\n\
             remove the file manually to continue.",
        );
    } else {
        msg.push('.');
    }
    msg
}

/// Escrita atômica com lock (`<arquivo>.lock` e rename).
pub fn write_locked(path: &[u8], data: &[u8]) -> Result<(), String> {
    let mut lock = LockFile::acquire(path).map_err(|e| lock_error_message(path, e))?;
    lock.write(data).map_err(|e| format!("unable to write {}: {}", String::from_utf8_lossy(path), e.message()))?;
    lock.commit().map_err(|e| format!("unable to rename {}: {}", String::from_utf8_lossy(path), e.message()))
}

// ---- utilidades -------------------------------------------------------------------------------

pub fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Modo de arquivo para o git a partir do `stat`.
pub fn git_mode_of(st: &Stat) -> u32 {
    match st.file_type() {
        FileType::Symlink => 0o120000,
        FileType::Directory => 0o040000,
        _ => {
            if st.mode & 0o100 != 0 {
                0o100755
            } else {
                0o100644
            }
        }
    }
}
