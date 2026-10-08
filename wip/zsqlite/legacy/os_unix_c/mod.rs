// Mesclado das partes traduzidas de os_unix_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----
use crate::prelude::*;

// Tradução de chunks/os_unix_c.000.c (cabeçalho do os_unix.c, 3.46.1).
//
// Decisões de `#ifdef` resolvidas para o Debian 13 em Linux x86_64:
//   SQLITE_ENABLE_LOCKING_STYLE = 0, SQLITE_MAX_MMAP_SIZE > 0, USE_PREAD = 1,
//   SQLITE_THREADSAFE = 1, HAVE_FCHMOD e HAVE_MREMAP definidos, sem VxWorks,
//   sem Apple, sem WASI, sem SQLITE_DEBUG, sem SQLITE_TEST.
// Constantes de cabeçalhos do sistema (O_RDONLY, O_CREAT, O_EXCL, O_CLOEXEC,
// errno, F_*LCK, PROT_*, MAP_*) vêm do prelude e não são redefinidas aqui.

/// Valores permitidos de `UnixFile::fs_flags` (só usado em MacOSX/msdos).
pub const SQLITE_FSFLAGS_IS_MSDOS: u32 = 0x1;

/// Permissões padrão ao criar um arquivo novo.
pub const SQLITE_DEFAULT_FILE_PERMISSIONS: u32 = 0o644;

/// Permissões padrão ao criar o diretório automático do proxy.
pub const SQLITE_DEFAULT_PROXYDIR_PERMISSIONS: u32 = 0o755;

/// Tamanho máximo de caminho suportado.
pub const MAX_PATHNAME: i32 = 512;

/// Número máximo de links simbólicos seguidos.
pub const SQLITE_MAX_SYMLINKS: i32 = 100;

/// `osGetpid(X)`: o pid do processo atual.
#[inline]
pub fn os_getpid() -> i32 {
    sys_getpid()
}

/// `IS_LOCK_ERROR(x)`: só grava `last_errno` se o código é um erro de verdade
/// e não um retorno normal esperado (SQLITE_BUSY ou SQLITE_OK).
#[inline]
pub fn is_lock_error(x: i32) -> bool {
    x != SQLITE_OK && x != SQLITE_BUSY
}

/// Descritor de arquivo que não pôde ser fechado de imediato depois que o
/// SQLite fechou o seu handle. Fica guardado até haver oportunidade de fechar
/// ou reaproveitar. A lista é de dono único (cada nó possui o próximo).
#[derive(Default)]
pub struct UnixUnusedFd {
    /// Descritor de arquivo a fechar.
    pub fd: i32,
    /// Flags com que o descritor foi aberto.
    pub flags: i32,
    /// Próximo descritor não usado do mesmo arquivo.
    pub p_next: Option<Box<UnixUnusedFd>>,
}

/// Subclasse de `sqlite3_file` específica das VFS unix.
/// Campos de `SQLITE_ENABLE_LOCKING_STYLE`, `__APPLE__`, `SQLITE_DEBUG`,
/// `SQLITE_TEST`, `SQLITE_ENABLE_SETLK_TIMEOUT` e VxWorks não existem nesta build.
#[derive(Default)]
pub struct UnixFile {
    /// Métodos de I/O do arquivo (sempre o primeiro campo no C).
    pub p_method: Option<&'static Sqlite3IoMethods>,
    /// A VFS que criou este `UnixFile`.
    pub p_vfs: Option<VfsRef>,
    /// Informação sobre os locks neste inode.
    pub p_inode: Option<UnixInodeInfoRef>,
    /// O descritor de arquivo.
    pub h: i32,
    /// O tipo de lock mantido neste descritor.
    pub e_file_lock: u8,
    /// Bits de comportamento (flags `UNIXFILE_*`).
    pub ctrl_flags: u16,
    /// O errno do unix da última falha de I/O.
    pub last_errno: i32,
    /// Estado específico do estilo de lock.
    pub locking_context: Option<Box<dyn std::any::Any>>,
    /// `UnixUnusedFd` pré-alocado.
    pub p_preallocated_unused: Option<Box<UnixUnusedFd>>,
    /// Nome do arquivo.
    pub z_path: Option<Vec<u8>>,
    /// Informação do segmento de memória compartilhada.
    pub p_shm: Option<std::rc::Rc<std::cell::RefCell<UnixShm>>>,
    /// Configurado por FCNTL_CHUNK_SIZE.
    pub sz_chunk: i32,
    /// Número de referências de xFetch em aberto.
    pub n_fetch_out: i32,
    /// Tamanho útil do mapeamento em `p_map_region`.
    pub mmap_size: i64,
    /// Tamanho real do mapeamento em `p_map_region`.
    pub mmap_size_actual: i64,
    /// Valor configurado por FCNTL_MMAP_SIZE.
    pub mmap_size_max: i64,
    /// Região mapeada em memória.
    pub p_map_region: Option<Vec<u8>>,
    /// Tamanho do setor do dispositivo.
    pub sector_size: i32,
    /// Características do dispositivo, pré-calculadas.
    pub device_characteristics: i32,
}

/// Valor de `UnixFile::ctrl_flags`: conexões de um processo só.
pub const UNIXFILE_EXCL: u16 = 0x01;
/// Valor de `UnixFile::ctrl_flags`: conexão somente leitura.
pub const UNIXFILE_RDONLY: u16 = 0x02;
/// Valor de `UnixFile::ctrl_flags`: modo WAL persistente.
pub const UNIXFILE_PERSIST_WAL: u16 = 0x04;
/// Valor de `UnixFile::ctrl_flags`: sincronização de diretório necessária.
pub const UNIXFILE_DIRSYNC: u16 = 0x08;
/// Valor de `UnixFile::ctrl_flags`: SQLITE_IOCAP_POWERSAFE_OVERWRITE.
pub const UNIXFILE_PSOW: u16 = 0x10;
/// Valor de `UnixFile::ctrl_flags`: apagar ao fechar.
pub const UNIXFILE_DELETE: u16 = 0x20;
/// Valor de `UnixFile::ctrl_flags`: o nome pode ter parâmetros de consulta.
pub const UNIXFILE_URI: u16 = 0x40;
/// Valor de `UnixFile::ctrl_flags`: sem locking de arquivo.
pub const UNIXFILE_NOLOCK: u16 = 0x80;

thread_local! {
    /// Pid de quando `xRandomness()` foi chamado. Se `xOpen()` rodar com outro
    /// pid, houve um fork e o PRNG é reiniciado.
    pub static RANDOMNESS_PID: std::cell::Cell<i32> = std::cell::Cell::new(0);
}

/// Macros ausentes em alguns sistemas (valores do Linux x86_64 com glibc).
pub const O_LARGEFILE: i32 = 0;
/// Não seguir link simbólico no componente final.
pub const O_NOFOLLOW: i32 = 0o400000;
/// Sem efeito no Linux.
pub const O_BINARY: i32 = 0;

/// Números mágicos de ioctl do Linux usados para controlar o F2FS.
pub const F2FS_IOCTL_MAGIC: u32 = 0xf5;
/// `_IO(F2FS_IOCTL_MAGIC, 1)`.
pub const F2FS_IOC_START_ATOMIC_WRITE: u32 = 0xf501;
/// `_IO(F2FS_IOCTL_MAGIC, 2)`.
pub const F2FS_IOC_COMMIT_ATOMIC_WRITE: u32 = 0xf502;
/// `_IO(F2FS_IOCTL_MAGIC, 3)`.
pub const F2FS_IOC_START_VOLATILE_WRITE: u32 = 0xf503;
/// `_IO(F2FS_IOCTL_MAGIC, 5)`.
pub const F2FS_IOC_ABORT_VOLATILE_WRITE: u32 = 0xf505;
/// `_IOR(F2FS_IOCTL_MAGIC, 12, u32)`.
pub const F2FS_IOC_GET_FEATURES: u32 = 0x8004f50c;
/// Bit de recurso de escrita atômica do F2FS.
pub const F2FS_FEATURE_ATOMIC_WRITE: u32 = 0x0004;

/// Invoca `open()` com a interface bem definida `(caminho, flags, modo)`.
/// É o valor padrão da entrada "open" da tabela de chamadas de sistema.
pub fn posix_open(z_file: &[u8], flags: i32, mode: i32) -> i32 {
    sys_open(z_file, flags, mode as u32)
}


// ---- part_001.rs ----
use crate::prelude::*;

// Tradução de chunks/os_unix_c.001.c.
// As referências adiante (`openDirectory`, `unixGetpagesize`) não existem em Rust.

/// Chamadas de sistema substituíveis em tempo de execução, para injeção de
/// falhas em testes e sandboxing. No C o valor é um ponteiro de função; aqui é
/// um token opaco (`Some(índice + 1)` para a implementação padrão) porque só a
/// identidade e o "nulo ou não" são observáveis por `xSetSystemCall`,
/// `xGetSystemCall` e `xNextSystemCall`.
pub type SyscallPtr = Option<usize>;

/// Uma entrada da tabela `aSyscall`.
pub struct UnixSyscall {
    /// Nome da chamada de sistema.
    pub z_name: &'static [u8],
    /// Valor atual.
    pub p_current: SyscallPtr,
    /// Valor padrão (começa nulo, como no C).
    pub p_default: SyscallPtr,
}

/// Índices das entradas de `aSyscall` (usados pelos `osXxx`).
pub const SYSCALL_OPEN: usize = 0;
pub const SYSCALL_CLOSE: usize = 1;
pub const SYSCALL_ACCESS: usize = 2;
pub const SYSCALL_GETCWD: usize = 3;
pub const SYSCALL_STAT: usize = 4;
pub const SYSCALL_FSTAT: usize = 5;
pub const SYSCALL_FTRUNCATE: usize = 6;
pub const SYSCALL_FCNTL: usize = 7;
pub const SYSCALL_READ: usize = 8;
pub const SYSCALL_PREAD: usize = 9;
pub const SYSCALL_PREAD64: usize = 10;
pub const SYSCALL_WRITE: usize = 11;
pub const SYSCALL_PWRITE: usize = 12;
pub const SYSCALL_PWRITE64: usize = 13;
pub const SYSCALL_FCHMOD: usize = 14;
pub const SYSCALL_FALLOCATE: usize = 15;
pub const SYSCALL_UNLINK: usize = 16;
pub const SYSCALL_OPEN_DIRECTORY: usize = 17;
pub const SYSCALL_MKDIR: usize = 18;
pub const SYSCALL_RMDIR: usize = 19;
pub const SYSCALL_FCHOWN: usize = 20;
pub const SYSCALL_GETEUID: usize = 21;
pub const SYSCALL_MMAP: usize = 22;
pub const SYSCALL_MUNMAP: usize = 23;
pub const SYSCALL_MREMAP: usize = 24;
pub const SYSCALL_GETPAGESIZE: usize = 25;
pub const SYSCALL_READLINK: usize = 26;
pub const SYSCALL_LSTAT: usize = 27;
pub const SYSCALL_IOCTL: usize = 28;

/// Nomes na ordem de `aSyscall` e se a entrada tem implementação nesta build.
/// Sem implementação (ponteiro nulo no C): `pread64` e `pwrite64` (USE_PREAD64
/// indefinido), `ioctl` (SQLITE_ENABLE_BATCH_ATOMIC_WRITE indefinido).
const SYSCALL_TABLE: [(&[u8], bool); 29] = [
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

static A_SYSCALL: std::sync::OnceLock<std::sync::Mutex<Vec<UnixSyscall>>> =
    std::sync::OnceLock::new();

/// Devolve a tabela global `aSyscall`, criando-a com os valores padrão.
pub fn a_syscall() -> std::sync::MutexGuard<'static, Vec<UnixSyscall>> {
    let table = A_SYSCALL.get_or_init(|| {
        std::sync::Mutex::new(
            SYSCALL_TABLE
                .iter()
                .enumerate()
                .map(|(i, (name, present))| UnixSyscall {
                    z_name: name,
                    p_current: if *present { Some(i + 1) } else { None },
                    p_default: None,
                })
                .collect(),
        )
    });
    table.lock().unwrap_or_else(|e| e.into_inner())
}

/// Evita chamar `fchown()` se não somos root: em alguns sistemas isso gera
/// mensagem em log de segurança para processos sem privilégio.
pub fn robust_fchown(fd: i32, uid: u32, gid: u32) -> i32 {
    if os_geteuid() != 0 {
        0
    } else {
        os_fchown(fd, uid, gid)
    }
}

/// Método `xSetSystemCall()` do `sqlite3_vfs` de todas as VFS "unix". Devolve
/// SQLITE_OK se atualizou, ou SQLITE_NOTFOUND se não há chamada com esse nome.
pub fn unix_set_system_call(z_name: Option<&[u8]>, p_new_func: SyscallPtr) -> i32 {
    let mut rc = SQLITE_NOTFOUND;
    let mut table = a_syscall();
    match z_name {
        None => {
            // Sem nome: restaura todas as chamadas para o padrão.
            rc = SQLITE_OK;
            for entry in table.iter_mut() {
                if entry.p_default.is_some() {
                    entry.p_current = entry.p_default;
                }
            }
        }
        Some(name) => {
            // Com nome: opera só na chamada indicada.
            for entry in table.iter_mut() {
                if name == entry.z_name {
                    if entry.p_default.is_none() {
                        entry.p_default = entry.p_current;
                    }
                    rc = SQLITE_OK;
                    let new_func = if p_new_func.is_none() { entry.p_default } else { p_new_func };
                    entry.p_current = new_func;
                    break;
                }
            }
        }
    }
    rc
}

/// Método `xGetSystemCall()`: valor de uma chamada de sistema. Devolve `None`
/// se o nome não é reconhecido ou se a chamada está indefinida no momento.
pub fn unix_get_system_call(z_name: &[u8]) -> SyscallPtr {
    let table = a_syscall();
    for entry in table.iter() {
        if z_name == entry.z_name {
            return entry.p_current;
        }
    }
    None
}

/// Método `xNextSystemCall()`: nome da primeira chamada depois de `z_name`
/// (ou da primeira, se `z_name` é `None`). Devolve `None` se `z_name` é a
/// última ou não é uma chamada válida.
pub fn unix_next_system_call(z_name: Option<&[u8]>) -> Option<&'static [u8]> {
    let table = a_syscall();
    let n = table.len() as i32;
    let mut i: i32 = -1;
    if let Some(name) = z_name {
        i = 0;
        while i < n - 1 {
            if name == table[i as usize].z_name {
                break;
            }
            i += 1;
        }
    }
    i += 1;
    while i < n {
        if table[i as usize].p_current.is_some() {
            return Some(table[i as usize].z_name);
        }
        i += 1;
    }
    None
}

/// Não aceita descritor menor que este valor, para não abrir o arquivo do
/// banco com descritores comumente usados por stdin, stdout e stderr.
pub const SQLITE_MINIMUM_FILE_DESCRIPTOR: i32 = 3;

/// Invoca `open()` repetidamente até ter sucesso ou falhar por motivo diferente
/// de EINTR.
///
/// Se o modo `m` é 0, usa SQLITE_DEFAULT_FILE_PERMISSIONS (0644) modificado
/// pelo umask. Se `m` não é 0, o modo é exatamente `m`, ignorando o umask: só é
/// diferente de zero para -wal, -journal e -shm, que precisam ter as mesmas
/// permissões do banco original para que um journal ativo deixado por uma
/// transação interrompida possa ser recuperado por quem consegue escrever.
pub fn robust_open(z: &[u8], f: i32, m: u32) -> i32 {
    let mut fd: i32;
    let m2: u32 = if m != 0 { m } else { SQLITE_DEFAULT_FILE_PERMISSIONS };
    loop {
        fd = os_open(z, f | O_CLOEXEC, m2);
        if fd < 0 {
            if get_errno() == EINTR {
                continue;
            }
            break;
        }
        if fd >= SQLITE_MINIMUM_FILE_DESCRIPTOR {
            break;
        }
        if (f & (O_EXCL | O_CREAT)) == (O_EXCL | O_CREAT) {
            let _ = os_unlink(z);
        }
        os_close(fd);
        sqlite3_log(
            SQLITE_WARNING,
            &[
                &b"attempt to open \""[..],
                z,
                &b"\" as file descriptor "[..],
                fd.to_string().as_bytes(),
            ]
            .concat(),
        );
        fd = -1;
        if os_open(b"/dev/null", O_RDONLY, m) < 0 {
            break;
        }
    }
    if fd >= 0 {
        if m != 0 {
            let mut statbuf = StatBuf::default();
            if os_fstat(fd, &mut statbuf) == 0
                && statbuf.st_size == 0
                && (statbuf.st_mode & 0o777) != m
            {
                os_fchmod(fd, m);
            }
        }
    }
    fd
}

