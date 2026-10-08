// Mesclado das partes traduzidas de os_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Contrato esperado de `trait VfsFile` e `trait Vfs` (módulo `os`, do tech lead).
// Método ausente no C (ponteiro de função nulo) vira `Option` no retorno.
//   Sqlite3File { p_methods: Option<Box<dyn VfsFile>> }   (implementa Default)
//   VfsRef = Rc<dyn Vfs>
//   VfsFile: x_close(&mut self), x_read(&mut self, &mut [u8], i64) -> i32,
//     x_write(&mut self, &[u8], i64) -> i32, x_truncate(&mut self, i64) -> i32,
//     x_sync(&mut self, i32) -> i32, x_file_size(&mut self, &mut i64) -> i32,
//     x_lock/x_unlock(&mut self, i32) -> i32, x_check_reserved_lock(&mut self, &mut i32) -> i32,
//     x_file_control(&mut self, i32, Option<&mut dyn Any>) -> i32,
//     x_sector_size(&mut self) -> Option<i32>, x_device_characteristics(&mut self) -> i32,
//     x_shm_map(&mut self, i32, i32, i32, &mut Option<Rc<RefCell<Vec<u8>>>>) -> i32,
//     x_shm_lock(&mut self, i32, i32, i32) -> i32, x_shm_barrier(&mut self),
//     x_shm_unmap(&mut self, i32) -> i32,
//     x_fetch(&mut self, i64, i32, &mut Option<Vec<u8>>) -> i32, x_unfetch(&mut self, i64) -> i32.
//   Vfs: i_version(&self) -> i32, sz_os_file(&self) -> i32, z_name(&self) -> &[u8] (sem NUL),
//     p_next(&self) -> Option<VfsRef>, set_p_next(&self, Option<VfsRef>),
//     x_open(&self, Option<&[u8]>, &mut Sqlite3File, i32, Option<&mut i32>) -> i32,
//     x_delete(&self, &[u8], i32) -> Option<i32>, x_access(&self, &[u8], i32, &mut i32) -> i32,
//     x_full_pathname(&self, &[u8], &mut [u8]) -> i32,
//     x_dl_open(&self, &[u8]) -> Option<usize>, x_dl_error(&self, &mut [u8]),
//     x_dl_sym(&self, usize, &[u8]) -> Option<DlSymFn>, x_dl_close(&self, usize),
//     x_randomness(&self, &mut [u8]) -> i32, x_sleep(&self, i32) -> i32,
//     x_current_time(&self, &mut f64) -> i32,
//     x_current_time_int64(&self, &mut i64) -> Option<i32>, x_get_last_error(&self) -> Option<i32>.

/// Ponteiro de função devolvido por `os_dl_sym` (`void (*)(void)` no C).
pub type DlSymFn = fn();

thread_local! {
    /// Lista de todas as implementações de VFS registradas (`vfsList` no C).
    pub static VFS_LIST: std::cell::RefCell<Option<VfsRef>> = const { std::cell::RefCell::new(None) };
}

/// Fecha o arquivo e zera os métodos. Sem métodos, não faz nada.
pub fn os_close(p_id: &mut Sqlite3File) {
    if let Some(mut methods) = p_id.p_methods.take() {
        methods.x_close();
    }
}

/// Lê `buf.len()` bytes a partir de `offset`.
pub fn os_read(id: &mut Sqlite3File, buf: &mut [u8], offset: i64) -> i32 {
    id.p_methods.as_mut().unwrap().x_read(buf, offset)
}

/// Escreve `buf.len()` bytes a partir de `offset`.
pub fn os_write(id: &mut Sqlite3File, buf: &[u8], offset: i64) -> i32 {
    id.p_methods.as_mut().unwrap().x_write(buf, offset)
}

/// Trunca o arquivo para `size` bytes.
pub fn os_truncate(id: &mut Sqlite3File, size: i64) -> i32 {
    id.p_methods.as_mut().unwrap().x_truncate(size)
}

/// Sincroniza o arquivo. Com `flags` zero não faz nada e devolve `SQLITE_OK`.
pub fn os_sync(id: &mut Sqlite3File, flags: i32) -> i32 {
    if flags != 0 {
        id.p_methods.as_mut().unwrap().x_sync(flags)
    } else {
        SQLITE_OK
    }
}

