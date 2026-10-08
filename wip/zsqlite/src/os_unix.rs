//! VFS do Unix (os_unix.c do SQLite 3.46.1), sobre as syscalls do `sysabi`.
//!
//! O projeto roda num Linux simulado e não usa libc nem `std::fs`: toda
//! chamada de sistema passa por `sysabi::sys::current()` (as syscalls do
//! pseudo-processo corrente da thread). O arquivo C é a autoridade; o que não
//! existe no Linux do Debian (Apple, VxWorks, AFP, NFS, proxy, `flock`,
//! `SQLITE_DEBUG`, `SQLITE_TEST`) some.
//!
//! Escolhas de modelagem (registradas para o integrador):
//!
//! * `unixFile` é [`UnixFile`]; o `sqlite3_io_methods` escolhido pelo "finder"
//!   é o campo [`LockStyle`] (`Posix`, `NoLock`, `DotLock`). Os quatro VFS do
//!   Debian (`unix`, `unix-none`, `unix-dotfile`, `unix-excl`) são quatro
//!   [`UnixVfs`] com o estilo fixo; `unix-excl` é `Posix` mais a flag
//!   `UNIXFILE_EXCL`, escolhida pelo nome do VFS como no C.
//! * `iVersion` da tabela de métodos: o estilo `Posix` é 3 (memória
//!   compartilhada do WAL e mapeamento). `NoLock` e `DotLock` têm `xShmMap`
//!   nulo no C, e o único jeito de o pager saber que não há WAL é a versão da
//!   tabela, então `i_version()` devolve 1 para os dois (o `xFetch` deles
//!   devolve nulo de qualquer forma).
//! * A lista `inodeList` e os dados de `unixInodeInfo` e `unixShmNode` ficam
//!   num `static Mutex` seguro (`GLOBALS`). Como os pseudo-processos são
//!   threads do mesmo processo hospedeiro, mas as travas POSIX do kernel
//!   simulado são por pseudo-processo (dono = pid), a chave do inode inclui o
//!   pid: cada pseudo-processo enxerga a sua própria `inodeList`, como cada
//!   processo real do Debian enxerga a dele. O `unixBigLock` e o `pLockMutex`
//!   de cada inode viram um mutex só. Como o escalonador do kernel simulado é
//!   por CPU cedida em pontos de syscall, o mutex nunca bloqueia a thread:
//!   quando está ocupado, quem espera cede a CPU com `sched_yield`.
//! * Memória mapeada: não há `mmap` em Rust seguro. O mapeamento fica
//!   desligado: `xFetch` devolve `None` e `mmapSize` é sempre 0. O limite
//!   `mmapSizeMax` e o `SQLITE_FCNTL_MMAP_SIZE` continuam existindo (o
//!   `PRAGMA mmap_size` depende deles).
//! * Memória compartilhada do WAL: ver a seção "Shared memory" mais abaixo
//!   (regiões lidas e escritas com `pread` e `pwrite` no arquivo `-shm`).
//! * Os logs `sqlite3_log` do C (`unixLogError`, `verifyDbFile`) não existem
//!   ainda neste porte e só alimentam o callback de log; ficam de fora.
//! * `errno`: o C lê a variável global; aqui cada erro de syscall observado
//!   por este módulo grava o número numa variável por thread
//!   (`LAST_ERRNO`), que alimenta `xGetLastError`.

use std::cell::Cell;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, TryLockError};
use std::time::Duration;

use sysabi::fcntl::{F_RDLCK, F_UNLCK, F_WRLCK, SEEK_SET};
use sysabi::{
    AccessMode, AtFlags, Clock, Errno, FallocFlags, Fd, FileType, Flock, LockCmd, OFlags, SetTime,
    Syscalls,
};

use crate::consts::*;
use crate::os::{vfs_register, DlHandle, DlSymbol, FileControlArg, ShmRegion, SyscallPtr, Vfs, VfsFile, VfsRef};

// ---------------------------------------------------------------------------
// Constantes e estado de configuração
// ---------------------------------------------------------------------------

/// `MAX_PATHNAME`: tamanho máximo de caminho do VFS.
pub const MAX_PATHNAME: i32 = 512;

/// `SQLITE_MINIMUM_FILE_DESCRIPTOR`.
const MINIMUM_FILE_DESCRIPTOR: i32 = 3;

/// `SQLITE_DEFAULT_FILE_PERMISSIONS`.
const DEFAULT_FILE_PERMISSIONS: u32 = 0o644;

/// Época Unix em milissegundos do dia juliano (`unixEpoch` de `unixCurrentTimeInt64`).
const UNIX_EPOCH_MS: i64 = 24405875 * 8640000;

/// Sufixo do diretório de trava do `unix-dotfile` (`DOTLOCK_SUFFIX`).
const DOTLOCK_SUFFIX: &[u8] = b".lock";

// Valores de `unixFile.ctrlFlags` (`UNIXFILE_*`).
const UNIXFILE_EXCL: u16 = 0x01;
const UNIXFILE_RDONLY: u16 = 0x02;
const UNIXFILE_PERSIST_WAL: u16 = 0x04;
const UNIXFILE_DIRSYNC: u16 = 0x08;
const UNIXFILE_PSOW: u16 = 0x10;
const UNIXFILE_DELETE: u16 = 0x20;
const UNIXFILE_URI: u16 = 0x40;
const UNIXFILE_NOLOCK: u16 = 0x80;

/// `sqlite3GlobalConfig.szMmap` (padrão `SQLITE_DEFAULT_MMAP_SIZE`).
static SZ_MMAP: AtomicI64 = AtomicI64::new(SQLITE_DEFAULT_MMAP_SIZE as i64);

/// `sqlite3GlobalConfig.mxMmap` (padrão `SQLITE_MAX_MMAP_SIZE`).
static MX_MMAP: AtomicI64 = AtomicI64::new(SQLITE_MAX_MMAP_SIZE as i64);

/// `sqlite3_temp_directory`.
static TEMP_DIRECTORY: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// Grava `sqlite3GlobalConfig.szMmap` e `mxMmap` (o `sqlite3_config(SQLITE_CONFIG_MMAP_SIZE)`).
/// O módulo de configuração global ainda não existe; quando existir, é ele quem
/// chama esta função.
pub fn config_mmap_size(sz_mmap: i64, mx_mmap: i64) {
    SZ_MMAP.store(sz_mmap, Ordering::Relaxed);
    MX_MMAP.store(mx_mmap, Ordering::Relaxed);
}

/// Grava `sqlite3_temp_directory` (`PRAGMA temp_store_directory`).
pub fn set_temp_directory(dir: Option<Vec<u8>>) {
    *TEMP_DIRECTORY.lock().unwrap_or_else(PoisonError::into_inner) = dir;
}

thread_local! {
    /// O `errno` da última syscall que falhou nesta thread (o pseudo-processo).
    static LAST_ERRNO: Cell<i32> = const { Cell::new(0) };
    /// A mensagem do último erro de `dlopen` (o `dlerror()` da glibc é por thread).
    static DL_ERROR: Cell<Option<Vec<u8>>> = const { Cell::new(None) };
}

/// Registra o errno de uma syscall que falhou e o devolve.
fn note(e: Errno) -> Errno {
    LAST_ERRNO.with(|c| c.set(e.0));
    e
}

/// As syscalls do pseudo-processo corrente.
fn sys() -> Arc<dyn Syscalls> {
    sysabi::sys::current()
}

// ---------------------------------------------------------------------------
// Utilitários de caminho e de URI
// ---------------------------------------------------------------------------

/// A parte de `z` antes do primeiro NUL (a string C).
fn cstr(z: &[u8]) -> &[u8] {
    match z.iter().position(|&c| c == 0) {
        Some(n) => &z[..n],
        None => z,
    }
}

/// `sqlite3_uri_parameter` sobre um `sqlite3_filename` (o caminho seguido de
/// NUL e dos pares chave/valor, terminado por NUL duplo). `z_filename` já
/// começa no caminho (o `databaseName()` do C só recua até ali).
fn uri_parameter<'a>(z_filename: &'a [u8], z_param: &[u8]) -> Option<&'a [u8]> {
    let mut i = cstr(z_filename).len() + 1;
    while i < z_filename.len() && z_filename[i] != 0 {
        let key = cstr(&z_filename[i..]);
        i += key.len() + 1;
        if i > z_filename.len() {
            return None;
        }
        let val = cstr(&z_filename[i..]);
        i += val.len() + 1;
        if key == z_param {
            return Some(val);
        }
    }
    None
}

/// `sqlite3GetBoolean(z, dflt)` (que é `getSafetyLevel(z, 1, dflt) != 0`).
fn get_boolean(z: &[u8], dflt: bool) -> bool {
    if z.first().is_some_and(|c| c.is_ascii_digit()) {
        return crate::util::atoi(z) as u8 != 0;
    }
    // "on", "no", "off", "false", "yes", "true", "extra", "full": com `omitFull`
    // só entram os de valor 0 ou 1.
    const TABLE: [(&[u8], u8); 6] = [(b"on", 1), (b"no", 0), (b"off", 0), (b"false", 0), (b"yes", 1), (b"true", 1)];
    let z = cstr(z);
    for (name, value) in TABLE {
        if name.len() == z.len() && name.eq_ignore_ascii_case(z) {
            return value != 0;
        }
    }
    dflt
}

/// `sqlite3_uri_boolean`: `z_filename` nulo (`None`) devolve o padrão.
pub(crate) fn uri_boolean(z_filename: Option<&[u8]>, z_param: &[u8], dflt: bool) -> bool {
    match z_filename.and_then(|f| uri_parameter(f, z_param)) {
        Some(z) => get_boolean(z, dflt),
        None => dflt,
    }
}

// ---------------------------------------------------------------------------
// Chamadas de sistema "robustas"
// ---------------------------------------------------------------------------

/// Monta o `struct flock` de uma trava de faixa.
fn flock_of(l_type: i16, start: i64, len: i64) -> Flock {
    Flock { l_type, whence: SEEK_SET, start, len, pid: 0 }
}

/// `fcntl(fd, F_SETLK, ...)`: trava POSIX do processo, sem bloquear.
fn set_lock(s: &dyn Syscalls, fd: Fd, l_type: i16, start: i64, len: i64) -> Result<(), Errno> {
    s.fcntl_lock(fd, LockCmd::Set, flock_of(l_type, start, len)).map(|_| ()).map_err(note)
}

/// `fcntl(fd, F_GETLK, ...)` com `F_WRLCK`: devolve o `l_type` que o kernel
/// preencheu (`F_UNLCK` quando nenhuma trava de outro dono conflita).
fn get_lock(s: &dyn Syscalls, fd: Fd, start: i64, len: i64) -> Result<i16, Errno> {
    s.fcntl_lock(fd, LockCmd::Get, flock_of(F_WRLCK, start, len)).map(|f| f.l_type).map_err(note)
}

/// Repete uma syscall que falhou com `EINTR`.
fn retry<T>(mut f: impl FnMut() -> Result<T, Errno>) -> Result<T, Errno> {
    loop {
        match f() {
            Err(Errno::EINTR) => continue,
            Err(e) => return Err(note(e)),
            Ok(v) => return Ok(v),
        }
    }
}

/// `robust_open`: repete em `EINTR`, nunca devolve um descritor menor que 3 e
/// acerta as permissões de arquivo novo para `m` quando ele é dado.
fn robust_open(s: &dyn Syscalls, z: &[u8], f: OFlags, m: u32) -> Result<Fd, Errno> {
    let m2 = if m != 0 { m } else { DEFAULT_FILE_PERMISSIONS };
    let fd = loop {
        let fd = retry(|| s.openat(Fd::CWD, z, f | OFlags::CLOEXEC, m2))?;
        if fd.0 >= MINIMUM_FILE_DESCRIPTOR {
            break fd;
        }
        if f.contains(OFlags::EXCL | OFlags::CREAT) {
            let _ = s.unlinkat(Fd::CWD, z, AtFlags::empty());
        }
        let _ = s.close(fd);
        // O descritor aberto aqui fica de propósito: ele ocupa o número baixo.
        if let Err(e) = s.openat(Fd::CWD, b"/dev/null", OFlags::RDONLY, m) {
            return Err(note(e));
        }
    };
    if m != 0
        && let Ok(st) = s.fstat(fd)
        && st.size == 0
        && (st.mode & 0o777) != m
    {
        let _ = s.fchmod(fd, m);
    }
    Ok(fd)
}

/// `robust_ftruncate`.
fn robust_ftruncate(s: &dyn Syscalls, fd: Fd, sz: i64) -> Result<(), Errno> {
    retry(|| s.ftruncate(fd, sz as u64))
}

/// `robust_close`: o erro só seria logado, então é descartado.
fn robust_close(s: &dyn Syscalls, fd: Fd) {
    if let Err(e) = s.close(fd) {
        note(e);
    }
}

