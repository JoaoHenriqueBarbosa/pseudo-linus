//! VFS em memória e serialização de bancos (memdb.c).
//!
//! Um banco memdb é um bloco contíguo de memória. Este módulo implementa o VFS
//! `memdb` (`MemVfs`), o arquivo aberto sobre ele (`MemFile`) e as operações de
//! `sqlite3_serialize` e `sqlite3_deserialize` que não dependem da conexão.
//!
//! Modelagem:
//!
//! * O `MemStore` do C vira `MemStore` dentro de `Arc<Mutex<MemStore>>`
//!   (`MemStoreRef`). O `pMutex` do C (só existe nos stores compartilhados) é o
//!   `Mutex` do `Arc`; stores separados também o têm, sem custo de contenção.
//! * A tabela `memdb_g.apMemStore` dos stores compartilhados (nome começando por
//!   `/`) é um `static Mutex<Vec<(nome, MemStoreRef)>>`, protegido como o
//!   `SQLITE_MUTEX_STATIC_VFS1` do C. A ordem dos elementos reproduz o C
//!   (inserção no fim, remoção por troca com o último).
//! * `aData`/`szAlloc` do C são `data: Vec<u8>` com `data.len() >= sz_alloc`.
//! * O `pMethods == &memdb_io_methods` que `memdbFromDbSchema` testa não tem
//!   equivalente sem `unsafe`: ver `memdb_from_file` e a seção ADIADAS na
//!   resposta do porte.
//! * `SQLITE_DESERIALIZE_FREEONCLOSE` ausente (o chamador mantém a posse do
//!   buffer no C) não existe em Rust seguro: `memdb_deserialize_store` sempre
//!   recebe a posse do `Vec`.
//! * `sqlite3GlobalConfig.mxMemdbSize` ainda não tem módulo próprio
//!   (`crate::global`): vive em `MX_MEMDB_SIZE` aqui, e vai para lá quando existir.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use crate::consts::{
    SQLITE_BUSY, SQLITE_CORRUPT, SQLITE_DESERIALIZE_FREEONCLOSE, SQLITE_DESERIALIZE_READONLY,
    SQLITE_DESERIALIZE_RESIZEABLE, SQLITE_ERROR, SQLITE_FCNTL_SIZE_LIMIT, SQLITE_FCNTL_VFSNAME,
    SQLITE_FULL, SQLITE_IOCAP_ATOMIC, SQLITE_IOCAP_POWERSAFE_OVERWRITE, SQLITE_IOCAP_SAFE_APPEND,
    SQLITE_IOCAP_SEQUENTIAL, SQLITE_IOERR_NOMEM, SQLITE_IOERR_SHORT_READ, SQLITE_IOERR_WRITE,
    SQLITE_LOCK_EXCLUSIVE, SQLITE_LOCK_NONE, SQLITE_LOCK_PENDING, SQLITE_LOCK_RESERVED,
    SQLITE_LOCK_SHARED, SQLITE_MEMDB_DEFAULT_MAXSIZE, SQLITE_NOTFOUND, SQLITE_OK, SQLITE_OPEN_MEMORY,
    SQLITE_READONLY, SQLITE_SERIALIZE_NOCOPY,
};
use crate::os::{
    os_current_time_int64, vfs_find, vfs_register, DlHandle, DlSymbol, FileControlArg, Vfs, VfsFile,
    VfsRef,
};

/// `sqlite3GlobalConfig.mxMemdbSize`: tamanho máximo padrão de um banco memdb.
pub static MX_MEMDB_SIZE: AtomicI64 = AtomicI64::new(SQLITE_MEMDB_DEFAULT_MAXSIZE);

/// Armazenamento de um arquivo memdb (a estrutura `MemStore`).
///
/// Pode ser compartilhado (nome começando por `/`, `f_name` preenchido) ou
/// separado (conectado a uma só conexão).
#[derive(Debug, Default)]
pub struct MemStore {
    /// Tamanho do arquivo.
    pub sz: i64,
    /// Espaço alocado para `data`.
    pub sz_alloc: i64,
    /// Tamanho máximo permitido do arquivo.
    pub sz_max: i64,
    /// Conteúdo do arquivo (`aData`); `data.len() >= sz_alloc`.
    pub data: Vec<u8>,
    /// Número de páginas mapeadas em memória (`xFetch` sem `xUnfetch`).
    pub n_mmap: i32,
    /// Flags `SQLITE_DESERIALIZE_*`.
    pub flags: u32,
    /// Número de leitores.
    pub n_rd_lock: i32,
    /// Número de escritores (sempre 0 ou 1).
    pub n_wr_lock: i32,
    /// Número de usuários deste store.
    pub n_ref: i32,
    /// Nome do arquivo, só nos stores compartilhados (`zFName`).
    pub f_name: Option<Vec<u8>>,
}

