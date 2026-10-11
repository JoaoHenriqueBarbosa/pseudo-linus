//! Implementação padrão do cache de páginas pluggable (pcache1.c): tabela hash
//! por número de página, lista LRU das páginas não fixadas, política de
//! reciclagem e limites (`nMin`, `nMax`, `n90pct`, `mxPinned`).
//!
//! O C chama isto de fora, por `sqlite3_pcache_methods2` (`xCreate`, `xFetch`,
//! `xUnpin`, `xRekey`, `xTruncate`, `xDestroy`, `xCachesize`, `xShrink`,
//! `xPagecount`); aqui são métodos de `PCache1<E>`, usados só por
//! `crate::pcache`. O `PCache1` possui o `Vec<PgSlot<E>>` com todas as linhas
//! do cache; `PgId` é o índice no vetor. O slot 0 é o nó âncora da LRU
//! circular (o `PGroup.lru` do C) e nunca é uma página.
//!
//! Configuração do Debian resolvida (CONVENTIONS.md):
//!
//! * `SQLITE_THREADSAFE=1` com `bCoreMutex` ligado e sem `SQLITE_CONFIG_PAGECACHE`
//!   dá `separateCache = 1`: cada `PCache1` é o único membro do seu `PGroup`
//!   (modo 1 do C, sem mutex). O grupo vira campos do próprio `PCache1`
//!   (`n_max_page`, `n_min_page`, `mx_pinned`, `n_purgeable`). O modo 2 (grupo
//!   global, `SQLITE_ENABLE_MEMORY_MANAGEMENT`) não existe, então a reciclagem
//!   de página de outro cache e `sqlite3PcacheReleaseMemory` somem.
//! * Sem `SQLITE_CONFIG_PAGECACHE`: `nSlot == 0`, `pcache1Alloc` é só alocação
//!   do heap (`Vec<u8>`), `bUnderPressure` nunca vale e a pressão de memória
//!   vem de `heap_nearly_full()`.
//! * A alocação em bloco (`pcache1InitBulk`, `pBulk`, `pFree`, `isBulkLocal`) é
//!   só otimização de `malloc`: não muda o número de páginas nem a ordem de
//!   reciclagem, então some. `pcache1FreePage` solta o buffer e devolve o
//!   slot à lista de livres.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::pcache::{PgId, PgSlot};

/// Teto de páginas do `SQLITE_MAX_MMAP_SIZE` usado em `pcache1Cachesize` (0x7fff0000).
const MAX_CACHE_PAGES: u32 = 0x7fff0000;

/// `mem0.nearlyFull` do malloc.c (`sqlite3HeapNearlyFull`). O módulo de memória
/// ainda não existe; quando existir, ele passa a atualizar este indicador (ou
/// a função é trocada pela dele).
static HEAP_NEARLY_FULL: AtomicBool = AtomicBool::new(false);

/// Atualiza o indicador de heap quase cheio (`soft_heap_limit`).
pub fn set_heap_nearly_full(nearly_full: bool) {
    HEAP_NEARLY_FULL.store(nearly_full, Ordering::Relaxed);
}

/// `sqlite3HeapNearlyFull` (também usada por `crate::vdbesort`).
pub(crate) fn heap_nearly_full() -> bool {
    HEAP_NEARLY_FULL.load(Ordering::Relaxed)
}

/// `pcache1UnderMemoryPressure` com `nSlot == 0`: vale a pressão do heap.
fn under_memory_pressure() -> bool {
    heap_nearly_full()
}

/// Um cache de páginas (o `PCache1` do C, com o `PGroup` embutido).
pub struct PCache1<E> {
    // ---- PGroup (um por cache, modo 1) ----
    /// `nMaxPage`: soma de `nMax` dos caches purgáveis do grupo.
    n_max_page: u32,
    /// `nMinPage`: soma de `nMin`.
    n_min_page: u32,
    /// `mxPinned`: `nMaxPage + 10 - nMinPage`.
    mx_pinned: u32,
    /// `nPurgeable`: páginas purgáveis alocadas.
    n_purgeable: u32,

