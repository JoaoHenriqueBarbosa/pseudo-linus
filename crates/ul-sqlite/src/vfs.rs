//! VFS do SQLite sobre o FS do sandbox.
//!
//! É um porte do VFS `unix` do SQLite 3.46.1 (`os_unix.c`) pra cima do `sysabi`, com a trait segura
//! do sqlite-plugin: abertura com os mesmos flags e modos, arquivos temporários `etilqs_*` no
//! diretório temporário do sandbox (apagados logo depois de abertos), caminho absoluto com os
//! symlinks resolvidos, tamanho 1 relatado como 0, e o protocolo de travas do `unixLock` (bytes
//! PENDING, RESERVED e SHARED) sobre as travas por open file description do kernel
//! (`ofd_setlk`/`ofd_getlk`). Como o dono da trava é a open file description, cada conexão já é um
//! dono separado e a camada `unixInodeInfo` do original não faz falta.
//!
//! Toda syscall daqui roda dentro de [`crate::unwind::guard`]: um `KillUnwind` no meio de uma leitura
//! vira erro de I/O pro SQLite e é relançado quando o controle volta pro CLI.
//!
//! O VFS é registrado como padrão com o nome `unix` e também como `unix-excl`, `unix-dotfile` e
//! `unix-none`: a libsqlite3 do Debian tem URI ligado, e `ATTACH 'file:x?vfs=unix'` encontraria o VFS
//! do host. Registrados depois, os nossos ficam na frente da lista e o `sqlite3_vfs_find` devolve
//! sempre um dos nossos.
//!
//! Limites conhecidos (STATUS.md): sem `xShmMap` (WAL só com `locking_mode=EXCLUSIVE`, que o CLI liga
//! sozinho), e `xRandomness`, `xSleep` e `xCurrentTime` não fazem parte da trait (o CLI troca as
//! funções SQL que dependem deles).

use std::borrow::Cow;
use std::ffi::CString;
use std::sync::OnceLock;

use sqlite_plugin::flags::{AccessFlags, LockLevel, OpenKind, OpenMode, OpenOpts};
use sqlite_plugin::vars;
use sqlite_plugin::vfs::{RegisterOpts, Vfs, VfsHandle, VfsResult, register_static};
use sysabi::{
    AccessMode, AtFlags, Errno, Fd, FileLock, FileType, LockKind, OFlags, SysResult, Syscalls, sys,
};

use crate::unwind::guard;

/// Nome do VFS padrão (o mesmo do VFS do host que ele substitui).
pub const VFS_NAME: &str = "unix";
/// Outros nomes do `os_unix.c` no Linux, registrados apontando pro nosso.
pub const VFS_ALIASES: &[&str] = &["unix-excl", "unix-dotfile", "unix-none"];

const PENDING_BYTE: u64 = 0x4000_0000;
const RESERVED_BYTE: u64 = PENDING_BYTE + 1;
const SHARED_FIRST: u64 = PENDING_BYTE + 2;
const SHARED_SIZE: u64 = 510;
/// `SQLITE_DEFAULT_FILE_PERMISSIONS`.
const DEFAULT_FILE_PERMISSIONS: u32 = 0o644;
/// `SQLITE_MAX_SYMLINK`.
const MAX_SYMLINK: usize = 100;
/// `SQLITE_DEFAULT_SECTOR_SIZE` do build do Debian.
const SECTOR_SIZE: i32 = 4096;

/// Arquivo aberto pelo VFS.
pub struct Handle {
    fd: Fd,
    readonly: bool,
    level: LockLevel,
}

impl VfsHandle for Handle {
    fn readonly(&self) -> bool {
        self.readonly
    }

    fn in_memory(&self) -> bool {
        false
    }
}

/// O VFS em si: sem estado, tudo vem do processo corrente da thread.
pub struct SandboxVfs;