/// Referência compartilhada a um `MemStore`.
pub type MemStoreRef = Arc<Mutex<MemStore>>;

/// Entra no mutex de um store (`memdbEnter`); o envenenamento é ignorado como o
/// C ignora um mutex "abandonado".
fn memdb_enter(p: &MemStoreRef) -> MutexGuard<'_, MemStore> {
    p.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `memdb_g`: os stores compartilhados, protegidos pelo mutex do VFS.
static MEMDB_G: Mutex<Vec<(Vec<u8>, MemStoreRef)>> = Mutex::new(Vec::new());

fn lock_memdb_g() -> MutexGuard<'static, Vec<(Vec<u8>, MemStoreRef)>> {
    MEMDB_G.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Um arquivo aberto (a estrutura `MemFile`).
pub struct MemFile {
    /// O armazenamento.
    store: MemStoreRef,
    /// Trava mais recente contra este arquivo.
    e_lock: i32,
}

impl MemFile {
    /// O `pStore` do arquivo.
    pub fn store(&self) -> MemStoreRef {
        self.store.clone()
    }
}

/// `memdbEnlarge`: tenta aumentar a alocação para conter ao menos `new_sz` bytes.
fn memdb_enlarge(p: &mut MemStore, mut new_sz: i64) -> i32 {
    if (p.flags & SQLITE_DESERIALIZE_RESIZEABLE) == 0 || p.n_mmap > 0 {
        return SQLITE_FULL;
    }
    if new_sz > p.sz_max {
        return SQLITE_FULL;
    }
    new_sz = new_sz.saturating_mul(2);
    if new_sz > p.sz_max {
        new_sz = p.sz_max;
    }
    let new_len = new_sz as usize;
    if new_len > p.data.len() {
        if p.data.try_reserve_exact(new_len - p.data.len()).is_err() {
            return SQLITE_IOERR_NOMEM;
        }
    }
    p.data.resize(new_len, 0);
    p.sz_alloc = new_sz;
    SQLITE_OK
}

impl VfsFile for MemFile {
    fn i_version(&self) -> i32 {
        3
    }

    /// `memdbClose`: libera o store quando a contagem de referências chega a zero.
    fn close(&mut self) -> i32 {
        let p = &self.store;
        let shared = memdb_enter(p).f_name.is_some();
        let mut g;
        if shared {
            let mut reg = lock_memdb_g();
            g = memdb_enter(p);
            if g.n_ref == 1 {
                if let Some(i) = reg.iter().position(|(_, s)| Arc::ptr_eq(s, p)) {
                    // apMemStore[i] = apMemStore[--nMemStore]
                    reg.swap_remove(i);
                }
            }
            drop(reg);
        } else {
            g = memdb_enter(p);
        }
        g.n_ref -= 1;
        if g.n_ref <= 0 && (g.flags & SQLITE_DESERIALIZE_FREEONCLOSE) != 0 {
            g.data = Vec::new();
            g.sz_alloc = 0;
        }
        SQLITE_OK
    }

    /// `memdbRead`.
    fn read(&mut self, buf: &mut [u8], offset: i64) -> i32 {
        let p = memdb_enter(&self.store);
        let i_amt = buf.len() as i64;
        if offset + i_amt > p.sz {
            buf.fill(0);
            if offset < p.sz {
                let n = (p.sz - offset) as usize;
                buf[..n].copy_from_slice(&p.data[offset as usize..offset as usize + n]);
            }
            return SQLITE_IOERR_SHORT_READ;
        }
        let o = offset as usize;
        buf.copy_from_slice(&p.data[o..o + buf.len()]);
        SQLITE_OK
    }

