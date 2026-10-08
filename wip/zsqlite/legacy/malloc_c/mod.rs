// Mesclado das partes traduzidas de malloc_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

// Modelo de memória deste módulo (decisões que o integrador precisa conhecer):
//
// - Bloco de heap do alocador global é `Vec<u8>` com `len == x_size(bloco)`
//   (o tamanho já arredondado por `x_roundup`). Falha de alocação é `None`.
// - Bloco de `db_malloc_*` é `DbBlock` (dados + origem: heap, lookaside grande
//   ou lookaside pequeno). A origem substitui as comparações de endereço do C
//   (`p<pEnd`, `p>=pMiddle`, `p>=pStart`), que não existem sem ponteiros.
//   Os campos `p_start`, `p_middle`, `p_end`, `p_true_end` de `Lookaside` somem;
//   `p_free`, `p_small_free`, `p_init`, `p_small_init` viram `Vec<usize>` usados
//   como pilha de índices de slot (o topo, `pop()`, é a cabeça da lista do C;
//   `p_init` e `p_small_init` ficam em ordem inversa para o `pop()` devolver o
//   slot de menor endereço primeiro, como a cadeia inicial do C).
// - `db.p_n_bytes_freed` é `Option<i32>` (o contador fica dentro do `Sqlite3`).
// - `global_config()` devolve uma cópia (snapshot) de `Sqlite3Config`;
//   `with_global_config_mut(|c| ...)` altera a configuração global.
//   `c.m` é `Option<Arc<dyn MemMethods>>` (`None` equivale a `xMalloc==0`).
// - O mutex `mem0.mutex` do C é um `std::sync::Mutex` em `MEM0`. O campo `mutex`
//   guarda apenas o `MutexRef` devolvido por `mutex_alloc`, para `malloc_mutex`.

/// Tamanho máximo de qualquer alocação individual (0x7ffffeff).
pub const SQLITE_MAX_ALLOCATION_SIZE: u64 = 2147483391;

/// Valor padrão do limite rígido de heap. Zero significa "sem limite".
pub const SQLITE_MAX_MEMORY: i64 = 0;

/// Interface do alocador de memória global (`sqlite3_mem_methods`).
pub trait MemMethods: Send + Sync {
    /// Aloca `n` bytes; `n` já vem arredondado por `x_roundup`.
    fn x_malloc(&self, n: i32) -> Option<Vec<u8>>;
    /// Libera o bloco.
    fn x_free(&self, p: Vec<u8>);
    /// Redimensiona. Em falha devolve o bloco antigo intacto em `Err`.
    fn x_realloc(&self, p: Vec<u8>, n: i32) -> Result<Vec<u8>, Vec<u8>>;
    /// Tamanho do bloco (já arredondado).
    fn x_size(&self, p: &[u8]) -> i32;
    /// Arredonda o tamanho pedido.
    fn x_roundup(&self, n: i32) -> i32;
    /// Inicializa o alocador.
    fn x_init(&self) -> i32;
    /// Encerra o alocador.
    fn x_shutdown(&self);
}

/// Estado local do subsistema de alocação de memória.
pub struct Mem0Global {
    /// Mutex estático de alocação (só para `malloc_mutex`).
    pub mutex: Option<MutexRef>,
    /// O limite flexível (soft) de heap.
    pub alarm_threshold: i64,
    /// O limite rígido (hard) de heap.
    pub hard_limit: i64,
}

impl Mem0Global {
    /// Equivalente ao `memset(&mem0, 0, sizeof(mem0))`.
    pub const fn zeroed() -> Mem0Global {
        Mem0Global { mutex: None, alarm_threshold: 0, hard_limit: 0 }
    }
}

/// `mem0` do C, serializado pelo mutex do Rust.
pub static MEM0: Mutex<Mem0Global> = Mutex::new(Mem0Global {
    mutex: None,
    alarm_threshold: SQLITE_MAX_MEMORY,
    hard_limit: SQLITE_MAX_MEMORY,
});