thread_local! {
    /// Processo "morrendo": o CLI está saindo sem `sqlite3_close` (comando da linha de comando que
    /// falhou, `.exit N`, morte por sinal). O SQLite ainda fecha a conexão pra liberar memória, mas
    /// nada mais chega ao FS: o rollback falha e o journal quente fica onde estava, como fica quando
    /// o processo real termina sem fechar o banco.
    static ABANDONED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Liga ou desliga o modo [`ABANDONED`] da thread.
pub fn set_abandoned(on: bool) {
    ABANDONED.with(|a| a.set(on));
}

fn abandoned() -> bool {
    ABANDONED.with(|a| a.get())
}

fn proc_sys() -> std::sync::Arc<dyn Syscalls> {
    sys::current()
}

/// Roda `f` com as syscalls do processo corrente e converte o unwind em `err`.
fn with_sys<T>(err: i32, f: impl FnOnce(&dyn Syscalls) -> VfsResult<T>) -> VfsResult<T> {
    match guard(|| {
        let s = proc_sys();
        f(s.as_ref())
    }) {
        Some(r) => r,
        None => {
            // O processo está terminando no meio do I/O: daqui em diante nada mais chega ao FS.
            set_abandoned(true);
            Err(err)
        }
    }
}

/// Repete em EINTR, como as funções `robust_*` do `os_unix.c`.
fn retry<T>(mut f: impl FnMut() -> SysResult<T>) -> SysResult<T> {
    loop {
        match f() {
            Err(Errno::EINTR) => continue,
            r => return r,
        }
    }
}

/// `sqliteErrorFromPosixError`.
fn posix_to_sqlite(e: Errno, io: i32) -> i32 {
    match e {
        Errno::EACCES | Errno::EAGAIN | Errno::ETIMEDOUT | Errno::EBUSY | Errno::EINTR | Errno::ENOLCK => {
            vars::SQLITE_BUSY
        }
        Errno::EPERM => vars::SQLITE_PERM,
        _ => io,
    }
}

/// Diretório temporário: `SQLITE_TMPDIR`, `TMPDIR`, /var/tmp, /usr/tmp, /tmp e ".", o primeiro que
/// for diretório com escrita e busca (`unixTempFileDir`).
fn temp_dir(s: &dyn Syscalls) -> Option<Vec<u8>> {
    let mut candidates: Vec<Vec<u8>> = Vec::new();
    for var in [&b"SQLITE_TMPDIR"[..], b"TMPDIR"] {
        if let Some(v) = s.getenv(var) {
            candidates.push(v);
        }
    }
    for d in ["/var/tmp", "/usr/tmp", "/tmp", "."] {
        candidates.push(d.as_bytes().to_vec());
    }
    candidates.into_iter().find(|d| {
        matches!(s.fstatat(Fd::CWD, d, AtFlags::empty()), Ok(st) if st.file_type() == FileType::Directory)
            && s.faccessat(Fd::CWD, d, AccessMode::W_OK | AccessMode::X_OK, AtFlags::empty()).is_ok()
    })
}

/// `unixGetTempname`: `<dir>/etilqs_<hex aleatório>` que ainda não existe.
fn temp_name(s: &dyn Syscalls) -> VfsResult<Vec<u8>> {
    let dir = temp_dir(s).ok_or(vars::SQLITE_IOERR_GETTEMPPATH)?;
    for _ in 0..=11 {
        let mut r = [0u8; 8];
        s.getrandom(&mut r).map_err(|_| vars::SQLITE_IOERR_GETTEMPPATH)?;
        let mut name = dir.clone();
        name.extend_from_slice(format!("/etilqs_{:x}", u64::from_le_bytes(r)).as_bytes());
        if s.faccessat(Fd::CWD, &name, AccessMode::empty(), AtFlags::empty()).is_err() {
            return Ok(name);
        }
    }
    Err(vars::SQLITE_ERROR)
}

/// `robust_open`: nunca devolve 0, 1 ou 2 (um banco aberto no lugar do stdout seria corrompido pelo
/// primeiro printf), e acerta as permissões de arquivo novo pra `mode` quando ele é dado.
fn robust_open(s: &dyn Syscalls, path: &[u8], flags: OFlags, mode: u32) -> SysResult<Fd> {
    let m2 = if mode != 0 { mode } else { DEFAULT_FILE_PERMISSIONS };
    let fd = loop {
        let fd = retry(|| s.openat(Fd::CWD, path, flags | OFlags::CLOEXEC, m2))?;
        if fd.0 > 2 {
            break fd;
        }
        if flags.contains(OFlags::EXCL | OFlags::CREAT) {
            let _ = s.unlinkat(Fd::CWD, path, AtFlags::empty());
        }
        let _ = s.close(fd);
        if s.openat(Fd::CWD, b"/dev/null", OFlags::RDONLY, mode).is_err() {
            return Err(Errno::ENOENT);
        }
    };
    if mode != 0
        && let Ok(st) = s.fstat(fd)
        && st.size == 0
        && st.mode & 0o777 != mode
    {
        let _ = s.fchmod(fd, mode);
    }
    Ok(fd)
}

/// `findCreateFileMode`: journal e WAL herdam modo, dono e grupo do banco; temporário é 0600.
fn create_mode(s: &dyn Syscalls, path: &[u8], kind: &OpenKind, delete_on_close: bool) -> VfsResult<(u32, Option<(u32, u32)>)> {
    if matches!(kind, OpenKind::Wal | OpenKind::MainJournal) {
        let mut n = path.len().saturating_sub(1);
        while n > 0 && path[n] != b'.' {
            if path[n] == b'-' {
                return match s.fstatat(Fd::CWD, &path[..n], AtFlags::empty()) {
                    Ok(st) => Ok((st.mode & 0o777, Some((st.uid, st.gid)))),
                    Err(_) => Err(vars::SQLITE_IOERR_FSTAT),
                };
            }
            n -= 1;
        }
        return Ok((0, None));
    }
    if delete_on_close {
        return Ok((0o600, None));
    }
    Ok((0, None))
}

impl SandboxVfs {
    fn open_file(&self, s: &dyn Syscalls, path: Option<&str>, opts: &OpenOpts) -> VfsResult<Handle> {
        let kind = opts.kind();
        let mode = opts.mode();
        let delete = opts.delete_on_close();
        let (mut readonly, create, exclusive) = match &mode {
            OpenMode::ReadOnly => (true, false, false),
            OpenMode::ReadWrite { create } => (
                false,
                !matches!(create, sqlite_plugin::flags::CreateMode::None),
                matches!(create, sqlite_plugin::flags::CreateMode::MustCreate),
            ),
        };
        let is_new_journal = create && matches!(kind, OpenKind::SuperJournal | OpenKind::MainJournal | OpenKind::Wal);
        let name: Vec<u8> = match path {
            Some(p) => p.as_bytes().to_vec(),
            None => temp_name(s)?,
        };
        let mut flags = if readonly { OFlags::RDONLY } else { OFlags::RDWR };
        if create {
            flags |= OFlags::CREAT;
        }
        if exclusive {
            flags |= OFlags::EXCL | OFlags::NOFOLLOW;
        }
        flags |= OFlags::NOFOLLOW;
        let (open_mode, owner) = create_mode(s, &name, &kind, delete)?;
        let fd = match robust_open(s, &name, flags, open_mode) {
            Ok(fd) => fd,
            Err(e) => {
                if is_new_journal
                    && e == Errno::EACCES
                    && s.faccessat(Fd::CWD, &name, AccessMode::empty(), AtFlags::empty()).is_err()
                {
                    return Err(vars::SQLITE_READONLY_DIRECTORY);
                }
                if e != Errno::EISDIR && !readonly {
                    // Sem permissão de escrita: tenta só leitura, como o original.
                    readonly = true;
                    let ro = flags.difference(OFlags::RDWR | OFlags::CREAT);
                    match robust_open(s, &name, ro, open_mode) {
                        Ok(fd) => fd,
                        Err(_) => return Err(vars::SQLITE_CANTOPEN),
                    }
                } else {
                    return Err(vars::SQLITE_CANTOPEN);
                }
            }
        };
        if open_mode != 0
            && let Some((uid, gid)) = owner
            && s.geteuid() == 0
        {
            let _ = s.fchownat(fd, b"", Some(uid), Some(gid), AtFlags::EMPTY_PATH);
        }
        if delete {
            let _ = s.unlinkat(Fd::CWD, &name, AtFlags::empty());
        }
        Ok(Handle { fd, readonly, level: LockLevel::Unlocked })
    }

    fn setlk(s: &dyn Syscalls, fd: Fd, kind: LockKind, start: u64, len: u64) -> SysResult<()> {
        s.ofd_setlk(fd, FileLock { kind, start, len }, false)
    }
}

/// Junta os componentes de `path` em `out`, resolvendo `.`, `..` e symlinks (`appendAllPathElements`).
fn append_all(s: &dyn Syscalls, out: &mut Vec<u8>, path: &[u8], links: &mut usize) -> VfsResult<()> {
    for comp in path.split(|b| *b == b'/') {
        if comp.is_empty() {
            continue;
        }
        append_one(s, out, comp, links)?;
    }
    Ok(())
}

fn append_one(s: &dyn Syscalls, out: &mut Vec<u8>, name: &[u8], links: &mut usize) -> VfsResult<()> {
    if name == b"." {
        return Ok(());
    }
    if name == b".." {
        if out.len() > 1 {
            while let Some(c) = out.pop() {
                if c == b'/' {
                    break;
                }
            }
        }
        return Ok(());
    }
    let before = out.len();
    out.push(b'/');
    out.extend_from_slice(name);
    match s.fstatat(Fd::CWD, out, AtFlags::SYMLINK_NOFOLLOW) {
        Err(Errno::ENOENT) => Ok(()),
        Err(_) => Err(vars::SQLITE_CANTOPEN),
        Ok(st) if st.file_type() == FileType::Symlink => {
            *links += 1;
            if *links > MAX_SYMLINK {
                return Err(vars::SQLITE_CANTOPEN);
            }
            let target = s.readlinkat(Fd::CWD, out).map_err(|_| vars::SQLITE_CANTOPEN)?;
            if target.is_empty() {
                return Err(vars::SQLITE_CANTOPEN);
            }
            if target.starts_with(b"/") {
                out.clear();
            } else {
                out.truncate(before);
            }
            append_all(s, out, &target, links)
        }
        Ok(_) => Ok(()),
    }
}

/// `unixFullPathname`.
pub fn full_pathname(s: &dyn Syscalls, path: &[u8]) -> VfsResult<Vec<u8>> {
    let mut out = Vec::new();
    let mut links = 0;
    if !path.starts_with(b"/") {
        let cwd = s.getcwd().map_err(|_| vars::SQLITE_CANTOPEN)?;
        append_all(s, &mut out, &cwd, &mut links)?;
    }
    append_all(s, &mut out, path, &mut links)?;
    if out.len() < 2 {
        return Err(vars::SQLITE_CANTOPEN);
    }
    Ok(out)
}

impl Vfs for SandboxVfs {
    type Handle = Handle;

    fn canonical_path<'a>(&self, path: Cow<'a, str>) -> VfsResult<Cow<'a, str>> {
        with_sys(vars::SQLITE_CANTOPEN, |s| {
            let full = full_pathname(s, path.as_bytes())?;
            Ok(Cow::Owned(String::from_utf8_lossy(&full).into_owned()))
        })
    }

    fn open(&self, path: Option<&str>, opts: OpenOpts) -> VfsResult<Self::Handle> {
        if abandoned() {
            return Err(vars::SQLITE_CANTOPEN);
        }
        with_sys(vars::SQLITE_CANTOPEN, |s| self.open_file(s, path, &opts))
    }

    fn delete(&self, path: &str) -> VfsResult<()> {
        if abandoned() {
            return Err(vars::SQLITE_IOERR_DELETE);
        }
        with_sys(vars::SQLITE_IOERR_DELETE, |s| match retry(|| s.unlinkat(Fd::CWD, path.as_bytes(), AtFlags::empty())) {
            Ok(()) => Ok(()),
            Err(Errno::ENOENT) => Err(vars::SQLITE_IOERR_DELETE_NOENT),
            Err(_) => Err(vars::SQLITE_IOERR_DELETE),
        })
    }

    fn access(&self, path: &str, flags: AccessFlags) -> VfsResult<bool> {
        with_sys(vars::SQLITE_IOERR_ACCESS, |s| {
            Ok(match flags {
                AccessFlags::Exists => match s.fstatat(Fd::CWD, path.as_bytes(), AtFlags::empty()) {
                    Ok(st) => st.file_type() != FileType::Regular || st.size > 0,
                    Err(_) => false,
                },
                AccessFlags::ReadWrite => s
                    .faccessat(Fd::CWD, path.as_bytes(), AccessMode::W_OK | AccessMode::R_OK, AtFlags::empty())
                    .is_ok(),
                AccessFlags::Read => s.faccessat(Fd::CWD, path.as_bytes(), AccessMode::R_OK, AtFlags::empty()).is_ok(),
            })
        })
    }

    fn file_size(&self, handle: &mut Self::Handle) -> VfsResult<usize> {
        let fd = handle.fd;
        with_sys(vars::SQLITE_IOERR_FSTAT, |s| {
            let st = s.fstat(fd).map_err(|_| vars::SQLITE_IOERR_FSTAT)?;
            // Tamanho 1 é relatado como 0 (ticket #3260 do SQLite), em qualquer sistema.
            let size = if st.size == 1 { 0 } else { st.size };
            usize::try_from(size).map_err(|_| vars::SQLITE_IOERR_FSTAT)
        })
    }

    fn truncate(&self, handle: &mut Self::Handle, size: usize) -> VfsResult<()> {
        if abandoned() {
            return Err(vars::SQLITE_IOERR_TRUNCATE);
        }
        let fd = handle.fd;
        with_sys(vars::SQLITE_IOERR_TRUNCATE, |s| {
            retry(|| s.ftruncate(fd, size as u64)).map_err(|_| vars::SQLITE_IOERR_TRUNCATE)
        })
    }

    fn write(&self, handle: &mut Self::Handle, offset: usize, data: &[u8]) -> VfsResult<usize> {
        if abandoned() {
            return Err(vars::SQLITE_IOERR_WRITE);
        }
        let fd = handle.fd;
        with_sys(vars::SQLITE_IOERR_WRITE, |s| {
            let mut done = 0;
            while done < data.len() {
                match retry(|| s.pwrite(fd, &data[done..], (offset + done) as u64)) {
                    Ok(0) => return Err(vars::SQLITE_FULL),
                    Ok(n) => done += n,
                    Err(Errno::ENOSPC) => return Err(vars::SQLITE_FULL),
                    Err(_) => return Err(vars::SQLITE_IOERR_WRITE),
                }
            }
            Ok(done)
        })
    }

    fn read(&self, handle: &mut Self::Handle, offset: usize, data: &mut [u8]) -> VfsResult<usize> {
        if abandoned() {
            return Err(vars::SQLITE_IOERR_READ);
        }
        let fd = handle.fd;
        with_sys(vars::SQLITE_IOERR_READ, |s| {
            let mut done = 0;
            while done < data.len() {
                match retry(|| s.pread(fd, &mut data[done..], (offset + done) as u64)) {
                    Ok(0) => break,
                    Ok(n) => done += n,
                    Err(_) => return Err(vars::SQLITE_IOERR_READ),
                }
            }
            Ok(done)
        })
    }

    fn lock(&self, handle: &mut Self::Handle, level: LockLevel) -> VfsResult<()> {
        if handle.level >= level {
            return Ok(());
        }
        if abandoned() {
            return Err(vars::SQLITE_IOERR_LOCK);
        }
        let fd = handle.fd;
        let current = handle.level;
        let mut new_level = handle.level;
        let r = with_sys(vars::SQLITE_IOERR_LOCK, |s| {
            let fail = |e: Errno| posix_to_sqlite(e, vars::SQLITE_IOERR_LOCK);
            if level == LockLevel::Shared || (level == LockLevel::Exclusive && current == LockLevel::Reserved) {
                let kind = if level == LockLevel::Shared { LockKind::Read } else { LockKind::Write };
                Self::setlk(s, fd, kind, PENDING_BYTE, 1).map_err(fail)?;
                if level == LockLevel::Exclusive {
                    new_level = LockLevel::Pending;
                }
            }
            if level == LockLevel::Shared {
                let got = Self::setlk(s, fd, LockKind::Read, SHARED_FIRST, SHARED_SIZE).map_err(fail);
                let dropped = Self::setlk(s, fd, LockKind::Unlock, PENDING_BYTE, 1);
                got?;
                if dropped.is_err() {
                    return Err(vars::SQLITE_IOERR_UNLOCK);
                }
            } else {
                let (start, len) = if level == LockLevel::Reserved { (RESERVED_BYTE, 1) } else { (SHARED_FIRST, SHARED_SIZE) };
                Self::setlk(s, fd, LockKind::Write, start, len).map_err(fail)?;
            }
            Ok(())
        });
        match r {
            Ok(()) => {
                handle.level = level;
                Ok(())
            }
            Err(e) => {
                handle.level = new_level;
                Err(e)
            }
        }
    }

    fn unlock(&self, handle: &mut Self::Handle, level: LockLevel) -> VfsResult<()> {
        if handle.level <= level {
            return Ok(());
        }
        if abandoned() {
            // As travas soltam quando o close fechar o fd, como na morte do processo.
            handle.level = level;
            return Ok(());
        }
        let fd = handle.fd;
        let current = handle.level;
        let r = with_sys(vars::SQLITE_IOERR_UNLOCK, |s| {
            if current > LockLevel::Shared {
                if level == LockLevel::Shared && Self::setlk(s, fd, LockKind::Read, SHARED_FIRST, SHARED_SIZE).is_err() {
                    return Err(vars::SQLITE_IOERR_RDLOCK);
                }
                Self::setlk(s, fd, LockKind::Unlock, PENDING_BYTE, 2).map_err(|_| vars::SQLITE_IOERR_UNLOCK)?;
            }
            if level == LockLevel::Unlocked {
                Self::setlk(s, fd, LockKind::Unlock, 0, 0).map_err(|_| vars::SQLITE_IOERR_UNLOCK)?;
            }
            Ok(())
        });
        match r {
            Ok(()) => {
                handle.level = level;
                Ok(())
            }
            Err(e) => {
                if level == LockLevel::Unlocked {
                    handle.level = LockLevel::Unlocked;
                }
                Err(e)
            }
        }
    }

    fn check_reserved_lock(&self, handle: &mut Self::Handle) -> VfsResult<bool> {
        if handle.level > LockLevel::Shared {
            return Ok(true);
        }
        let fd = handle.fd;
        with_sys(vars::SQLITE_IOERR_CHECKRESERVEDLOCK, |s| {
            match s.ofd_getlk(fd, FileLock { kind: LockKind::Write, start: RESERVED_BYTE, len: 1 }) {
                Ok(other) => Ok(other.is_some()),
                Err(_) => Err(vars::SQLITE_IOERR_CHECKRESERVEDLOCK),
            }
        })
    }

    fn sync(&self, handle: &mut Self::Handle) -> VfsResult<()> {
        if abandoned() {
            return Err(vars::SQLITE_IOERR_FSYNC);
        }
        let fd = handle.fd;
        with_sys(vars::SQLITE_IOERR_FSYNC, |s| retry(|| s.fsync(fd)).map_err(|_| vars::SQLITE_IOERR_FSYNC))
    }

    fn close(&self, handle: Self::Handle) -> VfsResult<()> {
        let fd = handle.fd;
        // O close solta as travas OFD (é a última referência da open file description).
        with_sys(vars::SQLITE_IOERR_CLOSE, |s| {
            let _ = s.close(fd);
            Ok(())
        })
    }

    fn sector_size(&self, _handle: &mut Self::Handle) -> VfsResult<i32> {
        Ok(SECTOR_SIZE)
    }

    fn device_characteristics(&self, _handle: &mut Self::Handle) -> VfsResult<i32> {
        // `unixDeviceCharacteristics` com psow ligado (o padrão).
        Ok(vars::SQLITE_IOCAP_POWERSAFE_OVERWRITE)
    }
}