    /// `memdbWrite`.
    fn write(&mut self, z: &[u8], offset: i64) -> i32 {
        let mut p = memdb_enter(&self.store);
        let i_amt = z.len() as i64;
        if (p.flags & SQLITE_DESERIALIZE_READONLY) != 0 {
            // Não acontece: `lock` devolve SQLITE_READONLY antes de chegar aqui.
            return SQLITE_IOERR_WRITE;
        }
        if offset + i_amt > p.sz {
            if offset + i_amt > p.sz_alloc {
                let rc = memdb_enlarge(&mut p, offset + i_amt);
                if rc != SQLITE_OK {
                    return rc;
                }
            }
            if offset > p.sz {
                let (from, to) = (p.sz as usize, offset as usize);
                p.data[from..to].fill(0);
            }
            p.sz = offset + i_amt;
        }
        let o = offset as usize;
        p.data[o..o + z.len()].copy_from_slice(z);
        SQLITE_OK
    }

    /// `memdbTruncate`: em modo rollback (o único do memdb) só reduz o arquivo.
    fn truncate(&mut self, size: i64) -> i32 {
        let mut p = memdb_enter(&self.store);
        if size > p.sz {
            // Só acontece com um banco corrompido em modo wal.
            SQLITE_CORRUPT
        } else {
            p.sz = size;
            SQLITE_OK
        }
    }

    /// `memdbSync`.
    fn sync(&mut self, _flags: i32) -> i32 {
        SQLITE_OK
    }

    /// `memdbFileSize`.
    fn file_size(&mut self, size: &mut i64) -> i32 {
        *size = memdb_enter(&self.store).sz;
        SQLITE_OK
    }

    /// `memdbLock`.
    fn lock(&mut self, e_lock: i32) -> i32 {
        let mut rc = SQLITE_OK;
        if e_lock <= self.e_lock {
            return SQLITE_OK;
        }
        let mut p = memdb_enter(&self.store);

        debug_assert!(p.n_wr_lock == 0 || p.n_wr_lock == 1);
        debug_assert!(self.e_lock <= SQLITE_LOCK_SHARED || p.n_wr_lock == 1);
        debug_assert!(self.e_lock == SQLITE_LOCK_NONE || p.n_rd_lock >= 1);

        if e_lock > SQLITE_LOCK_SHARED && (p.flags & SQLITE_DESERIALIZE_READONLY) != 0 {
            rc = SQLITE_READONLY;
        } else {
            match e_lock {
                SQLITE_LOCK_SHARED => {
                    debug_assert!(self.e_lock == SQLITE_LOCK_NONE);
                    if p.n_wr_lock > 0 {
                        rc = SQLITE_BUSY;
                    } else {
                        p.n_rd_lock += 1;
                    }
                }
                SQLITE_LOCK_RESERVED | SQLITE_LOCK_PENDING => {
                    debug_assert!(self.e_lock >= SQLITE_LOCK_SHARED);
                    if self.e_lock == SQLITE_LOCK_SHARED {
                        if p.n_wr_lock > 0 {
                            rc = SQLITE_BUSY;
                        } else {
                            p.n_wr_lock = 1;
                        }
                    }
                }
                _ => {
                    debug_assert!(e_lock == SQLITE_LOCK_EXCLUSIVE);
                    debug_assert!(self.e_lock >= SQLITE_LOCK_SHARED);
                    if p.n_rd_lock > 1 {
                        rc = SQLITE_BUSY;
                    } else if self.e_lock == SQLITE_LOCK_SHARED {
                        p.n_wr_lock = 1;
                    }
                }
            }
        }
        if rc == SQLITE_OK {
            self.e_lock = e_lock;
        }
        rc
    }

    /// `memdbUnlock`.
    fn unlock(&mut self, e_lock: i32) -> i32 {
        if e_lock >= self.e_lock {
            return SQLITE_OK;
        }
        let mut p = memdb_enter(&self.store);

        debug_assert!(e_lock == SQLITE_LOCK_SHARED || e_lock == SQLITE_LOCK_NONE);
        if e_lock == SQLITE_LOCK_SHARED {
            if self.e_lock > SQLITE_LOCK_SHARED {
                p.n_wr_lock -= 1;
            }
        } else {
            if self.e_lock > SQLITE_LOCK_SHARED {
                p.n_wr_lock -= 1;
            }
            p.n_rd_lock -= 1;
        }

        self.e_lock = e_lock;
        SQLITE_OK
    }

