//! Cache de páginas por conexão de arquivo (pcache.h, pcache.c).
//!
//! Modelo (CONVENTIONS.md, item 2): o `PCache<E>` POSSUI os slots das páginas;
//! uma página é o índice `PgId` de um slot. `E` é o conteúdo do `pExtra` do C
//! (o `MemPage` do btree, definido depois); ele começa em `E::default()` toda
//! vez que o slot é inicializado, no lugar dos 8 bytes zerados do C.
//!
//! O C separa o cache em duas camadas: pcache.c (lista de páginas sujas, contagem
//! de referências, políticas de criação) e pcache1.c (tabela hash, LRU,
//! reciclagem), ligadas por `sqlite3_pcache_methods2`. Aqui a camada de baixo
//! é `crate::pcache1::PCache1<E>`, campo `p1` do `PCache<E>`; o slot (`PgSlot`)
//! une o `PgHdr1` (campos de pcache1) e o `PgHdr` (campos de pcache), que no C
//! ficam colados na mesma alocação.
//!
//! Spill (`xStress`): no C o cache chama de volta o pager (`pagerStress`) no
//! meio do `sqlite3PcacheFetchStress`. Um callback `&mut dyn FnMut` que
//! recebesse o pager inteiro não fecha o empréstimo (o `PCache` é campo do
//! pager), então a função se divide em passos que o pager encadeia, mantendo
//! `&mut self` livre entre eles:
//!
//! 1. `fetch_stress_prepare()` decide o que fazer (nada, escolher vítima ou
//!    seguir direto) e devolve a página sugerida para o spill;
//! 2. o pager chama o seu `pager_stress(vítima)` com acesso total ao pager;
//! 3. `fetch_stress_complete(pgno)` faz o último passo do C (fetch com
//!    `createFlag == 2`).
//!
//! `fetch_stress(pgno, stress)` encadeia os três com um closure, para quando o
//! callback não precisa do pager. O erro do `xStress` diferente de `SQLITE_OK`
//! e `SQLITE_BUSY` aborta, como no C.
//!
//! Alocador de páginas: `Vec<u8>` simples. `SQLITE_CONFIG_PAGECACHE`,
//! alocação em bloco (`pBulk`) e lookaside somem.

use crate::consts::{SQLITE_BUSY, SQLITE_NOMEM_BKPT, SQLITE_OK};
use crate::pcache1::PCache1;

/// Página não está na lista de sujas.
pub const PGHDR_CLEAN: u16 = 0x001;
/// Página está na lista de sujas.
pub const PGHDR_DIRTY: u16 = 0x002;
/// Registrada no journal e pronta para modificação.
pub const PGHDR_WRITEABLE: u16 = 0x004;
/// Sincronizar o journal antes de gravar esta página no banco.
pub const PGHDR_NEED_SYNC: u16 = 0x008;
/// Não gravar o conteúdo em disco.
pub const PGHDR_DONT_WRITE: u16 = 0x010;
/// Objeto de página mapeado em memória.
pub const PGHDR_MMAP: u16 = 0x020;
/// Anexada ao arquivo WAL.
pub const PGHDR_WAL_APPEND: u16 = 0x040;

/// `PCACHE_DIRTYLIST_*`: argumento de `manage_dirty_list`.
const PCACHE_DIRTYLIST_REMOVE: u8 = 1;
const PCACHE_DIRTYLIST_ADD: u8 = 2;
const PCACHE_DIRTYLIST_FRONT: u8 = 3;

/// Número de baldes do ordenador por intercalação da lista suja.
const N_SORT_BUCKET: usize = 32;

/// Handle de uma página no `PCache` que a possui (índice do slot). Só vale
/// para o `PCache` que o devolveu e enquanto a página não for descartada
/// (`drop_page`, reciclagem ou truncamento).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PgId(pub u32);

impl PgId {
    #[inline]
    pub(crate) fn idx(self) -> usize {
        self.0 as usize
    }
}

