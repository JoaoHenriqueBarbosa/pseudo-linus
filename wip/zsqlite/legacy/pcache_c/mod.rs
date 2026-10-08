// Mesclado das partes traduzidas de pcache_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----
use crate::prelude::*;

/// Referência compartilhada a um `PCache` (o `PCache*` do C).

/// Cache de página completo. Cada entrada na cache detém uma única página do
/// arquivo de banco de dados. A camada btree só opera sobre a cópia em cache
/// das páginas do banco.
///
/// Uma entrada na cache de página é "limpa" se corresponde exatamente ao que
/// está atualmente no disco. Uma página é "suja" se foi modificada e precisa
/// ser persistida no disco.
///
/// `p_dirty`, `p_dirty_tail`, `p_synced`:
///   Todas as páginas sujas estão ligadas na lista duplamente ligada usando
///   `PgHdr.p_dirty_next` e `p_dirty_prev`. A lista é mantida em ordem LRU, de
///   modo que p foi adicionado à lista mais recentemente que `p.p_dirty_next`.
///   `PCache.p_dirty` aponta para o primeiro (mais novo) elemento da lista e
///   `p_dirty_tail` para o último (mais antigo).
///
///   A variável `PCache.p_synced` otimiza a busca por uma página suja a ejetar
///   da cache no meio da transação. É melhor ejetar uma página que não exige
///   sincronização do journal do que uma que exige. Por isso `p_synced` é
///   mantido de forma que quase sempre aponte para a página mais antiga da lista
///   `p_dirty`/`p_dirty_tail` com a flag PGHDR_NEED_SYNC limpa, ou para uma
///   página mais antiga que essa (assim a página certa a ejetar é encontrada
///   seguindo os ponteiros `p_dirty_prev`).
#[derive(Default)]
pub struct PCache {
    /// Lista de páginas sujas em ordem LRU (a primeira, mais nova)
    pub p_dirty: Option<PgHdrRef>,
    /// Última página (mais antiga) da lista suja
    pub p_dirty_tail: Option<PgHdrRef>,
    /// Última página sincronizada na lista de páginas sujas
    pub p_synced: Option<PgHdrRef>,
    /// Soma das contagens de referência de todas as páginas
    pub n_ref_sum: i64,
    /// Tamanho de cache configurado
    pub sz_cache: i32,
    /// Tamanho antes de ocorrer o derramamento (spill)
    pub sz_spill: i32,
    /// Tamanho de cada página desta cache
    pub sz_page: i32,
    /// Tamanho do espaço extra de cada página
    pub sz_extra: i32,
    /// Verdadeiro se as páginas têm armazenamento de apoio
    pub b_purgeable: u8,
    /// Valor de eCreate para xFetch()
    pub e_create: u8,
    /// Chamada para tentar deixar uma página limpa (xStress)
    pub x_stress: Option<Rc<dyn Fn(usize, &PgHdrRef) -> i32>>,
    /// Argumento de xStress
    pub p_stress: usize,
    /// Handle do módulo de cache plugável (0 é NULL)
    pub p_cache: usize,
}

// Valores permitidos para o segundo argumento de pcache_manage_dirty_list()

/// Remove a página da lista suja
pub const PCACHE_DIRTYLIST_REMOVE: u8 = 1;
/// Adiciona a página à lista suja
pub const PCACHE_DIRTYLIST_ADD: u8 = 2;
/// Move a página para a frente da lista
pub const PCACHE_DIRTYLIST_FRONT: u8 = 3;

/// Dono do cache de uma página (o `p->pCache` do C).
pub(super) fn pcache_of_page(p: &PgHdrRef) -> PCacheRef {
    p.borrow()
        .p_cache
        .as_ref()
        .and_then(|w| w.upgrade())
        .expect("PgHdr sem PCache")
}