thread_local! {
    /// Mutex global que protege os `UnixInodeInfo` deste arquivo, todos
    /// compartilháveis entre threads. Atribuído em `os_init`.
    /// Para evitar deadlock, `unixBigLock` deve ser adquirido antes do
    /// `p_lock_mutex` de um `UnixInodeInfo`, se os dois forem necessários.
    pub static UNIX_BIG_LOCK: std::cell::RefCell<Option<MutexRef>> =
        std::cell::RefCell::new(None);
}

/// Obtém o mutex global `unixBigLock`.
pub fn unix_enter_mutex() {
    let m = UNIX_BIG_LOCK.with(|m| m.borrow().clone());
    sqlite3_mutex_enter(m.as_ref());
}


// ---- part_002.rs ----
use crate::prelude::*;

// Tradução de chunks/os_unix_c.002.c.
// Fora da build: SQLITE_HAVE_OS_TRACE (`azFileLock`), SQLITE_LOCK_TRACE
// (`lockTrace`), SQLITE_DEBUG (`unixMutexHeld`), `__ANDROID__` em
// `robust_ftruncate` e todo o trecho `OS_VXWORKS` (`vxworksSimplifyName`,
// `vxworksFindFileId`, `vxworksReleaseFileId`).

/// Libera o mutex global `unixBigLock`.
pub fn unix_leave_mutex() {
    let m = UNIX_BIG_LOCK.with(|m| m.borrow().clone());
    sqlite3_mutex_leave(m.as_ref());
}

/// Repete as chamadas a `ftruncate()` que falham por EINTR. Toda chamada a
/// `ftruncate()` deste arquivo passa por este wrapper.
pub fn robust_ftruncate(h: i32, sz: i64) -> i32 {
    let mut rc: i32;
    loop {
        rc = os_ftruncate(h, sz);
        if !(rc < 0 && get_errno() == EINTR) {
            break;
        }
    }
    rc
}

/// Traduz um errno POSIX em algo útil para os clientes das funções do sqlite3:
/// uma variedade de erros de "tente de novo" vira SQLITE_BUSY, e uma variedade
/// de erros de "feche o descritor AGORA" vira SQLITE_IOERR.
///
/// Erros na inicialização de locks, ou no suporte do sistema de arquivos a
/// locks, devem tratar ENOLCK, ENOTSUP e EOPNOTSUPP separadamente.
pub fn sqlite_error_from_posix_error(posix_error: i32, sqlite_io_err: i32) -> i32 {
    match posix_error {
        // Erro aleatório de retry de NFS, exceto na introspecção do suporte do
        // sistema de arquivos, em que significa o que diz.
        EACCES | EAGAIN | ETIMEDOUT | EBUSY | EINTR | ENOLCK => SQLITE_BUSY,
        EPERM => SQLITE_PERM,
        _ => sqlite_io_err,
    }
}

/// Chave usada para localizar um `UnixInodeInfo`. O número do inode ocupa
/// sempre 64 bits, como no C (contorno para Android com `ino_t` de 32 bits).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct UnixFileId {
    /// Número do dispositivo.
    pub dev: u64,
    /// Número do inode.
    pub ino: u64,
}


// ---- part_003.rs ----
use crate::prelude::*;

// Tradução de chunks/os_unix_c.003.c.
// Fora da build: `unixFileMutexHeld`/`unixFileMutexNotheld` (SQLITE_DEBUG, só
// usadas em assert), o ramo `__APPLE__` de `findInodeInfo`, `sharedByte` e o
// ramo `OS_VXWORKS`. Os `assert()` somem como no C compilado com NDEBUG.

/// Referência compartilhada a um `UnixInodeInfo`.
pub type UnixInodeInfoRef = std::rc::Rc<std::cell::RefCell<UnixInodeInfo>>;

/// Alocado para cada inode aberto. Um inode pode ter vários descritores, então
/// cada `UnixFile` aponta para uma instância e este objeto conta quantos
/// `UnixFile` apontam para ele.
///
/// Regras de mutex:
///  (1) Só o `p_lock_mutex` precisa estar mantido para ler ou escrever os
///      campos de lock: `n_shared`, `n_lock`, `e_file_lock`, `b_process_lock`,
///      `p_unused`.
///  (2) Com `n_ref > 0`, `file_id` e `p_lock_mutex` não mudam e podem ser
///      lidos sem mutex.
///  (3) Fora essas exceções, os campos só se leem ou escrevem com o mutex
///      global `unixBigLock` mantido.
///
/// Prevenção de deadlock: `unixBigLock` não pode ser adquirido com
/// `p_lock_mutex` mantido; se os dois são necessários, `unixBigLock` vem antes.
///
/// Os campos `pNext` e `pPrev` do C viram a posição na lista global
/// `INODE_LIST` (lista de todos os `UnixInodeInfo`, cabeça primeiro).
#[derive(Default)]
pub struct UnixInodeInfo {
    /// A chave de busca.
    pub file_id: UnixFileId,
    /// Mutex a manter para os campos de lock.
    pub p_lock_mutex: Option<MutexRef>,
    /// Número de locks SHARED mantidos.
    pub n_shared: i32,
    /// Número de locks de arquivo em aberto.
    pub n_lock: i32,
    /// Um de SHARED_LOCK, RESERVED_LOCK etc.
    pub e_file_lock: u8,
    /// Há um lock exclusivo de processo.
    pub b_process_lock: u8,
    /// Descritores não usados a fechar.
    pub p_unused: Option<Box<UnixUnusedFd>>,
    /// Número de ponteiros para esta estrutura.
    pub n_ref: i32,
    /// Memória compartilhada associada a este inode.
    pub p_shm_node: Option<std::rc::Rc<std::cell::RefCell<UnixShmNode>>>,
}

thread_local! {
    /// Lista de todos os `UnixInodeInfo` (a inserção é na cabeça, índice 0).
    /// Só se lê ou escreve com `unixBigLock` mantido.
    pub static INODE_LIST: std::cell::RefCell<Vec<UnixInodeInfoRef>> =
        std::cell::RefCell::new(Vec::new());
}

/// Invocada, via a macro `unix_log_error!`, depois que um erro ocorre numa
/// função do SO com `errno` definido. Registra com `sqlite3_log()` o `errno` e,
/// se possível, o texto legível equivalente de `strerror_r()`.
///
/// O primeiro argumento é o código que será devolvido ao SQLite (por exemplo
/// SQLITE_IOERR_DELETE, SQLITE_CANTOPEN); os dois seguintes são o nome da
/// função do SO que falhou ("unlink", "open") e o caminho associado, se houver.
pub fn unix_log_error_at_line(
    errcode: i32,
    z_func: &[u8],
    z_path: Option<&[u8]>,
    i_line: i32,
) -> i32 {
    let i_errno = get_errno();
    // SQLITE_THREADSAFE e HAVE_STRERROR_R: usa strerror_r() (versão GNU).
    let z_err: Vec<u8> = os_strerror_r(i_errno);
    let z_path = z_path.unwrap_or(b"");
    sqlite3_log(
        errcode,
        &[
            &b"os_unix.c:"[..],
            i_line.to_string().as_bytes(),
            &b": ("[..],
            i_errno.to_string().as_bytes(),
            &b") "[..],
            z_func,
            &b"("[..],
            z_path,
            &b") - "[..],
            &z_err[..],
        ]
        .concat(),
    );
    errcode
}

/// Equivalente da macro `unixLogError(a,b,c)`: passa a linha do chamador.
#[macro_export]
macro_rules! unix_log_error {
    ($a:expr, $b:expr, $c:expr) => {
        $crate::os_unix_c::unix_log_error_at_line($a, $b, $c, line!() as i32)
    };
}

/// Fecha um descritor de arquivo.
///
/// Assume que `close()` quase sempre funciona. Se falhar, o descritor vaza,
/// mas o erro é registrado. Não é seguro repetir `close()` após EINTR, pois o
/// descritor pode já ter sido reutilizado por outra thread.
pub fn robust_close(p_file: Option<&UnixFile>, h: i32, lineno: i32) {
    if os_close(h) != 0 {
        let z_path = p_file.and_then(|f| f.z_path.as_deref());
        unix_log_error_at_line(SQLITE_IOERR_CLOSE, b"close", z_path, lineno);
    }
}

/// Define `p_file.last_errno`. Fica numa sub-rotina por ser um lugar
/// conveniente para um breakpoint.
pub fn store_last_errno(p_file: &mut UnixFile, error: i32) {
    p_file.last_errno = error;
}

/// Fecha todos os descritores acumulados na lista `p_unused` do inode.
pub fn close_pending_fds(p_file: &UnixFile) {
    let p_inode = match &p_file.p_inode {
        Some(p) => p.clone(),
        None => return,
    };
    let mut p = p_inode.borrow_mut().p_unused.take();
    while let Some(mut node) = p {
        p = node.p_next.take();
        robust_close(Some(p_file), node.fd, line!() as i32);
    }
}

/// Libera um `UnixInodeInfo` alocado antes por `find_inode_info()`.
///
/// O mutex global deve estar mantido na chamada, mas o mutex do inode sendo
/// apagado NÃO. Como no C, `p_file.p_inode` não é zerado aqui.
pub fn release_inode_info(p_file: &UnixFile) {
    let p_inode = match &p_file.p_inode {
        Some(p) => p.clone(),
        None => return,
    };
    let n_ref = {
        let mut inode = p_inode.borrow_mut();
        inode.n_ref -= 1;
        inode.n_ref
    };
    if n_ref == 0 {
        let lock_mutex = p_inode.borrow().p_lock_mutex.clone();
        sqlite3_mutex_enter(lock_mutex.as_ref());
        close_pending_fds(p_file);
        sqlite3_mutex_leave(lock_mutex.as_ref());
        INODE_LIST.with(|list| {
            let mut list = list.borrow_mut();
            if let Some(pos) = list.iter().position(|x| std::rc::Rc::ptr_eq(x, &p_inode)) {
                list.remove(pos);
            }
        });
        let m = p_inode.borrow_mut().p_lock_mutex.take();
        drop(lock_mutex);
        sqlite3_mutex_free(m);
    }
}

/// Dado um descritor de arquivo, localiza o `UnixInodeInfo` que o descreve,
/// criando um novo se necessário. O valor devolvido em `pp_inode` pode ficar
/// sem inicializar se ocorrer erro.
///
/// O mutex global deve estar mantido na chamada. Devolve um código de erro.
pub fn find_inode_info(p_file: &mut UnixFile, pp_inode: &mut Option<UnixInodeInfoRef>) -> i32 {
    // Informação de baixo nível do arquivo, usada para formar um nome único.
    let fd = p_file.h;
    let mut statbuf = StatBuf::default();
    let rc = os_fstat(fd, &mut statbuf);
    if rc != 0 {
        store_last_errno(p_file, get_errno());
        return SQLITE_IOERR;
    }

    let file_id = UnixFileId { dev: statbuf.st_dev, ino: statbuf.st_ino };
    let found = INODE_LIST.with(|list| {
        list.borrow().iter().find(|x| x.borrow().file_id == file_id).cloned()
    });
    let p_inode = match found {
        None => {
            let mut new_inode = UnixInodeInfo::default();
            new_inode.file_id = file_id;
            if sqlite3_global_config().b_core_mutex {
                new_inode.p_lock_mutex = sqlite3_mutex_alloc(SQLITE_MUTEX_FAST);
                if new_inode.p_lock_mutex.is_none() {
                    return SQLITE_NOMEM;
                }
            }
            new_inode.n_ref = 1;
            let p_inode = std::rc::Rc::new(std::cell::RefCell::new(new_inode));
            INODE_LIST.with(|list| list.borrow_mut().insert(0, p_inode.clone()));
            p_inode
        }
        Some(p_inode) => {
            p_inode.borrow_mut().n_ref += 1;
            p_inode
        }
    };
    *pp_inode = Some(p_inode);
    SQLITE_OK
}

/// Verdadeiro se `p_file` foi renomeado ou desvinculado desde que foi aberto.
pub fn file_has_moved(p_file: &UnixFile) -> bool {
    let p_inode = match &p_file.p_inode {
        Some(p) => p,
        None => return false,
    };
    let mut buf = StatBuf::default();
    os_stat(p_file.z_path.as_deref().unwrap_or(b""), &mut buf) != 0
        || buf.st_ino != p_inode.borrow().file_id.ino
}

/// Verifica um `UnixFile` que é banco de dados: (1) há exatamente um link
/// rígido, (2) não é link simbólico, (3) não foi renomeado nem desvinculado.
/// Emite `sqlite3_log(SQLITE_WARNING,...)` se algo não está certo.
pub fn verify_db_file(p_file: &UnixFile) {
    // Estas verificações valem só para o banco principal.
    if (p_file.ctrl_flags & UNIXFILE_NOLOCK) != 0 {
        return;
    }
    let z_path = p_file.z_path.as_deref().unwrap_or(b"");
    let mut buf = StatBuf::default();
    let rc = os_fstat(p_file.h, &mut buf);
    if rc != 0 {
        sqlite3_log(SQLITE_WARNING, &[&b"cannot fstat db file "[..], z_path].concat());
        return;
    }
    if buf.st_nlink == 0 {
        sqlite3_log(SQLITE_WARNING, &[&b"file unlinked while open: "[..], z_path].concat());
        return;
    }
    if buf.st_nlink > 1 {
        sqlite3_log(SQLITE_WARNING, &[&b"multiple links to file: "[..], z_path].concat());
        return;
    }
    if file_has_moved(p_file) {
        sqlite3_log(SQLITE_WARNING, &[&b"file renamed while open: "[..], z_path].concat());
    }
}


// ---- part_004.rs ----

/// Verifica se há um lock RESERVED mantido no arquivo especificado por este ou
/// qualquer outro processo. Se tal lock estiver mantido, define `*p_res_out` para um
/// valor diferente de zero, caso contrário `*p_res_out` é zero. O retorno é SQLITE_OK,
/// a menos que ocorra um erro de E/S durante a verificação do lock.
fn unix_check_reserved_lock(p_file: &mut UnixFile, p_res_out: &mut i32) -> i32 {
    let mut rc = SQLITE_OK;
    let mut reserved = 0;

    simulate_io_error! {
        return SQLITE_IOERR_CHECKRESERVEDLOCK;
    }

    debug_assert!(p_file.p_inode.is_some());
    debug_assert!((p_file.e_file_lock as i32) <= SHARED_LOCK);
    sqlite3_mutex_enter(&p_file.p_inode.as_ref().unwrap().p_lock_mutex);

    // Verifica se uma thread neste processo mantém tal lock
    if (p_file.p_inode.as_ref().unwrap().e_file_lock as i32) > SHARED_LOCK {
        reserved = 1;
    }

    // Caso contrário, vê se algum outro processo o mantém.
    if reserved == 0 && p_file.p_inode.as_ref().unwrap().b_process_lock == 0 {
        let mut lock = FLock::default();
        lock.l_whence = SEEK_SET;
        lock.l_start = reserved_byte();
        lock.l_len = 1;
        lock.l_type = F_WRLCK;
        if os_fcntl(p_file.h, F_GETLK, &mut lock) != 0 {
            rc = SQLITE_IOERR_CHECKRESERVEDLOCK;
            store_last_errno(p_file, errno());
        } else if lock.l_type != F_UNLCK {
            reserved = 1;
        }
    }

    sqlite3_mutex_leave(&p_file.p_inode.as_ref().unwrap().p_lock_mutex);
    ostrace!(("TEST WR-LOCK {} {} {} (unix)\n", p_file.h, rc, reserved));

    *p_res_out = reserved;
    rc
}

/// Define um POSIX advisory lock com uma tentativa não bloqueante. A variante com
/// `SQLITE_ENABLE_SETLK_TIMEOUT` não existe na compilação do Debian e foi omitida.
#[inline]
fn os_set_posix_advisory_lock(h: i32, p_lock: &mut FLock) -> i32 {
    os_fcntl(h, F_SETLK, p_lock)
}