/// Uma linha do cache: `PgHdr1` (pcache1) mais `PgHdr` (pcache), mais os bytes
/// da página (`pBuf`). Os campos são do crate; o pager usa os acessores do
/// `PCache`.
pub struct PgSlot<E> {
    // ---- PgHdr1 (pcache1.c) ----
    /// `iKey`: número da página.
    pub(crate) key: u32,
    /// `isAnchor`: este é o nó âncora da LRU (o `PGroup.lru`).
    pub(crate) is_anchor: bool,
    /// `pNext`: próximo na cadeia da tabela hash.
    pub(crate) next_hash: Option<PgId>,
    /// `pLruNext`: próximo na LRU circular. `None` significa página fixada (pinned).
    pub(crate) lru_next: Option<PgId>,
    /// `pLruPrev`: anterior na LRU; só vale quando `lru_next` é `Some`.
    pub(crate) lru_prev: Option<PgId>,
    /// `pBuf`: conteúdo da página.
    pub(crate) data: Vec<u8>,
    // ---- PgHdr (pcache.c) ----
    /// `pPage != 0`: o cabeçalho de pcache já foi inicializado por `fetch_finish`.
    pub(crate) is_init: bool,
    /// `pExtra`.
    pub(crate) extra: E,
    /// `pgno`.
    pub(crate) pgno: u32,
    /// `flags` (`PGHDR_*`).
    pub(crate) flags: u16,
    /// `nRef`.
    pub(crate) n_ref: i64,
    /// `pDirty`: lista transitória ordenada por `pgno`, montada por `dirty_list`.
    pub(crate) p_dirty: Option<PgId>,
    /// `pDirtyNext`: próxima da lista suja em ordem LRU.
    pub(crate) p_dirty_next: Option<PgId>,
    /// `pDirtyPrev`: anterior da lista suja em ordem LRU.
    pub(crate) p_dirty_prev: Option<PgId>,
}

impl<E: Default> PgSlot<E> {
    /// Slot sem página (livre ou âncora): sem buffer, nada inicializado.
    pub(crate) fn vacant() -> PgSlot<E> {
        PgSlot {
            key: 0,
            is_anchor: false,
            next_hash: None,
            lru_next: None,
            lru_prev: None,
            data: Vec::new(),
            is_init: false,
            extra: E::default(),
            pgno: 0,
            flags: 0,
            n_ref: 0,
            p_dirty: None,
            p_dirty_next: None,
            p_dirty_prev: None,
        }
    }
}

/// Resultado do primeiro passo de `fetch_stress`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StressStep {
    /// `eCreate == 2`: o C devolve 0 sem página (o pager trata como falta de memória).
    NoStress,
    /// Página suja não referenciada escolhida para o spill: o pager deve chamar
    /// o seu `xStress` com ela. `SQLITE_OK` e `SQLITE_BUSY` seguem; outro erro aborta.
    Spill(PgId),
    /// Nada a espalhar (cache abaixo de `szSpill` ou sem vítima): segue para o fetch.
    NoVictim,
}

/// Cache de páginas (`PCache` do C).
pub struct PCache<E> {
    /// `pDirty`: início (mais nova) da lista suja em ordem LRU.
    p_dirty: Option<PgId>,
    /// `pDirtyTail`: fim (mais antiga) da lista suja.
    p_dirty_tail: Option<PgId>,
    /// `pSynced`: última página da lista suja sem `PGHDR_NEED_SYNC` (otimização).
    p_synced: Option<PgId>,
    /// `nRefSum`: soma das contagens de referência de todas as páginas.
    n_ref_sum: i64,
    /// `szCache`: tamanho configurado do cache.
    sz_cache: i32,
    /// `szSpill`: tamanho a partir do qual o cache tenta espalhar páginas sujas.
    sz_spill: i32,
    /// `szPage`.
    sz_page: i32,
    /// `szExtra`: o `sizeof(MemPage)` do C. Entra só na aritmética de
    /// `number_of_cache_pages` e `set_spill_size`; o `E` do Rust não tem esse tamanho.
    sz_extra: i32,
    /// `bPurgeable`: as páginas têm arquivo de apoio.
    b_purgeable: bool,
    /// `eCreate`: 2 se criar página é fácil, 1 se há páginas sujas num cache purgável.
    e_create: u8,
    /// Cache pluggable (pcache1).
    p1: PCache1<E>,
}

impl<E: Default> PCache<E> {
    // ------------------------------------------------------------------
    // Acesso aos slots
    // ------------------------------------------------------------------

    #[inline]
    fn sl(&self, id: PgId) -> &PgSlot<E> {
        &self.p1.slots[id.idx()]
    }

    #[inline]
    fn sl_mut(&mut self, id: PgId) -> &mut PgSlot<E> {
        &mut self.p1.slots[id.idx()]
    }

    /// Bytes da página.
    pub fn page_data(&self, id: PgId) -> &[u8] {
        &self.sl(id).data
    }

    /// Bytes da página, para escrita.
    pub fn page_data_mut(&mut self, id: PgId) -> &mut [u8] {
        &mut self.sl_mut(id).data
    }

    /// `pExtra`.
    pub fn page_extra(&self, id: PgId) -> &E {
        &self.sl(id).extra
    }

    /// `pExtra`, para escrita.
    pub fn page_extra_mut(&mut self, id: PgId) -> &mut E {
        &mut self.sl_mut(id).extra
    }