/// `robustFchown`: só root chama `fchown`.
fn robust_fchown(s: &dyn Syscalls, fd: Fd, uid: u32, gid: u32) {
    if s.geteuid() == 0 {
        let _ = s.fchownat(fd, b"", Some(uid), Some(gid), AtFlags::EMPTY_PATH);
    }
}

/// `sqliteErrorFromPosixError`.
fn sqlite_error_from_posix_error(e: Errno, sqlite_io_err: i32) -> i32 {
    match e {
        Errno::EACCES | Errno::EAGAIN | Errno::ETIMEDOUT | Errno::EBUSY | Errno::EINTR | Errno::ENOLCK => {
            SQLITE_BUSY
        }
        Errno::EPERM => SQLITE_PERM,
        _ => sqlite_io_err,
    }
}

/// `full_fsync(fd, fullSync, dataOnly)` no Linux: `fdatasync`, que o `sysabi`
/// oferece como `fsync` (o tamanho do arquivo vai junto nos dois).
fn full_fsync(s: &dyn Syscalls, fd: Fd) -> Result<(), Errno> {
    s.fsync(fd).map_err(note)
}

/// `openDirectory`: abre o diretório que contém `z_filename` (para o `fsync`
/// do diretório). Erro é `SQLITE_CANTOPEN`.
fn open_directory(s: &dyn Syscalls, z_filename: &[u8]) -> Result<Fd, i32> {
    let n = z_filename.len().min((MAX_PATHNAME - 1) as usize);
    let mut z_dirname: Vec<u8> = z_filename[..n].to_vec();
    let mut ii = z_dirname.len();
    while ii > 0 && z_dirname.get(ii) != Some(&b'/') {
        ii -= 1;
    }
    if ii > 0 {
        z_dirname.truncate(ii);
    } else {
        if z_dirname.first() != Some(&b'/') {
            z_dirname = b".".to_vec();
        } else {
            z_dirname.truncate(1);
        }
    }
    robust_open(s, &z_dirname, OFlags::RDONLY, 0).map_err(|_| SQLITE_CANTOPEN)
}

// ---------------------------------------------------------------------------
// Estado global: unixInodeInfo, UnixUnusedFd e unixShmNode
// ---------------------------------------------------------------------------

/// `UnixUnusedFd`: descritor que o SQLite fechou mas que o `close()` soltaria
/// as travas POSIX do processo; espera o último `unlock`.
struct UnusedFd {
    fd: Fd,
    /// `flags & (SQLITE_OPEN_READONLY|SQLITE_OPEN_READWRITE)` do `open` original.
    flags: i32,
}

/// `unixFileId` mais o pid do pseudo-processo dono da `inodeList`.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FileId {
    pid: i32,
    dev: u64,
    ino: u64,
}

/// `unixShm`: a conexão de memória compartilhada de um `unixFile`.
pub(crate) struct ShmConn {
    /// O `unixShm.id` (único dentro do nó).
    pub(crate) id: u32,
    /// Máscara das travas compartilhadas que a conexão tem.
    pub(crate) shared_mask: u16,
    /// Máscara das travas exclusivas que a conexão tem.
    pub(crate) excl_mask: u16,
}

/// `unixShmNode`: a memória compartilhada (o arquivo `-shm`) de um inode.
pub(crate) struct ShmNode {
    /// `zFilename`: caminho do `-shm`.
    pub(crate) z_filename: Vec<u8>,
    /// `hShm`: descritor do `-shm` (`None` é o modo `heap-memory` do `unix-excl`).
    pub(crate) h_shm: Option<Fd>,
    /// `szRegion`.
    pub(crate) sz_region: i32,
    /// `nRegion`.
    pub(crate) n_region: u16,
    /// `isReadonly`.
    pub(crate) is_readonly: bool,
    /// `isUnlocked`: a trava DMS ainda não foi tomada.
    pub(crate) is_unlocked: bool,
    /// As regiões em memória do modo `heap-memory` (`apRegion` sem arquivo).
    pub(crate) heap: Vec<Vec<u8>>,
    /// `nRef`: quantas conexões usam o nó.
    pub(crate) n_ref: i32,
    /// A lista `pFirst` de conexões.
    pub(crate) conns: Vec<ShmConn>,
    /// Próximo `unixShm.id`.
    pub(crate) next_conn: u32,
    /// `aLock[SQLITE_SHM_NLOCK]`: travas compartilhadas por posição (-1 é exclusiva).
    pub(crate) a_lock: [i32; SQLITE_SHM_NLOCK as usize],
}

/// `unixInodeInfo`.
pub(crate) struct InodeInfo {
    /// Identifica o objeto para os `unixFile` que apontam para ele.
    handle: u64,
    file_id: FileId,
    /// `nShared`: travas SHARED mantidas.
    n_shared: i32,
    /// `nLock`: travas de arquivo pendentes.
    n_lock: i32,
    /// `eFileLock`.
    e_file_lock: i32,
    /// `bProcessLock`: há uma trava exclusiva de processo (`unix-excl`).
    b_process_lock: bool,
    /// `pUnused`.
    unused: Vec<UnusedFd>,
    /// `nRef`.
    n_ref: i32,
    /// `pShmNode`.
    pub(crate) shm_node: Option<ShmNode>,
}

/// O estado protegido pelo `unixBigLock`: a `inodeList`.
pub(crate) struct Globals {
    /// Mais novo primeiro, como a lista encadeada do C.
    inodes: Vec<InodeInfo>,
    next_handle: u64,
}

static GLOBALS: Mutex<Globals> = Mutex::new(Globals { inodes: Vec::new(), next_handle: 1 });

/// Cede a CPU do pseudo-processo (ou da thread, fora de um).
fn yield_cpu() {
    match sysabi::sys::try_current() {
        Some(s) => s.sched_yield(),
        None => std::thread::yield_now(),
    }
}

/// `unixEnterMutex`: pega o mutex global sem bloquear a thread no hospedeiro.
pub(crate) fn lock_globals() -> MutexGuard<'static, Globals> {
    loop {
        match GLOBALS.try_lock() {
            Ok(g) => return g,
            Err(TryLockError::Poisoned(p)) => return p.into_inner(),
            Err(TryLockError::WouldBlock) => yield_cpu(),
        }
    }
}

impl Globals {
    /// O `unixInodeInfo` de um `unixFile` (invariante: existe enquanto o arquivo
    /// o referencia).
    pub(crate) fn inode(&mut self, handle: u64) -> &mut InodeInfo {
        self.inodes
            .iter_mut()
            .find(|i| i.handle == handle)
            .expect("unixInodeInfo referenciado por um unixFile aberto")
    }
}

/// `closePendingFds`.
fn close_pending_fds(s: &dyn Syscalls, inode: &mut InodeInfo) {
    for u in inode.unused.drain(..) {
        robust_close(s, u.fd);
    }
}

/// `findInodeInfo`: o `unixInodeInfo` do descritor, criando um se preciso.
fn find_inode_info(
    g: &mut Globals,
    s: &dyn Syscalls,
    fd: Fd,
    last_errno: &mut i32,
) -> Result<(u64, u64), i32> {
    let st = match s.fstat(fd) {
        Ok(st) => st,
        Err(e) => {
            *last_errno = note(e).0;
            return Err(SQLITE_IOERR);
        }
    };
    let file_id = FileId { pid: s.getpid(), dev: st.dev, ino: st.ino };
    if let Some(i) = g.inodes.iter_mut().find(|i| i.file_id == file_id) {
        i.n_ref += 1;
        return Ok((i.handle, file_id.ino));
    }
    let handle = g.next_handle;
    g.next_handle += 1;
    g.inodes.insert(
        0,
        InodeInfo {
            handle,
            file_id,
            n_shared: 0,
            n_lock: 0,
            e_file_lock: 0,
            b_process_lock: false,
            unused: Vec::new(),
            n_ref: 1,
            shm_node: None,
        },
    );
    Ok((handle, file_id.ino))
}

/// `releaseInodeInfo`: solta uma referência; na última fecha os descritores
/// pendentes e tira o objeto da lista.
fn release_inode_info(g: &mut Globals, s: &dyn Syscalls, handle: u64) {
    let Some(pos) = g.inodes.iter().position(|i| i.handle == handle) else { return };
    g.inodes[pos].n_ref -= 1;
    if g.inodes[pos].n_ref == 0 {
        debug_assert!(g.inodes[pos].shm_node.is_none());
        close_pending_fds(s, &mut g.inodes[pos]);
        g.inodes.remove(pos);
    }
}

/// `findReusableFd`: um descritor do banco `path` que ficou pendente com as
/// mesmas flags de abertura.
fn find_reusable_fd(g: &mut Globals, s: &dyn Syscalls, path: &[u8], flags: i32) -> Option<UnusedFd> {
    if g.inodes.is_empty() {
        return None;
    }
    let st = s.fstatat(Fd::CWD, path, AtFlags::empty()).ok()?;
    let pid = s.getpid();
    let inode = g.inodes.iter_mut().find(|i| i.file_id.pid == pid && i.file_id.dev == st.dev && i.file_id.ino == st.ino)?;
    let flags = flags & (SQLITE_OPEN_READONLY | SQLITE_OPEN_READWRITE);
    let pos = inode.unused.iter().position(|u| u.flags == flags)?;
    Some(inode.unused.remove(pos))
}

// ---------------------------------------------------------------------------
// unixFile
// ---------------------------------------------------------------------------

/// Qual dos `sqlite3_io_methods` de trava o arquivo usa.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockStyle {
    /// `posixIoMethods`: travas de faixa POSIX (`unix` e `unix-excl`).
    Posix,
    /// `nolockIoMethods` (`unix-none`, e todo arquivo que não é o banco principal).
    NoLock,
    /// `dotlockIoMethods` (`unix-dotfile`).
    DotLock,
}

/// `unixFile`: um arquivo aberto pelo VFS do Unix.
pub struct UnixFile {
    /// `pVfs->zName`.
    vfs_name: &'static [u8],
    style: LockStyle,
    /// `pInode`: o handle do `unixInodeInfo`.
    pub(crate) inode: Option<u64>,
    /// `pInode->fileId.ino`, que não muda enquanto o arquivo o referencia.
    ino: u64,
    /// `h`: o descritor (`None` é `h == -1`).
    pub(crate) h: Option<Fd>,
    /// `eFileLock`.
    e_file_lock: i32,
    /// `ctrlFlags`.
    ctrl_flags: u16,
    /// `lastErrno`.
    last_errno: i32,
    /// `lockingContext` do dotlock: o caminho do diretório `.lock`.
    locking_context: Option<Vec<u8>>,
    /// `pPreallocatedUnused`: as flags de abertura quando o banco principal tem
    /// um `UnixUnusedFd` reservado.
    unused_flags: Option<i32>,
    /// `zPath`: o `sqlite3_filename` (caminho, NUL e pares de URI). `None` é
    /// arquivo temporário.
    pub(crate) z_path: Option<Vec<u8>>,
    /// A conexão de memória compartilhada (`pShm`): o `unixShm.id`.
    pub(crate) shm_conn: Option<u32>,
    /// `szChunk`.
    sz_chunk: i32,
    /// `mmapSizeMax`.
    mmap_size_max: i64,
    /// `sectorSize`.
    sector_size: i32,
    /// `deviceCharacteristics`.
    device_characteristics: i32,
}

impl UnixFile {
    /// O descritor; depois do `close` é um `Fd` inválido, que o kernel recusa com EBADF.
    pub(crate) fn fd(&self) -> Fd {
        self.h.unwrap_or(Fd(-1))
    }

    /// O caminho do arquivo (a string C de `zPath`).
    pub(crate) fn path(&self) -> &[u8] {
        match &self.z_path {
            Some(z) => cstr(z),
            None => b"",
        }
    }

    /// `storeLastErrno`.
    fn store_last_errno(&mut self, e: Errno) {
        self.last_errno = e.0;
    }

    /// `fileHasMoved`: o arquivo foi renomeado ou apagado desde que foi aberto.
    fn file_has_moved(&self, s: &dyn Syscalls) -> bool {
        self.inode.is_some()
            && match s.fstatat(Fd::CWD, self.path(), AtFlags::empty()) {
                Ok(st) => st.ino != self.ino,
                Err(_) => true,
            }
    }

    /// `closeUnixFile`: fecha o descritor e zera o objeto.
    fn close_unix_file(&mut self, s: &dyn Syscalls) -> i32 {
        if let Some(h) = self.h.take() {
            robust_close(s, h);
        }
        self.inode = None;
        self.shm_conn = None;
        self.e_file_lock = 0;
        self.ctrl_flags = 0;
        self.locking_context = None;
        self.unused_flags = None;
        self.z_path = None;
        self.sz_chunk = 0;
        self.mmap_size_max = 0;
        self.sector_size = 0;
        self.device_characteristics = 0;
        SQLITE_OK
    }

    /// `setDeviceCharacteristics` (a versão que não é do QNX).
    fn set_device_characteristics(&mut self) {
        if self.sector_size == 0 {
            if self.ctrl_flags & UNIXFILE_PSOW != 0 {
                self.device_characteristics |= SQLITE_IOCAP_POWERSAFE_OVERWRITE;
            }
            self.sector_size = SQLITE_DEFAULT_SECTOR_SIZE as i32;
        }
    }