    /// `memdbCheckReservedLock` (no C está em `#if 0`, a tabela guarda ponteiro
    /// nulo): só serve à recuperação de queda, que não existe em memória.
    fn check_reserved_lock(&mut self, res_out: &mut i32) -> i32 {
        *res_out = 0;
        SQLITE_OK
    }

    /// `memdbFileControl`.
    fn file_control(&mut self, op: i32, arg: &mut FileControlArg) -> i32 {
        let mut p = memdb_enter(&self.store);
        let mut rc = SQLITE_NOTFOUND;
        if op == SQLITE_FCNTL_VFSNAME {
            // "memdb(%p,%lld)": %p do glibc imprime "(nil)" para o ponteiro nulo.
            let ptr = if p.data.is_empty() {
                "(nil)".to_string()
            } else {
                format!("0x{:x}", p.data.as_ptr() as usize)
            };
            *arg = FileControlArg::Text(format!("memdb({},{})", ptr, p.sz).into_bytes());
            rc = SQLITE_OK;
        }
        if op == SQLITE_FCNTL_SIZE_LIMIT {
            if let FileControlArg::Int64(mut i_limit) = *arg {
                if i_limit < p.sz {
                    if i_limit < 0 {
                        i_limit = p.sz_max;
                    } else {
                        i_limit = p.sz;
                    }
                }
                p.sz_max = i_limit;
                *arg = FileControlArg::Int64(i_limit);
                rc = SQLITE_OK;
            }
        }
        rc
    }

    /// `memdbDeviceCharacteristics`.
    fn device_characteristics(&mut self) -> i32 {
        SQLITE_IOCAP_ATOMIC
            | SQLITE_IOCAP_POWERSAFE_OVERWRITE
            | SQLITE_IOCAP_SAFE_APPEND
            | SQLITE_IOCAP_SEQUENTIAL
    }

    /// `memdbFetch`: no C devolve um ponteiro para o `aData`; aqui uma cópia da
    /// faixa (ver `VfsFile::fetch`). A contagem `n_mmap` segue o C.
    fn fetch(&mut self, ofst: i64, amt: i32, pp: &mut Option<Vec<u8>>) -> i32 {
        let mut p = memdb_enter(&self.store);
        if ofst + amt as i64 > p.sz || (p.flags & SQLITE_DESERIALIZE_RESIZEABLE) != 0 {
            *pp = None;
        } else {
            p.n_mmap += 1;
            let o = ofst as usize;
            *pp = Some(p.data[o..o + amt as usize].to_vec());
        }
        SQLITE_OK
    }

    /// `memdbUnfetch`.
    fn unfetch(&mut self, _ofst: i64, _p: Option<Vec<u8>>) -> i32 {
        memdb_enter(&self.store).n_mmap -= 1;
        SQLITE_OK
    }
}

/// `memdbFromDbSchema` depois que a conexão achou o `MemFile`: devolve o store
/// só se ele for separado (`zFName == 0`); um store compartilhado dá `None`.
pub fn memdb_from_file(file: &MemFile) -> Option<MemStoreRef> {
    let shared = memdb_enter(&file.store).f_name.is_some();
    if shared {
        None
    } else {
        Some(file.store.clone())
    }
}

/// `sqlite3_serialize` quando o banco é um memdb separado (o ramo `if( p )`):
/// devolve o conteúdo e escreve o tamanho em `size_out`. `SQLITE_SERIALIZE_NOCOPY`
/// devolveria o ponteiro `aData`; sem ponteiros o resultado é uma cópia também.
/// Como `sqlite3_malloc64(0)` do C devolve nulo, tamanho zero sem NOCOPY é `None`.
pub fn memdb_serialize_store(store: &MemStoreRef, m_flags: u32, size_out: &mut i64) -> Option<Vec<u8>> {
    let p = memdb_enter(store);
    *size_out = p.sz;
    if (m_flags & SQLITE_SERIALIZE_NOCOPY) != 0 {
        if p.data.is_empty() {
            None
        } else {
            Some(p.data[..p.sz as usize].to_vec())
        }
    } else if p.sz == 0 {
        None
    } else {
        Some(p.data[..p.sz as usize].to_vec())
    }
}

