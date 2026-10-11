//! Camada de sistema operacional: interface de arquivo e de VFS (os.h, os.c,
//! as partes `sqlite3_io_methods` e `sqlite3_vfs` do sqlite3.h).
//!
//! No C um arquivo é um `sqlite3_file` com uma tabela de métodos (`pMethods`) e
//! um VFS é um `sqlite3_vfs` com uma tabela de funções. Aqui os dois viram
//! traits (`VfsFile` e `Vfs`), com os mesmos códigos `i32` do C.
//!
//! Escolhas de modelagem (todas registradas para o pager e o `os_unix`):
//!
//! * `pMethods == 0` (arquivo fechado ou nunca aberto) é `Option<Box<dyn VfsFile>>`
//!   em quem guarda o arquivo. Os wrappers que o C protege contra `pMethods == 0`
//!   (`os_close`, `os_file_control`, `os_file_control_hint`) recebem o `Option`.
//! * Método de tabela opcional (ponteiro nulo no C) vira método de trait com
//!   implementação padrão que reproduz o ramo do C para o ponteiro nulo.
//! * Os wrappers de `os.c` cujo corpo é só a chamada do método (`sqlite3OsRead`,
//!   `sqlite3OsWrite`, `sqlite3OsLock`, `sqlite3OsFileSize`, ...) não existem:
//!   o chamador chama o método do trait direto (regra de repasse do projeto).
//!   Ficam só os wrappers que acrescentam lógica.
//! * O registro de VFS é global ao processo, protegido por `Mutex`, como a lista
//!   `vfsList` protegida por `SQLITE_MUTEX_STATIC_MAIN` do C.
//! * Região de memória compartilhada (`shm_map`) e mapeamento (`fetch`) não
//!   devolvem ponteiro: ver `ShmRegion` e `VfsFile::fetch`.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::consts::{
    SQLITE_DEFAULT_SECTOR_SIZE, SQLITE_IOERR_SHMLOCK, SQLITE_IOERR_SHMMAP, SQLITE_NOTFOUND,
    SQLITE_OK, SQLITE_OPEN_EXCLUSIVE,
};

/// Argumento do `xFileControl` (o `void *pArg` do C). Cada variante é uma das
/// formas com que o núcleo e o pager de fato passam o argumento. A variante
/// também serve de saída: o VFS sobrescreve o conteúdo (por exemplo
/// `SQLITE_FCNTL_LOCKSTATE` escreve `Int`, `SQLITE_FCNTL_VFSNAME` escreve `Text`).
#[derive(Debug, Clone, PartialEq)]
pub enum FileControlArg {
    /// `pArg == NULL` (COMMIT_PHASETWO, OVERWRITE, CKPT_START, DB_UNCHANGED, ...)
    /// ou ponteiro que o VFS só precisa ignorar (PDB).
    None,
    /// `int*`: LOCKSTATE, LAST_ERRNO, CHUNK_SIZE, PERSIST_WAL, POWERSAFE_OVERWRITE,
    /// HAS_MOVED, LOCK_TIMEOUT, RESERVE_BYTES, EXTERNAL_READER.
    Int(i32),
    /// `sqlite3_int64*`: SIZE_HINT, MMAP_SIZE, SIZE_LIMIT.
    Int64(i64),
    /// `unsigned*`: DATA_VERSION.
    UInt(u32),
    /// `char*` de entrada (SYNC com o nome do super-journal, TRACE) ou `char**`
    /// de saída (VFSNAME, TEMPFILENAME). Bytes, sem o NUL final.
    Text(Vec<u8>),
    /// `char**` de três posições de `SQLITE_FCNTL_PRAGMA`: `[0]` mensagem de erro
    /// (saída), `[1]` nome do pragma, `[2]` valor (nulo quando ausente).
    Pragma {
        result: Option<Vec<u8>>,
        name: Vec<u8>,
        value: Option<Vec<u8>>,
    },
    /// `SQLITE_FCNTL_BUSYHANDLER`: o C passa o par (função, argumento) do busy
    /// handler como dica. Os VFS padrão ignoram; o callback fica no pager.
    BusyHandler,
}

/// Região de memória compartilhada devolvida por `shm_map` (o
/// `void volatile **pp` do C). Não carrega ponteiro: o acesso aos bytes é feito
/// por `VfsFile::shm_read` e `VfsFile::shm_write`, endereçados por este handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShmRegion {
    /// Índice da região (o `iPg` pedido).
    pub index: i32,
    /// Tamanho da região em bytes (o `pgsz` pedido).
    pub size: i32,
}

