//! Pager (pager.c, chunks 000 a 009): estado do pager, journal de rollback, travas e
//! reprodução (playback) do journal. O restante de pager.c (chunks 010 a 019) vive em
//! `crate::pager_ext` como um segundo `impl<E: Default> Pager<E>` do mesmo tipo.
//!
//! Modelo (CONVENTIONS.md, itens 2 e 5): o `Pager<E>` é o dono único do `PCache<E>` (campo
//! `pcache`) e, por ele, das páginas; uma página é um `PgId`. `E` é o conteúdo do `pExtra`
//! (o `MemPage` do btree). Arquivos são `Option<Box<dyn VfsFile>>` (`None` é `pMethods==0`,
//! ou seja, `isOpen(fd)` falso). O VFS é um `VfsRef`.
//!
//! Páginas mapeadas em memória (`PGHDR_MMAP`): o VFS devolve uma cópia (`Vec<u8>`) e o pager
//! guarda o objeto de página em `mmap_pages` (o `PgHdr` alocado à parte do C). O `PgId` dessas
//! páginas tem o bit `PGID_MMAP_BASE` ligado; os acessores `page_data`, `page_extra`,
//! `page_parts`, `page_pgno`, `page_flags`, `page_ref_count` abaixo fazem o despacho entre as
//! duas origens e são o único caminho que o resto do código deve usar para alcançar uma página
//! que pode ser mapeada. `pMmapFreelist` é `mmap_freelist` (pilha de índices de `mmap_pages`).
//!
//! Decisões e desvios do C (todos por causa de empréstimo ou de modelo):
//!
//! * As funções `static` de pager.c são `pub(crate)` porque `pager_ext.rs` as chama.
//! * O que o C faz por ponteiro para dentro do próprio pager (`&pPager->journalOff` passado a
//!   `pager_playback_one_page`) é feito por cópia de ida e volta.
//! * O `pTmpSpace` usado como buffer de `pager_playback_one_page` é retirado do pager durante
//!   a chamada; os nomes de super-journal, que o C guardava em `pTmpSpace`, são `Vec<u8>` locais.
//! * `pPager->pBackup` (lista de `sqlite3_backup`) é `Option<Box<dyn PagerBackup>>`.
//! * `xGet` é o enum `PagerGetter`; `xReiniter` ganha o refcount como terceiro argumento, porque
//!   o `pageReinit` do btree consulta `sqlite3PagerPageRefcount`.
//! * Funções que só repassavam a chamada ao pcache (`sqlite3PagerSetCachesize`,
//!   `sqlite3PagerSetSpillsize`, `sqlite3PagerShrink`) não existem: chame
//!   `pager.pcache.set_cache_size(n)`, `pager.pcache.set_spill_size(n)`, `pager.pcache.shrink()`.
//! * `assertTruncateConstraint`, `print_pager_state`, `pager_set_pagehash`, `CHECK_PAGE` e o
//!   contador `sqlite3_opentemp_count` são SQLITE_DEBUG/SQLITE_TEST/SQLITE_CHECK_PAGES e somem.
//! * `jrnlBufferSize` com `SQLITE_ENABLE_ATOMIC_WRITE` e `SQLITE_ENABLE_BATCH_ATOMIC_WRITE`
//!   desligados (Debian) devolve sempre 0.
//! * Falha de alocação (`SQLITE_NOMEM`) não existe em Rust: os ramos que só a tratavam somem.
//!
//! CAMPOS (struct `Pager<E>`, todos `pub(crate)`; nomes do C em snake_case):
//!   vfs: VfsRef                         pVfs
//!   exclusive_mode: bool                exclusiveMode
//!   journal_mode: i32                   journalMode (PAGER_JOURNALMODE_*)
//!   use_journal: bool                   useJournal
//!   no_sync: bool                       noSync
//!   full_sync: bool                     fullSync
//!   extra_sync: bool                    extraSync
//!   sync_flags: i32                     syncFlags
//!   wal_sync_flags: i32                 walSyncFlags
//!   temp_file: bool                     tempFile
//!   no_lock: bool                       noLock
//!   read_only: bool                     readOnly
//!   mem_db: bool                        memDb
//!   mem_vfs: bool                       memVfs
//!   e_state: u8                         eState (PAGER_OPEN .. PAGER_ERROR)
//!   e_lock: i32                         eLock (NO_LOCK .. UNKNOWN_LOCK)
//!   change_count_done: bool             changeCountDone
//!   set_super: bool                     setSuper
//!   do_not_spill: u8                    doNotSpill (SPILLFLAG_*)
//!   subj_in_memory: bool                subjInMemory
//!   b_use_fetch: bool                   bUseFetch
//!   has_held_shared_lock: bool          hasHeldSharedLock
//!   db_size: u32                        dbSize
//!   db_orig_size: u32                   dbOrigSize
//!   db_file_size: u32                   dbFileSize
//!   db_hint_size: u32                   dbHintSize
//!   err_code: i32                       errCode
//!   n_rec: i32                          nRec
//!   cksum_init: u32                     cksumInit
//!   n_sub_rec: u32                      nSubRec
//!   in_journal: Option<Box<Bitvec>>     pInJournal
//!   fd: Option<Box<dyn VfsFile>>        fd
//!   jfd: Option<Box<dyn VfsFile>>       jfd
//!   sjfd: Option<Box<dyn VfsFile>>      sjfd
//!   journal_off: i64                    journalOff
//!   journal_hdr: i64                    journalHdr
//!   backup: Option<Box<dyn PagerBackup>>  pBackup
//!   a_savepoint: Vec<PagerSavepoint>    aSavepoint (nSavepoint é a_savepoint.len())
//!   i_data_version: u32                 iDataVersion
//!   db_file_vers: [u8; 16]              dbFileVers
//!   n_mmap_out: i32                     nMmapOut
//!   sz_mmap: i64                        szMmap
//!   mmap_pages: Vec<MmapPage<E>>        objetos de página mapeada (os PgHdr do pMmapFreelist)
//!   mmap_freelist: Vec<u32>             pMmapFreelist (índices livres de mmap_pages)
//!   n_extra: u16                        nExtra
//!   n_reserve: i16                      nReserve
//!   vfs_flags: u32                      vfsFlags
//!   sector_size: u32                    sectorSize
//!   mx_pgno: u32                        mxPgno
//!   lck_pgno: u32                       lckPgno (PAGER_SJ_PGNO)
//!   page_size: i64                      pageSize
//!   journal_size_limit: i64             journalSizeLimit
//!   z_filename: Vec<u8>                 zFilename (o blob sqlite3_filename do Vfs::open)
//!   z_journal: Vec<u8>                  zJournal (idem)
//!   busy_handler: Option<Box<dyn FnMut() -> i32>>  xBusyHandler + pBusyHandlerArg
//!   a_stat: [u32; 4]                    aStat (PAGER_STAT_*)
//!   reiniter: Option<fn(&mut E, &mut [u8], i64)>  xReiniter (extra, bytes, refcount)
//!   x_get: PagerGetter                  xGet (Normal, Error, MMap)
//!   tmp_space: Vec<u8>                  pTmpSpace (page_size + 8 bytes)
//!   pcache: PCache<E>                   pPCache
//!   wal: Option<Wal>                    pWal
//!   z_wal: Vec<u8>                      zWal (blob, como z_journal)
//!
//! Os campos `pPage1`, `nRead` (SQLITE_TEST) e `pBusyHandlerArg` não existem: o primeiro é do
//! `BtShared`, o segundo é de teste e o terceiro está capturado no closure do `busy_handler`.
//!
//! ASSINATURAS que `pager_ext.rs` deve respeitar ao chamar este arquivo (todas `&mut self`,
//! salvo indicação):
//!   page_data(&self, PgId) -> &[u8]; page_data_mut(PgId) -> &mut [u8]; page_extra(&self, PgId)
//!   -> &E; page_extra_mut; page_parts(PgId) -> (&mut E, &mut [u8]); page_pgno(&self, PgId) -> u32;
//!   page_flags(&self, PgId) -> u16; page_flags_mut(PgId) -> &mut u16; page_ref_count(&self, PgId) -> i64
//!   Pager::zeroed(vfs: VfsRef, pcache: PCache<E>) -> Pager<E>   (o MallocZero do PagerOpen)
//!   set_getter_method(); subj_requires_page(PgId) -> bool; page_in_journal(&self, PgId) -> bool
//!   pager_unlock_db(e_lock: i32) -> i32; pager_lock_db(e_lock: i32) -> i32
//!   jrnl_buffer_size(&self) -> i32; read_super_journal (fn livre, ver abaixo)
//!   journal_hdr_offset(&self) -> i64; zero_journal_hdr(do_truncate: bool) -> i32
//!   write_journal_hdr() -> i32; read_journal_hdr(is_hot: bool, journal_size: i64,
//!   n_rec: &mut u32, db_size: &mut u32) -> i32; write_super_journal(z_super: Option<&[u8]>) -> i32
//!   pager_reset(); release_all_savepoints(); add_to_savepoint_bitvecs(pgno: u32) -> i32
//!   pager_unlock(); pager_error(rc: i32) -> i32; pager_flush_on_commit(&self, b_commit: bool) -> bool
//!   pager_end_transaction(has_super: bool, b_commit: bool) -> i32
//!   pager_unlock_and_rollback(); pager_cksum(&self, &[u8]) -> u32
//!   pager_playback_one_page(offset: &mut i64, done: Option<&mut Bitvec>, is_main_jrnl: bool,
//!   is_savepnt: bool) -> i32; pager_delsuper(z_super: &[u8]) -> i32; pager_truncate(n_page: u32) -> i32
//!   set_sector_size(); pager_playback(is_hot: bool) -> i32; read_db_page(PgId) -> i32
//!   pager_write_changecounter(PgId); pager_undo_callback(pgno: u32) -> i32; pager_rollback_wal() -> i32
//!   pager_wal_frames(list: Vec<PgId>, n_truncate: u32, is_commit: bool) -> i32
//!   pager_begin_read_transaction() -> i32; pager_pagecount(n_page: &mut u32) -> i32
//!   pager_open_wal_if_present() -> i32; pager_playback_savepoint(sp: Option<usize>) -> i32
//!   (o `sp` é o índice em `a_savepoint`; `None` é o `pSavepoint == NULL`)
//!   pager_fix_maplimit(); set_mmap_limit(i64); set_flags(u32); pager_opentemp(&self, i32) ->
//!   Result<Box<dyn VfsFile>, i32> (quem chama guarda em `fd` ou `sjfd`); set_busy_handler(...)
//!   set_pagesize(page_size: &mut u32, n_reserve: i32) -> i32; max_page_count(u32) -> u32
//!   read_fileheader(dest: &mut [u8]) -> i32 (N é dest.len()); truncate_image(u32)
//!   pager_wait_on_lock(locktype: i32) -> i32; pager_sync_hot_journal() -> i32
//!   pager_acquire_map_page(pgno: u32, data: Vec<u8>) -> Result<PgId, i32>
//!   direct_read_ok(pgno: u32) -> bool; sector_size(&mut dyn VfsFile) -> i32 (fn livre)
//!   backup_restart(); journal_hdr_sz(&self) -> i64; journal_pg_sz(&self) -> i64
//!   pager_use_wal(&self) -> bool; fd_device_characteristics() -> i32
//!   Macros do módulo, reexportadas: `file_of!(self, fd|jfd|sjfd)` e `wal_and_fd!(self)`.
//!
//! Chunk 009 termina em `pagerAcquireMapPage`; `pagerReleaseMapPage` (chunk 010) é do outro arquivo
//! e deve usar `mmap_pages`, `mmap_freelist` e `n_mmap_out` como descrito acima.
//!
//! ESPERADO DE `pager_ext.rs` (chamado daqui; `pub(crate)`, `&mut self`):
//!   rollback() -> i32                       sqlite3PagerRollback
//!   lookup(pgno: u32) -> Option<PgId>       sqlite3PagerLookup (já com a referência contada)
//!   get(pgno: u32, flags: i32) -> Result<PgId, i32>   sqlite3PagerGet (via x_get)
//!   unref_not_null(pg: PgId)                sqlite3PagerUnrefNotNull
//!   sync(z_super: Option<&[u8]>) -> i32     sqlite3PagerSync
//!   open_wal(p_open: Option<&mut i32>) -> i32   sqlite3PagerOpenWal

use crate::bitvec::{bitvec_create, bitvec_set, bitvec_test, bitvec_test_not_null, Bitvec};
use crate::consts::{
    EXCLUSIVE_LOCK, NO_LOCK, PAGER_CACHESPILL, PAGER_CKPT_FULLFSYNC, PAGER_FULLFSYNC,
    PAGER_JOURNALMODE_DELETE, PAGER_JOURNALMODE_MEMORY, PAGER_JOURNALMODE_OFF,
    PAGER_JOURNALMODE_PERSIST, PAGER_JOURNALMODE_TRUNCATE, PAGER_JOURNALMODE_WAL,
    PAGER_SYNCHRONOUS_EXTRA, PAGER_SYNCHRONOUS_FULL, PAGER_SYNCHRONOUS_MASK,
    PAGER_SYNCHRONOUS_OFF, PENDING_BYTE, PENDING_LOCK, RESERVED_LOCK, SHARED_LOCK,
    SQLITE_ACCESS_EXISTS, SQLITE_BUSY, SQLITE_DONE, SQLITE_FCNTL_BUSYHANDLER,
    SQLITE_FCNTL_COMMIT_PHASETWO, SQLITE_FCNTL_MMAP_SIZE, SQLITE_FCNTL_SIZE_HINT, SQLITE_FULL,
    SQLITE_IOCAP_BATCH_ATOMIC, SQLITE_IOCAP_POWERSAFE_OVERWRITE, SQLITE_IOCAP_SAFE_APPEND,
    SQLITE_IOCAP_UNDELETABLE_WHEN_OPEN, SQLITE_IOERR, SQLITE_IOERR_SHORT_READ,
    SQLITE_MAX_PAGE_SIZE, SQLITE_NOTFOUND, SQLITE_NOTICE_RECOVER_ROLLBACK, SQLITE_OK,
    SQLITE_OPEN_CREATE, SQLITE_OPEN_DELETEONCLOSE, SQLITE_OPEN_EXCLUSIVE, SQLITE_OPEN_READONLY,
    SQLITE_OPEN_READWRITE, SQLITE_OPEN_SUPER_JOURNAL, SQLITE_SYNC_DATAONLY, SQLITE_SYNC_FULL,
    SQLITE_SYNC_NORMAL, SQLITE_VERSION_NUMBER, WAL_SAVEPOINT_NDATA,
};
use crate::os::{
    os_close, os_file_control, os_file_control_hint, os_open, os_sync, FileControlArg, VfsFile,
    VfsRef,
};
use crate::pcache::{
    PCache, PgId, PGHDR_MMAP, PGHDR_NEED_SYNC, PGHDR_WAL_APPEND,
};
use crate::printf::PrintfArg;
use crate::util::{get4byte, put4byte};
use crate::wal::{Wal, WalPageRef};

// ---------------------------------------------------------------------------
// Macros de acesso a arquivo aberto (o `isOpen(fd)` do C vira invariante)
// ---------------------------------------------------------------------------

/// Arquivo do pager (`fd`, `jfd` ou `sjfd`) que o C supõe aberto (`assert(isOpen(..))`).
/// Expande para acesso direto ao campo, então o empréstimo é só daquele campo.
macro_rules! file_of {
    ($s:expr, $f:ident) => {
        $s.$f
            .as_deref_mut()
            .expect("pager: arquivo fechado (invariante isOpen do C)")
    };
}
pub(crate) use file_of;