/// Verifica invariantes de uma entrada PgHdr. Retorna verdadeiro se tudo está
/// certo. Só é usada dentro de asserções. As verificações caras
/// (pageOnDirtyList, SQLITE_ENABLE_EXPENSIVE_ASSERT) valem 1, como no C.
pub fn pcache_page_sanity(p_pg: &PgHdrRef) -> bool {
    let b = p_pg.borrow();
    // O número da página é 1 ou mais
    debug_assert!(b.pgno > 0 || b.p_pager.is_none());
    // Toda página tem um PCache associado
    let p_cache = b.p_cache.as_ref().and_then(|w| w.upgrade());
    debug_assert!(p_cache.is_some());
    if b.flags & PGHDR_CLEAN != 0 {
        // Não pode ser CLEAN e DIRTY ao mesmo tempo
        debug_assert!(b.flags & PGHDR_DIRTY == 0);
    } else {
        // Se não é CLEAN tem de ser DIRTY
        debug_assert!(b.flags & PGHDR_DIRTY != 0);
        debug_assert!(match &b.p_dirty_next {
            None => true,
            Some(n) => n
                .borrow()
                .p_dirty_prev
                .as_ref()
                .and_then(|w| w.upgrade())
                .map_or(false, |x| Rc::ptr_eq(&x, p_pg)),
        });
        debug_assert!(match b.p_dirty_prev.as_ref().and_then(|w| w.upgrade()) {
            None => true,
            Some(pv) => pv
                .borrow()
                .p_dirty_next
                .as_ref()
                .map_or(false, |x| Rc::ptr_eq(x, p_pg)),
        });
        debug_assert!(
            b.p_dirty_prev.is_some()
                || p_cache
                    .as_ref()
                    .and_then(|c| c.borrow().p_dirty.clone())
                    .map_or(false, |x| Rc::ptr_eq(&x, p_pg))
        );
    }
    // Páginas WRITEABLE também precisam ser DIRTY
    if b.flags & PGHDR_WRITEABLE != 0 {
        debug_assert!(b.flags & PGHDR_DIRTY != 0);
    }
    // NEED_SYNC pode ser definida independentemente de WRITEABLE. Isso acontece,
    // por exemplo, com a otimização sqlite3PagerDontWrite(): (1) a página X é
    // journalizada e ganha WRITEABLE e NEED_SEEK; (2) X vai para a freelist e
    // WRITEABLE é limpa; (3) X é reutilizada e WRITEABLE é definida de novo. Se
    // NEED_SYNC tivesse sido limpa no passo 2, não seria definida de novo no
    // passo 3, e a página poderia ser escrita no banco sem antes sincronizar o
    // journal de rollback, o que poderia corromper o banco numa queda de energia.
    //
    // Outro exemplo: quando o tamanho de página do banco é menor que o tamanho do
    // setor do disco. Quando qualquer página de um setor é journalizada, todas as
    // páginas do setor ganham NEED_SYNC mesmo ainda CLEAN, pois todas as páginas
    // do mesmo setor precisam ser journalizadas e sincronizadas antes de qualquer
    // uma delas ser escrita com segurança.
    true
}

/// Gerencia a participação de `p_page` na lista suja. Os bits do argumento
/// `add_remove` determinam a operação. O bit 0x01 significa primeiro remover
/// `p_page` da lista suja. O 0x02 significa adicionar `p_page` de volta à lista
/// suja. Fazer os dois move `p_page` para a frente da lista suja.
pub fn pcache_manage_dirty_list(p_page: &PgHdrRef, add_remove: u8) {
    let p_cache = pcache_of_page(p_page);
    let mut p = p_cache.borrow_mut();

    if add_remove & PCACHE_DIRTYLIST_REMOVE != 0 {
        let (next, prev) = {
            let b = p_page.borrow();
            (
                b.p_dirty_next.clone(),
                b.p_dirty_prev.as_ref().and_then(|w| w.upgrade()),
            )
        };
        debug_assert!(next.is_some() || p.p_dirty_tail.as_ref().map_or(false, |t| Rc::ptr_eq(t, p_page)));
        debug_assert!(prev.is_some() || p.p_dirty.as_ref().map_or(false, |t| Rc::ptr_eq(t, p_page)));

        // Atualiza a variável PCache.pSynced se necessário.
        if p.p_synced.as_ref().map_or(false, |s| Rc::ptr_eq(s, p_page)) {
            p.p_synced = prev.clone();
        }

        match &next {
            Some(n) => {
                n.borrow_mut().p_dirty_prev = p_page.borrow().p_dirty_prev.clone();
            }
            None => {
                debug_assert!(p.p_dirty_tail.as_ref().map_or(false, |t| Rc::ptr_eq(t, p_page)));
                p.p_dirty_tail = prev.clone();
            }
        }
        match &prev {
            Some(pv) => {
                pv.borrow_mut().p_dirty_next = next.clone();
            }
            None => {
                // Se agora não há páginas sujas na cache, define eCreate como 2.
                // É uma otimização que permite a pcache_fetch() pular a busca por
                // uma página suja a ejetar da cache quando de outro modo teria
                // de procurar.
                debug_assert!(p.p_dirty.as_ref().map_or(false, |t| Rc::ptr_eq(t, p_page)));
                p.p_dirty = next.clone();
                debug_assert!(p.b_purgeable != 0 || p.e_create == 2);
                if p.p_dirty.is_none() {
                    // OPTIMIZATION-IF-TRUE
                    debug_assert!(p.b_purgeable == 0 || p.e_create == 1);
                    p.e_create = 2;
                }
            }
        }
    }
    if add_remove & PCACHE_DIRTYLIST_ADD != 0 {
        {
            let mut b = p_page.borrow_mut();
            b.p_dirty_prev = None;
            b.p_dirty_next = p.p_dirty.clone();
        }
        match p.p_dirty.clone() {
            Some(n) => {
                debug_assert!(n.borrow().p_dirty_prev.is_none());
                n.borrow_mut().p_dirty_prev = Some(Rc::downgrade(p_page));
            }
            None => {
                p.p_dirty_tail = Some(Rc::clone(p_page));
                if p.b_purgeable != 0 {
                    debug_assert!(p.e_create == 2);
                    p.e_create = 1;
                }
            }
        }
        p.p_dirty = Some(Rc::clone(p_page));

        // Se pSynced é NULL e esta página tem NEED_SYNC limpa, faz pSynced apontar
        // para ela. Checar NEED_SYNC é uma otimização: se pSynced apontasse para
        // uma página com NEED_SYNC definida, pcache_fetch_stress() percorreria
        // todas as entradas mais novas da lista suja atrás de uma página com
        // NEED_SYNC limpa de qualquer forma.
        if p.p_synced.is_none()
            && (p_page.borrow().flags & PGHDR_NEED_SYNC) == 0 // OPTIMIZATION-IF-FALSE
        {
            p.p_synced = Some(Rc::clone(p_page));
        }
    }
}