    /// `seekAndRead`: lê `buf.len()` bytes de `offset`; devolve quantos leu, ou
    /// -1 em erro (e grava `lastErrno`).
    fn seek_and_read(&mut self, s: &dyn Syscalls, mut offset: i64, buf: &mut [u8]) -> isize {
        let mut pos = 0usize;
        let mut cnt = buf.len();
        let mut prior: isize = 0;
        let mut got: isize;
        loop {
            match s.pread(self.fd(), &mut buf[pos..pos + cnt], offset as u64) {
                Ok(n) => got = n as isize,
                Err(Errno::EINTR) => continue,
                Err(e) => {
                    prior = 0;
                    self.store_last_errno(note(e));
                    got = -1;
                    break;
                }
            }
            if got == cnt as isize {
                break;
            }
            if got > 0 {
                cnt -= got as usize;
                offset += got as i64;
                prior += got;
                pos += got as usize;
            }
            if got <= 0 {
                break;
            }
        }
        got + prior
    }

    /// `seekAndWriteFd`: escreve até `buf.len()` bytes em `offset`; devolve
    /// quantos escreveu, ou -1 em erro (o errno vai em `err`).
    fn seek_and_write_fd(s: &dyn Syscalls, fd: Fd, offset: i64, buf: &[u8], err: &mut i32) -> isize {
        let n_buf = buf.len() & 0x1ffff;
        match retry(|| s.pwrite(fd, &buf[..n_buf], offset as u64)) {
            Ok(n) => n as isize,
            Err(e) => {
                *err = e.0;
                -1
            }
        }
    }

    /// `unixModeBit`.
    fn unix_mode_bit(&mut self, mask: u16, arg: &mut FileControlArg) -> i32 {
        let FileControlArg::Int(v) = arg else { return SQLITE_MISUSE };
        if *v < 0 {
            *v = (self.ctrl_flags & mask != 0) as i32;
        } else if *v == 0 {
            self.ctrl_flags &= !mask;
        } else {
            self.ctrl_flags |= mask;
        }
        SQLITE_OK
    }

    /// `fcntlSizeHint`: o mapeamento está desligado, então o que sobra do ramo
    /// de `mmap` é o `ftruncate` que estende o arquivo até `nByte`.
    fn fcntl_size_hint(&mut self, s: &dyn Syscalls, n_byte: i64) -> i32 {
        if self.sz_chunk > 0 {
            let st = match s.fstat(self.fd()) {
                Ok(st) => st,
                Err(e) => {
                    note(e);
                    return SQLITE_IOERR_FSTAT;
                }
            };
            let sz_chunk = self.sz_chunk as i64;
            let n_size = ((n_byte + sz_chunk - 1) / sz_chunk) * sz_chunk;
            if n_size > st.size as i64 {
                // posix_fallocate(): zero em sucesso, número do erro em falha.
                let r = loop {
                    match s.fallocate(self.fd(), FallocFlags::empty(), st.size as i64, n_size - st.size as i64) {
                        Err(Errno::EINTR) => continue,
                        r => break r,
                    }
                };
                match r {
                    Ok(()) | Err(Errno::EINVAL) => {}
                    Err(Errno::EOPNOTSUPP) | Err(Errno::ENOSYS) => {
                        // A glibc recorre a escrever um byte em cada bloco: é a
                        // mesma técnica do ramo sem posix_fallocate do C.
                        let n_blk = (st.blksize as i64).max(1);
                        let mut i_write = (st.size as i64 / n_blk) * n_blk + n_blk - 1;
                        while i_write < n_size + n_blk - 1 {
                            if i_write >= n_size {
                                i_write = n_size - 1;
                            }
                            let mut err = 0;
                            if Self::seek_and_write_fd(s, self.fd(), i_write, b"\0", &mut err) != 1 {
                                self.last_errno = err;
                                return SQLITE_IOERR_WRITE;
                            }
                            i_write += n_blk;
                        }
                    }
                    Err(e) => {
                        note(e);
                        return SQLITE_IOERR_WRITE;
                    }
                }
            }
        }
        if self.mmap_size_max > 0 && n_byte > 0 {
            // `nByte > mmapSize` com o mapeamento desligado (mmapSize == 0).
            if self.sz_chunk <= 0 {
                if let Err(e) = robust_ftruncate(s, self.fd(), n_byte) {
                    self.store_last_errno(e);
                    return SQLITE_IOERR_TRUNCATE;
                }
            }
            // unixMapfile(): não há mapeamento a criar.
        }
        SQLITE_OK
    }

    // ----- posix: unixLock, posixUnlock, unixClose ------------------------

    /// `unixFileLock`: `F_SETLK`, ou a trava exclusiva única do `unix-excl`.
    fn unix_file_lock(
        s: &dyn Syscalls,
        fd: Fd,
        ctrl_flags: u16,
        inode: &mut InodeInfo,
        l_type: i16,
        start: i64,
        len: i64,
    ) -> Result<(), Errno> {
        if ctrl_flags & (UNIXFILE_EXCL | UNIXFILE_RDONLY) == UNIXFILE_EXCL {
            if !inode.b_process_lock {
                debug_assert!(inode.n_lock == 0);
                set_lock(s, fd, F_WRLCK, SHARED_FIRST, SHARED_SIZE as i64)?;
                inode.b_process_lock = true;
                inode.n_lock += 1;
            }
            Ok(())
        } else {
            set_lock(s, fd, l_type, start, len)
        }
    }

    /// Corpo de `unixLock` com o mutex do inode já tomado (o `goto end_lock`
    /// do C vira `return`).
    fn lock_locked(&mut self, s: &dyn Syscalls, inode: &mut InodeInfo, e_file_lock: i32) -> i32 {
        let fd = self.fd();
        let ctrl = self.ctrl_flags;
        let mut rc = SQLITE_OK;

        // Outra thread deste processo tem uma trava incompatível por outro unixFile.
        if self.e_file_lock != inode.e_file_lock && (inode.e_file_lock >= PENDING_LOCK || e_file_lock > SHARED_LOCK) {
            return SQLITE_BUSY;
        }

        // SHARED pedida e outro unixFile já tem SHARED ou RESERVED: só conta.
        if e_file_lock == SHARED_LOCK && (inode.e_file_lock == SHARED_LOCK || inode.e_file_lock == RESERVED_LOCK) {
            debug_assert!(self.e_file_lock == 0);
            debug_assert!(inode.n_shared > 0);
            self.e_file_lock = SHARED_LOCK;
            inode.n_shared += 1;
            inode.n_lock += 1;
            return SQLITE_OK;
        }

        // Um PENDING antes do SHARED (depois solto) e antes do EXCLUSIVE.
        if e_file_lock == SHARED_LOCK || (e_file_lock == EXCLUSIVE_LOCK && self.e_file_lock == RESERVED_LOCK) {
            let l_type = if e_file_lock == SHARED_LOCK { F_RDLCK } else { F_WRLCK };
            match Self::unix_file_lock(s, fd, ctrl, inode, l_type, PENDING_BYTE, 1) {
                Err(e) => {
                    rc = sqlite_error_from_posix_error(e, SQLITE_IOERR_LOCK);
                    if rc != SQLITE_BUSY {
                        self.store_last_errno(e);
                    }
                    return rc;
                }
                Ok(()) => {
                    if e_file_lock == EXCLUSIVE_LOCK {
                        self.e_file_lock = PENDING_LOCK;
                        inode.e_file_lock = PENDING_LOCK;
                    }
                }
            }
        }

        // Daqui em diante há chamadas de sistema para a trava pedida.
        if e_file_lock == SHARED_LOCK {
            debug_assert!(inode.n_shared == 0);
            debug_assert!(inode.e_file_lock == 0);
            let mut t_errno: Option<Errno> = None;

            // A trava de leitura.
            if let Err(e) = Self::unix_file_lock(s, fd, ctrl, inode, F_RDLCK, SHARED_FIRST, SHARED_SIZE as i64) {
                t_errno = Some(e);
                rc = sqlite_error_from_posix_error(e, SQLITE_IOERR_LOCK);
            }

            // Solta o PENDING temporário.
            if let Err(e) = Self::unix_file_lock(s, fd, ctrl, inode, F_UNLCK, PENDING_BYTE, 1)
                && rc == SQLITE_OK
            {
                // Pode acontecer num ponto de montagem de rede.
                t_errno = Some(e);
                rc = SQLITE_IOERR_UNLOCK;
            }

            if rc != SQLITE_OK {
                if rc != SQLITE_BUSY
                    && let Some(e) = t_errno
                {
                    self.store_last_errno(e);
                }
                return rc;
            }
            self.e_file_lock = SHARED_LOCK;
            inode.n_lock += 1;
            inode.n_shared = 1;
        } else if e_file_lock == EXCLUSIVE_LOCK && inode.n_shared > 1 {
            // Outra thread deste processo ainda tem uma trava SHARED.
            rc = SQLITE_BUSY;
        } else {
            // RESERVED ou EXCLUSIVE, com SHARED ou mais já mantida.
            debug_assert!(self.e_file_lock != 0);
            debug_assert!(e_file_lock == RESERVED_LOCK || e_file_lock == EXCLUSIVE_LOCK);
            let (start, len) =
                if e_file_lock == RESERVED_LOCK { (RESERVED_BYTE, 1) } else { (SHARED_FIRST, SHARED_SIZE as i64) };
            if let Err(e) = Self::unix_file_lock(s, fd, ctrl, inode, F_WRLCK, start, len) {
                rc = sqlite_error_from_posix_error(e, SQLITE_IOERR_LOCK);
                if rc != SQLITE_BUSY {
                    self.store_last_errno(e);
                }
            }
        }

        if rc == SQLITE_OK {
            self.e_file_lock = e_file_lock;
            inode.e_file_lock = e_file_lock;
        }
        rc
    }

    /// `unixLock`.
    fn unix_lock(&mut self, e_file_lock: i32) -> i32 {
        // Já há uma trava deste tipo ou mais restritiva.
        if self.e_file_lock >= e_file_lock {
            return SQLITE_OK;
        }
        debug_assert!(self.e_file_lock != NO_LOCK || e_file_lock == SHARED_LOCK);
        debug_assert!(e_file_lock != PENDING_LOCK);
        debug_assert!(e_file_lock != RESERVED_LOCK || self.e_file_lock == SHARED_LOCK);
        let Some(handle) = self.inode else { return SQLITE_MISUSE };
        let s = sys();
        let mut g = lock_globals();
        let inode = g.inode(handle);
        self.lock_locked(&*s, inode, e_file_lock)
    }