/// O par (`&mut Wal`, `&mut dyn VfsFile` do banco) para chamar o WAL. No C o `Wal` guarda o
/// `pDbFd`; aqui quem o possui é o pager, então todo método do `Wal` recebe o arquivo.
macro_rules! wal_and_fd {
    ($s:expr) => {
        (
            $s.wal
                .as_mut()
                .expect("pager: sem WAL (invariante pagerUseWal do C)"),
            $s.fd
                .as_deref_mut()
                .expect("pager: WAL sem arquivo do banco aberto"),
        )
    };
}
pub(crate) use wal_and_fd;

// ---------------------------------------------------------------------------
// Constantes
// ---------------------------------------------------------------------------

/// `Pager.eState`: ver o diagrama de estados do pager.c.
pub(crate) const PAGER_OPEN: u8 = 0;
pub(crate) const PAGER_READER: u8 = 1;
pub(crate) const PAGER_WRITER_LOCKED: u8 = 2;
pub(crate) const PAGER_WRITER_CACHEMOD: u8 = 3;
pub(crate) const PAGER_WRITER_DBMOD: u8 = 4;
pub(crate) const PAGER_WRITER_FINISHED: u8 = 5;
pub(crate) const PAGER_ERROR: u8 = 6;

/// `UNKNOWN_LOCK`: `Pager.eLock` depois de um `xUnlock` que falhou no estado de erro.
pub(crate) const UNKNOWN_LOCK: i32 = EXCLUSIVE_LOCK + 1;

/// `MAX_SECTOR_SIZE`: tamanho máximo de setor aceito (64 KiB).
pub(crate) const MAX_SECTOR_SIZE: i32 = 0x10000;

/// Bits de `Pager.doNotSpill`.
pub(crate) const SPILLFLAG_OFF: u8 = 0x01;
pub(crate) const SPILLFLAG_ROLLBACK: u8 = 0x02;
pub(crate) const SPILLFLAG_NOSYNC: u8 = 0x04;

/// Índices de `Pager.aStat`.
pub(crate) const PAGER_STAT_HIT: usize = 0;
pub(crate) const PAGER_STAT_MISS: usize = 1;
pub(crate) const PAGER_STAT_WRITE: usize = 2;
pub(crate) const PAGER_STAT_SPILL: usize = 3;

/// `aJournalMagic`: o início de todo cabeçalho de journal.
pub(crate) const A_JOURNAL_MAGIC: [u8; 8] = [0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7];

/// Bit que distingue o `PgId` de uma página mapeada (`mmap_pages`) do de uma página do pcache.
pub(crate) const PGID_MMAP_BASE: u32 = 0x8000_0000;

// ---------------------------------------------------------------------------
// Tipos auxiliares
// ---------------------------------------------------------------------------

/// Um savepoint ou transação de statement ativo (`PagerSavepoint`).
pub(crate) struct PagerSavepoint {
    /// `iOffset`: offset inicial no journal principal.
    pub(crate) i_offset: i64,
    /// `iHdrOffset`: offset logo depois do último registro escrito antes de um cabeçalho.
    pub(crate) i_hdr_offset: i64,
    /// `pInSavepoint`: conjunto das páginas deste savepoint (nunca nulo).
    pub(crate) in_savepoint: Box<Bitvec>,
    /// `nOrig`: número original de páginas do arquivo.
    pub(crate) n_orig: u32,
    /// `iSubRec`: índice do primeiro registro do sub-journal.
    pub(crate) i_sub_rec: u32,
    /// `bTruncateOnRelease`: o journal do statement pode ser truncado no RELEASE.
    pub(crate) b_truncate_on_release: bool,
    /// `aWalData`: contexto do savepoint no WAL.
    pub(crate) a_wal_data: [u32; WAL_SAVEPOINT_NDATA],
}

/// Qual rotina `xGet` está em uso (o `setGetterMethod` escolhe).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PagerGetter {
    /// `getPageNormal`.
    Normal,
    /// `getPageError`.
    Error,
    /// `getPageMMap`.
    MMap,
}

/// Objeto de página mapeada em memória (o `PgHdr` que `pagerAcquireMapPage` aloca).
pub(crate) struct MmapPage<E> {
    /// `pData`: cópia do que o `xFetch` devolveu.
    pub(crate) data: Vec<u8>,
    /// `pExtra`.
    pub(crate) extra: E,
    /// `pgno`.
    pub(crate) pgno: u32,
    /// `flags` (sempre `PGHDR_MMAP`).
    pub(crate) flags: u16,
    /// `nRef`.
    pub(crate) n_ref: i64,
}

/// A lista de backups em andamento (`sqlite3_backup *pBackup`) vista do pager: as duas
/// notificações que o pager.c faz.
pub trait PagerBackup {
    /// `sqlite3BackupRestart`.
    fn restart(&mut self);
    /// `sqlite3BackupUpdate(pBackup, iPage, aData)`.
    fn update(&mut self, pgno: u32, data: &[u8]);
}

/// Uma página a gravar no WAL (um elemento da lista `PgHdr` de `sqlite3WalFrames`). O WAL
/// lê `pgno` e `data` e pode ligar ou desligar `PGHDR_WAL_APPEND`, que o C altera em
/// `p->flags`; aqui o flag viaja em `wal_append` (entra com o valor atual e sai com o novo).
pub struct WalFrameSrc<'a> {
    pub pgno: u32,
    pub data: &'a [u8],
    pub wal_append: bool,
}

/// O pager (`struct Pager`). Ver o índice CAMPOS no topo do arquivo.
pub struct Pager<E: Default> {
    pub(crate) vfs: VfsRef,
    pub(crate) exclusive_mode: bool,
    pub(crate) journal_mode: i32,
    pub(crate) use_journal: bool,
    pub(crate) no_sync: bool,
    pub(crate) full_sync: bool,
    pub(crate) extra_sync: bool,
    pub(crate) sync_flags: i32,
    pub(crate) wal_sync_flags: i32,
    pub(crate) temp_file: bool,
    pub(crate) no_lock: bool,
    pub(crate) read_only: bool,
    pub(crate) mem_db: bool,
    pub(crate) mem_vfs: bool,

    // Bloco que muda na operação rotineira.
    pub(crate) e_state: u8,
    pub(crate) e_lock: i32,
    pub(crate) change_count_done: bool,
    pub(crate) set_super: bool,
    pub(crate) do_not_spill: u8,
    pub(crate) subj_in_memory: bool,
    pub(crate) b_use_fetch: bool,
    pub(crate) has_held_shared_lock: bool,
    pub(crate) db_size: u32,
    pub(crate) db_orig_size: u32,
    pub(crate) db_file_size: u32,
    pub(crate) db_hint_size: u32,
    pub(crate) err_code: i32,
    pub(crate) n_rec: i32,
    pub(crate) cksum_init: u32,
    pub(crate) n_sub_rec: u32,
    pub(crate) in_journal: Option<Box<Bitvec>>,
    pub(crate) fd: Option<Box<dyn VfsFile>>,
    pub(crate) jfd: Option<Box<dyn VfsFile>>,
    pub(crate) sjfd: Option<Box<dyn VfsFile>>,
    pub(crate) journal_off: i64,
    pub(crate) journal_hdr: i64,
    pub(crate) backup: Option<Box<dyn PagerBackup>>,
    pub(crate) a_savepoint: Vec<PagerSavepoint>,
    pub(crate) i_data_version: u32,
    pub(crate) db_file_vers: [u8; 16],
    pub(crate) n_mmap_out: i32,
    pub(crate) sz_mmap: i64,
    pub(crate) mmap_pages: Vec<MmapPage<E>>,
    pub(crate) mmap_freelist: Vec<u32>,

    pub(crate) n_extra: u16,
    pub(crate) n_reserve: i16,
    pub(crate) vfs_flags: u32,
    pub(crate) sector_size: u32,
    pub(crate) mx_pgno: u32,
    pub(crate) lck_pgno: u32,
    pub(crate) page_size: i64,
    pub(crate) journal_size_limit: i64,
    pub(crate) z_filename: Vec<u8>,
    pub(crate) z_journal: Vec<u8>,
    pub(crate) busy_handler: Option<Box<dyn FnMut() -> i32>>,
    pub(crate) a_stat: [u32; 4],
    pub(crate) reiniter: Option<fn(&mut E, &mut [u8], i64)>,
    pub(crate) x_get: PagerGetter,
    pub(crate) tmp_space: Vec<u8>,
    pub(crate) pcache: PCache<E>,
    pub(crate) wal: Option<Wal>,
    pub(crate) z_wal: Vec<u8>,
}

// ---------------------------------------------------------------------------
// Funções livres
// ---------------------------------------------------------------------------

/// Texto de uma "string C": os bytes até o primeiro NUL.
pub(crate) fn cstr(z: &[u8]) -> &[u8] {
    match z.iter().position(|&b| b == 0) {
        Some(n) => &z[..n],
        None => z,
    }
}

/// `sqlite3_filename` sem parâmetros de URI: o caminho, um NUL e o NUL duplo final. É o
/// formato que `Vfs::open` espera quando o pager abre um arquivo por nome solto.
pub(crate) fn filename_blob(path: &[u8]) -> Vec<u8> {
    let mut blob = cstr(path).to_vec();
    blob.push(0);
    blob.push(0);
    blob
}

/// `sqlite3JournalIsInMemory(p)` sobre um arquivo que pode estar fechado (`pMethods==0`).
pub(crate) fn is_mem_journal(f: &Option<Box<dyn VfsFile>>) -> bool {
    f.as_ref().is_some_and(|f| f.is_in_memory_journal())
}

/// `read32bits`: lê um inteiro de 32 bits (big-endian) do arquivo.
pub(crate) fn read32bits(fd: &mut dyn VfsFile, offset: i64, res: &mut u32) -> i32 {
    let mut ac = [0u8; 4];
    let rc = fd.read(&mut ac, offset);
    if rc == SQLITE_OK {
        *res = get4byte(&ac);
    }
    rc
}

/// `write32bits`: grava um inteiro de 32 bits (big-endian) no arquivo.
pub(crate) fn write32bits(fd: &mut dyn VfsFile, offset: i64, val: u32) -> i32 {
    let mut ac = [0u8; 4];
    put4byte(&mut ac, val);
    fd.write(&ac, offset)
}

/// `sqlite3SectorSize`: tamanho de setor saneado, entre 32 e `MAX_SECTOR_SIZE`.
pub(crate) fn sector_size(file: &mut dyn VfsFile) -> i32 {
    let mut i_ret = file.sector_size();
    if i_ret < 32 {
        i_ret = 512;
    } else if i_ret > MAX_SECTOR_SIZE {
        debug_assert!(MAX_SECTOR_SIZE >= 512);
        i_ret = MAX_SECTOR_SIZE;
    }
    i_ret
}

/// `readSuperJournal`: lê o nome do super-journal do fim do journal `jrnl`. O nome (sem NUL)
/// vai em `z_super`, que fica vazio se não houver nome (`zSuper[0] = 0` no C). Um nome com
/// NUL embutido vale só até o NUL, como a string C que ele é.
pub(crate) fn read_super_journal(jrnl: &mut dyn VfsFile, z_super: &mut Vec<u8>, n_super: u32) -> i32 {
    let mut len: u32 = 0;
    let mut sz_j: i64 = 0;
    let mut cksum: u32 = 0;
    let mut a_magic = [0u8; 8];
    z_super.clear();

    let mut rc = jrnl.file_size(&mut sz_j);
    if rc != SQLITE_OK || sz_j < 16 {
        return rc;
    }
    rc = read32bits(jrnl, sz_j - 16, &mut len);
    if rc != SQLITE_OK || len >= n_super || (len as i64) > sz_j - 16 || len == 0 {
        return rc;
    }
    rc = read32bits(jrnl, sz_j - 12, &mut cksum);
    if rc != SQLITE_OK {
        return rc;
    }
    rc = jrnl.read(&mut a_magic, sz_j - 8);
    if rc != SQLITE_OK || a_magic != A_JOURNAL_MAGIC {
        return rc;
    }
    let mut buf = vec![0u8; len as usize];
    rc = jrnl.read(&mut buf, sz_j - 16 - len as i64);
    if rc != SQLITE_OK {
        return rc;
    }

    // Confere o checksum com o nome (os bytes contam como `char` com sinal).
    for &b in &buf {
        cksum = cksum.wrapping_sub(b as i8 as i32 as u32);
    }
    if cksum == 0 {
        // Se o checksum não fecha, um setor com o nome está corrompido: reporta nome vazio.
        buf.truncate(cstr(&buf).len());
        *z_super = buf;
    }
    SQLITE_OK
}

// ---------------------------------------------------------------------------
// Pager
// ---------------------------------------------------------------------------

impl<E: Default> Pager<E> {
    /// O `Pager` recém-alocado com `sqlite3MallocZero` do `sqlite3PagerOpen`: tudo zerado,
    /// só o VFS e o cache de páginas já existem.
    pub(crate) fn zeroed(vfs: VfsRef, pcache: PCache<E>) -> Pager<E> {
        Pager {
            vfs,
            exclusive_mode: false,
            journal_mode: PAGER_JOURNALMODE_DELETE,
            use_journal: false,
            no_sync: false,
            full_sync: false,
            extra_sync: false,
            sync_flags: 0,
            wal_sync_flags: 0,
            temp_file: false,
            no_lock: false,
            read_only: false,
            mem_db: false,
            mem_vfs: false,
            e_state: PAGER_OPEN,
            e_lock: NO_LOCK,
            change_count_done: false,
            set_super: false,
            do_not_spill: 0,
            subj_in_memory: false,
            b_use_fetch: false,
            has_held_shared_lock: false,
            db_size: 0,
            db_orig_size: 0,
            db_file_size: 0,
            db_hint_size: 0,
            err_code: SQLITE_OK,
            n_rec: 0,
            cksum_init: 0,
            n_sub_rec: 0,
            in_journal: None,
            fd: None,
            jfd: None,
            sjfd: None,
            journal_off: 0,
            journal_hdr: 0,
            backup: None,
            a_savepoint: Vec::new(),
            i_data_version: 0,
            db_file_vers: [0; 16],
            n_mmap_out: 0,
            sz_mmap: 0,
            mmap_pages: Vec::new(),
            mmap_freelist: Vec::new(),
            n_extra: 0,
            n_reserve: 0,
            vfs_flags: 0,
            sector_size: 0,
            mx_pgno: 0,
            lck_pgno: 0,
            page_size: 0,
            journal_size_limit: 0,
            z_filename: Vec::new(),
            z_journal: Vec::new(),
            busy_handler: None,
            a_stat: [0; 4],
            reiniter: None,
            x_get: PagerGetter::Normal,
            tmp_space: Vec::new(),
            pcache,
            wal: None,
            z_wal: Vec::new(),
        }
    }

    // ------------------------------------------------------------------
    // Acesso às páginas (pcache ou mapeadas)
    // ------------------------------------------------------------------

    /// Bytes da página (`pPg->pData`).
    pub(crate) fn page_data(&self, pg: PgId) -> &[u8] {
        if pg.0 & PGID_MMAP_BASE != 0 {
            &self.mmap_pages[(pg.0 & !PGID_MMAP_BASE) as usize].data
        } else {
            self.pcache.page_data(pg)
        }
    }