/// Handle opaco de biblioteca dinâmica devolvido por `Vfs::dl_open`.
pub type DlHandle = u64;
/// Handle opaco de símbolo devolvido por `Vfs::dl_sym` (o ponteiro de função do C).
pub type DlSymbol = u64;
/// Handle opaco de chamada de sistema (o `sqlite3_syscall_ptr` do C).
pub type SyscallPtr = u64;

/// Máscara dos bits `SQLITE_OPEN_*` que podem descer até o VFS (os.c, `sqlite3OsOpen`).
const OPEN_FLAGS_TO_VFS: i32 = 0x1087f7f;

/// Arquivo aberto (`sqlite3_file` mais `sqlite3_io_methods`). Todo método devolve
/// o código do C (`SQLITE_OK`, `SQLITE_IOERR_*`, `SQLITE_BUSY`, ...).
pub trait VfsFile {
    /// `iVersion` da tabela de métodos: 1 (básico), 2 (com memória compartilhada
    /// do WAL) ou 3 (com `fetch`/`unfetch`).
    fn i_version(&self) -> i32 {
        1
    }

    /// `sqlite3JournalIsInMemory(p)` (`p->pMethods == &MemJournalMethods`): só o
    /// journal em memória (`crate::memjournal::MemJournal` ainda sem arquivo
    /// real) devolve `true`. Sem o downcast do C, o pager consulta o trait.
    fn is_in_memory_journal(&self) -> bool {
        false
    }

    /// `xClose`.
    fn close(&mut self) -> i32;

    /// `xRead`. Lê `buf.len()` bytes a partir de `offset`. Numa leitura curta
    /// devolve `SQLITE_IOERR_SHORT_READ` e DEVE zerar o resto de `buf`.
    fn read(&mut self, buf: &mut [u8], offset: i64) -> i32;

    /// `xWrite`.
    fn write(&mut self, buf: &[u8], offset: i64) -> i32;

    /// `xTruncate`.
    fn truncate(&mut self, size: i64) -> i32;

    /// `xSync`. O wrapper `os_sync` já trata `flags == 0`.
    fn sync(&mut self, flags: i32) -> i32;

    /// `xFileSize`.
    fn file_size(&mut self, size: &mut i64) -> i32;

    /// `xLock`. `lock_type` vai de `SQLITE_LOCK_SHARED` a `SQLITE_LOCK_EXCLUSIVE`.
    fn lock(&mut self, lock_type: i32) -> i32;

    /// `xUnlock`. `lock_type` é `SQLITE_LOCK_NONE` ou `SQLITE_LOCK_SHARED`.
    fn unlock(&mut self, lock_type: i32) -> i32;

    /// `xCheckReservedLock`.
    fn check_reserved_lock(&mut self, res_out: &mut i32) -> i32;

    /// `xFileControl`. Devolve `SQLITE_NOTFOUND` para opcode desconhecido.
    fn file_control(&mut self, op: i32, arg: &mut FileControlArg) -> i32;

    /// `xSectorSize`. A implementação padrão é o ramo do ponteiro nulo do C
    /// (`SQLITE_DEFAULT_SECTOR_SIZE`).
    fn sector_size(&mut self) -> i32 {
        SQLITE_DEFAULT_SECTOR_SIZE as i32
    }

    /// `xDeviceCharacteristics`: máscara `SQLITE_IOCAP_*`.
    fn device_characteristics(&mut self) -> i32;

    /// `xShmMap` (versão 2). Garante a região `i_page` de `pgsz` bytes e a
    /// devolve em `pp`. Com `b_extend == 0` e região inexistente, `pp` fica
    /// `None` e o código é `SQLITE_OK`, como o ponteiro nulo do C.
    fn shm_map(&mut self, i_page: i32, pgsz: i32, b_extend: i32, pp: &mut Option<ShmRegion>) -> i32 {
        let _ = (i_page, pgsz, b_extend);
        *pp = None;
        SQLITE_IOERR_SHMMAP
    }

    /// Lê bytes de uma região devolvida por `shm_map`. No C isto é desreferenciar
    /// o ponteiro mapeado; sem ponteiros, é método do arquivo.
    fn shm_read(&mut self, region: &ShmRegion, offset: usize, buf: &mut [u8]) -> i32 {
        let _ = (region, offset, buf);
        SQLITE_IOERR_SHMMAP
    }

    /// Escreve bytes numa região devolvida por `shm_map`.
    fn shm_write(&mut self, region: &ShmRegion, offset: usize, data: &[u8]) -> i32 {
        let _ = (region, offset, data);
        SQLITE_IOERR_SHMMAP
    }

