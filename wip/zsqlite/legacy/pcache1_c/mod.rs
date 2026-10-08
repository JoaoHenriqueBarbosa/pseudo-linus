// Mesclado das partes traduzidas de pcache1_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Modelo de memória do pcache1.c sem ponteiros.
//
// O módulo opera sobre ponteiros `PgHdr1*`, `PCache1*` e `PGroup*` que o resto do
// SQLite só enxerga como handles opacos (`sqlite3_pcache*` e `sqlite3_pcache_page*`
// são `usize` nos tipos de `Sqlite3PcacheMethods2`). Por isso os três tipos moram em
// arenas (`Vec<Option<T>>`) dentro do estado global `PCacheGlobal`, e cada "ponteiro"
// vira um handle `usize` igual ao índice da arena mais um (0 é o ponteiro nulo).
// Os métodos do módulo (`pcache1_fetch`, `pcache1_unpin`, ...) têm a assinatura de
// `fn(usize, ...)` exigida pela tabela de métodos e entram no estado por
// `with_pcache1`. As rotinas internas recebem `s: &mut PCacheGlobal` e nunca
// reentram em `with_pcache1`.
//
// Convenções do modelo, para o integrador:
//  - `PgHdr1.p_extra_hdr` é o `(PgHdr*)pPage->pExtra` do C (o `PgHdr` do pcache.c).
//  - O buffer da página (`pPage->pBuf`) tem sempre `sz_alloc` bytes, como a linha de
//    cache do C: a folga depois do conteúdo é a área de leitura excessiva.
//  - Os buffers do bloco global SQLITE_CONFIG_PAGECACHE são "slots" numerados; a
//    pertinência `SQLITE_WITHIN(p, pStart, pEnd)` vira `slot.is_some()`.
//  - `sqlite3Malloc` falha (devolve nulo) para tamanho <= 0 ou >= 0x7fffff00, e
//    `sqlite3MallocSize` devolve `ROUND8(n)`.

/// Tamanho de `sizeof(PgHdr1)` em x64: 16 (sqlite3_pcache_page) + 4 (iKey) + 2 + 2
/// (isBulkLocal, isAnchor) + 4 ponteiros de 8 bytes.
pub const SIZEOF_PGHDR1: usize = 56;

/// Maior alocação que o `sqlite3Malloc` aceita (acima disso devolve nulo).
const PCACHE1_MAX_ALLOC: i64 = 0x7fff_ff00;

/// Cada entrada do cache é representada por uma instância da seguinte
/// estrutura. Um buffer de `PgHdr1.p_cache.sz_page` bytes é alocado
/// diretamente antes desta estrutura e é usado para guardar o conteúdo da página.
///
/// Ao ler um arquivo de banco corrompido, o SQLite pode ler alguns bytes (no
/// máximo 16) além do fim do buffer da página, mas nunca escrever. Este objeto
/// fica logo depois do buffer da página para servir de área de proteção.
///
/// As variáveis `is_bulk_local` e `is_anchor` já foram `u8`, o que deixava uma
/// lacuna de 2 bytes na estrutura; com `u16` não há bytes não inicializados.
///
/// Os campos `p_lru_next` e `p_lru_prev` formam uma lista circular duplamente
/// ligada de todas as páginas não fixadas. O elemento `PGroup.lru` (o único com
/// `is_anchor` igual a 1) é o início e o fim da lista.
pub struct PgHdr1 {
    /// Classe base. `p_buf` é o conteúdo; `p_extra` fica vazio (o PgHdr vive em `p_extra_hdr`).
    pub page: Sqlite3PcachePage,
    /// Valor da chave (número da página).
    pub i_key: u32,
    /// Esta página vem do armazenamento em bloco local do cache.
    pub is_bulk_local: u16,
    /// Este é o elemento `PGroup.lru`.
    pub is_anchor: u16,
    /// Próximo na cadeia da tabela de hash (handle de página).
    pub p_next: Option<usize>,
    /// Cache que atualmente é dono da página (handle de cache, 0 se nenhum).
    pub p_cache: usize,
    /// Próximo na lista circular LRU de páginas não fixadas.
    pub p_lru_next: Option<usize>,
    /// Anterior na lista LRU. Só vale se `p_lru_next` for `Some`.
    pub p_lru_prev: Option<usize>,
    /// O `PgHdr` guardado em `pExtra` (os 8 primeiros bytes do C são o `pPage` dele).
    pub p_extra_hdr: Option<PgHdrRef>,
    /// Slot do bloco global de onde veio o buffer, se veio de lá.
    pub alloc_slot: Option<usize>,
    /// Tamanho em bytes pedido na alocação do buffer (para as estatísticas de overflow).
    pub alloc_bytes: usize,
}

/// Uma página está fixada se não está na lista LRU. Estar "fixada" significa
/// que a página está em uso ativo e não deve ser desalocada.
#[inline]
pub fn page_is_pinned(p: &PgHdr1) -> bool {
    p.p_lru_next.is_none()
}

/// Uma página está não fixada se está na lista LRU.
#[inline]
pub fn page_is_unpinned(p: &PgHdr1) -> bool {
    p.p_lru_next.is_some()
}

/// Cada cache de página (PCache) pertence a um PGroup. Um PGroup é um conjunto
/// de um ou mais PCaches capazes de reciclar as páginas não fixadas uns dos
/// outros quando há pressão de memória.
///
/// A implementação funciona em um de dois modos:
///
///   (1) cada PCache é o único membro do seu próprio PGroup;
///   (2) há um único PGroup global, do qual todos os PCaches participam.
///
/// O modo 1 usa mais memória, mas opera sem mutex e costuma ser mais rápido. O
/// modo 2 precisa de mutex para ser seguro entre threads, mas recicla melhor.
///
/// No modo (1) `mutex` é `None`. No modo (2) só existe o PGroup global (índice 0
/// da arena), e o mutex dele é SQLITE_MUTEX_STATIC_LRU.
pub struct PGroup {
    /// MUTEX_STATIC_LRU ou `None`.
    pub mutex: Option<Rc<sqlite3_mutex>>,
    /// Soma de `n_max` dos caches purgáveis.
    pub n_max_page: u32,
    /// Soma de `n_min` dos caches purgáveis.
    pub n_min_page: u32,
    /// `n_max_page + 10 - n_min_page`.
    pub mx_pinned: u32,
    /// Número de páginas purgáveis alocadas.
    pub n_purgeable: u32,
    /// Handle do elemento âncora da lista LRU (o `PgHdr1 lru` embutido do C).
    pub lru: usize,
}