    /// Bytes da página, para escrita.
    pub(crate) fn page_data_mut(&mut self, pg: PgId) -> &mut [u8] {
        if pg.0 & PGID_MMAP_BASE != 0 {
            &mut self.mmap_pages[(pg.0 & !PGID_MMAP_BASE) as usize].data
        } else {
            self.pcache.page_data_mut(pg)
        }
    }

    /// `pPg->pExtra`.
    pub(crate) fn page_extra(&self, pg: PgId) -> &E {
        if pg.0 & PGID_MMAP_BASE != 0 {
            &self.mmap_pages[(pg.0 & !PGID_MMAP_BASE) as usize].extra
        } else {
            self.pcache.page_extra(pg)
        }
    }

    /// `pPg->pExtra`, para escrita.
    pub(crate) fn page_extra_mut(&mut self, pg: PgId) -> &mut E {
        if pg.0 & PGID_MMAP_BASE != 0 {
            &mut self.mmap_pages[(pg.0 & !PGID_MMAP_BASE) as usize].extra
        } else {
            self.pcache.page_extra_mut(pg)
        }
    }

    /// Empréstimo dividido: o `pExtra` e os bytes da mesma página ao mesmo tempo.
    pub(crate) fn page_parts(&mut self, pg: PgId) -> (&mut E, &mut [u8]) {
        if pg.0 & PGID_MMAP_BASE != 0 {
            let p = &mut self.mmap_pages[(pg.0 & !PGID_MMAP_BASE) as usize];
            (&mut p.extra, p.data.as_mut_slice())
        } else {
            self.pcache.page_parts(pg)
        }
    }

    /// `pPg->pgno`.
    pub(crate) fn page_pgno(&self, pg: PgId) -> u32 {
        if pg.0 & PGID_MMAP_BASE != 0 {
            self.mmap_pages[(pg.0 & !PGID_MMAP_BASE) as usize].pgno
        } else {
            self.pcache.page_pgno(pg)
        }
    }

    /// `pPg->flags`.
    pub(crate) fn page_flags(&self, pg: PgId) -> u16 {
        if pg.0 & PGID_MMAP_BASE != 0 {
            self.mmap_pages[(pg.0 & !PGID_MMAP_BASE) as usize].flags
        } else {
            self.pcache.page_flags(pg)
        }
    }

    /// `pPg->flags`, para escrita.
    pub(crate) fn page_flags_mut(&mut self, pg: PgId) -> &mut u16 {
        if pg.0 & PGID_MMAP_BASE != 0 {
            &mut self.mmap_pages[(pg.0 & !PGID_MMAP_BASE) as usize].flags
        } else {
            self.pcache.page_flags_mut(pg)
        }
    }

    /// `pPg->nRef`.
    pub(crate) fn page_ref_count(&self, pg: PgId) -> i64 {
        if pg.0 & PGID_MMAP_BASE != 0 {
            self.mmap_pages[(pg.0 & !PGID_MMAP_BASE) as usize].n_ref
        } else {
            self.pcache.page_ref_count(pg)
        }
    }

    // ------------------------------------------------------------------
    // Macros do C
    // ------------------------------------------------------------------

    /// `JOURNAL_HDR_SZ(pPager)`: o tamanho do cabeçalho do journal (o do setor).
    #[inline]
    pub(crate) fn journal_hdr_sz(&self) -> i64 {
        self.sector_size as i64
    }

    /// `JOURNAL_PG_SZ(pPager)`: o tamanho de cada registro de página no journal.
    #[inline]
    pub(crate) fn journal_pg_sz(&self) -> i64 {
        self.page_size + 8
    }

    /// `pagerUseWal(x)`: o pager usa WAL?
    #[inline]
    pub(crate) fn pager_use_wal(&self) -> bool {
        self.wal.is_some()
    }

    /// `sqlite3OsDeviceCharacteristics(pPager->fd)`: arquivo fechado vale 0.
    pub(crate) fn fd_device_characteristics(&mut self) -> i32 {
        self.fd
            .as_deref_mut()
            .map_or(0, |f| f.device_characteristics())
    }

    /// `sqlite3BackupRestart(pPager->pBackup)`: sem backup, nada a fazer.
    pub(crate) fn backup_restart(&mut self) {
        if let Some(b) = self.backup.as_mut() {
            b.restart();
        }
    }

    // ------------------------------------------------------------------
    // sqlite3PagerDirectReadOk (SQLITE_DIRECT_OVERFLOW_READ)
    // ------------------------------------------------------------------

    /// `sqlite3PagerDirectReadOk`: a página `pgno` pode ser lida direto do arquivo do banco
    /// pelo btree? Sim se o arquivo está aberto, não há páginas sujas e a página não está
    /// no WAL.
    pub(crate) fn direct_read_ok(&mut self, pgno: u32) -> bool {
        if self.fd.is_none() {
            return false;
        }
        if self.pcache.is_dirty() {
            return false;
        }
        if self.pager_use_wal() {
            let mut i_read: u32 = 0;
            let (wal, fd) = wal_and_fd!(self);
            let _ = wal.find_frame(fd, pgno, &mut i_read);
            return i_read == 0;
        }
        true
    }

    // ------------------------------------------------------------------
    // assert_pager_state
    // ------------------------------------------------------------------

    /// `assert_pager_state`: confere as invariantes do estado do pager. Sempre devolve `true`
    /// (as conferências são `debug_assert!`).
    pub(crate) fn assert_pager_state(&mut self) -> bool {
        // O estado precisa ser válido.
        debug_assert!(
            self.e_state == PAGER_OPEN
                || self.e_state == PAGER_READER
                || self.e_state == PAGER_WRITER_LOCKED
                || self.e_state == PAGER_WRITER_CACHEMOD
                || self.e_state == PAGER_WRITER_DBMOD
                || self.e_state == PAGER_WRITER_FINISHED
                || self.e_state == PAGER_ERROR
        );

        // Arquivo temporário se comporta como se tivesse trava exclusiva e nunca atualiza o
        // change-counter.
        debug_assert!(!self.temp_file || self.e_lock == EXCLUSIVE_LOCK);
        debug_assert!(!self.temp_file || self.change_count_done);

        // Sem useJournal, o modo é OFF; e com modo OFF o journal não pode estar aberto.
        debug_assert!(self.journal_mode == PAGER_JOURNALMODE_OFF || self.use_journal);
        debug_assert!(self.journal_mode != PAGER_JOURNALMODE_OFF || self.jfd.is_none());

        // MEMDB implica noSync e journal em memória.
        if self.mem_db {
            debug_assert!(self.fd.is_none());
            debug_assert!(self.no_sync);
            debug_assert!(
                self.journal_mode == PAGER_JOURNALMODE_OFF
                    || self.journal_mode == PAGER_JOURNALMODE_MEMORY
            );
            debug_assert!(self.e_state != PAGER_ERROR && self.e_state != PAGER_OPEN);
            debug_assert!(!self.pager_use_wal());
        }

        // Com changeCountDone ligado, há trava RESERVED ou maior.
        debug_assert!(!self.change_count_done || self.e_lock >= RESERVED_LOCK);
        debug_assert!(self.e_lock != PENDING_LOCK);

        let batch_atomic = self.e_state == PAGER_WRITER_DBMOD || self.e_state == PAGER_WRITER_FINISHED;
        let dc_batch = if batch_atomic {
            self.fd_device_characteristics() & SQLITE_IOCAP_BATCH_ATOMIC != 0
        } else {
            false
        };
        let jfd_open = self.jfd.is_some();
        let use_wal = self.pager_use_wal();

        match self.e_state {
            PAGER_OPEN => {
                debug_assert!(!self.mem_db);
                debug_assert!(self.err_code == SQLITE_OK);
                debug_assert!(self.pcache.ref_count() == 0 || self.temp_file);
            }
            PAGER_READER => {
                debug_assert!(self.err_code == SQLITE_OK);
                debug_assert!(self.e_lock != UNKNOWN_LOCK);
                debug_assert!(self.e_lock >= SHARED_LOCK);
            }
            PAGER_WRITER_LOCKED => {
                debug_assert!(self.e_lock != UNKNOWN_LOCK);
                debug_assert!(self.err_code == SQLITE_OK);
                if !use_wal {
                    debug_assert!(self.e_lock >= RESERVED_LOCK);
                }
                debug_assert!(self.db_size == self.db_orig_size);
                debug_assert!(self.db_orig_size == self.db_file_size);
                debug_assert!(self.db_orig_size == self.db_hint_size);
                debug_assert!(!self.set_super);
            }
            PAGER_WRITER_CACHEMOD => {
                debug_assert!(self.e_lock != UNKNOWN_LOCK);
                debug_assert!(self.err_code == SQLITE_OK);
                if !use_wal {
                    // Com journal_mode=wal pode não haver journal nem WAL abertos: ocorre
                    // num rollback que troca de journal_mode=off para journal_mode=wal.
                    debug_assert!(self.e_lock >= RESERVED_LOCK);
                    debug_assert!(
                        jfd_open
                            || self.journal_mode == PAGER_JOURNALMODE_OFF
                            || self.journal_mode == PAGER_JOURNALMODE_WAL
                    );
                }
                debug_assert!(self.db_orig_size == self.db_file_size);
                debug_assert!(self.db_orig_size == self.db_hint_size);
            }
            PAGER_WRITER_DBMOD => {
                debug_assert!(self.e_lock == EXCLUSIVE_LOCK);
                debug_assert!(self.err_code == SQLITE_OK);
                debug_assert!(!use_wal);
                debug_assert!(self.e_lock >= EXCLUSIVE_LOCK);
                debug_assert!(
                    jfd_open
                        || self.journal_mode == PAGER_JOURNALMODE_OFF
                        || self.journal_mode == PAGER_JOURNALMODE_WAL
                        || dc_batch
                );
                debug_assert!(self.db_orig_size <= self.db_hint_size);
            }
            PAGER_WRITER_FINISHED => {
                debug_assert!(self.e_lock == EXCLUSIVE_LOCK);
                debug_assert!(self.err_code == SQLITE_OK);
                debug_assert!(!use_wal);
                debug_assert!(
                    jfd_open
                        || self.journal_mode == PAGER_JOURNALMODE_OFF
                        || self.journal_mode == PAGER_JOURNALMODE_WAL
                        || dc_batch
                );
            }
            PAGER_ERROR => {
                // Em ERROR há pelo menos uma referência pendente; senão o pager já teria
                // voltado a OPEN.
                debug_assert!(self.err_code != SQLITE_OK);
                debug_assert!(self.pcache.ref_count() > 0 || self.temp_file);
            }
            _ => {}
        }
        true
    }

    // ------------------------------------------------------------------
    // Getter, savepoints, 32 bits, travas
    // ------------------------------------------------------------------

    /// `setGetterMethod`: escolhe a rotina `xGet`.
    pub(crate) fn set_getter_method(&mut self) {
        self.x_get = if self.err_code != 0 {
            PagerGetter::Error
        } else if self.b_use_fetch {
            PagerGetter::MMap
        } else {
            PagerGetter::Normal
        };
    }

    /// `subjRequiresPage`: é preciso gravar a página no sub-journal? Sim se existe um
    /// savepoint aberto em que `pgno <= nOrig` e o bit da página não está em `pInSavepoint`.
    pub(crate) fn subj_requires_page(&mut self, pg: PgId) -> bool {
        let pgno = self.page_pgno(pg);
        let n = self.a_savepoint.len();
        let mut i = 0;
        while i < n {
            let p = &self.a_savepoint[i];
            if p.n_orig >= pgno && !bitvec_test_not_null(&p.in_savepoint, pgno) {
                for j in (i + 1)..n {
                    self.a_savepoint[j].b_truncate_on_release = false;
                }
                return true;
            }
            i += 1;
        }
        false
    }

    /// `pageInJournal` (só aparece em asserts no C): a página já está no journal?
    pub(crate) fn page_in_journal(&self, pg: PgId) -> bool {
        bitvec_test(self.in_journal.as_deref(), self.page_pgno(pg))
    }

    /// `pagerUnlockDb`: desce a trava do banco para `e_lock` (`NO_LOCK` ou `SHARED_LOCK`).
    /// `Pager.eLock` acompanha a trava tentada, mesmo que o `xUnlock` falhe, salvo se
    /// valer `UNKNOWN_LOCK`.
    pub(crate) fn pager_unlock_db(&mut self, e_lock: i32) -> i32 {
        let mut rc = SQLITE_OK;

        debug_assert!(!self.exclusive_mode || self.e_lock == e_lock);
        debug_assert!(e_lock == NO_LOCK || e_lock == SHARED_LOCK);
        debug_assert!(e_lock != NO_LOCK || !self.pager_use_wal());
        if let Some(fd) = self.fd.as_deref_mut() {
            debug_assert!(self.e_lock >= e_lock);
            rc = if self.no_lock {
                SQLITE_OK
            } else {
                fd.unlock(e_lock)
            };
            if self.e_lock != UNKNOWN_LOCK {
                self.e_lock = e_lock;
            }
        }
        self.change_count_done = self.temp_file; // ticket fb3b3024ea238d5c
        rc
    }

    /// `pagerLockDb`: sobe a trava do banco para `e_lock` (`SHARED_LOCK`, `RESERVED_LOCK` ou
    /// `EXCLUSIVE_LOCK`). `Pager.eLock` só muda se o `xLock` der certo (e, com
    /// `UNKNOWN_LOCK`, só para `EXCLUSIVE_LOCK`).
    pub(crate) fn pager_lock_db(&mut self, e_lock: i32) -> i32 {
        let mut rc = SQLITE_OK;

        debug_assert!(e_lock == SHARED_LOCK || e_lock == RESERVED_LOCK || e_lock == EXCLUSIVE_LOCK);
        if self.e_lock < e_lock || self.e_lock == UNKNOWN_LOCK {
            rc = if self.no_lock {
                SQLITE_OK
            } else {
                file_of!(self, fd).lock(e_lock)
            };
            if rc == SQLITE_OK && (self.e_lock != UNKNOWN_LOCK || e_lock == EXCLUSIVE_LOCK) {
                self.e_lock = e_lock;
            }
        }
        rc
    }

    /// `jrnlBufferSize`: tamanho do journal quando ele contém o registro de uma única página
    /// (otimização de escrita atômica), -1 para escrita atômica em lote, 0 sem otimização.
    /// Sem `SQLITE_ENABLE_ATOMIC_WRITE` e `SQLITE_ENABLE_BATCH_ATOMIC_WRITE` (Debian) é 0.
    pub(crate) fn jrnl_buffer_size(&self) -> i32 {
        debug_assert!(!self.mem_db);
        0
    }

    // ------------------------------------------------------------------
    // Cabeçalho do journal
    // ------------------------------------------------------------------

    /// `journalHdrOffset`: o offset da fronteira de setor igual ou logo depois de
    /// `journalOff`.
    pub(crate) fn journal_hdr_offset(&self) -> i64 {
        let mut offset = 0;
        let c = self.journal_off;
        if c != 0 {
            offset = ((c - 1) / self.journal_hdr_sz() + 1) * self.journal_hdr_sz();
        }
        debug_assert!(offset % self.journal_hdr_sz() == 0);
        debug_assert!(offset >= c);
        debug_assert!((offset - c) < self.journal_hdr_sz());
        offset
    }