/// O trecho de `sqlite3_deserialize` que preenche o store depois do `ATTACH`
/// (o ramo `p != 0`): instala o buffer, os tamanhos e as flags. `data` entra com
/// a posse (ver a nota do módulo sobre `FREEONCLOSE`); `mx_memdb_size` é
/// `sqlite3GlobalConfig.mxMemdbSize`.
pub fn memdb_deserialize_store(
    store: &MemStoreRef,
    mut data: Vec<u8>,
    sz_db: i64,
    sz_buf: i64,
    m_flags: u32,
    mx_memdb_size: i64,
) {
    let mut p = memdb_enter(store);
    // O C confia em `szBuf`; garante que os índices de `data` nunca saiam do vetor.
    let need = sz_buf.max(sz_db).max(0) as usize;
    if data.len() < need {
        data.resize(need, 0);
    }
    p.data = data;
    p.sz = sz_db;
    p.sz_alloc = sz_buf;
    p.sz_max = sz_buf;
    if p.sz_max < mx_memdb_size {
        p.sz_max = mx_memdb_size;
    }
    p.flags = m_flags;
}

/// O objeto `memdb_vfs`. `orig` é o `pAppData` do C: o VFS padrão no momento do
/// registro, ao qual se delega carga dinâmica, aleatoriedade, sono e tempo.
pub struct MemVfs {
    orig: Mutex<VfsRef>,
}