    /// `posixUnlock` (sem o ramo do NFS do macOS): baixa a trava para `e_file_lock`
    /// (`NO_LOCK` ou `SHARED_LOCK`).
    fn posix_unlock(&mut self, e_file_lock: i32) -> i32 {
        debug_assert!(e_file_lock <= SHARED_LOCK);
        if self.e_file_lock <= e_file_lock {
            return SQLITE_OK;
        }
        let Some(handle) = self.inode else { return SQLITE_MISUSE };
        let s = sys();
        let s: &dyn Syscalls = &*s;
        let mut g = lock_globals();
        let inode = g.inode(handle);
        let fd = self.fd();
        let ctrl = self.ctrl_flags;
        let mut rc = SQLITE_OK;
        'end: {
            debug_assert!(inode.n_shared != 0);
            if self.e_file_lock > SHARED_LOCK {
                debug_assert!(inode.e_file_lock == self.e_file_lock);
                if e_file_lock == SHARED_LOCK
                    && let Err(e) = Self::unix_file_lock(s, fd, ctrl, inode, F_RDLCK, SHARED_FIRST, SHARED_SIZE as i64)
                {
                    // Em teoria não falha por trava incompatível de outro processo.
                    // Se falhar, o outro não segue o protocolo: IOERR_RDLOCK, não BUSY.
                    rc = SQLITE_IOERR_RDLOCK;
                    self.store_last_errno(e);
                    break 'end;
                }
                // PENDING_BYTE + 1 == RESERVED_BYTE: solta os dois de uma vez.
                match Self::unix_file_lock(s, fd, ctrl, inode, F_UNLCK, PENDING_BYTE, 2) {
                    Ok(()) => inode.e_file_lock = SHARED_LOCK,
                    Err(e) => {
                        rc = SQLITE_IOERR_UNLOCK;
                        self.store_last_errno(e);
                        break 'end;
                    }
                }
            }
            if e_file_lock == NO_LOCK {
                // Só solta a trava do sistema quando todas as threads do processo soltaram.
                inode.n_shared -= 1;
                if inode.n_shared == 0 {
                    match Self::unix_file_lock(s, fd, ctrl, inode, F_UNLCK, 0, 0) {
                        Ok(()) => inode.e_file_lock = NO_LOCK,
                        Err(e) => {
                            rc = SQLITE_IOERR_UNLOCK;
                            self.store_last_errno(e);
                            inode.e_file_lock = NO_LOCK;
                            self.e_file_lock = NO_LOCK;
                        }
                    }
                }
                // Com zero travas, fecha os descritores cujo close foi adiado.
                inode.n_lock -= 1;
                debug_assert!(inode.n_lock >= 0);
                if inode.n_lock == 0 {
                    close_pending_fds(s, inode);
                }
            }
        }
        drop(g);
        if rc == SQLITE_OK {
            self.e_file_lock = e_file_lock;
        }
        rc
    }

    /// `unixClose`.
    fn unix_close(&mut self) -> i32 {
        let s = sys();
        let s: &dyn Syscalls = &*s;
        let Some(handle) = self.inode else { return self.close_unix_file(s) };
        // verifyDbFile() só gera mensagens de log.
        self.posix_unlock(NO_LOCK);
        let mut g = lock_globals();
        if g.inode(handle).n_lock != 0
            && let Some(flags) = self.unused_flags.take()
            && let Some(h) = self.h.take()
        {
            // Há travas pendentes: fechar agora as soltaria. O descritor fica na
            // lista do inode e é fechado quando a última trava sair (`setPendingFd`).
            g.inode(handle).unused.insert(0, UnusedFd { fd: h, flags });
        }
        release_inode_info(&mut g, s, handle);
        debug_assert!(self.shm_conn.is_none());
        self.close_unix_file(s)
    }

    // ----- dotfile --------------------------------------------------------

    /// `dotlockLock`.
    fn dotlock_lock(&mut self, e_file_lock: i32) -> i32 {
        let s = sys();
        let s: &dyn Syscalls = &*s;
        let z_lock_file = self.locking_context.clone().unwrap_or_default();
        // Com qualquer trava o diretório já existe: só muda o registro interno.
        if self.e_file_lock > NO_LOCK {
            self.e_file_lock = e_file_lock;
            // Sempre atualiza a data do diretório.
            let _ = s.utimensat(Fd::CWD, &z_lock_file, SetTime::Now, SetTime::Now, AtFlags::empty());
            return SQLITE_OK;
        }
        // Pega a trava exclusiva.
        if let Err(e) = s.mkdirat(Fd::CWD, &z_lock_file, 0o777) {
            note(e);
            if e == Errno::EEXIST {
                return SQLITE_BUSY;
            }
            let rc = sqlite_error_from_posix_error(e, SQLITE_IOERR_LOCK);
            if rc != SQLITE_BUSY {
                self.store_last_errno(e);
            }
            return rc;
        }
        self.e_file_lock = e_file_lock;
        SQLITE_OK
    }

    /// `dotlockUnlock`.
    fn dotlock_unlock(&mut self, e_file_lock: i32) -> i32 {
        debug_assert!(e_file_lock <= SHARED_LOCK);
        if self.e_file_lock == e_file_lock {
            return SQLITE_OK;
        }
        // Rebaixar para SHARED é só o registro interno.
        if e_file_lock == SHARED_LOCK {
            self.e_file_lock = SHARED_LOCK;
            return SQLITE_OK;
        }
        // Destravar de vez apaga o diretório de trava.
        debug_assert!(e_file_lock == NO_LOCK);
        let s = sys();
        let z_lock_file = self.locking_context.clone().unwrap_or_default();
        if let Err(e) = s.unlinkat(Fd::CWD, &z_lock_file, AtFlags::REMOVEDIR) {
            note(e);
            if e == Errno::ENOENT {
                return SQLITE_OK;
            }
            self.store_last_errno(e);
            return SQLITE_IOERR_UNLOCK;
        }
        self.e_file_lock = NO_LOCK;
        SQLITE_OK
    }
}

impl VfsFile for UnixFile {
    fn i_version(&self) -> i32 {
        match self.style {
            LockStyle::Posix => 3,
            LockStyle::NoLock | LockStyle::DotLock => 1,
        }
    }

    /// `unixClose`, `nolockClose` ou `dotlockClose`.
    fn close(&mut self) -> i32 {
        match self.style {
            LockStyle::Posix => self.unix_close(),
            LockStyle::NoLock => {
                let s = sys();
                self.close_unix_file(&*s)
            }
            LockStyle::DotLock => {
                self.dotlock_unlock(NO_LOCK);
                self.locking_context = None;
                let s = sys();
                self.close_unix_file(&*s)
            }
        }
    }

    /// `unixRead`.
    fn read(&mut self, buf: &mut [u8], offset: i64) -> i32 {
        debug_assert!(offset >= 0);
        debug_assert!(!buf.is_empty());
        let s = sys();
        let amt = buf.len();
        let got = self.seek_and_read(&*s, offset, buf);
        if got == amt as isize {
            SQLITE_OK
        } else if got < 0 {
            // `lastErrno` foi gravado por `seek_and_read`. Alguns erros viram
            // IOERR_CORRUPTFS, que o `sqlite3ApiExit` converte em SQLITE_CORRUPT.
            match Errno(self.last_errno) {
                Errno::ERANGE | Errno::EIO | Errno::ENXIO => SQLITE_IOERR_CORRUPTFS,
                _ => SQLITE_IOERR_READ,
            }
        } else {
            self.last_errno = 0; // não é erro de sistema
            buf[got as usize..].fill(0); // o resto do buffer precisa ser zerado
            SQLITE_IOERR_SHORT_READ
        }
    }

    /// `unixWrite`.
    fn write(&mut self, buf: &[u8], offset: i64) -> i32 {
        debug_assert!(!buf.is_empty());
        let s = sys();
        let mut amt = buf.len() as isize;
        let mut offset = offset;
        let mut pos = 0usize;
        let mut wrote;
        loop {
            let mut err = self.last_errno;
            wrote = Self::seek_and_write_fd(&*s, self.fd(), offset, &buf[pos..], &mut err);
            self.last_errno = err;
            if !(wrote < amt && wrote > 0) {
                break;
            }
            amt -= wrote;
            offset += wrote as i64;
            pos += wrote as usize;
        }
        if amt > wrote {
            if wrote < 0 && self.last_errno != Errno::ENOSPC.0 {
                // `lastErrno` foi gravado por `seek_and_write_fd`.
                SQLITE_IOERR_WRITE
            } else {
                self.last_errno = 0; // não é erro de sistema
                SQLITE_FULL
            }
        } else {
            SQLITE_OK
        }
    }

    /// `unixTruncate`.
    fn truncate(&mut self, n_byte: i64) -> i32 {
        let s = sys();
        let mut n_byte = n_byte;
        // Com tamanho de bloco configurado o arquivo fica com um número inteiro
        // de blocos (pode ficar maior que o pedido).
        if self.sz_chunk > 0 {
            let c = self.sz_chunk as i64;
            n_byte = ((n_byte + c - 1) / c) * c;
        }
        match robust_ftruncate(&*s, self.fd(), n_byte) {
            Ok(()) => SQLITE_OK, // com mapeamento desligado não há mmapSize a reduzir
            Err(e) => {
                self.store_last_errno(e);
                SQLITE_IOERR_TRUNCATE
            }
        }
    }

    /// `unixSync`.
    fn sync(&mut self, flags: i32) -> i32 {
        debug_assert!((flags & 0x0F) == SQLITE_SYNC_NORMAL || (flags & 0x0F) == SQLITE_SYNC_FULL);
        let s = sys();
        let s: &dyn Syscalls = &*s;
        if let Err(e) = full_fsync(s, self.fd()) {
            self.store_last_errno(e);
            return SQLITE_IOERR_FSYNC;
        }
        // Também sincroniza o diretório que contém o arquivo, uma vez só.
        // Muitos sistemas não conseguem; o erro é ignorado.
        if self.ctrl_flags & UNIXFILE_DIRSYNC != 0 {
            if let Ok(dirfd) = open_directory(s, self.path()) {
                let _ = full_fsync(s, dirfd);
                robust_close(s, dirfd);
            }
            self.ctrl_flags &= !UNIXFILE_DIRSYNC;
        }
        SQLITE_OK
    }

    /// `unixFileSize`.
    fn file_size(&mut self, size: &mut i64) -> i32 {
        let s = sys();
        match s.fstat(self.fd()) {
            Err(e) => {
                self.store_last_errno(note(e));
                SQLITE_IOERR_FSTAT
            }
            Ok(st) => {
                *size = st.size as i64;
                // O tamanho 1 é relatado como zero (ticket #3260).
                if *size == 1 {
                    *size = 0;
                }
                SQLITE_OK
            }
        }
    }

    /// `unixLock`, `nolockLock` ou `dotlockLock`.
    fn lock(&mut self, lock_type: i32) -> i32 {
        match self.style {
            LockStyle::Posix => self.unix_lock(lock_type),
            LockStyle::NoLock => SQLITE_OK,
            LockStyle::DotLock => self.dotlock_lock(lock_type),
        }
    }

    /// `unixUnlock`, `nolockUnlock` ou `dotlockUnlock`.
    fn unlock(&mut self, lock_type: i32) -> i32 {
        match self.style {
            LockStyle::Posix => self.posix_unlock(lock_type),
            LockStyle::NoLock => SQLITE_OK,
            LockStyle::DotLock => self.dotlock_unlock(lock_type),
        }
    }

    /// `unixCheckReservedLock`, `nolockCheckReservedLock` ou `dotlockCheckReservedLock`.
    fn check_reserved_lock(&mut self, res_out: &mut i32) -> i32 {
        match self.style {
            LockStyle::NoLock => {
                *res_out = 0;
                SQLITE_OK
            }
            LockStyle::DotLock => {
                let s = sys();
                let z = self.locking_context.clone().unwrap_or_default();
                *res_out = s.faccessat(Fd::CWD, &z, AccessMode::empty(), AtFlags::empty()).is_ok() as i32;
                SQLITE_OK
            }
            LockStyle::Posix => {
                let Some(handle) = self.inode else { return SQLITE_MISUSE };
                debug_assert!(self.e_file_lock <= SHARED_LOCK);
                let s = sys();
                let mut rc = SQLITE_OK;
                let mut reserved = false;
                let mut g = lock_globals();
                let inode = g.inode(handle);
                // Uma thread deste processo tem a trava?
                if inode.e_file_lock > SHARED_LOCK {
                    reserved = true;
                }
                // Senão, algum outro processo?
                if !reserved && !inode.b_process_lock {
                    match get_lock(&*s, self.fd(), RESERVED_BYTE, 1) {
                        Err(e) => {
                            rc = SQLITE_IOERR_CHECKRESERVEDLOCK;
                            self.store_last_errno(e);
                        }
                        Ok(l_type) => {
                            if l_type != F_UNLCK {
                                reserved = true;
                            }
                        }
                    }
                }
                drop(g);
                *res_out = reserved as i32;
                rc
            }
        }
    }

    /// `unixFileControl`.
    fn file_control(&mut self, op: i32, arg: &mut FileControlArg) -> i32 {
        match op {
            SQLITE_FCNTL_LOCKSTATE => {
                *arg = FileControlArg::Int(self.e_file_lock);
                SQLITE_OK
            }
            SQLITE_FCNTL_LAST_ERRNO => {
                *arg = FileControlArg::Int(self.last_errno);
                SQLITE_OK
            }
            SQLITE_FCNTL_CHUNK_SIZE => match arg {
                FileControlArg::Int(v) => {
                    self.sz_chunk = *v;
                    SQLITE_OK
                }
                _ => SQLITE_MISUSE,
            },
            SQLITE_FCNTL_SIZE_HINT => match arg {
                FileControlArg::Int64(v) => {
                    let s = sys();
                    self.fcntl_size_hint(&*s, *v)
                }
                _ => SQLITE_MISUSE,
            },
            SQLITE_FCNTL_PERSIST_WAL => self.unix_mode_bit(UNIXFILE_PERSIST_WAL, arg),
            SQLITE_FCNTL_POWERSAFE_OVERWRITE => self.unix_mode_bit(UNIXFILE_PSOW, arg),
            SQLITE_FCNTL_VFSNAME => {
                *arg = FileControlArg::Text(self.vfs_name.to_vec());
                SQLITE_OK
            }
            SQLITE_FCNTL_TEMPFILENAME => {
                // O resultado do `unixGetTempname` é ignorado: em falha o nome é vazio.
                let s = sys();
                let name = unix_get_tempname(&*s, MAX_PATHNAME).unwrap_or_default();
                *arg = FileControlArg::Text(name);
                SQLITE_OK
            }
            SQLITE_FCNTL_HAS_MOVED => {
                let s = sys();
                *arg = FileControlArg::Int(self.file_has_moved(&*s) as i32);
                SQLITE_OK
            }
            SQLITE_FCNTL_MMAP_SIZE => {
                let FileControlArg::Int64(v) = arg else { return SQLITE_MISUSE };
                let mut new_limit = *v;
                let mx = MX_MMAP.load(Ordering::Relaxed);
                if new_limit > mx {
                    new_limit = mx;
                }
                *v = self.mmap_size_max;
                // Sem nenhum `xFetch` pendente e sem mapeamento ativo, trocar o
                // limite é só gravar o valor.
                if new_limit >= 0 && new_limit != self.mmap_size_max {
                    self.mmap_size_max = new_limit;
                }
                SQLITE_OK
            }
            SQLITE_FCNTL_EXTERNAL_READER => self.fcntl_external_reader(arg),
            _ => SQLITE_NOTFOUND,
        }
    }