/// Envoltório do método xUnpin do cache plugável. Se a cache serve a um banco em
/// memória, esta função não faz nada.
pub fn pcache_unpin(p: &PgHdrRef) {
    let p_cache = pcache_of_page(p);
    let (b_purgeable, handle) = {
        let pc = p_cache.borrow();
        (pc.b_purgeable, pc.p_cache)
    };
    if b_purgeable != 0 {
        (SQLITE_CONFIG.pcache2.x_unpin.unwrap())(handle, pcache1_page_handle(p), 0);
    }
}

/// Calcula o número de páginas de cache pedido. `p.sz_cache` é o tamanho de cache
/// pedido pela instrução "PRAGMA cache_size".
pub fn number_of_cache_pages(p: &PCache) -> i32 {
    if p.sz_cache >= 0 {
        // IMPLEMENTATION-OF: R-42059-47211 Se o argumento N é positivo, o tamanho
        // de cache sugerido é definido como N.
        p.sz_cache
    } else {
        // IMPLEMENTATION-OF: R-59858-46238 Se o argumento N é negativo, o número
        // de páginas de cache é ajustado para um número de páginas que usaria
        // aproximadamente abs(N*1024) bytes de memória, com base no tamanho de
        // página atual.
        let mut n: i64 = (-1024i64 * (p.sz_cache as i64)) / ((p.sz_page + p.sz_extra) as i64);
        if n > 1_000_000_000 {
            n = 1_000_000_000;
        }
        n as i32
    }
}

/// Inicializa o subsistema de cache de página. Esta função e a de desligamento
/// não são thread-safe.
pub fn pcache_initialize() -> i32 {
    if SQLITE_CONFIG.pcache2.x_init.is_none() {
        // IMPLEMENTATION-OF: R-26801-64137 Se o método xInit() é NULL, o cache de
        // página embutido padrão é usado no lugar do definido pela aplicação.
        p_cache_set_default();
        debug_assert!(SQLITE_CONFIG.pcache2.x_init.is_some());
    }
    (SQLITE_CONFIG.pcache2.x_init.unwrap())(SQLITE_CONFIG.pcache2.p_arg)
}

/// Desliga o subsistema de cache de página.
pub fn pcache_shutdown() {
    if let Some(x_shutdown) = SQLITE_CONFIG.pcache2.x_shutdown {
        // IMPLEMENTATION-OF: R-26000-56589 O método xShutdown() pode ser NULL.
        x_shutdown(SQLITE_CONFIG.pcache2.p_arg);
    }
}

/// Retorna o tamanho em bytes de um objeto PCache.
pub fn pcache_size() -> i32 {
    core::mem::size_of::<PCache>() as i32
}

/// Cria um novo objeto PCache. O armazenamento do objeto já foi alocado e é
/// passado em `p` (o `PCache*` pré-alocado do C, aqui um `PCacheRef` criado com
/// `PCache::default()`).
///
/// `sz_extra` é espaço extra alocado para cada página. Os primeiros 8 bytes do
/// espaço extra são zerados quando a página é alocada, mas o conteúdo restante
/// fica não inicializado. Embora seja opaco para este módulo, o espaço extra
/// acaba sendo a estrutura MemPage do pager.
pub fn pcache_open(
    sz_page: i32,
    sz_extra: i32,
    b_purgeable: i32,
    x_stress: Option<Rc<dyn Fn(usize, &PgHdrRef) -> i32>>,
    p_stress: usize,
    p: &PCacheRef,
) -> i32 {
    {
        let mut pc = p.borrow_mut();
        *pc = PCache::default();
        pc.sz_page = 1;
        pc.sz_extra = sz_extra;
        debug_assert!(sz_extra >= 8); // Os 8 primeiros bytes serão zerados
        pc.b_purgeable = b_purgeable as u8;
        pc.e_create = 2;
        pc.x_stress = x_stress;
        pc.p_stress = p_stress;
        pc.sz_cache = 100;
        pc.sz_spill = 1;
    }
    pcache_set_page_size(p, sz_page)
}


// ---- part_001.rs ----
use crate::prelude::*;

// Pontes assumidas com o módulo de cache de página embutido (pcache1.c), porque o
// handle `sqlite3_pcache_page*` do C é um `usize` nos tipos de `Sqlite3PcacheMethods2`
// e o `PgHdr` mora em `pPage->pExtra`:
//   pcache1_page_hdr(handle: usize) -> PgHdrRef           (o (PgHdr*)pPage->pExtra)
//   pcache1_page_obj(handle: usize) -> Box<Sqlite3PcachePage>
//   pcache1_page_handle(p: &PgHdrRef) -> usize            (o p->pPage como handle)
// Dono do PCache a partir de um PgHdr (p->pCache): `pcache_of_page`, definido em part_000.

