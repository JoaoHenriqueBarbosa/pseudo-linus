// Mesclado das partes traduzidas de memdb_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

/// Armazenamento compartilhado de um arquivo memdb (`MemStore` do C).
///
/// Um objeto memdb pode ser compartilhado ou separado. Objetos compartilhados podem ser usados
/// por mais de uma conexão, e usam mutexes para coordenar o acesso. Objetos separados ficam
/// ligados a uma única conexão e não precisam de mutex adicional.
///
/// Objetos compartilhados têm `z_fname` e `p_mutex` presentes. Eles são criados com
/// "file:/name?vfs=memdb". O primeiro caractere do nome precisa ser "/" (ou "\\"), senão o
/// objeto é separado. Todos os compartilhados ficam em `MemFS::ap_mem_store`, em ordem
/// arbitrária. Objetos separados são criados com um nome que não começa por "/" ou por
/// `sqlite3_deserialize()`.
///
/// Regras de acesso aos objetos compartilhados:
///
///   * `z_fname` é inicializado na criação e não muda até a destruição. Pode ser lido a
///     qualquer momento em que o objeto não esteja sendo destruído, isto é, enquanto o mutex
///     SQLITE_MUTEX_STATIC_VFS1 ou `p_mutex` estiver seguro, ou o objeto não estiver em
///     `MemFS::ap_mem_store`.
///   * `p_mutex` só muda com SQLITE_MUTEX_STATIC_VFS1 seguro ou fora de `ap_mem_store`.
///   * Os demais campos só mudam com `p_mutex` seguro, ou quando `n_ref` é menor que zero e o
///     objeto não está em `ap_mem_store`.
///   * `a_data` só pode ser trocado (para redimensionar) quando `n_mmap` é zero.
///
/// `a_data` é o conteúdo do arquivo e tem sempre `len() == sz_alloc` (o equivalente seguro do
/// buffer de `szAlloc` bytes do C); o ponteiro nulo do C é o vetor vazio.
#[derive(Default)]
pub struct MemStore {
    /// Tamanho do arquivo
    pub sz: i64,
    /// Espaço alocado para `a_data`
    pub sz_alloc: i64,
    /// Tamanho máximo permitido do arquivo
    pub sz_max: i64,
    /// Conteúdo do arquivo
    pub a_data: Vec<u8>,
    /// Usado apenas por armazenamentos compartilhados
    pub p_mutex: Option<MutexRef>,
    /// Número de páginas mapeadas em memória
    pub n_mmap: i32,
    /// Flags (SQLITE_DESERIALIZE_*)
    pub m_flags: u32,
    /// Número de leitores
    pub n_rd_lock: i32,
    /// Número de escritores (sempre 0 ou 1)
    pub n_wr_lock: i32,
    /// Número de usuários deste MemStore
    pub n_ref: i32,
    /// Nome do arquivo, só para armazenamentos compartilhados (sem o NUL final)
    pub z_fname: Option<Vec<u8>>,
}

pub type MemStoreRef = Rc<RefCell<MemStore>>;

/// Arquivo aberto. O ponteiro de métodos de E/S do `sqlite3_file` (`base`) vira a
/// implementação de `VfsFile` para este tipo, feita pelo tech lead.
#[derive(Default)]
pub struct MemFile {
    /// O armazenamento
    pub p_store: Option<MemStoreRef>,
    /// Lock mais recente contra este arquivo
    pub e_lock: i32,
}

pub type MemFileRef = Rc<RefCell<MemFile>>;

/// Variável de escopo de arquivo (`memdb_g`) com os memdbs acessíveis a várias conexões.
/// `n_mem_store` do C é `ap_mem_store.len()`.
///
/// Deve-se segurar SQLITE_MUTEX_STATIC_VFS1 para acessar qualquer parte deste objeto.
#[derive(Default)]
pub struct MemFS {
    /// Todos os objetos MemStore compartilhados
    pub ap_mem_store: Vec<MemStoreRef>,
}

thread_local! {
    /// `memdb_g` do C.
    pub static MEMDB_G: RefCell<MemFS> = RefCell::new(MemFS::default());
}