    /// `unixSectorSize`.
    fn sector_size(&mut self) -> i32 {
        self.set_device_characteristics();
        self.sector_size
    }

    /// `unixDeviceCharacteristics`.
    fn device_characteristics(&mut self) -> i32 {
        self.set_device_characteristics();
        self.device_characteristics
    }

    /// `unixShmMap`.
    fn shm_map(&mut self, i_region: i32, sz_region: i32, b_extend: i32, pp: &mut Option<ShmRegion>) -> i32 {
        *pp = None;
        let s = sys();
        let s: &dyn Syscalls = &*s;
        // Se o arquivo de memória compartilhada ainda não foi aberto, abre agora.
        if self.shm_conn.is_none() {
            let rc = self.unix_open_shared_memory(s);
            if rc != SQLITE_OK {
                return rc;
            }
        }
        let (Some(handle), Some(_)) = (self.inode, self.shm_conn) else { return SQLITE_IOERR_SHMMAP };
        let n_shm_per_map = unix_shm_region_per_map();
        let mut g = lock_globals();
        let inode = g.inode(handle);
        let Some(node) = inode.shm_node.as_mut() else { return SQLITE_IOERR_SHMMAP };
        let mut rc = SQLITE_OK;
        'out: {
            if node.is_unlocked {
                rc = unix_lock_shared_memory(s, node);
                if rc != SQLITE_OK {
                    break 'out;
                }
                node.is_unlocked = false;
            }
            debug_assert!(sz_region == node.sz_region || node.n_region == 0);
            // Número mínimo de regiões que precisam estar mapeadas.
            let n_req_region = ((i_region + n_shm_per_map) / n_shm_per_map) * n_shm_per_map;
            if (node.n_region as i32) < n_req_region {
                let n_byte = n_req_region as i64 * sz_region as i64; // tamanho mínimo do arquivo
                node.sz_region = sz_region;
                if let Some(h_shm) = node.h_shm {
                    // A região pedida existe? (o arquivo -shm é grande o bastante)
                    let st = match s.fstat(h_shm) {
                        Ok(st) => st,
                        Err(e) => {
                            note(e);
                            rc = SQLITE_IOERR_SHMSIZE;
                            break 'out;
                        }
                    };
                    if (st.size as i64) < n_byte {
                        // Sem `bExtend` a região não existe: `pp` fica nulo.
                        if b_extend == 0 {
                            break 'out;
                        }
                        // Com `bExtend`, estende o arquivo escrevendo um byte no
                        // fim de cada página nova do sistema.
                        const PGSZ: i64 = 4096;
                        debug_assert!(n_byte % PGSZ == 0);
                        let mut i_pg = st.size as i64 / PGSZ;
                        while i_pg < n_byte / PGSZ {
                            let mut x = 0;
                            if Self::seek_and_write_fd(s, h_shm, i_pg * PGSZ + PGSZ - 1, b"\0", &mut x) != 1 {
                                rc = SQLITE_IOERR_SHMSIZE;
                                break 'out;
                            }
                            i_pg += 1;
                        }
                    }
                }
                // "Mapeia" as regiões: sem arquivo viram memória zerada; com
                // arquivo basta registrar que existem (o acesso é por pread/pwrite).
                while (node.n_region as i32) < n_req_region {
                    if node.h_shm.is_none() {
                        for _ in 0..n_shm_per_map {
                            node.heap.push(vec![0u8; sz_region as usize]);
                        }
                    }
                    node.n_region += n_shm_per_map as u16;
                }
            }
        }
        if (node.n_region as i32) > i_region {
            *pp = Some(ShmRegion { index: i_region, size: sz_region });
        }
        if node.is_readonly && rc == SQLITE_OK {
            rc = SQLITE_READONLY;
        }
        rc
    }

    /// Lê bytes de uma região de memória compartilhada (o acesso ao ponteiro
    /// mapeado do C).
    fn shm_read(&mut self, region: &ShmRegion, offset: usize, buf: &mut [u8]) -> i32 {
        let Some(handle) = self.inode else { return SQLITE_IOERR_SHMMAP };
        if self.shm_conn.is_none() || offset + buf.len() > region.size as usize {
            return SQLITE_IOERR_SHMMAP;
        }
        let s = sys();
        let base = region.index as i64 * region.size as i64 + offset as i64;
        let fd = {
            let mut g = lock_globals();
            let Some(node) = g.inode(handle).shm_node.as_mut() else { return SQLITE_IOERR_SHMMAP };
            match node.h_shm {
                Some(fd) => fd,
                None => {
                    let Some(r) = node.heap.get(region.index as usize) else { return SQLITE_IOERR_SHMMAP };
                    buf.copy_from_slice(&r[offset..offset + buf.len()]);
                    return SQLITE_OK;
                }
            }
        };
        let mut done = 0usize;
        while done < buf.len() {
            match retry(|| s.pread(fd, &mut buf[done..], (base + done as i64) as u64)) {
                Ok(0) => {
                    // A região mapeada nunca é lida além do fim do arquivo: o que
                    // faltar lê como zero.
                    buf[done..].fill(0);
                    return SQLITE_OK;
                }
                Ok(n) => done += n,
                Err(_) => return SQLITE_IOERR_SHMMAP,
            }
        }
        SQLITE_OK
    }

    /// Escreve bytes numa região de memória compartilhada.
    fn shm_write(&mut self, region: &ShmRegion, offset: usize, data: &[u8]) -> i32 {
        let Some(handle) = self.inode else { return SQLITE_IOERR_SHMMAP };
        if self.shm_conn.is_none() || offset + data.len() > region.size as usize {
            return SQLITE_IOERR_SHMMAP;
        }
        let s = sys();
        let base = region.index as i64 * region.size as i64 + offset as i64;
        let fd = {
            let mut g = lock_globals();
            let Some(node) = g.inode(handle).shm_node.as_mut() else { return SQLITE_IOERR_SHMMAP };
            // O mapeamento do C é só de leitura quando o -shm foi aberto assim.
            if node.is_readonly {
                return SQLITE_READONLY;
            }
            match node.h_shm {
                Some(fd) => fd,
                None => {
                    let Some(r) = node.heap.get_mut(region.index as usize) else { return SQLITE_IOERR_SHMMAP };
                    r[offset..offset + data.len()].copy_from_slice(data);
                    return SQLITE_OK;
                }
            }
        };
        let mut done = 0usize;
        while done < data.len() {
            match retry(|| s.pwrite(fd, &data[done..], (base + done as i64) as u64)) {
                Ok(0) | Err(_) => return SQLITE_IOERR_SHMMAP,
                Ok(n) => done += n,
            }
        }
        SQLITE_OK
    }

    /// `unixShmLock`.
    fn shm_lock(&mut self, ofst: i32, n: i32, flags: i32) -> i32 {
        let (Some(handle), Some(conn_id)) = (self.inode, self.shm_conn) else { return SQLITE_IOERR_SHMLOCK };
        let mask = ((1u32 << (ofst + n)) - (1u32 << ofst)) as u16; // travas a tomar ou soltar
        debug_assert!(ofst >= 0 && ofst + n <= SQLITE_SHM_NLOCK);
        debug_assert!(n >= 1);
        debug_assert!(
            flags == (SQLITE_SHM_LOCK | SQLITE_SHM_SHARED)
                || flags == (SQLITE_SHM_LOCK | SQLITE_SHM_EXCLUSIVE)
                || flags == (SQLITE_SHM_UNLOCK | SQLITE_SHM_SHARED)
                || flags == (SQLITE_SHM_UNLOCK | SQLITE_SHM_EXCLUSIVE)
        );
        debug_assert!(n == 1 || (flags & SQLITE_SHM_EXCLUSIVE) != 0);
        let s = sys();
        let s: &dyn Syscalls = &*s;
        let mut g = lock_globals();
        let inode = g.inode(handle);
        let Some(node) = inode.shm_node.as_mut() else { return SQLITE_IOERR_SHMLOCK };
        let Some(ci) = node.conns.iter().position(|c| c.id == conn_id) else { return SQLITE_IOERR_SHMLOCK };
        let mut rc = SQLITE_OK;

        // Há trabalho a fazer em três casos: (a) destravar o que está travado,
        // (b) travar compartilhado o que ainda não está, (c) travar exclusivo.
        // O núcleo nunca pede exclusivo o que já tem.
        debug_assert!(flags != (SQLITE_SHM_EXCLUSIVE | SQLITE_SHM_LOCK) || node.conns[ci].excl_mask & mask == 0);
        let c = &node.conns[ci];
        if ((flags & SQLITE_SHM_UNLOCK) != 0 && ((c.excl_mask | c.shared_mask) & mask) != 0)
            || (flags == (SQLITE_SHM_SHARED | SQLITE_SHM_LOCK) && (c.shared_mask & mask) == 0)
            || flags == (SQLITE_SHM_EXCLUSIVE | SQLITE_SHM_LOCK)
        {
            let o = ofst as usize;
            if (flags & SQLITE_SHM_UNLOCK) != 0 {
                // Caso (a): destravar.
                let mut b_unlock = true;
                // Numa trava SHARED, outros clientes do processo podem ter a
                // mesma: então a trava POSIX não sai do descritor.
                if (flags & SQLITE_SHM_SHARED) != 0 {
                    debug_assert!(n == 1);
                    debug_assert!(node.a_lock[o] >= 1);
                    if node.a_lock[o] > 1 {
                        b_unlock = false;
                        node.a_lock[o] -= 1;
                        node.conns[ci].shared_mask &= !mask;
                    }
                }
                if b_unlock {
                    rc = unix_shm_system_lock(s, node.h_shm, F_UNLCK, ofst + UNIX_SHM_BASE, n);
                    if rc == SQLITE_OK {
                        for l in &mut node.a_lock[o..o + n as usize] {
                            *l = 0;
                        }
                        node.conns[ci].shared_mask &= !mask;
                        node.conns[ci].excl_mask &= !mask;
                    }
                }
            } else if (flags & SQLITE_SHM_SHARED) != 0 {
                // Caso (b): trava compartilhada.
                if node.a_lock[o] < 0 {
                    // Alguma conexão tem a exclusiva.
                    rc = SQLITE_BUSY;
                } else if node.a_lock[o] == 0 {
                    rc = unix_shm_system_lock(s, node.h_shm, F_RDLCK, ofst + UNIX_SHM_BASE, n);
                }
                // As travas compartilhadas locais.
                if rc == SQLITE_OK {
                    node.conns[ci].shared_mask |= mask;
                    node.a_lock[o] += 1;
                }
            } else {
                // Caso (c): trava exclusiva.
                debug_assert!(flags == (SQLITE_SHM_LOCK | SQLITE_SHM_EXCLUSIVE));
                debug_assert!(node.conns[ci].shared_mask & mask == 0);
                debug_assert!(node.conns[ci].excl_mask & mask == 0);
                // Nenhuma conexão irmã pode ter travas que bloqueiem esta.
                for ii in o..o + n as usize {
                    if node.a_lock[ii] != 0 {
                        rc = SQLITE_BUSY;
                        break;
                    }
                }
                // Pega a trava no sistema e só então atualiza os valores em memória.
                if rc == SQLITE_OK {
                    rc = unix_shm_system_lock(s, node.h_shm, F_WRLCK, ofst + UNIX_SHM_BASE, n);
                    if rc == SQLITE_OK {
                        node.conns[ci].excl_mask |= mask;
                        for ii in o..o + n as usize {
                            node.a_lock[ii] = -1;
                        }
                    }
                }
            }
        }
        rc
    }

    /// `unixShmBarrier`: o C toma e solta o mutex global por redundância.
    fn shm_barrier(&mut self) {
        drop(lock_globals());
    }

    /// `unixShmUnmap`.
    fn shm_unmap(&mut self, delete_flag: i32) -> i32 {
        let (Some(handle), Some(conn_id)) = (self.inode, self.shm_conn) else { return SQLITE_OK };
        let s = sys();
        let s: &dyn Syscalls = &*s;
        let mut g = lock_globals();
        let inode = g.inode(handle);
        let Some(node) = inode.shm_node.as_mut() else { return SQLITE_OK };
        // Tira a conexão do conjunto do nó.
        if let Some(ci) = node.conns.iter().position(|c| c.id == conn_id) {
            node.conns.remove(ci);
        }
        self.shm_conn = None;
        // Se a contagem de referências chegou a 0, fecha também o arquivo.
        debug_assert!(node.n_ref > 0);
        node.n_ref -= 1;
        if node.n_ref == 0 {
            if delete_flag != 0 && node.h_shm.is_some() {
                let _ = s.unlinkat(Fd::CWD, &node.z_filename, AtFlags::empty());
            }
            unix_shm_purge(s, inode);
        }
        SQLITE_OK
    }

    /// `unixFetch`: o mapeamento está desligado, o ponteiro é sempre nulo.
    fn fetch(&mut self, ofst: i64, amt: i32, pp: &mut Option<Vec<u8>>) -> i32 {
        let _ = (ofst, amt);
        *pp = None;
        SQLITE_OK
    }

    /// `unixUnfetch`: sem mapeamento não há o que soltar.
    fn unfetch(&mut self, ofst: i64, p: Option<Vec<u8>>) -> i32 {
        let _ = (ofst, p);
        SQLITE_OK
    }
}