    /// `xShmLock` (versão 2).
    fn shm_lock(&mut self, offset: i32, n: i32, flags: i32) -> i32 {
        let _ = (offset, n, flags);
        SQLITE_IOERR_SHMLOCK
    }

    /// `xShmBarrier` (versão 2).
    fn shm_barrier(&mut self) {}

    /// `xShmUnmap` (versão 2).
    fn shm_unmap(&mut self, delete_flag: i32) -> i32 {
        let _ = delete_flag;
        SQLITE_OK
    }

    /// `xFetch` (versão 3, memória mapeada). A implementação padrão é o stub do
    /// C para mapeamento desligado: `*pp = 0` e `SQLITE_OK`. Quando o VFS
    /// devolve `Some`, o conteúdo é uma cópia somente leitura da faixa.
    fn fetch(&mut self, ofst: i64, amt: i32, pp: &mut Option<Vec<u8>>) -> i32 {
        let _ = (ofst, amt);
        *pp = None;
        SQLITE_OK
    }

    /// `xUnfetch` (versão 3).
    fn unfetch(&mut self, ofst: i64, p: Option<Vec<u8>>) -> i32 {
        let _ = (ofst, p);
        SQLITE_OK
    }
}

/// Sistema de arquivos virtual (`sqlite3_vfs`). Compartilhado entre conexões e
/// threads, por isso `Send + Sync`; estado interno do VFS usa `Mutex`/atômicos.
pub trait Vfs: Send + Sync {
    /// `iVersion` da estrutura (1, 2 ou 3).
    fn i_version(&self) -> i32 {
        3
    }

    /// `zName`.
    fn name(&self) -> &[u8];

    /// `mxPathname`.
    fn max_pathname(&self) -> i32;

    /// `xOpen`. `name` é o `sqlite3_filename` do C: o caminho seguido de NUL e
    /// dos pares chave/valor de URI (cada um terminado em NUL, a lista
    /// terminada por NUL duplo). `None` abre arquivo temporário. Em erro
    /// devolve o código; em sucesso, o arquivo, e `out_flags` recebe as flags
    /// efetivas.
    fn open(&self, name: Option<&[u8]>, flags: i32, out_flags: &mut i32) -> Result<Box<dyn VfsFile>, i32>;

    /// `xDelete`. A implementação padrão é o ramo do ponteiro nulo do C
    /// (`SQLITE_OK`).
    fn delete(&self, name: &[u8], sync_dir: i32) -> i32 {
        let _ = (name, sync_dir);
        SQLITE_OK
    }

    /// `xAccess`: `flags` é `SQLITE_ACCESS_*`; o resultado vai em `res_out`.
    fn access(&self, name: &[u8], flags: i32, res_out: &mut i32) -> i32;

    /// `xFullPathname`. `out` chega vazio (o wrapper zera) e recebe o caminho
    /// absoluto sem o NUL; `n_out` é o tamanho do buffer do C, NUL incluído.
    fn full_pathname(&self, name: &[u8], n_out: i32, out: &mut Vec<u8>) -> i32;

    /// `xDlOpen`: `None` quando a biblioteca não carrega.
    fn dl_open(&self, path: &[u8]) -> Option<DlHandle> {
        let _ = path;
        None
    }

    /// `xDlError`: escreve em `out` a mensagem do último erro de carga, com no
    /// máximo `n_byte` bytes.
    fn dl_error(&self, n_byte: i32, out: &mut Vec<u8>) {
        let _ = (n_byte, out);
    }

    /// `xDlSym`.
    fn dl_sym(&self, handle: DlHandle, symbol: &[u8]) -> Option<DlSymbol> {
        let _ = (handle, symbol);
        None
    }

    /// `xDlClose`.
    fn dl_close(&self, handle: DlHandle) {
        let _ = handle;
    }

    /// `xRandomness`: enche `out` com bytes aleatórios.
    fn randomness(&self, out: &mut [u8]) -> i32;

    /// `xSleep`: dorme `micro` microssegundos e devolve o tempo dormido.
    fn sleep(&self, micro: i32) -> i32;

    /// `xCurrentTime`: dia juliano como `f64`.
    fn current_time(&self, out: &mut f64) -> i32;

    /// `xGetLastError`. A implementação padrão é o ramo do ponteiro nulo do C (0).
    fn get_last_error(&self, n_buf: i32, buf: &mut Vec<u8>) -> i32 {
        let _ = (n_buf, buf);
        0
    }

    /// `xCurrentTimeInt64` (versão 2). `None` equivale ao ponteiro nulo (VFS de
    /// versão 1): o wrapper cai para `current_time`.
    fn current_time_int64(&self, out: &mut i64) -> Option<i32> {
        let _ = out;
        None
    }