/// Cada cache de página é uma instância do seguinte objeto. Cada arquivo de
/// banco aberto (inclusive bancos em memória e temporários) tem um único cache
/// de página, que é uma instância deste objeto. O handle `sqlite3_pcache*` é o
/// índice na arena de caches mais um.
pub struct PCache1 {
    // Parâmetros de configuração do cache. O tamanho de página (`sz_page`), o
    // sinalizador `b_purgeable` e o `pn_purgeable` são definidos na criação e nunca
    // mudam. `n_max` pode mudar a qualquer momento por `pcache1_cachesize()`; o
    // mutex do PGroup precisa estar travado ao acessá-lo.
    /// PGroup a que este cache pertence (índice na arena de grupos).
    pub p_group: usize,
    /// Verdadeiro se `pn_purgeable` aponta para `PGroup.n_purgeable`; falso se
    /// aponta para `n_purgeable_dummy`.
    pub pn_purgeable_in_group: bool,
    /// Tamanho da seção de conteúdo do banco.
    pub sz_page: i32,
    /// `sizeof(MemPage)+sizeof(PgHdr)`.
    pub sz_extra: i32,
    /// Tamanho total de uma linha do cache.
    pub sz_alloc: i32,
    /// Verdadeiro se o cache é purgável.
    pub b_purgeable: i32,
    /// Número mínimo de páginas reservadas.
    pub n_min: u32,
    /// Valor configurado de "cache_size".
    pub n_max: u32,
    /// `n_max*9/10`.
    pub n_90pct: u32,
    /// Maior chave vista desde o último `xTruncate()`.
    pub i_max_key: u32,
    /// `pn_purgeable` aponta para cá quando não é usado.
    pub n_purgeable_dummy: u32,

    // Tabela de hash de todas as páginas. As variáveis seguintes só podem ser
    // acessadas com o mutex do PGroup travado.
    /// Número de páginas na lista LRU.
    pub n_recyclable: u32,
    /// Número total de páginas em `ap_hash`.
    pub n_page: u32,
    /// Número de slots em `ap_hash`.
    pub n_hash: u32,
    /// Tabela de hash para busca rápida por chave (cabeças das cadeias).
    pub ap_hash: Vec<Option<usize>>,
    /// Lista de páginas não usadas do cache local (cabeça da cadeia por `p_next`).
    pub p_free: Option<usize>,
    /// Memória em bloco usada pelo cache local: tamanho em bytes se alocada.
    pub p_bulk: Option<usize>,
}

/// Dados globais usados por este cache (`pcache1_g`). Contém também as arenas
/// que fazem o papel dos ponteiros do C.
pub struct PCacheGlobal {
    /// Arena de grupos. O índice 0 é o PGroup global do modo (2) (`pcache1.grp`).
    pub groups: Vec<Option<PGroup>>,
    /// Arena de caches (handle = índice + 1).
    pub caches: Vec<Option<PCache1>>,
    /// Arena de páginas (handle = índice + 1).
    pub pages: Vec<Option<PgHdr1>>,
    /// Índices livres da arena de páginas, para reaproveitar.
    pub free_page_slots: Vec<usize>,

    // Variáveis ligadas a SQLITE_CONFIG_PAGECACHE. `sz_slot`, `n_slot`,
    // `n_reserve` e `is_init` são fixados em `sqlite3_initialize()` e não precisam
    // de mutex. `n_free_slot` e `p_free` precisam.
    /// Verdadeiro se inicializado.
    pub is_init: i32,
    /// Usar um PGroup novo para cada PCache.
    pub separate_cache: i32,
    /// Tamanho da alocação em bloco inicial.
    pub n_init_page: i32,
    /// Tamanho de cada slot livre.
    pub sz_slot: i32,
    /// Número de slots do cache.
    pub n_slot: i32,
    /// Tenta manter `n_free_slot` acima disto.
    pub n_reserve: i32,
    /// Mutex para acessar as variáveis seguintes.
    pub mutex: Option<Rc<sqlite3_mutex>>,
    /// Pilha de slots livres do bloco global (o topo é a cabeça da lista do C).
    pub p_free: Vec<usize>,
    /// Número de slots do cache não usados.
    pub n_free_slot: i32,
    /// Verdadeiro se falta memória do PAGECACHE. Mudar exige mutex; a leitura
    /// dispensa, porque é só uma otimização.
    pub b_under_pressure: i32,
}

/// Buffer obtido de `pcache1_alloc()` (ou de `page_malloc()`).
pub struct PageAlloc {
    /// Conteúdo do buffer.
    pub buf: Vec<u8>,
    /// Slot do bloco global, se o buffer veio de lá (então não é do heap).
    pub slot: Option<usize>,
    /// Tamanho pedido na alocação.
    pub n_byte: usize,
}

/// Cria um objeto `PgHdr1` sem nada ligado (todos os campos em zero).
pub fn blank_pg_hdr1()-> PgHdr1 {
    PgHdr1 {
        page: Sqlite3PcachePage {
            p_buf: Vec::new(),
            p_extra: Vec::new(),
        },
        i_key: 0,
        is_bulk_local: 0,
        is_anchor: 0,
        p_next: None,
        p_cache: 0,
        p_lru_next: None,
        p_lru_prev: None,
        p_extra_hdr: None,
        alloc_slot: None,
        alloc_bytes: 0,
    }
}

/// Cria um `PgHdr` zerado, o que o `*(void**)pPage->pExtra = 0` do C deixa em `pExtra`.
pub fn new_pg_hdr() -> PgHdrRef {
    Rc::new(RefCell::new(PgHdr {
        p_page: None,
        p_data: Vec::new(),
        p_extra: Vec::new(),
        p_cache: None,
        p_dirty: None,
        p_pager: None,
        pgno: 0,
        flags: 0,
        n_ref: 0,
        p_dirty_next: None,
        p_dirty_prev: None,
    }))
}

/// Cria um grupo zerado com o elemento âncora da LRU já alocado na arena de páginas.
pub fn new_group(pages: &mut Vec<Option<PgHdr1>>, free_slots: &mut Vec<usize>) -> PGroup {
    let lru = store_page(pages, free_slots, blank_pg_hdr1());
    PGroup {
        mutex: None,
        n_max_page: 0,
        n_min_page: 0,
        mx_pinned: 0,
        n_purgeable: 0,
        lru,
    }
}

/// Guarda uma página na arena e devolve o handle.
pub fn store_page(pages: &mut Vec<Option<PgHdr1>>, free_slots: &mut Vec<usize>, p: PgHdr1) -> usize {
    match free_slots.pop() {
        Some(i) => {
            pages[i] = Some(p);
            i + 1
        }
        None => {
            pages.push(Some(p));
            pages.len()
        }
    }
}

impl PCacheGlobal {
    /// Estado inicial, o equivalente do `memset(&pcache1, 0, sizeof(pcache1))`.
    pub fn new() -> PCacheGlobal {
        let mut pages: Vec<Option<PgHdr1>> = Vec::new();
        let mut free_page_slots: Vec<usize> = Vec::new();
        let grp = new_group(&mut pages, &mut free_page_slots);
        PCacheGlobal {
            groups: vec![Some(grp)],
            caches: Vec::new(),
            pages,
            free_page_slots,
            is_init: 0,
            separate_cache: 0,
            n_init_page: 0,
            sz_slot: 0,
            n_slot: 0,
            n_reserve: 0,
            mutex: None,
            p_free: Vec::new(),
            n_free_slot: 0,
            b_under_pressure: 0,
        }
    }

    /// Guarda uma página nova na arena.
    pub fn add_page(&mut self, p: PgHdr1) -> usize {
        store_page(&mut self.pages, &mut self.free_page_slots, p)
    }

    /// Remove uma página da arena (o `free` do bloco que a continha).
    pub fn drop_page(&mut self, h: usize) {
        self.pages[h - 1] = None;
        self.free_page_slots.push(h - 1);
    }