    // ---- PCache1 ----
    /// `szPage`.
    sz_page: usize,
    /// `bPurgeable`.
    b_purgeable: bool,
    /// `nMin`: páginas reservadas (10 se purgável).
    n_min: u32,
    /// `nMax`: o valor de `cache_size` configurado.
    n_max: u32,
    /// `n90pct`: `nMax*9/10`.
    n90pct: u32,
    /// `iMaxKey`: maior chave vista desde o último `xTruncate`.
    i_max_key: u32,
    /// `nPurgeableDummy`: para onde `pnPurgeable` aponta num cache não purgável.
    n_purgeable_dummy: u32,
    /// `nRecyclable`: páginas na LRU.
    n_recyclable: u32,
    /// `nPage`: páginas na tabela hash.
    n_page: u32,
    /// `nHash`: posições de `ap_hash`.
    n_hash: u32,
    /// `apHash`: cabeça de cada cadeia da tabela hash.
    ap_hash: Vec<Option<PgId>>,
    /// Todas as linhas. `slots[0]` é a âncora da LRU.
    pub(crate) slots: Vec<PgSlot<E>>,
    /// Posições de `slots` sem página (liberadas por `free_page`).
    free_slots: Vec<u32>,
}

impl<E: Default> PCache1<E> {
    // ------------------------------------------------------------------
    // Alocação de páginas
    // ------------------------------------------------------------------

    /// `*pCache->pnPurgeable`: contador de páginas purgáveis do grupo, ou o
    /// contador de descarte de um cache não purgável.
    fn purgeable_counter(&mut self) -> &mut u32 {
        if self.b_purgeable {
            &mut self.n_purgeable
        } else {
            &mut self.n_purgeable_dummy
        }
    }

    /// `pcache1AllocPage`: nova linha com buffer de `szPage` bytes. A falha de
    /// `malloc` do C não existe (o `Vec` aborta), então nunca devolve nulo.
    fn alloc_page(&mut self) -> PgId {
        let mut slot = PgSlot::vacant();
        slot.data = vec![0u8; self.sz_page];
        let idx = match self.free_slots.pop() {
            Some(i) => {
                self.slots[i as usize] = slot;
                i
            }
            None => {
                self.slots.push(slot);
                (self.slots.len() - 1) as u32
            }
        };
        let c = self.purgeable_counter();
        *c = c.wrapping_add(1);
        PgId(idx)
    }

    /// `pcache1FreePage`: solta o buffer e devolve a linha à lista de livres.
    fn free_page(&mut self, id: PgId) {
        debug_assert!(!self.slots[id.idx()].is_anchor);
        self.slots[id.idx()] = PgSlot::vacant();
        self.free_slots.push(id.0);
        let c = self.purgeable_counter();
        *c = c.wrapping_sub(1);
    }

    // ------------------------------------------------------------------
    // Funções gerais
    // ------------------------------------------------------------------

    /// `pcache1ResizeHash`: dobra a tabela hash (mínimo 256 posições),
    /// religando cada cadeia na ordem do C (inserção na frente).
    fn resize_hash(&mut self) {
        let mut n_new = self.n_hash.wrapping_mul(2);
        if n_new < 256 {
            n_new = 256;
        }
        let mut ap_new: Vec<Option<PgId>> = vec![None; n_new as usize];
        for i in 0..self.n_hash as usize {
            let mut p_next = self.ap_hash[i];
            while let Some(id) = p_next {
                let h = (self.slots[id.idx()].key % n_new) as usize;
                p_next = self.slots[id.idx()].next_hash;
                self.slots[id.idx()].next_hash = ap_new[h];
                ap_new[h] = Some(id);
            }
        }
        self.ap_hash = ap_new;
        self.n_hash = n_new;
    }