// ---------------------------------------------------------------------------
// Shared memory (memória compartilhada do WAL)
//
// No C o arquivo `-shm` é mapeado com mmap e as regiões de 32 KB são ponteiros.
// Aqui as regiões são lidas e escritas com pread e pwrite no mesmo arquivo
// (coerentes entre conexões e processos pelo cache do kernel). O modo
// `heap-memory` do `unix-excl` (sem arquivo) guarda as regiões em `ShmNode::heap`.
// ---------------------------------------------------------------------------

/// `UNIX_SHM_BASE`: primeiro byte de trava do `-shm`.
const UNIX_SHM_BASE: i32 = (22 + SQLITE_SHM_NLOCK) * 4;

/// `UNIX_SHM_DMS`: o byte do "deadman switch".
const UNIX_SHM_DMS: i32 = UNIX_SHM_BASE + SQLITE_SHM_NLOCK;

/// `unixGetpagesize`: o tamanho de página do Linux simulado.
fn unix_getpagesize() -> i32 {
    4096
}

/// `unixShmRegionPerMap`: quantas regiões de 32 KB cada mapeamento cobre no mínimo.
fn unix_shm_region_per_map() -> i32 {
    let shmsz = 32 * 1024;
    let pgsz = unix_getpagesize();
    debug_assert!(((pgsz - 1) & pgsz) == 0);
    if pgsz < shmsz { 1 } else { pgsz / shmsz }
}

/// `unixShmSystemLock`: trava POSIX nos bytes `ofst..ofst+n` do `-shm`. Sem
/// arquivo (modo `heap-memory`) não há o que travar.
fn unix_shm_system_lock(s: &dyn Syscalls, h_shm: Option<Fd>, lock_type: i16, ofst: i32, n: i32) -> i32 {
    debug_assert!(
        (ofst == UNIX_SHM_DMS && n == 1) || (ofst >= UNIX_SHM_BASE && ofst + n <= UNIX_SHM_BASE + SQLITE_SHM_NLOCK)
    );
    // As travas compartilhadas nunca cobrem mais de um byte.
    debug_assert!(n == 1 || lock_type != F_RDLCK);
    debug_assert!(n >= 1 && n <= SQLITE_SHM_NLOCK);
    if let Some(fd) = h_shm
        && set_lock(s, fd, lock_type, ofst as i64, n as i64).is_err()
    {
        return SQLITE_BUSY;
    }
    SQLITE_OK
}

/// `unixShmPurge`: se o nó não tem mais conexões, fecha o `-shm` e o descarta.
fn unix_shm_purge(s: &dyn Syscalls, inode: &mut InodeInfo) {
    if let Some(p) = inode.shm_node.as_mut()
        && p.n_ref == 0
    {
        if let Some(h) = p.h_shm.take() {
            robust_close(s, h);
        }
        inode.shm_node = None;
    }
}

/// `unixLockSharedMemory`: toma a trava DMS do `-shm`. Com `readonly_shm=1` e
/// nenhum outro processo segurando, devolve `SQLITE_READONLY_CANTINIT` e marca
/// `isUnlocked`.
fn unix_lock_shared_memory(s: &dyn Syscalls, node: &mut ShmNode) -> i32 {
    let Some(h_shm) = node.h_shm else { return SQLITE_OK };
    let mut rc = SQLITE_OK;
    // Com F_GETLK vê as travas que outros processos têm no byte DMS. Se alguém
    // tem SHARED, este processo também pode e segue. Se ninguém tem nada, este é
    // o primeiro: toma a EXCLUSIVE, trunca o -shm e rebaixa para SHARED. Se
    // alguém tem EXCLUSIVE, devolve BUSY (quem chamou tenta de novo).
    let l_type = match get_lock(s, h_shm, UNIX_SHM_DMS as i64, 1) {
        Err(_) => {
            rc = SQLITE_IOERR_LOCK;
            F_RDLCK
        }
        Ok(t) => t,
    };
    if rc != SQLITE_OK {
        // IOERR_LOCK: nada mais a fazer.
    } else if l_type == F_UNLCK {
        if node.is_readonly {
            node.is_unlocked = true;
            rc = SQLITE_READONLY_CANTINIT;
        } else {
            rc = unix_shm_system_lock(s, node.h_shm, F_WRLCK, UNIX_SHM_DMS, 1);
            // O primeiro a anexar trunca o -shm para 3 bytes (menos que o
            // cabeçalho), como auxílio de depuração.
            if rc == SQLITE_OK && robust_ftruncate(s, h_shm, 3).is_err() {
                rc = SQLITE_IOERR_SHMOPEN;
            }
        }
    } else if l_type == F_WRLCK {
        rc = SQLITE_BUSY;
    }
    if rc == SQLITE_OK {
        rc = unix_shm_system_lock(s, node.h_shm, F_RDLCK, UNIX_SHM_DMS, 1);
    }
    rc
}

impl UnixFile {
    /// `unixOpenSharedMemory`: cria a conexão de memória compartilhada, e o nó
    /// (abrindo o `-shm`) se este for o primeiro uso do inode.
    fn unix_open_shared_memory(&mut self, s: &dyn Syscalls) -> i32 {
        debug_assert!(self.shm_conn.is_none());
        let Some(handle) = self.inode else { return SQLITE_IOERR_SHMOPEN };
        let mut g = lock_globals();
        let inode = g.inode(handle);
        let mut rc = SQLITE_OK;
        if inode.shm_node.is_none() {
            // As permissões do banco valem para um -shm novo.
            let st = match s.fstat(self.fd()) {
                Ok(st) => st,
                Err(e) => {
                    note(e);
                    return SQLITE_IOERR_FSTAT;
                }
            };
            let mut z_shm = self.path().to_vec();
            z_shm.extend_from_slice(b"-shm");
            inode.shm_node = Some(ShmNode {
                z_filename: z_shm.clone(),
                h_shm: None,
                sz_region: 0,
                n_region: 0,
                is_readonly: false,
                is_unlocked: false,
                heap: Vec::new(),
                n_ref: 0,
                conns: Vec::new(),
                next_conn: 0,
                a_lock: [0; SQLITE_SHM_NLOCK as usize],
            });
            if !inode.b_process_lock {
                let mode = st.mode & 0o777;
                let mut h_shm = None;
                if !uri_boolean(self.z_path.as_deref(), b"readonly_shm", false) {
                    h_shm = robust_open(s, &z_shm, OFlags::RDWR | OFlags::CREAT | OFlags::NOFOLLOW, mode).ok();
                }
                let mut is_readonly = false;
                if h_shm.is_none() {
                    match robust_open(s, &z_shm, OFlags::RDONLY | OFlags::NOFOLLOW, mode) {
                        Ok(h) => {
                            h_shm = Some(h);
                            is_readonly = true;
                        }
                        Err(_) => {
                            rc = SQLITE_CANTOPEN;
                        }
                    }
                }
                if rc == SQLITE_OK {
                    // Root faz o -shm do mesmo dono do banco, senão o dono original
                    // não consegue conectar.
                    if let Some(h) = h_shm {
                        robust_fchown(s, h, st.uid, st.gid);
                    }
                    if let Some(node) = inode.shm_node.as_mut() {
                        node.h_shm = h_shm;
                        node.is_readonly = is_readonly;
                        rc = unix_lock_shared_memory(s, node);
                    }
                }
                if rc != SQLITE_OK && rc != SQLITE_READONLY_CANTINIT {
                    // shm_open_err: libera o nó se for o caso.
                    unix_shm_purge(s, inode);
                    return rc;
                }
            }
        }
        // Faz da conexão nova uma filha do nó.
        let Some(node) = inode.shm_node.as_mut() else { return SQLITE_IOERR_SHMOPEN };
        let id = node.next_conn;
        node.next_conn += 1;
        node.n_ref += 1;
        node.conns.insert(0, ShmConn { id, shared_mask: 0, excl_mask: 0 });
        self.shm_conn = Some(id);
        rc
    }

    /// `unixFcntlExternalReader`: há leitores com transação aberta no modo WAL em
    /// outros processos? (`F_GETLK` nas travas de leitura 3 em diante.)
    fn fcntl_external_reader(&mut self, arg: &mut FileControlArg) -> i32 {
        let mut out = 0;
        let mut rc = SQLITE_OK;
        if let (Some(handle), Some(_)) = (self.inode, self.shm_conn) {
            let s = sys();
            let mut g = lock_globals();
            let h_shm = g.inode(handle).shm_node.as_ref().and_then(|n| n.h_shm);
            // Sem arquivo (`heap-memory`) o C chama fcntl com hShm == -1 e recebe EBADF.
            let fd = h_shm.unwrap_or(Fd(-1));
            match get_lock(&*s, fd, (UNIX_SHM_BASE + 3) as i64, (SQLITE_SHM_NLOCK - 3) as i64) {
                Err(_) => rc = SQLITE_IOERR_LOCK,
                Ok(t) => out = (t != F_UNLCK) as i32,
            }
        }
        *arg = FileControlArg::Int(out);
        rc
    }
}

// ---------------------------------------------------------------------------
// Métodos do sqlite3_vfs
// ---------------------------------------------------------------------------