    /// Empréstimo dividido: metadados e bytes da mesma página ao mesmo tempo.
    pub fn page_parts(&mut self, id: PgId) -> (&mut E, &mut [u8]) {
        let s = self.sl_mut(id);
        (&mut s.extra, s.data.as_mut_slice())
    }

    /// `pgno`.
    pub fn page_pgno(&self, id: PgId) -> u32 {
        self.sl(id).pgno
    }

    /// `flags` (`PGHDR_*`).
    pub fn page_flags(&self, id: PgId) -> u16 {
        self.sl(id).flags
    }

    /// `flags` para escrita (o pager faz `pPg->flags |= PGHDR_NEED_SYNC` e afins).
    pub fn page_flags_mut(&mut self, id: PgId) -> &mut u16 {
        &mut self.sl_mut(id).flags
    }

    /// `sqlite3PcachePageRefcount`: referências à página.
    pub fn page_ref_count(&self, id: PgId) -> i64 {
        self.sl(id).n_ref
    }

    // ------------------------------------------------------------------
    // Lista suja
    // ------------------------------------------------------------------

    /// `pcacheManageDirtyList`: participação da página na lista suja. O bit 1
    /// de `add_remove` retira a página da lista; o bit 2 a coloca na frente.
    /// Os dois juntos movem a página para a frente.
    fn manage_dirty_list(&mut self, id: PgId, add_remove: u8) {
        if add_remove & PCACHE_DIRTYLIST_REMOVE != 0 {
            let (next, prev) = {
                let s = self.sl(id);
                (s.p_dirty_next, s.p_dirty_prev)
            };
            debug_assert!(next.is_some() || self.p_dirty_tail == Some(id));
            debug_assert!(prev.is_some() || self.p_dirty == Some(id));

            // Atualiza PCache.pSynced se necessário.
            if self.p_synced == Some(id) {
                self.p_synced = prev;
            }

            match next {
                Some(n) => self.sl_mut(n).p_dirty_prev = prev,
                None => {
                    debug_assert!(self.p_dirty_tail == Some(id));
                    self.p_dirty_tail = prev;
                }
            }
            match prev {
                Some(pv) => self.sl_mut(pv).p_dirty_next = next,
                None => {
                    // Sem páginas sujas, eCreate volta a 2: sqlite3PcacheFetch()
                    // pode pular a busca por página suja para ejetar.
                    debug_assert!(self.p_dirty == Some(id));
                    debug_assert!(self.b_purgeable || self.e_create == 2);
                    self.p_dirty = next;
                    if self.p_dirty.is_none() {
                        debug_assert!(!self.b_purgeable || self.e_create == 1);
                        self.e_create = 2;
                    }
                }
            }
        }
        if add_remove & PCACHE_DIRTYLIST_ADD != 0 {
            let head = self.p_dirty;
            {
                let s = self.sl_mut(id);
                s.p_dirty_prev = None;
                s.p_dirty_next = head;
            }
            match head {
                Some(h) => {
                    debug_assert!(self.sl(h).p_dirty_prev.is_none());
                    self.sl_mut(h).p_dirty_prev = Some(id);
                }
                None => {
                    self.p_dirty_tail = Some(id);
                    if self.b_purgeable {
                        debug_assert!(self.e_create == 2);
                        self.e_create = 1;
                    }
                }
            }
            self.p_dirty = Some(id);

            // Se pSynced é nulo e a página tem NEED_SYNC limpo, ela vira pSynced.
            // A checagem do flag é otimização: com pSynced apontando para página
            // com NEED_SYNC, fetch_stress já procura entre as mais novas.
            if self.p_synced.is_none() && (self.sl(id).flags & PGHDR_NEED_SYNC) == 0 {
                self.p_synced = Some(id);
            }
        }
    }

    /// `pcacheUnpin`: cache em memória (não purgável) não usa a LRU.
    fn unpin(&mut self, id: PgId) {
        if self.b_purgeable {
            self.p1.unpin(id, false);
        }
    }

    /// `numberOfCachePages`: páginas pedidas. Com `szCache` negativo, o tamanho
    /// em KiB do `PRAGMA cache_size` vira número de páginas pelo tamanho atual.
    fn number_of_cache_pages(&self) -> i32 {
        if self.sz_cache >= 0 {
            self.sz_cache
        } else {
            let mut n = (-1024i64 * self.sz_cache as i64) / (self.sz_page as i64 + self.sz_extra as i64);
            if n > 1000000000 {
                n = 1000000000;
            }
            n as i32
        }
    }

    // ------------------------------------------------------------------
    // Abertura, fechamento, configuração
    // ------------------------------------------------------------------