    /// `pcache1PinPage`: tira a página da LRU (fixa-a).
    fn pin_page(&mut self, id: PgId) {
        let (prev, next) = {
            let s = &self.slots[id.idx()];
            (s.lru_prev, s.lru_next)
        };
        let prev = prev.expect("pcache1PinPage: página fora da LRU");
        let next = next.expect("pcache1PinPage: página fora da LRU");
        self.slots[prev.idx()].lru_next = Some(next);
        self.slots[next.idx()].lru_prev = Some(prev);
        self.slots[id.idx()].lru_next = None;
        // lru_prev não precisa ser limpo: ninguém o lê com lru_next nulo.
        debug_assert!(!self.slots[id.idx()].is_anchor);
        debug_assert!(self.slots[0].is_anchor);
        self.n_recyclable -= 1;
    }

    /// `pcache1RemoveFromHash`: tira a página da tabela hash; com `free_flag`,
    /// também a libera.
    fn remove_from_hash(&mut self, id: PgId, free_flag: bool) {
        let h = (self.slots[id.idx()].key % self.n_hash) as usize;
        let mut prev: Option<PgId> = None;
        let mut cur = self.ap_hash[h];
        loop {
            match cur {
                Some(c) if c == id => break,
                Some(c) => {
                    prev = Some(c);
                    cur = self.slots[c.idx()].next_hash;
                }
                None => panic!("pcache1RemoveFromHash: página fora da tabela hash"),
            }
        }
        let next = self.slots[id.idx()].next_hash;
        match prev {
            None => self.ap_hash[h] = next,
            Some(pv) => self.slots[pv.idx()].next_hash = next,
        }
        self.n_page -= 1;
        if free_flag {
            self.free_page(id);
        }
    }

    /// `pcache1EnforceMaxPage`: se há mais de `nMaxPage` páginas alocadas,
    /// recicla da ponta antiga da LRU até chegar a `nMaxPage`. (A liberação do
    /// bloco `pBulk` do C não existe.)
    fn enforce_max_page(&mut self) {
        while self.n_purgeable > self.n_max_page {
            let p = self.slots[0].lru_prev.expect("âncora da LRU sem anterior");
            if self.slots[p.idx()].is_anchor {
                break;
            }
            debug_assert!(self.slots[p.idx()].lru_next.is_some());
            self.pin_page(p);
            self.remove_from_hash(p, true);
        }
    }

    /// `pcache1TruncateUnsafe`: descarta as páginas com chave maior ou igual a
    /// `i_limit`; as fixadas são desafixadas antes.
    fn truncate_unsafe(&mut self, i_limit: u32) {
        debug_assert!(self.i_max_key >= i_limit);
        debug_assert!(self.n_hash > 0);
        let mut h: u32;
        let i_stop: u32;
        if self.i_max_key.wrapping_sub(i_limit) < self.n_hash {
            // Só as últimas páginas saem: basta varrer as posições da tabela
            // que podem conter chaves a remover.
            h = i_limit % self.n_hash;
            i_stop = self.i_max_key % self.n_hash;
        } else {
            // Caso geral: varre a tabela inteira.
            h = self.n_hash / 2;
            i_stop = h.wrapping_sub(1);
        }
        loop {
            debug_assert!(h < self.n_hash);
            let hi = h as usize;
            let mut prev: Option<PgId> = None;
            let mut cur = self.ap_hash[hi];
            while let Some(id) = cur {
                let next = self.slots[id.idx()].next_hash;
                if self.slots[id.idx()].key >= i_limit {
                    self.n_page -= 1;
                    match prev {
                        None => self.ap_hash[hi] = next,
                        Some(pv) => self.slots[pv.idx()].next_hash = next,
                    }
                    if self.slots[id.idx()].lru_next.is_some() {
                        self.pin_page(id);
                    }
                    self.free_page(id);
                } else {
                    prev = Some(id);
                }
                cur = next;
            }
            if h == i_stop {
                break;
            }
            h = (h + 1) % self.n_hash;
        }
    }

    // ------------------------------------------------------------------
    // Métodos de sqlite3_pcache
    // ------------------------------------------------------------------