    /// `zeroJournalHdr`: finaliza o journal persistente. Nada acontece se o journal não foi
    /// escrito na transação (`journalOff == 0`). Com `do_truncate` ou `journalSizeLimit == 0`
    /// o arquivo é truncado a zero; senão os 28 bytes do cabeçalho são zerados. Sem
    /// `noSync` o arquivo é sincronizado. Com limite positivo, trunca ao limite se maior.
    pub(crate) fn zero_journal_hdr(&mut self, do_truncate: bool) -> i32 {
        let mut rc = SQLITE_OK;
        debug_assert!(self.jfd.is_some());
        debug_assert!(!is_mem_journal(&self.jfd));
        if self.journal_off != 0 {
            let i_limit = self.journal_size_limit;

            if do_truncate || i_limit == 0 {
                rc = file_of!(self, jfd).truncate(0);
            } else {
                let zero_hdr = [0u8; 28];
                rc = file_of!(self, jfd).write(&zero_hdr, 0);
            }
            if rc == SQLITE_OK && !self.no_sync {
                rc = os_sync(file_of!(self, jfd), SQLITE_SYNC_DATAONLY | self.sync_flags);
            }

            // A transação está confirmada mas a trava de escrita segue presa: se há limite
            // para o journal persistente e ele o excede, trunca agora (sem sync).
            if rc == SQLITE_OK && i_limit > 0 {
                let mut sz: i64 = 0;
                rc = file_of!(self, jfd).file_size(&mut sz);
                if rc == SQLITE_OK && sz > i_limit {
                    rc = file_of!(self, jfd).truncate(i_limit);
                }
            }
        }
        rc
    }

    /// `writeJournalHdr`: escreve um cabeçalho de journal (`JOURNAL_HDR_SZ` bytes) na posição
    /// atual. Formato: 8 bytes de magia, 4 de nRec (ou -1 em no-sync), 4 do inicializador
    /// do checksum, 4 do tamanho inicial do banco, 4 do tamanho de setor, 4 do tamanho de
    /// página, e o resto sem uso.
    pub(crate) fn write_journal_hdr(&mut self) -> i32 {
        let mut rc = SQLITE_OK;
        let mut n_header = self.page_size as u32;

        debug_assert!(self.jfd.is_some());

        if n_header as i64 > self.journal_hdr_sz() {
            n_header = self.journal_hdr_sz() as u32;
        }

        // Savepoints criados desde o último cabeçalho ganham o iHdrOffset.
        for ii in 0..self.a_savepoint.len() {
            if self.a_savepoint[ii].i_hdr_offset == 0 {
                self.a_savepoint[ii].i_hdr_offset = self.journal_off;
            }
        }

        self.journal_off = self.journal_hdr_offset();
        self.journal_hdr = self.journal_off;

        // O campo nRec: normalmente zero agora e corrigido depois (syncJournal). Com 0xFFFFFFFF
        // o leitor assume que o resto do arquivo são registros válidos; isso só é seguro em
        // modo no-sync, em journal de memória ou com SQLITE_IOCAP_SAFE_APPEND.
        debug_assert!(self.fd.is_some() || self.no_sync);
        let assume_valid = self.no_sync
            || self.journal_mode == PAGER_JOURNALMODE_MEMORY
            || (self.fd_device_characteristics() & SQLITE_IOCAP_SAFE_APPEND) != 0;
        if assume_valid {
            self.tmp_space[..8].copy_from_slice(&A_JOURNAL_MAGIC);
            put4byte(&mut self.tmp_space[8..], 0xffffffff);
        } else {
            self.tmp_space[..12].fill(0);
        }

        // O inicializador aleatório do checksum.
        if self.journal_mode != PAGER_JOURNALMODE_MEMORY {
            let mut b = [0u8; 4];
            crate::global::randomness(&mut b);
            self.cksum_init = u32::from_ne_bytes(b);
        }
        put4byte(&mut self.tmp_space[12..], self.cksum_init);

        // O tamanho inicial do banco.
        put4byte(&mut self.tmp_space[16..], self.db_orig_size);
        // O tamanho de setor assumido por este processo.
        put4byte(&mut self.tmp_space[20..], self.sector_size);
        // O tamanho da página.
        put4byte(&mut self.tmp_space[24..], self.page_size as u32);

        // Inicializar o resto não é necessário, mas evita reclamação do valgrind.
        self.tmp_space[28..n_header as usize].fill(0);

        // Em teoria bastam os 28 bytes do cabeçalho, mas escrever o setor inteiro de forma
        // contígua é bem mais rápido em alguns sistemas. O laço existe porque o setor pode
        // ser maior que a página, e o buffer tem só `pageSize` bytes.
        let mut n_write: u32 = 0;
        while rc == SQLITE_OK && (n_write as i64) < self.journal_hdr_sz() {
            rc = file_of!(self, jfd).write(&self.tmp_space[..n_header as usize], self.journal_off);
            debug_assert!(self.journal_hdr <= self.journal_off);
            self.journal_off += n_header as i64;
            n_write += n_header;
        }

        rc
    }

    /// `readJournalHdr`: lê um cabeçalho do journal na posição `journalOff`. Em sucesso
    /// `n_rec` recebe o número de registros que seguem, `db_size` o tamanho original do banco
    /// em páginas e `cksumInit` o valor do cabeçalho. `SQLITE_DONE` indica cabeçalho corrompido
    /// ou ausente; outro código é erro de leitura.
    pub(crate) fn read_journal_hdr(
        &mut self,
        is_hot: bool,
        journal_size: i64,
        p_n_rec: &mut u32,
        p_db_size: &mut u32,
    ) -> i32 {
        let mut rc: i32;
        let mut a_magic = [0u8; 8];

        debug_assert!(self.jfd.is_some());

        // Avança journalOff para o início do próximo setor. Sem espaço para um cabeçalho,
        // `SQLITE_DONE`.
        self.journal_off = self.journal_hdr_offset();
        if self.journal_off + self.journal_hdr_sz() > journal_size {
            return SQLITE_DONE;
        }
        let i_hdr_off = self.journal_off;

        // Os 8 primeiros bytes precisam bater com a magia; senão `SQLITE_DONE`.
        if is_hot || i_hdr_off != self.journal_hdr {
            rc = file_of!(self, jfd).read(&mut a_magic, i_hdr_off);
            if rc != 0 {
                return rc;
            }
            if a_magic != A_JOURNAL_MAGIC {
                return SQLITE_DONE;
            }
        }

        // nRec, o inicializador do checksum e o tamanho do banco no início da transação.
        rc = read32bits(file_of!(self, jfd), i_hdr_off + 8, p_n_rec);
        if rc != SQLITE_OK {
            return rc;
        }
        rc = read32bits(file_of!(self, jfd), i_hdr_off + 12, &mut self.cksum_init);
        if rc != SQLITE_OK {
            return rc;
        }
        rc = read32bits(file_of!(self, jfd), i_hdr_off + 16, p_db_size);
        if rc != SQLITE_OK {
            return rc;
        }

        if self.journal_off == 0 {
            let mut i_page_size: u32 = 0;
            let mut i_sector_size: u32 = 0;

            // Os campos de tamanho de página e de setor.
            rc = read32bits(file_of!(self, jfd), i_hdr_off + 20, &mut i_sector_size);
            if rc != SQLITE_OK {
                return rc;
            }
            rc = read32bits(file_of!(self, jfd), i_hdr_off + 24, &mut i_page_size);
            if rc != SQLITE_OK {
                return rc;
            }

            // Versões anteriores à 3.5.8 gravavam zero no tamanho de página.
            if i_page_size == 0 {
                i_page_size = self.page_size as u32;
            }

            // Os dois precisam ser potências de dois dentro dos limites; senão o processo que
            // escreveu o cabeçalho caiu antes de sincronizá-lo: para de ler aqui.
            if i_page_size < 512
                || i_sector_size < 32
                || i_page_size > SQLITE_MAX_PAGE_SIZE as u32
                || i_sector_size > MAX_SECTOR_SIZE as u32
                || (i_page_size.wrapping_sub(1) & i_page_size) != 0
                || (i_sector_size.wrapping_sub(1) & i_sector_size) != 0
            {
                return SQLITE_DONE;
            }

            // Ajusta o tamanho de página ao do journal.
            rc = self.set_pagesize(&mut i_page_size, -1);

            // O tamanho de setor assumido passa a ser o do processo que criou o journal;
            // pager_playback restaura o valor local ao final.
            self.sector_size = i_sector_size;
        }

        self.journal_off += self.journal_hdr_sz();
        rc
    }

    /// `writeSuperJournal`: grava o nome do super-journal no fim do journal, na posição atual
    /// (alinhada ao próximo setor em modo full-sync). Formato: 4 bytes `PAGER_SJ_PGNO`, o nome
    /// em UTF-8, 4 bytes com o comprimento, 4 com o checksum (soma dos bytes do nome como
    /// inteiros de 8 bits com sinal) e 8 de magia. Sem nome é um no-op.
    pub(crate) fn write_super_journal(&mut self, z_super: Option<&[u8]>) -> i32 {
        debug_assert!(!self.set_super);
        debug_assert!(!self.pager_use_wal());

        let z_super = match z_super {
            Some(z) if self.journal_mode != PAGER_JOURNALMODE_MEMORY && self.jfd.is_some() => {
                cstr(z)
            }
            _ => return SQLITE_OK,
        };
        self.set_super = true;
        debug_assert!(self.journal_hdr <= self.journal_off);

        // Comprimento e checksum do nome.
        let n_super = z_super.len() as i64;
        let mut cksum: u32 = 0;
        for &b in z_super {
            cksum = cksum.wrapping_add(b as i8 as i32 as u32);
        }

        // Em full-sync avança até o próximo setor antes de escrever o nome, caso a última
        // página escrita no journal já tenha sido sincronizada.
        if self.full_sync {
            self.journal_off = self.journal_hdr_offset();
        }
        let i_hdr_off = self.journal_off;

        // Escreve os dados no fim do journal; qualquer erro volta ao chamador.
        let mut rc = write32bits(file_of!(self, jfd), i_hdr_off, self.lck_pgno);
        if rc != 0 {
            return rc;
        }
        rc = file_of!(self, jfd).write(z_super, i_hdr_off + 4);
        if rc != 0 {
            return rc;
        }
        rc = write32bits(file_of!(self, jfd), i_hdr_off + 4 + n_super, n_super as u32);
        if rc != 0 {
            return rc;
        }
        rc = write32bits(file_of!(self, jfd), i_hdr_off + 4 + n_super + 4, cksum);
        if rc != 0 {
            return rc;
        }
        rc = file_of!(self, jfd).write(&A_JOURNAL_MAGIC, i_hdr_off + 4 + n_super + 8);
        if rc != 0 {
            return rc;
        }
        self.journal_off += n_super + 20;

        // Em modo persistente o arquivo físico pode passar do fim do nome e da magia, o que
        // impediria achar o super-journal na reversão de um journal quente: trunca.
        let mut jrnl_size: i64 = 0;
        rc = file_of!(self, jfd).file_size(&mut jrnl_size);
        if rc == SQLITE_OK && jrnl_size > self.journal_off {
            rc = file_of!(self, jfd).truncate(self.journal_off);
        }
        rc
    }

    // ------------------------------------------------------------------
    // Reset, savepoints, unlock, erro, fim de transação
    // ------------------------------------------------------------------

    /// `pager_reset`: descarta todo o conteúdo do cache de páginas.
    pub(crate) fn pager_reset(&mut self) {
        self.i_data_version = self.i_data_version.wrapping_add(1);
        self.backup_restart();
        self.pcache.clear();
    }

    /// `sqlite3PagerDataVersion`.
    pub(crate) fn data_version(&self) -> u32 {
        self.i_data_version
    }

    /// `releaseAllSavepoints`: libera todos os savepoints e fecha o sub-journal se ele está
    /// aberto e o pager não está em modo exclusivo (ou se é um journal em memória).
    pub(crate) fn release_all_savepoints(&mut self) {
        // Os bitvecs (`sqlite3BitvecDestroy`) caem junto com o vetor.
        if !self.exclusive_mode || is_mem_journal(&self.sjfd) {
            os_close(&mut self.sjfd);
        }
        self.a_savepoint.clear();
        self.n_sub_rec = 0;
    }

    /// `addToSavepointBitvecs`: liga o bit `pgno` em `pInSavepoint` de todos os savepoints
    /// abertos para os quais `pgno <= nOrig`.
    pub(crate) fn add_to_savepoint_bitvecs(&mut self, pgno: u32) -> i32 {
        let mut rc = SQLITE_OK;
        for p in self.a_savepoint.iter_mut() {
            if pgno <= p.n_orig {
                rc |= bitvec_set(Some(&mut *p.in_savepoint), pgno);
                debug_assert!(rc == SQLITE_OK || rc == crate::consts::SQLITE_NOMEM);
            }
        }
        rc
    }

    /// `pager_unlock`: leva o pager a `PAGER_OPEN` (no-op em modo exclusivo fora do estado de
    /// erro). Fora do modo exclusivo solta a trava do banco e, se o sistema de arquivos não
    /// tem `UNDELETABLE_WHEN_OPEN`, fecha o journal. No estado de erro descarta o cache.
    pub(crate) fn pager_unlock(&mut self) {
        debug_assert!(
            self.e_state == PAGER_READER || self.e_state == PAGER_OPEN || self.e_state == PAGER_ERROR
        );

        self.in_journal = None;
        self.release_all_savepoints();

        if self.pager_use_wal() {
            debug_assert!(self.jfd.is_none());
            let (wal, fd) = wal_and_fd!(self);
            wal.end_read_transaction(fd);
            self.e_state = PAGER_OPEN;
        } else if !self.exclusive_mode {
            let i_dc = self.fd_device_characteristics();

            // Se o sistema de arquivos aceita apagar arquivo aberto, fecha o journal ao soltar
            // a trava do banco; senão outra conexão com journal_mode=delete poderia apagá-lo.
            // (Os asserts de `(modo & 5) == 1` valem só para TRUNCATE e PERSIST.)
            if 0 == (i_dc & SQLITE_IOCAP_UNDELETABLE_WHEN_OPEN) || 1 != (self.journal_mode & 5) {
                os_close(&mut self.jfd);
            }

            // Se a trava falhar no estado de erro, eLock vira UNKNOWN_LOCK.
            let rc = self.pager_unlock_db(NO_LOCK);
            if rc != SQLITE_OK && self.e_state == PAGER_ERROR {
                self.e_lock = UNKNOWN_LOCK;
            }

            // De PAGER_ERROR para PAGER_OPEN sem limpar o erro: isso é intencional, o código
            // de erro é limpo e o cache zerado logo abaixo.
            debug_assert!(self.err_code != 0 || self.e_state != PAGER_ERROR);
            self.e_state = PAGER_OPEN;
        }

        // Com errCode ligado o cache não é confiável; sem referências pendentes o pager pode
        // voltar a PAGER_OPEN, em modo normal ou exclusivo.
        debug_assert!(self.err_code == SQLITE_OK || !self.mem_db);
        if self.err_code != 0 {
            if !self.temp_file {
                self.pager_reset();
                self.change_count_done = false;
                self.e_state = PAGER_OPEN;
            } else {
                self.e_state = if self.jfd.is_some() {
                    PAGER_OPEN
                } else {
                    PAGER_READER
                };
            }
            if self.b_use_fetch {
                if let Some(fd) = self.fd.as_deref_mut() {
                    fd.unfetch(0, None);
                }
            }
            self.err_code = SQLITE_OK;
            self.set_getter_method();
        }

        self.journal_off = 0;
        self.journal_hdr = 0;
        self.set_super = false;
    }