/// Obtém o tamanho do arquivo em `p_size`.
pub fn os_file_size(id: &mut Sqlite3File, p_size: &mut i64) -> i32 {
    id.p_methods.as_mut().unwrap().x_file_size(p_size)
}

/// Adquire um lock do tipo `lock_type`.
pub fn os_lock(id: &mut Sqlite3File, lock_type: i32) -> i32 {
    id.p_methods.as_mut().unwrap().x_lock(lock_type)
}

/// Libera o lock para `lock_type` (`SQLITE_LOCK_NONE` ou `SQLITE_LOCK_SHARED`).
pub fn os_unlock(id: &mut Sqlite3File, lock_type: i32) -> i32 {
    id.p_methods.as_mut().unwrap().x_unlock(lock_type)
}

/// Verifica se algum processo mantém um lock reservado; resultado em `p_res_out`.
pub fn os_check_reserved_lock(id: &mut Sqlite3File, p_res_out: &mut i32) -> i32 {
    id.p_methods.as_mut().unwrap().x_check_reserved_lock(p_res_out)
}

/// File control que pode falhar e cujo erro importa. Sem métodos devolve `SQLITE_NOTFOUND`.
pub fn os_file_control(id: &mut Sqlite3File, op: i32, p_arg: Option<&mut dyn std::any::Any>) -> i32 {
    match id.p_methods.as_mut() {
        None => SQLITE_NOTFOUND,
        Some(methods) => methods.x_file_control(op, p_arg),
    }
}

/// File control como dica: o retorno é descartado e a falta de métodos é ignorada.
pub fn os_file_control_hint(id: &mut Sqlite3File, op: i32, p_arg: Option<&mut dyn std::any::Any>) {
    if let Some(methods) = id.p_methods.as_mut() {
        let _ = methods.x_file_control(op, p_arg);
    }
}

/// Tamanho de setor do arquivo; `SQLITE_DEFAULT_SECTOR_SIZE` quando o VFS não implementa.
pub fn os_sector_size(id: &mut Sqlite3File) -> i32 {
    match id.p_methods.as_mut().unwrap().x_sector_size() {
        Some(n) => n,
        None => SQLITE_DEFAULT_SECTOR_SIZE,
    }
}

/// Características do dispositivo; zero se o arquivo não tem métodos.
pub fn os_device_characteristics(id: &mut Sqlite3File) -> i32 {
    match id.p_methods.as_mut() {
        None => 0,
        Some(methods) => methods.x_device_characteristics(),
    }
}

/// Lock na memória compartilhada do WAL.
pub fn os_shm_lock(id: &mut Sqlite3File, offset: i32, n: i32, flags: i32) -> i32 {
    id.p_methods.as_mut().unwrap().x_shm_lock(offset, n, flags)
}

/// Barreira de memória da região compartilhada do WAL.
pub fn os_shm_barrier(id: &mut Sqlite3File) {
    id.p_methods.as_mut().unwrap().x_shm_barrier();
}

/// Desfaz o mapeamento da memória compartilhada do WAL.
pub fn os_shm_unmap(id: &mut Sqlite3File, delete_flag: i32) -> i32 {
    id.p_methods.as_mut().unwrap().x_shm_unmap(delete_flag)
}

/// Mapeia a página `i_page` da memória compartilhada; `b_extend` pede extensão do arquivo.
pub fn os_shm_map(
    id: &mut Sqlite3File,
    i_page: i32,
    pgsz: i32,
    b_extend: i32,
    pp: &mut Option<std::rc::Rc<std::cell::RefCell<Vec<u8>>>>,
) -> i32 {
    id.p_methods.as_mut().unwrap().x_shm_map(i_page, pgsz, b_extend, pp)
}

/// Implementação real de xFetch (`SQLITE_MAX_MMAP_SIZE` é maior que zero no Debian).
pub fn os_fetch(id: &mut Sqlite3File, i_off: i64, i_amt: i32, pp: &mut Option<Vec<u8>>) -> i32 {
    id.p_methods.as_mut().unwrap().x_fetch(i_off, i_amt, pp)
}

/// Implementação real de xUnfetch.
pub fn os_unfetch(id: &mut Sqlite3File, i_off: i64) -> i32 {
    id.p_methods.as_mut().unwrap().x_unfetch(i_off)
}