    /// `sqlite3PcacheOpen`. O `xStress`/`pStress` do C não existem: o spill é
    /// conduzido pelo pager (ver o cabeçalho do módulo). `sz_extra` deve ser o
    /// `sizeof(MemPage)` do C (arredondado como o pager faz), porque entra na
    /// conta do `cache_size` negativo.
    pub fn open(sz_page: i32, sz_extra: i32, b_purgeable: bool) -> PCache<E> {
        debug_assert!(sz_extra >= 8);
        let mut p = PCache {
            p_dirty: None,
            p_dirty_tail: None,
            p_synced: None,
            n_ref_sum: 0,
            sz_cache: 100,
            sz_spill: 1,
            sz_page: 1,
            sz_extra,
            b_purgeable,
            e_create: 2,
            p1: PCache1::create(sz_page, sz_extra, b_purgeable),
        };
        // Último passo de sqlite3PcacheSetPageSize para o cache recém-aberto: o
        // cache de baixo já foi criado acima; falta o xCachesize e o szPage.
        let n = p.number_of_cache_pages();
        p.p1.cachesize(n);
        p.sz_page = sz_page;
        p
    }

    /// `sqlite3PcacheSetPageSize`: o chamador garante que não há referências
    /// nem páginas sujas. O cache de baixo é recriado e as páginas em cache se
    /// perdem. A conta de páginas usa o `szPage` antigo, como no C.
    pub fn set_page_size(&mut self, sz_page: i32) -> i32 {
        debug_assert!(self.n_ref_sum == 0 && self.p_dirty.is_none());
        let mut p_new = PCache1::create(sz_page, self.sz_extra, self.b_purgeable);
        p_new.cachesize(self.number_of_cache_pages());
        self.p1 = p_new;
        self.sz_page = sz_page;
        SQLITE_OK
    }

    /// `sqlite3PcacheClose`.
    pub fn close(self) {
        self.p1.destroy();
    }

    /// `sqlite3PcacheClear`: descarta o conteúdo do cache.
    pub fn clear(&mut self) {
        self.truncate(0);
    }

    /// `sqlite3PcacheSetCachesize`.
    pub fn set_cache_size(&mut self, mx_page: i32) {
        self.sz_cache = mx_page;
        let n = self.number_of_cache_pages();
        self.p1.cachesize(n);
    }

    /// `sqlite3PcacheSetSpillsize`: sem mudança se o argumento é zero. Devolve o
    /// tamanho efetivo, o maior entre `szSpill` e o tamanho do cache.
    pub fn set_spill_size(&mut self, mut mx_page: i32) -> i32 {
        if mx_page != 0 {
            if mx_page < 0 {
                mx_page = ((-1024i64 * mx_page as i64) / (self.sz_page as i64 + self.sz_extra as i64)) as i32;
            }
            self.sz_spill = mx_page;
        }
        let mut res = self.number_of_cache_pages();
        if res < self.sz_spill {
            res = self.sz_spill;
        }
        res
    }

    /// `sqlite3PcacheShrink`: libera o máximo de memória possível.
    pub fn shrink(&mut self) {
        self.p1.shrink();
    }

    // ------------------------------------------------------------------
    // Busca de páginas
    // ------------------------------------------------------------------

    /// `sqlite3PcacheFetch`: primeira etapa. Devolve o slot da página se ela
    /// está no cache ou pôde ser criada; `None` caso contrário. O slot sai
    /// fixado (pinned) mas ainda não referenciado nem inicializado: o chamador
    /// precisa chamar `fetch_finish`. `create_flag` é 0 (só procura) ou 3
    /// (tenta criar).
    pub fn fetch(&mut self, pgno: u32, create_flag: i32) -> Option<PgId> {
        debug_assert!(create_flag == 3 || create_flag == 0);
        debug_assert!(
            self.e_create == if self.b_purgeable && self.p_dirty.is_some() { 1 } else { 2 }
        );

        // eCreate: 0 não aloca; 1 aloca se for barato (purgável com páginas
        // sujas); 2 aloca mesmo que seja difícil.
        let e_create = create_flag & self.e_create as i32;
        debug_assert!(e_create == 0 || e_create == 1 || e_create == 2);
        debug_assert!(create_flag == 0 || self.e_create as i32 == e_create);
        debug_assert!(
            create_flag == 0
                || e_create == 1 + (!self.b_purgeable || self.p_dirty.is_none()) as i32
        );
        self.p1.fetch(pgno, e_create)
    }