    /// `pager_error`: chamada quando pode ter ocorrido um erro IOERR ou FULL que exige o estado
    /// de erro. Devolve `rc`. Para `SQLITE_FULL`, `SQLITE_IOERR` ou subcódigos, o pager entra
    /// em `PAGER_ERROR` e guarda o código em `errCode`.
    pub(crate) fn pager_error(&mut self, rc: i32) -> i32 {
        let rc2 = rc & 0xff;
        debug_assert!(rc == SQLITE_OK || !self.mem_db);
        debug_assert!(
            self.err_code == SQLITE_FULL
                || self.err_code == SQLITE_OK
                || (self.err_code & 0xff) == SQLITE_IOERR
        );
        if rc2 == SQLITE_FULL || rc2 == SQLITE_IOERR {
            self.err_code = rc;
            self.e_state = PAGER_ERROR;
            self.set_getter_method();
        }
        rc
    }

    /// `pagerFlushOnCommit`: todas as páginas sujas devem ser gravadas? Em bancos não
    /// temporários sempre. Em temporários só no COMMIT (não no ROLLBACK), com o arquivo
    /// de apoio já criado (por um spill) e mais de 25% do cache sujo.
    pub(crate) fn pager_flush_on_commit(&self, b_commit: bool) -> bool {
        if !self.temp_file {
            return true;
        }
        if !b_commit {
            return false;
        }
        if self.fd.is_none() {
            return false;
        }
        self.pcache.percent_dirty() >= 25
    }

    /// `pager_end_transaction`: encerra a transação (COMMIT ou ROLLBACK). Libera os savepoints
    /// e finaliza o journal conforme o modo (MEMORY fecha, TRUNCATE trunca, PERSIST zera o
    /// cabeçalho, DELETE fecha e apaga). Depois move o pager a `PAGER_READER` e, se não é modo
    /// exclusivo, volta a trava a SHARED. Devolve o primeiro erro.
    pub(crate) fn pager_end_transaction(&mut self, has_super: bool, b_commit: bool) -> i32 {
        let mut rc = SQLITE_OK;
        let mut rc2 = SQLITE_OK;

        // Nada a fazer sem transação de escrita ou trava RESERVED.
        debug_assert!(self.assert_pager_state());
        debug_assert!(self.e_state != PAGER_ERROR);
        if self.e_state < PAGER_WRITER_LOCKED && self.e_lock < RESERVED_LOCK {
            return SQLITE_OK;
        }

        self.release_all_savepoints();
        debug_assert!(
            self.jfd.is_some()
                || self.in_journal.is_none()
                || (self.fd_device_characteristics() & SQLITE_IOCAP_BATCH_ATOMIC) != 0
        );
        if self.jfd.is_some() {
            debug_assert!(!self.pager_use_wal());

            // Finaliza o journal.
            if is_mem_journal(&self.jfd) {
                os_close(&mut self.jfd);
            } else if self.journal_mode == PAGER_JOURNALMODE_TRUNCATE {
                if self.journal_off == 0 {
                    rc = SQLITE_OK;
                } else {
                    rc = file_of!(self, jfd).truncate(0);
                    if rc == SQLITE_OK && self.full_sync {
                        // Grava o novo tamanho no inode já: o journal poderia ressuscitar
                        // depois de uma queda de energia e reverter a última transação.
                        rc = os_sync(file_of!(self, jfd), self.sync_flags);
                    }
                }
                self.journal_off = 0;
            } else if self.journal_mode == PAGER_JOURNALMODE_PERSIST
                || (self.exclusive_mode && self.journal_mode != PAGER_JOURNALMODE_WAL)
            {
                rc = self.zero_journal_hdr(has_super || self.temp_file);
                self.journal_off = 0;
            } else {
                // Também executado com journalMode==MEMORY depois da reversão de um journal
                // quente: o arquivo é fechado e apagado.
                let b_delete = !self.temp_file;
                debug_assert!(!is_mem_journal(&self.jfd));
                debug_assert!(
                    self.journal_mode == PAGER_JOURNALMODE_DELETE
                        || self.journal_mode == PAGER_JOURNALMODE_MEMORY
                        || self.journal_mode == PAGER_JOURNALMODE_WAL
                );
                os_close(&mut self.jfd);
                if b_delete {
                    rc = self
                        .vfs
                        .delete(cstr(&self.z_journal), self.extra_sync as i32);
                }
            }
        }

        self.in_journal = None;
        self.n_rec = 0;
        if rc == SQLITE_OK {
            if self.mem_db || self.pager_flush_on_commit(b_commit) {
                self.pcache.clean_all();
            } else {
                self.pcache.clear_writable();
            }
            self.pcache.truncate(self.db_size);
        }

        if self.pager_use_wal() {
            // Solta a trava de escrita do WAL, se houver; e, se a conexão estava em
            // locking_mode=exclusive e não está mais, solta a trava EXCLUSIVE do banco.
            let (wal, fd) = wal_and_fd!(self);
            rc2 = wal.end_write_transaction(fd);
            debug_assert!(rc2 == SQLITE_OK);
        } else if rc == SQLITE_OK && b_commit && self.db_file_size > self.db_size {
            // No modo journal, se o arquivo em disco é maior que a imagem do banco, trunca-o
            // ao mínimo necessário (o journal já foi finalizado e a trava EXCLUSIVE segue).
            debug_assert!(self.e_lock == EXCLUSIVE_LOCK);
            rc = self.pager_truncate(self.db_size);
        }

        if rc == SQLITE_OK && b_commit {
            rc = os_file_control(
                self.fd.as_deref_mut(),
                SQLITE_FCNTL_COMMIT_PHASETWO,
                &mut FileControlArg::None,
            );
            if rc == SQLITE_NOTFOUND {
                rc = SQLITE_OK;
            }
        }

        let unlock = if self.exclusive_mode {
            false
        } else if !self.pager_use_wal() {
            true
        } else {
            let (wal, fd) = wal_and_fd!(self);
            wal.exclusive_mode(fd, 0) != 0
        };
        if unlock {
            rc2 = self.pager_unlock_db(SHARED_LOCK);
        }
        self.e_state = PAGER_READER;
        self.set_super = false;

        if rc == SQLITE_OK {
            rc2
        } else {
            rc
        }
    }

    /// `pagerUnlockAndRollback`: reverte a transação ativa e solta a trava do banco. No estado
    /// de erro não tenta reverter: `pager_unlock` descarta o cache e o próximo leitor reverte
    /// o journal quente, se houver.
    pub(crate) fn pager_unlock_and_rollback(&mut self) {
        if self.e_state != PAGER_ERROR && self.e_state != PAGER_OPEN {
            debug_assert!(self.assert_pager_state());
            if self.e_state >= PAGER_WRITER_LOCKED {
                // (sqlite3BeginBenignMalloc e sqlite3EndBenignMalloc só importam na injeção
                // de falhas de alocação, que não existe aqui.)
                self.rollback();
            } else if !self.exclusive_mode {
                debug_assert!(self.e_state == PAGER_READER);
                self.pager_end_transaction(false, false);
            }
        } else if self.e_state == PAGER_ERROR
            && self.journal_mode == PAGER_JOURNALMODE_MEMORY
            && self.jfd.is_some()
        {
            // Caso especial de ROLLBACK por erro de E/S com journal em memória: reverte já,
            // antes de o journal ser fechado, porque fechado ele esquece tudo.
            let err_code = self.err_code;
            let e_lock = self.e_lock;
            self.e_state = PAGER_OPEN;
            self.err_code = SQLITE_OK;
            self.e_lock = EXCLUSIVE_LOCK;
            self.pager_playback(true);
            self.err_code = err_code;
            self.e_lock = e_lock;
        }
        self.pager_unlock();
    }

    // ------------------------------------------------------------------
    // Playback do journal
    // ------------------------------------------------------------------

    /// `pager_cksum`: o "checksum" do registro: o inicializador `cksumInit` mais cada
    /// duzentésimo byte da página, a partir do offset `pageSize % 200`.
    pub(crate) fn pager_cksum(&self, a_data: &[u8]) -> u32 {
        let mut cksum = self.cksum_init;
        let mut i = self.page_size - 200;
        while i > 0 {
            cksum = cksum.wrapping_add(a_data[i as usize] as u32);
            i -= 200;
        }
        cksum
    }

    /// `pager_playback_one_page` com o `pOffset` do C: o journal principal usa
    /// `&pPager->journalOff`, que o Rust não pode emprestar junto com `&mut self`; então o
    /// valor vai por cópia e volta no fim.
    fn playback_one_journal_page(&mut self, p_done: Option<&mut Bitvec>, is_savepnt: bool) -> i32 {
        let mut off = self.journal_off;
        let rc = self.pager_playback_one_page(&mut off, p_done, true, is_savepnt);
        self.journal_off = off;
        rc
    }

    /// Arquivo do journal principal (`is_main`) ou do sub-journal.
    fn journal_fd(&mut self, is_main: bool) -> &mut dyn VfsFile {
        if is_main {
            file_of!(self, jfd)
        } else {
            file_of!(self, sjfd)
        }
    }

    /// `pager_playback_one_page`: lê uma página do journal (ou do sub-journal, se
    /// `is_main_jrnl` é falso) a partir de `*p_offset` e a reproduz. O offset avança para o
    /// registro seguinte. `SQLITE_DONE` indica registro corrompido; páginas além de `dbSize`
    /// ou já em `p_done` são puladas. O buffer da página é o `pTmpSpace`, retirado do pager
    /// durante a chamada.
    pub(crate) fn pager_playback_one_page(
        &mut self,
        p_offset: &mut i64,
        p_done: Option<&mut Bitvec>,
        is_main_jrnl: bool,
        is_savepnt: bool,
    ) -> i32 {
        let mut a_data = std::mem::take(&mut self.tmp_space);
        let rc = self.playback_one_page_buf(&mut a_data, p_offset, p_done, is_main_jrnl, is_savepnt);
        self.tmp_space = a_data;
        rc
    }

    /// Corpo de `pager_playback_one_page`, com o buffer `aData` já separado do pager.
    fn playback_one_page_buf(
        &mut self,
        a_data_full: &mut [u8],
        p_offset: &mut i64,
        p_done: Option<&mut Bitvec>,
        is_main_jrnl: bool,
        is_savepnt: bool,
    ) -> i32 {
        let mut p_done = p_done;
        let sz = self.page_size as usize;
        let a_data = &mut a_data_full[..sz];
        let mut pgno: u32 = 0;
        let mut cksum: u32 = 0;
        let mut rc: i32;

        debug_assert!(is_main_jrnl || p_done.is_some()); // pDone sempre usado em sub-journals
        debug_assert!(is_savepnt || p_done.is_none()); // pDone nunca usado fora de savepoint
        debug_assert!(!self.pager_use_wal() || (!is_main_jrnl && is_savepnt));

        // Ou o estado é maior que CACHEMOD (reversão pedida pelo chamador) ou é a reversão
        // de um journal quente (estado OPEN com trava EXCLUSIVE, só o journal principal).
        debug_assert!(
            self.e_state >= PAGER_WRITER_CACHEMOD
                || (self.e_state == PAGER_OPEN && self.e_lock == EXCLUSIVE_LOCK)
        );
        debug_assert!(self.e_state >= PAGER_WRITER_CACHEMOD || is_main_jrnl);

        // Lê o número da página e os dados; erro de E/S volta ao chamador.
        rc = read32bits(self.journal_fd(is_main_jrnl), *p_offset, &mut pgno);
        if rc != SQLITE_OK {
            return rc;
        }
        rc = self.journal_fd(is_main_jrnl).read(a_data, *p_offset + 4);
        if rc != SQLITE_OK {
            return rc;
        }
        *p_offset += self.page_size + 4 + if is_main_jrnl { 4 } else { 0 };

        // Consistência da página: uma queda de energia durante a escrita do journal pode ter
        // deixado dados inválidos, que precisam ser detectados e ignorados.
        if pgno == 0 || pgno == self.lck_pgno {
            debug_assert!(!is_savepnt);
            return SQLITE_DONE;
        }
        if pgno > self.db_size || bitvec_test(p_done.as_deref(), pgno) {
            return SQLITE_OK;
        }
        if is_main_jrnl {
            rc = read32bits(self.journal_fd(true), *p_offset - 4, &mut cksum);
            if rc != 0 {
                return rc;
            }
            if !is_savepnt && self.pager_cksum(a_data) != cksum {
                return SQLITE_DONE;
            }
        }

        // Página já revertida antes nesta reversão: não repete.
        if let Some(d) = p_done.as_deref_mut() {
            rc = bitvec_set(Some(d), pgno);
            if rc != SQLITE_OK {
                return rc;
            }
        }

        // Ao reverter a página 1, restaura a configuração de nReserve.
        if pgno == 1 && self.n_reserve != a_data[20] as i16 {
            self.n_reserve = a_data[20] as i16;
        }

        // Em CACHEMOD a página tem de estar no cache e só o cache é atualizado (a página
        // continua suja). Em DBMOD, FINISHED ou OPEN atualiza o cache, se a página existe, e o
        // arquivo principal, e a página fica limpa. Só se grava no banco com o arquivo aberto
        // e se o conteúdo original está sincronizado no journal principal.
        let mut p_pg: Option<PgId> = if self.pager_use_wal() {
            None
        } else {
            self.lookup(pgno)
        };
        debug_assert!(p_pg.is_some() || !self.mem_db);
        debug_assert!(self.e_state != PAGER_OPEN || p_pg.is_none() || self.temp_file);
        let is_synced = if is_main_jrnl {
            self.no_sync || (*p_offset <= self.journal_hdr)
        } else {
            match p_pg {
                None => true,
                Some(pg) => (self.pcache.page_flags(pg) & PGHDR_NEED_SYNC) == 0,
            }
        };
        if self.fd.is_some()
            && (self.e_state >= PAGER_WRITER_DBMOD || self.e_state == PAGER_OPEN)
            && is_synced
        {
            let ofst = (pgno as i64 - 1) * self.page_size;
            debug_assert!(!self.pager_use_wal());

            // Grava de volta no banco os dados lidos do journal.
            rc = file_of!(self, fd).write(a_data, ofst);

            if pgno > self.db_file_size {
                self.db_file_size = pgno;
            }
            if let Some(b) = self.backup.as_mut() {
                b.update(pgno, a_data);
            }
        } else if !is_main_jrnl && p_pg.is_none() {
            // Reversão de savepoint sem gravar no banco e sem a página em memória: quando a
            // página for buscada ela viria do arquivo, que pode estar desatualizado. A solução
            // é pôr no cache uma página com os dados do sub-journal, suja, e com
            // NEED_SYNC se o pager exige sync do journal.
            debug_assert!(is_savepnt);
            debug_assert!((self.do_not_spill & SPILLFLAG_ROLLBACK) == 0);
            self.do_not_spill |= SPILLFLAG_ROLLBACK;
            let got = self.get(pgno, 1);
            debug_assert!((self.do_not_spill & SPILLFLAG_ROLLBACK) != 0);
            self.do_not_spill &= !SPILLFLAG_ROLLBACK;
            match got {
                Err(e) => return e,
                Ok(pg) => {
                    self.pcache.make_dirty(pg);
                    p_pg = Some(pg);
                }
            }
        }
        if let Some(pg) = p_pg {
            // Nenhuma página em uso deve ser revertida explicitamente, salvo a página 1, que
            // fica referenciada para manter a trava; mas ela pode ser revertida por um erro
            // interno que chama sqlite3PagerRollback().
            let n_ref = self.pcache.page_ref_count(pg);
            {
                let (extra, data) = self.pcache.page_parts(pg);
                data[..sz].copy_from_slice(a_data);
                if let Some(reiniter) = self.reiniter {
                    reiniter(extra, data, n_ref);
                }
            }
            // O C chamava sqlite3PcacheMakeClean(pPg) aqui, mas foi retirado: o cache é limpo
            // por sqlite3PcacheCleanAll() depois da reversão.

            // Na página 1, restaura dbFileVers antes de qualquer decodificação.
            if pgno == 1 {
                let n = self.db_file_vers.len();
                self.db_file_vers.copy_from_slice(&a_data[24..24 + n]);
            }
            self.pcache.release(pg);
        }
        rc
    }