/// Muda o tamanho de página do objeto PCache. O chamador deve garantir que não há
/// referências de página pendentes quando esta função é chamada.
pub fn pcache_set_page_size(p_cache: &PCacheRef, sz_page: i32) -> i32 {
    let (cur_sz_page, sz_extra, b_purgeable) = {
        let pc = p_cache.borrow();
        debug_assert!(pc.n_ref_sum == 0 && pc.p_dirty.is_none());
        (pc.sz_page, pc.sz_extra, pc.b_purgeable)
    };
    if cur_sz_page != 0 {
        let p_new: usize = (SQLITE_CONFIG.pcache2.x_create.unwrap())(
            sz_page,
            sz_extra + round8(core::mem::size_of::<PgHdr>()) as i32,
            b_purgeable as i32,
        );
        if p_new == 0 {
            return SQLITE_NOMEM_BKPT;
        }
        let n_pages = number_of_cache_pages(&p_cache.borrow());
        (SQLITE_CONFIG.pcache2.x_cachesize.unwrap())(p_new, n_pages);
        let p_old = p_cache.borrow().p_cache;
        if p_old != 0 {
            (SQLITE_CONFIG.pcache2.x_destroy.unwrap())(p_old);
        }
        let mut pc = p_cache.borrow_mut();
        pc.p_cache = p_new;
        pc.sz_page = sz_page;
    }
    SQLITE_OK
}

/// Tenta obter uma página do cache.
///
/// Retorna o handle de um objeto sqlite3_pcache_page se ele já está no cache ou se um
/// novo foi criado, e 0 (NULL) se o objeto não estava no cache e não pôde ser criado.
///
/// `create_flag` deve ser 0 para procurar páginas existentes e 3 (não 1, mas 3) para
/// tentar criar uma nova página.
///
/// Se `create_flag` é 0, retorna sempre NULL quando a página não está no cache. Se é 1,
/// uma nova página só é criada se isso puder ser feito sem despejar páginas sujas e sem
/// passar do limite de tamanho do cache.
///
/// O chamador precisa invocar `pcache_fetch_finish()` para inicializar o objeto e
/// convertê-lo num PgHdr.
pub fn pcache_fetch(p_cache: &PCacheRef, pgno: Pgno, create_flag: i32) -> usize {
    let pc = p_cache.borrow();
    debug_assert!(pc.p_cache != 0);
    debug_assert!(create_flag == 3 || create_flag == 0);
    debug_assert!(
        pc.e_create == if pc.b_purgeable != 0 && pc.p_dirty.is_some() { 1 } else { 2 }
    );

    // eCreate define o que fazer se a página não existe.
    //    0     Não aloca página nova.  (createFlag==0)
    //    1     Aloca página nova se for barato.
    //          (createFlag==1 AND bPurgeable AND pDirty)
    //    2     Aloca página nova mesmo que seja difícil.
    //          (createFlag==1 AND !(bPurgeable AND pDirty)
    let e_create: i32 = create_flag & pc.e_create as i32;
    debug_assert!(e_create == 0 || e_create == 1 || e_create == 2);
    debug_assert!(create_flag == 0 || pc.e_create as i32 == e_create);
    debug_assert!(
        create_flag == 0
            || e_create == 1 + (pc.b_purgeable == 0 || pc.p_dirty.is_none()) as i32
    );
    (SQLITE_CONFIG.pcache2.x_fetch.unwrap())(pc.p_cache, pgno, e_create)
}

/// Se `pcache_fetch()` não consegue alocar uma página nova porque não há páginas limpas
/// para reaproveitar e o limite de tamanho do cache foi atingido, esta rotina tenta com
/// mais empenho. Ela pode invocar o callback de estresse para despejar páginas sujas no
/// journal, e só falha ao alocar numa falta de memória.
///
/// Deve ser invocada apenas depois que `pcache_fetch()` falha.
pub fn pcache_fetch_stress(p_cache: &PCacheRef, pgno: Pgno, pp_page: &mut usize) -> i32 {
    if p_cache.borrow().e_create == 2 {
        return 0;
    }

    if pcache_pagecount(p_cache) > p_cache.borrow().sz_spill {
        // Acha uma página suja para escrever e reciclar. Primeiro tenta uma página que não
        // exige sync do journal (PGHDR_NEED_SYNC limpo); se não houver, aceita qualquer
        // outra página suja sem referências.
        //
        // Se a página LRU da lista suja com NEED_SYNC limpo está referenciada, o que segue
        // pode deixar pSynced apontando para outra que não a LRU com NEED_SYNC limpo. Tudo
        // bem: pSynced é só uma otimização.
        let mut p_pg: Option<PgHdrRef> = p_cache.borrow().p_synced.clone();
        while let Some(pg) = p_pg.clone() {
            let (busy, prev) = {
                let b = pg.borrow();
                (
                    b.n_ref != 0 || (b.flags & PGHDR_NEED_SYNC) != 0,
                    b.p_dirty_prev.as_ref().and_then(|w| w.upgrade()),
                )
            };
            if !busy {
                break;
            }
            p_pg = prev;
        }
        p_cache.borrow_mut().p_synced = p_pg.clone();
        if p_pg.is_none() {
            p_pg = p_cache.borrow().p_dirty_tail.clone();
            while let Some(pg) = p_pg.clone() {
                let (n_ref, prev) = {
                    let b = pg.borrow();
                    (b.n_ref, b.p_dirty_prev.as_ref().and_then(|w| w.upgrade()))
                };
                if n_ref == 0 {
                    break;
                }
                p_pg = prev;
            }
        }
        if let Some(pg) = p_pg {
            let (x_stress, p_stress) = {
                let pc = p_cache.borrow();
                (pc.x_stress.clone(), pc.p_stress)
            };
            let rc: i32 = (x_stress.unwrap())(p_stress, &pg);
            if rc != SQLITE_OK && rc != SQLITE_BUSY {
                return rc;
            }
        }
    }
    let p_handle = p_cache.borrow().p_cache;
    *pp_page = (SQLITE_CONFIG.pcache2.x_fetch.unwrap())(p_handle, pgno, 2);
    if *pp_page == 0 {
        SQLITE_NOMEM_BKPT
    } else {
        SQLITE_OK
    }
}