/// `unixTempFileDir`: um diretório para arquivos temporários, ou `None`.
fn unix_temp_file_dir(s: &dyn Syscalls) -> Option<Vec<u8>> {
    let mut candidates: Vec<Vec<u8>> = Vec::new();
    if let Some(d) = TEMP_DIRECTORY.lock().unwrap_or_else(PoisonError::into_inner).clone() {
        candidates.push(d);
    }
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

/// A parte de `unixRandomness` que enche o buffer. Devolve o `nBuf` final.
fn fill_random(s: &dyn Syscalls, out: &mut [u8]) -> usize {
    out.fill(0);
    let mut n_buf = out.len();
    match robust_open(s, b"/dev/urandom", OFlags::RDONLY, 0) {
        Err(_) => {
            // Sem /dev/urandom: a hora e o pid.
            let t = s.clock_gettime(Clock::Realtime).map(|t| t.sec).unwrap_or(0);
            let pid = s.getpid();
            let mut seed = [0u8; 12];
            seed[..8].copy_from_slice(&t.to_ne_bytes());
            seed[8..].copy_from_slice(&pid.to_ne_bytes());
            let n = seed.len().min(out.len());
            out[..n].copy_from_slice(&seed[..n]);
            n_buf = n;
        }
        Ok(fd) => {
            let _ = retry(|| s.read(fd, out));
            robust_close(s, fd);
        }
    }
    n_buf
}

/// `unixGetTempname`: `<dir>/etilqs_<hex aleatório>` que ainda não existe, mais
/// o NUL duplo que o `sqlite3_uri_parameter` espera (não incluído no vetor).
/// O C sorteia com `sqlite3_randomness`; aqui vem direto do `xRandomness`.
fn unix_get_tempname(s: &dyn Syscalls, n_buf: i32) -> Result<Vec<u8>, i32> {
    let mut i_limit = 0;
    let Some(dir) = unix_temp_file_dir(s) else { return Err(SQLITE_IOERR_GETTEMPPATH) };
    loop {
        let mut r = [0u8; 8];
        fill_random(s, &mut r);
        let mut name = dir.clone();
        name.extend_from_slice(b"/");
        name.extend_from_slice(SQLITE_TEMP_FILE_PREFIX);
        name.extend_from_slice(format!("{:x}", u64::from_ne_bytes(r)).as_bytes());
        let truncated = name.len() as i32 >= n_buf - 1;
        let too_many = i_limit > 10;
        i_limit += 1;
        if truncated || too_many {
            return Err(SQLITE_ERROR);
        }
        if s.faccessat(Fd::CWD, &name, AccessMode::empty(), AtFlags::empty()).is_err() {
            return Ok(name);
        }
    }
}

/// `getFileMode`: modo, dono e grupo de `z_file`.
fn get_file_mode(s: &dyn Syscalls, z_file: &[u8]) -> Result<(u32, u32, u32), i32> {
    match s.fstatat(Fd::CWD, z_file, AtFlags::empty()) {
        Ok(st) => Ok((st.mode & 0o777, st.uid, st.gid)),
        Err(e) => {
            note(e);
            Err(SQLITE_IOERR_FSTAT)
        }
    }
}

/// `findCreateFileMode`: as permissões (e dono) com que criar o arquivo. Zero é
/// "padrão menos o umask". Journal e WAL herdam do banco.
fn find_create_file_mode(s: &dyn Syscalls, z_path_full: &[u8], flags: i32) -> Result<(u32, u32, u32), i32> {
    let z_path = cstr(z_path_full);
    if flags & (SQLITE_OPEN_WAL | SQLITE_OPEN_MAIN_JOURNAL) != 0 {
        // Deriva o caminho do banco de "<banco>-journal", "<banco>-wal" e das
        // variantes com número. Sem '-' (ou com ele na primeira posição) o modo fica 0.
        let mut n_db = z_path.len() as isize - 1;
        while n_db > 0 && z_path[n_db as usize] != b'.' {
            if z_path[n_db as usize] == b'-' {
                return get_file_mode(s, &z_path[..n_db as usize]);
            }
            n_db -= 1;
        }
    } else if flags & SQLITE_OPEN_DELETEONCLOSE != 0 {
        return Ok((0o600, 0, 0));
    } else if flags & SQLITE_OPEN_URI != 0 {
        // Banco aberto por URI: o parâmetro "modeof" nomeia um arquivo cujo modo,
        // dono e grupo são copiados.
        if let Some(z) = uri_parameter(z_path_full, b"modeof") {
            return get_file_mode(s, z);
        }
    }
    Ok((0, 0, 0))
}

/// `fillInUnixFile`: monta o `unixFile` de um descritor aberto.
fn fill_in_unix_file(
    vfs: &UnixVfs,
    s: &dyn Syscalls,
    h: Fd,
    z_filename: Option<Vec<u8>>,
    ctrl_flags: u16,
    unused_flags: Option<i32>,
) -> Result<UnixFile, i32> {
    // Arquivo temporário nunca é travado.
    debug_assert!(z_filename.is_some() || (ctrl_flags & UNIXFILE_NOLOCK) != 0);
    let mut new = UnixFile {
        vfs_name: vfs.name,
        style: LockStyle::NoLock,
        inode: None,
        ino: 0,
        h: Some(h),
        e_file_lock: 0,
        ctrl_flags,
        last_errno: 0,
        locking_context: None,
        unused_flags,
        z_path: z_filename,
        shm_conn: None,
        sz_chunk: 0,
        mmap_size_max: SZ_MMAP.load(Ordering::Relaxed),
        sector_size: 0,
        device_characteristics: 0,
    };
    let uri_name = if ctrl_flags & UNIXFILE_URI != 0 { new.z_path.as_deref() } else { None };
    if uri_boolean(uri_name, b"psow", SQLITE_POWERSAFE_OVERWRITE != 0) {
        new.ctrl_flags |= UNIXFILE_PSOW;
    }
    if vfs.name == b"unix-excl" {
        new.ctrl_flags |= UNIXFILE_EXCL;
    }
    // O "finder": arquivo sem trava usa `nolock`; senão vale o estilo do VFS.
    new.style = if ctrl_flags & UNIXFILE_NOLOCK != 0 { LockStyle::NoLock } else { vfs.style };
    match new.style {
        LockStyle::Posix => {
            let mut g = lock_globals();
            let mut last_errno = 0;
            match find_inode_info(&mut g, s, h, &mut last_errno) {
                Ok((handle, ino)) => {
                    new.inode = Some(handle);
                    new.ino = ino;
                }
                Err(rc) => {
                    // Fecha o descritor já, antes de soltar o mutex.
                    robust_close(s, h);
                    new.h = None;
                    new.last_errno = last_errno;
                    return Err(rc);
                }
            }
        }
        LockStyle::DotLock => {
            // O dotlock usa o caminho: guarda o do diretório de trava.
            let mut z_lock_file = cstr(new.z_path.as_deref().unwrap_or(b"")).to_vec();
            z_lock_file.extend_from_slice(DOTLOCK_SUFFIX);
            new.locking_context = Some(z_lock_file);
        }
        LockStyle::NoLock => {}
    }
    new.last_errno = 0;
    // verifyDbFile() só gera mensagens de log.
    Ok(new)
}

/// `unixOpen`.
fn unix_open(
    vfs: &UnixVfs,
    name: Option<&[u8]>,
    flags_in: i32,
    out_flags: &mut i32,
) -> Result<Box<dyn VfsFile>, i32> {
    let s = sys();
    let s: &dyn Syscalls = &*s;
    let mut flags = flags_in;
    let e_type = flags & 0x0FFF00; // tipo do arquivo a abrir
    let is_exclusive = flags & SQLITE_OPEN_EXCLUSIVE != 0;
    let is_delete = flags & SQLITE_OPEN_DELETEONCLOSE != 0;
    let is_create = flags & SQLITE_OPEN_CREATE != 0;
    let mut is_readonly = flags & SQLITE_OPEN_READONLY != 0;
    let is_readwrite = flags & SQLITE_OPEN_READWRITE != 0;

    // Journal novo (de super-journal, principal ou WAL): o diretório é
    // sincronizado no primeiro `unixSync`.
    let is_new_jrnl = is_create
        && (e_type == SQLITE_OPEN_SUPER_JOURNAL || e_type == SQLITE_OPEN_MAIN_JOURNAL || e_type == SQLITE_OPEN_WAL);

    debug_assert!((!is_readonly || !is_readwrite) && (is_readwrite || is_readonly));
    debug_assert!(!is_create || is_readwrite);
    debug_assert!(!is_exclusive || is_create);
    debug_assert!(!is_delete || is_create);

    // Banco principal, journal, WAL e super-journal nunca são apagados no close
    // nem são temporários. Nome nulo pede um arquivo temporário.
    let mut fd: Option<Fd> = None;
    let mut prealloc = false;
    // O `sqlite3_filename` inteiro (com os pares de URI) e o caminho puro.
    let z_full: Vec<u8>;
    let z_name: Vec<u8>;
    if e_type == SQLITE_OPEN_MAIN_DB {
        let Some(n) = name else { return Err(SQLITE_MISUSE) };
        z_full = n.to_vec();
        z_name = cstr(n).to_vec();
        let mut g = lock_globals();
        if let Some(u) = find_reusable_fd(&mut g, s, &z_name, flags) {
            fd = Some(u.fd);
        }
        drop(g);
        prealloc = true;
    } else if let Some(n) = name {
        z_full = n.to_vec();
        z_name = cstr(n).to_vec();
    } else {
        // Nome nulo: o chamador pede um arquivo temporário. Os nomes gerados
        // têm NUL duplo no fim para o `sqlite3_uri_parameter`.
        debug_assert!(is_delete && !is_new_jrnl);
        z_name = unix_get_tempname(s, MAX_PATHNAME)?;
        let mut f = z_name.clone();
        f.extend_from_slice(b"\0\0");
        z_full = f;
    }

    // As flags do open(). Valem mesmo sem chamar open(): ficam no arquivo.
    let mut open_flags = OFlags::empty();
    if is_readwrite {
        open_flags |= OFlags::RDWR;
    }
    if is_create {
        open_flags |= OFlags::CREAT;
    }
    if is_exclusive {
        open_flags |= OFlags::EXCL | OFlags::NOFOLLOW;
    }
    open_flags |= OFlags::NOFOLLOW;

    if fd.is_none() {
        let (open_mode, uid, gid) = find_create_file_mode(s, &z_full, flags)?;
        let mut rc = SQLITE_OK;
        match robust_open(s, &z_name, open_flags, open_mode) {
            Ok(f) => fd = Some(f),
            Err(e) => {
                if is_new_jrnl
                    && e == Errno::EACCES
                    && s.faccessat(Fd::CWD, &z_name, AccessMode::empty(), AtFlags::empty()).is_err()
                {
                    // Sem poder criar o journal por o diretório não ser gravável.
                    rc = SQLITE_READONLY_DIRECTORY;
                } else if e != Errno::EISDIR && is_readwrite {
                    // Sem acesso de escrita: tenta só leitura.
                    flags &= !(SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE);
                    open_flags.remove(OFlags::RDWR | OFlags::CREAT);
                    flags |= SQLITE_OPEN_READONLY;
                    is_readonly = true;
                    let mut g = lock_globals();
                    let reusable = find_reusable_fd(&mut g, s, &z_name, flags);
                    drop(g);
                    fd = match reusable {
                        Some(u) => Some(u.fd),
                        None => robust_open(s, &z_name, open_flags, open_mode).ok(),
                    };
                }
            }
        }
        let Some(f) = fd else {
            return Err(if rc == SQLITE_OK { SQLITE_CANTOPEN } else { rc });
        };
        // O dono do journal e do WAL deve ser o do banco (só root consegue).
        if open_mode != 0 && flags & (SQLITE_OPEN_WAL | SQLITE_OPEN_MAIN_JOURNAL) != 0 {
            robust_fchown(s, f, uid, gid);
        }
    }
    let Some(fd) = fd else { return Err(SQLITE_CANTOPEN) };
    *out_flags = flags;

    let unused_flags = if prealloc { Some(flags & (SQLITE_OPEN_READONLY | SQLITE_OPEN_READWRITE)) } else { None };

    if is_delete {
        // Apaga o nome já: o arquivo some no close, sem deixar rastro se o processo morrer.
        let _ = s.unlinkat(Fd::CWD, &z_name, AtFlags::empty());
    }

    // As flags de controle.
    let mut ctrl_flags = 0u16;
    if is_delete {
        ctrl_flags |= UNIXFILE_DELETE;
    }
    if is_readonly {
        ctrl_flags |= UNIXFILE_RDONLY;
    }
    if e_type != SQLITE_OPEN_MAIN_DB {
        ctrl_flags |= UNIXFILE_NOLOCK;
    }
    if is_new_jrnl {
        ctrl_flags |= UNIXFILE_DIRSYNC;
    }
    if flags & SQLITE_OPEN_URI != 0 {
        ctrl_flags |= UNIXFILE_URI;
    }

    // O `zPath` do arquivo é o nome original: nulo para temporário.
    let z_path = name.map(|n| n.to_vec());
    let file = fill_in_unix_file(vfs, s, fd, z_path, ctrl_flags, unused_flags)?;
    Ok(Box::new(file))
}

/// `DbPath`: o caminho absoluto em construção de `unixFullPathname`.
struct DbPath {
    /// Diferente de zero depois de qualquer erro.
    rc: i32,
    /// Quantos links simbólicos foram resolvidos.
    n_symlink: i32,
    /// O caminho (`zOut[..nUsed]`).
    z_out: Vec<u8>,
    /// Bytes disponíveis (`nOut`).
    n_out: i32,
}

/// `appendOnePathElement`.
fn append_one_path_element(s: &dyn Syscalls, p: &mut DbPath, z_name: &[u8]) {
    debug_assert!(!z_name.is_empty());
    if z_name[0] == b'.' {
        if z_name.len() == 1 {
            return;
        }
        if z_name[1] == b'.' && z_name.len() == 2 {
            if p.z_out.len() > 1 {
                debug_assert!(p.z_out[0] == b'/');
                while let Some(c) = p.z_out.pop() {
                    if c == b'/' {
                        break;
                    }
                }
            }
            return;
        }
    }
    if p.z_out.len() as i32 + z_name.len() as i32 + 2 >= p.n_out {
        p.rc = SQLITE_ERROR;
        return;
    }
    p.z_out.push(b'/');
    p.z_out.extend_from_slice(z_name);
    if p.rc == SQLITE_OK {
        match s.fstatat(Fd::CWD, &p.z_out, AtFlags::SYMLINK_NOFOLLOW) {
            Err(e) => {
                if e != Errno::ENOENT {
                    note(e);
                    p.rc = SQLITE_CANTOPEN;
                }
            }
            Ok(st) => {
                if st.file_type() == FileType::Symlink {
                    let n = p.n_symlink;
                    p.n_symlink += 1;
                    if n > SQLITE_MAX_SYMLINK as i32 {
                        p.rc = SQLITE_CANTOPEN;
                        return;
                    }
                    let lnk = match s.readlinkat(Fd::CWD, &p.z_out) {
                        Ok(l) if !l.is_empty() && l.len() < SQLITE_MAX_PATHLEN => l,
                        _ => {
                            p.rc = SQLITE_CANTOPEN;
                            return;
                        }
                    };
                    if lnk[0] == b'/' {
                        p.z_out.clear();
                    } else {
                        let keep = p.z_out.len() - (z_name.len() + 1);
                        p.z_out.truncate(keep);
                    }
                    append_all_path_elements(s, p, &lnk);
                }
            }
        }
    }
}

/// `appendAllPathElements`.
fn append_all_path_elements(s: &dyn Syscalls, p: &mut DbPath, z_path: &[u8]) {
    for comp in cstr(z_path).split(|&c| c == b'/') {
        if !comp.is_empty() {
            append_one_path_element(s, p, comp);
        }
    }
}

/// `unixCurrentTimeInt64`.
fn unix_current_time_int64(s: &dyn Syscalls, out: &mut i64) -> i32 {
    let now = s.clock_gettime(Clock::Realtime).map(|t| (t.sec, t.nsec as i64)).unwrap_or((0, 0));
    *out = UNIX_EPOCH_MS + 1000 * now.0 + now.1 / 1_000_000;
    SQLITE_OK
}

/// Nomes de `aSyscall[]` e se a entrada tem ponteiro (não nulo) no build do
/// Debian. As entradas nulas dependem de `HAVE_PREAD64`, `ioctl` etc.
const SYSCALL_NAMES: [(&[u8], bool); 29] = [
    (b"open", true),
    (b"close", true),
    (b"access", true),
    (b"getcwd", true),
    (b"stat", true),
    (b"fstat", true),
    (b"ftruncate", true),
    (b"fcntl", true),
    (b"read", true),
    (b"pread", true),
    (b"pread64", false),
    (b"write", true),
    (b"pwrite", true),
    (b"pwrite64", false),
    (b"fchmod", true),
    (b"fallocate", true),
    (b"unlink", true),
    (b"openDirectory", true),
    (b"mkdir", true),
    (b"rmdir", true),
    (b"fchown", true),
    (b"geteuid", true),
    (b"mmap", true),
    (b"munmap", true),
    (b"mremap", true),
    (b"getpagesize", true),
    (b"readlink", true),
    (b"lstat", true),
    (b"ioctl", false),
];

/// Valores postos por `xSetSystemCall`. Servem só ao registro: as chamadas de
/// sistema deste módulo vão sempre ao `sysabi`, então trocar o ponteiro não
/// muda o comportamento (nenhum ponteiro de função de C existe aqui).
static SYSCALL_OVERRIDE: Mutex<[Option<SyscallPtr>; 29]> = Mutex::new([None; 29]);

/// O VFS do Unix: `unix`, `unix-none`, `unix-dotfile` ou `unix-excl`.
pub struct UnixVfs {
    name: &'static [u8],
    style: LockStyle,
}

impl Vfs for UnixVfs {
    fn name(&self) -> &[u8] {
        self.name
    }

    fn max_pathname(&self) -> i32 {
        MAX_PATHNAME
    }

    /// `unixOpen`.
    fn open(&self, name: Option<&[u8]>, flags: i32, out_flags: &mut i32) -> Result<Box<dyn VfsFile>, i32> {
        unix_open(self, name, flags, out_flags)
    }

    /// `unixDelete`.
    fn delete(&self, name: &[u8], sync_dir: i32) -> i32 {
        let s = sys();
        let s: &dyn Syscalls = &*s;
        let z_path = cstr(name);
        if let Err(e) = retry(|| s.unlinkat(Fd::CWD, z_path, AtFlags::empty())) {
            return if e == Errno::ENOENT { SQLITE_IOERR_DELETE_NOENT } else { SQLITE_IOERR_DELETE };
        }
        let mut rc = SQLITE_OK;
        if (sync_dir & 1) != 0 {
            match open_directory(s, z_path) {
                Ok(fd) => {
                    if full_fsync(s, fd).is_err() {
                        rc = SQLITE_IOERR_DIR_FSYNC;
                    }
                    robust_close(s, fd);
                }
                Err(_) => {} // CANTOPEN: ignorado
            }
        }
        rc
    }

    /// `unixAccess`.
    fn access(&self, name: &[u8], flags: i32, res_out: &mut i32) -> i32 {
        let s = sys();
        let z_path = cstr(name);
        debug_assert!(flags == SQLITE_ACCESS_EXISTS || flags == SQLITE_ACCESS_READWRITE);
        if flags == SQLITE_ACCESS_EXISTS {
            *res_out = match s.fstatat(Fd::CWD, z_path, AtFlags::empty()) {
                Ok(st) => st.file_type() != FileType::Regular || st.size > 0,
                Err(_) => false,
            } as i32;
        } else {
            *res_out = s
                .faccessat(Fd::CWD, z_path, AccessMode::W_OK | AccessMode::R_OK, AtFlags::empty())
                .is_ok() as i32;
        }
        SQLITE_OK
    }

    /// `unixFullPathname`.
    fn full_pathname(&self, name: &[u8], n_out: i32, out: &mut Vec<u8>) -> i32 {
        let s = sys();
        let s: &dyn Syscalls = &*s;
        let z_path = cstr(name);
        let mut path = DbPath { rc: 0, n_symlink: 0, z_out: Vec::new(), n_out };
        if z_path.first() != Some(&b'/') {
            match s.getcwd() {
                Ok(cwd) if cwd.len() < SQLITE_MAX_PATHLEN => append_all_path_elements(s, &mut path, &cwd),
                _ => return SQLITE_CANTOPEN,
            }
        }
        append_all_path_elements(s, &mut path, z_path);
        out.extend_from_slice(&path.z_out);
        if path.rc != 0 || path.z_out.len() < 2 {
            return SQLITE_CANTOPEN;
        }
        if path.n_symlink != 0 {
            return SQLITE_OK_SYMLINK;
        }
        SQLITE_OK
    }

    /// `unixDlOpen`: o Linux simulado não tem carregador de objetos ELF para o
    /// SQLite. As falhas que o `dlopen` da glibc daria por arquivo ausente,
    /// curto ou que não é ELF saem com a mesma mensagem; um ELF válido é
    /// ADIADO (falha com a mensagem do `mmap` recusado).
    fn dl_open(&self, path: &[u8]) -> Option<DlHandle> {
        let s = sys();
        let s: &dyn Syscalls = &*s;
        let z = cstr(path);
        let mut msg = z.to_vec();
        match s.openat(Fd::CWD, z, OFlags::RDONLY | OFlags::CLOEXEC, 0) {
            Err(e) => {
                msg.extend_from_slice(b": cannot open shared object file: ");
                msg.extend_from_slice(e.message().as_bytes());
            }
            Ok(fd) => {
                let mut head = [0u8; 64];
                let n = s.read(fd, &mut head).unwrap_or(0);
                let _ = s.close(fd);
                if n < 64 {
                    msg.extend_from_slice(b": file too short");
                } else if &head[..4] != b"\x7fELF" {
                    msg.extend_from_slice(b": invalid ELF header");
                } else {
                    msg.extend_from_slice(b": failed to map segment from shared object");
                }
            }
        }
        DL_ERROR.with(|c| c.set(Some(msg)));
        None
    }

    /// `unixDlError`: a mensagem (e a limpa, como o `dlerror()`).
    fn dl_error(&self, n_byte: i32, out: &mut Vec<u8>) {
        if let Some(e) = DL_ERROR.with(|c| c.take()) {
            let n = e.len().min((n_byte - 1).max(0) as usize);
            out.extend_from_slice(&e[..n]);
        }
    }

    /// `unixDlSym`: nenhuma biblioteca é carregada, então não há símbolo.
    fn dl_sym(&self, handle: DlHandle, symbol: &[u8]) -> Option<DlSymbol> {
        let _ = (handle, symbol);
        None
    }

    /// `unixDlClose`.
    fn dl_close(&self, handle: DlHandle) {
        let _ = handle;
    }

    /// `unixRandomness`: devolve o `nBuf` usado.
    fn randomness(&self, out: &mut [u8]) -> i32 {
        let s = sys();
        fill_random(&*s, out) as i32
    }

    /// `unixSleep`.
    fn sleep(&self, micro: i32) -> i32 {
        let s = sys();
        let d = Duration::new((micro / 1_000_000) as u64, ((micro % 1_000_000) * 1000) as u32);
        let _ = s.nanosleep(d);
        micro
    }

    /// `unixCurrentTime`.
    fn current_time(&self, out: &mut f64) -> i32 {
        let s = sys();
        let mut i = 0i64;
        let rc = unix_current_time_int64(&*s, &mut i);
        *out = i as f64 / 86400000.0;
        rc
    }

    /// `unixGetLastError`: o `errno` (a mensagem não é escrita).
    fn get_last_error(&self, n_buf: i32, buf: &mut Vec<u8>) -> i32 {
        let _ = (n_buf, buf);
        LAST_ERRNO.with(|c| c.get())
    }

    /// `unixCurrentTimeInt64`.
    fn current_time_int64(&self, out: &mut i64) -> Option<i32> {
        let s = sys();
        Some(unix_current_time_int64(&*s, out))
    }

    /// `unixSetSystemCall`.
    fn set_system_call(&self, name: Option<&[u8]>, ptr: Option<SyscallPtr>) -> i32 {
        let mut table = SYSCALL_OVERRIDE.lock().unwrap_or_else(PoisonError::into_inner);
        match name {
            // Sem nome, restaura todas as chamadas ao padrão.
            None => {
                *table = [None; 29];
                SQLITE_OK
            }
            Some(n) => match SYSCALL_NAMES.iter().position(|(z, _)| *z == n) {
                Some(i) => {
                    table[i] = ptr;
                    SQLITE_OK
                }
                None => SQLITE_NOTFOUND,
            },
        }
    }

    /// `unixGetSystemCall`: o valor atual (o padrão é um identificador estável).
    fn get_system_call(&self, name: &[u8]) -> Option<SyscallPtr> {
        let table = SYSCALL_OVERRIDE.lock().unwrap_or_else(PoisonError::into_inner);
        SYSCALL_NAMES.iter().position(|(z, _)| *z == name).and_then(|i| table[i].or(SYSCALL_NAMES[i].1.then_some(i as u64 + 1)))
    }

    /// `unixNextSystemCall`.
    fn next_system_call(&self, name: Option<&[u8]>) -> Option<&'static [u8]> {
        let mut i: isize = -1;
        if let Some(n) = name {
            i = 0;
            while (i as usize) < SYSCALL_NAMES.len() - 1 {
                if SYSCALL_NAMES[i as usize].0 == n {
                    break;
                }
                i += 1;
            }
        }
        i += 1;
        while (i as usize) < SYSCALL_NAMES.len() {
            if SYSCALL_NAMES[i as usize].1 {
                return Some(SYSCALL_NAMES[i as usize].0);
            }
            i += 1;
        }
        None
    }
}

