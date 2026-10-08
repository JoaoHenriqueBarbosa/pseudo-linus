// Mesclado das partes traduzidas de pcache_h (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// `PCache` é definido em pcache.c e `Pager` em pager.c; `sqlite3_pcache_page`
// vem de sqlite3.h (Sqlite3PcachePage). Aqui só os apelidos de referência.

/// Referência compartilhada a PCache.
pub type PCacheRef = Rc<RefCell<PCache>>;

/// Referência compartilhada a PgHdr.
pub type PgHdrRef = Rc<RefCell<PgHdr>>;

/// Cada página no cache é controlada por uma instância da seguinte estrutura.
/// PgHdr é compartilhada entre as listas de páginas sujas, logo vive atrás de
/// Rc<RefCell<>>; os ponteiros de volta são Weak.
pub struct PgHdr {
    // Elementos públicos (exceto p_cache), agrupados com p_cache por eficiência.
    /// Identificador de página do Pcache.
    pub p_page: Option<Box<Sqlite3PcachePage>>,

    /// Dados da página (conteúdo).
    pub p_data: Vec<u8>,

    /// Conteúdo extra associado à página.
    pub p_extra: Vec<u8>,

    /// Cache que é dono desta página (PRIVADO).
    pub p_cache: Option<Weak<RefCell<PCache>>>,

    /// Lista transitória de páginas sujas, ordenada por número de página.
    pub p_dirty: Option<PgHdrRef>,

    /// O Pager do qual esta página faz parte.
    pub p_pager: Option<Weak<RefCell<Pager>>>,

    /// Número da página.
    pub pgno: Pgno,

    /// Sinalizadores PGHDR definidos abaixo.
    pub flags: u16,

    // Os elementos a seguir são privados de pcache.c.
    /// Número de usuários desta página.
    pub n_ref: i64,

    /// Próximo elemento da lista de páginas sujas.
    /// Indefinido se o objeto PgHdr não está sujo.
    pub p_dirty_next: Option<PgHdrRef>,

    /// Elemento anterior da lista de páginas sujas.
    /// Indefinido se o objeto PgHdr não está sujo.
    pub p_dirty_prev: Option<Weak<RefCell<PgHdr>>>,
}

// Valores de bit para PgHdr.flags

/// Página não está na lista PCache.p_dirty.
pub const PGHDR_CLEAN: u16 = 0x001;

/// Página está na lista PCache.p_dirty.
pub const PGHDR_DIRTY: u16 = 0x002;

/// Registrada no journal e pronta para ser modificada.
pub const PGHDR_WRITEABLE: u16 = 0x004;

/// Fazer fsync do journal de rollback antes de escrever esta página no banco.
pub const PGHDR_NEED_SYNC: u16 = 0x008;

/// Não escrever o conteúdo desta página no disco.
pub const PGHDR_DONT_WRITE: u16 = 0x010;

/// Este é um objeto de página mapeado em memória (mmap).
pub const PGHDR_MMAP: u16 = 0x020;

/// Página anexada ao arquivo WAL.
pub const PGHDR_WAL_APPEND: u16 = 0x040;