/// Auxiliar de `pcache_fetch_finish()`.
///
/// No caso incomum em que a página buscada ainda não foi inicializada, esta rotina faz a
/// inicialização. Fica separada porque exige manipulação extra de pilha que pode ser
/// evitada no caso comum.
#[inline(never)]
fn pcache_fetch_finish_with_init(p_cache: &PCacheRef, pgno: Pgno, p_page: usize) -> PgHdrRef {
    debug_assert!(p_page != 0);
    let p_pg_hdr = pcache1_page_hdr(p_page);
    debug_assert!(p_pg_hdr.borrow().p_page.is_none());
    let sz_extra = p_cache.borrow().sz_extra as usize;
    let mut p_obj = pcache1_page_obj(p_page);
    {
        let mut h = p_pg_hdr.borrow_mut();
        // memset(&pPgHdr->pDirty, 0, sizeof(PgHdr) - offsetof(PgHdr,pDirty))
        h.p_dirty = None;
        h.p_pager = None;
        h.pgno = 0;
        h.flags = 0;
        h.n_ref = 0;
        h.p_dirty_next = None;
        h.p_dirty_prev = None;
        h.p_data = core::mem::take(&mut p_obj.p_buf);
        h.p_page = Some(p_obj);
        // pExtra = &pPgHdr[1], com os 8 primeiros bytes zerados
        h.p_extra = vec![0u8; sz_extra];
        h.p_cache = Some(Rc::downgrade(p_cache));
        h.pgno = pgno;
        h.flags = PGHDR_CLEAN;
    }
    pcache_fetch_finish(p_cache, pgno, p_page)
}

/// Converte o objeto sqlite3_pcache_page devolvido por `pcache_fetch()` num PgHdr
/// inicializado. Deve ser chamada depois de `pcache_fetch()` para obter um resultado
/// utilizável.
pub fn pcache_fetch_finish(p_cache: &PCacheRef, pgno: Pgno, p_page: usize) -> PgHdrRef {
    debug_assert!(p_page != 0);
    let p_pg_hdr = pcache1_page_hdr(p_page);

    if p_pg_hdr.borrow().p_page.is_none() {
        return pcache_fetch_finish_with_init(p_cache, pgno, p_page);
    }
    p_cache.borrow_mut().n_ref_sum += 1;
    p_pg_hdr.borrow_mut().n_ref += 1;
    debug_assert!(pcache_page_sanity(&p_pg_hdr));
    p_pg_hdr
}

/// Decrementa a contagem de referências de uma página. Se a página está limpa e a
/// contagem cai a 0, ela se torna elegível para reciclagem.
#[inline(never)]
pub fn pcache_release(p: &PgHdrRef) {
    debug_assert!(p.borrow().n_ref > 0);
    pcache_of_page(p).borrow_mut().n_ref_sum -= 1;
    let (n_ref, flags) = {
        let mut b = p.borrow_mut();
        b.n_ref -= 1;
        (b.n_ref, b.flags)
    };
    if n_ref == 0 {
        if flags & PGHDR_CLEAN != 0 {
            pcache_unpin(p);
        } else {
            pcache_manage_dirty_list(p, PCACHE_DIRTYLIST_FRONT);
            debug_assert!(pcache_page_sanity(p));
        }
    }
}

/// Aumenta em 1 a contagem de referências da página.
pub fn pcache_ref(p: &PgHdrRef) {
    debug_assert!(p.borrow().n_ref > 0);
    debug_assert!(pcache_page_sanity(p));
    p.borrow_mut().n_ref += 1;
    pcache_of_page(p).borrow_mut().n_ref_sum += 1;
}