    /// Guarda um cache novo na arena e devolve o handle.
    pub fn add_cache(&mut self, c: PCache1) -> usize {
        for (i, slot) in self.caches.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(c);
                return i + 1;
            }
        }
        self.caches.push(Some(c));
        self.caches.len()
    }

    /// Guarda um grupo novo na arena e devolve o índice.
    pub fn add_group(&mut self, g: PGroup) -> usize {
        for (i, slot) in self.groups.iter_mut().enumerate().skip(1) {
            if slot.is_none() {
                *slot = Some(g);
                return i;
            }
        }
        self.groups.push(Some(g));
        self.groups.len() - 1
    }
}

/// Página pelo handle.
#[inline]
pub fn pg(s: &PCacheGlobal, h: usize) -> &PgHdr1 {
    s.pages[h - 1].as_ref().expect("handle de página inválido")
}

/// Página pelo handle, mutável.
#[inline]
pub fn pg_mut(s: &mut PCacheGlobal, h: usize) -> &mut PgHdr1 {
    s.pages[h - 1].as_mut().expect("handle de página inválido")
}

/// Cache pelo handle.
#[inline]
pub fn cache(s: &PCacheGlobal, h: usize) -> &PCache1 {
    s.caches[h - 1].as_ref().expect("handle de cache inválido")
}

/// Cache pelo handle, mutável.
#[inline]
pub fn cache_mut(s: &mut PCacheGlobal, h: usize) -> &mut PCache1 {
    s.caches[h - 1].as_mut().expect("handle de cache inválido")
}

/// Grupo pelo índice.
#[inline]
pub fn group(s: &PCacheGlobal, g: usize) -> &PGroup {
    s.groups[g].as_ref().expect("índice de grupo inválido")
}

/// Grupo pelo índice, mutável.
#[inline]
pub fn group_mut(s: &mut PCacheGlobal, g: usize) -> &mut PGroup {
    s.groups[g].as_mut().expect("índice de grupo inválido")
}

thread_local! {
    /// O `pcache1_g` do C. Todo o código do arquivo acessa o estado global por
    /// `with_pcache1`.
    static PCACHE1_G: RefCell<PCacheGlobal> = RefCell::new(PCacheGlobal::new());
}

/// Executa `f` com o estado global do cache de página.
pub fn with_pcache1<R>(f: impl FnOnce(&mut PCacheGlobal) -> R) -> R {
    PCACHE1_G.with(|g| f(&mut g.borrow_mut()))
}

/// Verdadeiro se o `sqlite3Malloc` conseguiria atender uma alocação deste tamanho.
#[inline]
pub fn pcache1_heap_alloc_ok(n_byte: i64) -> bool {
    n_byte > 0 && n_byte < PCACHE1_MAX_ALLOC
}

// As macros pcache1EnterMutex e pcache1LeaveMutex do C só fazem `assert(mutex==0)`
// quando SQLITE_ENABLE_MEMORY_MANAGEMENT não está definido (o caso do Debian), então
// não geram código e não existem aqui; PCACHE1_MIGHT_USE_GROUP_MUTEX vale 0.

/// Esta função é chamada na inicialização se um buffer estático for fornecido para
/// o cache de página com o verbo SQLITE_CONFIG_PAGECACHE de `sqlite3_config()`. O
/// parâmetro `p_buf` aponta para uma alocação grande o bastante para `n` buffers de
/// `sz` bytes cada.
///
/// É chamada de `sqlite3_initialize()`, portanto já está serializada. Não precisa de
/// mais exclusão mútua.
pub fn pcache_buffer_setup(p_buf: Option<&mut [u8]>, sz: i32, n: i32) {
    with_pcache1(|s| {
        if s.is_init != 0 {
            let mut sz = sz;
            let mut n = n;
            if p_buf.is_none() {
                sz = 0;
                n = 0;
            }
            if n == 0 {
                sz = 0;
            }
            sz = rounddown8(sz as usize) as i32;
            s.sz_slot = sz;
            s.n_slot = n;
            s.n_free_slot = n;
            s.n_reserve = if n > 90 { 10 } else { n / 10 + 1 };
            s.p_free.clear();
            s.b_under_pressure = 0;
            // Cada slot entra na cabeça da lista, então o último vira o topo.
            for i in 0..n.max(0) {
                s.p_free.push(i as usize);
            }
        }
    });
}

/// Tenta inicializar os campos `p_free` e `p_bulk` do cache. Devolve verdadeiro (1)
/// se `p_free` termina com uma ou mais páginas livres.
pub fn pcache1_init_bulk(s: &mut PCacheGlobal, h_cache: usize) -> i32 {
    if s.n_init_page == 0 {
        return 0;
    }
    let (sz_alloc, n_max) = {
        let c = cache(s, h_cache);
        (c.sz_alloc as i64, c.n_max as i64)
    };
    // Não vale a pena um bloco se o cache for muito pequeno.
    if n_max < 3 {
        return 0;
    }
    begin_benign_malloc();
    let mut sz_bulk: i64 = if s.n_init_page > 0 {
        sz_alloc * s.n_init_page as i64
    } else {
        -1024 * s.n_init_page as i64
    };
    if sz_bulk > sz_alloc * n_max {
        sz_bulk = sz_alloc * n_max;
    }
    let bulk_ok = pcache1_heap_alloc_ok(sz_bulk);
    cache_mut(s, h_cache).p_bulk = if bulk_ok { Some(sz_bulk as usize) } else { None };
    end_benign_malloc();
    if bulk_ok {
        // nBulk = sqlite3MallocSize(zBulk)/szAlloc
        let mut n_bulk: i64 = round8(sz_bulk as usize) as i64 / sz_alloc;
        loop {
            let mut px = blank_pg_hdr1();
            px.page.p_buf = vec![0u8; sz_alloc as usize];
            px.is_bulk_local = 1;
            px.is_anchor = 0;
            px.p_next = cache(s, h_cache).p_free;
            px.p_lru_prev = None;
            px.p_extra_hdr = Some(new_pg_hdr());
            let hx = s.add_page(px);
            cache_mut(s, h_cache).p_free = Some(hx);
            n_bulk -= 1;
            if n_bulk == 0 {
                break;
            }
        }
    }
    if cache(s, h_cache).p_free.is_some() {
        1
    } else {
        0
    }
}

/// Função de malloc usada neste arquivo para obter espaço do buffer configurado com
/// a opção `sqlite3_config(SQLITE_CONFIG_PAGECACHE)`. Se não houver tal buffer, ou
/// não houver mais espaço nele, recorre a `sqlite3Malloc()`.
///
/// Várias threads podem executar esta rotina ao mesmo tempo. As variáveis globais do
/// pcache1 precisam ser protegidas por mutex.
pub fn pcache1_alloc(s: &mut PCacheGlobal, n_byte: i32) -> Option<PageAlloc> {
    let mut slot: Option<usize> = None;
    if n_byte <= s.sz_slot {
        mutex_enter(s.mutex.as_deref());
        if let Some(i) = s.p_free.pop() {
            slot = Some(i);
            s.n_free_slot -= 1;
            s.b_under_pressure = if s.n_free_slot < s.n_reserve { 1 } else { 0 };
            status_highwater(SQLITE_STATUS_PAGECACHE_SIZE, n_byte);
            status_up(SQLITE_STATUS_PAGECACHE_USED, 1);
        }
        mutex_leave(s.mutex.as_deref());
    }
    if slot.is_none() {
        // Não há memória no pool SQLITE_CONFIG_PAGECACHE: pega de sqlite3Malloc.
        if !pcache1_heap_alloc_ok(n_byte as i64) {
            return None;
        }
        // SQLITE_DISABLE_PAGECACHE_OVERFLOW_STATS não está definido.
        let sz = round8(n_byte as usize) as i32;
        mutex_enter(s.mutex.as_deref());
        status_highwater(SQLITE_STATUS_PAGECACHE_SIZE, n_byte);
        status_up(SQLITE_STATUS_PAGECACHE_OVERFLOW, sz);
        mutex_leave(s.mutex.as_deref());
    }
    Some(PageAlloc {
        buf: vec![0u8; n_byte.max(0) as usize],
        slot,
        n_byte: n_byte.max(0) as usize,
    })
}