    /// `sqlite3PcacheFetchStress`, passo 1: decide o spill. Ver `StressStep`.
    /// Atualiza `pSynced` como o C.
    pub fn fetch_stress_prepare(&mut self) -> StressStep {
        if self.e_create == 2 {
            return StressStep::NoStress;
        }
        if self.page_count() > self.sz_spill {
            // Procura uma página suja para gravar e reciclar. Primeiro uma sem
            // PGHDR_NEED_SYNC (não exige sync do journal); se não houver, qualquer
            // página suja sem referências. Se a mais antiga sem NEED_SYNC estiver
            // referenciada, pSynced pode ficar errado, o que é aceitável: é só
            // uma otimização.
            let mut p = self.p_synced;
            while let Some(id) = p {
                let s = self.sl(id);
                if s.n_ref != 0 || (s.flags & PGHDR_NEED_SYNC) != 0 {
                    p = s.p_dirty_prev;
                } else {
                    break;
                }
            }
            self.p_synced = p;
            if p.is_none() {
                p = self.p_dirty_tail;
                while let Some(id) = p {
                    let s = self.sl(id);
                    if s.n_ref != 0 {
                        p = s.p_dirty_prev;
                    } else {
                        break;
                    }
                }
            }
            if let Some(id) = p {
                return StressStep::Spill(id);
            }
        }
        StressStep::NoVictim
    }

    /// `sqlite3PcacheFetchStress`, passo 3: tenta de novo, agora com
    /// `createFlag == 2` (só falha por falta de memória). Como a primeira
    /// etapa, devolve o slot ainda sem `fetch_finish`.
    pub fn fetch_stress_complete(&mut self, pgno: u32) -> Option<PgId> {
        self.p1.fetch(pgno, 2)
    }

    /// `sqlite3PcacheFetchStress` inteira, com o `xStress` como closure (que
    /// recebe o cache e a vítima). `Ok(None)` é o retorno 0 do C sem página
    /// (`eCreate == 2`): o pager o trata como `SQLITE_NOMEM`. `Err(rc)` é o erro
    /// do `xStress` (qualquer um além de `SQLITE_OK` e `SQLITE_BUSY`) ou
    /// `SQLITE_NOMEM` se a alocação final falhar.
    pub fn fetch_stress(
        &mut self,
        pgno: u32,
        stress: &mut dyn FnMut(&mut PCache<E>, PgId) -> i32,
    ) -> Result<Option<PgId>, i32> {
        match self.fetch_stress_prepare() {
            StressStep::NoStress => return Ok(None),
            StressStep::Spill(victim) => {
                let rc = stress(self, victim);
                if rc != SQLITE_OK && rc != SQLITE_BUSY {
                    return Err(rc);
                }
            }
            StressStep::NoVictim => {}
        }
        match self.fetch_stress_complete(pgno) {
            None => Err(SQLITE_NOMEM_BKPT),
            p => Ok(p),
        }
    }

    /// `sqlite3PcacheFetchFinish` (com `pcacheFetchFinishWithInit`): converte o
    /// slot devolvido por `fetch` no objeto de página, inicializando-o se for
    /// novo, e conta a referência.
    pub fn fetch_finish(&mut self, pgno: u32, page: PgId) -> PgId {
        let s = self.sl_mut(page);
        if !s.is_init {
            debug_assert!(s.n_ref == 0);
            s.p_dirty = None;
            s.p_dirty_next = None;
            s.p_dirty_prev = None;
            s.n_ref = 0;
            s.flags = PGHDR_CLEAN;
            s.is_init = true;
            s.extra = E::default();
            s.pgno = pgno;
        }
        self.n_ref_sum += 1;
        self.sl_mut(page).n_ref += 1;
        page
    }

    // ------------------------------------------------------------------
    // Referências, sujo e limpo
    // ------------------------------------------------------------------

    /// `sqlite3PcacheRelease`: solta uma referência. Página limpa sem
    /// referências vira reciclável; suja vai para a frente da lista suja.
    pub fn release(&mut self, id: PgId) {
        debug_assert!(self.sl(id).n_ref > 0);
        self.n_ref_sum -= 1;
        let s = self.sl_mut(id);
        s.n_ref -= 1;
        if s.n_ref == 0 {
            if s.flags & PGHDR_CLEAN != 0 {
                self.unpin(id);
            } else {
                self.manage_dirty_list(id, PCACHE_DIRTYLIST_FRONT);
            }
        }
    }

    /// `sqlite3PcacheRef`: mais uma referência a uma página já referenciada.
    pub fn page_ref(&mut self, id: PgId) {
        debug_assert!(self.sl(id).n_ref > 0);
        self.sl_mut(id).n_ref += 1;
        self.n_ref_sum += 1;
    }