/// Tenta definir um lock de sistema no arquivo `p_file`. O lock é descrito por `p_lock`.
///
/// Se `p_file` foi aberto para leitura e escrita a partir de unix-excl, o único lock
/// jamais obtido é um exclusivo, obtido exatamente uma vez na primeira tentativa de lock.
/// Todas as operações de lock de sistema seguintes viram no-ops. As operações de lock
/// continuam acontecendo internamente, para coordenar o acesso entre conexões separadas
/// dentro deste processo, mas isso é tratado em memória e o sistema operacional não
/// participa.
///
/// A função é um pass-through para fcntl(F_SETLK) se `p_file` usa qualquer VFS que não
/// seja "unix-excl", ou se foi aberto em "unix-excl" e é somente leitura.
///
/// Devolve zero se a chamada termina com sucesso, ou -1 se fcntl() falha, caso em que
/// errno fica definido.
fn unix_file_lock(p_file: &mut UnixFile, p_lock: &mut FLock) -> i32 {
    let rc: i32;
    debug_assert!(p_file.p_inode.is_some());
    debug_assert!(sqlite3_mutex_held(&p_file.p_inode.as_ref().unwrap().p_lock_mutex) != 0);
    if (p_file.ctrl_flags & (UNIXFILE_EXCL | UNIXFILE_RDONLY)) == UNIXFILE_EXCL {
        if p_file.p_inode.as_ref().unwrap().b_process_lock == 0 {
            debug_assert!(p_file.p_inode.as_ref().unwrap().n_lock == 0);
            let mut lock = FLock::default();
            lock.l_whence = SEEK_SET;
            lock.l_start = shared_first();
            lock.l_len = SHARED_SIZE as i64;
            lock.l_type = F_WRLCK;
            rc = os_set_posix_advisory_lock(p_file.h, &mut lock);
            if rc < 0 {
                return rc;
            }
            p_file.p_inode.as_mut().unwrap().b_process_lock = 1;
            p_file.p_inode.as_mut().unwrap().n_lock += 1;
        } else {
            rc = 0;
        }
    } else {
        rc = os_set_posix_advisory_lock(p_file.h, p_lock);
    }
    rc
}

/// Trava o arquivo com o lock especificado por `e_file_lock`, um entre:
///
///     (1) SHARED_LOCK
///     (2) RESERVED_LOCK
///     (3) PENDING_LOCK
///     (4) EXCLUSIVE_LOCK
///
/// Às vezes, ao pedir um estado de lock, estados intermediários são inseridos. O lock
/// pode falhar numa das transições posteriores, deixando o estado diferente do inicial,
/// mas ainda aquém do objetivo. Transições permitidas e estados intermediários:
///
///    UNLOCKED -> SHARED
///    SHARED -> RESERVED
///    SHARED -> EXCLUSIVE
///    RESERVED -> (PENDING) -> EXCLUSIVE
///    PENDING -> EXCLUSIVE
///
/// A rotina só aumenta um lock. Use `os_unlock()` para baixar o nível.
fn unix_lock(p_file: &mut UnixFile, e_file_lock: i32) -> i32 {
    // O seguinte descreve a implementação dos vários locks e transições em termos dos
    // primitivos de lock advisory compartilhado e exclusivo do POSIX (chamados read-locks
    // e write-locks abaixo, para evitar confusão com os nomes de lock do SQLite). Os
    // algoritmos são um pouco complicados para serem compatíveis com sistemas Windows95
    // acessando simultaneamente o mesmo arquivo, caso isso um dia seja necessário.
    //
    // Os símbolos definidos em os.h identificam o 'pending byte' e o 'reserved byte',
    // bytes únicos em offsets conhecidos, e o 'shared byte range', um intervalo de 510
    // bytes em um offset conhecido.
    //
    // Para obter um lock SHARED, obtém-se um read-lock no 'pending byte'. Se der certo,
    // o 'shared byte range' recebe read-lock e o lock do 'pending byte' é liberado.
    // (Nota de legado: quando o SQLite foi criado, o Windows95 ainda era comum e não tem
    // lock compartilhado. Lá, um único byte escolhido ao acaso do 'shared byte range' é
    // travado. A solução persiste por compatibilidade retroativa.)
    //
    // Um processo só pode obter um lock RESERVED depois de ter um SHARED. O RESERVED é
    // implementado com um write-lock no 'reserved byte'.
    //
    // Um lock EXCLUSIVE só pode ser pedido depois de se manter um SHARED ou RESERVED. É
    // implementado com um write-lock em todo o 'shared byte range'. Como todos os outros
    // locks exigem um read-lock num byte desse intervalo, isso garante que nenhum outro
    // lock esteja mantido no banco.
    //
    // Se um processo com lock RESERVED pede um EXCLUSIVE, um lock PENDING é obtido antes.
    // Ele é implementado com um write-lock no 'pending byte', o que impede novos SHARED,
    // mas deixa os existentes persistirem. Se a chamada falhar em obter o EXCLUSIVE, ela
    // mantém o PENDING. O cliente pode tentar o EXCLUSIVE de novo depois que os SHARED
    // existentes forem liberados.
    let mut rc = SQLITE_OK;
    let mut lock = FLock::default();
    let mut t_errno = 0;

    debug_assert!(p_file.p_inode.is_some());
    ostrace!((
        "LOCK    {} {} was {}({},{}) pid={} (unix)\n",
        p_file.h,
        az_file_lock(e_file_lock),
        az_file_lock(p_file.e_file_lock as i32),
        az_file_lock(p_file.p_inode.as_ref().unwrap().e_file_lock as i32),
        p_file.p_inode.as_ref().unwrap().n_shared,
        os_getpid(0)
    ));

    // Se já existe um lock deste tipo ou mais restritivo no UnixFile, não faz nada. Não
    // usa a saída end_lock porque unix_enter_mutex() ainda não foi chamado.
    if (p_file.e_file_lock as i32) >= e_file_lock {
        ostrace!((
            "LOCK    {} {} ok (already held) (unix)\n",
            p_file.h,
            az_file_lock(e_file_lock)
        ));
        return SQLITE_OK;
    }

    // Garante que a sequência de locks está correta.
    //  (1) Nunca se vai de unlocked para algo acima de shared.
    //  (2) O SQLite nunca pede explicitamente um lock pending.
    //  (3) Um lock shared sempre é mantido quando um reserved é pedido.
    debug_assert!(p_file.e_file_lock as i32 != NO_LOCK || e_file_lock == SHARED_LOCK);
    debug_assert!(e_file_lock != PENDING_LOCK);
    debug_assert!(e_file_lock != RESERVED_LOCK || p_file.e_file_lock as i32 == SHARED_LOCK);

    // Este mutex é necessário porque p_file.p_inode é compartilhado entre threads
    sqlite3_mutex_enter(&p_file.p_inode.as_ref().unwrap().p_lock_mutex);

    // O `goto end_lock` do C vira este bloco rotulado: `break 'end_lock` pula o resto,
    // inclusive a atualização final de e_file_lock.
    'end_lock: {
        // Se alguma thread deste PID tem um lock por outro handle UnixFile que impede o
        // lock pedido, devolve BUSY.
        if p_file.e_file_lock != p_file.p_inode.as_ref().unwrap().e_file_lock
            && (p_file.p_inode.as_ref().unwrap().e_file_lock as i32 >= PENDING_LOCK
                || e_file_lock > SHARED_LOCK)
        {
            rc = SQLITE_BUSY;
            break 'end_lock;
        }

        // Se um lock SHARED é pedido e alguma thread deste PID já tem SHARED ou RESERVED,
        // incrementa os contadores de referência e devolve SQLITE_OK.
        if e_file_lock == SHARED_LOCK
            && (p_file.p_inode.as_ref().unwrap().e_file_lock as i32 == SHARED_LOCK
                || p_file.p_inode.as_ref().unwrap().e_file_lock as i32 == RESERVED_LOCK)
        {
            debug_assert!(e_file_lock == SHARED_LOCK);
            debug_assert!(p_file.e_file_lock == 0);
            debug_assert!(p_file.p_inode.as_ref().unwrap().n_shared > 0);
            p_file.e_file_lock = SHARED_LOCK as u8;
            p_file.p_inode.as_mut().unwrap().n_shared += 1;
            p_file.p_inode.as_mut().unwrap().n_lock += 1;
            break 'end_lock;
        }

        // Um lock PENDING é necessário antes de adquirir um SHARED e antes de adquirir um
        // EXCLUSIVE. No caso do SHARED, o PENDING será liberado.
        lock.l_len = 1;
        lock.l_whence = SEEK_SET;
        if e_file_lock == SHARED_LOCK
            || (e_file_lock == EXCLUSIVE_LOCK && p_file.e_file_lock as i32 == RESERVED_LOCK)
        {
            lock.l_type = if e_file_lock == SHARED_LOCK { F_RDLCK } else { F_WRLCK };
            lock.l_start = pending_byte();
            if unix_file_lock(p_file, &mut lock) != 0 {
                t_errno = errno();
                rc = sqlite_error_from_posix_error(t_errno, SQLITE_IOERR_LOCK);
                if rc != SQLITE_BUSY {
                    store_last_errno(p_file, t_errno);
                }
                break 'end_lock;
            } else if e_file_lock == EXCLUSIVE_LOCK {
                p_file.e_file_lock = PENDING_LOCK as u8;
                p_file.p_inode.as_mut().unwrap().e_file_lock = PENDING_LOCK as u8;
            }
        }

        // Chegando aqui, faz de fato as chamadas ao sistema operacional para o lock pedido.
        if e_file_lock == SHARED_LOCK {
            debug_assert!(p_file.p_inode.as_ref().unwrap().n_shared == 0);
            debug_assert!(p_file.p_inode.as_ref().unwrap().e_file_lock == 0);
            debug_assert!(rc == SQLITE_OK);

            // Agora obtém o read-lock
            lock.l_start = shared_first();
            lock.l_len = SHARED_SIZE as i64;
            if unix_file_lock(p_file, &mut lock) != 0 {
                t_errno = errno();
                rc = sqlite_error_from_posix_error(t_errno, SQLITE_IOERR_LOCK);
            }

            // Solta o lock PENDING temporário
            lock.l_start = pending_byte();
            lock.l_len = 1;
            lock.l_type = F_UNLCK;
            if unix_file_lock(p_file, &mut lock) != 0 && rc == SQLITE_OK {
                // Isto pode acontecer com um mount de rede
                t_errno = errno();
                rc = SQLITE_IOERR_UNLOCK;
            }

            if rc != 0 {
                if rc != SQLITE_BUSY {
                    store_last_errno(p_file, t_errno);
                }
                break 'end_lock;
            } else {
                p_file.e_file_lock = SHARED_LOCK as u8;
                p_file.p_inode.as_mut().unwrap().n_lock += 1;
                p_file.p_inode.as_mut().unwrap().n_shared = 1;
            }
        } else if e_file_lock == EXCLUSIVE_LOCK
            && p_file.p_inode.as_ref().unwrap().n_shared > 1
        {
            // Tentando um lock exclusivo, mas outra thread deste mesmo processo ainda
            // mantém um lock compartilhado.
            rc = SQLITE_BUSY;
        } else {
            // O pedido foi de um lock RESERVED ou EXCLUSIVE. Presume-se que já exista um
            // lock SHARED ou maior no arquivo.
            debug_assert!(p_file.e_file_lock != 0);
            lock.l_type = F_WRLCK;

            debug_assert!(e_file_lock == RESERVED_LOCK || e_file_lock == EXCLUSIVE_LOCK);
            if e_file_lock == RESERVED_LOCK {
                lock.l_start = reserved_byte();
                lock.l_len = 1;
            } else {
                lock.l_start = shared_first();
                lock.l_len = SHARED_SIZE as i64;
            }

            if unix_file_lock(p_file, &mut lock) != 0 {
                t_errno = errno();
                rc = sqlite_error_from_posix_error(t_errno, SQLITE_IOERR_LOCK);
                if rc != SQLITE_BUSY {
                    store_last_errno(p_file, t_errno);
                }
            }
        }

        if rc == SQLITE_OK {
            p_file.e_file_lock = e_file_lock as u8;
            p_file.p_inode.as_mut().unwrap().e_file_lock = e_file_lock as u8;
        }
    }

    // end_lock:
    sqlite3_mutex_leave(&p_file.p_inode.as_ref().unwrap().p_lock_mutex);
    ostrace!((
        "LOCK    {} {} {} (unix)\n",
        p_file.h,
        az_file_lock(e_file_lock),
        if rc == SQLITE_OK { "ok" } else { "failed" }
    ));
    rc
}


// ---- part_005.rs ----

/// Adiciona o descritor de arquivo usado pelo manipulador `p_file` à lista `p_unused`
/// do inode correspondente.
fn set_pending_fd(p_file: &mut UnixFile) {
    debug_assert!(unix_file_mutex_held(p_file));
    let mut p = p_file.p_preallocated_unused.take();
    let p_inode = p_file.p_inode.as_mut().unwrap();
    if let Some(unused) = p.as_mut() {
        unused.p_next = p_inode.p_unused.take();
    }
    p_inode.p_unused = p;
    p_file.h = -1;
    p_file.p_preallocated_unused = None;
}

/// Reduz o nível de bloqueio do descritor `p_file` para `e_file_lock`, que deve ser
/// NO_LOCK ou SHARED_LOCK. Se o nível atual já está no ou abaixo do pedido, não faz nada.
///
/// `handle_nfs_unlock` só tem efeito em plataformas com `SQLITE_ENABLE_LOCKING_STYLE`
/// (MacOSX); no Debian o ramo não existe e o valor precisa ser zero.
fn posix_unlock(p_file: &mut UnixFile, e_file_lock: i32, handle_nfs_unlock: i32) -> i32 {
    let mut lock = FLock::default();
    let mut rc = SQLITE_OK;

    ostrace!((
        "UNLOCK  {} {} was {}({},{}) pid={} (unix)\n",
        p_file.h,
        e_file_lock,
        p_file.e_file_lock,
        p_file.p_inode.as_ref().unwrap().e_file_lock,
        p_file.p_inode.as_ref().unwrap().n_shared,
        os_getpid(0)
    ));

    debug_assert!(e_file_lock <= SHARED_LOCK);
    if (p_file.e_file_lock as i32) <= e_file_lock {
        return SQLITE_OK;
    }
    sqlite3_mutex_enter(&p_file.p_inode.as_ref().unwrap().p_lock_mutex);
    debug_assert!(p_file.p_inode.as_ref().unwrap().n_shared != 0);
    // O `goto end_unlock` do C vira este bloco rotulado: `break 'end_unlock` pula o resto.
    'end_unlock: {
        if (p_file.e_file_lock as i32) > SHARED_LOCK {
            debug_assert!(
                p_file.p_inode.as_ref().unwrap().e_file_lock == p_file.e_file_lock
            );

            // Rebaixar para SHARED em NFS exige limpar o bloqueio de escrita antes de
            // estabelecer o de leitura; o ramo `handleNFSUnlock` é só de MacOSX.
            if e_file_lock == SHARED_LOCK {
                let _ = handle_nfs_unlock;
                debug_assert!(handle_nfs_unlock == 0);
                lock.l_type = F_RDLCK;
                lock.l_whence = SEEK_SET;
                lock.l_start = shared_first();
                lock.l_len = SHARED_SIZE as i64;
                if unix_file_lock(p_file, &mut lock) != 0 {
                    // Em teoria unix_file_lock() não pode falhar por causa de bloqueio
                    // incompatível de outro processo. Se falhar, o outro processo não está
                    // seguindo o protocolo; devolve SQLITE_IOERR_RDLOCK, pois SQLITE_BUSY
                    // confundiria a camada de cima.
                    rc = SQLITE_IOERR_RDLOCK;
                    store_last_errno(p_file, errno());
                    break 'end_unlock;
                }
            }
            lock.l_type = F_UNLCK;
            lock.l_whence = SEEK_SET;
            lock.l_start = pending_byte();
            lock.l_len = 2;
            debug_assert!(pending_byte() + 1 == reserved_byte());
            if unix_file_lock(p_file, &mut lock) == 0 {
                p_file.p_inode.as_mut().unwrap().e_file_lock = SHARED_LOCK as u8;
            } else {
                rc = SQLITE_IOERR_UNLOCK;
                store_last_errno(p_file, errno());
                break 'end_unlock;
            }
        }
        if e_file_lock == NO_LOCK {
            // Decrementa o contador de bloqueios compartilhados. Só libera o bloqueio no
            // SO quando todas as threads deste processo o liberaram.
            p_file.p_inode.as_mut().unwrap().n_shared -= 1;
            if p_file.p_inode.as_ref().unwrap().n_shared == 0 {
                lock.l_type = F_UNLCK;
                lock.l_whence = SEEK_SET;
                lock.l_start = 0;
                lock.l_len = 0;
                if unix_file_lock(p_file, &mut lock) == 0 {
                    p_file.p_inode.as_mut().unwrap().e_file_lock = NO_LOCK as u8;
                } else {
                    rc = SQLITE_IOERR_UNLOCK;
                    store_last_errno(p_file, errno());
                    p_file.p_inode.as_mut().unwrap().e_file_lock = NO_LOCK as u8;
                    p_file.e_file_lock = NO_LOCK as u8;
                }
            }

            // Decrementa a contagem de bloqueios contra este mesmo arquivo. Ao chegar a
            // zero, fecha os outros descritores cujo fechamento foi adiado.
            p_file.p_inode.as_mut().unwrap().n_lock -= 1;
            debug_assert!(p_file.p_inode.as_ref().unwrap().n_lock >= 0);
            if p_file.p_inode.as_ref().unwrap().n_lock == 0 {
                close_pending_fds(p_file);
            }
        }
    }

    sqlite3_mutex_leave(&p_file.p_inode.as_ref().unwrap().p_lock_mutex);
    if rc == SQLITE_OK {
        p_file.e_file_lock = e_file_lock as u8;
    }
    rc
}