    /// `pcache1Create`. O `assert` do C exige `szPage` potência de dois entre
    /// 512 e 65536. O cache é o único membro do seu grupo (modo 1).
    pub fn create(sz_page: i32, sz_extra: i32, b_purgeable: bool) -> PCache1<E> {
        debug_assert!((sz_page & (sz_page - 1)) == 0 && (512..=65536).contains(&sz_page));
        debug_assert!(sz_extra < 300);
        let mut anchor = PgSlot::vacant();
        anchor.is_anchor = true;
        anchor.lru_next = Some(PgId(0));
        anchor.lru_prev = Some(PgId(0));
        let mut p = PCache1 {
            n_max_page: 0,
            n_min_page: 0,
            mx_pinned: 10,
            n_purgeable: 0,
            sz_page: sz_page as usize,
            b_purgeable,
            n_min: 0,
            n_max: 0,
            n90pct: 0,
            i_max_key: 0,
            n_purgeable_dummy: 0,
            n_recyclable: 0,
            n_page: 0,
            n_hash: 0,
            ap_hash: Vec::new(),
            slots: vec![anchor],
            free_slots: Vec::new(),
        };
        p.resize_hash();
        if b_purgeable {
            p.n_min = 10;
            p.n_min_page += p.n_min;
            p.mx_pinned = p.n_max_page.wrapping_add(10).wrapping_sub(p.n_min_page);
        }
        p
    }

    /// `pcache1Cachesize`: configura o limite `cache_size`.
    pub fn cachesize(&mut self, n_max: i32) {
        debug_assert!(n_max >= 0);
        if self.b_purgeable {
            let mut n = n_max as u32;
            let teto = MAX_CACHE_PAGES.wrapping_sub(self.n_max_page).wrapping_add(self.n_max);
            if n > teto {
                n = teto;
            }
            self.n_max_page = self.n_max_page.wrapping_add(n.wrapping_sub(self.n_max));
            self.mx_pinned = self.n_max_page.wrapping_add(10).wrapping_sub(self.n_min_page);
            self.n_max = n;
            self.n90pct = self.n_max.wrapping_mul(9) / 10;
            self.enforce_max_page();
        }
    }

    /// `pcache1Shrink`: libera o máximo de memória (todas as páginas recicláveis).
    pub fn shrink(&mut self) {
        if self.b_purgeable {
            let saved_max_page = self.n_max_page;
            self.n_max_page = 0;
            self.enforce_max_page();
            self.n_max_page = saved_max_page;
        }
    }

    /// `pcache1Pagecount`.
    pub fn pagecount(&self) -> u32 {
        self.n_page
    }

    /// `pcache1FetchStage2`: passos 3, 4 e 5 do algoritmo de `pcache1Fetch`.
    fn fetch_stage2(&mut self, i_key: u32, create_flag: i32) -> Option<PgId> {
        // Passo 3: aborta se createFlag é 1 e o cache está quase cheio.
        debug_assert!(self.n_page >= self.n_recyclable);
        let n_pinned = self.n_page - self.n_recyclable;
        debug_assert!(self.mx_pinned == self.n_max_page.wrapping_add(10).wrapping_sub(self.n_min_page));
        debug_assert!(self.n90pct == self.n_max.wrapping_mul(9) / 10);
        if create_flag == 1
            && (n_pinned >= self.mx_pinned
                || n_pinned >= self.n90pct
                || (under_memory_pressure() && self.n_recyclable < n_pinned))
        {
            return None;
        }

        if self.n_page >= self.n_hash {
            self.resize_hash();
        }
        debug_assert!(self.n_hash > 0 && !self.ap_hash.is_empty());

        // Passo 4: tenta reciclar uma página da LRU.
        let mut p_page: Option<PgId> = None;
        let oldest = self.slots[0].lru_prev.expect("âncora da LRU sem anterior");
        if self.b_purgeable
            && !self.slots[oldest.idx()].is_anchor
            && (self.n_page + 1 >= self.n_max || under_memory_pressure())
        {
            debug_assert!(self.slots[oldest.idx()].lru_next.is_some());
            self.remove_from_hash(oldest, false);
            self.pin_page(oldest);
            // No modo 1 a página reciclada é sempre deste cache: o `szAlloc` é
            // igual e `nPurgeable -= (pOther->bPurgeable - pCache->bPurgeable)`
            // subtrai zero.
            p_page = Some(oldest);
        }

        // Passo 5: se ainda não há buffer utilizável, aloca um novo.
        let id = match p_page {
            Some(id) => id,
            None => self.alloc_page(),
        };

        let h = (i_key % self.n_hash) as usize;
        self.n_page += 1;
        let next_hash = self.ap_hash[h];
        let s = &mut self.slots[id.idx()];
        s.key = i_key;
        s.next_hash = next_hash;
        s.lru_next = None;
        // lru_prev não precisa ser limpo: não é lido com lru_next nulo.
        // `*(void **)pPage->page.pExtra = 0` do C: o cabeçalho de pcache ainda
        // não foi inicializado (ver PCache::fetch_finish).
        s.is_init = false;
        self.ap_hash[h] = Some(id);
        if i_key > self.i_max_key {
            self.i_max_key = i_key;
        }
        Some(id)
    }