    /// `sqlite3PcacheDrop`: tira a página do cache. Deve haver exatamente uma
    /// referência; ela é consumida e o `PgId` fica inválido.
    pub fn drop_page(&mut self, id: PgId) {
        debug_assert!(self.sl(id).n_ref == 1);
        if self.sl(id).flags & PGHDR_DIRTY != 0 {
            self.manage_dirty_list(id, PCACHE_DIRTYLIST_REMOVE);
        }
        self.n_ref_sum -= 1;
        self.p1.unpin(id, true);
    }

    /// `sqlite3PcacheMakeDirty`.
    pub fn make_dirty(&mut self, id: PgId) {
        debug_assert!(self.sl(id).n_ref > 0);
        let flags = self.sl(id).flags;
        if flags & (PGHDR_CLEAN | PGHDR_DONT_WRITE) != 0 {
            self.sl_mut(id).flags &= !PGHDR_DONT_WRITE;
            if flags & PGHDR_CLEAN != 0 {
                self.sl_mut(id).flags ^= PGHDR_DIRTY | PGHDR_CLEAN;
                debug_assert!(self.sl(id).flags & (PGHDR_DIRTY | PGHDR_CLEAN) == PGHDR_DIRTY);
                self.manage_dirty_list(id, PCACHE_DIRTYLIST_ADD);
            }
        }
    }

    /// `sqlite3PcacheMakeClean`.
    pub fn make_clean(&mut self, id: PgId) {
        debug_assert!(self.sl(id).flags & PGHDR_DIRTY != 0);
        debug_assert!(self.sl(id).flags & PGHDR_CLEAN == 0);
        self.manage_dirty_list(id, PCACHE_DIRTYLIST_REMOVE);
        let s = self.sl_mut(id);
        s.flags &= !(PGHDR_DIRTY | PGHDR_NEED_SYNC | PGHDR_WRITEABLE);
        s.flags |= PGHDR_CLEAN;
        if s.n_ref == 0 {
            self.unpin(id);
        }
    }

    /// `sqlite3PcacheCleanAll`: limpa todas as páginas sujas.
    pub fn clean_all(&mut self) {
        while let Some(p) = self.p_dirty {
            self.make_clean(p);
        }
    }

    /// `sqlite3PcacheClearWritable`: limpa NEED_SYNC e WRITEABLE de todas as sujas.
    pub fn clear_writable(&mut self) {
        let mut p = self.p_dirty;
        while let Some(id) = p {
            let s = self.sl_mut(id);
            s.flags &= !(PGHDR_NEED_SYNC | PGHDR_WRITEABLE);
            p = s.p_dirty_next;
        }
        self.p_synced = self.p_dirty_tail;
    }

    /// `sqlite3PcacheClearSyncFlags`: limpa NEED_SYNC de todas as sujas.
    pub fn clear_sync_flags(&mut self) {
        let mut p = self.p_dirty;
        while let Some(id) = p {
            let s = self.sl_mut(id);
            s.flags &= !PGHDR_NEED_SYNC;
            p = s.p_dirty_next;
        }
        self.p_synced = self.p_dirty_tail;
    }

    /// `sqlite3PcacheMove`: muda o número da página (usado pelo incr-vacuum).
    /// Se já existe página em `new_pgno`, ela é descartada.
    pub fn move_page(&mut self, id: PgId, new_pgno: u32) {
        debug_assert!(self.sl(id).n_ref > 0);
        debug_assert!(new_pgno > 0);
        if let Some(other) = self.p1.fetch(new_pgno, 0) {
            debug_assert!(self.sl(other).n_ref == 0);
            self.sl_mut(other).n_ref += 1;
            self.n_ref_sum += 1;
            self.drop_page(other);
        }
        let old = self.sl(id).pgno;
        self.p1.rekey(id, old, new_pgno);
        self.sl_mut(id).pgno = new_pgno;
        let flags = self.sl(id).flags;
        if flags & PGHDR_DIRTY != 0 && flags & PGHDR_NEED_SYNC != 0 {
            self.manage_dirty_list(id, PCACHE_DIRTYLIST_FRONT);
        }
    }

    /// `sqlite3PcacheTruncate`: descarta toda página com número maior que
    /// `pgno`. O chamador garante que nenhuma delas tem referência, salvo a
    /// página 1. Com `pgno == 0` e a página 1 referenciada, os bytes dela são
    /// zerados mas o objeto fica.
    pub fn truncate(&mut self, mut pgno: u32) {
        let mut p = self.p_dirty;
        while let Some(id) = p {
            let next = self.sl(id).p_dirty_next;
            // Com pgno positivo isto só roda logo depois de clean_all(); então
            // se há páginas sujas, pgno == 0.
            debug_assert!(self.sl(id).pgno > 0);
            if self.sl(id).pgno > pgno {
                debug_assert!(self.sl(id).flags & PGHDR_DIRTY != 0);
                self.make_clean(id);
            }
            p = next;
        }
        if pgno == 0 && self.n_ref_sum != 0 {
            // A página 1 sempre está no cache, porque nRefSum > 0.
            if let Some(page1) = self.p1.fetch(1, 0) {
                self.sl_mut(page1).data.fill(0);
                pgno = 1;
            }
        }
        self.p1.truncate(pgno.wrapping_add(1));
    }