/// Reduz o nível de bloqueio do descritor `p_file` para `e_file_lock`, que deve ser
/// NO_LOCK ou SHARED_LOCK. Se o nível atual já está no ou abaixo do pedido, não faz nada.
fn unix_unlock(p_file: &mut UnixFile, e_file_lock: i32) -> i32 {
    debug_assert!(e_file_lock == SHARED_LOCK || p_file.n_fetch_out == 0);
    posix_unlock(p_file, e_file_lock, 0)
}

/// Executa as partes da operação "fechar arquivo" comuns a todos os esquemas de
/// bloqueio: fecha o descritor, se válido, e zera todos os campos do `UnixFile`.
///
/// Não é preciso manter o mutex ao chamar esta rotina.
fn close_unix_file(p_file: &mut UnixFile) -> i32 {
    unix_unmapfile(p_file);
    if p_file.h >= 0 {
        robust_close(Some(&*p_file), p_file.h, line!() as i32);
        p_file.h = -1;
    }
    ostrace!(("CLOSE   {:<3}\n", p_file.h));
    open_counter(-1);
    // O bloco pré-alocado é liberado ao zerar a estrutura inteira (memset do C).
    *p_file = UnixFile::default();
    SQLITE_OK
}

/// Fecha um arquivo.
fn unix_close(p_file: &mut UnixFile) -> i32 {
    debug_assert!(p_file.p_inode.is_some());
    verify_db_file(p_file);
    unix_unlock(p_file, NO_LOCK);
    debug_assert!(unix_file_mutex_notheld(p_file));
    unix_enter_mutex();

    // `p_inode` é sempre válido aqui. Senão, outra rotina de fechamento (nolock_close())
    // seria chamada no lugar.
    debug_assert!(
        p_file.p_inode.as_ref().unwrap().n_lock > 0
            || p_file.p_inode.as_ref().unwrap().b_process_lock == 0
    );
    sqlite3_mutex_enter(&p_file.p_inode.as_ref().unwrap().p_lock_mutex);
    if p_file.p_inode.as_ref().unwrap().n_lock != 0 {
        // Com bloqueios pendentes não se fecha o arquivo agora, pois isso os limparia.
        // O descritor vai para a lista `p_unused` e é fechado quando o último bloqueio
        // for liberado.
        set_pending_fd(p_file);
    }
    sqlite3_mutex_leave(&p_file.p_inode.as_ref().unwrap().p_lock_mutex);
    release_inode_info(p_file);
    debug_assert!(p_file.p_shm.is_none());
    let rc = close_unix_file(p_file);
    unix_leave_mutex();
    rc
}

// Fim da implementação de bloqueio advisory POSIX.

// Bloqueio no-op: o bloqueio é ignorado e nenhuma tentativa é feita de travar o arquivo
// para leitura ou escrita. Apropriado para bancos somente leitura ou quando o aplicativo
// impede por outro meio o acesso simultâneo; há risco sério de corrupção se várias
// conexões escreverem no mesmo arquivo com este modo.

fn nolock_check_reserved_lock(_not_used: &mut UnixFile, p_res_out: &mut i32) -> i32 {
    *p_res_out = 0;
    SQLITE_OK
}

fn nolock_lock(_not_used: &mut UnixFile, _not_used2: i32) -> i32 {
    SQLITE_OK
}

fn nolock_unlock(_not_used: &mut UnixFile, _not_used2: i32) -> i32 {
    SQLITE_OK
}

/// Fecha o arquivo.
fn nolock_close(p_file: &mut UnixFile) -> i32 {
    close_unix_file(p_file)
}

// Bloqueio por dot-file: a existência de um arquivo (na verdade um diretório) de
// bloqueio separado controla o acesso ao banco. Funciona em quase qualquer sistema de
// arquivos, mas não há concorrência (um leitor bloqueia todos) e uma queda deixa
// arquivos de bloqueio obsoletos. O diretório tem o nome do banco mais ".lock"; sua
// existência implica bloqueio EXCLUSIVE, e SHARED, RESERVED e PENDING viram EXCLUSIVE.

/// Sufixo acrescentado ao nome do banco para formar o diretório de bloqueio.
pub const DOTLOCK_SUFFIX: &[u8] = b".lock";

/// Verifica se há bloqueio RESERVED mantido no arquivo por este ou outro processo. Se
/// houver, define `*p_res_out` como não zero, senão como zero. Retorna SQLITE_OK, a menos
/// que ocorra erro de E/S na verificação.
///
/// No dot-file ou existe bloqueio ou não existe: `*p_res_out` é verdadeiro se qualquer
/// bloqueio é mantido e falso se o arquivo está livre.
fn dotlock_check_reserved_lock(p_file: &mut UnixFile, p_res_out: &mut i32) -> i32 {
    let rc = SQLITE_OK;

    simulate_io_error! {
        return SQLITE_IOERR_CHECKRESERVEDLOCK;
    }

    let reserved = (os_access(&p_file.locking_context, 0) == 0) as i32;
    ostrace!(("TEST WR-LOCK {} {} {} (dotlock)\n", p_file.h, rc, reserved));
    *p_res_out = reserved;
    rc
}


// ---- part_006.rs ----

/// Trava o arquivo com o lock especificado por `e_file_lock`, um entre:
///
///     (1) SHARED_LOCK
///     (2) RESERVED_LOCK
///     (3) PENDING_LOCK
///     (4) EXCLUSIVE_LOCK
///
/// Às vezes, ao pedir um estado de lock, estados intermediários são inseridos. O lock
/// pode falhar numa das transições posteriores, deixando o estado diferente do inicial,
/// mas ainda aquém do objetivo. Transições permitidas e estados intermediários:
///
///    UNLOCKED -> SHARED
///    SHARED -> RESERVED
///    SHARED -> (PENDING) -> EXCLUSIVE
///    RESERVED -> (PENDING) -> EXCLUSIVE
///    PENDING -> EXCLUSIVE
///
/// A rotina só aumenta um lock. Use `os_unlock()` para baixar o nível.
///
/// Com lock por dotfile só o estado (4), EXCLUSIVE, é de fato suportado, mas os outros
/// níveis são rastreados internamente.
fn dotlock_lock(p_file: &mut UnixFile, e_file_lock: i32) -> i32 {
    let z_lock_file: Vec<u8> = p_file.locking_context.clone();
    let mut rc;

    // Se já temos algum lock, o arquivo de lock existe. Basta ajustar o registro interno
    // do nível de lock.
    if (p_file.e_file_lock as i32) > NO_LOCK {
        p_file.e_file_lock = e_file_lock as u8;
        // Sempre atualiza o timestamp do arquivo antigo (utime e utimes com NULL fazem o
        // mesmo no Linux)
        let _ = os_utimes(&z_lock_file, None);
        return SQLITE_OK;
    }

    // Obtém um lock exclusivo
    rc = os_mkdir(&z_lock_file, 0o777);
    if rc < 0 {
        // Falhou ao abrir ou criar o diretório de lock
        let t_errno = errno();
        if t_errno == EEXIST {
            rc = SQLITE_BUSY;
        } else {
            rc = sqlite_error_from_posix_error(t_errno, SQLITE_IOERR_LOCK);
            if rc != SQLITE_BUSY {
                store_last_errno(p_file, t_errno);
            }
        }
        return rc;
    }

    // Conseguiu: define o tipo e devolve ok
    p_file.e_file_lock = e_file_lock as u8;
    rc
}

/// Baixa o nível de lock do descritor `p_file` para `e_file_lock`, que deve ser NO_LOCK
/// ou SHARED_LOCK. Se o nível já está no pedido ou abaixo dele, é um no-op.
///
/// Quando o nível chega a NO_LOCK, apaga o arquivo de lock.
fn dotlock_unlock(p_file: &mut UnixFile, e_file_lock: i32) -> i32 {
    let z_lock_file: Vec<u8> = p_file.locking_context.clone();

    ostrace!((
        "UNLOCK  {} {} was {} pid={} (dotlock)\n",
        p_file.h,
        e_file_lock,
        p_file.e_file_lock,
        os_getpid(0)
    ));
    debug_assert!(e_file_lock <= SHARED_LOCK);

    // No-op se possível
    if p_file.e_file_lock as i32 == e_file_lock {
        return SQLITE_OK;
    }

    // Para baixar para shared basta atualizar a noção interna do estado do lock. Não é
    // preciso mexer no arquivo em disco.
    if e_file_lock == SHARED_LOCK {
        p_file.e_file_lock = SHARED_LOCK as u8;
        return SQLITE_OK;
    }

    // Para destravar de vez o banco, apaga o arquivo de lock
    debug_assert!(e_file_lock == NO_LOCK);
    let mut rc = os_rmdir(&z_lock_file);
    if rc < 0 {
        let t_errno = errno();
        if t_errno == ENOENT {
            rc = SQLITE_OK;
        } else {
            rc = SQLITE_IOERR_UNLOCK;
            store_last_errno(p_file, t_errno);
        }
        return rc;
    }
    p_file.e_file_lock = NO_LOCK as u8;
    SQLITE_OK
}

/// Fecha um arquivo. Garante que o lock foi liberado antes de fechar.
fn dotlock_close(p_file: &mut UnixFile) -> i32 {
    dotlock_unlock(p_file, NO_LOCK);
    // O `sqlite3_free(pFile->lockingContext)` do C é o drop do Vec: close_unix_file()
    // zera a estrutura inteira.
    close_unix_file(p_file)
}

// Fim da implementação de lock por dotfile.
//
// As seções de lock por flock() (SQLITE_ENABLE_LOCKING_STYLE só vale no MacOSX) e de
// semáforo nomeado (só VxWorks) não existem na compilação do Debian e foram omitidas.


// ---- part_007.rs ----

// Este trecho do C (semXLock, semXUnlock, semXClose e todo o lock AFP: afpSetLock,
// afpCheckReservedLock, afpLock) existe só sob `OS_VXWORKS` (semáforo nomeado) e
// `defined(__APPLE__) && SQLITE_ENABLE_LOCKING_STYLE` (AFP). Nenhum dos dois vale na
// compilação do Debian 13 (Linux), então não há item a traduzir aqui, como manda a
// convenção para ramos de outra plataforma. O arquivo existe para o `mod.rs` do
// integrador declarar `mod part_007;` sem condicionais.


// ---- part_008.rs ----

// As implementações AFP e NFS de bloqueio (afpUnlock, afpClose, nfsUnlock) só existem em
// MacOSX com SQLITE_ENABLE_LOCKING_STYLE e por isso somem no Debian.

/// Posiciona no deslocamento passado e lê `cnt` bytes em `p_buf`. Retorna o número de bytes
/// realmente lidos.
///
/// Para não pisar no valor de errno numa leitura que falhou, `last_errno` é gravado antes de
/// retornar.
pub fn seek_and_read(id: &mut UnixFile, offset: i64, p_buf: &mut [u8], cnt: i32) -> i32 {
    let mut cnt = cnt;
    let mut offset = offset;
    let mut got: i32;
    let mut prior: i32 = 0;
    let mut buf_idx: usize = 0;
    debug_assert!(cnt == (cnt & 0x1ffff));
    debug_assert!(id.h > 2);
    loop {
        got = os_pread(id.h, &mut p_buf[buf_idx..buf_idx + cnt as usize], offset);
        if got == cnt {
            break;
        }
        if got < 0 {
            if errno() == EINTR {
                got = 1;
                continue;
            }
            prior = 0;
            store_last_errno(id, errno());
            break;
        } else if got > 0 {
            cnt -= got;
            offset += got as i64;
            prior += got;
            buf_idx += got as usize;
        }
        if got <= 0 {
            break;
        }
    }
    got + prior
}

/// Lê dados de um arquivo para um buffer. Retorna SQLITE_OK se todos os bytes foram lidos
/// com sucesso e SQLITE_IOERR se algo deu errado.
pub fn unix_read(id: &mut UnixFile, p_buf: &mut [u8], amt: i32, offset: i64) -> i32 {
    let mut amt = amt;
    let mut offset = offset;
    let mut buf_idx: usize = 0;
    debug_assert!(offset >= 0);
    debug_assert!(amt > 0);

    // Atende o máximo possível do pedido copiando dados do mapeamento de memória.
    if offset < id.mmap_size {
        if offset + amt as i64 <= id.mmap_size {
            let start = offset as usize;
            p_buf[..amt as usize].copy_from_slice(&id.p_map_region[start..start + amt as usize]);
            return SQLITE_OK;
        } else {
            let n_copy = (id.mmap_size - offset) as i32;
            let start = offset as usize;
            p_buf[..n_copy as usize].copy_from_slice(&id.p_map_region[start..start + n_copy as usize]);
            buf_idx += n_copy as usize;
            amt -= n_copy;
            offset += n_copy as i64;
        }
    }

    let got = seek_and_read(id, offset, &mut p_buf[buf_idx..], amt);
    if got == amt {
        SQLITE_OK
    } else if got < 0 {
        // last_errno foi gravado por seek_and_read(). Normalmente retornamos
        // SQLITE_IOERR_READ, mas para alguns erros retornamos SQLITE_IOERR_CORRUPTFS, que
        // api_exit() converte em SQLITE_CORRUPT antes de voltar à aplicação.
        match id.last_errno {
            ERANGE | EIO | ENXIO => SQLITE_IOERR_CORRUPTFS,
            _ => SQLITE_IOERR_READ,
        }
    } else {
        store_last_errno(id, 0); // não é um erro de sistema
        // As partes não lidas do buffer devem ser zeradas
        p_buf[buf_idx + got as usize..buf_idx + amt as usize].fill(0);
        SQLITE_IOERR_SHORT_READ
    }
}

/// Tenta posicionar o descritor `fd` no deslocamento absoluto `i_off` e escrever `n_buf` bytes
/// de `p_buf`. Em erro retorna -1 e grava `pi_errno`. Caso contrário retorna o número de bytes
/// escritos (que pode ser menor que `n_buf`).
pub fn seek_and_write_fd(fd: i32, i_off: i64, p_buf: &[u8], n_buf: i32, pi_errno: &mut i32) -> i32 {
    let mut rc: i32;
    let n_buf = n_buf & 0x1ffff;
    debug_assert!(fd > 2);

    loop {
        rc = os_pwrite(fd, &p_buf[..n_buf as usize], i_off);
        if !(rc < 0 && errno() == EINTR) {
            break;
        }
    }

    if rc < 0 {
        *pi_errno = errno();
    }
    rc
}

/// Posiciona no deslocamento e escreve `cnt` bytes de `p_buf`. Retorna o número de bytes
/// escritos.
///
/// Para não pisar no valor de errno numa escrita que falhou, `last_errno` é gravado antes de
/// retornar.
pub fn seek_and_write(id: &mut UnixFile, offset: i64, p_buf: &[u8], cnt: i32) -> i32 {
    seek_and_write_fd(id.h, offset, p_buf, cnt, &mut id.last_errno)
}


// ---- part_009.rs ----