/// Os quatro VFS do Linux, na ordem do `aVfs[]` do C (a mesma instância em
/// toda chamada de `os_init`, como o array estático).
static VFS_TABLE: OnceLock<[Arc<UnixVfs>; 4]> = OnceLock::new();

/// `sqlite3_os_init`: registra `unix` (o padrão), `unix-none`, `unix-dotfile` e
/// `unix-excl`. O primeiro da lista é o padrão; os demais entram depois dele,
/// então a lista final é `unix`, `unix-excl`, `unix-dotfile`, `unix-none`.
pub fn os_init() -> i32 {
    let table = VFS_TABLE.get_or_init(|| {
        [
            Arc::new(UnixVfs { name: b"unix", style: LockStyle::Posix }),
            Arc::new(UnixVfs { name: b"unix-none", style: LockStyle::NoLock }),
            Arc::new(UnixVfs { name: b"unix-dotfile", style: LockStyle::DotLock }),
            Arc::new(UnixVfs { name: b"unix-excl", style: LockStyle::Posix }),
        ]
    });
    for (i, v) in table.iter().enumerate() {
        let vfs: VfsRef = v.clone();
        vfs_register(vfs, i == 0);
    }
    // O diretório temporário (`unixTempFileInit`) é lido do ambiente do
    // pseudo-processo no momento do uso, em `unix_temp_file_dir`.
    SQLITE_OK
}

/// `sqlite3_os_end`: no Unix não há nada a desfazer.
pub fn os_end() -> i32 {
    SQLITE_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_parameters_and_booleans() {
        let f: &[u8] = b"/tmp/a.db\0mode\0ro\0psow\0off\0\0";
        assert_eq!(uri_parameter(f, b"mode"), Some(&b"ro"[..]));
        assert_eq!(uri_parameter(f, b"psow"), Some(&b"off"[..]));
        assert_eq!(uri_parameter(f, b"nope"), None);
        assert!(!uri_boolean(Some(f), b"psow", true));
        assert!(uri_boolean(Some(f), b"nope", true));
        assert!(uri_boolean(None, b"psow", true));
        assert_eq!(uri_parameter(b"/tmp/a.db\0\0", b"mode"), None);
    }

    #[test]
    fn booleans_follow_get_safety_level() {
        assert!(get_boolean(b"on", false));
        assert!(get_boolean(b"YES", false));
        assert!(get_boolean(b"1", false));
        assert!(!get_boolean(b"0", true));
        assert!(!get_boolean(b"false", true));
        assert!(get_boolean(b"full", true));
        assert!(!get_boolean(b"full", false));
    }

    #[test]
    fn shm_lock_constants_match_the_c_asserts() {
        assert_eq!(UNIX_SHM_BASE, 120);
        assert_eq!(UNIX_SHM_DMS, 128);
        assert_eq!(unix_shm_region_per_map(), 1);
    }

    #[test]
    fn next_system_call_walks_the_non_null_entries() {
        let v = UnixVfs { name: b"unix", style: LockStyle::Posix };
        assert_eq!(v.next_system_call(None), Some(&b"open"[..]));
        assert_eq!(v.next_system_call(Some(b"pread")), Some(&b"write"[..]));
        assert_eq!(v.next_system_call(Some(b"lstat")), None);
        assert_eq!(v.next_system_call(Some(b"nao-existe")), None);
    }
}