/// `mem0.nearlyFull`: verdadeiro se o heap está quase "cheio" conforme o
/// `soft_heap_limit`. Fica fora do mutex porque o C usa `AtomicLoad`/`AtomicStore`.
pub static MEM0_NEARLY_FULL: AtomicI32 = AtomicI32::new(0);

/// Pega o lock de `mem0` (envenenamento é ignorado, como não existe no C).
pub fn mem0_lock() -> MutexGuard<'static, Mem0Global> {
    MEM0.lock().unwrap_or_else(|e| e.into_inner())
}

/// Alocador global configurado (`sqlite3GlobalConfig.m`).
pub fn mem_methods() -> Arc<dyn MemMethods> {
    global_config().m.expect("alocador de memória não inicializado")
}

/// Origem de um bloco de `db_malloc_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DbOrigin {
    /// Veio do alocador global.
    Heap,
    /// Slot grande do lookaside (`lookaside.sz_true` bytes).
    LookasideLarge(usize),
    /// Slot pequeno do lookaside (`LOOKASIDE_SMALL` bytes).
    LookasideSmall(usize),
}

/// Bloco devolvido por `db_malloc_*`: dados e origem.
pub struct DbBlock {
    /// Conteúdo do bloco. Em bloco de heap, `data.len()` é o tamanho de `x_size`.
    pub data: Vec<u8>,
    /// De onde o bloco veio.
    pub origin: DbOrigin,
}

impl DbBlock {
    /// Bloco originado do alocador global.
    pub fn heap(data: Vec<u8>) -> DbBlock {
        DbBlock { data, origin: DbOrigin::Heap }
    }

    /// Visão do bloco como string C: bytes até o primeiro NUL.
    pub fn cstr(&self) -> &[u8] {
        let n = self.data.iter().position(|&c| c == 0).unwrap_or(self.data.len());
        &self.data[..n]
    }
}

/// Tenta liberar até `n` bytes de memória não essencial. Sem
/// `SQLITE_ENABLE_MEMORY_MANAGEMENT` é uma operação nula que devolve zero.
pub fn api_release_memory(n: i32) -> i32 {
    let _ = n;
    0
}

/// Retorna o mutex do alocador de memória (`sqlite3_status` precisa dele).
pub fn malloc_mutex() -> Option<MutexRef> {
    mem0_lock().mutex.clone()
}

/// Interface obsoleta que definia um alarme de memória. Agora não faz nada.
pub fn api_memory_alarm(
    x_callback: Option<&dyn Fn(i64, i32)>,
    i_threshold: i64,
) -> i32 {
    let _ = x_callback;
    let _ = i_threshold;
    SQLITE_OK
}

/// Define o limite flexível de heap. Zero desativa o limite; negativo apenas
/// consulta. Devolve o valor anterior. Com limite rígido ativo, o limite
/// flexível não pode ser desativado nem passar do rígido.
pub fn api_soft_heap_limit64(n: i64) -> i64 {
    let mut n = n;
    if api::initialize() != 0 {
        return -1;
    }
    let mut g = mem0_lock();
    let prior_limit = g.alarm_threshold;
    if n < 0 {
        return prior_limit;
    }
    if g.hard_limit > 0 && (n > g.hard_limit || n == 0) {
        n = g.hard_limit;
    }
    g.alarm_threshold = n;
    let n_used = status_value(SQLITE_STATUS_MEMORY_USED);
    MEM0_NEARLY_FULL.store((n > 0 && n <= n_used) as i32, Ordering::SeqCst);
    drop(g);
    let excess = api_memory_used() - n;
    if excess > 0 {
        api_release_memory((excess & 0x7fffffff) as i32);
    }
    prior_limit
}

/// Versão antiga (32 bits) de `api_soft_heap_limit64`.
pub fn api_soft_heap_limit(n: i32) {
    let n = if n < 0 { 0 } else { n };
    api_soft_heap_limit64(n as i64);
}