// ---- part_001.rs ----

/// Libera um buffer obtido de `pcache1_alloc()`.
pub fn pcache1_free(s: &mut PCacheGlobal, p: Option<PageAlloc>) {
    let p = match p {
        None => return,
        Some(p) => p,
    };
    if let Some(slot) = p.slot {
        // SQLITE_WITHIN(p, pcache1.pStart, pcache1.pEnd): o buffer é um slot do pool.
        mutex_enter(s.mutex.as_deref());
        status_down(SQLITE_STATUS_PAGECACHE_USED, 1);
        s.p_free.push(slot);
        s.n_free_slot += 1;
        s.b_under_pressure = if s.n_free_slot < s.n_reserve { 1 } else { 0 };
        mutex_leave(s.mutex.as_deref());
    } else {
        // SQLITE_DISABLE_PAGECACHE_OVERFLOW_STATS não está definido.
        let n_freed = round8(p.n_byte) as i32;
        mutex_enter(s.mutex.as_deref());
        status_down(SQLITE_STATUS_PAGECACHE_OVERFLOW, n_freed);
        mutex_leave(s.mutex.as_deref());
        // sqlite3_free(p): o Vec é solto aqui.
    }
}

// pcache1MemSize só existe com SQLITE_ENABLE_MEMORY_MANAGEMENT, que o Debian não define.

/// Soma ou subtrai 1 do contador de páginas purgáveis do cache (o `(*pCache->pnPurgeable)++`
/// e `--` do C, aritmética sem sinal que dá a volta).
pub fn pcache1_purgeable_adjust(s: &mut PCacheGlobal, h_cache: usize, up: bool) {
    let (in_group, g) = {
        let c = cache(s, h_cache);
        (c.pn_purgeable_in_group, c.p_group)
    };
    let slot: &mut u32 = if in_group {
        &mut group_mut(s, g).n_purgeable
    } else {
        &mut cache_mut(s, h_cache).n_purgeable_dummy
    };
    *slot = if up {
        slot.wrapping_add(1)
    } else {
        slot.wrapping_sub(1)
    };
}

/// Zera o `PgHdr` guardado em `pExtra` (o `*(void**)pPage->page.pExtra = 0` do C) e
/// garante que a página tenha o buffer: o conteúdo antigo, se houver, é mantido como
/// no C, onde o buffer continua no mesmo lugar.
pub fn pcache1_reset_page_extra(s: &mut PCacheGlobal, hp: usize, sz_alloc: usize) {
    let p = pg_mut(s, hp);
    if let Some(old) = p.p_extra_hdr.take() {
        let data = core::mem::take(&mut old.borrow_mut().p_data);
        if !data.is_empty() {
            p.page.p_buf = data;
        }
    }
    if p.page.p_buf.is_empty() {
        p.page.p_buf = vec![0u8; sz_alloc];
    }
    p.p_extra_hdr = Some(new_pg_hdr());
}

/// Aloca um novo objeto de página inicialmente associado ao cache `h_cache`.
pub fn pcache1_alloc_page(s: &mut PCacheGlobal, h_cache: usize, benign_malloc: i32) -> Option<usize> {
    let p: usize;
    let has_free = cache(s, h_cache).p_free.is_some();
    if has_free || (cache(s, h_cache).n_page == 0 && pcache1_init_bulk(s, h_cache) != 0) {
        let hp = cache(s, h_cache).p_free.expect("pFree vazio");
        let next = pg(s, hp).p_next;
        cache_mut(s, h_cache).p_free = next;
        pg_mut(s, hp).p_next = None;
        p = hp;
    } else {
        // SQLITE_ENABLE_MEMORY_MANAGEMENT não está definido: o mutex do grupo não é
        // solto em volta de pcache1_alloc().
        let sz_alloc = cache(s, h_cache).sz_alloc;
        if benign_malloc != 0 {
            begin_benign_malloc();
        }
        let p_pg = pcache1_alloc(s, sz_alloc);
        if benign_malloc != 0 {
            end_benign_malloc();
        }
        let p_pg = p_pg?;
        let mut np = blank_pg_hdr1();
        np.page.p_buf = p_pg.buf;
        np.is_bulk_local = 0;
        np.is_anchor = 0;
        np.p_lru_prev = None;
        np.p_extra_hdr = Some(new_pg_hdr());
        np.alloc_slot = p_pg.slot;
        np.alloc_bytes = p_pg.n_byte;
        p = s.add_page(np);
    }
    pcache1_purgeable_adjust(s, h_cache, true);
    Some(p)
}

/// Libera um objeto de página alocado por `pcache1_alloc_page()`.
pub fn pcache1_free_page(s: &mut PCacheGlobal, hp: usize) {
    let h_cache = pg(s, hp).p_cache;
    if pg(s, hp).is_bulk_local != 0 {
        let head = cache(s, h_cache).p_free;
        pg_mut(s, hp).p_next = head;
        cache_mut(s, h_cache).p_free = Some(hp);
    } else {
        let p = pg_mut(s, hp);
        let a = PageAlloc {
            buf: core::mem::take(&mut p.page.p_buf),
            slot: p.alloc_slot,
            n_byte: p.alloc_bytes,
        };
        pcache1_free(s, Some(a));
        // O PgHdr1 morava dentro do buffer liberado.
        s.drop_page(hp);
    }
    pcache1_purgeable_adjust(s, h_cache, false);
}

/// Malloc usado pelo SQLite para obter espaço do buffer configurado com a opção
/// `sqlite3_config(SQLITE_CONFIG_PAGECACHE)`. Sem esse buffer, recorre a `sqlite3Malloc()`.
pub fn page_malloc(sz: i32) -> Option<PageAlloc> {
    debug_assert!(sz <= 65536 + 8); // Estas alocações nunca são muito grandes.
    with_pcache1(|s| pcache1_alloc(s, sz))
}

/// Libera um buffer obtido de `page_malloc()`.
pub fn page_free(p: Option<PageAlloc>) {
    with_pcache1(|s| pcache1_free(s, p))
}

/// Devolve verdadeiro se é desejável evitar a alocação de uma nova entrada do cache.
///
/// Se a memória foi alocada especificamente para o cache com SQLITE_CONFIG_PAGECACHE
/// mas já foi toda usada, convém evitar uma entrada nova, porque presumivelmente o
/// SQLITE_CONFIG_PAGECACHE devia bastar para todo o cache e não se deve derramar a
/// alocação para o heap.
///
/// Ou, se o heap é usado para toda a memória do cache mas está sob pressão, de novo
/// convém evitar uma entrada nova para não estressar mais o heap.
pub fn pcache1_under_memory_pressure(s: &PCacheGlobal, h_cache: usize) -> i32 {
    let c = cache(s, h_cache);
    if s.n_slot != 0 && (c.sz_page + c.sz_extra) <= s.sz_slot {
        s.b_under_pressure
    } else {
        heap_nearly_full()
    }
}