/// Argumento de `memdb_file_control` (o `void *pArg` do C, que depende da operação).
pub enum MemdbFcntlArg<'a> {
    /// SQLITE_FCNTL_VFSNAME: recebe a string alocada (`char**`)
    VfsName(&'a mut Option<Vec<u8>>),
    /// SQLITE_FCNTL_SIZE_LIMIT: limite de entrada e de saída (`sqlite3_int64*`)
    SizeLimit(&'a mut i64),
    /// Qualquer outra operação
    Other,
}

/// Versão da estrutura `sqlite3_vfs` do memdb (campo iVersion).
pub const MEMDB_VFS_VERSION: i32 = 2;
/// `mxPathname` do VFS memdb.
pub const MEMDB_VFS_MX_PATHNAME: i32 = 1024;
/// `zName` do VFS memdb.
pub const MEMDB_VFS_NAME: &[u8] = b"memdb";
/// Versão da estrutura `sqlite3_io_methods` do memdb (campo iVersion).
pub const MEMDB_IO_METHODS_VERSION: i32 = 3;

// As tabelas `memdb_vfs` e `memdb_io_methods` do C viram implementações de `Vfs` e `VfsFile`
// (módulo `os`) feitas pelo tech lead sobre as funções `memdb_*` deste módulo. Ficam sem
// método (nulo no C): xDelete, xCurrentTime, xCheckReservedLock, xSectorSize, xShmMap,
// xShmLock, xShmBarrier, xShmUnmap, xSetSystemCall, xGetSystemCall e xNextSystemCall.

/// Devolve o armazenamento de um arquivo aberto (`((MemFile*)pFile)->pStore`).
pub fn memdb_store(p_file: &MemFile) -> MemStoreRef {
    p_file
        .p_store
        .clone()
        .expect("arquivo memdb usado antes de memdb_open")
}

/// Entra no mutex de um MemStore.
pub fn memdb_enter(p: &MemStoreRef) {
    let m = p.borrow().p_mutex.clone();
    mutex_enter(m.as_ref());
}

/// Sai do mutex de um MemStore.
pub fn memdb_leave(p: &MemStoreRef) {
    let m = p.borrow().p_mutex.clone();
    mutex_leave(m.as_ref());
}

/// Fecha um arquivo memdb. Libera o MemStore subjacente quando a contagem de referências cai
/// para zero ou menos.
pub fn memdb_close(p_file: &mut MemFile) -> i32 {
    let p = memdb_store(p_file);
    if p.borrow().z_fname.is_some() {
        let p_vfs_mutex = mutex_alloc(SQLITE_MUTEX_STATIC_VFS1);
        mutex_enter(p_vfs_mutex.as_ref());
        MEMDB_G.with(|g| {
            let mut g = g.borrow_mut();
            // for(i=0; ALWAYS(i<memdb_g.nMemStore); i++)
            for i in 0..g.ap_mem_store.len() {
                if Rc::ptr_eq(&g.ap_mem_store[i], &p) {
                    memdb_enter(&p);
                    if p.borrow().n_ref == 1 {
                        // apMemStore[i] = apMemStore[--nMemStore]
                        g.ap_mem_store.swap_remove(i);
                        if g.ap_mem_store.is_empty() {
                            // sqlite3_free(memdb_g.apMemStore); apMemStore = 0
                            g.ap_mem_store = Vec::new();
                        }
                    }
                    break;
                }
            }
        });
        mutex_leave(p_vfs_mutex.as_ref());
    } else {
        memdb_enter(&p);
    }
    let n_ref = {
        let mut st = p.borrow_mut();
        st.n_ref -= 1;
        st.n_ref
    };
    if n_ref <= 0 {
        {
            let mut st = p.borrow_mut();
            if st.m_flags & SQLITE_DESERIALIZE_FREEONCLOSE != 0 {
                st.a_data = Vec::new();
            }
        }
        memdb_leave(&p);
        let m = p.borrow_mut().p_mutex.take();
        mutex_free(m);
    } else {
        memdb_leave(&p);
    }
    SQLITE_OK
}

/// Lê dados de um arquivo memdb.
pub fn memdb_read(p_file: &mut MemFile, z_buf: &mut [u8], i_amt: i32, i_ofst: i64) -> i32 {
    let p = memdb_store(p_file);
    memdb_enter(&p);
    let rc = {
        let st = p.borrow();
        let n_amt = i_amt as usize;
        if i_ofst + i_amt as i64 > st.sz {
            z_buf[..n_amt].fill(0);
            if i_ofst < st.sz {
                let n = (st.sz - i_ofst) as usize;
                z_buf[..n].copy_from_slice(&st.a_data[i_ofst as usize..i_ofst as usize + n]);
            }
            SQLITE_IOERR_SHORT_READ
        } else {
            z_buf[..n_amt].copy_from_slice(&st.a_data[i_ofst as usize..i_ofst as usize + n_amt]);
            SQLITE_OK
        }
    };
    memdb_leave(&p);
    rc
}

/// Tenta ampliar a alocação de memória para conter pelo menos `new_sz` bytes.
pub fn memdb_enlarge(p: &mut MemStore, new_sz: i64) -> i32 {
    // NEVER(p->nMmap>0)
    if (p.m_flags & SQLITE_DESERIALIZE_RESIZEABLE) == 0 || p.n_mmap > 0 {
        return SQLITE_FULL;
    }
    if new_sz > p.sz_max {
        return SQLITE_FULL;
    }
    let mut new_sz = new_sz.wrapping_mul(2);
    if new_sz > p.sz_max {
        new_sz = p.sz_max;
    }
    // pNew = sqlite3Realloc(p->aData, newSz); falha de alocação vira SQLITE_IOERR_NOMEM
    let n_new = new_sz as usize;
    if n_new > p.a_data.len() {
        if p.a_data.try_reserve_exact(n_new - p.a_data.len()).is_err() {
            return SQLITE_IOERR_NOMEM;
        }
    }
    p.a_data.resize(n_new, 0);
    p.sz_alloc = new_sz;
    SQLITE_OK
}

/// Escreve dados em um arquivo memdb.
pub fn memdb_write(p_file: &mut MemFile, z: &[u8], i_amt: i32, i_ofst: i64) -> i32 {
    let p = memdb_store(p_file);
    memdb_enter(&p);
    let rc = {
        let mut st = p.borrow_mut();
        if st.m_flags & SQLITE_DESERIALIZE_READONLY != 0 {
            // Não acontece: memdb_lock() devolve SQLITE_READONLY antes de chegar aqui
            SQLITE_IOERR_WRITE
        } else {
            let end = i_ofst + i_amt as i64;
            let mut rc = SQLITE_OK;
            if end > st.sz {
                if end > st.sz_alloc {
                    let r = memdb_enlarge(&mut st, end);
                    if r != SQLITE_OK {
                        rc = r;
                    }
                }
                if rc == SQLITE_OK {
                    if i_ofst > st.sz {
                        let from = st.sz as usize;
                        st.a_data[from..i_ofst as usize].fill(0);
                    }
                    st.sz = end;
                }
            }
            if rc == SQLITE_OK {
                st.a_data[i_ofst as usize..end as usize].copy_from_slice(&z[..i_amt as usize]);
            }
            rc
        }
    };
    memdb_leave(&p);
    rc
}

/// Trunca um arquivo memdb.
///
/// Em modo rollback (sempre o caso do memdb, que não suporta WAL) truncate() só é usado para
/// reduzir o tamanho do arquivo, nunca para aumentar.
pub fn memdb_truncate(p_file: &mut MemFile, size: i64) -> i32 {
    let p = memdb_store(p_file);
    let mut rc = SQLITE_OK;
    memdb_enter(&p);
    {
        let mut st = p.borrow_mut();
        if size > st.sz {
            // Só pode acontecer com um banco corrompido em modo WAL
            rc = SQLITE_CORRUPT;
        } else {
            st.sz = size;
        }
    }
    memdb_leave(&p);
    rc
}

/// Sincroniza um arquivo memdb (não faz nada).
pub fn memdb_sync(_p_file: &mut MemFile, _flags: i32) -> i32 {
    SQLITE_OK
}


// ---- part_001.rs ----

/// Devolve o tamanho atual de um arquivo memdb.
pub fn memdb_file_size(p_file: &mut MemFile, p_size: &mut i64) -> i32 {
    let p = memdb_store(p_file);
    memdb_enter(&p);
    *p_size = p.borrow().sz;
    memdb_leave(&p);
    SQLITE_OK
}

/// Trava um arquivo memdb.
pub fn memdb_lock(p_this: &mut MemFile, e_lock: i32) -> i32 {
    let p = memdb_store(p_this);
    let mut rc = SQLITE_OK;
    if e_lock <= p_this.e_lock {
        return SQLITE_OK;
    }
    memdb_enter(&p);
    {
        let mut st = p.borrow_mut();

        debug_assert!(st.n_wr_lock == 0 || st.n_wr_lock == 1);
        debug_assert!(p_this.e_lock <= SQLITE_LOCK_SHARED || st.n_wr_lock == 1);
        debug_assert!(p_this.e_lock == SQLITE_LOCK_NONE || st.n_rd_lock >= 1);

        if e_lock > SQLITE_LOCK_SHARED && (st.m_flags & SQLITE_DESERIALIZE_READONLY) != 0 {
            rc = SQLITE_READONLY;
        } else {
            match e_lock {
                SQLITE_LOCK_SHARED => {
                    debug_assert!(p_this.e_lock == SQLITE_LOCK_NONE);
                    if st.n_wr_lock > 0 {
                        rc = SQLITE_BUSY;
                    } else {
                        st.n_rd_lock += 1;
                    }
                }
                SQLITE_LOCK_RESERVED | SQLITE_LOCK_PENDING => {
                    debug_assert!(p_this.e_lock >= SQLITE_LOCK_SHARED);
                    // ALWAYS(pThis->eLock==SQLITE_LOCK_SHARED)
                    if p_this.e_lock == SQLITE_LOCK_SHARED {
                        if st.n_wr_lock > 0 {
                            rc = SQLITE_BUSY;
                        } else {
                            st.n_wr_lock = 1;
                        }
                    }
                }
                _ => {
                    debug_assert!(e_lock == SQLITE_LOCK_EXCLUSIVE);
                    debug_assert!(p_this.e_lock >= SQLITE_LOCK_SHARED);
                    if st.n_rd_lock > 1 {
                        rc = SQLITE_BUSY;
                    } else if p_this.e_lock == SQLITE_LOCK_SHARED {
                        st.n_wr_lock = 1;
                    }
                }
            }
        }
    }
    if rc == SQLITE_OK {
        p_this.e_lock = e_lock;
    }
    memdb_leave(&p);
    rc
}

/// Destrava um arquivo memdb.
pub fn memdb_unlock(p_this: &mut MemFile, e_lock: i32) -> i32 {
    let p = memdb_store(p_this);
    if e_lock >= p_this.e_lock {
        return SQLITE_OK;
    }
    memdb_enter(&p);
    {
        let mut st = p.borrow_mut();
        debug_assert!(e_lock == SQLITE_LOCK_SHARED || e_lock == SQLITE_LOCK_NONE);
        if e_lock == SQLITE_LOCK_SHARED {
            // ALWAYS(pThis->eLock>SQLITE_LOCK_SHARED)
            if p_this.e_lock > SQLITE_LOCK_SHARED {
                st.n_wr_lock -= 1;
            }
        } else {
            if p_this.e_lock > SQLITE_LOCK_SHARED {
                st.n_wr_lock -= 1;
            }
            st.n_rd_lock -= 1;
        }
    }
    p_this.e_lock = e_lock;
    memdb_leave(&p);
    SQLITE_OK
}

// memdbCheckReservedLock (#if 0 no C): só serviria à recuperação de falhas, que não ocorre
// em banco em memória. Não é traduzida.

/// Método de controle de arquivo, para operações customizadas em um arquivo memdb.
pub fn memdb_file_control(p_file: &mut MemFile, op: i32, p_arg: MemdbFcntlArg) -> i32 {
    let p = memdb_store(p_file);
    let mut rc = SQLITE_NOTFOUND;
    memdb_enter(&p);
    {
        let mut st = p.borrow_mut();
        match p_arg {
            MemdbFcntlArg::VfsName(out) if op == SQLITE_FCNTL_VFSNAME => {
                // sqlite3_mprintf("memdb(%p,%lld)", p->aData, p->sz): o %p do SQLite imprime o
                // endereço em hexadecimal minúsculo com prefixo 0x. O endereço do buffer não
                // é reproduzível entre o C e o Rust; o vetor vazio faz o papel do ponteiro nulo.
                let addr: usize = if st.a_data.is_empty() {
                    0
                } else {
                    st.a_data.as_ptr() as usize
                };
                *out = Some(format!("memdb(0x{:x},{})", addr, st.sz).into_bytes());
                rc = SQLITE_OK;
            }
            MemdbFcntlArg::SizeLimit(arg) if op == SQLITE_FCNTL_SIZE_LIMIT => {
                let mut i_limit = *arg;
                if i_limit < st.sz {
                    if i_limit < 0 {
                        i_limit = st.sz_max;
                    } else {
                        i_limit = st.sz;
                    }
                }
                st.sz_max = i_limit;
                *arg = i_limit;
                rc = SQLITE_OK;
            }
            _ => {}
        }
    }
    memdb_leave(&p);
    rc
}

// memdbSectorSize (#if 0 no C, não usada por causa de SQLITE_IOCAP_POWERSAFE_OVERWRITE):
// devolveria 1024. Não é traduzida.

/// Devolve as flags de características de dispositivo suportadas por um arquivo memdb.
pub fn memdb_device_characteristics(_p_file: &MemFile) -> i32 {
    SQLITE_IOCAP_ATOMIC
        | SQLITE_IOCAP_POWERSAFE_OVERWRITE
        | SQLITE_IOCAP_SAFE_APPEND
        | SQLITE_IOCAP_SEQUENTIAL
}

/// Busca uma página de um arquivo mapeado em memória. O ponteiro `aData + iOfst` do C vira o
/// deslocamento dentro de `a_data` (`None` é o ponteiro nulo).
pub fn memdb_fetch(p_file: &mut MemFile, i_ofst: i64, i_amt: i32, pp: &mut Option<usize>) -> i32 {
    let p = memdb_store(p_file);
    memdb_enter(&p);
    {
        let mut st = p.borrow_mut();
        if i_ofst + i_amt as i64 > st.sz || (st.m_flags & SQLITE_DESERIALIZE_RESIZEABLE) != 0 {
            *pp = None;
        } else {
            st.n_mmap += 1;
            *pp = Some(i_ofst as usize);
        }
    }
    memdb_leave(&p);
    SQLITE_OK
}

/// Libera uma página mapeada em memória.
pub fn memdb_unfetch(p_file: &mut MemFile, _i_ofst: i64, _p_page: Option<usize>) -> i32 {
    let p = memdb_store(p_file);
    memdb_enter(&p);
    p.borrow_mut().n_mmap -= 1;
    memdb_leave(&p);
    SQLITE_OK
}

/// Abre um identificador de arquivo memdb. `z_name` é o nome sem o NUL final.
pub fn memdb_open(
    _p_vfs: &MemVfs,
    z_name: Option<&[u8]>,
    p_file: &mut MemFile,
    flags: i32,
    p_out_flags: Option<&mut i32>,
) -> i32 {
    // memset(pFile, 0, sizeof(*pFile))
    *p_file = MemFile::default();
    // sqlite3Strlen30 para no primeiro NUL
    let z_name: &[u8] = match z_name {
        Some(z) => {
            let n = z.iter().position(|&c| c == 0).unwrap_or(z.len());
            &z[..n]
        }
        None => &[],
    };
    let sz_name = z_name.len();
    let p: MemStoreRef;
    if sz_name > 1 && (z_name[0] == b'/' || z_name[0] == b'\\') {
        let p_vfs_mutex = mutex_alloc(SQLITE_MUTEX_STATIC_VFS1);
        mutex_enter(p_vfs_mutex.as_ref());
        let found: Option<MemStoreRef> = MEMDB_G.with(|g| {
            g.borrow()
                .ap_mem_store
                .iter()
                .find(|s| s.borrow().z_fname.as_deref() == Some(z_name))
                .cloned()
        });
        match found {
            None => {
                let new: MemStoreRef = Rc::new(RefCell::new(MemStore {
                    m_flags: SQLITE_DESERIALIZE_RESIZEABLE | SQLITE_DESERIALIZE_FREEONCLOSE,
                    sz_max: global_config().mx_memdb_size,
                    z_fname: Some(z_name.to_vec()),
                    ..MemStore::default()
                }));
                MEMDB_G.with(|g| g.borrow_mut().ap_mem_store.push(new.clone()));
                let m = mutex_alloc(SQLITE_MUTEX_FAST);
                if m.is_none() {
                    // memdb_g.nMemStore--
                    MEMDB_G.with(|g| {
                        g.borrow_mut().ap_mem_store.pop();
                    });
                    mutex_leave(p_vfs_mutex.as_ref());
                    return SQLITE_NOMEM;
                }
                {
                    let mut st = new.borrow_mut();
                    st.p_mutex = m;
                    st.n_ref = 1;
                }
                memdb_enter(&new);
                p = new;
            }
            Some(s) => {
                memdb_enter(&s);
                s.borrow_mut().n_ref += 1;
                p = s;
            }
        }
        mutex_leave(p_vfs_mutex.as_ref());
    } else {
        p = Rc::new(RefCell::new(MemStore {
            m_flags: SQLITE_DESERIALIZE_RESIZEABLE | SQLITE_DESERIALIZE_FREEONCLOSE,
            sz_max: global_config().mx_memdb_size,
            ..MemStore::default()
        }));
    }
    p_file.p_store = Some(p.clone());
    if let Some(out) = p_out_flags {
        *out = flags | SQLITE_OPEN_MEMORY;
    }
    // pFd->pMethods = &memdb_io_methods: feito pela implementação de VfsFile de MemFile
    memdb_leave(&p);
    SQLITE_OK
}

// memdbDelete (#if 0 no C): só serviria para apagar journals, super-journals e arquivos WAL,
// que não existem no memdb. Nunca é usada, não é traduzida.

/// Testa permissões de acesso. Com memdb nenhum arquivo existe em disco, então sempre devolve
/// falso.
pub fn memdb_access(_p_vfs: &MemVfs, _z_path: &[u8], _flags: i32, p_res_out: &mut i32) -> i32 {
    *p_res_out = 0;
    SQLITE_OK
}

/// Preenche `z_out` com o nome de caminho canônico completo de `z_path`
/// (`sqlite3_snprintf(nOut, zOut, "%s", zPath)`: copia no máximo `n_out - 1` bytes e termina
/// com NUL).
pub fn memdb_full_pathname(_p_vfs: &MemVfs, z_path: &[u8], n_out: i32, z_out: &mut [u8]) -> i32 {
    if n_out > 0 {
        let z_path = &z_path[..z_path.iter().position(|&c| c == 0).unwrap_or(z_path.len())];
        let cap = (n_out as usize - 1).min(z_out.len().saturating_sub(1));
        let n = z_path.len().min(cap);
        z_out[..n].copy_from_slice(&z_path[..n]);
        if n < z_out.len() {
            z_out[n] = 0;
        }
    }
    SQLITE_OK
}

/// Abre a biblioteca dinâmica em `z_path` e devolve um identificador (delegado ao VFS de baixo).
pub fn memdb_dl_open(p_vfs: &MemVfs, z_path: &[u8]) -> Option<DlHandle> {
    orig_vfs(p_vfs).x_dl_open(z_path)
}

/// Preenche `z_err_msg` (`n_byte` bytes) com uma string utf-8 legível sobre o erro mais recente
/// de bibliotecas dinâmicas (delegado ao VFS de baixo).
pub fn memdb_dl_error(p_vfs: &MemVfs, n_byte: i32, z_err_msg: &mut [u8]) {
    orig_vfs(p_vfs).x_dl_error(n_byte, z_err_msg)
}

/// Devolve o símbolo `z_sym` da biblioteca dinâmica `p` (delegado ao VFS de baixo).
pub fn memdb_dl_sym(p_vfs: &MemVfs, p: &DlHandle, z_sym: &[u8]) -> Option<DlSym> {
    orig_vfs(p_vfs).x_dl_sym(p, z_sym)
}

/// Fecha o identificador de biblioteca dinâmica (delegado ao VFS de baixo).
pub fn memdb_dl_close(p_vfs: &MemVfs, p_handle: DlHandle) {
    orig_vfs(p_vfs).x_dl_close(p_handle)
}

/// Preenche o buffer com `n_byte` bytes aleatórios (delegado ao VFS de baixo).
pub fn memdb_randomness(p_vfs: &MemVfs, n_byte: i32, z_buf_out: &mut [u8]) -> i32 {
    orig_vfs(p_vfs).x_randomness(n_byte, z_buf_out)
}


// ---- part_002.rs ----

/// O VFS memdb (`memdb_vfs` do C). `p_app_data` guarda o VFS de baixo (`ORIGVFS`), que dá
/// carregamento dinâmico, aleatoriedade, sleep e hora. A tabela de métodos do C vira a
/// implementação de `Vfs` feita pelo tech lead sobre as funções `memdb_*`.
pub struct MemVfs {
    /// Tamanho do objeto de arquivo aberto (`szOsFile`), ajustado em `memdb_init`
    pub sz_os_file: i32,
    /// O VFS de baixo (`pAppData`), definido quando registrado
    pub p_app_data: Option<Rc<dyn Vfs>>,
}

thread_local! {
    /// O objeto estático `memdb_vfs` do C, criado por `memdb_init`.
    pub static MEMDB_VFS: RefCell<Option<Rc<MemVfs>>> = RefCell::new(None);
}

/// Acesso ao VFS de baixo (macro `ORIGVFS` do C).
pub fn orig_vfs(p: &MemVfs) -> &Rc<dyn Vfs> {
    p.p_app_data
        .as_ref()
        .expect("memdb_vfs usado antes de memdb_init")
}

/// Dorme por `n_micro` microssegundos. Devolve os microssegundos realmente dormidos.
pub fn memdb_sleep(p_vfs: &MemVfs, n_micro: i32) -> i32 {
    orig_vfs(p_vfs).x_sleep(n_micro)
}

// memdbCurrentTime (#if 0 no C): nunca usada, os núcleos modernos só chamam
// xCurrentTimeInt64(). Não é traduzida.

pub fn memdb_get_last_error(p_vfs: &MemVfs, a: i32, b: &mut [u8]) -> i32 {
    orig_vfs(p_vfs).x_get_last_error(a, b)
}

pub fn memdb_current_time_int64(p_vfs: &MemVfs, p: &mut i64) -> i32 {
    orig_vfs(p_vfs).x_current_time_int64(p)
}

/// Traduz uma conexão e um nome de esquema em um MemFile. Devolve `None` se o arquivo não é
/// memdb (o teste `pMethods != &memdb_io_methods` do C é feito por `file_control_file_pointer`,
/// que devolve `None` quando o arquivo não é um MemFile) ou se o armazenamento é compartilhado.
pub fn memdb_from_db_schema(db: &Sqlite3Ref, z_schema: &[u8]) -> Option<MemFileRef> {
    let (rc, p) = api::file_control_file_pointer(db, z_schema);
    if rc != SQLITE_OK {
        return None;
    }
    let p = p?;
    let p_store = memdb_store(&p.borrow());
    memdb_enter(&p_store);
    let shared = p_store.borrow().z_fname.is_some();
    memdb_leave(&p_store);
    if shared {
        None
    } else {
        Some(p)
    }
}

/// Resultado de `api::serialize`: o buffer próprio, ou (SQLITE_SERIALIZE_NOCOPY) o armazenamento
/// cujo `a_data` o C devolveria diretamente como ponteiro.
pub enum Serialized {
    Owned(Vec<u8>),
    NoCopy(MemStoreRef),
}

/// Aloca `n` bytes zerados; `None` quando a alocação falha (`sqlite3_malloc64` devolvendo nulo).
fn memdb_alloc_zeroed(n: usize) -> Option<Vec<u8>> {
    let mut v: Vec<u8> = Vec::new();
    if v.try_reserve_exact(n).is_err() {
        return None;
    }
    v.resize(n, 0);
    Some(v)
}

/// Escapa `z` como `%w` (aspas duplas dobradas) ou `%Q` sem as aspas externas (aspas simples
/// dobradas) do `sqlite3_mprintf`.
fn memdb_escape(z: &[u8], q: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(z.len() + 2);
    for &c in z {
        out.push(c);
        if c == q {
            out.push(q);
        }
    }
    out
}

/// Devolve a serialização de um banco de dados (`sqlite3_serialize`).
pub fn serialize(
    db: &Sqlite3Ref,
    z_schema: Option<&[u8]>,
    mut pi_size: Option<&mut i64>,
    m_flags: u32,
) -> Option<Serialized> {
    let z_schema: Vec<u8> = match z_schema {
        Some(z) => z.to_vec(),
        None => db.borrow().a_db[0].z_db_s_name.clone(),
    };
    let p = memdb_from_db_schema(db, &z_schema);
    let i_db = find_db_name(db, &z_schema);
    if let Some(s) = pi_size.as_mut() {
        **s = -1;
    }
    if i_db < 0 {
        return None;
    }
    if let Some(p) = p {
        let p_store = memdb_store(&p.borrow());
        let st = p_store.borrow();
        debug_assert!(st.p_mutex.is_none());
        if let Some(s) = pi_size.as_mut() {
            **s = st.sz;
        }
        if m_flags & SQLITE_SERIALIZE_NOCOPY != 0 {
            drop(st);
            return Some(Serialized::NoCopy(p_store));
        }
        let sz = st.sz as usize;
        let mut p_out = memdb_alloc_zeroed(sz)?;
        p_out.copy_from_slice(&st.a_data[..sz]);
        return Some(Serialized::Owned(p_out));
    }
    let p_bt = db.borrow().a_db[i_db as usize].p_bt.clone();
    let p_bt = p_bt?;
    let sz_page = btree_get_page_size(&p_bt);
    let mut z_sql: Vec<u8> = b"PRAGMA \"".to_vec();
    z_sql.extend_from_slice(&memdb_escape(&z_schema, b'"'));
    z_sql.extend_from_slice(b"\".page_count");
    let (rc, p_stmt) = api::prepare_v2(db, &z_sql, -1);
    if rc != 0 {
        return None;
    }
    let mut p_out: Option<Serialized> = None;
    let rc = api::step(&p_stmt);
    if rc == SQLITE_ROW {
        let mut sz: i64 = api::column_int64(&p_stmt, 0) * sz_page as i64;
        if sz == 0 {
            api::reset(&p_stmt);
            api::exec(db, b"BEGIN IMMEDIATE; COMMIT;", None, None);
            let rc = api::step(&p_stmt);
            if rc == SQLITE_ROW {
                sz = api::column_int64(&p_stmt, 0) * sz_page as i64;
            }
        }
        if let Some(s) = pi_size.as_mut() {
            **s = sz;
        }
        if m_flags & SQLITE_SERIALIZE_NOCOPY == 0 {
            if let Some(mut out) = memdb_alloc_zeroed(sz as usize) {
                let n_page = api::column_int(&p_stmt, 0);
                let p_pager = btree_pager(&p_bt);
                for pgno in 1..=n_page {
                    let to = sz_page as usize * (pgno - 1) as usize;
                    let (rc, p_page) = pager_get(&p_pager, pgno as u32, 0);
                    if rc == SQLITE_OK {
                        let data = pager_get_data(p_page.as_ref().unwrap());
                        out[to..to + sz_page as usize].copy_from_slice(&data[..sz_page as usize]);
                    } else {
                        out[to..to + sz_page as usize].fill(0);
                    }
                    pager_unref(p_page);
                }
                p_out = Some(Serialized::Owned(out));
            }
        }
    }
    api::finalize(p_stmt);
    p_out
}

/// Converte `z_schema` em um MemDB e inicializa seu conteúdo (`sqlite3_deserialize`). O buffer
/// `p_data` passa a pertencer ao armazenamento; seu `len()` deve ser `sz_buf`.
pub fn deserialize(
    db: &Sqlite3Ref,
    z_schema: Option<&[u8]>,
    mut p_data: Option<Vec<u8>>,
    sz_db: i64,
    sz_buf: i64,
    m_flags: u32,
) -> i32 {
    let db_mutex = db.borrow().mutex.clone();
    mutex_enter(db_mutex.as_ref());
    let z_schema: Vec<u8> = match z_schema {
        Some(z) => z.to_vec(),
        None => db.borrow().a_db[0].z_db_s_name.clone(),
    };
    let i_db = find_db_name(db, &z_schema);
    let mut rc = SQLITE_OK;
    let mut p_stmt: Option<StmtRef> = None;
    'end_deserialize: {
        if i_db < 2 && i_db != 0 {
            rc = SQLITE_ERROR;
            break 'end_deserialize;
        }
        let mut z_sql: Vec<u8> = b"ATTACH x AS '".to_vec();
        z_sql.extend_from_slice(&memdb_escape(&z_schema, b'\''));
        z_sql.push(b'\'');
        let (r, st) = api::prepare_v2(db, &z_sql, -1);
        rc = r;
        p_stmt = st;
        if rc != 0 {
            break 'end_deserialize;
        }
        db.borrow_mut().init.i_db = i_db as u8;
        db.borrow_mut().init.reopen_memdb = 1;
        rc = api::step(&p_stmt);
        db.borrow_mut().init.reopen_memdb = 0;
        if rc != SQLITE_DONE {
            rc = SQLITE_ERROR;
            break 'end_deserialize;
        }
        match memdb_from_db_schema(db, &z_schema) {
            None => {
                rc = SQLITE_ERROR;
            }
            Some(p) => {
                let p_store = memdb_store(&p.borrow());
                let mut st = p_store.borrow_mut();
                st.a_data = p_data.take().unwrap_or_default();
                st.sz = sz_db;
                st.sz_alloc = sz_buf;
                st.sz_max = sz_buf;
                if st.sz_max < global_config().mx_memdb_size {
                    st.sz_max = global_config().mx_memdb_size;
                }
                st.m_flags = m_flags;
                rc = SQLITE_OK;
            }
        }
    }
    api::finalize(p_stmt);
    if p_data.is_some() && (m_flags & SQLITE_DESERIALIZE_FREEONCLOSE) != 0 {
        // sqlite3_free(pData)
        drop(p_data);
    }
    mutex_leave(db_mutex.as_ref());
    rc
}