/// Define o limite rígido de heap. Zero desativa; negativo apenas consulta.
/// Devolve o valor anterior. Ativar o limite rígido também ativa o flexível
/// e o restringe a no máximo o rígido.
pub fn api_hard_heap_limit64(n: i64) -> i64 {
    if api::initialize() != 0 {
        return -1;
    }
    let mut g = mem0_lock();
    let prior_limit = g.hard_limit;
    if n >= 0 {
        g.hard_limit = n;
        if n < g.alarm_threshold || g.alarm_threshold == 0 {
            g.alarm_threshold = n;
        }
    }
    prior_limit
}

/// Inicializa o subsistema de alocação de memória.
pub fn malloc_init() -> i32 {
    if global_config().m.is_none() {
        mem_set_default();
    }
    let mutex = mutex_alloc(SQLITE_MUTEX_STATIC_MEM);
    mem0_lock().mutex = mutex;
    with_global_config_mut(|c| {
        if c.p_page.is_none() || c.sz_page < 512 || c.n_page <= 0 {
            c.p_page = None;
            c.sz_page = 0;
        }
    });
    let rc = mem_methods().x_init();
    if rc != SQLITE_OK {
        *mem0_lock() = Mem0Global::zeroed();
        MEM0_NEARLY_FULL.store(0, Ordering::SeqCst);
    }
    rc
}

/// Verdadeiro se o heap está sob pressão de memória, isto é, se o uso está
/// perto do limite definido por `soft_heap_limit`.
pub fn heap_nearly_full() -> i32 {
    MEM0_NEARLY_FULL.load(Ordering::SeqCst)
}

/// Desinicializa o subsistema de alocação de memória.
pub fn malloc_end() {
    if let Some(m) = global_config().m {
        m.x_shutdown();
    }
    *mem0_lock() = Mem0Global::zeroed();
    MEM0_NEARLY_FULL.store(0, Ordering::SeqCst);
}

/// Quantidade de memória atualmente em uso.
pub fn api_memory_used() -> i64 {
    let mut res: i64 = 0;
    let mut mx: i64 = 0;
    api::status64(SQLITE_STATUS_MEMORY_USED, &mut res, &mut mx, 0);
    res
}

/// Máximo de memória já em uso desde o início do processo ou do último reset.
pub fn api_memory_highwater(reset_flag: i32) -> i64 {
    let mut res: i64 = 0;
    let mut mx: i64 = 0;
    api::status64(SQLITE_STATUS_MEMORY_USED, &mut res, &mut mx, reset_flag);
    mx
}