/// Escreve dados de um buffer em um arquivo. Retorna SQLITE_OK em sucesso ou outro código de
/// erro em falha.
pub fn unix_write(id: &mut UnixFile, p_buf: &[u8], amt: i32, offset: i64) -> i32 {
    let mut amt = amt;
    let mut offset = offset;
    let mut buf_idx: usize = 0;
    let mut wrote: i32;
    debug_assert!(amt > 0);

    // SQLITE_MMAP_READWRITE não é definido no Debian: a escrita nunca passa pelo mapeamento.

    loop {
        wrote = seek_and_write(id, offset, &p_buf[buf_idx..], amt);
        if wrote < amt && wrote > 0 {
            amt -= wrote;
            offset += wrote as i64;
            buf_idx += wrote as usize;
        } else {
            break;
        }
    }

    if amt > wrote {
        if wrote < 0 && id.last_errno != ENOSPC {
            // last_errno gravado por seek_and_write
            return SQLITE_IOERR_WRITE;
        } else {
            store_last_errno(id, 0); // não é um erro de sistema
            return SQLITE_FULL;
        }
    }

    SQLITE_OK
}

/// A chamada de sistema fsync() não funciona como anunciado em muitos sistemas unix. Este
/// procedimento é uma tentativa de fazê-la funcionar melhor.
///
/// O SQLite ativa a flag `data_only` se o tamanho do arquivo não mudou. Porém, como o
/// fdatasync() também grava o inode quando o tamanho muda, para o SQLite um fdatasync() é
/// sempre adequado, independentemente de `data_only`. Em Linux não há F_FULLFSYNC.
pub fn full_fsync(fd: i32, _full_sync: i32, _data_only: i32) -> i32 {
    os_fdatasync(fd)
}

/// Abre um descritor de arquivo para o diretório que contém `z_filename`. Se bem-sucedido
/// retorna `Ok(fd)`. Se ocorrer erro retorna `Err` com SQLITE_CANTOPEN.
///
/// O descritor do diretório serve para uma única coisa: fsync() do diretório, para garantir
/// que os eventos de criação e remoção de arquivo cheguem ao disco.
pub fn open_directory(z_filename: &[u8]) -> Result<i32, i32> {
    // sqlite3_snprintf(MAX_PATHNAME, ...) copia no máximo MAX_PATHNAME-1 bytes
    let n = z_filename.len().min(MAX_PATHNAME as usize - 1);
    let mut z_dirname: Vec<u8> = z_filename[..n].to_vec();

    // O laço do C começa em strlen(), onde há o NUL, e procura a última barra: o nome do
    // diretório é tudo o que vem antes dela (a barra é descartada).
    let mut ii = z_dirname.len();
    while ii > 0 && z_dirname[ii - 1] != b'/' {
        ii -= 1;
    }
    // aqui ii-1 é o índice da última barra, ou ii==0 se não há nenhuma
    if ii > 1 {
        z_dirname.truncate(ii - 1);
    } else {
        let first = z_dirname.first().copied().unwrap_or(0);
        z_dirname.clear();
        z_dirname.push(if first != b'/' { b'.' } else { b'/' });
    }
    let fd = robust_open(&z_dirname, O_RDONLY | O_BINARY, 0);
    if fd >= 0 {
        return Ok(fd);
    }
    Err(unix_log_error(SQLITE_CANTOPEN_BKPT, "openDirectory", &z_dirname))
}

/// Garante que todas as escritas em um arquivo foram confirmadas no disco.
///
/// Se `dataOnly==0` sincroniza o arquivo e seus metadados. Se `dataOnly!=0` sincroniza só os
/// dados. Em Unix também garante que a entrada de diretório do arquivo foi criada, com fsync
/// do diretório que contém o arquivo.
pub fn unix_sync(id: &mut UnixFile, flags: i32) -> i32 {
    let mut rc: i32;

    let is_data_only = flags & SQLITE_SYNC_DATAONLY;
    let is_fullsync = ((flags & 0x0F) == SQLITE_SYNC_FULL) as i32;

    rc = full_fsync(id.h, is_fullsync, is_data_only);
    if rc != 0 {
        store_last_errno(id, errno());
        return unix_log_error(SQLITE_IOERR_FSYNC, "full_fsync", &id.z_path);
    }

    // Também faz fsync do diretório que contém o arquivo se a flag DIRSYNC está ligada. Isso
    // acontece uma única vez. Muitos sistemas não conseguem fazer fsync de diretório, então
    // erros nesse fsync são ignorados.
    if id.ctrl_flags & (UNIXFILE_DIRSYNC as u16) != 0 {
        match open_directory(&id.z_path) {
            Ok(dirfd) => {
                full_fsync(dirfd, 0, 0);
                robust_close(id, dirfd, line!() as i32);
                rc = SQLITE_OK;
            }
            Err(_) => {
                // o único erro possível é SQLITE_CANTOPEN
                rc = SQLITE_OK;
            }
        }
        id.ctrl_flags &= !(UNIXFILE_DIRSYNC as u16);
    }
    rc
}

/// Trunca um arquivo aberto para o tamanho especificado.
pub fn unix_truncate(id: &mut UnixFile, n_byte: i64) -> i32 {
    let mut n_byte = n_byte;

    // Se o usuário configurou um tamanho de chunk para este arquivo, trunca de modo que o
    // arquivo tenha um número inteiro de chunks (o tamanho real depois da operação pode ser
    // maior que o pedido).
    if id.sz_chunk > 0 {
        let sz = id.sz_chunk as i64;
        n_byte = ((n_byte + sz - 1) / sz) * sz;
    }

    let rc = robust_ftruncate(id.h, n_byte);
    if rc != 0 {
        store_last_errno(id, errno());
        unix_log_error(SQLITE_IOERR_TRUNCATE, "ftruncate", &id.z_path)
    } else {
        // Se o arquivo foi truncado para menos que a região mapeada, reduz também o tamanho
        // efetivo do mapeamento. Daí em diante o SQLite usa read() e write() além desse ponto.
        if n_byte < id.mmap_size {
            id.mmap_size = n_byte;
        }
        SQLITE_OK
    }
}


// ---- part_010.rs ----

/// Determina o tamanho atual de um arquivo em bytes.
pub fn unix_file_size(id: &mut UnixFile, p_size: &mut i64) -> i32 {
    let mut buf = StatBuf::default();
    let rc = os_fstat(id.h, &mut buf);
    if rc != 0 {
        store_last_errno(id, errno());
        return SQLITE_IOERR_FSTAT;
    }
    *p_size = buf.st_size;

    // Ao abrir um banco de tamanho zero, find_inode_info() escreve um único byte no arquivo
    // para contornar um bug do sistema de arquivos msdos do OS-X. Para evitar problemas nas
    // camadas superiores, o tamanho é reportado como zero mesmo sendo 1. Ticket #3260.
    if *p_size == 1 {
        *p_size = 0;
    }

    SQLITE_OK
}

/// Trata a operação de controle de arquivo SQLITE_FCNTL_SIZE_HINT. Amplia o banco para
/// `n_byte` (arredondado para o próximo tamanho de chunk). Se o banco já tem `n_byte` ou mais,
/// não faz nada.
pub fn fcntl_size_hint(p_file: &mut UnixFile, n_byte: i64) -> i32 {
    if p_file.sz_chunk > 0 {
        let mut buf = StatBuf::default();

        if os_fstat(p_file.h, &mut buf) != 0 {
            return SQLITE_IOERR_FSTAT;
        }

        let sz = p_file.sz_chunk as i64;
        let n_size = ((n_byte + sz - 1) / sz) * sz;
        if n_size > buf.st_size {
            // posix_fallocate() devolve zero em sucesso ou o número do erro em falha.
            let mut err: i32;
            loop {
                err = os_fallocate(p_file.h, buf.st_size, n_size - buf.st_size);
                if err != EINTR {
                    break;
                }
            }
            if err != 0 && err != EINVAL {
                return SQLITE_IOERR_WRITE;
            }
        }
    }

    if p_file.mmap_size_max > 0 && n_byte > p_file.mmap_size {
        if p_file.sz_chunk <= 0 {
            if robust_ftruncate(p_file.h, n_byte) != 0 {
                store_last_errno(p_file, errno());
                return unix_log_error(SQLITE_IOERR_TRUNCATE, "ftruncate", &p_file.z_path);
            }
        }

        return unix_mapfile(p_file, n_byte);
    }

    SQLITE_OK
}

/// Se `*p_arg` é inicialmente negativo, é uma consulta: grava em `*p_arg` 1 ou 0 conforme o
/// bit `mask` de `ctrl_flags` esteja ligado ou não. Se `*p_arg` é 0 ou 1, desliga ou liga o
/// bit.
pub fn unix_mode_bit(p_file: &mut UnixFile, mask: u8, p_arg: &mut i32) {
    if *p_arg < 0 {
        *p_arg = ((p_file.ctrl_flags & mask as u16) != 0) as i32;
    } else if *p_arg == 0 {
        p_file.ctrl_flags &= !(mask as u16);
    } else {
        p_file.ctrl_flags |= mask as u16;
    }
}

/// Informação e controle de um identificador de arquivo aberto. O `void *pArg` do C é o
/// enum `FileControlArg`, com a variante do tipo que cada operação espera.
pub fn unix_file_control(p_file: &mut UnixFile, op: i32, p_arg: &mut FileControlArg) -> i32 {
    match op {
        SQLITE_FCNTL_LOCKSTATE => {
            *p_arg = FileControlArg::Int(p_file.e_file_lock as i32);
            SQLITE_OK
        }
        SQLITE_FCNTL_LAST_ERRNO => {
            *p_arg = FileControlArg::Int(p_file.last_errno);
            SQLITE_OK
        }
        SQLITE_FCNTL_CHUNK_SIZE => {
            if let FileControlArg::Int(v) = *p_arg {
                p_file.sz_chunk = v;
            }
            SQLITE_OK
        }
        SQLITE_FCNTL_SIZE_HINT => {
            if let FileControlArg::Int64(v) = *p_arg {
                return fcntl_size_hint(p_file, v);
            }
            SQLITE_OK
        }
        SQLITE_FCNTL_PERSIST_WAL => {
            if let FileControlArg::Int(ref mut v) = *p_arg {
                unix_mode_bit(p_file, UNIXFILE_PERSIST_WAL as u8, v);
            }
            SQLITE_OK
        }
        SQLITE_FCNTL_POWERSAFE_OVERWRITE => {
            if let FileControlArg::Int(ref mut v) = *p_arg {
                unix_mode_bit(p_file, UNIXFILE_PSOW as u8, v);
            }
            SQLITE_OK
        }
        SQLITE_FCNTL_VFSNAME => {
            *p_arg = FileControlArg::Text(Some(p_file.p_vfs.z_name.clone()));
            SQLITE_OK
        }
        SQLITE_FCNTL_TEMPFILENAME => {
            let mut z_tfile = vec![0u8; p_file.p_vfs.mx_pathname as usize];
            unix_get_tempname(p_file.p_vfs.mx_pathname, &mut z_tfile);
            let n = z_tfile.iter().position(|&b| b == 0).unwrap_or(z_tfile.len());
            z_tfile.truncate(n);
            *p_arg = FileControlArg::Text(Some(z_tfile));
            SQLITE_OK
        }
        SQLITE_FCNTL_HAS_MOVED => {
            *p_arg = FileControlArg::Int(file_has_moved(p_file));
            SQLITE_OK
        }
        SQLITE_FCNTL_MMAP_SIZE => {
            let mut new_limit = match *p_arg {
                FileControlArg::Int64(v) => v,
                _ => 0,
            };
            let mut rc = SQLITE_OK;
            if new_limit > global_config().mx_mmap {
                new_limit = global_config().mx_mmap;
            }

            // O valor de new_limit pode acabar convertido para size_t e passado ao mmap().
            // Restringe a 2GB se size_t não for pelo menos de 64 bits.
            if new_limit > 0 && std::mem::size_of::<usize>() < 8 {
                new_limit &= 0x7FFFFFFF;
            }

            *p_arg = FileControlArg::Int64(p_file.mmap_size_max);
            if new_limit >= 0 && new_limit != p_file.mmap_size_max && p_file.n_fetch_out == 0 {
                p_file.mmap_size_max = new_limit;
                if p_file.mmap_size > 0 {
                    unix_unmapfile(p_file);
                    rc = unix_mapfile(p_file, -1);
                }
            }
            rc
        }
        SQLITE_FCNTL_EXTERNAL_READER => {
            let mut out = 0i32;
            let rc = unix_fcntl_external_reader(p_file, &mut out);
            *p_arg = FileControlArg::Int(out);
            rc
        }
        _ => SQLITE_NOTFOUND,
    }
}

/// Se `p_fd.sector_size` não é zero ao chamar esta função, ela não faz nada. Caso contrário,
/// `sector_size` e `device_characteristics` são definidos conforme as características do
/// sistema de arquivos. (A versão QNX não existe no Debian.)
pub fn set_device_characteristics(p_fd: &mut UnixFile) {
    if p_fd.sector_size == 0 {
        // Liga a flag POWERSAFE_OVERWRITE se pedida.
        if p_fd.ctrl_flags & (UNIXFILE_PSOW as u16) != 0 {
            p_fd.device_characteristics |= SQLITE_IOCAP_POWERSAFE_OVERWRITE;
        }

        p_fd.sector_size = SQLITE_DEFAULT_SECTOR_SIZE as u32;
    }
}


// ---- part_011.rs ----

/// Retorna o tamanho do setor em bytes do dispositivo de bloco subjacente ao arquivo.
/// Quase sempre são 512 bytes, mas pode ser maior em alguns dispositivos.
///
/// O código do SQLite assume que esta função não pode falhar, e que dois arquivos criados no
/// mesmo diretório (um banco e seu journal) têm o mesmo tamanho de setor.
pub fn unix_sector_size(p_fd: &mut UnixFile) -> i32 {
    set_device_characteristics(p_fd);
    p_fd.sector_size as i32
}

/// Retorna as características de dispositivo do arquivo.
///
/// Este VFS retorna SQLITE_IOCAP_POWERSAFE_OVERWRITE por padrão. A escolha é controversa
/// porque o sistema de arquivos nem sempre garante sobrescrita segura contra perda de energia,
/// mas o comportamento sem PSOW é raríssimo e afirmar PSOW reduz muito a E/S do journal, pois
/// elimina muito preenchimento. Há um controle de arquivo e um parâmetro de consulta de URI
/// para desligá-lo.
pub fn unix_device_characteristics(p_fd: &mut UnixFile) -> i32 {
    set_device_characteristics(p_fd);
    p_fd.device_characteristics
}

/// Retorna o tamanho de página do sistema. Não deve ser chamada diretamente por outro código
/// deste arquivo: o acesso é por `os_getpagesize()`.
pub fn unix_getpagesize() -> i32 {
    sys_getpagesize()
}

/// Objeto que representa um buffer de memória compartilhada.
///
/// Quando várias threads referenciam o mesmo wal-index, cada uma tem seu próprio `UnixShm`,
/// mas todas apontam para uma única instância de `UnixShmNode`. Cada wal-index é aberto uma
/// vez por processo. Cada `UnixShmNode` está ligado a um único `UnixInodeInfo`, que guarda a
/// referência para ele.
///
/// `unix_mutex_held()` deve ser verdadeiro ao criar ou destruir o objeto ou ao ler e escrever
/// `n_ref`. `h_shm` e `z_filename` são somente leitura depois da criação. Para os demais
/// campos, o mutex `p_shm_mutex` deve estar preso, ou `n_ref==0` com `unix_mutex_held()`.
///
/// `a_lock[SQLITE_SHM_NLOCK]` registra os locks mantidos pelos clientes em cada slot: 0 é
/// nenhum lock, -1 é lock EXCLUSIVE, positivo é o número de locks compartilhados.
pub struct UnixShmNode {
    /// UnixInodeInfo dono deste nó SHM (ponteiro de volta)
    pub p_inode: Option<Weak<RefCell<UnixInodeInfo>>>,
    /// Mutex para acessar este objeto
    pub p_shm_mutex: Option<MutexRef>,
    /// Nome do arquivo mapeado
    pub z_filename: Vec<u8>,
    /// Descritor de arquivo aberto
    pub h_shm: i32,
    /// Tamanho das regiões de memória compartilhada
    pub sz_region: i32,
    /// Tamanho do vetor ap_region
    pub n_region: u16,
    /// Verdadeiro se somente leitura
    pub is_readonly: u8,
    /// Verdadeiro se nenhum lock DMS é mantido
    pub is_unlocked: u8,
    /// Regiões de memória compartilhada mapeadas
    pub ap_region: Vec<Option<Vec<u8>>>,
    /// Número de objetos UnixShm apontando para este
    pub n_ref: i32,
    /// Todos os objetos UnixShm apontando para este
    pub p_first: Option<UnixShmRef>,
    /// Número de locks compartilhados no slot, -1 é lock exclusivo
    pub a_lock: [i32; SQLITE_SHM_NLOCK as usize],
}