/// Devolve verdadeiro se o VFS é o memvfs.
pub fn memdb_is_memdb(p_vfs: &Rc<dyn Vfs>) -> i32 {
    MEMDB_VFS.with(|v| match v.borrow().as_ref() {
        Some(m) => std::ptr::addr_eq(Rc::as_ptr(p_vfs), Rc::as_ptr(m)) as i32,
        None => 0,
    })
}

/// Chamada quando a extensão é carregada. Registra o novo VFS.
pub fn memdb_init() -> i32 {
    let p_lower = match vfs_find(None) {
        Some(p) => p,
        None => return SQLITE_ERROR,
    };
    let mut sz = p_lower.sz_os_file() as u32;
    // O condicional seguinte só é verdadeiro ao compilar para Windows x86 com
    // SQLITE_MAX_MMAP_SIZE=0. Fica sempre, por segurança (NO_TEST no C).
    if (sz as usize) < std::mem::size_of::<MemFile>() {
        sz = std::mem::size_of::<MemFile>() as u32;
    }
    let vfs = Rc::new(MemVfs {
        sz_os_file: sz as i32,
        p_app_data: Some(p_lower),
    });
    MEMDB_VFS.with(|v| *v.borrow_mut() = Some(vfs.clone()));
    vfs_register(vfs, 0)
}