/// Usada para redimensionar a tabela de hash do cache `h_cache`.
///
/// O mutex do PCache precisa estar travado ao chamar esta função.
pub fn pcache1_resize_hash(s: &mut PCacheGlobal, h_cache: usize) {
    let n_hash_old = cache(s, h_cache).n_hash;
    let mut n_new: u32 = n_hash_old.wrapping_mul(2);
    if n_new < 256 {
        n_new = 256;
    }

    if n_hash_old != 0 {
        begin_benign_malloc();
    }
    // sqlite3MallocZero(sizeof(PgHdr1 *)*nNew)
    let ok = pcache1_heap_alloc_ok(8 * n_new as i64);
    if n_hash_old != 0 {
        end_benign_malloc();
    }
    if ok {
        let mut ap_new: Vec<Option<usize>> = vec![None; n_new as usize];
        for i in 0..n_hash_old as usize {
            let mut p_next = cache(s, h_cache).ap_hash[i];
            while let Some(p_page) = p_next {
                let h = (pg(s, p_page).i_key % n_new) as usize;
                p_next = pg(s, p_page).p_next;
                pg_mut(s, p_page).p_next = ap_new[h];
                ap_new[h] = Some(p_page);
            }
        }
        let c = cache_mut(s, h_cache);
        c.ap_hash = ap_new;
        c.n_hash = n_new;
    }
}

/// Usada internamente para tirar a página `p_page` da lista LRU do PGroup, se ela
/// fizer parte dela. Se não fizer parte, a função não faz nada.
///
/// O mutex do PGroup precisa estar travado ao chamar esta função.
pub fn pcache1_pin_page(s: &mut PCacheGlobal, p_page: usize) -> usize {
    let prev = pg(s, p_page).p_lru_prev.expect("pLruPrev nulo");
    let next = pg(s, p_page).p_lru_next.expect("pLruNext nulo");
    pg_mut(s, prev).p_lru_next = Some(next);
    pg_mut(s, next).p_lru_prev = Some(prev);
    pg_mut(s, p_page).p_lru_next = None;
    // Não precisa zerar p_lru_prev: nunca é lido se p_lru_next for None.
    let h_cache = pg(s, p_page).p_cache;
    cache_mut(s, h_cache).n_recyclable -= 1;
    p_page
}

/// Tira `p_page` da cadeia de hash `slot` do cache `h_cache` (o `*pp = (*pp)->pNext`
/// depois de andar com `pp` até a página). O `p_next` da própria página não muda.
pub fn pcache1_unlink_hash(s: &mut PCacheGlobal, h_cache: usize, slot: usize, p_page: usize) {
    let after = pg(s, p_page).p_next;
    let head = cache(s, h_cache).ap_hash[slot].expect("página fora da cadeia de hash");
    if head == p_page {
        cache_mut(s, h_cache).ap_hash[slot] = after;
        return;
    }
    let mut cur = head;
    loop {
        let nx = pg(s, cur).p_next.expect("página fora da cadeia de hash");
        if nx == p_page {
            pg_mut(s, cur).p_next = after;
            return;
        }
        cur = nx;
    }
}

/// Tira a página dada da tabela de hash (`ap_hash` do PCache1) em que ela está.
/// Também libera a página se `free_flag` for verdadeiro.
///
/// O mutex do PGroup precisa estar travado ao chamar esta função.
pub fn pcache1_remove_from_hash(s: &mut PCacheGlobal, p_page: usize, free_flag: i32) {
    let h_cache = pg(s, p_page).p_cache;
    let h = (pg(s, p_page).i_key % cache(s, h_cache).n_hash) as usize;
    pcache1_unlink_hash(s, h_cache, h, p_page);
    cache_mut(s, h_cache).n_page -= 1;
    if free_flag != 0 {
        pcache1_free_page(s, p_page);
    }
}

/// Libera a memória em bloco do cache: as páginas locais que sobraram em `p_free` e
/// o marcador `p_bulk` (o `sqlite3_free(pCache->pBulk)` do C).
pub fn pcache1_free_bulk(s: &mut PCacheGlobal, h_cache: usize) {
    let mut cur = cache(s, h_cache).p_free;
    while let Some(hp) = cur {
        cur = pg(s, hp).p_next;
        s.drop_page(hp);
    }
    let c = cache_mut(s, h_cache);
    c.p_bulk = None;
    c.p_free = None;
}

/// Se há mais de `n_max_page` páginas alocadas, tenta reciclar páginas para reduzir
/// o número alocado a `n_max_page`.
pub fn pcache1_enforce_max_page(s: &mut PCacheGlobal, h_cache: usize) {
    let g = cache(s, h_cache).p_group;
    loop {
        let (n_purgeable, n_max_page, lru) = {
            let gr = group(s, g);
            (gr.n_purgeable, gr.n_max_page, gr.lru)
        };
        if n_purgeable <= n_max_page {
            break;
        }
        let p = pg(s, lru).p_lru_prev.expect("pLruPrev nulo");
        if pg(s, p).is_anchor != 0 {
            break;
        }
        pcache1_pin_page(s, p);
        pcache1_remove_from_hash(s, p, 1);
    }
    let c = cache(s, h_cache);
    if c.n_page == 0 && c.p_bulk.is_some() {
        pcache1_free_bulk(s, h_cache);
    }
}

/// Descarta todas as páginas do cache `h_cache` com número de página (valor da
/// chave) maior ou igual a `i_limit`. As páginas fixadas que atendem o critério são
/// desafixadas antes de descartadas.
///
/// O mutex do PCache precisa estar travado ao chamar esta função.
pub fn pcache1_truncate_unsafe(s: &mut PCacheGlobal, h_cache: usize, i_limit: u32) {
    let (n_hash, i_max_key) = {
        let c = cache(s, h_cache);
        (c.n_hash, c.i_max_key)
    };
    let mut h: u32;
    let i_stop: u32;
    if i_max_key.wrapping_sub(i_limit) < n_hash {
        // Se só estamos aparando as últimas páginas do cache, não adianta varrer a
        // tabela toda: só os slots que podem conter páginas a remover.
        h = i_limit % n_hash;
        i_stop = i_max_key % n_hash;
    } else {
        // Caso geral, em que muitas páginas são removidas: varre a tabela inteira.
        h = n_hash / 2;
        i_stop = h.wrapping_sub(1);
    }
    loop {
        let mut prev: Option<usize> = None;
        let mut cur = cache(s, h_cache).ap_hash[h as usize];
        while let Some(p_page) = cur {
            let next = pg(s, p_page).p_next;
            if pg(s, p_page).i_key >= i_limit {
                cache_mut(s, h_cache).n_page -= 1;
                match prev {
                    None => cache_mut(s, h_cache).ap_hash[h as usize] = next,
                    Some(pv) => pg_mut(s, pv).p_next = next,
                }
                if page_is_unpinned(pg(s, p_page)) {
                    pcache1_pin_page(s, p_page);
                }
                pcache1_free_page(s, p_page);
                cur = next;
            } else {
                prev = Some(p_page);
                cur = next;
            }
        }
        if h == i_stop {
            break;
        }
        h = (h + 1) % n_hash;
    }
}