    /// `pcache1Fetch` (`pcache1FetchNoMutex`): busca uma página pela chave.
    /// `create_flag`: 0 não aloca; 1 aloca se há espaço fácil; 2 faz de tudo
    /// para alocar. A página achada é fixada (sai da LRU) se estava nela.
    pub fn fetch(&mut self, i_key: u32, create_flag: i32) -> Option<PgId> {
        debug_assert!(self.b_purgeable || create_flag != 1);
        debug_assert!(self.b_purgeable || self.n_min == 0);
        debug_assert!(!self.b_purgeable || self.n_min == 10);
        debug_assert!(self.n_min == 0 || self.b_purgeable);
        debug_assert!(self.n_hash > 0);

        // Passo 1: procura a chave na tabela hash.
        let mut p_page = self.ap_hash[(i_key % self.n_hash) as usize];
        while let Some(id) = p_page {
            if self.slots[id.idx()].key == i_key {
                break;
            }
            p_page = self.slots[id.idx()].next_hash;
        }

        // Passo 2: achou, devolve; não achou e createFlag 0, aborta; senão
        // segue para os passos 3, 4 e 5.
        match p_page {
            Some(id) => {
                if self.slots[id.idx()].lru_next.is_some() {
                    self.pin_page(id);
                }
                Some(id)
            }
            None if create_flag != 0 => self.fetch_stage2(i_key, create_flag),
            None => None,
        }
    }

    /// `pcache1Unpin`: marca a página como não fixada (elegível para
    /// reciclagem). Com `reuse_unlikely`, ou se há páginas demais, ela é
    /// descartada em vez de ir para a LRU.
    pub fn unpin(&mut self, id: PgId, reuse_unlikely: bool) {
        // É erro chamar isto com a página já na LRU.
        debug_assert!(self.slots[id.idx()].lru_next.is_none());
        if reuse_unlikely || self.n_purgeable > self.n_max_page {
            self.remove_from_hash(id, true);
        } else {
            // Insere a página na frente da LRU.
            let first = self.slots[0].lru_next.expect("âncora da LRU sem próximo");
            self.slots[id.idx()].lru_prev = Some(PgId(0));
            self.slots[id.idx()].lru_next = Some(first);
            self.slots[first.idx()].lru_prev = Some(id);
            self.slots[0].lru_next = Some(id);
            self.n_recyclable += 1;
        }
    }