/// Dispara o alarme. O lock de `mem0` é solto durante `release_memory` e
/// retomado depois, como o `mutex_leave`/`mutex_enter` do C.
pub fn malloc_alarm(g: MutexGuard<'static, Mem0Global>, n_byte: i32) -> MutexGuard<'static, Mem0Global> {
    if g.alarm_threshold <= 0 {
        return g;
    }
    drop(g);
    api_release_memory(n_byte);
    mem0_lock()
}

/// Aloca memória com estatísticas e alarmes. O lock de `mem0` já está preso;
/// devolve o lock (possivelmente retomado) junto com o resultado.
pub fn malloc_with_alarm(
    g: MutexGuard<'static, Mem0Global>,
    n: i32,
) -> (MutexGuard<'static, Mem0Global>, Option<Vec<u8>>) {
    let mut g = g;
    let m = mem_methods();
    debug_assert!(n > 0);

    // O `x_roundup` precisa ser chamado mesmo que o resultado pudesse ser
    // otimizado fora (o Firefox depende disso).
    let mut n_full = m.x_roundup(n);

    status_highwater(SQLITE_STATUS_MALLOC_SIZE, n);
    if g.alarm_threshold > 0 {
        let mut n_used = status_value(SQLITE_STATUS_MEMORY_USED);
        if n_used >= g.alarm_threshold - n_full as i64 {
            MEM0_NEARLY_FULL.store(1, Ordering::SeqCst);
            g = malloc_alarm(g, n_full);
            if g.hard_limit != 0 {
                n_used = status_value(SQLITE_STATUS_MEMORY_USED);
                if n_used >= g.hard_limit - n_full as i64 {
                    return (g, None);
                }
            }
        } else {
            MEM0_NEARLY_FULL.store(0, Ordering::SeqCst);
        }
    }
    let p = m.x_malloc(n_full);
    if let Some(p) = &p {
        n_full = malloc_size(p);
        status_up(SQLITE_STATUS_MEMORY_USED, n_full);
        status_up(SQLITE_STATUS_MALLOC_COUNT, 1);
    }
    (g, p)
}

/// Aloca memória. Como `api_malloc`, mas supõe o subsistema já inicializado.
pub fn malloc(n: u64) -> Option<Vec<u8>> {
    if n == 0 || n > SQLITE_MAX_ALLOCATION_SIZE {
        None
    } else if global_config().b_memstat != 0 {
        let g = mem0_lock();
        let (_g, p) = malloc_with_alarm(g, n as i32);
        p
    } else {
        mem_methods().x_malloc(n as i32)
    }
}

/// Versão da alocação para uso da aplicação: garante a inicialização antes.
pub fn api_malloc(n: i32) -> Option<Vec<u8>> {
    if api::initialize() != 0 {
        return None;
    }
    if n <= 0 {
        None
    } else {
        malloc(n as u64)
    }
}

/// Versão de 64 bits de `api_malloc`.
pub fn api_malloc64(n: u64) -> Option<Vec<u8>> {
    if api::initialize() != 0 {
        return None;
    }
    malloc(n)
}

/// Verdadeiro se o bloco `p` é uma alocação de lookaside.
pub fn is_lookaside(p: &DbBlock) -> bool {
    p.origin != DbOrigin::Heap
}


// ---- part_001.rs ----

/// Resultado de um redimensionamento. `Ok` traz o bloco novo. `Err(Some(velho))`
/// é a falha do C em que o ponteiro antigo continua válido; `Err(None)` é o
/// retorno NULL sem bloco antigo a preservar (bloco liberado por tamanho zero,
/// ou ponteiro antigo nulo com falha de alocação).
pub type ReallocResult<T> = Result<T, Option<T>>;

/// Tamanho de uma alocação obtida de `malloc` ou `api_malloc`.
pub fn malloc_size(p: &[u8]) -> i32 {
    mem_methods().x_size(p)
}

/// Tamanho do slot de lookaside de um bloco (grade de dois tamanhos).
pub fn lookaside_malloc_size(db: &Sqlite3, p: &DbBlock) -> i32 {
    match p.origin {
        DbOrigin::LookasideSmall(_) => LOOKASIDE_SMALL as i32,
        _ => db.lookaside.sz_true as i32,
    }
}

/// Tamanho de uma alocação obtida de `db_malloc_*` (ou do heap global, com
/// `db` ausente).
pub fn db_malloc_size(db: Option<&Sqlite3>, p: &DbBlock) -> i32 {
    if let Some(db) = db {
        match p.origin {
            DbOrigin::LookasideSmall(_) => return LOOKASIDE_SMALL as i32,
            DbOrigin::LookasideLarge(_) => return db.lookaside.sz_true as i32,
            DbOrigin::Heap => {}
        }
    }
    mem_methods().x_size(&p.data)
}

/// Tamanho de uma alocação de heap (`sqlite3_msize`); `None` equivale a NULL.
pub fn api_msize(p: Option<&[u8]>) -> u64 {
    match p {
        Some(p) => mem_methods().x_size(p) as u64,
        None => 0,
    }
}

/// Libera memória obtida de `malloc`. Chamar com `None` não faz nada.
pub fn api_free(p: Option<Vec<u8>>) {
    let p = match p {
        Some(p) => p,
        None => return,
    };
    let m = mem_methods();
    if global_config().b_memstat != 0 {
        let _g = mem0_lock();
        status_down(SQLITE_STATUS_MEMORY_USED, malloc_size(&p));
        status_down(SQLITE_STATUS_MALLOC_COUNT, 1);
        m.x_free(p);
    } else {
        m.x_free(p);
    }
}

/// Soma o tamanho da alocação `p` ao contador `db.p_n_bytes_freed`.
pub fn measure_allocation_size(db: &mut Sqlite3, p: &DbBlock) {
    let n = db_malloc_size(Some(&*db), p);
    if let Some(c) = db.p_n_bytes_freed.as_mut() {
        *c += n;
    }
}

/// Libera memória associada à conexão `db` (que não pode ser nula aqui).
pub fn db_nn_free_nn(db: &mut Sqlite3, p: DbBlock) {
    match p.origin {
        DbOrigin::LookasideSmall(slot) => {
            debug_assert!(db.p_n_bytes_freed.is_none());
            db.lookaside.p_small_free.push(slot);
            return;
        }
        DbOrigin::LookasideLarge(slot) => {
            debug_assert!(db.p_n_bytes_freed.is_none());
            db.lookaside.p_free.push(slot);
            return;
        }
        DbOrigin::Heap => {}
    }
    if db.p_n_bytes_freed.is_some() {
        measure_allocation_size(db, &p);
        return;
    }
    api_free(Some(p.data));
}

/// Libera memória possivelmente associada a uma conexão. `db` pode ser nulo.
pub fn db_free_nn(db: Option<&mut Sqlite3>, p: DbBlock) {
    match db {
        Some(db) => db_nn_free_nn(db, p),
        None => api_free(Some(p.data)),
    }
}

/// Libera memória possivelmente associada a uma conexão. Chamar com `p`
/// ausente não faz nada.
pub fn db_free(db: Option<&mut Sqlite3>, p: Option<DbBlock>) {
    if let Some(p) = p {
        db_free_nn(db, p);
    }
}

/// Muda o tamanho de uma alocação existente. Em falha o bloco antigo volta
/// em `Err` (ver `ReallocResult`).
pub fn realloc(p_old: Option<Vec<u8>>, n_bytes: u64) -> ReallocResult<Vec<u8>> {
    let p_old = match p_old {
        None => return malloc(n_bytes).ok_or(None),
        Some(p) => p,
    };
    if n_bytes == 0 {
        api_free(Some(p_old));
        return Err(None);
    }
    if n_bytes >= 0x7fffff00 {
        // O limite 0x7fffff00 é explicado nos comentários de `malloc`.
        return Err(Some(p_old));
    }
    let m = mem_methods();
    let n_old = malloc_size(&p_old);
    // O segundo argumento de `x_realloc` é sempre um valor devolvido antes
    // por `x_roundup`.
    let n_new = m.x_roundup(n_bytes as i32);
    if n_old == n_new {
        return Ok(p_old);
    }
    if global_config().b_memstat != 0 {
        let mut g = mem0_lock();
        status_highwater(SQLITE_STATUS_MALLOC_SIZE, n_bytes as i32);
        let n_diff = n_new - n_old;
        if n_diff > 0 {
            let n_used = status_value(SQLITE_STATUS_MEMORY_USED);
            if n_used >= g.alarm_threshold - n_diff as i64 {
                g = malloc_alarm(g, n_diff);
                if g.hard_limit > 0 && n_used >= g.hard_limit - n_diff as i64 {
                    return Err(Some(p_old));
                }
            }
        }
        match m.x_realloc(p_old, n_new) {
            Ok(p_new) => {
                let n_new = malloc_size(&p_new);
                status_up(SQLITE_STATUS_MEMORY_USED, n_new - n_old);
                Ok(p_new)
            }
            Err(p_old) => Err(Some(p_old)),
        }
    } else {
        m.x_realloc(p_old, n_new).map_err(Some)
    }
}

/// Interface pública de `realloc`: garante a inicialização do subsistema.
pub fn api_realloc64(p_old: Option<Vec<u8>>, n: u64) -> ReallocResult<Vec<u8>> {
    if api::initialize() != 0 {
        return Err(p_old);
    }
    realloc(p_old, n)
}

/// Versão de 32 bits de `api_realloc64`: tamanho negativo vale zero.
pub fn api_realloc(p_old: Option<Vec<u8>>, n: i32) -> ReallocResult<Vec<u8>> {
    if api::initialize() != 0 {
        return Err(p_old);
    }
    let n = if n < 0 { 0 } else { n };
    realloc(p_old, n as u64)
}

/// Aloca memória zerada.
pub fn malloc_zero(n: u64) -> Option<Vec<u8>> {
    let mut p = malloc(n);
    if let Some(p) = p.as_mut() {
        p[..n as usize].fill(0);
    }
    p
}

/// Aloca memória zerada. Se a alocação falhar, marca `malloc_failed` na
/// conexão (via `db_malloc_raw`).
pub fn db_malloc_zero(db: Option<&mut Sqlite3>, n: u64) -> Option<DbBlock> {
    let mut p = db_malloc_raw(db, n);
    if let Some(p) = p.as_mut() {
        p.data[..n as usize].fill(0);
    }
    p
}

/// Termina o trabalho de `db_malloc_raw_nn` no caso incomum e mais lento em
/// que a alocação não pode sair do lookaside.
pub fn db_malloc_raw_finish(db: &mut Sqlite3, n: u64) -> Option<DbBlock> {
    let p = malloc(n);
    if p.is_none() {
        oom_fault(db);
    }
    p.map(DbBlock::heap)
}

/// Aloca memória, do lookaside se possível, senão do heap. Em falha marca
/// `malloc_failed` na conexão.
///
/// Se `db` existe e `db.malloc_failed` é verdadeiro (falha anterior na mesma
/// conexão), devolve sempre `None`. Assim, para uma conexão, depois que o
/// malloc começa a falhar ele falha de forma consistente até `malloc_failed`
/// ser zerado. Muito código depende disso: se um malloc posterior funcionou,
/// os anteriores também funcionaram.
///
/// A variante `db_malloc_raw_nn` garante que `db` não é nulo.
pub fn db_malloc_raw(db: Option<&mut Sqlite3>, n: u64) -> Option<DbBlock> {
    match db {
        Some(db) => db_malloc_raw_nn(db, n),
        None => malloc(n).map(DbBlock::heap),
    }
}

/// Retira um slot do lookaside: primeiro da lista livre, depois da inicial.
/// Conta o acerto em `an_stat[0]`.
pub fn lookaside_take(free: &mut Vec<usize>, init: &mut Vec<usize>, an_stat0: &mut u32) -> Option<usize> {
    let slot = free.pop().or_else(|| init.pop());
    if slot.is_some() {
        *an_stat0 += 1;
    }
    slot
}

/// Aloca memória (lookaside ou heap) com `db` garantidamente não nulo.
pub fn db_malloc_raw_nn(db: &mut Sqlite3, n: u64) -> Option<DbBlock> {
    debug_assert!(db.p_n_bytes_freed.is_none());
    if n > db.lookaside.sz as u64 {
        if db.lookaside.b_disable == 0 {
            db.lookaside.an_stat[1] += 1;
        } else if db.malloc_failed != 0 {
            return None;
        }
        return db_malloc_raw_finish(db, n);
    }
    if n <= LOOKASIDE_SMALL as u64 {
        let lk = &mut db.lookaside;
        if let Some(slot) = lookaside_take(&mut lk.p_small_free, &mut lk.p_small_init, &mut lk.an_stat[0]) {
            return Some(DbBlock {
                data: vec![0u8; LOOKASIDE_SMALL as usize],
                origin: DbOrigin::LookasideSmall(slot),
            });
        }
    }
    let sz_true = db.lookaside.sz_true as usize;
    let lk = &mut db.lookaside;
    if let Some(slot) = lookaside_take(&mut lk.p_free, &mut lk.p_init, &mut lk.an_stat[0]) {
        return Some(DbBlock { data: vec![0u8; sz_true], origin: DbOrigin::LookasideLarge(slot) });
    }
    db.lookaside.an_stat[2] += 1;
    db_malloc_raw_finish(db, n)
}


// ---- part_002.rs ----
use std::sync::atomic::Ordering;

/// Redimensiona o bloco `p` para `n` bytes. Se falhar, marca `malloc_failed`
/// na conexão. Em falha o bloco antigo volta em `Err` (ver `ReallocResult`).
pub fn db_realloc(db: &mut Sqlite3, p: Option<DbBlock>, n: u64) -> ReallocResult<DbBlock> {
    let p = match p {
        None => return db_malloc_raw_nn(db, n).ok_or(None),
        Some(p) => p,
    };
    match p.origin {
        DbOrigin::LookasideSmall(_) => {
            if n <= LOOKASIDE_SMALL as u64 {
                return Ok(p);
            }
        }
        DbOrigin::LookasideLarge(_) => {
            if n <= db.lookaside.sz_true as u64 {
                return Ok(p);
            }
        }
        DbOrigin::Heap => {}
    }
    db_realloc_finish(db, p, n)
}

/// Parte lenta de `db_realloc`.
pub fn db_realloc_finish(db: &mut Sqlite3, p: DbBlock, n: u64) -> ReallocResult<DbBlock> {
    if db.malloc_failed != 0 {
        return Err(Some(p));
    }
    if is_lookaside(&p) {
        match db_malloc_raw_nn(db, n) {
            Some(mut p_new) => {
                let sz = lookaside_malloc_size(db, &p) as usize;
                p_new.data[..sz].copy_from_slice(&p.data[..sz]);
                db_nn_free_nn(db, p);
                Ok(p_new)
            }
            None => Err(Some(p)),
        }
    } else {
        match realloc(Some(p.data), n) {
            Ok(data) => Ok(DbBlock::heap(data)),
            Err(old) => {
                oom_fault(db);
                Err(old.map(DbBlock::heap))
            }
        }
    }
}

/// Tenta redimensionar `p`. Se falhar, libera `p` e marca `malloc_failed`.
pub fn db_realloc_or_free(db: &mut Sqlite3, p: Option<DbBlock>, n: u64) -> Option<DbBlock> {
    match db_realloc(db, p, n) {
        Ok(p_new) => Some(p_new),
        Err(old) => {
            db_free(Some(db), old);
            None
        }
    }
}

/// Copia uma string C (bytes até o primeiro NUL, mais o terminador) para
/// memória obtida de `db_malloc_raw`. `z` ausente devolve `None`.
pub fn db_str_dup(db: Option<&mut Sqlite3>, z: Option<&[u8]>) -> Option<DbBlock> {
    let z = z?;
    let len = z.iter().position(|&c| c == 0).unwrap_or(z.len());
    let n = len + 1;
    let mut z_new = db_malloc_raw(db, n as u64)?;
    z_new.data[..len].copy_from_slice(&z[..len]);
    z_new.data[len] = 0;
    Some(z_new)
}

/// Copia `n` bytes de `z` mais um NUL para memória obtida de `db_malloc_raw_nn`.
pub fn db_str_n_dup(db: &mut Sqlite3, z: Option<&[u8]>, n: u64) -> Option<DbBlock> {
    debug_assert!(z.is_some() || n == 0);
    debug_assert!((n & 0x7fffffff) == n);
    let z = z?;
    let mut z_new = db_malloc_raw_nn(db, n + 1)?;
    let n = n as usize;
    z_new.data[..n].copy_from_slice(&z[..n]);
    z_new.data[n] = 0;
    Some(z_new)
}

/// O texto de `z[start..end]` é uma frase dentro de uma instrução SQL maior.
/// Copia a frase para memória de `db_malloc`, omitindo espaços no início e no fim.
pub fn db_span_dup(db: &mut Sqlite3, z: &[u8], start: usize, end: usize) -> Option<DbBlock> {
    // Pela forma como o parser funciona, o trecho tem ao menos um caractere
    // que não é espaço.
    let mut start = start;
    while isspace(z[start]) {
        start += 1;
    }
    let mut n = end - start;
    while isspace(z[start + n - 1]) {
        n -= 1;
    }
    db_str_n_dup(db, Some(&z[start..]), n as u64)
}

/// Libera o conteúdo anterior de `pz` e o substitui por uma cópia de `z_new`.
pub fn set_string(pz: &mut Option<DbBlock>, db: Option<&mut Sqlite3>, z_new: Option<&[u8]>) {
    let mut db = db;
    let z = db_str_dup(db.as_deref_mut(), z_new);
    db_free(db, pz.take());
    *pz = z;
}

/// Registra que ocorreu um erro de falta de memória (OOM). Marca
/// `db.malloc_failed`, desativa temporariamente o lookaside e interrompe os
/// VDBEs em execução. O C devolve sempre NULL para uso em
/// `return sqlite3OomFault(db);`; aqui o chamador devolve `None` por conta própria.
pub fn oom_fault(db: &mut Sqlite3) {
    if db.malloc_failed == 0 && db.b_benign_malloc == 0 {
        db.malloc_failed = 1;
        if db.n_vdbe_exec > 0 {
            db.u1.is_interrupted.store(1, Ordering::SeqCst);
        }
        disable_lookaside(db);
        if let Some(p_parse) = db.p_parse.clone() {
            {
                let mut pp = p_parse.borrow_mut();
                error_msg(db, &mut pp, b"out of memory");
                pp.rc = SQLITE_NOMEM_BKPT;
            }
            let mut outer = p_parse.borrow().p_outer_parse.clone();
            while let Some(o) = outer.take() {
                let mut om = o.borrow_mut();
                om.n_err += 1;
                om.rc = SQLITE_NOMEM;
                outer = om.p_outer_parse.clone();
            }
        }
    }
}

/// Reativa o alocador e limpa `db.malloc_failed` conforme necessário. O
/// alocador não é reiniciado se houver VDBEs em execução.
pub fn oom_clear(db: &mut Sqlite3) {
    if db.malloc_failed != 0 && db.n_vdbe_exec == 0 {
        db.malloc_failed = 0;
        db.u1.is_interrupted.store(0, Ordering::SeqCst);
        debug_assert!(db.lookaside.b_disable > 0);
        enable_lookaside(db);
    }
}

/// Toma ações no fim de uma chamada de API para tratar códigos de erro.
pub fn api_handle_error(db: &mut Sqlite3, rc: i32) -> i32 {
    if db.malloc_failed != 0 || rc == SQLITE_IOERR_NOMEM {
        oom_clear(db);
        error(db, SQLITE_NOMEM);
        return SQLITE_NOMEM_BKPT;
    }
    rc & db.err_mask
}

/// Deve ser chamada antes de sair de qualquer função de API (devolver o
/// controle ao usuário) que tenha chamado `api_malloc` ou `api_realloc`.
///
/// O valor devolvido costuma ser cópia de `rc`. Porém, se houve falha de
/// malloc desde a chamada anterior, devolve `SQLITE_NOMEM`. Se ocorreu OOM, o
/// código de erro da conexão (o de `sqlite3_errcode()`) vira `SQLITE_NOMEM`.
pub fn api_exit(db: &mut Sqlite3, rc: i32) -> i32 {
    // O chamador deve segurar o mutex da conexão: a leitura (e a escrita
    // possível) de `malloc_failed` e a chamada a `error` dependem disso.
    if db.malloc_failed != 0 || rc != 0 {
        return api_handle_error(db, rc);
    }
    0
}