    /// `pager_delsuper`: um journal que apontava para o super-journal `z_super` acaba de ser
    /// revertido. Apaga o super-journal se nenhum dos seus journals filhos ainda existe com
    /// uma referência a ele. O arquivo lista os nomes dos filhos, cada um terminado em NUL.
    pub(crate) fn pager_delsuper(&mut self, z_super: &[u8]) -> i32 {
        let vfs = self.vfs.clone();
        let z_super = cstr(z_super);
        let mut rc = SQLITE_OK;
        let mut p_super: Option<Box<dyn VfsFile>> = None;
        let flags = SQLITE_OPEN_READONLY | SQLITE_OPEN_SUPER_JOURNAL;

        'delsuper_out: {
            // Abre o super-journal para leitura.
            let blob = filename_blob(z_super);
            let mut out_flags = 0;
            match os_open(&*vfs, Some(blob.as_slice()), flags, &mut out_flags) {
                Ok(f) => p_super = Some(f),
                Err(e) => {
                    rc = e;
                    break 'delsuper_out;
                }
            }

            // Carrega o super-journal inteiro; `n_super_ptr` é o espaço para os nomes de
            // super-journal extraídos dos journals comuns.
            let mut n_super_journal: i64 = 0;
            let Some(sup) = p_super.as_deref_mut() else {
                break 'delsuper_out;
            };
            rc = sup.file_size(&mut n_super_journal);
            if rc != SQLITE_OK {
                break 'delsuper_out;
            }
            let n_super_ptr = vfs.max_pathname() + 1;
            let n = n_super_journal as usize;
            let mut z_super_journal = vec![0u8; n + 2];
            rc = sup.read(&mut z_super_journal[..n], 0);
            if rc != SQLITE_OK {
                break 'delsuper_out;
            }

            let mut pos: usize = 0;
            while (pos as i64) < n_super_journal {
                let end = pos
                    + z_super_journal[pos..]
                        .iter()
                        .position(|&b| b == 0)
                        .unwrap_or(z_super_journal.len() - pos);
                let z_journal = &z_super_journal[pos..end];
                let mut exists: i32 = 0;
                rc = vfs.access(z_journal, SQLITE_ACCESS_EXISTS, &mut exists);
                if rc != SQLITE_OK {
                    break 'delsuper_out;
                }
                if exists != 0 {
                    // Um dos journals apontados existe: abre-o e vê se aponta para o
                    // super-journal; se sim, não apaga o super-journal.
                    let jblob = filename_blob(z_journal);
                    let mut jrnl = match os_open(&*vfs, Some(jblob.as_slice()), flags, &mut out_flags) {
                        Ok(f) => f,
                        Err(e) => {
                            rc = e;
                            break 'delsuper_out;
                        }
                    };
                    let mut z_super_ptr: Vec<u8> = Vec::new();
                    rc = read_super_journal(&mut *jrnl, &mut z_super_ptr, n_super_ptr as u32);
                    jrnl.close();
                    if rc != SQLITE_OK {
                        break 'delsuper_out;
                    }

                    if !z_super_ptr.is_empty() && z_super_ptr == z_super {
                        // Achou: não apaga o super-journal.
                        break 'delsuper_out;
                    }
                }
                pos = end + 1;
            }

            os_close(&mut p_super);
            rc = vfs.delete(z_super, 0);
        }
        os_close(&mut p_super);
        rc
    }

    /// `pager_truncate`: muda o tamanho real do arquivo do banco para `n_page` páginas. Só
    /// acontece ao confirmar ou reverter (inclusive o journal quente). Sem arquivo aberto, ou
    /// fora de DBMOD e OPEN, é um no-op. Se o arquivo em disco é menor que o novo tamanho,
    /// alguns sistemas se confundem com truncar para cima: grava um byte zero no fim.
    pub(crate) fn pager_truncate(&mut self, n_page: u32) -> i32 {
        let mut rc = SQLITE_OK;
        debug_assert!(self.e_state != PAGER_ERROR);
        debug_assert!(self.e_state != PAGER_READER);

        if self.fd.is_some() && (self.e_state >= PAGER_WRITER_DBMOD || self.e_state == PAGER_OPEN) {
            let mut current_size: i64 = 0;
            let sz_page = self.page_size;
            debug_assert!(self.e_lock == EXCLUSIVE_LOCK);
            rc = file_of!(self, fd).file_size(&mut current_size);
            let new_size = sz_page * n_page as i64;
            if rc == SQLITE_OK && current_size != new_size {
                if current_size > new_size {
                    rc = file_of!(self, fd).truncate(new_size);
                } else if (current_size + sz_page) <= new_size {
                    self.tmp_space[..sz_page as usize].fill(0);
                    let mut arg = FileControlArg::Int64(new_size);
                    os_file_control_hint(self.fd.as_deref_mut(), SQLITE_FCNTL_SIZE_HINT, &mut arg);
                    rc = file_of!(self, fd)
                        .write(&self.tmp_space[..sz_page as usize], new_size - sz_page);
                }
                if rc == SQLITE_OK {
                    self.db_file_size = n_page;
                }
            }
        }
        rc
    }

    /// `setSectorSize`: define `sectorSize` pelo `xSectorSize` do arquivo aberto. Para arquivo
    /// temporário, ou com `SQLITE_IOCAP_POWERSAFE_OVERWRITE`, vale o mínimo, 512.
    pub(crate) fn set_sector_size(&mut self) {
        debug_assert!(self.fd.is_some() || self.temp_file);

        if self.temp_file
            || (self.fd_device_characteristics() & SQLITE_IOCAP_POWERSAFE_OVERWRITE) != 0
        {
            // Para arquivo temporário o tamanho do setor não importa, e o arquivo pode nem
            // ter sido aberto ainda.
            self.sector_size = 512;
        } else {
            self.sector_size = sector_size(file_of!(self, fd)) as u32;
        }
    }

    /// `pager_playback`: reproduz o journal e restaura o banco ao estado anterior à transação.
    /// O formato do journal está descrito em `write_journal_hdr`. `is_hot` indica um journal
    /// que pode ser quente (ou preservado por PERSIST/TRUNCATE); se for quente, o cache é
    /// zerado antes de reverter qualquer conteúdo. Erro de E/S ou de memória deixa o
    /// journal no lugar e devolve o código.
    pub(crate) fn pager_playback(&mut self, is_hot: bool) -> i32 {
        let vfs = self.vfs.clone();
        let mut sz_j: i64 = 0;
        let mut n_rec: u32 = 0;
        let mut mx_pg: u32 = 0;
        let mut rc: i32;
        let mut res: i32 = 1;
        let mut z_super: Vec<u8> = Vec::new();
        let mut n_playback: i32 = 0;
        let saved_page_size = self.page_size as u32;
        let n_super = (vfs.max_pathname() + 1) as u32;

        // Quantos registros o journal tem; aborta cedo se estiver vazio.
        debug_assert!(self.jfd.is_some());
        'end_playback: {
            rc = file_of!(self, jfd).file_size(&mut sz_j);
            if rc != SQLITE_OK {
                break 'end_playback;
            }

            // Lê o nome do super-journal, se houver. Se ele é citado mas não existe em
            // disco, o journal não é quente e não precisa de playback.
            rc = read_super_journal(file_of!(self, jfd), &mut z_super, n_super);
            if rc == SQLITE_OK && !z_super.is_empty() {
                rc = vfs.access(&z_super, SQLITE_ACCESS_EXISTS, &mut res);
            }
            z_super.clear();
            if rc != SQLITE_OK || res == 0 {
                break 'end_playback;
            }
            self.journal_off = 0;
            let mut need_pager_reset = is_hot;

            // O laço termina quando `read_journal_hdr` ou `pager_playback_one_page` devolve
            // SQLITE_DONE ou quando ocorre erro de E/S.
            loop {
                // Lê o próximo cabeçalho. Sem bytes para um cabeçalho completo, ou
                // corrompido, algum processo falhou ao escrevê-lo e não há mais o que reverter.
                rc = self.read_journal_hdr(is_hot, sz_j, &mut n_rec, &mut mx_pg);
                if rc != SQLITE_OK {
                    if rc == SQLITE_DONE {
                        rc = SQLITE_OK;
                    }
                    break 'end_playback;
                }

                // nRec == 0xffffffff: journal criado em modo no-sync, o resto do arquivo são
                // páginas, sem mais cabeçalhos. Calcula nRec com essa premissa.
                if n_rec == 0xffffffff {
                    debug_assert!(self.journal_off == self.journal_hdr_sz());
                    n_rec = ((sz_j - self.journal_hdr_sz()) / self.journal_pg_sz()) as i32 as u32;
                }

                // nRec == 0 numa reversão de transação deste processo, no último cabeçalho do
                // journal: a parte final estava sendo preenchida e não foi sincronizada, então
                // o número de páginas sai do tamanho do arquivo (ticket #2565).
                if n_rec == 0
                    && !is_hot
                    && self.journal_hdr + self.journal_hdr_sz() == self.journal_off
                {
                    n_rec = ((sz_j - self.journal_off) / self.journal_pg_sz()) as i32 as u32;
                }

                // No primeiro cabeçalho, trunca o banco de volta ao tamanho original.
                if self.journal_off == self.journal_hdr_sz() {
                    rc = self.pager_truncate(mx_pg);
                    if rc != SQLITE_OK {
                        break 'end_playback;
                    }
                    self.db_size = mx_pg;
                    if self.mx_pgno < mx_pg {
                        self.mx_pgno = mx_pg;
                    }
                }

                // Copia as páginas originais do journal de volta ao arquivo e ao cache.
                let mut u: u32 = 0;
                while u < n_rec {
                    if need_pager_reset {
                        self.pager_reset();
                        need_pager_reset = false;
                    }
                    rc = self.playback_one_journal_page(None, false);
                    if rc == SQLITE_OK {
                        n_playback += 1;
                    } else if rc == SQLITE_DONE {
                        self.journal_off = sz_j;
                        break;
                    } else if rc == SQLITE_IOERR_SHORT_READ {
                        // Journal truncado: provavelmente não foi escrito e sincronizado por
                        // inteiro antes de uma queda. O banco nem deveria ter sido escrito,
                        // então abandonar a reversão é correto.
                        rc = SQLITE_OK;
                        break 'end_playback;
                    } else {
                        // Sem conseguir reverter, sai com o erro: o pager entra em estado de
                        // erro e talvez o próximo processo consiga reverter.
                        break 'end_playback;
                    }
                    u += 1;
                }
            }
        }

        // end_playback:
        if rc == SQLITE_OK {
            let mut ps = saved_page_size;
            rc = self.set_pagesize(&mut ps, -1);
        }
        // (O SQLITE_FCNTL_DB_UNCHANGED que o C manda aqui é só de SQLITE_DEBUG.)

        // Se esta reversão é automática, por erro de E/S ou de memória depois de atualizar o
        // change-counter mas antes de confirmar, a atualização pode ter sido revertida. Em
        // modo exclusivo as transações seguintes não atualizariam mais o change-counter:
        // limpa changeCountDone por precaução.
        self.change_count_done = self.temp_file;

        if rc == SQLITE_OK {
            rc = read_super_journal(file_of!(self, jfd), &mut z_super, n_super);
        }
        if rc == SQLITE_OK && (self.e_state >= PAGER_WRITER_DBMOD || self.e_state == PAGER_OPEN) {
            rc = self.sync(None);
        }
        if rc == SQLITE_OK {
            rc = self.pager_end_transaction(!z_super.is_empty(), false);
        }
        if rc == SQLITE_OK && !z_super.is_empty() && res != 0 {
            // Com um super-journal, e se a rotina vai devolver sucesso, vê se dá para apagá-lo.
            rc = self.pager_delsuper(&z_super);
        }
        if is_hot && n_playback != 0 {
            crate::global::log(
                SQLITE_NOTICE_RECOVER_ROLLBACK,
                b"recovered %d pages from %s",
                &[
                    PrintfArg::Int(n_playback as i64),
                    PrintfArg::Text(Some(cstr(&self.z_journal).to_vec())),
                ],
            );
        }

        // sectorSize pode ter mudado ao reverter um journal criado por um processo com outro
        // tamanho de setor: volta ao valor deste processo.
        self.set_sector_size();
        rc
    }

    // ------------------------------------------------------------------
    // Leitura de páginas e WAL
    // ------------------------------------------------------------------

    /// `readDbPage`: lê o conteúdo da página `pg` do arquivo do banco (ou do WAL, se a cópia
    /// mais recente estiver lá) para os bytes da página. Exige trava SHARED ou maior. Se for a
    /// página 1, atualiza `dbFileVers`.
    pub(crate) fn read_db_page(&mut self, pg: PgId) -> i32 {
        let mut rc: i32;
        let mut i_frame: u32 = 0;
        let pgno = self.pcache.page_pgno(pg);
        let sz = self.page_size as usize;

        debug_assert!(self.e_state >= PAGER_READER && !self.mem_db);
        debug_assert!(self.fd.is_some());

        if self.pager_use_wal() {
            let (wal, fd) = wal_and_fd!(self);
            rc = wal.find_frame(fd, pgno, &mut i_frame);
            if rc != SQLITE_OK {
                return rc;
            }
        }
        if i_frame != 0 {
            let wal = self.wal.as_mut().expect("pager: sem WAL (invariante pagerUseWal do C)");
            rc = wal.read_frame(i_frame, &mut self.pcache.page_data_mut(pg)[..sz]);
        } else {
            let i_offset = (pgno as i64 - 1) * self.page_size;
            rc = file_of!(self, fd).read(&mut self.pcache.page_data_mut(pg)[..sz], i_offset);
            if rc == SQLITE_IOERR_SHORT_READ {
                rc = SQLITE_OK;
            }
        }

        if pgno == 1 {
            if rc != 0 {
                // Leitura sem sucesso: dbFileVers vira algo que nunca é uma versão válida
                // (bytes 24..39 do banco; os bytes 28..31 são zero ou o tamanho em páginas e
                // os seguintes são números de página, nunca 0xffffffff).
                self.db_file_vers = [0xff; 16];
            } else {
                let data = self.pcache.page_data(pg);
                self.db_file_vers.copy_from_slice(&data[24..40]);
            }
        }
        rc
    }

    /// `pager_write_changecounter`: atualiza incondicionalmente o change-counter dos bytes 24
    /// e 92 do cabeçalho e o número da versão do SQLite no byte 96.
    pub(crate) fn pager_write_changecounter(&mut self, pg: PgId) {
        // Incrementa o valor lido de dbFileVers e o grava no byte 24.
        let change_counter = get4byte(&self.db_file_vers).wrapping_add(1);
        let data = self.page_data_mut(pg);
        put4byte(&mut data[24..], change_counter);

        // Guarda também a versão do SQLite nos bytes 96..99 e, nos bytes 92..95, o
        // change-counter para o qual o número da versão vale.
        put4byte(&mut data[92..], change_counter);
        put4byte(&mut data[96..], SQLITE_VERSION_NUMBER as u32);
    }

    /// `pagerUndoCallback`: chamada para cada página já gravada no log quando uma transação
    /// WAL é revertida. Página no cache sem outras referências é descartada; com
    /// referências, o conteúdo é relido do banco (e o erro dessa releitura é devolvido).
    pub(crate) fn pager_undo_callback(&mut self, i_pg: u32) -> i32 {
        let mut rc = SQLITE_OK;

        debug_assert!(self.pager_use_wal());
        if let Some(pg) = self.lookup(i_pg) {
            if self.pcache.page_ref_count(pg) == 1 {
                self.pcache.drop_page(pg);
            } else {
                rc = self.read_db_page(pg);
                if rc == SQLITE_OK {
                    let n_ref = self.pcache.page_ref_count(pg);
                    if let Some(reiniter) = self.reiniter {
                        let (extra, data) = self.pcache.page_parts(pg);
                        reiniter(extra, data, n_ref);
                    }
                }
                self.unref_not_null(pg);
            }
        }

        // Numa reversão normal os backups são atualizados enquanto os dados saem do journal
        // para o banco. Com WAL isso não é possível: se quadros já foram escritos no log (e
        // portanto copiados para os backups) nesta transação, os backups precisam recomeçar.
        self.backup_restart();

        rc
    }

    /// `pagerRollbackWal`: reverte uma transação num banco WAL. Para cada página do cache que
    /// está suja ou já foi gravada (sem confirmar) no log, descarta a página (sem referências)
    /// ou relê o conteúdo do banco (com referências).
    pub(crate) fn pager_rollback_wal(&mut self) -> i32 {
        self.db_size = self.db_orig_size;

        // `sqlite3WalUndo(pWal, pagerUndoCallback, pPager)` em duas fases, porque o callback
        // usa o próprio `Wal` (readDbPage) e não pode rodar com ele emprestado.
        let begun = {
            let (wal, fd) = wal_and_fd!(self);
            wal.undo_begin(fd)
        };
        let mut rc = SQLITE_OK;
        match begun {
            Ok((pgnos, i_max)) => {
                for pgno in pgnos {
                    rc = self.pager_undo_callback(pgno);
                    if rc != SQLITE_OK {
                        break;
                    }
                }
                let (wal, fd) = wal_and_fd!(self);
                let rc_end = wal.undo_end(fd, i_max);
                if rc == SQLITE_OK {
                    rc = rc_end;
                }
            }
            Err(e) => rc = e,
        }

        // Os números de página saem antes do laço: o callback pode descartar páginas da lista.
        let dirty = self.pcache.dirty_list();
        let pgnos: Vec<u32> = dirty.iter().map(|&p| self.pcache.page_pgno(p)).collect();
        for pgno in pgnos {
            if rc != SQLITE_OK {
                break;
            }
            rc = self.pager_undo_callback(pgno);
        }

        rc
    }

    /// `pagerWalFrames`: envoltório de `sqlite3WalFrames()`. Além de gravar as páginas da
    /// lista (em ordem crescente de `pgno`, com a página 1, se houver, primeiro), avisa os
    /// backups ativos de que elas mudaram.
    pub(crate) fn pager_wal_frames(&mut self, list: Vec<PgId>, n_truncate: u32, is_commit: bool) -> i32 {
        let mut list = list;
        let n_list: i32;

        debug_assert!(self.wal.is_some());
        debug_assert!(!list.is_empty());
        debug_assert!(list
            .windows(2)
            .all(|w| self.pcache.page_pgno(w[0]) < self.pcache.page_pgno(w[1])));
        debug_assert!(list.len() == 1 || is_commit);
        if is_commit {
            // Num COMMIT de WAL, páginas com número maior que nTruncate nunca serão lidas: não
            // vão para o arquivo do WAL.
            let pcache = &self.pcache;
            list.retain(|&p| pcache.page_pgno(p) <= n_truncate);
            n_list = list.len() as i32;
            debug_assert!(!list.is_empty());
        } else {
            n_list = 1;
        }
        self.a_stat[PAGER_STAT_WRITE] += n_list as u32;

        if let Some(&first) = list.first() {
            if self.pcache.page_pgno(first) == 1 {
                self.pager_write_changecounter(first);
            }
        }

        // O flag PGHDR_WAL_APPEND só é lido e escrito dentro de uma chamada de `walFrames` (todas
        // as páginas da lista são decididas na mesma chamada), então vive dentro do `Wal`.
        let rc;
        {
            let pcache = &self.pcache;
            let frames: Vec<WalPageRef> = list
                .iter()
                .map(|&p| WalPageRef { pgno: pcache.page_pgno(p), data: pcache.page_data(p), flags: 0 })
                .collect();
            let (wal, fd) = wal_and_fd!(self);
            rc = wal.frames(fd, self.page_size as i32, &frames, n_truncate, is_commit, self.wal_sync_flags);
        }
        if rc == SQLITE_OK {
            if let Some(b) = self.backup.as_mut() {
                for &p in &list {
                    b.update(self.pcache.page_pgno(p), self.pcache.page_data(p));
                }
            }
        }

        rc
    }

    /// `pagerBeginReadTransaction` (antes `pagerOpenSnapshot`): inicia uma transação de leitura
    /// no WAL, fixando um retrato do banco no instante atual.
    pub(crate) fn pager_begin_read_transaction(&mut self) -> i32 {
        let mut changed: i32 = 0; // verdadeiro se o cache precisa ser zerado

        debug_assert!(self.pager_use_wal());
        debug_assert!(self.e_state == PAGER_OPEN || self.e_state == PAGER_READER);

        // sqlite3WalEndReadTransaction() não foi chamada na transação anterior em
        // locking_mode=EXCLUSIVE; chama agora. Em locking_mode=NORMAL a chamada repetida é
        // inofensiva.
        let rc = {
            let (wal, fd) = wal_and_fd!(self);
            wal.end_read_transaction(fd);
            wal.begin_read_transaction(fd, &mut changed)
        };
        if rc != SQLITE_OK || changed != 0 {
            self.pager_reset();
            if self.b_use_fetch {
                if let Some(fd) = self.fd.as_deref_mut() {
                    fd.unfetch(0, None);
                }
            }
        }

        rc
    }

    /// `pagerPagecount`: parte da passagem de `PAGER_OPEN` a `PAGER_READER`: o tamanho do
    /// banco em páginas com o `pageSize` atual. O WAL informa o tamanho se tem transação
    /// confirmada; senão, vem do tamanho do arquivo (arredondado para cima).
    pub(crate) fn pager_pagecount(&mut self, pn_page: &mut u32) -> i32 {
        debug_assert!(self.e_state == PAGER_OPEN);
        debug_assert!(self.e_lock >= SHARED_LOCK);
        debug_assert!(self.fd.is_some());
        debug_assert!(!self.temp_file);

        // `sqlite3WalDbsize` devolve zero se o WAL não está aberto ou não tem o tamanho.
        let mut n_page: u32 = match self.wal.as_ref() {
            Some(wal) => wal.dbsize(),
            None => 0,
        };

        // Se o WAL não sabe, usa o tamanho do arquivo; se não é múltiplo da página, arredonda
        // para cima.
        if n_page == 0 && self.fd.is_some() {
            let mut n: i64 = 0;
            let rc = file_of!(self, fd).file_size(&mut n);
            if rc != SQLITE_OK {
                return rc;
            }
            n_page = ((n + self.page_size - 1) / self.page_size) as u32;
        }

        // Se o arquivo tem mais páginas que o máximo configurado, sobe o limite para ele poder
        // ser lido.
        if n_page > self.mx_pgno {
            self.mx_pgno = n_page;
        }

        *pn_page = n_page;
        SQLITE_OK
    }

    /// `pagerOpenWalIfPresent`: se o banco não está vazio e existe o arquivo `-wal`, abre o
    /// pager em modo WAL. Se o banco está vazio, apaga um `-wal` que exista. Sem `-wal`, garante
    /// que `journalMode` não seja WAL. Exige trava SHARED.
    pub(crate) fn pager_open_wal_if_present(&mut self) -> i32 {
        let mut rc = SQLITE_OK;
        debug_assert!(self.e_state == PAGER_OPEN);
        debug_assert!(self.e_lock >= SHARED_LOCK);

        if !self.temp_file {
            let mut is_wal: i32 = 0; // verdadeiro se o arquivo WAL existe
            rc = self
                .vfs
                .access(cstr(&self.z_wal), SQLITE_ACCESS_EXISTS, &mut is_wal);
            if rc == SQLITE_OK {
                if is_wal != 0 {
                    let mut n_page: u32 = 0;

                    rc = self.pager_pagecount(&mut n_page);
                    if rc != 0 {
                        return rc;
                    }
                    if n_page == 0 {
                        rc = self.vfs.delete(cstr(&self.z_wal), 0);
                    } else {
                        rc = self.open_wal(None);
                    }
                } else if self.journal_mode == PAGER_JOURNALMODE_WAL {
                    self.journal_mode = PAGER_JOURNALMODE_DELETE;
                }
            }
        }
        rc
    }

    // ------------------------------------------------------------------
    // Playback de savepoint
    // ------------------------------------------------------------------

    /// `pagerPlaybackSavepoint`: reverte o savepoint `sp` (índice em `a_savepoint`) ou, com
    /// `None`, todo o journal (ROLLBACK TO num savepoint de transação). A reversão de um
    /// savepoint comum tem até três fases: o journal principal de `iOffset` até `iHdrOffset`
    /// (ou o fim); depois, se `iHdrOffset != 0`, do cabeçalho seguinte ao fim; e por fim o
    /// sub-journal a partir de `iSubRec`. Cada página revertida liga seu bit em `pDone`, para
    /// ser revertida só na primeira vez. Antes de começar, `dbSize` volta ao valor do início do
    /// savepoint (ou da transação).
    pub(crate) fn pager_playback_savepoint(&mut self, sp: Option<usize>) -> i32 {
        let mut rc = SQLITE_OK;
        let mut p_done: Option<Box<Bitvec>> = None;

        debug_assert!(self.e_state != PAGER_ERROR);
        debug_assert!(self.e_state >= PAGER_WRITER_LOCKED);

        // Bitvec das páginas já revertidas.
        if let Some(i) = sp {
            p_done = Some(bitvec_create(self.a_savepoint[i].n_orig));
        }

        // O tamanho do banco volta ao de antes do savepoint.
        self.db_size = match sp {
            Some(i) => self.a_savepoint[i].n_orig,
            None => self.db_orig_size,
        };
        self.change_count_done = self.temp_file;

        if sp.is_none() && self.pager_use_wal() {
            return self.pager_rollback_wal();
        }

        // journalOff é o tamanho efetivo do journal principal: o arquivo pode ser maior em
        // TRUNCATE e PERSIST, mas o que passa de journalOff é proibido.
        let sz_j = self.journal_off;
        debug_assert!(!self.pager_use_wal() || sz_j == 0);

        // Primeiro os registros do journal principal de `iOffset` até o próximo cabeçalho. Pode
        // haver registros com página acima de dbSize, que são pulados sozinhos.
        match sp {
            Some(i) if !self.pager_use_wal() => {
                let i_hdr_off = if self.a_savepoint[i].i_hdr_offset != 0 {
                    self.a_savepoint[i].i_hdr_offset
                } else {
                    sz_j
                };
                self.journal_off = self.a_savepoint[i].i_offset;
                while rc == SQLITE_OK && self.journal_off < i_hdr_off {
                    rc = self.playback_one_journal_page(p_done.as_deref_mut(), true);
                }
                debug_assert!(rc != SQLITE_DONE);
            }
            _ => {
                self.journal_off = 0;
            }
        }

        // Segue pelos registros do journal principal a partir do primeiro cabeçalho visto, até
        // o fim efetivo do arquivo, pulando páginas fora do limite e marcando em pDone.
        while rc == SQLITE_OK && self.journal_off < sz_j {
            let mut n_j_rec: u32 = 0; // número de registros do journal
            let mut dummy: u32 = 0;
            rc = self.read_journal_hdr(false, sz_j, &mut n_j_rec, &mut dummy);
            debug_assert!(rc != SQLITE_DONE);

            // O teste de journalHdr + JOURNAL_HDR_SZ == journalOff é do ticket #2565; ver
            // pager_playback.
            if n_j_rec == 0 && self.journal_hdr + self.journal_hdr_sz() == self.journal_off {
                n_j_rec = ((sz_j - self.journal_off) / self.journal_pg_sz()) as u32;
            }
            let mut ii: u32 = 0;
            while rc == SQLITE_OK && ii < n_j_rec && self.journal_off < sz_j {
                rc = self.playback_one_journal_page(p_done.as_deref_mut(), true);
                ii += 1;
            }
            debug_assert!(rc != SQLITE_DONE);
        }
        debug_assert!(rc != SQLITE_OK || self.journal_off >= sz_j);

        // Por fim as páginas do sub-journal. As que saíram do journal principal (já em pDone) e
        // as fora do limite são puladas.
        if let Some(i) = sp {
            let mut offset = self.a_savepoint[i].i_sub_rec as i64 * (4 + self.page_size);

            if self.pager_use_wal() {
                let mut wal_data = self.a_savepoint[i].a_wal_data;
                let (wal, fd) = wal_and_fd!(self);
                rc = wal.savepoint_undo(fd, &mut wal_data);
            }
            let mut ii = self.a_savepoint[i].i_sub_rec;
            while rc == SQLITE_OK && ii < self.n_sub_rec {
                debug_assert!(offset == ii as i64 * (4 + self.page_size));
                rc = self.pager_playback_one_page(&mut offset, p_done.as_deref_mut(), false, true);
                ii += 1;
            }
            debug_assert!(rc != SQLITE_DONE);
        }

        // (`sqlite3BitvecDestroy(pDone)` é o drop de `p_done`.)
        drop(p_done);
        if rc == SQLITE_OK {
            self.journal_off = sz_j;
        }

        rc
    }

    // ------------------------------------------------------------------
    // Configuração
    // ------------------------------------------------------------------

    /// `pagerFixMaplimit`: invoca `SQLITE_FCNTL_MMAP_SIZE` com o valor atual de `szMmap`.
    pub(crate) fn pager_fix_maplimit(&mut self) {
        // SQLITE_MAX_MMAP_SIZE > 0 no Debian.
        let versioned = self.fd.as_ref().is_some_and(|f| f.i_version() >= 3);
        if versioned {
            let sz = self.sz_mmap;
            self.b_use_fetch = sz > 0;
            self.set_getter_method();
            let mut arg = FileControlArg::Int64(sz);
            os_file_control_hint(self.fd.as_deref_mut(), SQLITE_FCNTL_MMAP_SIZE, &mut arg);
        }
    }

    /// `sqlite3PagerSetMmapLimit`: muda o tamanho máximo de qualquer mapeamento do banco.
    pub(crate) fn set_mmap_limit(&mut self, sz_mmap: i64) {
        self.sz_mmap = sz_mmap;
        self.pager_fix_maplimit();
    }

    /// `sqlite3PagerSetFlags`: ajusta o pager às opções de `pg_flags`. O nível em
    /// `pg_flags & PAGER_SYNCHRONOUS_MASK` (OFF=1, NORMAL=2, FULL=3, EXTRA=4) define quantos
    /// `sync()` o pager faz nos journals; no WAL, OFF nunca sincroniza, NORMAL sincroniza o
    /// WAL antes do checkpoint e o banco no fim dele, e FULL também sincroniza o WAL a cada
    /// commit. `SQLITE_SYNC_FULL` (fsync completo do MacOS) não é o mesmo que
    /// synchronous=FULL.
    pub(crate) fn set_flags(&mut self, pg_flags: u32) {
        let level = pg_flags & PAGER_SYNCHRONOUS_MASK;
        if self.temp_file {
            self.no_sync = true;
            self.full_sync = false;
            self.extra_sync = false;
        } else {
            self.no_sync = level == PAGER_SYNCHRONOUS_OFF;
            self.full_sync = level >= PAGER_SYNCHRONOUS_FULL;
            self.extra_sync = level == PAGER_SYNCHRONOUS_EXTRA;
        }
        if self.no_sync {
            self.sync_flags = 0;
        } else if pg_flags & PAGER_FULLFSYNC != 0 {
            self.sync_flags = SQLITE_SYNC_FULL;
        } else {
            self.sync_flags = SQLITE_SYNC_NORMAL;
        }
        self.wal_sync_flags = self.sync_flags << 2;
        if self.full_sync {
            self.wal_sync_flags |= self.sync_flags;
        }
        if (pg_flags & PAGER_CKPT_FULLFSYNC) != 0 && !self.no_sync {
            self.wal_sync_flags |= SQLITE_SYNC_FULL << 2;
        }
        if pg_flags & PAGER_CACHESPILL != 0 {
            self.do_not_spill &= !SPILLFLAG_OFF;
        } else {
            self.do_not_spill |= SPILLFLAG_OFF;
        }
    }

    /// `pagerOpentemp`: abre um arquivo temporário e o devolve (o C o escreve em `*pFile`; o
    /// chamador o guarda em `fd` ou em `sjfd`). O VFS apaga o arquivo ao fechá-lo. Às flags de
    /// `vfs_flags` somam-se READWRITE, CREATE, EXCLUSIVE e DELETEONCLOSE.
    pub(crate) fn pager_opentemp(&self, vfs_flags: i32) -> Result<Box<dyn VfsFile>, i32> {
        let vfs_flags = vfs_flags
            | SQLITE_OPEN_READWRITE
            | SQLITE_OPEN_CREATE
            | SQLITE_OPEN_EXCLUSIVE
            | SQLITE_OPEN_DELETEONCLOSE;
        let mut out_flags = 0;
        os_open(&*self.vfs, None, vfs_flags, &mut out_flags)
    }

    /// `sqlite3PagerSetBusyHandler`: o pager invoca o busy-handler quando `xLock` devolve
    /// SQLITE_BUSY ao subir de NO_LOCK para SHARED ou de RESERVED para EXCLUSIVE; não ao ir de
    /// SHARED para RESERVED nem de SHARED para EXCLUSIVE (reversão de journal quente). Se o
    /// handler devolve não-zero, a trava é tentada de novo.
    pub(crate) fn set_busy_handler(&mut self, handler: Option<Box<dyn FnMut() -> i32>>) {
        self.busy_handler = handler;
        os_file_control_hint(
            self.fd.as_deref_mut(),
            SQLITE_FCNTL_BUSYHANDLER,
            &mut FileControlArg::BusyHandler,
        );
    }

    /// `sqlite3PagerSetPagesize`: muda o tamanho de página do pager para `*p_page_size`
    /// (potência de dois entre 512 e `SQLITE_MAX_PAGE_SIZE`; zero só consulta). Só muda se não
    /// há referências a páginas e, num banco em memória, se ele tem zero páginas. Em erro de
    /// estado o tamanho antigo é mantido. `*p_page_size` recebe o tamanho vigente. Com
    /// `n_reserve < 0` mantém o `nReserve` atual.
    pub(crate) fn set_pagesize(&mut self, p_page_size: &mut u32, n_reserve: i32) -> i32 {
        let mut rc = SQLITE_OK;

        // Não dá para um assert_pager_state() completo aqui: a função pode ser chamada de
        // dentro de PagerOpen(), antes de o objeto estar consistente.
        let page_size = *p_page_size;
        debug_assert!(
            page_size == 0 || (page_size >= 512 && (page_size as i32) <= SQLITE_MAX_PAGE_SIZE)
        );
        if (!self.mem_db || self.db_size == 0)
            && self.pcache.ref_count() == 0
            && page_size != 0
            && page_size as i64 != self.page_size
        {
            let mut n_byte: i64 = 0;
            let mut p_new: Vec<u8> = Vec::new();

            if self.e_state > PAGER_OPEN && self.fd.is_some() {
                rc = file_of!(self, fd).file_size(&mut n_byte);
            }
            if rc == SQLITE_OK {
                // 8 bytes zerados de sobra garantem que o parser do cabeçalho de célula do
                // btree nunca ultrapasse o fim da alocação.
                p_new = vec![0u8; page_size as usize + 8];
            }

            if rc == SQLITE_OK {
                self.pager_reset();
                rc = self.pcache.set_page_size(page_size as i32);
            }
            if rc == SQLITE_OK {
                self.tmp_space = p_new;
                self.db_size = ((n_byte + page_size as i64 - 1) / page_size as i64) as u32;
                self.page_size = page_size as i64;
                self.lck_pgno = (PENDING_BYTE / page_size as i64) as u32 + 1;
            }
        }

        *p_page_size = self.page_size as u32;
        if rc == SQLITE_OK {
            let mut n_reserve = n_reserve;
            if n_reserve < 0 {
                n_reserve = self.n_reserve as i32;
            }
            debug_assert!((0..1000).contains(&n_reserve));
            self.n_reserve = n_reserve as i16;
            self.pager_fix_maplimit();
        }
        rc
    }

    /// `sqlite3PagerTempSpace`: o buffer temporário do pager, grande o bastante para uma
    /// página (mais 8 bytes zerados). É usado internamente nas reversões e pode ser usado por
    /// outros módulos enquanto nenhuma reversão ocorre.
    pub(crate) fn temp_space(&mut self) -> &mut [u8] {
        &mut self.tmp_space
    }

    /// `sqlite3PagerMaxPageCount`: com `mx_page > 0`, muda o máximo de páginas do banco.
    /// Devolve o máximo vigente.
    pub(crate) fn max_page_count(&mut self, mx_page: u32) -> u32 {
        if mx_page > 0 {
            self.mx_pgno = mx_page;
        }
        debug_assert!(self.e_state != PAGER_OPEN); // chamada só por OP_MaxPgcnt
        // (mxPgno >= dbSize não é assert: OP_MaxPgcnt pode passar menos que dbSize.)
        self.mx_pgno
    }

    /// `sqlite3PagerReadFileheader`: lê os primeiros `dest.len()` bytes do arquivo. Pager de
    /// arquivo transitório, ou arquivo menor que isso, devolve zeros e SQLITE_OK (o cabeçalho de
    /// um banco novo é todo zeros). Erro de E/S fora o `SHORT_READ` volta ao chamador.
    pub(crate) fn read_fileheader(&mut self, dest: &mut [u8]) -> i32 {
        let mut rc = SQLITE_OK;
        dest.fill(0);
        debug_assert!(self.fd.is_some() || self.temp_file);

        // Só o btree chama, logo depois de criar o Pager: ainda não houve chance de ir a WAL.
        debug_assert!(!self.pager_use_wal());

        if let Some(fd) = self.fd.as_deref_mut() {
            rc = fd.read(dest, 0);
            if rc == SQLITE_IOERR_SHORT_READ {
                rc = SQLITE_OK;
            }
        }
        rc
    }

    /// `sqlite3PagerPagecount`: número de páginas do banco (com leitura aberta). Um arquivo
    /// entre 1 e `<page-size>` bytes vale 1 página.
    pub(crate) fn pagecount(&self) -> i32 {
        debug_assert!(self.e_state >= PAGER_READER);
        debug_assert!(self.e_state != PAGER_WRITER_FINISHED);
        self.db_size as i32
    }

    /// `pager_wait_on_lock`: tenta a trava `locktype`; se `SQLITE_BUSY`, chama o busy-handler e
    /// repete até ele devolver zero ou a trava ser obtida. Sem trava a obter (já tem igual ou
    /// maior) é um no-op.
    pub(crate) fn pager_wait_on_lock(&mut self, locktype: i32) -> i32 {
        // Ou é um no-op (trava já presente), ou uma das transições em que o busy-handler pode
        // ser chamado, segundo o comentário de `set_busy_handler`.
        debug_assert!(
            self.e_lock >= locktype
                || (self.e_lock == NO_LOCK && locktype == SHARED_LOCK)
                || (self.e_lock == RESERVED_LOCK && locktype == EXCLUSIVE_LOCK)
        );

        loop {
            let rc = self.pager_lock_db(locktype);
            if rc != SQLITE_BUSY {
                return rc;
            }
            // O handler sai do pager enquanto roda e volta depois, para poder ser chamado sem
            // emprestar o pager inteiro.
            let retry = match self.busy_handler.take() {
                Some(mut h) => {
                    let r = h();
                    if self.busy_handler.is_none() {
                        self.busy_handler = Some(h);
                    }
                    r != 0
                }
                None => false,
            };
            if !retry {
                return rc;
            }
        }
    }

    /// `sqlite3PagerTruncateImage`: trunca a imagem do banco em memória para `n_page` páginas,
    /// sem tocar no arquivo em disco: o truncamento acontece ao confirmar. Só é chamada logo
    /// antes de confirmar; depois dela a transação precisa ser confirmada ou revertida.
    pub(crate) fn truncate_image(&mut self, n_page: u32) {
        debug_assert!(self.db_size >= n_page);
        debug_assert!(self.e_state >= PAGER_WRITER_CACHEMOD);
        self.db_size = n_page;
        // (O assertTruncateConstraint() já não vale: a função só roda antes de confirmar, e
        // os savepoints abertos não podem mais ser revertidos.)
    }

    /// `pagerSyncHotJournal`: antes de reverter um journal quente, sincroniza-o em disco e põe
    /// `journalHdr` no tamanho do arquivo, para `pager_playback` saber que o journal inteiro
    /// está sincronizado. Assim, se faltar energia durante a reversão, quem tentar depois verá
    /// o mesmo conteúdo.
    pub(crate) fn pager_sync_hot_journal(&mut self) -> i32 {
        let mut rc = SQLITE_OK;
        if !self.no_sync {
            rc = os_sync(file_of!(self, jfd), SQLITE_SYNC_NORMAL);
        }
        if rc == SQLITE_OK {
            rc = file_of!(self, jfd).file_size(&mut self.journal_hdr);
        }
        rc
    }

    /// `pagerAcquireMapPage`: obtém uma referência a um objeto de página mapeada para `pgno`,
    /// que usa os dados `p_data` devolvidos por `xFetch()`. O objeto vem de `mmap_freelist`
    /// se houver, senão é criado. A referência se solta com `pagerReleaseMapPage()` (do
    /// outro arquivo). (A falha de alocação do C, que desfazia o fetch, não existe em Rust.)
    pub(crate) fn pager_acquire_map_page(&mut self, pgno: u32, p_data: Vec<u8>) -> Result<PgId, i32> {
        let idx = match self.mmap_freelist.pop() {
            Some(i) => {
                debug_assert!(self.n_extra >= 8);
                self.mmap_pages[i as usize].extra = E::default();
                i
            }
            None => {
                self.mmap_pages.push(MmapPage {
                    data: Vec::new(),
                    extra: E::default(),
                    pgno: 0,
                    flags: PGHDR_MMAP,
                    n_ref: 1,
                });
                (self.mmap_pages.len() - 1) as u32
            }
        };

        let p = &mut self.mmap_pages[idx as usize];
        debug_assert!(p.flags == PGHDR_MMAP);
        debug_assert!(p.n_ref == 1);

        p.pgno = pgno;
        p.data = p_data;
        self.n_mmap_out += 1;

        Ok(PgId(PGID_MMAP_BASE | idx))
    }
}