pub type UnixShmNodeRef = Rc<RefCell<UnixShmNode>>;

/// Estrutura usada por este VFS para registrar o estado de uma conexão de memória
/// compartilhada aberta.
///
/// `p_shm_node` e `id` são inicializados na criação e somente leitura depois. Os demais campos
/// são leitura e escrita, com o `p_shm_mutex` do nó preso.
pub struct UnixShm {
    /// O UnixShmNode subjacente
    pub p_shm_node: Option<UnixShmNodeRef>,
    /// Próximo UnixShm com o mesmo UnixShmNode
    pub p_next: Option<UnixShmRef>,
    /// Verdadeiro se mantém o p_shm_mutex do nó
    pub has_mutex: u8,
    /// Id desta conexão dentro do seu UnixShmNode
    pub id: u8,
    /// Máscara de locks compartilhados mantidos
    pub shared_mask: u16,
    /// Máscara de locks exclusivos mantidos
    pub excl_mask: u16,
}

pub type UnixShmRef = Rc<RefCell<UnixShm>>;

/// Constantes usadas para bloqueio: primeiro byte de lock
pub const UNIX_SHM_BASE: i32 = (22 + SQLITE_SHM_NLOCK) * 4;
/// Byte do deadman switch
pub const UNIX_SHM_DMS: i32 = UNIX_SHM_BASE + SQLITE_SHM_NLOCK;

/// Usa F_GETLK para verificar se há leitores com transações wal-mode abertas em outros
/// processos no banco `p_file`. Sem erro, retorna SQLITE_OK e grava em `*pi_out` 1 se há tais
/// transações, ou 0 caso contrário. Em erro retorna um código de erro do SQLite.
pub fn unix_fcntl_external_reader(p_file: &mut UnixFile, pi_out: &mut i32) -> i32 {
    let mut rc = SQLITE_OK;
    *pi_out = 0;
    if let Some(p_shm) = p_file.p_shm.clone() {
        let p_shm_node = p_shm.borrow().p_shm_node.clone().unwrap();
        let mut f = FLock::default();
        f.l_type = F_WRLCK as _;
        f.l_whence = SEEK_SET as _;
        f.l_start = (UNIX_SHM_BASE + 3) as _;
        f.l_len = (SQLITE_SHM_NLOCK - 3) as _;

        let node = p_shm_node.borrow();
        mutex_enter(node.p_shm_mutex.as_ref());
        if os_fcntl(node.h_shm, F_GETLK, &mut f) < 0 {
            rc = SQLITE_IOERR_LOCK;
        } else {
            *pi_out = (f.l_type as i32 != F_UNLCK) as i32;
        }
        mutex_leave(node.p_shm_mutex.as_ref());
    }

    rc
}

/// Aplica locks advisory POSIX em todos os bytes de `ofst` até `ofst+n-1`.
///
/// Os locks bloqueiam se a máscara é exatamente UNIX_SHM_C e são não bloqueantes nos demais
/// casos.
pub fn unix_shm_system_lock(p_file: &mut UnixFile, lock_type: i32, ofst: i32, n: i32) -> i32 {
    let mut rc = SQLITE_OK;

    let p_inode = p_file.p_inode.clone().unwrap();
    let p_shm_node = p_inode.borrow().p_shm_node.clone().unwrap();

    // Locks compartilhados nunca abrangem mais de um byte
    debug_assert!(n == 1 || lock_type != F_RDLCK);

    // Os locks estão dentro do intervalo
    debug_assert!(n >= 1 && n <= SQLITE_SHM_NLOCK);
    debug_assert!(ofst >= UNIX_SHM_BASE && ofst <= (UNIX_SHM_DMS + SQLITE_SHM_NLOCK));

    let h_shm = p_shm_node.borrow().h_shm;
    if h_shm >= 0 {
        // Inicializa os parâmetros de lock
        let mut f = FLock::default();
        f.l_type = lock_type as _;
        f.l_whence = SEEK_SET as _;
        f.l_start = ofst as _;
        f.l_len = n as _;
        let res = os_set_posix_advisory_lock(h_shm, &mut f, p_file);
        if res == -1 {
            rc = SQLITE_BUSY;
        }
    }

    rc
}

/// Retorna o número mínimo de regiões shm de 32KB que devem ser mapeadas de uma vez, supondo
/// que cada mapeamento deve ser múltiplo inteiro do tamanho de página do sistema.
///
/// Normalmente é 1. A exceção são sistemas com páginas de 64KB, onde cada mapeamento cobre
/// pelo menos duas regiões shm.
pub fn unix_shm_region_per_map() -> i32 {
    let shmsz = 32 * 1024; // tamanho da região SHM
    let pgsz = os_getpagesize(); // tamanho de página do sistema
    debug_assert!(((pgsz - 1) & pgsz) == 0); // a página deve ser potência de 2
    if pgsz < shmsz {
        return 1;
    }
    pgsz / shmsz
}

/// Elimina o UnixShmNode do inode de `p_fd` se `n_ref==0`.
///
/// Não é um método VFS de memória compartilhada: é uma função utilitária chamada por eles.
pub fn unix_shm_purge(p_fd: &mut UnixFile) {
    let p_inode = p_fd.p_inode.clone().unwrap();
    let p_node = p_inode.borrow().p_shm_node.clone();
    if let Some(p) = p_node {
        if p.borrow().n_ref == 0 {
            let n_shm_per_map = unix_shm_region_per_map() as usize;
            {
                let mut node = p.borrow_mut();
                mutex_free(node.p_shm_mutex.take());
                let n_region = node.n_region as usize;
                let h_shm = node.h_shm;
                let sz_region = node.sz_region;
                let mut i = 0usize;
                while i < n_region {
                    let region = node.ap_region[i].take();
                    if h_shm >= 0 {
                        os_munmap(region, sz_region);
                    }
                    // sem arquivo, a região é memória comum e basta soltá-la
                    i += n_shm_per_map;
                }
                node.ap_region = Vec::new();
                if node.h_shm >= 0 {
                    robust_close(p_fd, node.h_shm, line!() as i32);
                    node.h_shm = -1;
                }
            }
            p_inode.borrow_mut().p_shm_node = None;
        }
    }
}

/// O lock DMS ainda não foi obtido no arquivo shm `p_shm_node`. Tenta obtê-lo agora. Retorna
/// SQLITE_OK em sucesso ou um código de erro do SQLite.
///
/// Se o DMS não pode ser travado porque esta é uma conexão readonly_shm=1 e nenhum outro
/// processo mantém lock, retorna SQLITE_READONLY_CANTINIT e liga `is_unlocked`.
pub fn unix_lock_shared_memory(p_db_fd: &mut UnixFile, p_shm_node: &mut UnixShmNode) -> i32 {
    let mut lock = FLock::default();
    let mut rc = SQLITE_OK;

    // Usa F_GETLK para saber os locks que outros processos mantêm no byte DMS. Se outro
    // processo mantém lock SHARED, este também pode tomar SHARED e abrir o arquivo *-shm.
    //
    // Se nenhum outro processo mantém lock, este é o primeiro a abri-lo: toma um lock
    // EXCLUSIVE no byte DMS, trunca o *-shm e rebaixa para SHARED.
    //
    // Se outro processo mantém EXCLUSIVE no byte DMS, retorna SQLITE_BUSY ao chamador (que
    // tenta de novo). Uma versão anterior tentava o lock SHARED neste ponto, mas isso criava
    // uma condição de corrida: se o processo com EXCLUSIVE falhasse antes de truncar o *-shm,
    // este poderia usá-lo sem truncar, e um *-shm corrompido por queda de energia poderia
    // corromper o próprio banco.
    lock.l_whence = SEEK_SET as _;
    lock.l_start = UNIX_SHM_DMS as _;
    lock.l_len = 1 as _;
    lock.l_type = F_WRLCK as _;
    if os_fcntl(p_shm_node.h_shm, F_GETLK, &mut lock) != 0 {
        rc = SQLITE_IOERR_LOCK;
    } else if lock.l_type as i32 == F_UNLCK {
        if p_shm_node.is_readonly != 0 {
            p_shm_node.is_unlocked = 1;
            rc = SQLITE_READONLY_CANTINIT;
        } else {
            rc = unix_shm_system_lock(p_db_fd, F_WRLCK, UNIX_SHM_DMS, 1);

            // A primeira conexão a entrar deve truncar o arquivo -shm. Trunca para 3 bytes
            // (um número pequeno e arbitrário, menor que o cabeçalho do -shm) e não para 0,
            // como auxílio de depuração do sistema, para detectar se o truncamento do -shm
            // é legítimo ou obra de um processo desgarrado.
            if rc == SQLITE_OK && robust_ftruncate(p_shm_node.h_shm, 3) != 0 {
                rc = unix_log_error(SQLITE_IOERR_SHMOPEN, "ftruncate", &p_shm_node.z_filename);
            }
        }
    } else if lock.l_type as i32 == F_WRLCK {
        rc = SQLITE_BUSY;
    }

    if rc == SQLITE_OK {
        debug_assert!(lock.l_type as i32 == F_UNLCK || lock.l_type as i32 == F_RDLCK);
        rc = unix_shm_system_lock(p_db_fd, F_RDLCK, UNIX_SHM_DMS, 1);
    }
    rc
}


// ---- part_015.rs ----

use std::sync::Mutex;

// O trecho C começa no fim do bloco Apple (`autolockIoFinder`) e o bloco
// `OS_VXWORKS` (`vxworksIoFinderImpl`) também some: o alvo é o Debian 13, e
// `SQLITE_ENABLE_LOCKING_STYLE` vale 0 fora do Apple.

/// Tipo abstrato de uma função que escolhe o método de E/S (estilo de lock)
/// a partir do nome do arquivo e do objeto aberto. É o que o C guarda em
/// `pVfs->pAppData` e chama como `(**(finder_type*)pVfs->pAppData)(...)`.
pub type FinderType = fn(Option<&[u8]>, &mut UnixFile) -> &'static Sqlite3IoMethods;

/// Sufixo do arquivo de lock do estilo dotfile.
const DOTLOCK_SUFFIX_BYTES: &[u8] = b".lock";

/// Inicializa o conteúdo do `UnixFile` apontado por `p_new`.
///
/// `p_vfs` é o VFS dono, `h` o descritor aberto, `z_filename` o nome do
/// arquivo (`None` em arquivo transiente) e `ctrl_flags` zero ou mais
/// valores `UNIXFILE_*`. O `find_inode_info` grava `p_new.p_inode`.
fn fill_in_unix_file(
    p_vfs: &Rc<Sqlite3Vfs>,
    mut h: i32,
    p_new: &mut UnixFile,
    z_filename: Option<&[u8]>,
    ctrl_flags: i32,
) -> i32 {
    let p_locking_style: &'static Sqlite3IoMethods;
    let mut rc = SQLITE_OK;

    debug_assert!(p_new.p_inode.is_none());

    // Sem lock em arquivo temporário.
    debug_assert!(z_filename.is_some() || (ctrl_flags & UNIXFILE_NOLOCK) != 0);

    p_new.h = h;
    p_new.p_vfs = Some(Rc::clone(p_vfs));
    p_new.z_path = z_filename.map(|z| z.to_vec());
    p_new.ctrl_flags = ctrl_flags as u8;
    // SQLITE_MAX_MMAP_SIZE>0 no Linux.
    p_new.mmap_size_max = sqlite3_global_config().sz_mmap;
    let uri_name = if (ctrl_flags & UNIXFILE_URI) != 0 {
        z_filename
    } else {
        None
    };
    if uri_boolean(uri_name, b"psow", SQLITE_POWERSAFE_OVERWRITE) != 0 {
        p_new.ctrl_flags |= UNIXFILE_PSOW as u8;
    }
    if p_vfs.z_name.as_slice() == b"unix-excl" {
        p_new.ctrl_flags |= UNIXFILE_EXCL as u8;
    }

    if (ctrl_flags & UNIXFILE_NOLOCK) != 0 {
        p_locking_style = &NOLOCK_IO_METHODS;
    } else {
        p_locking_style = (p_vfs.p_app_data)(z_filename, p_new);
    }

    if ptr::eq(p_locking_style, &POSIX_IO_METHODS) {
        unix_enter_mutex();
        rc = find_inode_info(p_new);
        if rc != SQLITE_OK {
            // Se find_inode_info falhou (fstat ou malloc), fecha o descritor
            // já, antes de soltar o mutex: nenhum lock posix é perdido no caso
            // do malloc, e no caso do fstat as coisas já estão ruins demais.
            robust_close(p_new, h, line!() as i32);
            h = -1;
        }
        unix_leave_mutex();
    } else if ptr::eq(p_locking_style, &DOTLOCK_IO_METHODS) {
        // O lock por dotfile usa o caminho, então ele entra no
        // dotlockLockingContext.
        debug_assert!(z_filename.is_some());
        let z_filename_bytes = z_filename.unwrap_or(&[]);
        let n_filename = z_filename_bytes.len() + 6;
        let mut z_lock_file: Vec<u8> = Vec::new();
        if z_lock_file.try_reserve_exact(n_filename).is_err() {
            rc = SQLITE_NOMEM_BKPT;
            p_new.locking_context = None;
        } else {
            z_lock_file.extend_from_slice(z_filename_bytes);
            z_lock_file.extend_from_slice(DOTLOCK_SUFFIX_BYTES);
            p_new.locking_context = Some(z_lock_file);
        }
    }

    store_last_errno(p_new, 0);
    if rc != SQLITE_OK {
        if h >= 0 {
            robust_close(p_new, h, line!() as i32);
        }
    } else {
        p_new.p_methods = Some(p_locking_style);
        verify_db_file(p_new);
    }
    rc
}

/// Os dois primeiros diretórios candidatos para arquivos temporários, lidos
/// de `SQLITE_TMPDIR` e `TMPDIR` por `unix_temp_file_init`.
static AZ_TEMP_DIRS_ENV: Mutex<[Option<Vec<u8>>; 2]> = Mutex::new([None, None]);

/// Número de entradas de `azTempDirs[]` no C.
const AZ_TEMP_DIRS_LEN: usize = 6;

/// Entrada `i` de `azTempDirs[]`: duas de ambiente, depois `/var/tmp`,
/// `/usr/tmp`, `/tmp` e `.`.
fn az_temp_dirs(i: usize) -> Option<Vec<u8>> {
    match i {
        0 | 1 => AZ_TEMP_DIRS_ENV
            .lock()
            .unwrap_or_else(|e| e.into_inner())[i]
            .clone(),
        2 => Some(b"/var/tmp".to_vec()),
        3 => Some(b"/usr/tmp".to_vec()),
        4 => Some(b"/tmp".to_vec()),
        _ => Some(b".".to_vec()),
    }
}

/// Inicializa os dois primeiros membros de `azTempDirs[]`.
fn unix_temp_file_init() {
    use std::os::unix::ffi::OsStrExt;
    let mut g = AZ_TEMP_DIRS_ENV.lock().unwrap_or_else(|e| e.into_inner());
    g[0] = std::env::var_os("SQLITE_TMPDIR").map(|v| v.as_bytes().to_vec());
    g[1] = std::env::var_os("TMPDIR").map(|v| v.as_bytes().to_vec());
}

/// Devolve o nome de um diretório para arquivos temporários, ou `None` se
/// nenhum serve.
fn unix_temp_file_dir() -> Option<Vec<u8>> {
    let mut i: usize = 0;
    let mut z_dir: Option<Vec<u8>> = sqlite3_temp_directory();

    loop {
        let usable = match &z_dir {
            Some(d) => match os_stat(d) {
                Some(buf) => s_isdir(buf.st_mode) && os_access(d, 0o3) == 0,
                None => false,
            },
            None => false,
        };
        if usable {
            return z_dir;
        }
        if i >= AZ_TEMP_DIRS_LEN {
            break;
        }
        z_dir = az_temp_dirs(i);
        i += 1;
    }
    None
}