impl MemVfs {
    fn orig(&self) -> VfsRef {
        self.orig.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// A instância única `memdb_vfs`.
static MEMDB_VFS: Mutex<Option<Arc<MemVfs>>> = Mutex::new(None);

/// Corta o nome no primeiro NUL (`strlen`): o `sqlite3_filename` carrega os
/// parâmetros de URI depois do NUL.
fn c_str(name: &[u8]) -> &[u8] {
    match name.iter().position(|&b| b == 0) {
        Some(i) => &name[..i],
        None => name,
    }
}

impl Vfs for MemVfs {
    fn i_version(&self) -> i32 {
        2
    }

    fn name(&self) -> &[u8] {
        b"memdb"
    }

    fn max_pathname(&self) -> i32 {
        1024
    }

    /// `memdbOpen`: nome com mais de um caractere começando por `/` (ou `\`) é um
    /// store compartilhado, achado ou criado em `memdb_g`; qualquer outro nome
    /// (inclusive `None`) é um store separado. Em ambos `szMax` é
    /// `mxMemdbSize` e as flags são `RESIZEABLE | FREEONCLOSE`.
    fn open(&self, name: Option<&[u8]>, flags: i32, out_flags: &mut i32) -> Result<Box<dyn VfsFile>, i32> {
        let z_name: &[u8] = match name {
            Some(n) => c_str(n),
            None => &[],
        };
        let mx = MX_MEMDB_SIZE.load(Ordering::Relaxed);
        let store: MemStoreRef;
        if z_name.len() > 1 && (z_name[0] == b'/' || z_name[0] == b'\\') {
            let mut reg = lock_memdb_g();
            let found = reg.iter().find(|(n, _)| n.as_slice() == z_name).map(|(_, s)| s.clone());
            match found {
                Some(s) => {
                    memdb_enter(&s).n_ref += 1;
                    store = s;
                }
                None => {
                    let s: MemStoreRef = Arc::new(Mutex::new(MemStore {
                        flags: SQLITE_DESERIALIZE_RESIZEABLE | SQLITE_DESERIALIZE_FREEONCLOSE,
                        sz_max: mx,
                        f_name: Some(z_name.to_vec()),
                        n_ref: 1,
                        ..MemStore::default()
                    }));
                    reg.push((z_name.to_vec(), s.clone()));
                    store = s;
                }
            }
        } else {
            store = Arc::new(Mutex::new(MemStore {
                flags: SQLITE_DESERIALIZE_RESIZEABLE | SQLITE_DESERIALIZE_FREEONCLOSE,
                sz_max: mx,
                ..MemStore::default()
            }));
        }
        *out_flags = flags | SQLITE_OPEN_MEMORY;
        Ok(Box::new(MemFile { store, e_lock: SQLITE_LOCK_NONE }))
    }

    /// `memdbDelete` está em `#if 0` e a tabela guarda ponteiro nulo: o padrão do
    /// trait (`SQLITE_OK`) é o ramo do ponteiro nulo.
    /// `memdbAccess`: nenhum arquivo existe em disco, então sempre falso.
    fn access(&self, _name: &[u8], _flags: i32, res_out: &mut i32) -> i32 {
        *res_out = 0;
        SQLITE_OK
    }

    /// `memdbFullPathname`: `sqlite3_snprintf(nOut, zOut, "%s", zPath)`.
    fn full_pathname(&self, name: &[u8], n_out: i32, out: &mut Vec<u8>) -> i32 {
        if n_out > 0 {
            let z = c_str(name);
            let n = z.len().min(n_out as usize - 1);
            out.extend_from_slice(&z[..n]);
        }
        SQLITE_OK
    }

    fn dl_open(&self, path: &[u8]) -> Option<DlHandle> {
        self.orig().dl_open(path)
    }

    fn dl_error(&self, n_byte: i32, out: &mut Vec<u8>) {
        self.orig().dl_error(n_byte, out);
    }

    fn dl_sym(&self, handle: DlHandle, symbol: &[u8]) -> Option<DlSymbol> {
        self.orig().dl_sym(handle, symbol)
    }

    fn dl_close(&self, handle: DlHandle) {
        self.orig().dl_close(handle);
    }

    fn randomness(&self, out: &mut [u8]) -> i32 {
        self.orig().randomness(out)
    }

    fn sleep(&self, micro: i32) -> i32 {
        self.orig().sleep(micro)
    }

    /// `memdbCurrentTime` está em `#if 0`; o núcleo só chama `xCurrentTimeInt64`.
    fn current_time(&self, out: &mut f64) -> i32 {
        self.orig().current_time(out)
    }

    fn get_last_error(&self, n_buf: i32, buf: &mut Vec<u8>) -> i32 {
        self.orig().get_last_error(n_buf, buf)
    }

    /// `memdbCurrentTimeInt64`: chama o `xCurrentTimeInt64` do VFS original (com
    /// o recuo para `xCurrentTime` de `sqlite3OsCurrentTimeInt64` se ele for v1).
    fn current_time_int64(&self, out: &mut i64) -> Option<i32> {
        Some(os_current_time_int64(&*self.orig(), out))
    }
}

/// `sqlite3IsMemdb`: verdadeiro se o VFS é o `memdb_vfs`.
pub fn is_memdb(vfs: &dyn Vfs) -> bool {
    let g = MEMDB_VFS.lock().unwrap_or_else(PoisonError::into_inner);
    match &*g {
        Some(a) => std::ptr::addr_eq(vfs as *const dyn Vfs, Arc::as_ptr(a)),
        None => false,
    }
}

/// `sqlite3MemdbInit`: registra o VFS `memdb` (sem torná-lo o padrão). O VFS
/// por baixo é o padrão atual (`sqlite3_vfs_find(0)`).
pub fn memdb_init() -> i32 {
    let Some(lower) = vfs_find(None) else {
        return SQLITE_ERROR;
    };
    let vfs: Arc<MemVfs> = {
        let mut g = MEMDB_VFS.lock().unwrap_or_else(PoisonError::into_inner);
        match &*g {
            Some(v) => {
                *v.orig.lock().unwrap_or_else(PoisonError::into_inner) = lower;
                v.clone()
            }
            None => {
                let v = Arc::new(MemVfs { orig: Mutex::new(lower) });
                *g = Some(v.clone());
                v
            }
        }
    };
    vfs_register(vfs, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::SQLITE_FCNTL_SIZE_LIMIT;

    struct NullVfs;

    impl Vfs for NullVfs {
        fn name(&self) -> &[u8] {
            b"memdb-test-orig"
        }
        fn max_pathname(&self) -> i32 {
            512
        }
        fn open(&self, _: Option<&[u8]>, _: i32, _: &mut i32) -> Result<Box<dyn VfsFile>, i32> {
            Err(crate::consts::SQLITE_CANTOPEN)
        }
        fn access(&self, _: &[u8], _: i32, r: &mut i32) -> i32 {
            *r = 0;
            SQLITE_OK
        }
        fn full_pathname(&self, n: &[u8], _: i32, out: &mut Vec<u8>) -> i32 {
            out.extend_from_slice(n);
            SQLITE_OK
        }
        fn randomness(&self, out: &mut [u8]) -> i32 {
            out.fill(7);
            SQLITE_OK
        }
        fn sleep(&self, m: i32) -> i32 {
            m
        }
        fn current_time(&self, o: &mut f64) -> i32 {
            *o = 2.0;
            SQLITE_OK
        }
    }

    fn vfs() -> MemVfs {
        MemVfs { orig: Mutex::new(Arc::new(NullVfs)) }
    }

    #[test]
    fn separate_store_write_read_truncate() {
        let v = vfs();
        let mut of = 0;
        let mut f = v.open(Some(b"x.db"), 0, &mut of).unwrap();
        assert_eq!(of & SQLITE_OPEN_MEMORY, SQLITE_OPEN_MEMORY);
        assert_eq!(f.lock(SQLITE_LOCK_SHARED), SQLITE_OK);
        assert_eq!(f.lock(SQLITE_LOCK_EXCLUSIVE), SQLITE_OK);
        assert_eq!(f.write(&[1, 2, 3, 4], 8), SQLITE_OK);
        let mut sz = 0;
        f.file_size(&mut sz);
        assert_eq!(sz, 12);
        let mut b = [9u8; 16];
        assert_eq!(f.read(&mut b, 0), SQLITE_IOERR_SHORT_READ);
        assert_eq!(&b[..12], &[0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4]);
        assert_eq!(&b[12..], &[0, 0, 0, 0]);
        assert_eq!(f.truncate(100), SQLITE_CORRUPT);
        assert_eq!(f.truncate(4), SQLITE_OK);
        let mut arg = FileControlArg::Int64(-1);
        assert_eq!(f.file_control(SQLITE_FCNTL_SIZE_LIMIT, &mut arg), SQLITE_OK);
        assert_eq!(arg, FileControlArg::Int64(MX_MEMDB_SIZE.load(Ordering::Relaxed)));
        let mut arg = FileControlArg::Int64(2);
        f.file_control(SQLITE_FCNTL_SIZE_LIMIT, &mut arg);
        assert_eq!(arg, FileControlArg::Int64(4));
        assert_eq!(f.write(&[0; 8], 30), SQLITE_FULL);
        assert_eq!(f.unlock(SQLITE_LOCK_NONE), SQLITE_OK);
        assert_eq!(f.close(), SQLITE_OK);
    }

    #[test]
    fn shared_store_by_name() {
        let v = vfs();
        let mut of = 0;
        let mut a = v.open(Some(b"/memdb-test-shared\0vfs=memdb\0\0"), 0, &mut of).unwrap();
        let mut b = v.open(Some(b"/memdb-test-shared"), 0, &mut of).unwrap();
        assert_eq!(a.write(b"hello", 0), SQLITE_OK);
        let mut buf = [0u8; 5];
        assert_eq!(b.read(&mut buf, 0), SQLITE_OK);
        assert_eq!(&buf, b"hello");
        a.close();
        assert!(lock_memdb_g().iter().any(|(n, _)| n == b"/memdb-test-shared"));
        b.close();
        assert!(!lock_memdb_g().iter().any(|(n, _)| n == b"/memdb-test-shared"));
    }

    #[test]
    fn serialize_deserialize_store() {
        let v = vfs();
        let mut of = 0;
        let _f = v.open(None, 0, &mut of).unwrap();
        let store: MemStoreRef = Arc::new(Mutex::new(MemStore::default()));
        memdb_deserialize_store(&store, vec![5, 6, 7, 8, 0, 0], 4, 6, 0, 10);
        let mut sz = 0;
        let out = memdb_serialize_store(&store, 0, &mut sz).unwrap();
        assert_eq!((sz, &out[..]), (4, &[5u8, 6, 7, 8][..]));
        assert_eq!(store.lock().unwrap().sz_max, 10);
    }

    #[test]
    fn full_pathname_truncates() {
        let v = vfs();
        let mut out = Vec::new();
        v.full_pathname(b"abcdef", 4, &mut out);
        assert_eq!(out, b"abc");
    }
}