/// Abre um arquivo pelo VFS. Só os flags de `0x1087f7f` descem até o VFS; outros
/// (como `SQLITE_OPEN_FULLMUTEX` e `SQLITE_OPEN_SHAREDCACHE`) são bloqueados aqui.
pub fn os_open(
    p_vfs: &dyn Vfs,
    z_path: Option<&[u8]>,
    p_file: &mut Sqlite3File,
    flags: i32,
    p_flags_out: Option<&mut i32>,
) -> i32 {
    p_vfs.x_open(z_path, p_file, flags & 0x1087f7f, p_flags_out)
}

/// Remove um arquivo pelo VFS. VFS sem xDelete devolve `SQLITE_OK`.
pub fn os_delete(p_vfs: &dyn Vfs, z_path: &[u8], dir_sync: i32) -> i32 {
    match p_vfs.x_delete(z_path, dir_sync) {
        Some(rc) => rc,
        None => SQLITE_OK,
    }
}

/// Verifica a existência ou a acessibilidade de um arquivo; resultado em `p_res_out`.
pub fn os_access(p_vfs: &dyn Vfs, z_path: &[u8], flags: i32, p_res_out: &mut i32) -> i32 {
    p_vfs.x_access(z_path, flags, p_res_out)
}

/// Caminho absoluto de `z_path` em `z_path_out` (o tamanho do buffer é `n_path_out`).
pub fn os_full_pathname(p_vfs: &dyn Vfs, z_path: &[u8], z_path_out: &mut [u8]) -> i32 {
    if !z_path_out.is_empty() {
        z_path_out[0] = 0;
    }
    p_vfs.x_full_pathname(z_path, z_path_out)
}

/// Abre uma biblioteca dinâmica (`SQLITE_OMIT_LOAD_EXTENSION` não está definido).
pub fn os_dl_open(p_vfs: &dyn Vfs, z_path: &[u8]) -> Option<usize> {
    p_vfs.x_dl_open(z_path)
}

/// Mensagem de erro da última operação de biblioteca dinâmica.
pub fn os_dl_error(p_vfs: &dyn Vfs, z_buf_out: &mut [u8]) {
    p_vfs.x_dl_error(z_buf_out);
}

/// Endereço do símbolo `z_sym` na biblioteca `p_hdle`.
pub fn os_dl_sym(p_vfs: &dyn Vfs, p_hdle: usize, z_sym: &[u8]) -> Option<DlSymFn> {
    p_vfs.x_dl_sym(p_hdle, z_sym)
}

/// Fecha uma biblioteca dinâmica.
pub fn os_dl_close(p_vfs: &dyn Vfs, p_handle: usize) {
    p_vfs.x_dl_close(p_handle);
}

/// Preenche `z_buf_out` com bytes aleatórios; com `iPrngSeed` configurado usa a semente.
pub fn os_randomness(p_vfs: &dyn Vfs, z_buf_out: &mut [u8]) -> i32 {
    let seed = sqlite3_config().i_prng_seed;
    if seed != 0 {
        z_buf_out.fill(0);
        let n = z_buf_out.len().min(std::mem::size_of::<u32>());
        z_buf_out[..n].copy_from_slice(&seed.to_ne_bytes()[..n]);
        SQLITE_OK
    } else {
        p_vfs.x_randomness(z_buf_out)
    }
}

/// Dorme `n_micro` microssegundos.
pub fn os_sleep(p_vfs: &dyn Vfs, n_micro: i32) -> i32 {
    p_vfs.x_sleep(n_micro)
}

/// Último erro do sistema operacional; zero se o VFS não implementa xGetLastError.
pub fn os_get_last_error(p_vfs: &dyn Vfs) -> i32 {
    match p_vfs.x_get_last_error() {
        Some(rc) => rc,
        None => 0,
    }
}

/// Data e hora atuais em milissegundos julianos. Usa xCurrentTimeInt64 quando a versão
/// é 2 ou maior e o método existe; senão converte o resultado de xCurrentTime.
pub fn os_current_time_int64(p_vfs: &dyn Vfs, p_time_out: &mut i64) -> i32 {
    if p_vfs.i_version() >= 2 {
        if let Some(rc) = p_vfs.x_current_time_int64(p_time_out) {
            return rc;
        }
    }
    let mut r: f64 = 0.0;
    let rc = p_vfs.x_current_time(&mut r);
    *p_time_out = (r * 86400000.0) as i64;
    rc
}