/// Implementação do método `sqlite3_pcache.xInit`.
pub fn pcache1_init(_not_used: usize) -> i32 {
    with_pcache1(|s| {
        debug_assert!(s.is_init == 0);
        *s = PCacheGlobal::new();

        // `separate_cache` é verdadeiro se cada PCache tem seu PGroup privado (modo 1)
        // e falso se o único PGroup `grp` serve a todos os caches (modo 2).
        //
        //   * Sempre cache unificado (modo 2) com ENABLE_MEMORY_MANAGEMENT (não é o caso);
        //   * cache unificado em aplicações de uma thread que configuraram um buffer
        //     inicial com sqlite3_config(SQLITE_CONFIG_PAGECACHE, pBuf, sz, N) com pBuf
        //     não nulo;
        //   * senão, caches separados (modo 1).
        //
        // SQLITE_THREADSAFE vale 1 no Debian.
        s.separate_cache = if SQLITE_CONFIG.p_page.is_none() || SQLITE_CONFIG.b_core_mutex > 0 {
            1
        } else {
            0
        };

        if SQLITE_CONFIG.b_core_mutex != 0 {
            group_mut(s, 0).mutex = mutex_alloc(SQLITE_MUTEX_STATIC_LRU);
            s.mutex = mutex_alloc(SQLITE_MUTEX_STATIC_PMEM);
        }
        if s.separate_cache != 0 && SQLITE_CONFIG.n_page != 0 && SQLITE_CONFIG.p_page.is_none() {
            s.n_init_page = SQLITE_CONFIG.n_page;
        } else {
            s.n_init_page = 0;
        }
        group_mut(s, 0).mx_pinned = 10;
        s.is_init = 1;
        SQLITE_OK
    })
}


// ---- part_002.rs ----

/// Implementação do método `sqlite3_pcache.xShutdown`. O mutex estático alocado em
/// xInit não precisa ser liberado.
pub fn pcache1_shutdown(_not_used: usize) {
    with_pcache1(|s| {
        debug_assert!(s.is_init != 0);
        *s = PCacheGlobal::new();
    })
}

/// Implementação do método `sqlite3_pcache.xCreate`. Aloca um cache novo e devolve o
/// handle (0 se faltou memória).
pub fn pcache1_create(sz_page: i32, sz_extra: i32, b_purgeable: i32) -> usize {
    with_pcache1(|s| {
        debug_assert!((sz_page & (sz_page - 1)) == 0 && sz_page >= 512 && sz_page <= 65536);
        debug_assert!(sz_extra < 300);

        // sz = sizeof(PCache1) + sizeof(PGroup)*pcache1.separateCache: o PGroup do
        // modo 1 mora junto do PCache1.
        let h_cache = s.add_cache(PCache1 {
            p_group: 0,
            pn_purgeable_in_group: false,
            sz_page: 0,
            sz_extra: 0,
            sz_alloc: 0,
            b_purgeable: 0,
            n_min: 0,
            n_max: 0,
            n_90pct: 0,
            i_max_key: 0,
            n_purgeable_dummy: 0,
            n_recyclable: 0,
            n_page: 0,
            n_hash: 0,
            ap_hash: Vec::new(),
            p_free: None,
            p_bulk: None,
        });
        let p_group: usize;
        if s.separate_cache != 0 {
            let g = new_group(&mut s.pages, &mut s.free_page_slots);
            p_group = s.add_group(g);
            group_mut(s, p_group).mx_pinned = 10;
        } else {
            p_group = 0;
        }
        let lru = group(s, p_group).lru;
        if pg(s, lru).is_anchor == 0 {
            let a = pg_mut(s, lru);
            a.is_anchor = 1;
            a.p_lru_prev = Some(lru);
            a.p_lru_next = Some(lru);
        }
        {
            let c = cache_mut(s, h_cache);
            c.p_group = p_group;
            c.sz_page = sz_page;
            c.sz_extra = sz_extra;
            c.sz_alloc = sz_page + sz_extra + round8(SIZEOF_PGHDR1) as i32;
            c.b_purgeable = if b_purgeable != 0 { 1 } else { 0 };
        }
        pcache1_resize_hash(s, h_cache);
        if b_purgeable != 0 {
            let n_min = 10;
            cache_mut(s, h_cache).n_min = n_min;
            cache_mut(s, h_cache).pn_purgeable_in_group = true;
            let gr = group_mut(s, p_group);
            gr.n_min_page = gr.n_min_page.wrapping_add(n_min);
            gr.mx_pinned = gr
                .n_max_page
                .wrapping_add(10)
                .wrapping_sub(gr.n_min_page);
        } else {
            cache_mut(s, h_cache).pn_purgeable_in_group = false;
        }
        if cache(s, h_cache).n_hash == 0 {
            pcache1_destroy_cache(s, h_cache);
            return 0;
        }
        h_cache
    })
}

/// Implementação do método `sqlite3_pcache.xCachesize`. Configura o limite de
/// `cache_size` de um cache.
pub fn pcache1_cachesize(p: usize, n_max: i32) {
    with_pcache1(|s| {
        debug_assert!(n_max >= 0);
        if cache(s, p).b_purgeable != 0 {
            let g = cache(s, p).p_group;
            let c_n_max = cache(s, p).n_max;
            let mut n: u32 = n_max as u32;
            let gr = group_mut(s, g);
            let limit = 0x7fff_0000u32
                .wrapping_sub(gr.n_max_page)
                .wrapping_add(c_n_max);
            if n > limit {
                n = limit;
            }
            gr.n_max_page = gr.n_max_page.wrapping_add(n.wrapping_sub(c_n_max));
            gr.mx_pinned = gr
                .n_max_page
                .wrapping_add(10)
                .wrapping_sub(gr.n_min_page);
            let c = cache_mut(s, p);
            c.n_max = n;
            c.n_90pct = c.n_max.wrapping_mul(9) / 10;
            pcache1_enforce_max_page(s, p);
        }
    })
}

/// Implementação do método `sqlite3_pcache.xShrink`. Libera o máximo de memória possível.
pub fn pcache1_shrink(p: usize) {
    with_pcache1(|s| {
        if cache(s, p).b_purgeable != 0 {
            let g = cache(s, p).p_group;
            let saved_max_page = group(s, g).n_max_page;
            group_mut(s, g).n_max_page = 0;
            pcache1_enforce_max_page(s, p);
            group_mut(s, g).n_max_page = saved_max_page;
        }
    })
}

/// Implementação do método `sqlite3_pcache.xPagecount`.
pub fn pcache1_pagecount(p: usize) -> i32 {
    with_pcache1(|s| cache(s, p).n_page as i32)
}