    // ------------------------------------------------------------------
    // Consultas
    // ------------------------------------------------------------------

    /// `sqlite3PcacheDirtyList`: páginas sujas em ordem crescente de `pgno`
    /// (intercalação em baldes do C, sobre o campo `p_dirty`).
    pub fn dirty_list(&mut self) -> Vec<PgId> {
        let mut p = self.p_dirty;
        while let Some(id) = p {
            let s = self.sl_mut(id);
            s.p_dirty = s.p_dirty_next;
            p = s.p_dirty_next;
        }
        let mut cur = self.sort_dirty_list(self.p_dirty);
        let mut out = Vec::new();
        while let Some(id) = cur {
            out.push(id);
            cur = self.sl(id).p_dirty;
        }
        out
    }

    /// `pcacheMergeDirtyList`: intercala duas listas ligadas por `p_dirty`,
    /// ordenadas por `pgno`. Não conserta `p_dirty_prev`.
    fn merge_dirty_list(&mut self, mut a: PgId, mut b: PgId) -> PgId {
        let mut head: Option<PgId> = None;
        let mut tail: Option<PgId> = None;
        loop {
            if self.sl(a).pgno < self.sl(b).pgno {
                self.append_dirty(&mut head, &mut tail, a);
                match self.sl(a).p_dirty {
                    None => {
                        self.sl_mut(a).p_dirty = Some(b);
                        break;
                    }
                    Some(n) => a = n,
                }
            } else {
                self.append_dirty(&mut head, &mut tail, b);
                match self.sl(b).p_dirty {
                    None => {
                        self.sl_mut(b).p_dirty = Some(a);
                        break;
                    }
                    Some(n) => b = n,
                }
            }
        }
        head.unwrap_or(a)
    }

    /// `pTail->pDirty = x; pTail = x` do `pcacheMergeDirtyList` (o `result`
    /// fictício do C é o `head`).
    fn append_dirty(&mut self, head: &mut Option<PgId>, tail: &mut Option<PgId>, x: PgId) {
        match *tail {
            None => *head = Some(x),
            Some(t) => self.sl_mut(t).p_dirty = Some(x),
        }
        *tail = Some(x);
    }

    /// `pcacheSortDirtyList`: ordena por `pgno` as páginas ligadas por `p_dirty`.
    /// Não pode haver mais de 2^31 páginas, logo bastam 31 baldes; o 32º pega
    /// o excesso se isso mudar.
    fn sort_dirty_list(&mut self, p_in: Option<PgId>) -> Option<PgId> {
        let mut a: [Option<PgId>; N_SORT_BUCKET] = [None; N_SORT_BUCKET];
        let mut p_in = p_in;
        while let Some(first) = p_in {
            let mut p = first;
            p_in = self.sl(p).p_dirty;
            self.sl_mut(p).p_dirty = None;
            let mut i = 0;
            while i < N_SORT_BUCKET - 1 {
                match a[i] {
                    None => {
                        a[i] = Some(p);
                        break;
                    }
                    Some(bucket) => {
                        p = self.merge_dirty_list(bucket, p);
                        a[i] = None;
                    }
                }
                i += 1;
            }
            if i == N_SORT_BUCKET - 1 {
                // Impossível: exigiria 2^N_SORT_BUCKET elementos.
                if let Some(bucket) = a[i] {
                    a[i] = Some(self.merge_dirty_list(bucket, p));
                } else {
                    a[i] = Some(p);
                }
            }
        }
        let mut p = a[0];
        for bucket in a.iter().skip(1) {
            if let Some(b) = *bucket {
                p = match p {
                    Some(cur) => Some(self.merge_dirty_list(cur, b)),
                    None => Some(b),
                };
            }
        }
        p
    }

    /// `sqlite3PcacheRefCount`: soma das referências de todas as páginas (não o
    /// número de páginas referenciadas).
    pub fn ref_count(&self) -> i64 {
        self.n_ref_sum
    }

    /// `sqlite3PcachePagecount`: número de páginas no cache.
    pub fn page_count(&self) -> i32 {
        self.p1.pagecount() as i32
    }