/// Aloca um arquivo zerado e o abre. Em erro, `pp_file` fica `None`.
pub fn os_open_malloc(
    p_vfs: &dyn Vfs,
    z_file: Option<&[u8]>,
    pp_file: &mut Option<Box<Sqlite3File>>,
    flags: i32,
    p_out_flags: Option<&mut i32>,
) -> i32 {
    let mut p_file = Box::new(Sqlite3File::default());
    let rc = os_open(p_vfs, z_file, &mut p_file, flags, p_out_flags);
    if rc != SQLITE_OK {
        *pp_file = None;
    } else {
        *pp_file = Some(p_file);
    }
    rc
}

/// Fecha o arquivo e libera a memória.
pub fn os_close_free(mut p_file: Box<Sqlite3File>) {
    os_close(&mut p_file);
}

/// Inicializa a camada de OS: testa uma alocação e chama `sqlite3_os_init()`.
pub fn os_init() -> i32 {
    let p = malloc(10);
    if p.is_none() {
        return SQLITE_NOMEM_BKPT;
    }
    drop(p);
    crate::os_unix::os_init()
}

/// Localiza um VFS pelo nome; sem nome devolve o primeiro da lista.
pub fn vfs_find(z_vfs: Option<&[u8]>) -> Option<VfsRef> {
    let rc = api::initialize();
    if rc != 0 {
        return None;
    }
    let mutex = mutex_alloc(SQLITE_MUTEX_STATIC_MAIN);
    mutex_enter(mutex.as_ref());
    let mut p_vfs = VFS_LIST.with(|l| l.borrow().clone());
    while let Some(v) = p_vfs.clone() {
        match z_vfs {
            None => break,
            Some(z) => {
                if z == v.z_name() {
                    break;
                }
            }
        }
        p_vfs = v.p_next();
    }
    mutex_leave(mutex.as_ref());
    p_vfs
}


// ---- part_001.rs ----

/// Remove um VFS da lista encadeada. O chamador mantém o mutex `SQLITE_MUTEX_STATIC_MAIN`.
fn vfs_unlink(p_vfs: Option<&VfsRef>) {
    let Some(p_vfs) = p_vfs else {
        // Sem operação
        return;
    };
    let head = VFS_LIST.with(|l| l.borrow().clone());
    let Some(head) = head else {
        return;
    };
    if std::rc::Rc::ptr_eq(&head, p_vfs) {
        VFS_LIST.with(|l| *l.borrow_mut() = head.p_next());
    } else {
        let mut p = head;
        while let Some(next) = p.p_next() {
            if std::rc::Rc::ptr_eq(&next, p_vfs) {
                break;
            }
            p = next;
        }
        if let Some(next) = p.p_next() {
            if std::rc::Rc::ptr_eq(&next, p_vfs) {
                p.set_p_next(p_vfs.p_next());
            }
        }
    }
}

/// Registra um VFS. Registrar o mesmo VFS várias vezes é inofensivo. O novo VFS vira
/// o padrão se `make_dflt` for verdadeiro ou se a lista estiver vazia.
pub fn vfs_register(p_vfs: &VfsRef, make_dflt: i32) -> i32 {
    let rc = api::initialize();
    if rc != 0 {
        return rc;
    }
    let mutex = mutex_alloc(SQLITE_MUTEX_STATIC_MAIN);
    mutex_enter(mutex.as_ref());
    vfs_unlink(Some(p_vfs));
    let head = VFS_LIST.with(|l| l.borrow().clone());
    match head {
        Some(first) if make_dflt == 0 => {
            p_vfs.set_p_next(first.p_next());
            first.set_p_next(Some(p_vfs.clone()));
        }
        head => {
            p_vfs.set_p_next(head);
            VFS_LIST.with(|l| *l.borrow_mut() = Some(p_vfs.clone()));
        }
    }
    mutex_leave(mutex.as_ref());
    SQLITE_OK
}

/// Remove o registro de um VFS para que deixe de ser acessível.
pub fn vfs_unregister(p_vfs: Option<&VfsRef>) -> i32 {
    let rc = api::initialize();
    if rc != 0 {
        return rc;
    }
    let mutex = mutex_alloc(SQLITE_MUTEX_STATIC_MAIN);
    mutex_enter(mutex.as_ref());
    vfs_unlink(p_vfs);
    mutex_leave(mutex.as_ref());
    SQLITE_OK
}