/// Cria um nome de arquivo temporário em `z_buf`, que precisa ter pelo menos
/// `n_buf` bytes (o `pVfs->mxPathname`).
fn unix_get_tempname(n_buf: i32, z_buf: &mut [u8]) -> i32 {
    let mut i_limit: i32 = 0;
    let mut rc = SQLITE_OK;

    z_buf[0] = 0;

    let p_mutex = mutex_alloc(SQLITE_MUTEX_STATIC_TEMPDIR);
    mutex_enter(&p_mutex);
    let z_dir = unix_temp_file_dir();
    match z_dir {
        None => {
            rc = SQLITE_IOERR_GETTEMPPATH;
        }
        Some(z_dir) => loop {
            let mut r_bytes = [0u8; 8];
            randomness(&mut r_bytes);
            let r = u64::from_ne_bytes(r_bytes);
            debug_assert!(n_buf > 2);
            let n = n_buf as usize;
            z_buf[n - 2] = 0;

            // "%s/" SQLITE_TEMP_FILE_PREFIX "%llx%c" com %c de 0: o texto
            // completo termina em um NUL embutido, e o snprintf trunca em
            // n_buf - 1 bytes seguidos do NUL final.
            let mut full: Vec<u8> = Vec::new();
            full.extend_from_slice(&z_dir);
            full.push(b'/');
            full.extend_from_slice(SQLITE_TEMP_FILE_PREFIX.as_bytes());
            full.extend_from_slice(format!("{:x}", r).as_bytes());
            full.push(0);
            let take = full.len().min(n - 1);
            z_buf[..take].copy_from_slice(&full[..take]);
            z_buf[take] = 0;

            let overflow = z_buf[n - 2] != 0;
            let over_limit = i_limit > 10;
            i_limit += 1;
            if overflow || over_limit {
                rc = SQLITE_ERROR;
                break;
            }
            let end = z_buf.iter().position(|&b| b == 0).unwrap_or(n);
            if os_access(&z_buf[..end], 0) != 0 {
                break;
            }
        },
    }
    mutex_leave(&p_mutex);
    rc
}

/// Procura um descritor não usado, aberto no arquivo de banco de dados
/// (não journal nem super-journal) de caminho `z_path` e com flags
/// `SQLITE_OPEN_*` iguais às de `flags`.
///
/// Tal descritor existe quando uma conexão foi fechada mas o descritor não
/// pôde ser fechado porque outro descritor no mesmo arquivo segura um
/// file-lock (ver `unix_close` e o comentário "Posix Advisory Locking").
/// Devolve o descritor, já desligado da lista, ou `None`.
fn find_reusable_fd(z_path: &[u8], flags: i32) -> Option<Box<UnixUnusedFd>> {
    let mut p_unused: Option<Box<UnixUnusedFd>> = None;

    unix_enter_mutex();

    // Um stat() pode falhar; nesse caso o open() seguinte quase certamente
    // também falha, então o erro é ignorado e nada é reaproveitado.
    if !inode_list().is_empty() {
        if let Some(s_stat) = os_stat(z_path) {
            let mut found: Option<UnixInodeInfoRef> = None;
            for p_inode in inode_list() {
                let hit = {
                    let ino = p_inode.borrow();
                    ino.file_id.dev == s_stat.st_dev as u64 && ino.file_id.ino == s_stat.st_ino as u64
                };
                if hit {
                    found = Some(p_inode);
                    break;
                }
            }
            if let Some(p_inode) = found {
                let mut ino = p_inode.borrow_mut();
                mutex_enter(&ino.p_lock_mutex);
                let flags = flags & (SQLITE_OPEN_READONLY | SQLITE_OPEN_READWRITE);
                let mut pp = &mut ino.p_unused;
                while pp.as_ref().map_or(false, |n| n.flags != flags) {
                    pp = &mut pp.as_mut().unwrap().p_next;
                }
                p_unused = pp.take();
                if let Some(u) = p_unused.as_mut() {
                    *pp = u.p_next.take();
                }
                mutex_leave(&ino.p_lock_mutex);
            }
        }
    }
    unix_leave_mutex();
    p_unused
}


// ---- part_016.rs ----

/// Encontra o modo, uid e gid do arquivo `z_file`.
fn get_file_mode(z_file: &[u8], p_mode: &mut u32, p_uid: &mut u32, p_gid: &mut u32) -> i32 {
    let mut s_stat = Stat::default();
    let mut rc = SQLITE_OK;
    if 0 == os_stat(z_file, &mut s_stat) {
        *p_mode = s_stat.st_mode & 0o777;
        *p_uid = s_stat.st_uid;
        *p_gid = s_stat.st_gid;
    } else {
        rc = SQLITE_IOERR_FSTAT;
    }
    rc
}

/// Esta função é chamada por `unix_open()` para determinar as permissões Unix
/// com que criar novos arquivos. Se nenhum erro ocorrer, `SQLITE_OK` é retornado
/// e um valor adequado para o terceiro argumento de open(2) é escrito em
/// `*p_mode`. Se ocorrer um erro de IO, um código de erro do SQLite é retornado
/// e o valor de `*p_mode` não é modificado.
///
/// Na maioria dos casos esta rotina define `*p_mode` como 0, o que se torna uma
/// indicação para `robust_open()` criar o arquivo com
/// `SQLITE_DEFAULT_FILE_PERMISSIONS` ajustado pela umask. Mas se o arquivo
/// aberto for um WAL ou um journal regular, a função consulta o sistema de
/// arquivos pelas permissões do arquivo de banco correspondente e define
/// `*p_mode` com esse valor. Sempre que possível, WAL e journal são criados com
/// as mesmas permissões do banco associado.
///
/// Se `SQLITE_ENABLE_8_3_NAMES` estivesse habilitada, o nome original do arquivo
/// não estaria disponível; essa opção não existe no Debian, então não há ramo
/// para ela.
fn find_create_file_mode(
    z_path: &[u8],
    flags: i32,
    p_mode: &mut u32,
    p_uid: &mut u32,
    p_gid: &mut u32,
) -> i32 {
    let mut rc = SQLITE_OK;
    *p_mode = 0;
    *p_uid = 0;
    *p_gid = 0;
    if (flags & (SQLITE_OPEN_WAL | SQLITE_OPEN_MAIN_JOURNAL)) != 0 {
        // `z_path` é o caminho de um WAL ou journal. Este bloco deriva o caminho
        // do banco associado. Trata os nomes "<banco>-journal", "<banco>-wal",
        // "<banco>-journalNN" e "<banco>-walNN", onde NN é um número decimal
        // (usado pelo módulo test_multiplex.c). Se o '-' faltar ou for o primeiro
        // caractere, retorna `SQLITE_OK` com `*p_mode==0`.
        let mut n_db: usize = strlen30(z_path) as usize;
        n_db = n_db.wrapping_sub(1);
        while n_db > 0 && n_db < z_path.len() && z_path[n_db] != b'.' {
            if z_path[n_db] == b'-' {
                let z_db = &z_path[..n_db];
                rc = get_file_mode(z_db, p_mode, p_uid, p_gid);
                break;
            }
            n_db -= 1;
        }
    } else if (flags & SQLITE_OPEN_DELETEONCLOSE) != 0 {
        *p_mode = 0o600;
    } else if (flags & SQLITE_OPEN_URI) != 0 {
        // Arquivo principal aberto por URI: procura o parâmetro "modeof". Se
        // presente, interpreta o valor como nome de arquivo e copia dele o modo,
        // uid e gid.
        if let Some(z) = api::uri_parameter(z_path, b"modeof") {
            rc = get_file_mode(&z, p_mode, p_uid, p_gid);
        }
    }
    rc
}

/// Abre o arquivo `z_path`.
///
/// Antes, a camada de SO do SQLite usava três funções no lugar desta:
/// `OpenReadWrite` (READWRITE|CREATE), `OpenReadOnly` (READONLY) e
/// `OpenExclusive` (READWRITE|CREATE|EXCLUSIVE). O antigo `OpenExclusive`
/// aceitava um booleano "delFlag"; para o mesmo efeito, some DELETEONCLOSE.
fn unix_open(
    p_vfs: &Sqlite3Vfs,
    z_path: Option<&[u8]>,
    p: &mut UnixFile,
    mut flags: i32,
    p_out_flags: Option<&mut i32>,
) -> i32 {
    let mut fd: i32 = -1;
    let mut open_flags: i32 = 0;
    let e_type: i32 = flags & 0x0FFF00;
    let mut rc: i32 = SQLITE_OK;
    let mut ctrl_flags: i32 = 0;

    let is_exclusive = (flags & SQLITE_OPEN_EXCLUSIVE) != 0;
    let is_delete = (flags & SQLITE_OPEN_DELETEONCLOSE) != 0;
    let is_create = (flags & SQLITE_OPEN_CREATE) != 0;
    let mut is_readonly = (flags & SQLITE_OPEN_READONLY) != 0;
    let is_read_write = (flags & SQLITE_OPEN_READWRITE) != 0;

    // Criando journal de super ou principal, abre também um descritor no
    // diretório; o primeiro `unix_sync()` faz fsync e close nele.
    let is_new_jrnl = is_create
        && (e_type == SQLITE_OPEN_SUPER_JOURNAL
            || e_type == SQLITE_OPEN_MAIN_JOURNAL
            || e_type == SQLITE_OPEN_WAL);

    // Se `z_path` for None, é preciso abrir um arquivo temporário e este buffer
    // guarda o nome.
    let mut z_tmpname: Vec<u8> = Vec::new();
    let mut z_name: Option<&[u8]> = z_path;

    debug_assert!((!is_readonly || !is_read_write) && (is_read_write || is_readonly));
    debug_assert!(!is_create || is_read_write);
    debug_assert!(!is_exclusive || is_create);
    debug_assert!(!is_delete || is_create);

    debug_assert!((!is_delete && z_name.is_some()) || e_type != SQLITE_OPEN_MAIN_DB);
    debug_assert!((!is_delete && z_name.is_some()) || e_type != SQLITE_OPEN_MAIN_JOURNAL);
    debug_assert!((!is_delete && z_name.is_some()) || e_type != SQLITE_OPEN_SUPER_JOURNAL);
    debug_assert!((!is_delete && z_name.is_some()) || e_type != SQLITE_OPEN_WAL);

    debug_assert!(
        e_type == SQLITE_OPEN_MAIN_DB
            || e_type == SQLITE_OPEN_TEMP_DB
            || e_type == SQLITE_OPEN_MAIN_JOURNAL
            || e_type == SQLITE_OPEN_TEMP_JOURNAL
            || e_type == SQLITE_OPEN_SUBJOURNAL
            || e_type == SQLITE_OPEN_SUPER_JOURNAL
            || e_type == SQLITE_OPEN_TRANSIENT_DB
            || e_type == SQLITE_OPEN_WAL
    );

    // Detecta mudança de pid e reinicia o PRNG. Há uma corrida em que vários
    // threads reiniciam o PRNG, mas reinícios múltiplos são inofensivos.
    if RANDOMNESS_PID.load(std::sync::atomic::Ordering::Relaxed) != os_getpid() {
        RANDOMNESS_PID.store(os_getpid(), std::sync::atomic::Ordering::Relaxed);
        api::randomness(&mut []);
    }
    *p = UnixFile::default();

    if e_type == SQLITE_OPEN_MAIN_DB {
        let p_unused = match find_reusable_fd(z_name, flags) {
            Some(u) => {
                fd = u.fd;
                u
            }
            None => Box::new(UnixUnusedFd::default()),
        };
        p.p_preallocated_unused = Some(p_unused);
    } else if z_name.is_none() {
        // Sem nome, a camada superior pede um arquivo temporário.
        debug_assert!(is_delete && !is_new_jrnl);
        rc = unix_get_tempname(p_vfs.mx_pathname, &mut z_tmpname);
        if rc != SQLITE_OK {
            return rc;
        }
        z_name = Some(&z_tmpname);
    }

    // Calcula as flags para open(2). Precisam ser calculadas mesmo sem chamar
    // open(), pois podem ser guardadas no handle.
    if is_readonly {
        open_flags |= O_RDONLY;
    }
    if is_read_write {
        open_flags |= O_RDWR;
    }
    if is_create {
        open_flags |= O_CREAT;
    }
    if is_exclusive {
        open_flags |= O_EXCL | O_NOFOLLOW;
    }
    open_flags |= O_LARGEFILE | O_BINARY | O_NOFOLLOW;

    'open_finished: {
        if fd < 0 {
            let mut open_mode: u32 = 0;
            let mut uid: u32 = 0;
            let mut gid: u32 = 0;
            // Aqui `z_name` é sempre Some: arquivo principal ou temporário
            // têm nome, os demais tipos sempre chegam com nome.
            let name: &[u8] = z_name.unwrap_or(&[]);
            rc = find_create_file_mode(name, flags, &mut open_mode, &mut uid, &mut gid);
            if rc != SQLITE_OK {
                debug_assert!(p.p_preallocated_unused.is_none());
                debug_assert!(e_type == SQLITE_OPEN_WAL || e_type == SQLITE_OPEN_MAIN_JOURNAL);
                return rc;
            }
            fd = robust_open(name, open_flags, open_mode);
            debug_assert!(!is_exclusive || (open_flags & O_CREAT) != 0);
            if fd < 0 {
                let err_no = get_errno();
                if is_new_jrnl && err_no == EACCES && os_access(name, F_OK) != 0 {
                    // Sem poder criar o journal porque o diretório não é
                    // gravável, troca o código de erro para indicar isso.
                    rc = SQLITE_READONLY_DIRECTORY;
                } else if err_no != EISDIR && is_read_write {
                    // Falhou abrir para leitura e escrita: tenta só leitura.
                    flags &= !(SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE);
                    open_flags &= !(O_RDWR | O_CREAT);
                    flags |= SQLITE_OPEN_READONLY;
                    open_flags |= O_RDONLY;
                    is_readonly = true;
                    match find_reusable_fd(z_name, flags) {
                        Some(p_readonly) => {
                            fd = p_readonly.fd;
                        }
                        None => {
                            fd = robust_open(name, open_flags, open_mode);
                        }
                    }
                }
            }
            if fd < 0 {
                let rc2 = unix_log_error_at_line(
                    SQLITE_CANTOPEN_BKPT,
                    "open",
                    Some(name),
                    line!() as i32,
                );
                if rc == SQLITE_OK {
                    rc = rc2;
                }
                break 'open_finished;
            }

            // O dono do journal de rollback ou do WAL deve ser o mesmo do banco.
            // Tenta garantir isso; o chown vira no-op sem privilégio de root.
            // Se `open_mode==0`, uid e gid não estão corretos e não se tenta.
            if open_mode != 0 && (flags & (SQLITE_OPEN_WAL | SQLITE_OPEN_MAIN_JOURNAL)) != 0 {
                robust_fchown(fd, uid, gid);
            }
        }
        debug_assert!(fd >= 0);
        if let Some(out_flags) = p_out_flags {
            *out_flags = flags;
        }

        if let Some(unused) = p.p_preallocated_unused.as_mut() {
            unused.fd = fd;
            unused.flags = flags & (SQLITE_OPEN_READONLY | SQLITE_OPEN_READWRITE);
        }

        if is_delete {
            // SQLITE_UNLINK_AFTER_CLOSE não está definido no Debian.
            if let Some(name) = z_name {
                os_unlink(name);
            }
        }

        // Monta as ctrl_flags apropriadas.
        if is_delete {
            ctrl_flags |= UNIXFILE_DELETE;
        }
        if is_readonly {
            ctrl_flags |= UNIXFILE_RDONLY;
        }
        let no_lock = e_type != SQLITE_OPEN_MAIN_DB;
        if no_lock {
            ctrl_flags |= UNIXFILE_NOLOCK;
        }
        if is_new_jrnl {
            ctrl_flags |= UNIXFILE_DIRSYNC;
        }
        if (flags & SQLITE_OPEN_URI) != 0 {
            ctrl_flags |= UNIXFILE_URI;
        }

        debug_assert!(
            z_path.map_or(true, |z| z.first() == Some(&b'/'))
                || e_type == SQLITE_OPEN_SUPER_JOURNAL
                || e_type == SQLITE_OPEN_MAIN_JOURNAL
        );
        rc = fill_in_unix_file(p_vfs, fd, p, z_path, ctrl_flags);
    }

    // open_finished:
    if rc != SQLITE_OK {
        p.p_preallocated_unused = None;
    }
    rc
}


// ---- part_017.rs ----