/// Implementa os passos 3, 4 e 5 do algoritmo de `pcache1_fetch()` descrito no
/// cabeçalho dessa função.
///
/// Os passos ficam em um procedimento separado porque em geral não são necessários, e
/// evitando a inicialização de pilha que exigem a rotina principal roda mais rápido.
pub fn pcache1_fetch_stage2(
    s: &mut PCacheGlobal,
    h_cache: usize,
    i_key: u32,
    create_flag: i32,
) -> Option<usize> {
    let g = cache(s, h_cache).p_group;
    let mut p_page: Option<usize> = None;

    // Passo 3: aborta se create_flag é 1 mas o cache está quase cheio.
    let n_pinned = cache(s, h_cache)
        .n_page
        .wrapping_sub(cache(s, h_cache).n_recyclable);
    if create_flag == 1
        && (n_pinned >= group(s, g).mx_pinned
            || n_pinned >= cache(s, h_cache).n_90pct
            || (pcache1_under_memory_pressure(s, h_cache) != 0
                && cache(s, h_cache).n_recyclable < n_pinned))
    {
        return None;
    }

    if cache(s, h_cache).n_page >= cache(s, h_cache).n_hash {
        pcache1_resize_hash(s, h_cache);
    }
    debug_assert!(cache(s, h_cache).n_hash > 0);

    // Passo 4: tenta reciclar uma página.
    let lru = group(s, g).lru;
    if cache(s, h_cache).b_purgeable != 0
        && pg(s, pg(s, lru).p_lru_prev.expect("pLruPrev nulo")).is_anchor == 0
        && ((cache(s, h_cache).n_page + 1 >= cache(s, h_cache).n_max)
            || pcache1_under_memory_pressure(s, h_cache) != 0)
    {
        let p = pg(s, lru).p_lru_prev.expect("pLruPrev nulo");
        pcache1_remove_from_hash(s, p, 0);
        pcache1_pin_page(s, p);
        let p_other = pg(s, p).p_cache;
        if cache(s, p_other).sz_alloc != cache(s, h_cache).sz_alloc {
            pcache1_free_page(s, p);
            p_page = None;
        } else {
            let diff = cache(s, p_other).b_purgeable - cache(s, h_cache).b_purgeable;
            let gr = group_mut(s, g);
            gr.n_purgeable = gr.n_purgeable.wrapping_sub(diff as u32);
            p_page = Some(p);
        }
    }

    // Passo 5: se ainda não há um buffer de página utilizável, tenta alocar um novo.
    if p_page.is_none() {
        p_page = pcache1_alloc_page(s, h_cache, if create_flag == 1 { 1 } else { 0 });
    }

    if let Some(hp) = p_page {
        let h = (i_key % cache(s, h_cache).n_hash) as usize;
        let head = cache(s, h_cache).ap_hash[h];
        let sz_alloc = cache(s, h_cache).sz_alloc as usize;
        cache_mut(s, h_cache).n_page += 1;
        {
            let p = pg_mut(s, hp);
            p.i_key = i_key;
            p.p_next = head;
            p.p_cache = h_cache;
            p.p_lru_next = None;
            // Não precisa zerar p_lru_prev: não é lido enquanto p_lru_next for None.
        }
        // *(void **)pPage->page.pExtra = 0
        pcache1_reset_page_extra(s, hp, sz_alloc);
        let c = cache_mut(s, h_cache);
        c.ap_hash[h] = Some(hp);
        if i_key > c.i_max_key {
            c.i_max_key = i_key;
        }
    }
    p_page
}

/// Implementação do método `sqlite3_pcache.xFetch`. Busca uma página pela chave.
///
/// Se uma página nova pode ser alocada depende de `create_flag`: 0 não aloca; 1 aloca
/// se há espaço facilmente disponível; 2 tenta de verdade alocar.
///
/// Num cache não purgável (usado como armazenamento de banco em memória) não há
/// diferença entre `create_flag` 1 e 2, então o chamador (pcache.c) nunca passa 1 a
/// um cache não purgável.
///
/// Há três abordagens para obter espaço para a página, conforme `create_flag`:
///
///   1. Qualquer que seja `create_flag`, procura-se no cache uma cópia da página
///      pedida. Se achada, é devolvida.
///
///   2. Se `create_flag==0` e a página não está no cache, devolve nulo.
///
///   3. Se `create_flag` é 1 e a página não está no cache, devolve nulo (não aloca)
///      se uma das condições for verdadeira:
///
///       (a) o número de páginas fixadas pelo cache é maior que `n_max`, ou
///
///       (b) o número de páginas fixadas pelo cache é maior que a soma de `n_max` de
///           todos os caches purgáveis, menos a soma de `n_min` dos outros.
///
///   4. Se nenhuma das três primeiras condições vale e o cache é purgável, e uma das
///      seguintes é verdadeira:
///
///       (a) o número de páginas alocadas ao cache já é `n_max`, ou
///
///       (b) o número de páginas alocadas a todos os caches purgáveis já é igual ou
///           maior que a soma de `n_max` de todos eles, ou
///
///       (c) o sistema está sob pressão de memória e quer evitar entradas de cache
///           desnecessárias,
///
///      tenta reciclar uma página da lista LRU. Se tem o tamanho certo, devolve o
///      buffer reciclado. Senão, libera o buffer e segue para o passo 5.
///
///   5. Senão, aloca e devolve um novo buffer de página.
///
/// A versão com mutex (`pcache1FetchWithMutex`) só existe quando
/// PCACHE1_MIGHT_USE_GROUP_MUTEX vale 1, o que não ocorre aqui.
pub fn pcache1_fetch_no_mutex(
    s: &mut PCacheGlobal,
    h_cache: usize,
    i_key: u32,
    create_flag: i32,
) -> Option<usize> {
    // Passo 1: procura na tabela de hash uma entrada existente.
    let n_hash = cache(s, h_cache).n_hash;
    let mut p_page = cache(s, h_cache).ap_hash[(i_key % n_hash) as usize];
    while let Some(pp) = p_page {
        if pg(s, pp).i_key == i_key {
            break;
        }
        p_page = pg(s, pp).p_next;
    }

    // Passo 2: se a página está na tabela de hash, devolve. Se não está e
    // create_flag é 0, aborta. Senão continua com os passos seguintes.
    if let Some(pp) = p_page {
        if page_is_unpinned(pg(s, pp)) {
            Some(pcache1_pin_page(s, pp))
        } else {
            Some(pp)
        }
    } else if create_flag != 0 {
        // Passos 3, 4 e 5 implementados por esta sub-rotina.
        pcache1_fetch_stage2(s, h_cache, i_key, create_flag)
    } else {
        None
    }
}

/// Implementação do método `sqlite3_pcache.xFetch`: devolve o handle da página ou 0.
pub fn pcache1_fetch(p: usize, i_key: u32, create_flag: i32) -> usize {
    with_pcache1(|s| {
        debug_assert!(cache(s, p).b_purgeable != 0 || create_flag != 1);
        debug_assert!(cache(s, p).b_purgeable != 0 || cache(s, p).n_min == 0);
        debug_assert!(cache(s, p).b_purgeable == 0 || cache(s, p).n_min == 10);
        debug_assert!(cache(s, p).n_min == 0 || cache(s, p).b_purgeable != 0);
        debug_assert!(cache(s, p).n_hash > 0);
        pcache1_fetch_no_mutex(s, p, i_key, create_flag).unwrap_or(0)
    })
}