/// Tira uma página do cache. Deve haver exatamente uma referência à página. Esta função
/// apaga essa referência, então depois que retorna a página `p` é inválida.
pub fn pcache_drop(p: &PgHdrRef) {
    debug_assert!(p.borrow().n_ref == 1);
    debug_assert!(pcache_page_sanity(p));
    if p.borrow().flags & PGHDR_DIRTY != 0 {
        pcache_manage_dirty_list(p, PCACHE_DIRTYLIST_REMOVE);
    }
    let p_cache = pcache_of_page(p);
    p_cache.borrow_mut().n_ref_sum -= 1;
    let p_handle = p_cache.borrow().p_cache;
    (SQLITE_CONFIG.pcache2.x_unpin.unwrap())(p_handle, pcache1_page_handle(p), 1);
}

/// Garante que a página está marcada como suja. Se ainda não está, marca.
pub fn pcache_make_dirty(p: &PgHdrRef) {
    debug_assert!(p.borrow().n_ref > 0);
    debug_assert!(pcache_page_sanity(p));
    if p.borrow().flags & (PGHDR_CLEAN | PGHDR_DONT_WRITE) != 0 {
        // OPTIMIZATION-IF-FALSE
        let was_clean = {
            let mut b = p.borrow_mut();
            b.flags &= !PGHDR_DONT_WRITE;
            if b.flags & PGHDR_CLEAN != 0 {
                b.flags ^= PGHDR_DIRTY | PGHDR_CLEAN;
                debug_assert!((b.flags & (PGHDR_DIRTY | PGHDR_CLEAN)) == PGHDR_DIRTY);
                true
            } else {
                false
            }
        };
        if was_clean {
            pcache_manage_dirty_list(p, PCACHE_DIRTYLIST_ADD);
            debug_assert!(pcache_page_sanity(p));
        }
        debug_assert!(pcache_page_sanity(p));
    }
}

/// Garante que a página está marcada como limpa. Se ainda não está, marca.
pub fn pcache_make_clean(p: &PgHdrRef) {
    debug_assert!(pcache_page_sanity(p));
    debug_assert!(p.borrow().flags & PGHDR_DIRTY != 0);
    debug_assert!(p.borrow().flags & PGHDR_CLEAN == 0);
    pcache_manage_dirty_list(p, PCACHE_DIRTYLIST_REMOVE);
    let n_ref = {
        let mut b = p.borrow_mut();
        b.flags &= !(PGHDR_DIRTY | PGHDR_NEED_SYNC | PGHDR_WRITEABLE);
        b.flags |= PGHDR_CLEAN;
        b.n_ref
    };
    debug_assert!(pcache_page_sanity(p));
    if n_ref == 0 {
        pcache_unpin(p);
    }
}

/// Deixa todas as páginas do cache limpas.
pub fn pcache_clean_all(p_cache: &PCacheRef) {
    loop {
        let p = p_cache.borrow().p_dirty.clone();
        match p {
            Some(p) => pcache_make_clean(&p),
            None => break,
        }
    }
}

/// Limpa os sinalizadores PGHDR_NEED_SYNC e PGHDR_WRITEABLE de todas as páginas sujas.
pub fn pcache_clear_writable(p_cache: &PCacheRef) {
    let mut p = p_cache.borrow().p_dirty.clone();
    while let Some(cur) = p {
        let mut b = cur.borrow_mut();
        b.flags &= !(PGHDR_NEED_SYNC | PGHDR_WRITEABLE);
        let next = b.p_dirty_next.clone();
        drop(b);
        p = next;
    }
    let mut pc = p_cache.borrow_mut();
    pc.p_synced = pc.p_dirty_tail.clone();
}

/// Limpa o sinalizador PGHDR_NEED_SYNC de todas as páginas sujas.
pub fn pcache_clear_sync_flags(p_cache: &PCacheRef) {
    let mut p = p_cache.borrow().p_dirty.clone();
    while let Some(cur) = p {
        let mut b = cur.borrow_mut();
        b.flags &= !PGHDR_NEED_SYNC;
        let next = b.p_dirty_next.clone();
        drop(b);
        p = next;
    }
    let mut pc = p_cache.borrow_mut();
    pc.p_synced = pc.p_dirty_tail.clone();
}

/// Muda o número da página `p` para `new_pgno`.
pub fn pcache_move(p: &PgHdrRef, new_pgno: Pgno) {
    let p_cache = pcache_of_page(p);
    debug_assert!(p.borrow().n_ref > 0);
    debug_assert!(new_pgno > 0);
    debug_assert!(pcache_page_sanity(p));
    let p_handle = p_cache.borrow().p_cache;
    let p_other: usize = (SQLITE_CONFIG.pcache2.x_fetch.unwrap())(p_handle, new_pgno, 0);
    if p_other != 0 {
        let p_x_page = pcache1_page_hdr(p_other);
        debug_assert!(p_x_page.borrow().n_ref == 0);
        p_x_page.borrow_mut().n_ref += 1;
        p_cache.borrow_mut().n_ref_sum += 1;
        pcache_drop(&p_x_page);
    }
    let old_pgno = p.borrow().pgno;
    (SQLITE_CONFIG.pcache2.x_rekey.unwrap())(p_handle, pcache1_page_handle(p), old_pgno, new_pgno);
    p.borrow_mut().pgno = new_pgno;
    let flags = p.borrow().flags;
    if (flags & PGHDR_DIRTY) != 0 && (flags & PGHDR_NEED_SYNC) != 0 {
        pcache_manage_dirty_list(p, PCACHE_DIRTYLIST_FRONT);
        debug_assert!(pcache_page_sanity(p));
    }
}