    /// `xSetSystemCall` (versão 3).
    fn set_system_call(&self, name: Option<&[u8]>, ptr: Option<SyscallPtr>) -> i32 {
        let _ = (name, ptr);
        SQLITE_NOTFOUND
    }

    /// `xGetSystemCall` (versão 3).
    fn get_system_call(&self, name: &[u8]) -> Option<SyscallPtr> {
        let _ = name;
        None
    }

    /// `xNextSystemCall` (versão 3).
    fn next_system_call(&self, name: Option<&[u8]>) -> Option<&'static [u8]> {
        let _ = name;
        None
    }
}

/// Referência compartilhada a um VFS registrado (o `sqlite3_vfs *` do C).
pub type VfsRef = Arc<dyn Vfs>;

// ---------------------------------------------------------------------------
// Wrappers de os.c que acrescentam lógica sobre o método do trait.
// ---------------------------------------------------------------------------

/// `sqlite3OsClose`: fecha o arquivo e zera `pMethods` (vira `None`).
pub fn os_close(file: &mut Option<Box<dyn VfsFile>>) {
    if let Some(mut f) = file.take() {
        f.close();
    }
}

/// `sqlite3OsSync`: sem flags não há nada a sincronizar.
pub fn os_sync(f: &mut dyn VfsFile, flags: i32) -> i32 {
    if flags != 0 {
        f.sync(flags)
    } else {
        SQLITE_OK
    }
}

/// `sqlite3OsFileControl`: arquivo ausente (`pMethods == 0`) é `SQLITE_NOTFOUND`.
pub fn os_file_control<F: VfsFile + ?Sized>(f: Option<&mut F>, op: i32, arg: &mut FileControlArg) -> i32 {
    match f {
        None => SQLITE_NOTFOUND,
        Some(f) => f.file_control(op, arg),
    }
}

/// `sqlite3OsFileControlHint`: dica ao VFS; o resultado não importa.
pub fn os_file_control_hint<F: VfsFile + ?Sized>(f: Option<&mut F>, op: i32, arg: &mut FileControlArg) {
    if let Some(f) = f {
        let _ = f.file_control(op, arg);
    }
}

/// `sqlite3OsOpen`: só as flags de `OPEN_FLAGS_TO_VFS` chegam ao VFS (por exemplo
/// `SQLITE_OPEN_FULLMUTEX` e `SQLITE_OPEN_SHAREDCACHE` são barradas aqui).
pub fn os_open(
    vfs: &dyn Vfs,
    path: Option<&[u8]>,
    flags: i32,
    out_flags: &mut i32,
) -> Result<Box<dyn VfsFile>, i32> {
    debug_assert!(path.is_some() || (flags & SQLITE_OPEN_EXCLUSIVE) != 0);
    vfs.open(path, flags & OPEN_FLAGS_TO_VFS, out_flags)
}

/// `sqlite3OsFullPathname`: zera a saída antes de chamar o VFS (`zPathOut[0] = 0`).
pub fn os_full_pathname(vfs: &dyn Vfs, path: &[u8], n_out: i32, out: &mut Vec<u8>) -> i32 {
    out.clear();
    vfs.full_pathname(path, n_out, out)
}

/// `sqlite3OsRandomness`. `prng_seed` é `sqlite3Config.iPrngSeed` (o módulo de
/// configuração global ainda não existe, então vem por parâmetro): com semente
/// fixa os bytes saem da semente e o VFS não é consultado.
pub fn os_randomness(vfs: &dyn Vfs, out: &mut [u8], prng_seed: u32) -> i32 {
    if prng_seed != 0 {
        out.fill(0);
        let seed = prng_seed.to_ne_bytes();
        let n = out.len().min(seed.len());
        out[..n].copy_from_slice(&seed[..n]);
        SQLITE_OK
    } else {
        vfs.randomness(out)
    }
}

/// `sqlite3OsCurrentTimeInt64`: usa `xCurrentTimeInt64` quando a versão do VFS
/// é 2 ou mais e o método existe; senão cai para `xCurrentTime`.
pub fn os_current_time_int64(vfs: &dyn Vfs, out: &mut i64) -> i32 {
    if vfs.i_version() >= 2 {
        if let Some(rc) = vfs.current_time_int64(out) {
            return rc;
        }
    }
    let mut r: f64 = 0.0;
    let rc = vfs.current_time(&mut r);
    *out = (r * 86400000.0) as i64;
    rc
}