    /// `sqlite3PCachePercentDirty`: páginas sujas em porcentagem do tamanho do cache.
    pub fn percent_dirty(&self) -> i32 {
        let mut n_dirty: i64 = 0;
        let n_cache = self.number_of_cache_pages();
        let mut p = self.p_dirty;
        while let Some(id) = p {
            n_dirty += 1;
            p = self.sl(id).p_dirty_next;
        }
        if n_cache != 0 {
            ((n_dirty * 100) / n_cache as i64) as i32
        } else {
            0
        }
    }

    /// `sqlite3PCacheIsDirty` (`SQLITE_DIRECT_OVERFLOW_READ`): há páginas sujas?
    pub fn is_dirty(&self) -> bool {
        self.p_dirty.is_some()
    }

    /// `sqlite3PcacheIterateDirty`: percorre as páginas sujas em ordem LRU
    /// (da mais nova para a mais antiga).
    pub fn iter_dirty(&self) -> impl Iterator<Item = PgId> + '_ {
        let mut cur = self.p_dirty;
        std::iter::from_fn(move || {
            let id = cur?;
            cur = self.sl(id).p_dirty_next;
            Some(id)
        })
    }
}

/// `sqlite3HeaderSizePcache`: `ROUND8(sizeof(PgHdr))` na x86-64 do C, valor que
/// `SQLITE_CONFIG_PCACHE_HDRSZ` soma ao do btree e ao de pcache1.
pub const fn header_size_pcache() -> i32 {
    80
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(pc: &mut PCache<u32>, pgno: u32) -> PgId {
        let raw = pc.fetch(pgno, 3).expect("página criada");
        pc.fetch_finish(pgno, raw)
    }

    #[test]
    fn dirty_list_is_sorted_and_clean_all_works() {
        let mut pc: PCache<u32> = PCache::open(512, 8, true);
        let mut ids = Vec::new();
        for pgno in [5u32, 2, 9, 1, 7] {
            let id = get(&mut pc, pgno);
            pc.make_dirty(id);
            ids.push(id);
        }
        assert_eq!(pc.ref_count(), 5);
        let order: Vec<u32> = pc.dirty_list().iter().map(|&i| pc.page_pgno(i)).collect();
        assert_eq!(order, vec![1, 2, 5, 7, 9]);
        let lru: Vec<u32> = pc.iter_dirty().map(|i| pc.page_pgno(i)).collect();
        assert_eq!(lru, vec![7, 1, 9, 2, 5]);
        for id in ids {
            pc.release(id);
        }
        assert_eq!(pc.ref_count(), 0);
        assert!(pc.is_dirty());
        pc.clean_all();
        assert!(!pc.is_dirty());
        assert!(pc.dirty_list().is_empty());
    }

    #[test]
    fn lru_recycles_oldest_clean_page() {
        let mut pc: PCache<u32> = PCache::open(512, 8, true);
        pc.set_cache_size(3);
        for pgno in 1..=4u32 {
            let id = get(&mut pc, pgno);
            pc.release(id);
        }
        assert!(pc.page_count() <= 3);
        // A página 1 era a mais antiga na LRU e foi reciclada.
        assert!(pc.fetch(1, 0).is_none());
        // A mais nova continua.
        assert!(pc.fetch(4, 0).is_some());
    }

    #[test]
    fn fetch_stress_picks_unreferenced_dirty_page() {
        let mut pc: PCache<u32> = PCache::open(512, 8, true);
        let a = get(&mut pc, 1);
        pc.make_dirty(a);
        pc.release(a);
        // Há página suja: eCreate == 1, então o spill é possível.
        pc.set_spill_size(0);
        assert_eq!(pc.page_count(), 1);
        // pageCount (1) não excede szSpill (1): sem vítima.
        assert_eq!(pc.fetch_stress_prepare(), StressStep::NoVictim);
        let b = get(&mut pc, 2);
        pc.release(b);
        assert_eq!(pc.fetch_stress_prepare(), StressStep::Spill(a));
        let mut seen = None;
        let r = pc.fetch_stress(3, &mut |c, victim| {
            c.make_clean(victim);
            seen = Some(victim);
            SQLITE_OK
        });
        assert!(matches!(r, Ok(Some(_))));
        assert_eq!(seen, Some(a));
    }

    #[test]
    fn move_page_replaces_existing() {
        let mut pc: PCache<u32> = PCache::open(512, 8, false);
        let a = get(&mut pc, 3);
        let b = get(&mut pc, 8);
        pc.release(b);
        pc.move_page(a, 8);
        assert_eq!(pc.page_pgno(a), 8);
        pc.release(a);
        assert_eq!(pc.page_count(), 1);
    }
}