/// Lê os primeiros bytes de um arquivo do sandbox (pra decidir o modo de trava antes do SQLite abrir).
pub fn header_is_wal(path: &[u8]) -> bool {
    guard(|| {
        let s = proc_sys();
        let Ok(fd) = s.openat(Fd::CWD, path, OFlags::RDONLY | OFlags::CLOEXEC, 0) else { return false };
        let mut buf = [0u8; 20];
        let n = s.pread(fd, &mut buf, 0).unwrap_or(0);
        let _ = s.close(fd);
        n == 20 && buf.starts_with(b"SQLite format 3\0") && buf[18] == 2 && buf[19] == 2
    })
    .unwrap_or(false)
}

static REGISTERED: OnceLock<Result<(), i32>> = OnceLock::new();

/// Registra o VFS (uma vez por processo host). Os apelidos vêm primeiro e o `unix` por último, como
/// padrão, pra que o sqlite-plugin capture o VFS do host como base de todos (é pra ele que vão
/// xRandomness, xSleep, xCurrentTime e xDl*, que a trait não cobre).
pub fn register() -> Result<(), i32> {
    *REGISTERED.get_or_init(|| {
        for alias in VFS_ALIASES {
            let name = CString::new(*alias).map_err(|_| vars::SQLITE_INTERNAL)?;
            register_static(name, SandboxVfs, RegisterOpts { make_default: false })?;
        }
        let name = CString::new(VFS_NAME).map_err(|_| vars::SQLITE_INTERNAL)?;
        register_static(name, SandboxVfs, RegisterOpts { make_default: true })?;
        Ok(())
    })
}