    /// `pcache1Rekey`: troca a chave da página de `i_old` para `i_new`.
    pub fn rekey(&mut self, id: PgId, i_old: u32, i_new: u32) {
        debug_assert!(self.slots[id.idx()].key == i_old);
        debug_assert!(i_old != i_new);

        let h_old = (i_old % self.n_hash) as usize;
        let mut prev: Option<PgId> = None;
        let mut cur = self.ap_hash[h_old];
        while cur != Some(id) {
            let c = cur.expect("pcache1Rekey: página fora da tabela hash");
            prev = Some(c);
            cur = self.slots[c.idx()].next_hash;
        }
        let next = self.slots[id.idx()].next_hash;
        match prev {
            None => self.ap_hash[h_old] = next,
            Some(pv) => self.slots[pv.idx()].next_hash = next,
        }

        let h_new = (i_new % self.n_hash) as usize;
        let head = self.ap_hash[h_new];
        let s = &mut self.slots[id.idx()];
        s.key = i_new;
        s.next_hash = head;
        self.ap_hash[h_new] = Some(id);
        if i_new > self.i_max_key {
            self.i_max_key = i_new;
        }
    }

    /// `pcache1Truncate`: descarta as páginas com chave igual ou maior que
    /// `i_limit`; as fixadas ficam implicitamente desafixadas.
    pub fn truncate(&mut self, i_limit: u32) {
        if i_limit <= self.i_max_key {
            self.truncate_unsafe(i_limit);
            self.i_max_key = i_limit.wrapping_sub(1);
        }
    }

    /// `pcache1Destroy`. Como o grupo é só deste cache, os acertos de
    /// `nMaxPage`/`nMinPage` do C incidem num grupo que morre junto, mas são
    /// mantidos na mesma ordem.
    pub fn destroy(mut self) {
        debug_assert!(self.b_purgeable || (self.n_max == 0 && self.n_min == 0));
        if self.n_page != 0 {
            self.truncate_unsafe(0);
        }
        debug_assert!(self.n_max_page >= self.n_max);
        self.n_max_page = self.n_max_page.wrapping_sub(self.n_max);
        debug_assert!(self.n_min_page >= self.n_min);
        self.n_min_page = self.n_min_page.wrapping_sub(self.n_min);
        self.mx_pinned = self.n_max_page.wrapping_add(10).wrapping_sub(self.n_min_page);
        self.enforce_max_page();
    }
}

/// `sqlite3HeaderSizePcache1`: `ROUND8(sizeof(PgHdr1))` na x86-64 do C.
pub const fn header_size_pcache1() -> i32 {
    56
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_fetch_unpin_recycle() {
        let mut c: PCache1<u32> = PCache1::create(512, 8, true);
        c.cachesize(3);
        let a = c.fetch(1, 2).expect("cria");
        assert_eq!(c.pagecount(), 1);
        c.unpin(a, false);
        // Página não fixada volta pela mesma chave.
        let a2 = c.fetch(1, 0).expect("achada");
        assert_eq!(a, a2);
        c.unpin(a2, false);
        let b = c.fetch(2, 2).expect("cria");
        c.unpin(b, false);
        // nPage+1 >= nMax (3): a chave 3 recicla a mais antiga (1).
        let d = c.fetch(3, 2).expect("recicla");
        assert!(c.fetch(1, 0).is_none());
        assert_eq!(c.pagecount(), 2);
        c.unpin(d, true);
        assert_eq!(c.pagecount(), 1);
        c.truncate(1);
        assert_eq!(c.pagecount(), 0);
        c.destroy();
    }

    #[test]
    fn create_flag_one_refuses_when_pinned_over_limit() {
        let mut c: PCache1<u32> = PCache1::create(512, 8, true);
        c.cachesize(3);
        // n90pct = 2: com duas páginas fixadas, createFlag 1 recusa.
        assert!(c.fetch(1, 1).is_some());
        assert!(c.fetch(2, 1).is_some());
        assert!(c.fetch(3, 1).is_none());
        assert!(c.fetch(3, 2).is_some());
    }

    #[test]
    fn rekey_moves_between_buckets() {
        let mut c: PCache1<u32> = PCache1::create(512, 8, false);
        let a = c.fetch(5, 2).expect("cria");
        c.rekey(a, 5, 300);
        assert!(c.fetch(5, 0).is_none());
        assert_eq!(c.fetch(300, 0), Some(a));
    }
}