// ---------------------------------------------------------------------------
// USO DO WAL (para o integrador reconciliar com wal.rs). Todo método recebe o arquivo do banco
// (`fd: &mut dyn VfsFile`) logo depois de `self`, porque o `Wal` do C guarda o `pDbFd` e aqui
// quem o possui é o pager. Todos devolvem `i32` (código do C) salvo indicação.
//
//   sqlite3WalFindFrame(pWal, pgno, &iFrame)   -> wal.find_frame(fd, pgno: u32, &mut u32) -> i32
//   sqlite3WalReadFrame(pWal, iFrame, n, pOut) -> wal.read_frame(fd, i_frame: u32, out: &mut [u8]) -> i32
//   sqlite3WalUndo(pWal, xUndo, ctx)           -> DUAS FASES (o callback usa o próprio Wal):
//       wal.undo_begin(fd) -> (Vec<u32>, u32)  restaura o hdr do wal-index e devolve os pgno dos
//                                              quadros mxFrame+1..=iMax e o iMax (vazio e
//                                              iMax == mxFrame se não há `writeLock`)
//       wal.undo_end(fd, i_max: u32)           faz o `walCleanupHash` se iMax != mxFrame
//   sqlite3WalFrames(pWal, szPage, pList, nTruncate, isCommit, syncFlags)
//                                              -> wal.frames(fd, sz_page: i32, frames: &mut [WalFrameSrc],
//                                                 n_truncate: u32, is_commit: i32, sync_flags: i32) -> i32
//       (`WalFrameSrc` está em pager.rs; `wal_append` entra com o PGHDR_WAL_APPEND da página e
//        sai com o novo valor; o pager o devolve ao `flags` da página)
//   sqlite3WalEndReadTransaction(pWal)         -> wal.end_read_transaction(fd)            (sem retorno)
//   sqlite3WalBeginReadTransaction(pWal, &chg) -> wal.begin_read_transaction(fd, &mut i32) -> i32
//   sqlite3WalEndWriteTransaction(pWal)        -> wal.end_write_transaction(fd) -> i32
//   sqlite3WalExclusiveMode(pWal, op)          -> wal.exclusive_mode(fd, op: i32) -> i32
//   sqlite3WalDbsize(pWal)                     -> wal.dbsize() -> u32   (`&self`, sem fd; o pager
//                                                 já trata o WAL ausente como zero)
//   sqlite3WalSavepointUndo(pWal, aWalData)    -> wal.savepoint_undo(fd, &[u32; 4]) -> i32
//
// USO DE OUTROS MÓDULOS (a definir pelos respectivos agentes):
//   crate::global::randomness(out: &mut [u8])                       sqlite3_randomness
//   crate::global::log(code: i32, fmt: &[u8], args: &[PrintfArg])   sqlite3_log
//   VfsFile::is_in_memory_journal (acrescentado a os.rs, com a implementação do MemJournal em
//   memjournal.rs): substitui sqlite3JournalIsInMemory(p) sobre um `dyn VfsFile`.
//
// ADIADAS (pertencem a outros chunks ou módulos; este arquivo as chama pelo nome acima):
//   sqlite3PagerRollback, sqlite3PagerLookup, sqlite3PagerGet, sqlite3PagerUnrefNotNull,
//   sqlite3PagerSync, sqlite3PagerOpenWal (pager_ext.rs); pagerReleaseMapPage e pagerFreeMapHdrs
//   (chunk 010); sqlite3BackupRestart e sqlite3BackupUpdate (backup.rs, via `PagerBackup`).