/// Descarta toda entrada do cache cujo número de página é maior que `pgno`. O chamador
/// deve garantir que não há referências pendentes a páginas, exceto a página 1, com
/// número maior que `pgno`.
///
/// Se há uma referência à página 1 e `pgno` é 0, a área de dados da página 1 é zerada,
/// mas o objeto de página não é descartado.
pub fn pcache_truncate(p_cache: &PCacheRef, mut pgno: Pgno) {
    let p_handle = p_cache.borrow().p_cache;
    if p_handle != 0 {
        let mut p = p_cache.borrow().p_dirty.clone();
        while let Some(cur) = p {
            let p_next = cur.borrow().p_dirty_next.clone();
            // Esta rotina nunca é chamada com pgno positivo, exceto logo depois de
            // pcache_clean_all(). Então, se há páginas sujas, pgno==0.
            debug_assert!(cur.borrow().pgno > 0);
            if cur.borrow().pgno > pgno {
                debug_assert!(cur.borrow().flags & PGHDR_DIRTY != 0);
                pcache_make_clean(&cur);
            }
            p = p_next;
        }
        if pgno == 0 && p_cache.borrow().n_ref_sum != 0 {
            let p_page1: usize = (SQLITE_CONFIG.pcache2.x_fetch.unwrap())(p_handle, 1, 0);
            if p_page1 != 0 {
                // A página 1 sempre está no cache, porque nRefSum>0.
                let sz_page = p_cache.borrow().sz_page as usize;
                pcache1_page_hdr(p_page1).borrow_mut().p_data[..sz_page].fill(0);
                pgno = 1;
            }
        }
        (SQLITE_CONFIG.pcache2.x_truncate.unwrap())(p_handle, pgno + 1);
    }
}


// ---- part_002.rs ----
use crate::prelude::*;

/// Fecha um cache.
pub fn pcache_close(p_cache: &PCacheRef) {
    let handle = p_cache.borrow().p_cache;
    debug_assert!(handle != 0);
    (SQLITE_CONFIG.pcache2.x_destroy.unwrap())(handle);
}

/// Descarta o conteúdo do cache.
pub fn pcache_clear(p_cache: &PCacheRef) {
    pcache_truncate(p_cache, 0);
}

/// Mescla duas listas de páginas ligadas por `p_dirty` e em ordem de pgno. Não
/// se preocupa em corrigir os ponteiros `p_dirty_prev`.
fn pcache_merge_dirty_list(p_a: PgHdrRef, p_b: PgHdrRef) -> PgHdrRef {
    let mut p_a = p_a;
    let mut p_b = p_b;
    // `result` do C é um PgHdr de pilha cujo `pDirty` é a cabeça da lista.
    let mut result: Option<PgHdrRef> = None;
    let mut p_tail: Option<PgHdrRef> = None;
    loop {
        let take_a = p_a.borrow().pgno < p_b.borrow().pgno;
        let chosen = if take_a { p_a.clone() } else { p_b.clone() };
        match &p_tail {
            Some(t) => t.borrow_mut().p_dirty = Some(chosen.clone()),
            None => result = Some(chosen.clone()),
        }
        p_tail = Some(chosen.clone());
        let next = chosen.borrow().p_dirty.clone();
        match next {
            Some(n) => {
                if take_a {
                    p_a = n;
                } else {
                    p_b = n;
                }
            }
            None => {
                let other = if take_a { p_b.clone() } else { p_a.clone() };
                chosen.borrow_mut().p_dirty = Some(other);
                break;
            }
        }
    }
    result.unwrap()
}

/// Número de baldes do ordenador por mesclagem.
pub const N_SORT_BUCKET: usize = 32;

/// Ordena a lista de páginas em ordem ascendente por pgno. As páginas estão
/// ligadas por ponteiros `p_dirty`. Os ponteiros `p_dirty_prev` são corrompidos
/// por esta ordenação.
///
/// Como não pode haver mais de 2^31 páginas distintas num banco, o ordenador por
/// mesclagem não precisa de mais de 31 baldes. Um balde extra é adicionado para
/// capturar overflow caso algo mude e torne a frase anterior falsa.
fn pcache_sort_dirty_list(p_in: Option<PgHdrRef>) -> Option<PgHdrRef> {
    let mut a: [Option<PgHdrRef>; N_SORT_BUCKET] = Default::default();
    let mut p_in = p_in;
    while let Some(p0) = p_in {
        p_in = p0.borrow().p_dirty.clone();
        p0.borrow_mut().p_dirty = None;
        let mut p: Option<PgHdrRef> = Some(p0);
        for i in 0..N_SORT_BUCKET - 1 {
            match a[i].take() {
                None => {
                    a[i] = p.take();
                    break;
                }
                Some(ai) => {
                    p = Some(pcache_merge_dirty_list(ai, p.take().unwrap()));
                }
            }
        }
        if let Some(rem) = p {
            // Para chegar aqui seriam necessários 2^(N_SORT_BUCKET) elementos na
            // lista de entrada. Mas isso é impossível.
            let i = N_SORT_BUCKET - 1;
            a[i] = Some(match a[i].take() {
                Some(ai) => pcache_merge_dirty_list(ai, rem),
                None => rem,
            });
        }
    }
    let mut p = a[0].take();
    for i in 1..N_SORT_BUCKET {
        let Some(ai) = a[i].take() else { continue };
        p = Some(match p {
            Some(pp) => pcache_merge_dirty_list(pp, ai),
            None => ai,
        });
    }
    p
}