// ---------------------------------------------------------------------------
// Registro de VFS (lista `vfsList` de os.c).
// ---------------------------------------------------------------------------

/// Lista de VFS registrados; o primeiro é o padrão. Global ao processo.
static VFS_LIST: Mutex<Vec<VfsRef>> = Mutex::new(Vec::new());

fn lock_vfs_list() -> MutexGuard<'static, Vec<VfsRef>> {
    VFS_LIST.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Identidade de VFS: o endereço do objeto (o ponteiro do C).
fn same_vfs(a: &VfsRef, b: &VfsRef) -> bool {
    std::ptr::addr_eq(Arc::as_ptr(a), Arc::as_ptr(b))
}

/// `sqlite3_vfs_find`: sem nome devolve o primeiro da lista (o padrão).
/// O `sqlite3_initialize()` do C não é chamado aqui: quem inicializa o
/// processo registra os VFS padrão antes.
pub fn vfs_find(name: Option<&[u8]>) -> Option<VfsRef> {
    let list = lock_vfs_list();
    match name {
        None => list.first().cloned(),
        Some(n) => list.iter().find(|v| v.name() == n).cloned(),
    }
}

/// `vfsUnlink`: retira o VFS da lista, se estiver nela.
fn vfs_unlink(list: &mut Vec<VfsRef>, vfs: &VfsRef) {
    if let Some(i) = list.iter().position(|v| same_vfs(v, vfs)) {
        list.remove(i);
    }
}

/// `sqlite3_vfs_register`: registrar de novo o mesmo VFS é inofensivo. Com
/// `make_dflt` (ou lista vazia) ele vira o primeiro; senão entra logo depois
/// do primeiro.
pub fn vfs_register(vfs: VfsRef, make_dflt: bool) -> i32 {
    let mut list = lock_vfs_list();
    vfs_unlink(&mut list, &vfs);
    if make_dflt || list.is_empty() {
        list.insert(0, vfs);
    } else {
        list.insert(1, vfs);
    }
    SQLITE_OK
}

/// `sqlite3_vfs_unregister`.
pub fn vfs_unregister(vfs: &VfsRef) -> i32 {
    let mut list = lock_vfs_list();
    vfs_unlink(&mut list, vfs);
    SQLITE_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NullVfs(&'static [u8]);

    impl Vfs for NullVfs {
        fn name(&self) -> &[u8] {
            self.0
        }
        fn max_pathname(&self) -> i32 {
            512
        }
        fn open(&self, _: Option<&[u8]>, _: i32, _: &mut i32) -> Result<Box<dyn VfsFile>, i32> {
            Err(crate::consts::SQLITE_CANTOPEN)
        }
        fn access(&self, _: &[u8], _: i32, res_out: &mut i32) -> i32 {
            *res_out = 0;
            SQLITE_OK
        }
        fn full_pathname(&self, name: &[u8], _: i32, out: &mut Vec<u8>) -> i32 {
            out.extend_from_slice(name);
            SQLITE_OK
        }
        fn randomness(&self, out: &mut [u8]) -> i32 {
            out.fill(0xAB);
            SQLITE_OK
        }
        fn sleep(&self, micro: i32) -> i32 {
            micro
        }
        fn current_time(&self, out: &mut f64) -> i32 {
            *out = 2.0;
            SQLITE_OK
        }
    }

    #[test]
    fn register_find_unregister() {
        let v: VfsRef = Arc::new(NullVfs(b"os-test-a"));
        assert!(vfs_find(Some(b"os-test-a")).is_none());
        assert_eq!(vfs_register(v.clone(), false), SQLITE_OK);
        assert!(vfs_find(Some(b"os-test-a")).is_some());
        assert_eq!(vfs_register(v.clone(), false), SQLITE_OK);
        assert_eq!(vfs_unregister(&v), SQLITE_OK);
        assert!(vfs_find(Some(b"os-test-a")).is_none());
    }

    #[test]
    fn randomness_with_seed_and_time_fallback() {
        let v = NullVfs(b"os-test-b");
        let mut buf = [0xFFu8; 8];
        assert_eq!(os_randomness(&v, &mut buf, 0x01020304), SQLITE_OK);
        assert_eq!(&buf[..4], &0x01020304u32.to_ne_bytes());
        assert_eq!(&buf[4..], &[0, 0, 0, 0]);
        let mut buf = [0u8; 4];
        os_randomness(&v, &mut buf, 0);
        assert_eq!(buf, [0xAB; 4]);
        let mut t = 0i64;
        assert_eq!(os_current_time_int64(&v, &mut t), SQLITE_OK);
        assert_eq!(t, 172800000);
    }
}