/// Implementação do método `sqlite3_pcache.xUnpin`. Marca uma página como desafixada
/// (elegível para reciclagem assíncrona).
pub fn pcache1_unpin(p: usize, p_pg: usize, reuse_unlikely: i32) {
    with_pcache1(|s| {
        debug_assert!(pg(s, p_pg).p_cache == p);
        let g = cache(s, p).p_group;

        // É um erro chamar esta função se a página já está na lista LRU do PGroup.
        debug_assert!(pg(s, p_pg).p_lru_next.is_none());
        debug_assert!(page_is_pinned(pg(s, p_pg)));

        if reuse_unlikely != 0 || group(s, g).n_purgeable > group(s, g).n_max_page {
            pcache1_remove_from_hash(s, p_pg, 1);
        } else {
            // Põe a página na lista LRU do PGroup, logo depois da âncora.
            let lru = group(s, g).lru;
            let first = pg(s, lru).p_lru_next.expect("pLruNext nulo");
            pg_mut(s, p_pg).p_lru_prev = Some(lru);
            pg_mut(s, p_pg).p_lru_next = Some(first);
            pg_mut(s, first).p_lru_prev = Some(p_pg);
            pg_mut(s, lru).p_lru_next = Some(p_pg);
            cache_mut(s, p).n_recyclable += 1;
        }
    })
}


// ---- part_003.rs ----

/// Implementação do método `sqlite3_pcache.xRekey`.
pub fn pcache1_rekey(p: usize, p_pg: usize, i_old: u32, i_new: u32) {
    with_pcache1(|s| {
        debug_assert!(pg(s, p_pg).i_key == i_old);
        debug_assert!(pg(s, p_pg).p_cache == p);
        debug_assert!(i_old != i_new); // O número da página realmente muda.

        let n_hash = cache(s, p).n_hash;
        let h_old = (i_old % n_hash) as usize;
        pcache1_unlink_hash(s, p, h_old, p_pg);

        let h_new = (i_new % n_hash) as usize;
        let head = cache(s, p).ap_hash[h_new];
        {
            let page = pg_mut(s, p_pg);
            page.i_key = i_new;
            page.p_next = head;
        }
        let c = cache_mut(s, p);
        c.ap_hash[h_new] = Some(p_pg);
        if i_new > c.i_max_key {
            c.i_max_key = i_new;
        }
    })
}

/// Implementação do método `sqlite3_pcache.xTruncate`.
///
/// Descarta todas as páginas desafixadas do cache com número igual ou maior que
/// `i_limit`. As páginas fixadas com número igual ou maior que `i_limit` são
/// desafixadas implicitamente.
pub fn pcache1_truncate(p: usize, i_limit: u32) {
    with_pcache1(|s| {
        if i_limit <= cache(s, p).i_max_key {
            pcache1_truncate_unsafe(s, p, i_limit);
            cache_mut(s, p).i_max_key = i_limit.wrapping_sub(1);
        }
    })
}

/// Destrói um cache criado por `pcache1_create()`, dentro do estado já travado.
pub fn pcache1_destroy_cache(s: &mut PCacheGlobal, p: usize) {
    let g = cache(s, p).p_group;
    debug_assert!(
        cache(s, p).b_purgeable != 0 || (cache(s, p).n_max == 0 && cache(s, p).n_min == 0)
    );
    if cache(s, p).n_page != 0 {
        pcache1_truncate_unsafe(s, p, 0);
    }
    let (n_max, n_min) = (cache(s, p).n_max, cache(s, p).n_min);
    {
        let gr = group_mut(s, g);
        debug_assert!(gr.n_max_page >= n_max);
        gr.n_max_page = gr.n_max_page.wrapping_sub(n_max);
        debug_assert!(gr.n_min_page >= n_min);
        gr.n_min_page = gr.n_min_page.wrapping_sub(n_min);
        gr.mx_pinned = gr
            .n_max_page
            .wrapping_add(10)
            .wrapping_sub(gr.n_min_page);
    }
    pcache1_enforce_max_page(s, p);
    // sqlite3_free(pCache->pBulk): as páginas locais que sobraram vão junto.
    pcache1_free_bulk(s, p);
    // sqlite3_free(pCache->apHash) e sqlite3_free(pCache): o PGroup do modo 1 mora
    // dentro da alocação do PCache1 e some com ele, com o elemento âncora da LRU.
    if g != 0 {
        let lru = group(s, g).lru;
        s.drop_page(lru);
        s.groups[g] = None;
    }
    s.caches[p - 1] = None;
}

/// Implementação do método `sqlite3_pcache.xDestroy`. Destrói um cache alocado por
/// `pcache1_create()`.
pub fn pcache1_destroy(p: usize) {
    with_pcache1(|s| pcache1_destroy_cache(s, p))
}

/// Chamada durante a inicialização (`sqlite3_initialize()`) para instalar o módulo de
/// cache plugável padrão, supondo que o usuário não tenha dado um alternativo.
pub fn pcache_set_default() {
    let default_methods = Sqlite3PcacheMethods2 {
        i_version: 1,
        p_arg: 0,
        x_init: Some(pcache1_init),
        x_shutdown: Some(pcache1_shutdown),
        x_create: Some(pcache1_create),
        x_cachesize: Some(pcache1_cachesize),
        x_pagecount: Some(pcache1_pagecount),
        x_fetch: Some(pcache1_fetch),
        x_unpin: Some(pcache1_unpin),
        x_rekey: Some(pcache1_rekey),
        x_truncate: Some(pcache1_truncate),
        x_destroy: Some(pcache1_destroy),
        x_shrink: Some(pcache1_shrink),
    };
    api::config(
        SQLITE_CONFIG_PCACHE2,
        vec![ConfigArg::PcacheMethods2(default_methods)],
    );
}

/// Devolve o tamanho do cabeçalho de cada página desta implementação de PCACHE.
pub fn header_size_pcache1() -> i32 {
    round8(SIZEOF_PGHDR1) as i32
}

/// Devolve o mutex global usado por esta implementação de PCACHE. A rotina
/// `sqlite3_status()` precisa de acesso a ele.
pub fn pcache1_mutex() -> Option<Rc<sqlite3_mutex>> {
    with_pcache1(|s| s.mutex.clone())
}

// sqlite3PcacheReleaseMemory (SQLITE_ENABLE_MEMORY_MANAGEMENT) e sqlite3PcacheStats
// (SQLITE_TEST) não existem nesta configuração.

/// Ponte para `pcache.c`: o `PgHdr` guardado em `pExtra` da página `handle` (o
/// `(PgHdr*)pPage->pExtra` do C).
pub fn pcache1_page_hdr(handle: usize) -> PgHdrRef {
    with_pcache1(|s| {
        pg(s, handle)
            .p_extra_hdr
            .clone()
            .expect("página sem PgHdr")
    })
}

/// Ponte para `pcache.c`: o objeto `sqlite3_pcache_page` da página `handle`. O buffer
/// de conteúdo passa para o chamador (que o guarda em `PgHdr.p_data`), e `p_extra`
/// carrega o handle da página em 8 bytes little-endian, para `pcache1_page_handle()`
/// voltar do `PgHdr` ao handle.
pub fn pcache1_page_obj(handle: usize) -> Box<Sqlite3PcachePage> {
    with_pcache1(|s| {
        let p = pg_mut(s, handle);
        Box::new(Sqlite3PcachePage {
            p_buf: core::mem::take(&mut p.page.p_buf),
            p_extra: (handle as u64).to_le_bytes().to_vec(),
        })
    })
}

/// Ponte para `pcache.c`: o `p->pPage` do C como handle, lido do `p_extra` do
/// objeto de página que `pcache1_page_obj()` entregou.
pub fn pcache1_page_handle(p: &PgHdrRef) -> usize {
    let b = p.borrow();
    let obj = b.p_page.as_ref().expect("PgHdr sem p_page");
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&obj.p_extra[..8]);
    u64::from_le_bytes(raw) as usize
}