/// Retorna uma lista de todas as páginas sujas do cache, ordenadas por número de
/// página.
pub fn pcache_dirty_list(p_cache: &PCacheRef) -> Option<PgHdrRef> {
    let mut p = p_cache.borrow().p_dirty.clone();
    while let Some(cur) = p {
        let next = cur.borrow().p_dirty_next.clone();
        cur.borrow_mut().p_dirty = next.clone();
        p = next;
    }
    let head = p_cache.borrow().p_dirty.clone();
    pcache_sort_dirty_list(head)
}

/// Retorna o número total de referências a todas as páginas mantidas pela cache.
///
/// Não é o número total de páginas referenciadas, mas a soma da contagem de
/// referências de todas as páginas.
pub fn pcache_ref_count(p_cache: &PCacheRef) -> i64 {
    p_cache.borrow().n_ref_sum
}

/// Retorna o número de referências à página dada como argumento.
pub fn pcache_page_refcount(p: &PgHdrRef) -> i64 {
    p.borrow().n_ref as i64
}

/// Retorna o número total de páginas da cache.
pub fn pcache_pagecount(p_cache: &PCacheRef) -> i32 {
    let handle = p_cache.borrow().p_cache;
    debug_assert!(handle != 0);
    (SQLITE_CONFIG.pcache2.x_pagecount.unwrap())(handle)
}

/// Define o valor de tamanho de cache sugerido.
pub fn pcache_set_cachesize(p_cache: &PCacheRef, mx_page: i32) {
    let (handle, n_pages) = {
        let mut pc = p_cache.borrow_mut();
        debug_assert!(pc.p_cache != 0);
        pc.sz_cache = mx_page;
        (pc.p_cache, number_of_cache_pages(&pc))
    };
    (SQLITE_CONFIG.pcache2.x_cachesize.unwrap())(handle, n_pages);
}

/// Define o valor de derramamento (spill) sugerido. Não muda nada se o argumento é
/// zero. Retorna o tamanho de spill efetivo, o maior entre szSpill e szCache.
pub fn pcache_set_spillsize(p: &PCacheRef, mx_page: i32) -> i32 {
    let mut pc = p.borrow_mut();
    debug_assert!(pc.p_cache != 0);
    let mut mx_page = mx_page;
    if mx_page != 0 {
        if mx_page < 0 {
            mx_page = ((-1024i64 * (mx_page as i64)) / ((pc.sz_page + pc.sz_extra) as i64)) as i32;
        }
        pc.sz_spill = mx_page;
    }
    let mut res = number_of_cache_pages(&pc);
    if res < pc.sz_spill {
        res = pc.sz_spill;
    }
    res
}

/// Libera o máximo de memória possível da cache de páginas.
pub fn pcache_shrink(p_cache: &PCacheRef) {
    let handle = p_cache.borrow().p_cache;
    debug_assert!(handle != 0);
    (SQLITE_CONFIG.pcache2.x_shrink.unwrap())(handle);
}

/// Retorna o tamanho do cabeçalho acrescentado por esta camada intermediária na
/// hierarquia do cache de páginas.
pub fn header_size_pcache() -> i32 {
    round8(core::mem::size_of::<PgHdr>()) as i32
}

/// Retorna o número de páginas sujas atualmente na cache, como porcentagem do
/// tamanho de cache configurado.
pub fn p_cache_percent_dirty(p_cache: &PCacheRef) -> i32 {
    let n_cache = number_of_cache_pages(&p_cache.borrow());
    let mut n_dirty: i32 = 0;
    let mut p_dirty = p_cache.borrow().p_dirty.clone();
    while let Some(cur) = p_dirty {
        n_dirty += 1;
        p_dirty = cur.borrow().p_dirty_next.clone();
    }
    if n_cache != 0 {
        (((n_dirty as i64) * 100) / (n_cache as i64)) as i32
    } else {
        0
    }
}

/// Retorna verdadeiro se há uma ou mais páginas sujas na cache. Senão, falso.
/// (SQLITE_DIRECT_OVERFLOW_READ fica sempre ligado no btree do SQLite; a função
/// é inofensiva quando não usada.)
pub fn p_cache_is_dirty(p_cache: &PCacheRef) -> i32 {
    p_cache.borrow().p_dirty.is_some() as i32
}