/// Apaga o arquivo em `z_path`. Se `dir_sync` for verdadeiro, faz fsync() no
/// diretório depois de apagar o arquivo.
fn unix_delete(_not_used: Option<&Sqlite3Vfs>, z_path: &[u8], dir_sync: i32) -> i32 {
    let mut rc = SQLITE_OK;
    if os_unlink(z_path) == -1 {
        if get_errno() == ENOENT {
            rc = SQLITE_IOERR_DELETE_NOENT;
        } else {
            rc = unix_log_error_at_line(SQLITE_IOERR_DELETE, "unlink", Some(z_path), line!() as i32);
        }
        return rc;
    }
    // SQLITE_DISABLE_DIRSYNC não está definido no Debian.
    if (dir_sync & 1) != 0 {
        let mut fd: i32 = -1;
        rc = os_open_directory(z_path, &mut fd);
        if rc == SQLITE_OK {
            if full_fsync(fd, 0, 0) != 0 {
                rc = unix_log_error_at_line(
                    SQLITE_IOERR_DIR_FSYNC,
                    "fsync",
                    Some(z_path),
                    line!() as i32,
                );
            }
            robust_close(None, fd, line!() as i32);
        } else {
            debug_assert!(rc == SQLITE_CANTOPEN);
            rc = SQLITE_OK;
        }
    }
    rc
}

/// Testa a existência ou as permissões de acesso do arquivo `z_path`. O teste
/// depende de `flags`:
///
///   SQLITE_ACCESS_EXISTS: devolve 1 se o arquivo existe
///   SQLITE_ACCESS_READWRITE: devolve 1 se o arquivo é legível e gravável
///   SQLITE_ACCESS_READONLY: devolve 1 se o arquivo é legível
///
/// Caso contrário devolve 0.
fn unix_access(_not_used: Option<&Sqlite3Vfs>, z_path: &[u8], flags: i32, p_res_out: &mut i32) -> i32 {
    // A especificação cita três valores possíveis para `flags`, mas só dois são
    // realmente usados.
    debug_assert!(flags == SQLITE_ACCESS_EXISTS || flags == SQLITE_ACCESS_READWRITE);

    if flags == SQLITE_ACCESS_EXISTS {
        let mut buf = Stat::default();
        *p_res_out = (0 == os_stat(z_path, &mut buf) && (!s_isreg(buf.st_mode) || buf.st_size > 0))
            as i32;
    } else {
        *p_res_out = (os_access(z_path, W_OK | R_OK) == 0) as i32;
    }
    SQLITE_OK
}

/// Um caminho em construção.
struct DbPath<'a> {
    /// Não zero depois de qualquer erro.
    rc: i32,
    /// Número de symlinks resolvidos.
    n_symlink: i32,
    /// O caminho é escrito aqui.
    z_out: &'a mut [u8],
    /// Bytes de espaço disponíveis em `z_out`.
    n_out: i32,
    /// Bytes de `z_out` atualmente em uso.
    n_used: i32,
}

/// Acrescenta um único elemento de caminho ao `DbPath` em construção.
fn append_one_path_element(p_path: &mut DbPath, z_name: &[u8]) {
    let n_name = z_name.len() as i32;
    debug_assert!(n_name > 0);
    if z_name[0] == b'.' {
        if n_name == 1 {
            return;
        }
        if z_name[1] == b'.' && n_name == 2 {
            if p_path.n_used > 1 {
                debug_assert!(p_path.z_out[0] == b'/');
                loop {
                    p_path.n_used -= 1;
                    if p_path.z_out[p_path.n_used as usize] == b'/' {
                        break;
                    }
                }
            }
            return;
        }
    }
    if p_path.n_used + n_name + 2 >= p_path.n_out {
        p_path.rc = SQLITE_ERROR;
        return;
    }
    p_path.z_out[p_path.n_used as usize] = b'/';
    p_path.n_used += 1;
    let n_used = p_path.n_used as usize;
    p_path.z_out[n_used..n_used + z_name.len()].copy_from_slice(z_name);
    p_path.n_used += n_name;
    // HAVE_READLINK e HAVE_LSTAT valem no Debian.
    if p_path.rc == SQLITE_OK {
        let n_used = p_path.n_used as usize;
        p_path.z_out[n_used] = 0;
        let z_in: Vec<u8> = p_path.z_out[..n_used].to_vec();
        let mut buf = Stat::default();
        if os_lstat(&z_in, &mut buf) != 0 {
            if get_errno() != ENOENT {
                p_path.rc =
                    unix_log_error_at_line(SQLITE_CANTOPEN_BKPT, "lstat", Some(&z_in), line!() as i32);
            }
        } else if s_islnk(buf.st_mode) {
            let mut z_lnk = vec![0u8; SQLITE_MAX_PATHLEN + 2];
            let n_prev = p_path.n_symlink;
            p_path.n_symlink += 1;
            if n_prev > SQLITE_MAX_SYMLINK {
                p_path.rc = SQLITE_CANTOPEN_BKPT;
                return;
            }
            let got = os_readlink(&z_in, &mut z_lnk[..SQLITE_MAX_PATHLEN]);
            if got <= 0 || got >= SQLITE_MAX_PATHLEN as isize {
                p_path.rc =
                    unix_log_error_at_line(SQLITE_CANTOPEN_BKPT, "readlink", Some(&z_in), line!() as i32);
                return;
            }
            let got = got as usize;
            z_lnk[got] = 0;
            if z_lnk[0] == b'/' {
                p_path.n_used = 0;
            } else {
                p_path.n_used -= n_name + 1;
            }
            append_all_path_elements(p_path, &z_lnk[..got]);
        }
    }
}

/// Acrescenta todos os elementos de caminho de `z_path` ao `DbPath` em
/// construção. `z_path` termina no primeiro NUL ou no fim da fatia.
fn append_all_path_elements(p_path: &mut DbPath, z_path: &[u8]) {
    let at = |k: usize| -> u8 { z_path.get(k).copied().unwrap_or(0) };
    let mut i: usize = 0;
    let mut j: usize = 0;
    loop {
        while at(i) != 0 && at(i) != b'/' {
            i += 1;
        }
        if i > j {
            append_one_path_element(p_path, &z_path[j..i]);
        }
        j = i + 1;
        let c = at(i);
        i += 1;
        if c == 0 {
            break;
        }
    }
}

/// Transforma um caminho relativo em caminho completo. O caminho relativo está
/// em `z_path`; `z_out` tem pelo menos `mx_pathname` bytes (aqui `MAX_PATHNAME`)
/// e recebe o caminho completo antes do retorno.
fn unix_full_pathname(_p_vfs: Option<&Sqlite3Vfs>, z_path: &[u8], n_out: i32, z_out: &mut [u8]) -> i32 {
    let mut path = DbPath { rc: 0, n_used: 0, n_symlink: 0, n_out, z_out };
    if z_path.first() != Some(&b'/') {
        let mut z_pwd = vec![0u8; SQLITE_MAX_PATHLEN + 2];
        match os_getcwd(&mut z_pwd[..SQLITE_MAX_PATHLEN]) {
            None => {
                return unix_log_error_at_line(
                    SQLITE_CANTOPEN_BKPT,
                    "getcwd",
                    Some(z_path),
                    line!() as i32,
                );
            }
            Some(n) => append_all_path_elements(&mut path, &z_pwd[..n]),
        }
    }
    append_all_path_elements(&mut path, z_path);
    let n_used = path.n_used as usize;
    path.z_out[n_used] = 0;
    if path.rc != 0 || path.n_used < 2 {
        return sqlite_cantopen_bkpt(line!() as i32);
    }
    if path.n_symlink != 0 {
        return SQLITE_OK_SYMLINK;
    }
    SQLITE_OK
}

/// Última mensagem de erro do carregador dinâmico (o que `dlerror()` devolveria).
static DL_LAST_ERROR: std::sync::Mutex<Option<Vec<u8>>> = std::sync::Mutex::new(None);

/// Abre uma biblioteca compartilhada. Com `forbid(unsafe_code)` não existe
/// dlopen real: a falha é registrada com a mensagem do glibc para arquivo
/// inexistente e o handle devolvido é sempre `None`.
fn unix_dl_open(_not_used: Option<&Sqlite3Vfs>, z_filename: &[u8]) -> Option<usize> {
    let mut msg = z_filename.to_vec();
    if std::fs::metadata(String::from_utf8_lossy(z_filename).as_ref()).is_ok() {
        msg.extend_from_slice(b": cannot open shared object file: Invalid argument");
    } else {
        msg.extend_from_slice(b": cannot open shared object file: No such file or directory");
    }
    if let Ok(mut g) = DL_LAST_ERROR.lock() {
        *g = Some(msg);
    }
    None
}

/// O SQLite chama esta função logo depois de `unix_dl_sym()` ou `unix_dl_open()`
/// falhar. Se houver mensagem detalhada, ela é escrita em `z_buf_out` (cortada
/// em `n_buf - 1` bytes, como `sqlite3_snprintf`); senão o buffer fica intacto.
fn unix_dl_error(_not_used: Option<&Sqlite3Vfs>, n_buf: i32, z_buf_out: &mut [u8]) {
    unix_enter_mutex();
    let z_err = match DL_LAST_ERROR.lock() {
        Ok(mut g) => g.take(),
        Err(_) => None,
    };
    if let Some(err) = z_err {
        api::snprintf(n_buf, z_buf_out, b"%s", &[PrintfArg::Text(&err)]);
    }
    unix_leave_mutex();
}

/// Procura um símbolo numa biblioteca aberta. Sem dlopen real, nunca há
/// biblioteca aberta, então o resultado é sempre `None`.
fn unix_dl_sym(_not_used: Option<&Sqlite3Vfs>, _p: usize, _z_sym: &[u8]) -> Option<usize> {
    None
}

/// Fecha uma biblioteca aberta (no-op, ver `unix_dl_open`).
fn unix_dl_close(_not_used: Option<&Sqlite3Vfs>, _p_handle: usize) {}

/// Escreve `n_buf` bytes de dados aleatórios em `z_buf`.
fn unix_randomness(_not_used: Option<&Sqlite3Vfs>, mut n_buf: i32, z_buf: &mut [u8]) -> i32 {
    debug_assert!(n_buf as usize >= std::mem::size_of::<i64>() + std::mem::size_of::<i32>());

    // Inicializa `z_buf` com zeros (silencia o valgrind, como no C).
    z_buf[..n_buf as usize].fill(0);
    let pid = os_getpid();
    RANDOMNESS_PID.store(pid, std::sync::atomic::Ordering::Relaxed);
    // SQLITE_TEST e SQLITE_OMIT_RANDOMNESS não estão definidos.
    {
        use std::io::Read;
        match std::fs::File::open("/dev/urandom") {
            Err(_) => {
                let t: i64 = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let tb = t.to_ne_bytes();
                z_buf[..tb.len()].copy_from_slice(&tb);
                let pb = pid.to_ne_bytes();
                z_buf[tb.len()..tb.len() + pb.len()].copy_from_slice(&pb);
                debug_assert!(tb.len() + pb.len() <= n_buf as usize);
                n_buf = (tb.len() + pb.len()) as i32;
            }
            Ok(mut f) => loop {
                match f.read(&mut z_buf[..n_buf as usize]) {
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    _ => break,
                }
            },
        }
    }
    n_buf
}

/// Dorme um pouco. Devolve o tempo dormido. O argumento é o número de
/// microssegundos a dormir; o retorno é o número de microssegundos realmente
/// pedido ao sistema operacional, que pode ser maior ou igual ao argumento, mas
/// nunca menor.
fn unix_sleep(_not_used: Option<&Sqlite3Vfs>, microseconds: i32) -> i32 {
    // HAVE_NANOSLEEP vale 1 por padrão.
    let sec = (microseconds / 1_000_000) as u64;
    let nsec = ((microseconds % 1_000_000) * 1000) as u32;
    std::thread::sleep(std::time::Duration::new(sec, nsec));
    microseconds
}

/// Acha a hora atual (UTC). Escreve em `*pi_now` a data e hora como número do
/// Dia Juliano vezes 86_400_000, ou seja, os milissegundos desde a época juliana
/// (meio-dia em Greenwich de 24 de novembro de 4714 a.C., calendário gregoriano
/// proléptico). Devolve `SQLITE_OK` em sucesso.
fn unix_current_time_int64(_not_used: Option<&Sqlite3Vfs>, pi_now: &mut i64) -> i32 {
    const UNIX_EPOCH: i64 = 24405875 * 8640000i64;
    let rc = SQLITE_OK;
    // gettimeofday não falha com argumentos válidos.
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    *pi_now = UNIX_EPOCH + 1000 * (d.as_secs() as i64) + (d.subsec_micros() as i64) / 1000;
    rc
}


// ---- part_018.rs ----

/// Acha a hora atual (UTC). Escreve a data e hora como número do Dia Juliano
/// em `*pr_now` e devolve 0. Devolve 1 se a hora não puder ser obtida.
/// (`SQLITE_OMIT_DEPRECATED` não está definido no Debian.)
fn unix_current_time(_not_used: Option<&Sqlite3Vfs>, pr_now: &mut f64) -> i32 {
    let mut i: i64 = 0;
    let rc = unix_current_time_int64(None, &mut i);
    *pr_now = i as f64 / 86400000.0;
    rc
}

/// O método xGetLastError() devolve uma mensagem de baixo nível melhor quando
/// surgem problemas do sistema operacional. Hoje só o código inteiro é usado.
fn unix_get_last_error(_not_used: Option<&Sqlite3Vfs>, _not_used2: i32, _not_used3: &mut [u8]) -> i32 {
    get_errno()
}

// O restante do trecho C (proxyGetLockPath, proxyCreateLockPath e
// proxyCreateUnixFile, dentro de `#if defined(__APPLE__) &&
// SQLITE_ENABLE_LOCKING_STYLE`) só existe no macOS e some no Debian.


// ---- part_019.rs ----

// Todo o trecho C correspondente (proxyGetHostID, proxyBreakConchLock,
// proxyConchLock e proxyTakeConch) está dentro de `#if defined(__APPLE__) &&
// SQLITE_ENABLE_LOCKING_STYLE` (proxy locking, só no macOS) e some no Debian.


// ---- part_020.rs ----

// Trecho os_unix_c.020.c: implementação do bloqueio proxy (proxyReleaseConch,
// proxyCreateConchPathname, switchLockProxyPath, proxyGetDbPathForUnixFile,
// proxyTransformUnixFile, proxyFileControl, proxyCheckReservedLock, proxyLock).
//
// Todo o trecho C está dentro de `#if defined(__APPLE__) && SQLITE_ENABLE_LOCKING_STYLE`.
// O sqlite3 do Debian 13 (Linux) não define nenhuma das duas condições, então o
// bloco inteiro some do binário e, pela convenção de plataforma, também some aqui.


// ---- part_021.rs ----

// Trecho os_unix_c.021.c: proxyUnlock e proxyClose ficam dentro de
// `#if defined(__APPLE__) && SQLITE_ENABLE_LOCKING_STYLE` e somem no Debian/Linux.
// Restam sqlite3_os_init e sqlite3_os_end.

/// Inicializa a interface do sistema operacional.
///
/// Esta rotina registra todas as implementações de VFS para sistemas unix-like. Ela e
/// `os_end` devem ser as únicas rotinas deste arquivo visíveis para outros arquivos.
///
/// É chamada uma vez durante a inicialização do SQLite e por uma única thread. Os
/// subsistemas de memória e de mutex não necessariamente foram inicializados.
pub fn os_init() -> i32 {
    // Todos os VFS padrão do unix. SQLITE_ENABLE_LOCKING_STYLE não está definido no
    // Debian/Linux, então "unix-posix" e "unix-flock" não existem; os ramos de VxWorks
    // e de MacOSX (afp, nfs, proxy, autolock) também somem.
    // `new_unix_vfs` é a macro UNIXVFS do C: versão 3, `sz_os_file` do `UnixFile`,
    // `mx_pathname` MAX_PATHNAME e os métodos unix_open ... unix_next_system_call; o
    // finder é o que o C guardava em `pAppData`.
    let a_vfs: [VfsRef; 4] = [
        new_unix_vfs("unix", posix_io_finder),
        new_unix_vfs("unix-none", nolock_io_finder),
        new_unix_vfs("unix-dotfile", dotlock_io_finder),
        new_unix_vfs("unix-excl", posix_io_finder),
    ];

    // Registra todos os VFS definidos no array a_vfs; só o primeiro vira o padrão.
    for (i, vfs) in a_vfs.iter().enumerate() {
        vfs_register(vfs.clone(), i == 0);
    }

    unix_big_lock_set(mutex_alloc(SQLITE_MUTEX_STATIC_VFS1));

    // Inicializa o array de diretórios de arquivos temporários.
    unix_temp_file_init();

    SQLITE_OK
}

/// Encerra a interface do sistema operacional.
///
/// Alguns sistemas operacionais precisam liberar objetos alocados dinamicamente aqui,
/// mas não o unix. Esta rotina só zera o mutex global.
pub fn os_end() -> i32 {
    unix_big_lock_set(None);
    SQLITE_OK
}

