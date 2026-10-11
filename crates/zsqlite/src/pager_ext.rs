//! Pager (pager.c, chunks 010 a 019): segunda metade do arquivo. Fecha o pager (`close`), abre o
//! journal e o WAL, busca páginas (`get`, `lookup`), torna páginas graváveis (`write`), confirma
//! (`commit_phase_one` e `commit_phase_two`), reverte (`rollback`), savepoints, modo de journal,
//! checkpoint e `pager_open`.
//!
//! É um segundo `impl<E: Default> Pager<E>` do mesmo tipo de `crate::pager`: os campos são os de
//! lá (todos `pub(crate)`) e as funções que o pager.rs deixou `ADIADAS` estão aqui com a
//! assinatura que ele chama (`rollback`, `lookup`, `get`, `unref_not_null`, `sync`, `open_wal`,
//! `release_map_page`, `free_map_hdrs`).
//!
//! Desvios do C, todos de modelo ou de empréstimo (CONVENTIONS.md):
//!
//! * Uma lista de páginas sujas ligadas por `pDirty` é um `&[PgId]` ou um `Vec<PgId>`
//!   (`pcache.dirty_list()` já devolve a lista em ordem crescente de `pgno`). O `pPg->pDirty = 0`
//!   que o `pagerStress` faz para fabricar uma lista de um só elemento é o slice `&[pg]`.
//! * `pPg->pPager != 0` (a página já foi inicializada pelo pager) é `PCache::page_is_init`
//!   consultado ENTRE `fetch` e `fetch_finish` (o finish é quem liga o bit).
//! * O `xStress` do pcache é conduzido aqui, em passos: `fetch_stress_prepare`, `pager_stress`
//!   (com o pager inteiro livre) e `fetch_stress_complete`. Ver `get_page_normal`.
//! * `sqlite3PagerClose(pPager, db)` recebe do `db` só o que o C lê dele: o flag
//!   `SQLITE_NoCkptOnClose` e o sinal de interrupção (ver `PagerCloseDb`).
//! * `sqlite3PagerCheckpoint` não pode executar `PRAGMA table_list` (precisaria da conexão
//!   inteira, que é dona deste pager): o chamador consulta `checkpoint_needs_wal_init` antes.
//! * Falha de alocação (`SQLITE_NOMEM`) não existe em Rust; os ramos que só a tratavam somem.
//! * `SQLITE_ENABLE_ATOMIC_WRITE`, `SQLITE_ENABLE_BATCH_ATOMIC_WRITE`, `SQLITE_ENABLE_SNAPSHOT`,
//!   `SQLITE_ENABLE_SETLK_TIMEOUT`, `SQLITE_ENABLE_ZIPVFS`, `SQLITE_USE_SEH`, `SQLITE_DEBUG`,
//!   `SQLITE_TEST`, `PAGERTRACE`, `IOTRACE`, `pager_pagehash` e `CHECK_PAGE` somem (desligados no
//!   Debian ou só de depuração).
//!
//! Funções do C que viram acesso direto a campo ou a método de outro módulo (não existem aqui):
//! `sqlite3PagerRef` = `pager.pcache.page_ref(pg)`; `sqlite3PagerGetData` = `page_data`;
//! `sqlite3PagerGetExtra` = `page_extra`; `sqlite3PagerPageRefcount` = `page_ref_count`;
//! `sqlite3PagerVfs` = `pager.vfs`; `sqlite3PagerFile` = `pager.fd`; `sqlite3PagerJournalname` =
//! `pager.z_journal`; `sqlite3PagerIsreadonly` = `pager.read_only`; `sqlite3PagerGetJournalMode` =
//! `pager.journal_mode`; `sqlite3PagerBackupPtr` = `pager.backup`; `sqlite3PagerSetCachesize`,
//! `sqlite3PagerSetSpillsize` e `sqlite3PagerShrink` = métodos de `pager.pcache`.

use crate::bitvec::{bitvec_clear, bitvec_create, bitvec_set, bitvec_test};
use crate::consts::{
    DEFAULT_JOURNAL_SIZE_LIMIT, EXCLUSIVE_LOCK, NO_LOCK, PAGER_CACHESPILL, PAGER_GET_NOCONTENT,
    PAGER_GET_READONLY, PAGER_JOURNALMODE_MEMORY, PAGER_JOURNALMODE_OFF, PAGER_JOURNALMODE_WAL,
    PAGER_MEMORY, PAGER_OMIT_JOURNAL, RESERVED_LOCK, SAVEPOINT_RELEASE, SAVEPOINT_ROLLBACK,
    SHARED_LOCK, SQLITE_ABORT, SQLITE_ACCESS_EXISTS, SQLITE_BUSY, SQLITE_CANTOPEN,
    SQLITE_CANTOPEN_BKPT, SQLITE_CANTOPEN_SYMLINK, SQLITE_CHECKPOINT_PASSIVE, SQLITE_CORRUPT_BKPT,
    SQLITE_DBSTATUS_CACHE_HIT, SQLITE_DEFAULT_PAGE_SIZE, SQLITE_DEFAULT_SYNCHRONOUS,
    SQLITE_FCNTL_HAS_MOVED, SQLITE_FCNTL_SIZE_HINT, SQLITE_FCNTL_SYNC, SQLITE_FULL,
    SQLITE_IOCAP_IMMUTABLE, SQLITE_IOCAP_SAFE_APPEND, SQLITE_IOCAP_SEQUENTIAL,
    SQLITE_IOERR_SHORT_READ, SQLITE_MAX_DEFAULT_PAGE_SIZE, SQLITE_MAX_PAGE_COUNT,
    SQLITE_NOMEM_BKPT, SQLITE_NOTFOUND, SQLITE_OK, SQLITE_OK_SYMLINK, SQLITE_OPEN_CREATE,
    SQLITE_OPEN_DELETEONCLOSE, SQLITE_OPEN_EXCLUSIVE, SQLITE_OPEN_MAIN_JOURNAL, SQLITE_OPEN_MEMORY,
    SQLITE_OPEN_NOFOLLOW, SQLITE_OPEN_READONLY, SQLITE_OPEN_READWRITE, SQLITE_OPEN_SUBJOURNAL,
    SQLITE_OPEN_TEMP_JOURNAL, SQLITE_READONLY_DBMOVED, SQLITE_READONLY_ROLLBACK,
    SQLITE_STMTJRNL_SPILL, SQLITE_SYNC_DATAONLY, SQLITE_SYNC_FULL, WAL_SAVEPOINT_NDATA,
};
use crate::memjournal::{journal_open, mem_journal_open};
use crate::os::{
    os_close, os_file_control, os_file_control_hint, os_full_pathname, os_open, os_sync,
    FileControlArg, VfsFile, VfsRef,
};
use crate::os_unix::uri_boolean;
use crate::pager::*;
use crate::pcache::{
    header_size_pcache, PCache, PgId, StressStep, PGHDR_DIRTY, PGHDR_DONT_WRITE, PGHDR_MMAP,
    PGHDR_NEED_SYNC, PGHDR_WRITEABLE,
};
use crate::printf::PrintfArg;
use crate::util::put4byte;
use crate::wal::{Wal, WalHooks};

// ---------------------------------------------------------------------------
// Auxiliares
// ---------------------------------------------------------------------------

/// O que `sqlite3PagerClose` lê da conexão `db` do C: `db->flags & SQLITE_NoCkptOnClose` e o
/// sinal de interrupção (`db->u1.isInterrupted`) que o checkpoint do `sqlite3WalClose` consulta
/// (devolve 0 para seguir, ou o código a devolver).
pub(crate) struct PagerCloseDb<'a> {
    /// `db->flags & SQLITE_NoCkptOnClose`.
    pub no_ckpt_on_close: bool,
    /// Consulta do sinal de interrupção da conexão.
    pub interrupt: &'a mut dyn FnMut() -> i32,
}

/// `zFilename && zFilename[0]`: o nome existe e não é vazio.
fn nonempty(z: Option<&[u8]>) -> Option<&[u8]> {
    z.filter(|z| z.first().is_some_and(|&b| b != 0))
}

/// Nome para passar ao VFS: o `zJournal` e o `zWal` são `NULL` no C quando o pager não tem nome
/// (arquivo temporário e banco em memória sem nome), e aqui são vetores vazios.
fn opt_name(z: &[u8]) -> Option<&[u8]> {
    if z.is_empty() {
        None
    } else {
        Some(z)
    }
}

/// Os `nUriByte` bytes que o `sqlite3PagerOpen` copia de `&zFilename[strlen(zFilename)+1]`: os
/// pares chave e valor (cada um terminado em NUL) mais o NUL que fecha a lista. Num nome sem
/// parâmetros é um único NUL.
fn uri_param_bytes(z: &[u8]) -> Vec<u8> {
    let start = (cstr(z).len() + 1).min(z.len());
    let mut p = start;
    while p < z.len() && z[p] != 0 {
        p += cstr(&z[p..]).len() + 1;
        let q = p.min(z.len());
        p += cstr(&z[q..]).len() + 1;
    }
    let mut out = z[start..p.min(z.len())].to_vec();
    out.push(0);
    out
}

/// `sqlite3_log` do WAL: a mensagem já chega formatada.
fn wal_log(code: i32, msg: &[u8]) {
    crate::global::log(code, b"%s", &[PrintfArg::Text(Some(msg.to_vec()))]);
}

/// As funções globais que o `Wal::open` pede (`sqlite3_randomness` e `sqlite3_log`).
fn wal_hooks() -> WalHooks {
    WalHooks {
        randomness: crate::global::randomness,
        log: wal_log,
    }
}

// ---------------------------------------------------------------------------
// sqlite3PagerOpen
// ---------------------------------------------------------------------------

/// `sqlite3PagerOpen`: aloca e inicializa um pager. `z_filename` é um `sqlite3_filename`
/// (caminho, NUL, pares de URI, NUL final) ou `None`/vazio para um arquivo temporário; com
/// `PAGER_MEMORY` o nome só dá o nome do banco em memória e nunca é aberto. `n_extra` é o
/// `sizeof(MemPage)` do btree (arredondado para 8 aqui). Em erro o arquivo já aberto é fechado.
pub(crate) fn pager_open<E: Default>(
    vfs: VfsRef,
    z_filename: Option<&[u8]>,
    n_extra: i32,
    flags: i32,
    vfs_flags: i32,
    reiniter: Option<fn(&mut E, &mut [u8], i64)>,
) -> Result<Pager<E>, i32> {
    let mut rc = SQLITE_OK;
    let mut temp_file = false; // verdadeiro para temporários (inclusive em memória)
    let mut mem_db = false; // verdadeiro para banco em memória
    let mut mem_jm = false; // modo de journal em memória
    let mut read_only = false; // verdadeiro se o arquivo é somente leitura
    let mut vfs_flags = vfs_flags;
    let use_journal = (flags & PAGER_OMIT_JOURNAL) == 0; // falso para omitir o journal
    let mut sz_page_dflt: u32 = SQLITE_DEFAULT_PAGE_SIZE as u32; // tamanho de página padrão
    let mut z_pathname: Vec<u8> = Vec::new(); // caminho completo do banco
    let mut uri_bytes: Vec<u8> = vec![0]; // parâmetros de URI a copiar (nUriByte = 1 sem eles)

    let mut z_name = nonempty(z_filename);

    if flags & PAGER_MEMORY != 0 {
        mem_db = true;
        if let Some(z) = z_name {
            z_pathname = cstr(z).to_vec();
            z_name = None;
        }
    }

    // Calcula o caminho completo. Para temporário, o caminho fica vazio.
    if let Some(z) = z_name {
        let n_pathname = vfs.max_pathname() + 1;
        let mut full: Vec<u8> = Vec::new();
        rc = os_full_pathname(&*vfs, cstr(z), n_pathname, &mut full);
        if rc != SQLITE_OK && rc == SQLITE_OK_SYMLINK {
            if vfs_flags & SQLITE_OPEN_NOFOLLOW != 0 {
                rc = SQLITE_CANTOPEN_SYMLINK;
            } else {
                rc = SQLITE_OK;
            }
        }
        z_pathname = cstr(&full).to_vec();
        uri_bytes = uri_param_bytes(z);
        if rc == SQLITE_OK && (z_pathname.len() as i64) + 8 > vfs.max_pathname() as i64 {
            // O caminho do journal passaria de mxPathname: o banco não pode ser aberto, pois
            // não seria possível abrir o journal nem procurar um journal quente.
            rc = SQLITE_CANTOPEN_BKPT;
        }
        if rc != SQLITE_OK {
            return Err(rc);
        }
    }
    let n_pathname = z_pathname.len();

    // Os nomes: banco (caminho, NUL, parâmetros de URI), journal e WAL.
    let z_file_blob: Vec<u8> = if n_pathname > 0 {
        let mut b = z_pathname.clone();
        b.push(0);
        b.extend_from_slice(&uri_bytes);
        b
    } else {
        vec![0, 0]
    };
    let (z_journal, z_wal) = if n_pathname > 0 {
        let mut j = z_pathname.clone();
        j.extend_from_slice(b"-journal");
        let mut w = z_pathname.clone();
        w.extend_from_slice(b"-wal");
        (filename_blob(&j), filename_blob(&w))
    } else {
        (Vec::new(), Vec::new())
    };

    // O cache nasce com o tamanho de página padrão e é refeito por `set_pagesize` abaixo, que
    // no C vem antes de `sqlite3PcacheOpen` (o estado final é o mesmo).
    let n_extra = (n_extra + 7) & !7;
    debug_assert!((8..1000).contains(&n_extra));
    let pcache: PCache<E> = PCache::open(SQLITE_DEFAULT_PAGE_SIZE, n_extra, !mem_db);
    let mut pager: Pager<E> = Pager::zeroed(vfs.clone(), pcache);
    pager.z_filename = z_file_blob;
    pager.z_journal = z_journal;
    pager.z_wal = z_wal;
    pager.vfs_flags = vfs_flags as u32;

    // Abre o arquivo do pager.
    let mut act_like_temp_file = true;
    if z_name.is_some() {
        act_like_temp_file = false;
        let mut fout: i32 = 0; // flags do VFS devolvidas por xOpen()
        match os_open(&*vfs, Some(pager.z_filename.as_slice()), vfs_flags, &mut fout) {
            Ok(f) => pager.fd = Some(f),
            Err(e) => rc = e,
        }
        pager.mem_vfs = (fout & SQLITE_OPEN_MEMORY) != 0;
        mem_jm = pager.mem_vfs;
        read_only = (fout & SQLITE_OPEN_READONLY) != 0;

        // Com o arquivo aberto em leitura e escrita, escolhe o tamanho de página padrão para o
        // caso de ter de criar o banco: o maior entre SQLITE_DEFAULT_PAGE_SIZE, o tamanho de
        // setor e (sob ATOMIC_WRITE, desligado no Debian) o maior tamanho atômico.
        if rc == SQLITE_OK {
            let i_dc = pager.fd_device_characteristics();
            if !read_only {
                pager.set_sector_size();
                debug_assert!(SQLITE_DEFAULT_PAGE_SIZE <= SQLITE_MAX_DEFAULT_PAGE_SIZE);
                if (sz_page_dflt as i64) < pager.sector_size as i64 {
                    if pager.sector_size as i64 > SQLITE_MAX_DEFAULT_PAGE_SIZE as i64 {
                        sz_page_dflt = SQLITE_MAX_DEFAULT_PAGE_SIZE as u32;
                    } else {
                        sz_page_dflt = pager.sector_size;
                    }
                }
            }
            pager.no_lock = uri_boolean(Some(pager.z_filename.as_slice()), b"nolock", false);
            if (i_dc & SQLITE_IOCAP_IMMUTABLE) != 0
                || uri_boolean(Some(pager.z_filename.as_slice()), b"immutable", false)
            {
                vfs_flags |= SQLITE_OPEN_READONLY;
                act_like_temp_file = true; // goto act_like_temp_file
            }
        }
    }
    if act_like_temp_file {
        // Um arquivo temporário não é aberto de imediato: aceita o tamanho de página padrão e
        // adia a abertura até o primeiro xWrite(). O mesmo vale para banco em memória (um
        // temporário que nunca vai a disco e usa journal em memória) e para arquivo imutável.
        temp_file = true;
        pager.e_state = PAGER_READER; // finge que já tem trava
        pager.e_lock = EXCLUSIVE_LOCK; // finge estar em modo exclusivo
        pager.no_lock = true; // sem travas
        read_only = (vfs_flags & SQLITE_OPEN_READONLY) != 0;
    }

    // `set_pagesize` fixa `pageSize` e aloca `pTmpSpace`.
    if rc == SQLITE_OK {
        debug_assert!(!pager.mem_db);
        rc = pager.set_pagesize(&mut sz_page_dflt, -1);
    }

    // Em erro fecha o arquivo e descarta o pager.
    if rc != SQLITE_OK {
        os_close(&mut pager.fd);
        return Err(rc);
    }

    pager.use_journal = use_journal;
    pager.mx_pgno = SQLITE_MAX_PAGE_COUNT;
    pager.temp_file = temp_file;
    pager.exclusive_mode = temp_file; // PAGER_LOCKINGMODE_EXCLUSIVE == 1
    pager.change_count_done = pager.temp_file;
    pager.mem_db = mem_db;
    pager.read_only = read_only;
    debug_assert!(use_journal || pager.temp_file);
    pager.set_flags(((SQLITE_DEFAULT_SYNCHRONOUS + 1) as u32) | PAGER_CACHESPILL);
    pager.n_extra = n_extra as u16;
    pager.journal_size_limit = DEFAULT_JOURNAL_SIZE_LIMIT;
    debug_assert!(pager.fd.is_some() || temp_file);
    pager.set_sector_size();
    if !use_journal {
        pager.journal_mode = PAGER_JOURNALMODE_OFF;
    } else if mem_db || mem_jm {
        pager.journal_mode = PAGER_JOURNALMODE_MEMORY;
    }
    pager.reiniter = reiniter;
    pager.set_getter_method();

    Ok(pager)
}

impl<E: Default> Pager<E> {
    // ------------------------------------------------------------------
    // Páginas mapeadas e fechamento
    // ------------------------------------------------------------------

    /// `pagerReleaseMapPage`: solta a referência a uma página devolvida antes por
    /// `pager_acquire_map_page`. O objeto vai para `mmap_freelist` e os bytes voltam ao VFS.
    pub(crate) fn release_map_page(&mut self, pg: PgId) {
        debug_assert!(pg.0 & PGID_MMAP_BASE != 0);
        let idx = (pg.0 & !PGID_MMAP_BASE) as usize;
        self.n_mmap_out -= 1;
        self.mmap_freelist.push(idx as u32);

        debug_assert!(self.fd.as_ref().is_some_and(|f| f.i_version() >= 3));
        let ofst = (self.mmap_pages[idx].pgno as i64 - 1) * self.page_size;
        let data = std::mem::take(&mut self.mmap_pages[idx].data);
        if let Some(fd) = self.fd.as_deref_mut() {
            fd.unfetch(ofst, Some(data));
        }
    }

    /// `pagerFreeMapHdrs`: libera todos os objetos de página guardados em `mmap_freelist`.
    pub(crate) fn free_map_hdrs(&mut self) {
        let free = std::mem::take(&mut self.mmap_freelist);
        for i in free {
            let p = &mut self.mmap_pages[i as usize];
            p.data = Vec::new();
            p.extra = E::default();
        }
    }

    /// `databaseIsUnmoved`: confere que o arquivo do banco não foi apagado nem renomeado por
    /// fora. `SQLITE_OK` se ele segue onde deveria; `SQLITE_READONLY_DBMOVED` ou outro erro do
    /// `xFileControl` se sumiu.
    fn database_is_unmoved(&mut self) -> i32 {
        if self.temp_file {
            return SQLITE_OK;
        }
        if self.db_size == 0 {
            return SQLITE_OK;
        }
        debug_assert!(self.z_filename.first().is_some_and(|&b| b != 0));
        let mut arg = FileControlArg::Int(0);
        let mut rc = os_file_control(self.fd.as_deref_mut(), SQLITE_FCNTL_HAS_MOVED, &mut arg);
        if rc == SQLITE_NOTFOUND {
            // Sem o file-control HAS_MOVED presume que o arquivo não mudou de lugar: é o
            // comportamento histórico (antes da 3.8.3 nunca se conferia).
            rc = SQLITE_OK;
        } else if rc == SQLITE_OK && matches!(arg, FileControlArg::Int(n) if n != 0) {
            rc = SQLITE_READONLY_DBMOVED;
        }
        rc
    }

    /// `sqlite3PagerClose`: encerra o cache de páginas, libera a memória e fecha os arquivos.
    /// Sempre dá certo: com transação ativa tenta revertê-la e, se a reversão falha, pode sobrar
    /// um journal quente sem que o erro volte ao chamador. Páginas ainda referenciadas deixam
    /// de existir. `db` é a conexão (`None` só vale sem WAL).
    pub(crate) fn close(mut self, db: Option<PagerCloseDb<'_>>) -> i32 {
        debug_assert!(db.is_some() || !self.pager_use_wal());
        debug_assert!(self.assert_pager_state());
        self.free_map_hdrs();
        // pPager->errCode = 0;
        self.exclusive_mode = false;
        {
            debug_assert!(db.is_some() || self.wal.is_none());
            let want_ckpt = match db.as_ref() {
                Some(d) => !d.no_ckpt_on_close && self.database_is_unmoved() == SQLITE_OK,
                None => false,
            };
            if let Some(wal) = self.wal.take() {
                let sz = self.page_size as usize;
                let mut no_interrupt = || 0;
                let interrupt: &mut dyn FnMut() -> i32 = match db {
                    Some(d) => d.interrupt,
                    None => &mut no_interrupt,
                };
                let buf: Option<&mut [u8]> = if want_ckpt {
                    Some(&mut self.tmp_space[..sz])
                } else {
                    None
                };
                wal.close(file_of!(self, fd), interrupt, self.wal_sync_flags, buf);
            }
        }
        self.pager_reset();
        if self.mem_db {
            self.pager_unlock();
        } else {
            // Com o journal aberto, sincroniza-o antes de `pager_unlock_and_rollback`: sem isso
            // uma parte não sincronizada poderia ser reproduzida no banco e, se faltasse
            // energia nesse momento, o banco ficaria corrompido. Se o sync falha, o pager vai
            // para o estado de erro: o rollback então só destrava e fecha o journal, e o próximo
            // usuário reverte o journal quente.
            if self.jfd.is_some() {
                let rc = self.pager_sync_hot_journal();
                self.pager_error(rc);
            }
            self.pager_unlock_and_rollback();
        }
        os_close(&mut self.jfd);
        os_close(&mut self.fd);
        self.pcache.close();
        debug_assert!(self.a_savepoint.is_empty() && self.in_journal.is_none());
        debug_assert!(self.jfd.is_none() && self.sjfd.is_none());
        SQLITE_OK
    }

    /// `syncJournal`: faz as páginas já gravadas no journal chegarem de fato ao disco, para que
    /// possam ser restauradas por um journal quente. Sem efeito com `noSync`. Senão, conforme o modo e as características do
    /// dispositivo: journal em memória não faz nada; sem `SAFE_APPEND` o campo `nRec` do último
    /// cabeçalho é atualizado (com sync antes, em full-sync); sem `SEQUENTIAL` o journal é
    /// sincronizado. Em sucesso limpa `PGHDR_NEED_SYNC` de todas as páginas.
    fn sync_journal(&mut self, new_hdr: bool) -> i32 {
        debug_assert!(
            self.e_state == PAGER_WRITER_CACHEMOD || self.e_state == PAGER_WRITER_DBMOD
        );
        debug_assert!(self.assert_pager_state());
        debug_assert!(!self.pager_use_wal());

        let mut rc = self.exclusive_lock();
        if rc != SQLITE_OK {
            return rc;
        }

        if !self.no_sync {
            debug_assert!(!self.temp_file);
            if self.jfd.is_some() && self.journal_mode != PAGER_JOURNALMODE_MEMORY {
                let i_dc = self.fd_device_characteristics();

                if 0 == (i_dc & SQLITE_IOCAP_SAFE_APPEND) {
                    // Problema obscuro: se a última conexão a escrever no banco usava journal
                    // persistente, o arquivo pode ser maior que journalOff e ter, logo depois, um
                    // cabeçalho de journal antigo. Se acontecer uma queda depois de nRec ser
                    // atualizado e antes de qualquer outra escrita, a reversão do journal quente
                    // reverteria os dados desta conexão e depois os antigos: corrupção. Para
                    // evitar, escreve um byte 0x00 no início desse cabeçalho, se ele existir.
                    let mut a_magic = [0u8; 8];
                    let mut z_header = [0u8; 12];
                    z_header[..8].copy_from_slice(&A_JOURNAL_MAGIC);
                    put4byte(&mut z_header[8..], self.n_rec as u32);

                    let i_next_hdr_offset = self.journal_hdr_offset();
                    rc = file_of!(self, jfd).read(&mut a_magic, i_next_hdr_offset);
                    if rc == SQLITE_OK && a_magic == A_JOURNAL_MAGIC {
                        rc = file_of!(self, jfd).write(&[0u8], i_next_hdr_offset);
                    }
                    if rc != SQLITE_OK && rc != SQLITE_IOERR_SHORT_READ {
                        return rc;
                    }

                    // Grava nRec no cabeçalho. Em full-sync sincroniza antes, para garantir
                    // que tudo chegou ao disco antes de marcar o journal como candidato a
                    // reversão. Com SAFE_APPEND nRec nasce 0xFFFFFFFF e nunca é atualizado.
                    if self.full_sync && 0 == (i_dc & SQLITE_IOCAP_SEQUENTIAL) {
                        rc = os_sync(file_of!(self, jfd), self.sync_flags);
                        if rc != SQLITE_OK {
                            return rc;
                        }
                    }
                    rc = file_of!(self, jfd).write(&z_header, self.journal_hdr);
                    if rc != SQLITE_OK {
                        return rc;
                    }
                }
                if 0 == (i_dc & SQLITE_IOCAP_SEQUENTIAL) {
                    let extra = if self.sync_flags == SQLITE_SYNC_FULL {
                        SQLITE_SYNC_DATAONLY
                    } else {
                        0
                    };
                    rc = os_sync(file_of!(self, jfd), self.sync_flags | extra);
                    if rc != SQLITE_OK {
                        return rc;
                    }
                }

                self.journal_hdr = self.journal_off;
                if new_hdr && 0 == (i_dc & SQLITE_IOCAP_SAFE_APPEND) {
                    self.n_rec = 0;
                    rc = self.write_journal_hdr();
                    if rc != SQLITE_OK {
                        return rc;
                    }
                }
            } else {
                self.journal_hdr = self.journal_off;
            }
        }

        // Fora do modo noSync o journal acabou de ser sincronizado; de todo jeito, limpa
        // PGHDR_NEED_SYNC de todas as páginas.
        self.pcache.clear_sync_flags();
        self.e_state = PAGER_WRITER_DBMOD;
        debug_assert!(self.assert_pager_state());
        SQLITE_OK
    }

    /// `pager_write_pagelist`: grava no arquivo do banco cada página da lista (que o C liga por
    /// `pDirty`; aqui um slice, possivelmente vazio). O pager precisa de ao menos uma trava
    /// RESERVED e, antes de gravar, ela já subiu a EXCLUSIVE (feito pelo chamador). Se o pager é
    /// de arquivo temporário e o arquivo ainda não existe, ele é criado e aberto antes. Páginas
    /// com número maior que `dbSize` ou com `PGHDR_DONT_WRITE` são puladas. Em página 1 atualiza
    /// `dbFileVers`; se o arquivo cresce, `dbFileSize`.
    fn pager_write_pagelist(&mut self, list: &[PgId]) -> i32 {
        let mut rc = SQLITE_OK;

        // Só roda em pager de rollback no estado WRITER_DBMOD.
        debug_assert!(!self.pager_use_wal());
        debug_assert!(self.temp_file || self.e_state == PAGER_WRITER_DBMOD);
        debug_assert!(self.e_lock == EXCLUSIVE_LOCK);
        debug_assert!(self.fd.is_some() || list.len() <= 1);

        // Arquivo temporário ainda não aberto: abre agora. `pager_wait_on_lock` é no-op para
        // temporários, então rc não pode ser outro que SQLITE_OK na prática.
        if self.fd.is_none() {
            debug_assert!(self.temp_file);
            match self.pager_opentemp(self.vfs_flags as i32) {
                Ok(f) => self.fd = Some(f),
                Err(e) => rc = e,
            }
        }

        // Antes da primeira escrita, dá ao VFS uma dica do tamanho final do arquivo.
        debug_assert!(rc != SQLITE_OK || self.fd.is_some());
        if rc == SQLITE_OK && self.db_hint_size < self.db_size {
            if let Some(&first) = list.first() {
                if list.len() > 1 || self.pcache.page_pgno(first) > self.db_hint_size {
                    let sz_file = self.page_size * self.db_size as i64;
                    let mut arg = FileControlArg::Int64(sz_file);
                    os_file_control_hint(self.fd.as_deref_mut(), SQLITE_FCNTL_SIZE_HINT, &mut arg);
                    self.db_hint_size = self.db_size;
                }
            }
        }

        let sz = self.page_size as usize;
        for &pg in list {
            if rc != SQLITE_OK {
                break;
            }
            let pgno = self.pcache.page_pgno(pg);

            // Páginas sujas com número maior que dbSize significam que sqlite3PagerTruncateImage()
            // reduziu o arquivo (o auto-vacuum): não são gravadas. Também não se grava página
            // com PGHDR_DONT_WRITE (ligado por sqlite3PagerDontWrite()).
            if pgno <= self.db_size && 0 == (self.pcache.page_flags(pg) & PGHDR_DONT_WRITE) {
                let offset = (pgno as i64 - 1) * self.page_size;

                debug_assert!((self.pcache.page_flags(pg) & PGHDR_NEED_SYNC) == 0);
                if pgno == 1 {
                    self.pager_write_changecounter(pg);
                }

                // Grava os dados da página.
                rc = file_of!(self, fd).write(&self.pcache.page_data(pg)[..sz], offset);

                // Se gravou a página 1, dbFileVers passa a valer o que está no arquivo. Se a
                // gravação fez o arquivo crescer, atualiza dbFileSize.
                if pgno == 1 {
                    self.db_file_vers
                        .copy_from_slice(&self.pcache.page_data(pg)[24..40]);
                }
                if pgno > self.db_file_size {
                    self.db_file_size = pgno;
                }
                self.a_stat[PAGER_STAT_WRITE] += 1;

                // Avisa os backups que copiam este pager.
                if let Some(b) = self.backup.as_mut() {
                    b.update(pgno, &self.pcache.page_data(pg)[..sz]);
                }
            }
        }

        rc
    }

    // ------------------------------------------------------------------
    // Sub-journal
    // ------------------------------------------------------------------

    /// `openSubJournal`: garante que o sub-journal esteja aberto. Em memória se o modo é
    /// MEMORY ou `subjInMemory`; senão um arquivo temporário que passa a disco quando o
    /// sub-journal chega ao limite `nStmtSpill`.
    fn open_sub_journal(&mut self) -> i32 {
        let mut rc = SQLITE_OK;
        if self.sjfd.is_none() {
            let flags = SQLITE_OPEN_SUBJOURNAL
                | SQLITE_OPEN_READWRITE
                | SQLITE_OPEN_CREATE
                | SQLITE_OPEN_EXCLUSIVE
                | SQLITE_OPEN_DELETEONCLOSE;
            let mut n_stmt_spill = SQLITE_STMTJRNL_SPILL;
            if self.journal_mode == PAGER_JOURNALMODE_MEMORY || self.subj_in_memory {
                n_stmt_spill = -1;
            }
            match journal_open(Some(self.vfs.clone()), None, flags, n_stmt_spill) {
                Ok(j) => self.sjfd = Some(Box::new(j)),
                Err(e) => rc = e,
            }
        }
        rc
    }

    /// `subjournalPage`: acrescenta ao sub-journal o registro do estado atual da página. Em
    /// sucesso liga o bit de `pgno` nos bitvecs de todos os savepoints abertos.
    fn subjournal_page(&mut self, pg: PgId) -> i32 {
        let mut rc = SQLITE_OK;
        let pgno = self.pcache.page_pgno(pg);
        if self.journal_mode != PAGER_JOURNALMODE_OFF {
            // Abre o sub-journal, se ainda não está aberto.
            debug_assert!(self.use_journal);
            debug_assert!(self.jfd.is_some() || self.pager_use_wal());
            debug_assert!(self.sjfd.is_some() || self.n_sub_rec == 0);
            debug_assert!(
                self.pager_use_wal() || self.page_in_journal(pg) || pgno > self.db_orig_size
            );
            rc = self.open_sub_journal();

            // Com o sub-journal aberto (ou já aberto), grava o registro.
            if rc == SQLITE_OK {
                let sz = self.page_size as usize;
                let offset = self.n_sub_rec as i64 * (4 + self.page_size);
                rc = write32bits(file_of!(self, sjfd), offset, pgno);
                if rc == SQLITE_OK {
                    rc = file_of!(self, sjfd).write(&self.pcache.page_data(pg)[..sz], offset + 4);
                }
            }
        }
        if rc == SQLITE_OK {
            self.n_sub_rec += 1;
            debug_assert!(!self.a_savepoint.is_empty());
            rc = self.add_to_savepoint_bitvecs(pgno);
        }
        rc
    }

    /// `subjournalPageIfRequired`.
    fn subjournal_page_if_required(&mut self, pg: PgId) -> i32 {
        if self.subj_requires_page(pg) {
            self.subjournal_page(pg)
        } else {
            SQLITE_OK
        }
    }

    // ------------------------------------------------------------------
    // Spill e flush
    // ------------------------------------------------------------------

    /// `pagerStress`: chamada pelo cache quando atinge um limite macio de memória. `pg` é uma
    /// página suja sem referências pendentes (de um pager sempre "purgável", nunca em memória).
    /// Torna a página limpa gravando-a no arquivo, o que pode exigir sincronizar o journal. Em
    /// sucesso chama `make_clean` e devolve `SQLITE_OK`. Em erro de E/S devolve o código; se
    /// a página não pode ser limpa por outro motivo, devolve `SQLITE_OK` sem chamar `make_clean`.
    fn pager_stress(&mut self, pg: PgId) -> i32 {
        let mut rc = SQLITE_OK;

        debug_assert!(self.pcache.page_flags(pg) & PGHDR_DIRTY != 0);

        // O bit NOSYNC de doNotSpill vale quando sincronizar o journal (e pôr cabeçalho novo)
        // é proibido: durante sqlite3PagerWrite() ao registrar várias páginas do mesmo setor.
        // Os bits ROLLBACK e OFF inibem todo spill, haja ou não sync a fazer. Em estado de
        // erro também não se espalha (na implementação atual é impossível chegar aqui assim).
        if self.err_code != 0 {
            return SQLITE_OK;
        }
        if self.do_not_spill != 0
            && ((self.do_not_spill & (SPILLFLAG_ROLLBACK | SPILLFLAG_OFF)) != 0
                || (self.pcache.page_flags(pg) & PGHDR_NEED_SYNC) != 0)
        {
            return SQLITE_OK;
        }

        self.a_stat[PAGER_STAT_SPILL] += 1;
        if self.pager_use_wal() {
            // Grava um único quadro para esta página no log.
            rc = self.subjournal_page_if_required(pg);
            if rc == SQLITE_OK {
                rc = self.pager_wal_frames(vec![pg], 0, false);
            }
        } else {
            // Sincroniza o journal, se for preciso.
            if (self.pcache.page_flags(pg) & PGHDR_NEED_SYNC) != 0
                || self.e_state == PAGER_WRITER_CACHEMOD
            {
                rc = self.sync_journal(true);
            }

            // Grava o conteúdo da página no arquivo do banco.
            if rc == SQLITE_OK {
                debug_assert!((self.pcache.page_flags(pg) & PGHDR_NEED_SYNC) == 0);
                rc = self.pager_write_pagelist(&[pg]);
            }
        }

        // Marca a página como limpa.
        if rc == SQLITE_OK {
            self.pcache.make_clean(pg);
        }

        self.pager_error(rc)
    }

    /// `sqlite3PagerFlush`: grava em disco todas as páginas sujas sem referências.
    pub(crate) fn flush(&mut self) -> i32 {
        let mut rc = self.err_code;
        if !self.mem_db {
            let list = self.pcache.dirty_list();
            debug_assert!(self.assert_pager_state());
            for pg in list {
                if rc != SQLITE_OK {
                    break;
                }
                if self.pcache.page_ref_count(pg) == 0 {
                    rc = self.pager_stress(pg);
                }
            }
        }
        rc
    }

    // ------------------------------------------------------------------
    // Trava compartilhada e journal quente
    // ------------------------------------------------------------------

    /// `hasHotJournal`: chamada depois de PAGER_UNLOCK para PAGER_SHARED. Um journal quente
    /// existe no sistema de arquivos, nenhum processo tem trava RESERVED ou maior, o banco tem
    /// mais de 0 bytes e o primeiro byte do journal existe e não é 0x00. Com banco de 0 páginas
    /// e um journal, o journal é resto de um banco anterior e é apagado. Não confere o nome de
    /// super-journal no fim: um falso positivo é descoberto por `pager_playback`. `p_exists`
    /// recebe o resultado; em erro de E/S o código volta e `p_exists` fica indefinido.
    fn has_hot_journal(&mut self, p_exists: &mut bool) -> i32 {
        let vfs = self.vfs.clone();
        let mut rc = SQLITE_OK;
        let mut exists: i32 = 1; // verdadeiro se há um arquivo de journal
        let jrnl_open = self.jfd.is_some();

        debug_assert!(self.use_journal);
        debug_assert!(self.fd.is_some());
        debug_assert!(self.e_state == PAGER_OPEN);

        *p_exists = false;
        if !jrnl_open {
            rc = vfs.access(cstr(&self.z_journal), SQLITE_ACCESS_EXISTS, &mut exists);
        }
        if rc == SQLITE_OK && exists != 0 {
            let mut locked: i32 = 0; // verdadeiro se algum processo tem trava RESERVED

            // Condição de corrida: outro processo pode ter segurado a trava RESERVED e o
            // journal no access() acima, e apagado tudo antes do check_reserved_lock(). O
            // resultado é um falso positivo que o playback resolve (ticket #3883).
            rc = file_of!(self, fd).check_reserved_lock(&mut locked);
            if rc == SQLITE_OK && locked == 0 {
                let mut n_page: u32 = 0; // número de páginas do arquivo

                debug_assert!(!self.temp_file);
                rc = self.pager_pagecount(&mut n_page);
                if rc == SQLITE_OK {
                    // Banco de 0 páginas: ou o journal é resto de um banco anterior com o mesmo
                    // nome (só o arquivo do banco foi apagado), ou a transação inicial que
                    // popula um banco novo está sendo revertida. Nos dois casos o journal pode
                    // ser apagado, salvo se já está aberto por journal_mode=PERSIST.
                    if n_page == 0 && !jrnl_open {
                        if self.pager_lock_db(RESERVED_LOCK) == SQLITE_OK {
                            vfs.delete(cstr(&self.z_journal), 0);
                            if !self.exclusive_mode {
                                self.pager_unlock_db(SHARED_LOCK);
                            }
                        }
                    } else {
                        // O journal existe e ninguém tem trava RESERVED ou maior. Confere se há
                        // ao menos um byte não nulo no início: se houver, é quente.
                        if !jrnl_open {
                            let f = SQLITE_OPEN_READONLY | SQLITE_OPEN_MAIN_JOURNAL;
                            let mut out_flags = 0;
                            match os_open(&*vfs, opt_name(&self.z_journal), f, &mut out_flags) {
                                Ok(j) => self.jfd = Some(j),
                                Err(e) => rc = e,
                            }
                        }
                        if rc == SQLITE_OK {
                            let mut first = [0u8; 1];
                            rc = file_of!(self, jfd).read(&mut first, 0);
                            if rc == SQLITE_IOERR_SHORT_READ {
                                rc = SQLITE_OK;
                            }
                            if !jrnl_open {
                                os_close(&mut self.jfd);
                            }
                            *p_exists = first[0] != 0;
                        } else if rc == SQLITE_CANTOPEN {
                            // Sem poder abrir o journal para ver se o cabeçalho é zero (erro de
                            // E/S ou a corrida acima), presume que é quente: pode ser falso
                            // positivo, que a reversão automática resolve sob trava EXCLUSIVE.
                            *p_exists = true;
                            rc = SQLITE_OK;
                        }
                    }
                }
            }
        }

        rc
    }

    /// `sqlite3PagerSharedLock`: obtém trava compartilhada no banco. É ilegal chamar `get`
    /// antes de esta função dar certo; se a trava já existe é um no-op. Em `PAGER_OPEN` tenta
    /// SHARED, procura um journal quente (revertendo-o, se houver) e invalida o cache se o
    /// change-counter do arquivo mudou. Em modo exclusivo, sem referências pendentes e em
    /// estado de erro, tenta limpar o erro descartando o cache e revertendo o journal aberto.
    pub(crate) fn shared_lock(&mut self) -> i32 {
        let mut rc = SQLITE_OK; // código de retorno

        // Só o btree chama, e só sem páginas pendentes: o estado é OPEN ou READER (READER só
        // se o pager é ou foi exclusivo).
        debug_assert!(self.pcache.ref_count() == 0);
        debug_assert!(self.assert_pager_state());
        debug_assert!(self.e_state == PAGER_OPEN || self.e_state == PAGER_READER);
        debug_assert!(self.err_code == SQLITE_OK);

        'failed: {
            if !self.pager_use_wal() && self.e_state == PAGER_OPEN {
                let mut b_hot_journal = true; // verdadeiro se há um journal quente

                debug_assert!(!self.mem_db);
                debug_assert!(!self.temp_file || self.e_lock == EXCLUSIVE_LOCK);

                rc = self.pager_wait_on_lock(SHARED_LOCK);
                if rc != SQLITE_OK {
                    debug_assert!(self.e_lock == NO_LOCK || self.e_lock == UNKNOWN_LOCK);
                    break 'failed;
                }

                // Se há um journal e ninguém tem trava RESERVED, ele precisa ser revertido ou
                // apagado.
                if self.e_lock <= SHARED_LOCK {
                    rc = self.has_hot_journal(&mut b_hot_journal);
                }
                if rc != SQLITE_OK {
                    break 'failed;
                }
                if b_hot_journal {
                    if self.read_only {
                        rc = SQLITE_READONLY_ROLLBACK;
                        break 'failed;
                    }

                    // Pega a trava EXCLUSIVE sem passar por RESERVED: se passasse, outro
                    // processo poderia ver a trava RESERVED e concluir que o banco é seguro
                    // para ler enquanto este reverte o journal quente. Sem passar por ela,
                    // quem tentar também falha na trava EXCLUSIVE. Fora do modo exclusivo a
                    // trava volta a SHARED antes de esta função retornar.
                    rc = self.pager_lock_db(EXCLUSIVE_LOCK);
                    if rc != SQLITE_OK {
                        break 'failed;
                    }

                    // Se ainda não está aberto e o arquivo existe, abre o journal em leitura e
                    // escrita: em modo exclusivo o descritor fica aberto e pode servir a uma
                    // transação seguinte, e a escrita costuma ser necessária para finalizar o
                    // journal em modo PERSIST (e TRUNCATE em alguns sistemas). Sem o arquivo, é
                    // que outra conexão o reverteu antes da trava acima, ou o pager estava em
                    // estado de erro.
                    if self.jfd.is_none() && self.journal_mode != PAGER_JOURNALMODE_OFF {
                        let vfs = self.vfs.clone();
                        let mut b_exists: i32 = 0; // verdadeiro se o journal existe
                        rc = vfs.access(cstr(&self.z_journal), SQLITE_ACCESS_EXISTS, &mut b_exists);
                        if rc == SQLITE_OK && b_exists != 0 {
                            let mut fout = 0;
                            let f = SQLITE_OPEN_READWRITE | SQLITE_OPEN_MAIN_JOURNAL;
                            debug_assert!(!self.temp_file);
                            match os_open(&*vfs, opt_name(&self.z_journal), f, &mut fout) {
                                Ok(j) => {
                                    self.jfd = Some(j);
                                    if fout & SQLITE_OPEN_READONLY != 0 {
                                        rc = SQLITE_CANTOPEN_BKPT;
                                        os_close(&mut self.jfd);
                                    }
                                }
                                Err(e) => rc = e,
                            }
                        }
                    }

                    // Reproduz e apaga o journal, larga a trava de escrita e retoma a de
                    // leitura. Zera o cache antes para não ficar inconsistente. Sincroniza o
                    // journal quente antes: o processo que caiu provavelmente não o sincronizou
                    // e é obrigatório sincronizar sempre antes de reproduzir.
                    if self.jfd.is_some() {
                        debug_assert!(rc == SQLITE_OK);
                        rc = self.pager_sync_hot_journal();
                        if rc == SQLITE_OK {
                            rc = self.pager_playback(!self.temp_file);
                            self.e_state = PAGER_OPEN;
                        }
                    } else if !self.exclusive_mode {
                        self.pager_unlock_db(SHARED_LOCK);
                    }

                    if rc != SQLITE_OK {
                        // Erro ao abrir ou reverter um journal quente com trava EXCLUSIVE:
                        // `pager_unlock` será chamado antes de retornar, para soltar o arquivo.
                        // Se soltar falhar, eLock tem de virar UNKNOWN_LOCK; para
                        // `pager_unlock` fazer isso basta pôr eState em PAGER_ERROR agora. Isso
                        // não conta como transição ao estado de erro do diagrama, pois a mesma
                        // chamada logo leva o pager a OPEN.
                        self.pager_error(rc);
                        break 'failed;
                    }

                    debug_assert!(self.e_state == PAGER_OPEN);
                    debug_assert!(
                        self.e_lock == SHARED_LOCK
                            || (self.exclusive_mode && self.e_lock > SHARED_LOCK)
                    );
                }

                if !self.temp_file && self.has_held_shared_lock {
                    // Acabou de pegar a trava compartilhada: vê se o banco foi modificado. Se
                    // sim, descarta o cache. O flag hasHeldSharedLock evita isto no primeiro
                    // acesso ao arquivo, poupando um xRead. A mudança se detecta pelos 16 bytes
                    // a partir do offset 24: os 4 primeiros são um contador incrementado a cada
                    // mudança; os outros mudam ao acaso a cada mudança se há um codec. A chance
                    // de uma mudança passar sem ser detectada é ínfima.
                    let mut db_file_vers = [0u8; 16];

                    rc = file_of!(self, fd).read(&mut db_file_vers, 24);
                    if rc != SQLITE_OK {
                        if rc != SQLITE_IOERR_SHORT_READ {
                            break 'failed;
                        }
                        db_file_vers = [0u8; 16];
                    }

                    if self.db_file_vers != db_file_vers {
                        self.pager_reset();

                        // Desmapeia o arquivo: processos externos podem tê-lo truncado e
                        // estendido de volta ao tamanho original sem trava, e um mapeamento com
                        // o tamanho certo poderia não ser válido.
                        if self.b_use_fetch {
                            if let Some(fd) = self.fd.as_deref_mut() {
                                fd.unfetch(0, None);
                            }
                        }
                    }
                }

                // Se há um arquivo WAL no sistema de arquivos, abre o banco em modo WAL; senão a
                // chamada abaixo não faz nada.
                rc = self.pager_open_wal_if_present();
                debug_assert!(self.wal.is_none() || rc == SQLITE_OK);
            }

            if self.pager_use_wal() {
                debug_assert!(rc == SQLITE_OK);
                rc = self.pager_begin_read_transaction();
            }

            if !self.temp_file && self.e_state == PAGER_OPEN && rc == SQLITE_OK {
                let mut n_page: u32 = 0;
                rc = self.pager_pagecount(&mut n_page);
                if rc == SQLITE_OK {
                    self.db_size = n_page;
                }
            }
        }

        // failed:
        if rc != SQLITE_OK {
            debug_assert!(!self.mem_db);
            self.pager_unlock();
            debug_assert!(self.e_state == PAGER_OPEN);
        } else {
            self.e_state = PAGER_READER;
            self.has_held_shared_lock = true;
        }
        rc
    }

    // ------------------------------------------------------------------
    // Obtenção e liberação de páginas
    // ------------------------------------------------------------------

    /// `pagerUnlockIfUnused`: se a contagem de referências chegou a zero, reverte a transação
    /// ativa e destrava o pager. Em locking_mode=EXCLUSIVE sem nada no journal de rollback é
    /// um no-op.
    fn pager_unlock_if_unused(&mut self) {
        if self.pcache.ref_count() == 0 {
            debug_assert!(self.n_mmap_out == 0); // a página 1 nunca é mapeada
            self.pager_unlock_and_rollback();
        }
    }

    /// `getPageNormal`: o getter comum. Se a página está no cache, devolve-a; senão aloca um
    /// objeto e o preenche com dados lidos do arquivo (o pcache pode reaproveitar um objeto sem
    /// referências). O extra (`pExtra`) é zerado na primeira vez em que a página entra na
    /// memória e fica como estava se ela já estava no cache. Com o banco menor que a página
    /// pedida, ou com `PAGER_GET_NOCONTENT` e a página fora do cache, não há leitura em disco e
    /// a imagem é zerada. Com `NOCONTENT`, os bits de `pgno` em `pInJournal` e nos
    /// `pInSavepoint` dos savepoints são ligados: se a página for tornada gravável depois, o
    /// conteúdo dela não vai ao journal (poupa E/S).
    fn get_page_normal(&mut self, pgno: u32, flags: i32) -> Result<PgId, i32> {
        debug_assert!(self.err_code == SQLITE_OK);
        debug_assert!(self.e_state >= PAGER_READER);
        debug_assert!(self.assert_pager_state());
        debug_assert!(self.has_held_shared_lock);

        if pgno == 0 {
            return Err(SQLITE_CORRUPT_BKPT);
        }
        let mut p_pg: Option<PgId> = None;
        match self.get_page_normal_body(pgno, flags, &mut p_pg) {
            Ok(pg) => Ok(pg),
            Err(rc) => {
                // pager_acquire_err:
                debug_assert!(rc != SQLITE_OK);
                if let Some(pg) = p_pg {
                    self.pcache.drop_page(pg);
                }
                self.pager_unlock_if_unused();
                Err(rc)
            }
        }
    }

    /// Corpo de `getPageNormal` depois do teste de `pgno == 0`. Em erro, `p_pg` diz a página
    /// que o chamador deve descartar (`None` se não há nenhuma).
    fn get_page_normal_body(
        &mut self,
        pgno: u32,
        flags: i32,
        p_pg: &mut Option<PgId>,
    ) -> Result<PgId, i32> {
        // O `sqlite3PcacheFetchStress` do C, em passos (ver o cabeçalho do módulo).
        let base = match self.pcache.fetch(pgno, 3) {
            Some(b) => b,
            None => {
                match self.pcache.fetch_stress_prepare() {
                    StressStep::NoStress => return Err(SQLITE_NOMEM_BKPT),
                    StressStep::Spill(victim) => {
                        let rc = self.pager_stress(victim);
                        if rc != SQLITE_OK && rc != SQLITE_BUSY {
                            return Err(rc);
                        }
                    }
                    StressStep::NoVictim => {}
                }
                match self.pcache.fetch_stress_complete(pgno) {
                    Some(b) => b,
                    None => return Err(SQLITE_NOMEM_BKPT),
                }
            }
        };
        // `pPg->pPager != 0` depois do finish: a página já estava inicializada no cache.
        let was_init = self.pcache.page_is_init(base);
        let pg = self.pcache.fetch_finish(pgno, base);
        *p_pg = Some(pg);
        debug_assert!(self.pcache.page_pgno(pg) == pgno);

        let no_content = (flags & PAGER_GET_NOCONTENT) != 0;
        if was_init && !no_content {
            // O cache já tem uma cópia inicializada da página: devolve sem mais.
            debug_assert!(pgno != self.lck_pgno);
            self.a_stat[PAGER_STAT_HIT] += 1;
            return Ok(pg);
        }

        // O cache criou uma página nova e o conteúdo precisa ser inicializado. Antes, as
        // conferências: nunca se busca a página de travas.
        if pgno == self.lck_pgno {
            return Err(SQLITE_CORRUPT_BKPT);
        }

        let sz = self.page_size as usize;
        debug_assert!(self.fd.is_none() || !self.mem_db);
        if self.fd.is_none() || self.db_size < pgno || no_content {
            if pgno > self.mx_pgno {
                if pgno <= self.db_size {
                    self.pcache.release(pg);
                    *p_pg = None;
                }
                return Err(SQLITE_FULL);
            }
            if no_content {
                // Falhar ao ligar os bits nos bitvecs é benigno: só significa trabalho a mais
                // para pôr no journal uma página que não precisaria.
                if pgno <= self.db_orig_size {
                    let _ = bitvec_set(self.in_journal.as_deref_mut(), pgno);
                }
                let _ = self.add_to_savepoint_bitvecs(pgno);
            }
            self.pcache.page_data_mut(pg)[..sz].fill(0);
        } else {
            self.a_stat[PAGER_STAT_MISS] += 1;
            let rc = self.read_db_page(pg);
            if rc != SQLITE_OK {
                return Err(rc);
            }
        }
        Ok(pg)
    }

    /// `getPageMMap`: o getter com E/S mapeada em memória ligada. Uma página mapeada
    /// (somente leitura) serve para qualquer página menos a 1, sem transação de escrita aberta
    /// ou com `PAGER_GET_READONLY`, e que não esteja no WAL.
    fn get_page_mmap(&mut self, pgno: u32, flags: i32) -> Result<PgId, i32> {
        let mut i_frame: u32 = 0; // quadro a ler do WAL

        let b_mmap_ok =
            pgno > 1 && (self.e_state == PAGER_READER || (flags & PAGER_GET_READONLY) != 0);

        debug_assert!(self.b_use_fetch);

        if pgno == 0 {
            return Err(SQLITE_CORRUPT_BKPT);
        }
        debug_assert!(self.e_state >= PAGER_READER);
        debug_assert!(self.assert_pager_state());
        debug_assert!(self.has_held_shared_lock);
        debug_assert!(self.err_code == SQLITE_OK);

        if b_mmap_ok && self.pager_use_wal() {
            let (wal, fd) = wal_and_fd!(self);
            let rc = wal.find_frame(fd, pgno, &mut i_frame);
            if rc != SQLITE_OK {
                return Err(rc);
            }
        }
        if b_mmap_ok && i_frame == 0 {
            let ofst = (pgno as i64 - 1) * self.page_size;
            let amt = self.page_size as i32;
            let mut p_data: Option<Vec<u8>> = None;
            let mut rc = file_of!(self, fd).fetch(ofst, amt, &mut p_data);
            if rc == SQLITE_OK {
                if let Some(data) = p_data {
                    let mut p_pg: Option<PgId> = None;
                    if self.e_state > PAGER_READER || self.temp_file {
                        p_pg = self.lookup(pgno);
                    }
                    match p_pg {
                        None => match self.pager_acquire_map_page(pgno, data) {
                            Ok(pg) => return Ok(pg),
                            Err(e) => rc = e,
                        },
                        Some(pg) => {
                            file_of!(self, fd).unfetch(ofst, Some(data));
                            return Ok(pg);
                        }
                    }
                }
            }
            if rc != SQLITE_OK {
                return Err(rc);
            }
        }
        self.get_page_normal(pgno, flags)
    }

    /// `getPageError`: o getter de quando o pager está em estado de erro.
    fn get_page_error(&self) -> Result<PgId, i32> {
        debug_assert!(self.err_code != SQLITE_OK);
        Err(self.err_code)
    }

    /// `sqlite3PagerGet`: despacha a busca ao getter vigente (`xGet`). Devolve a página já com
    /// a referência contada.
    pub(crate) fn get(&mut self, pgno: u32, flags: i32) -> Result<PgId, i32> {
        match self.x_get {
            PagerGetter::Normal => self.get_page_normal(pgno, flags),
            PagerGetter::Error => self.get_page_error(),
            PagerGetter::MMap => self.get_page_mmap(pgno, flags),
        }
    }

    /// `sqlite3PagerLookup`: obtém a página se já está no cache, sem ler do disco; `None` se
    /// não está. A referência já vem contada. Ao contrário de `get`, nunca vai ao disco, logo
    /// nunca lida com travas nem com journals.
    pub(crate) fn lookup(&mut self, pgno: u32) -> Option<PgId> {
        debug_assert!(pgno != 0);
        let page = self.pcache.fetch(pgno, 0)?;
        debug_assert!(self.has_held_shared_lock);
        Some(self.pcache.fetch_finish(pgno, page))
    }

    /// `sqlite3PagerUnrefNotNull`: solta uma referência à página. Serve para qualquer página
    /// menos a última referência à página 1 (o btree a mantém aberta até o fim); para essa há
    /// `unref_page_one`.
    pub(crate) fn unref_not_null(&mut self, pg: PgId) {
        if self.page_flags(pg) & PGHDR_MMAP != 0 {
            debug_assert!(self.page_pgno(pg) != 1); // a página 1 nunca é mapeada
            self.release_map_page(pg);
        } else {
            self.pcache.release(pg);
        }
        // Não usar esta rotina para soltar a última referência à página 1.
        debug_assert!(self.pcache.ref_count() > 0);
    }

    /// `sqlite3PagerUnref`: como `unref_not_null`, aceitando página nula.
    pub(crate) fn unref(&mut self, pg: Option<PgId>) {
        if let Some(pg) = pg {
            self.unref_not_null(pg);
        }
    }

    /// `sqlite3PagerUnrefPageOne`: solta a referência à página 1. Se o total de referências
    /// chega a zero, larga a trava do banco.
    pub(crate) fn unref_page_one(&mut self, pg: PgId) {
        debug_assert!(self.pcache.page_pgno(pg) == 1);
        debug_assert!((self.pcache.page_flags(pg) & PGHDR_MMAP) == 0); // a página 1 nunca é mapeada
        self.pcache.release(pg);
        self.pager_unlock_if_unused();
    }

    // ------------------------------------------------------------------
    // Transação de escrita
    // ------------------------------------------------------------------

    /// `pager_open_journal`: chamada no início de toda transação de escrita, já com trava
    /// RESERVED ou EXCLUSIVE. Abre o journal e grava o cabeçalho no início; se o journal já está
    /// aberto (modo exclusivo), só grava o cabeçalho. Em qualquer caso cria `pInJournal`. Não
    /// serve para abrir um journal quente para reverter.
    fn pager_open_journal(&mut self) -> i32 {
        let mut rc = SQLITE_OK;
        let vfs = self.vfs.clone();

        debug_assert!(self.e_state == PAGER_WRITER_LOCKED);
        debug_assert!(self.assert_pager_state());
        debug_assert!(self.in_journal.is_none());

        // Já em erro: é um no-op, mas esta rotina nunca é chamada nesse estado.
        if self.err_code != 0 {
            return self.err_code;
        }

        if !self.pager_use_wal() && self.journal_mode != PAGER_JOURNALMODE_OFF {
            self.in_journal = Some(bitvec_create(self.db_size));

            // Abre o journal, se ainda não está aberto.
            if self.jfd.is_none() {
                if self.journal_mode == PAGER_JOURNALMODE_MEMORY {
                    self.jfd = Some(Box::new(mem_journal_open()));
                } else {
                    let mut flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE;
                    let n_spill;

                    if self.temp_file {
                        flags |= SQLITE_OPEN_DELETEONCLOSE | SQLITE_OPEN_TEMP_JOURNAL;
                        flags |= SQLITE_OPEN_EXCLUSIVE;
                        n_spill = SQLITE_STMTJRNL_SPILL;
                    } else {
                        flags |= SQLITE_OPEN_MAIN_JOURNAL;
                        n_spill = self.jrnl_buffer_size();
                    }

                    // Confere que o banco ainda tem o nome que tinha quando foi aberto.
                    rc = self.database_is_unmoved();
                    if rc == SQLITE_OK {
                        match journal_open(Some(vfs), opt_name(&self.z_journal), flags, n_spill) {
                            Ok(j) => self.jfd = Some(Box::new(j)),
                            Err(e) => rc = e,
                        }
                    }
                }
                debug_assert!(rc != SQLITE_OK || self.jfd.is_some());
            }

            // Grava o primeiro cabeçalho no journal.
            if rc == SQLITE_OK {
                // TODO do C: conferir se todos estes são mesmo necessários.
                self.n_rec = 0;
                self.journal_off = 0;
                self.set_super = false;
                self.journal_hdr = 0;
                rc = self.write_journal_hdr();
            }
        }

        if rc != SQLITE_OK {
            self.in_journal = None;
            self.journal_off = 0;
        } else {
            debug_assert!(self.e_state == PAGER_WRITER_LOCKED);
            self.e_state = PAGER_WRITER_CACHEMOD;
        }

        rc
    }

    /// `sqlite3PagerBegin`: começa uma transação de escrita. Se já há uma aberta é um no-op.
    /// Com `ex_flag` falso pega ao menos uma trava RESERVED; com `ex_flag` verdadeiro, ao menos
    /// EXCLUSIVE. Com `subj_in_memory`, um sub-journal aberto nesta transação será em memória;
    /// sem ele, será em memória só se o banco é em memória, senão um arquivo temporário.
    pub(crate) fn begin(&mut self, ex_flag: bool, subj_in_memory: bool) -> i32 {
        let mut rc = SQLITE_OK;

        if self.err_code != 0 {
            return self.err_code;
        }
        debug_assert!(self.e_state >= PAGER_READER && self.e_state < PAGER_ERROR);
        self.subj_in_memory = subj_in_memory;

        if self.e_state == PAGER_READER {
            debug_assert!(self.in_journal.is_none());

            if self.pager_use_wal() {
                // Com locking_mode=exclusive e sem a trava exclusiva do banco, pega-a agora.
                let need_exclusive = self.exclusive_mode && {
                    let (wal, fd) = wal_and_fd!(self);
                    wal.exclusive_mode(fd, -1) != 0
                };
                if need_exclusive {
                    rc = self.pager_lock_db(EXCLUSIVE_LOCK);
                    if rc != SQLITE_OK {
                        return rc;
                    }
                    let (wal, fd) = wal_and_fd!(self);
                    let _ = wal.exclusive_mode(fd, 1);
                }

                // Pega a trava de escrita do log. Se conseguir, passa a PAGER_RESERVED; senão
                // devolve o erro. O busy-handler não é chamado se outra conexão já tem a trava
                // de escrita: se for possível, a camada de cima o chama.
                let (wal, fd) = wal_and_fd!(self);
                rc = wal.begin_write_transaction(fd);
            } else {
                // Trava RESERVED no banco; com `ex_flag`, sobe já a EXCLUSIVE. O busy-handler
                // pode ser usado ao subir a EXCLUSIVE, não ao pegar a RESERVED.
                rc = self.pager_lock_db(RESERVED_LOCK);
                if rc == SQLITE_OK && ex_flag {
                    rc = self.pager_wait_on_lock(EXCLUSIVE_LOCK);
                }
            }

            if rc == SQLITE_OK {
                // Passa a WRITER_LOCKED. O modo WAL põe eState em WRITER_LOCKED ou CACHEMOD ao
                // abrir uma transação, mas nunca em DBMOD ou FINISHED: nesses estados a
                // reversão de savepoint pode copiar dados do sub-journal para o arquivo do
                // banco além do cache, o que seria errado no WAL.
                self.e_state = PAGER_WRITER_LOCKED;
                self.db_hint_size = self.db_size;
                self.db_file_size = self.db_size;
                self.db_orig_size = self.db_size;
                self.journal_off = 0;
            }

            debug_assert!(rc == SQLITE_OK || self.e_state == PAGER_READER);
            debug_assert!(rc != SQLITE_OK || self.e_state == PAGER_WRITER_LOCKED);
            debug_assert!(self.assert_pager_state());
        }

        rc
    }

    /// `pagerAddPageToRollbackJournal`: grava a página no fim do journal de rollback.
    fn pager_add_page_to_rollback_journal(&mut self, pg: PgId) -> i32 {
        let i_off = self.journal_off;
        let pgno = self.pcache.page_pgno(pg);
        let sz = self.page_size as usize;

        // Nunca se grava no journal a página que contém as travas do banco.
        debug_assert!(pgno != self.lck_pgno);

        debug_assert!(self.journal_hdr <= self.journal_off);
        let cksum = self.pager_cksum(self.pcache.page_data(pg));

        // Mesmo com erro de E/S ou de disco cheio ao gravar a página, o flag de precisa-de-sync
        // fica ligado: senão, ao reverter, `pager_playback_one_page` acharia que a página
        // precisa ser restaurada no arquivo, e um erro de E/S nisso poderia corromper.
        *self.pcache.page_flags_mut(pg) |= PGHDR_NEED_SYNC;

        let mut rc = write32bits(file_of!(self, jfd), i_off, pgno);
        if rc != SQLITE_OK {
            return rc;
        }
        rc = file_of!(self, jfd).write(&self.pcache.page_data(pg)[..sz], i_off + 4);
        if rc != SQLITE_OK {
            return rc;
        }
        rc = write32bits(file_of!(self, jfd), i_off + self.page_size + 4, cksum);
        if rc != SQLITE_OK {
            return rc;
        }

        self.journal_off += 8 + self.page_size;
        self.n_rec += 1;
        debug_assert!(self.in_journal.is_some());
        rc = bitvec_set(self.in_journal.as_deref_mut(), pgno);
        rc |= self.add_to_savepoint_bitvecs(pgno);
        debug_assert!(rc == SQLITE_OK || rc == crate::consts::SQLITE_NOMEM);
        rc
    }

    /// `pager_write`: torna gravável uma única página de dados. A página é gravada no journal
    /// principal ou no sub-journal, conforme o caso; se vai a algum, o bit correspondente é
    /// ligado em `pInJournal` e nos `pInSavepoint` dos savepoints cabíveis.
    fn pager_write(&mut self, pg: PgId) -> i32 {
        let mut rc = SQLITE_OK;
        let pgno = self.pcache.page_pgno(pg);

        // Só chamada com uma transação de escrita aberta; o journal pode ou não estar aberto.
        // Nunca no estado de erro.
        debug_assert!(
            self.e_state == PAGER_WRITER_LOCKED
                || self.e_state == PAGER_WRITER_CACHEMOD
                || self.e_state == PAGER_WRITER_DBMOD
        );
        debug_assert!(self.assert_pager_state());
        debug_assert!(self.err_code == 0);
        debug_assert!(!self.read_only);

        // O journal precisa estar aberto. As travas já foram obtidas por camadas acima, mas o
        // journal de rollback pode ainda não existir. Abre-o antes de `make_dirty`: se um erro
        // ocorresse depois, o pager ficaria em WRITER_LOCKED com páginas sujas no cache.
        if self.e_state == PAGER_WRITER_LOCKED {
            rc = self.pager_open_journal();
            if rc != SQLITE_OK {
                return rc;
            }
        }
        debug_assert!(self.e_state >= PAGER_WRITER_CACHEMOD);
        debug_assert!(self.assert_pager_state());

        // Marca a página que vai mudar como suja.
        self.pcache.make_dirty(pg);

        // Com journal de rollback em uso, garante que a página a mudar está nele; ou, se é uma
        // página nova além do fim do arquivo, que ela tem PGHDR_NEED_SYNC.
        debug_assert!(self.in_journal.is_some() == self.jfd.is_some());
        if self.in_journal.is_some()
            && !bitvec_test(self.in_journal.as_deref(), pgno)
        {
            debug_assert!(!self.pager_use_wal());
            if pgno <= self.db_orig_size {
                rc = self.pager_add_page_to_rollback_journal(pg);
                if rc != SQLITE_OK {
                    return rc;
                }
            } else if self.e_state != PAGER_WRITER_DBMOD {
                *self.pcache.page_flags_mut(pg) |= PGHDR_NEED_SYNC;
            }
        }

        // O bit PGHDR_DIRTY foi ligado acima, ao pôr a página na lista suja e antes de gravá-la
        // no journal. Só agora, com a página registrada no journal, liga PGHDR_WRITEABLE, que
        // indica que ela pode ser alterada com segurança.
        *self.pcache.page_flags_mut(pg) |= PGHDR_WRITEABLE;

        // Se o sub-journal está aberto e a página não está nele, grava-a lá.
        if !self.a_savepoint.is_empty() {
            rc = self.subjournal_page_if_required(pg);
        }

        // Atualiza o tamanho do banco e devolve.
        if self.db_size < pgno {
            self.db_size = pgno;
        }
        rc
    }

    /// `pagerWriteLargeSector`: variante de `write` para quando o setor é maior que a página.
    /// O SQLite supõe que todos os bytes de um setor são gravados juntos pelo hardware; então
    /// todos os bytes do setor precisam estar no journal contra uma queda no meio da escrita.
    fn pager_write_large_sector(&mut self, pg: PgId) -> i32 {
        let mut rc = SQLITE_OK;
        let mut need_sync = false; // verdadeiro se alguma página tem PGHDR_NEED_SYNC
        let pgno = self.pcache.page_pgno(pg);
        let n_page_per_sector = (self.sector_size as i64 / self.page_size) as u32;

        // Liga o bit NOSYNC de doNotSpill: não pode entrar um cabeçalho de journal entre as
        // páginas registradas por esta função.
        debug_assert!(!self.mem_db);
        debug_assert!((self.do_not_spill & SPILLFLAG_NOSYNC) == 0);
        self.do_not_spill |= SPILLFLAG_NOSYNC;

        // Supõe que o tamanho de página e o de setor são potências de dois; pg1 é a primeira
        // página do setor onde está `pg`.
        let pg1 = ((pgno - 1) & !(n_page_per_sector - 1)) + 1;

        let n_page_count = self.db_size;
        let n_page: u32 = if pgno > n_page_count {
            (pgno - pg1) + 1
        } else if (pg1 + n_page_per_sector - 1) > n_page_count {
            n_page_count + 1 - pg1
        } else {
            n_page_per_sector
        };
        debug_assert!(n_page > 0);
        debug_assert!(pg1 <= pgno);
        debug_assert!((pg1 + n_page) > pgno);

        let mut ii: u32 = 0;
        while ii < n_page && rc == SQLITE_OK {
            let p = pg1 + ii;
            if p == pgno || !bitvec_test(self.in_journal.as_deref(), p) {
                if p != self.lck_pgno {
                    match self.get(p, 0) {
                        Ok(page) => {
                            rc = self.pager_write(page);
                            if self.pcache.page_flags(page) & PGHDR_NEED_SYNC != 0 {
                                need_sync = true;
                            }
                            self.unref_not_null(page);
                        }
                        Err(e) => rc = e,
                    }
                }
            } else if let Some(page) = self.lookup(p) {
                if self.pcache.page_flags(page) & PGHDR_NEED_SYNC != 0 {
                    need_sync = true;
                }
                self.unref_not_null(page);
            }
            ii += 1;
        }

        // Se alguma das nPage páginas a partir de pg1 tem PGHDR_NEED_SYNC, todas precisam ter:
        // gravar em qualquer uma pode danificar as outras, então o journal precisa ter cópias
        // sincronizadas de todas antes de qualquer uma ir ao arquivo do banco.
        if rc == SQLITE_OK && need_sync {
            debug_assert!(!self.mem_db);
            for ii in 0..n_page {
                if let Some(page) = self.lookup(pg1 + ii) {
                    *self.pcache.page_flags_mut(page) |= PGHDR_NEED_SYNC;
                    self.unref_not_null(page);
                }
            }
        }

        debug_assert!((self.do_not_spill & SPILLFLAG_NOSYNC) != 0);
        self.do_not_spill &= !SPILLFLAG_NOSYNC;
        rc
    }

    /// `sqlite3PagerWrite`: marca uma página de dados como gravável. Precisa ser chamada antes
    /// de mudar a página, e quem chama só pode mexer nos dados se ela devolver `SQLITE_OK`.
    /// Difere de `pager_write` por tratar o caso de duas ou mais páginas caberem num setor: aí
    /// todas as páginas vizinhas já devem estar no journal ao retornar.
    pub(crate) fn write(&mut self, pg: PgId) -> i32 {
        debug_assert!((self.pcache.page_flags(pg) & PGHDR_MMAP) == 0);
        debug_assert!(self.e_state >= PAGER_WRITER_LOCKED);
        debug_assert!(self.assert_pager_state());
        let pgno = self.pcache.page_pgno(pg);
        if (self.pcache.page_flags(pg) & PGHDR_WRITEABLE) != 0 && self.db_size >= pgno {
            if !self.a_savepoint.is_empty() {
                return self.subjournal_page_if_required(pg);
            }
            SQLITE_OK
        } else if self.err_code != 0 {
            self.err_code
        } else if self.sector_size as i64 > self.page_size {
            debug_assert!(!self.temp_file);
            self.pager_write_large_sector(pg)
        } else {
            self.pager_write(pg)
        }
    }

    /// `sqlite3PagerDontWrite`: avisa que a página não precisa ir ao disco, ainda que esteja
    /// marcada como suja (por exemplo, virou folha da lista livre e o conteúdo não importa). A
    /// página fica marcada para não ser gravada. Não vale para arquivo temporário: a página
    /// pode ter estado suja no início da transação, e se a pressão de memória a tirar do cache
    /// o conteúdo precisa ir ao disco para poder ser relido se a transação for revertida.
    pub(crate) fn dont_write(&mut self, pg: PgId) {
        if !self.temp_file
            && (self.pcache.page_flags(pg) & PGHDR_DIRTY) != 0
            && self.a_savepoint.is_empty()
        {
            let flags = self.pcache.page_flags_mut(pg);
            *flags |= PGHDR_DONT_WRITE;
            *flags &= !PGHDR_WRITEABLE;
        }
    }

    // ------------------------------------------------------------------
    // Commit
    // ------------------------------------------------------------------

    /// `pager_incr_changecounter`: incrementa o change-counter do banco (4 bytes big-endian a
    /// partir do offset 24; o secundário em 92 e a versão do SQLite em 96 também mudam). Só se
    /// `changeCountDone` é falso, para não agitar a página 1 à toa. Com `is_direct_mode`
    /// (só sob `SQLITE_ENABLE_ATOMIC_WRITE`, desligado no Debian) o arquivo seria atualizado
    /// direto; aqui sempre se torna a página 1 gravável e o arquivo é atualizado no commit.
    fn pager_incr_changecounter(&mut self, is_direct_mode: bool) -> i32 {
        let mut rc = SQLITE_OK;

        debug_assert!(
            self.e_state == PAGER_WRITER_CACHEMOD || self.e_state == PAGER_WRITER_DBMOD
        );
        debug_assert!(self.assert_pager_state());
        debug_assert!(!is_direct_mode);

        if !self.change_count_done && self.db_size > 0 {
            debug_assert!(!self.temp_file && self.fd.is_some());

            // Abre a página 1 para escrita. Fora do modo direto ela está sempre no cache e a
            // busca sempre dá certo.
            match self.get(1, 0) {
                Ok(pg) => {
                    rc = self.write(pg);
                    if rc == SQLITE_OK {
                        // Atualiza o change-counter de fato.
                        self.pager_write_changecounter(pg);
                        self.change_count_done = true;
                    }
                    // Solta a referência à página.
                    self.unref_not_null(pg);
                }
                Err(e) => rc = e,
            }
        }
        rc
    }

    /// `sqlite3PagerSync`: sincroniza o arquivo do banco com o disco. Sem efeito em banco em
    /// memória e com `noSync`. `z_super` é o nome do super-journal, passado ao VFS por
    /// `SQLITE_FCNTL_SYNC`.
    pub(crate) fn sync(&mut self, z_super: Option<&[u8]>) -> i32 {
        let mut arg = match z_super {
            Some(z) => FileControlArg::Text(cstr(z).to_vec()),
            None => FileControlArg::None,
        };
        let mut rc = os_file_control(self.fd.as_deref_mut(), SQLITE_FCNTL_SYNC, &mut arg);
        if rc == SQLITE_NOTFOUND {
            rc = SQLITE_OK;
        }
        if rc == SQLITE_OK && !self.no_sync {
            debug_assert!(!self.mem_db);
            rc = os_sync(file_of!(self, fd), self.sync_flags);
        }
        rc
    }

    /// `sqlite3PagerExclusiveLock`: só em transação de escrita de rollback; no WAL é um no-op.
    /// Sem trava EXCLUSIVE no banco, tenta obtê-la. Devolve `SQLITE_OK` se já a tem, se
    /// conseguiu ou se está em WAL; senão `SQLITE_BUSY` ou um `SQLITE_IOERR_XXX`.
    pub(crate) fn exclusive_lock(&mut self) -> i32 {
        let mut rc = self.err_code;
        debug_assert!(self.assert_pager_state());
        if rc == SQLITE_OK {
            debug_assert!(
                self.e_state == PAGER_WRITER_CACHEMOD
                    || self.e_state == PAGER_WRITER_DBMOD
                    || self.e_state == PAGER_WRITER_LOCKED
            );
            debug_assert!(self.assert_pager_state());
            if !self.pager_use_wal() {
                rc = self.pager_wait_on_lock(EXCLUSIVE_LOCK);
            }
        }
        rc
    }

    /// `sqlite3PagerCommitPhaseOne`: sincroniza o arquivo do banco do pager. `z_super` é o nome
    /// de um super-journal a gravar no journal individual (`None` é transação de um banco só).
    /// Garante que o change-counter foi atualizado, o journal sincronizado (salvo com escrita
    /// atômica), todas as páginas sujas gravadas no banco, o arquivo truncado se preciso e o
    /// banco sincronizado. Resta só finalizar o journal (apagar, truncar ou zerar) ou apagar o
    /// super-journal. Com `zSuper == NULL` não sobrescreve um valor de chamada anterior. Com
    /// `no_sync` o arquivo do banco não é sincronizado; o chamador chama `sync` antes de
    /// `commit_phase_two`.
    pub(crate) fn commit_phase_one(&mut self, z_super: Option<&[u8]>, no_sync: bool) -> i32 {
        let mut rc = SQLITE_OK;

        debug_assert!(
            self.e_state == PAGER_WRITER_LOCKED
                || self.e_state == PAGER_WRITER_CACHEMOD
                || self.e_state == PAGER_WRITER_DBMOD
                || self.e_state == PAGER_ERROR
        );
        debug_assert!(self.assert_pager_state());

        // Se houve erro antes, informa o mesmo erro de novo.
        if self.err_code != 0 {
            return self.err_code;
        }

        // Sem mudanças no banco, volta cedo.
        if self.e_state < PAGER_WRITER_CACHEMOD {
            return SQLITE_OK;
        }

        debug_assert!(!self.mem_db || self.temp_file);
        debug_assert!(self.fd.is_some() || self.temp_file);
        'commit_phase_one_exit: {
            if !self.pager_flush_on_commit(true) {
                // Banco em memória, nenhuma página escrita, ou esta função já foi chamada: é
                // quase um no-op, mas um backup em andamento precisa recomeçar.
                self.backup_restart();
            } else if self.pager_use_wal() {
                let mut list = self.pcache.dirty_list();
                let mut page_one: Option<PgId> = None;
                if list.is_empty() {
                    // Precisa de ao menos uma página para o flag de commit do WAL (ticket
                    // 2d1a5c67dfc2363e44f29d9bbd57f, 2011-05-18).
                    match self.get(1, 0) {
                        Ok(p) => {
                            page_one = Some(p);
                            list.push(p);
                        }
                        Err(e) => rc = e,
                    }
                }
                debug_assert!(rc == SQLITE_OK);
                if !list.is_empty() {
                    rc = self.pager_wal_frames(list, self.db_size, true);
                }
                self.unref(page_one);
                if rc == SQLITE_OK {
                    self.pcache.clean_all();
                }
            } else {
                // O journal de rollback: atualiza o change-counter (no modo indireto).
                rc = self.pager_incr_changecounter(false);
                if rc != SQLITE_OK {
                    break 'commit_phase_one_exit;
                }

                // Grava o nome do super-journal no journal. Se já gravou um, ou se `z_super` é
                // `None` (sem super-journal), é um no-op.
                rc = self.write_super_journal(z_super);
                if rc != SQLITE_OK {
                    break 'commit_phase_one_exit;
                }

                // Sincroniza o journal e grava as páginas sujas no banco. Como a página do
                // change-counter acaba de mudar, é quase certo que o journal precisa de sync
                // aqui; em locking_mode=exclusive sob pressão de memória pode não precisar, e
                // então o xSync() redundante provavelmente vira no-op no sistema operacional.
                rc = self.sync_journal(false);
                if rc != SQLITE_OK {
                    break 'commit_phase_one_exit;
                }

                let list = self.pcache.dirty_list();
                rc = self.pager_write_pagelist(&list);
                if rc != SQLITE_OK {
                    debug_assert!(rc != crate::consts::SQLITE_IOERR_BLOCKED);
                    break 'commit_phase_one_exit;
                }
                self.pcache.clean_all();

                // Se o arquivo em disco é menor que a imagem do banco, usa `pager_truncate`
                // para fazê-lo crescer. Pode acontecer se a imagem foi estendida na transação
                // e a última página foi movida para a lista livre: a última página nunca vai
                // ao disco e o arquivo fica pequeno demais.
                if self.db_size > self.db_file_size {
                    let n_new = self.db_size - (self.db_size == self.lck_pgno) as u32;
                    debug_assert!(self.e_state == PAGER_WRITER_DBMOD);
                    rc = self.pager_truncate(n_new);
                    if rc != SQLITE_OK {
                        break 'commit_phase_one_exit;
                    }
                }

                // Por fim, sincroniza o arquivo do banco.
                if !no_sync {
                    rc = self.sync(z_super);
                }
            }
        }

        // commit_phase_one_exit:
        if rc == SQLITE_OK && !self.pager_use_wal() {
            self.e_state = PAGER_WRITER_FINISHED;
        }
        rc
    }

    /// `sqlite3PagerCommitPhaseTwo`: o banco já reflete a transação e foi sincronizado, mas o
    /// journal ainda existe (se faltar energia agora ele serve de journal quente e a transação
    /// é revertida). Finaliza o journal (apaga, trunca ou zera o início), tornando a transação
    /// irrevogável. Em erro de E/S o pager vai ao estado de erro.
    pub(crate) fn commit_phase_two(&mut self) -> i32 {
        // Não deve ser chamada após erro; se for (erro de codificação), devolve o mesmo código.
        if self.err_code != 0 {
            return self.err_code;
        }
        self.i_data_version = self.i_data_version.wrapping_add(1);

        debug_assert!(
            self.e_state == PAGER_WRITER_LOCKED
                || self.e_state == PAGER_WRITER_FINISHED
                || (self.pager_use_wal() && self.e_state == PAGER_WRITER_CACHEMOD)
        );
        debug_assert!(self.assert_pager_state());

        // Otimização: se o banco não foi alterado, o pager é exclusivo e usa journal
        // persistente, é um no-op. O início do journal tem um só cabeçalho com nRec 0; usado
        // como journal quente, reverteria 0 mudanças; não precisa zerar o cabeçalho. Em modo
        // exclusivo também não há travas a soltar.
        if self.e_state == PAGER_WRITER_LOCKED
            && self.exclusive_mode
            && self.journal_mode == crate::consts::PAGER_JOURNALMODE_PERSIST
        {
            debug_assert!(self.journal_off == self.journal_hdr_sz() || self.journal_off == 0);
            self.e_state = PAGER_READER;
            return SQLITE_OK;
        }

        let set_super = self.set_super;
        let rc = self.pager_end_transaction(set_super, true);
        self.pager_error(rc)
    }

    /// `sqlite3PagerRollback`: com transação de escrita aberta, reverte tudo e fecha a
    /// transação. Volta a `PAGER_READER` (ou `PAGER_ERROR` se algo falha). Já em estado de erro
    /// devolve `errCode` sem trabalho. No modo rollback reverte o journal e o finaliza (só se a
    /// reversão dá certo). No WAL, as entradas do cache modificadas na transação são expulsas ou
    /// relidas do banco ou do WAL, e a transação do WAL é fechada.
    pub(crate) fn rollback(&mut self) -> i32 {
        let mut rc = SQLITE_OK;

        // É um no-op em READER ou OPEN. Em estado de erro a reversão não é tentada aqui: o
        // código de erro volta ao chamador.
        debug_assert!(self.assert_pager_state());
        if self.e_state == PAGER_ERROR {
            return self.err_code;
        }
        if self.e_state <= PAGER_READER {
            return SQLITE_OK;
        }

        if self.pager_use_wal() {
            rc = self.savepoint(SAVEPOINT_ROLLBACK, -1);
            let set_super = self.set_super;
            let rc2 = self.pager_end_transaction(set_super, false);
            if rc == SQLITE_OK {
                rc = rc2;
            }
        } else if self.jfd.is_none() || self.e_state == PAGER_WRITER_LOCKED {
            let e_state = self.e_state;
            rc = self.pager_end_transaction(false, false);
            if !self.mem_db && e_state > PAGER_WRITER_LOCKED {
                // Pode acontecer com journal_mode=off. Põe o pager em estado de erro: o conteúdo
                // do cache não é confiável e os leitores ativos recebem SQLITE_ABORT.
                self.err_code = SQLITE_ABORT;
                self.e_state = PAGER_ERROR;
                self.set_getter_method();
                return rc;
            }
        } else {
            rc = self.pager_playback(false);
        }

        debug_assert!(self.e_state == PAGER_READER || rc != SQLITE_OK);
        debug_assert!(
            rc == SQLITE_OK
                || rc == SQLITE_FULL
                || rc == crate::consts::SQLITE_CORRUPT
                || rc == crate::consts::SQLITE_NOMEM
                || (rc & 0xFF) == crate::consts::SQLITE_IOERR
                || rc == SQLITE_CANTOPEN
        );

        // Erro durante o ROLLBACK: o cache não é mais confiável, então `pager_error` torna o
        // erro persistente.
        self.pager_error(rc)
    }

    // ------------------------------------------------------------------
    // Estatísticas
    // ------------------------------------------------------------------

    /// `sqlite3PagerMemUsed`: bytes de memória usados pelo pager e seu cache. A parcela
    /// `sqlite3MallocSize(pPager)` é a da alocação única do C (estrutura do pager, do cache,
    /// nomes); o `sizeof(sqlite3_file)` do VFS e o do journal não existem em Rust e ficam fora
    /// (ver as dúvidas do relatório).
    pub(crate) fn mem_used(&self) -> i32 {
        // sizeof(Pager) = 312 e sizeof(PCache) = 80 na x86-64 do C, cada um arredondado a 8.
        const SZ_PAGER: i32 = 312;
        const SZ_PCACHE: i32 = 80;
        let n_pathname = cstr(&self.z_filename).len() as i32;
        let n_names = if self.z_journal.is_empty() {
            1 + 1
        } else {
            let n_uri = self.z_filename.len() as i32 - (n_pathname + 1);
            (n_pathname + 1) + n_uri + (n_pathname + 8 + 1) + (n_pathname + 4 + 1)
        };
        let alloc = SZ_PAGER + SZ_PCACHE + 8 + 4 + n_names + 3;
        let alloc = (alloc + 7) & !7;
        let per_page_size =
            self.page_size as i32 + self.n_extra as i32 + header_size_pcache() + 5 * 8;
        per_page_size * self.pcache.page_count() + alloc + self.page_size as i32
    }

    /// `sqlite3PagerCacheStat`: soma em `pn_val` a estatística `e_stat` (`SQLITE_DBSTATUS_CACHE_HIT`,
    /// `_MISS`, `_WRITE` ou `_WRITE + 1`, que é o SPILL) e, com `reset`, a zera.
    pub(crate) fn cache_stat(&mut self, e_stat: i32, reset: bool, pn_val: &mut u64) {
        debug_assert!(
            (SQLITE_DBSTATUS_CACHE_HIT..=SQLITE_DBSTATUS_CACHE_HIT + 3).contains(&e_stat)
        );
        let idx = (e_stat - SQLITE_DBSTATUS_CACHE_HIT) as usize;
        *pn_val += self.a_stat[idx] as u64;
        if reset {
            self.a_stat[idx] = 0;
        }
    }

    /// `sqlite3PagerIsMemdb`: verdadeiro para pager em memória ou de arquivo temporário.
    pub(crate) fn is_memdb(&self) -> bool {
        self.temp_file || self.mem_vfs
    }

    // ------------------------------------------------------------------
    // Savepoints
    // ------------------------------------------------------------------

    /// `pagerOpenSavepoint`: garante que há ao menos `n_savepoint` savepoints abertos; abre os
    /// que faltam.
    fn pager_open_savepoint(&mut self, n_savepoint: usize) -> i32 {
        let n_current = self.a_savepoint.len(); // número atual de savepoints

        debug_assert!(self.e_state >= PAGER_WRITER_LOCKED);
        debug_assert!(self.assert_pager_state());
        debug_assert!(n_savepoint > n_current && self.use_journal);

        // Preenche as estruturas novas.
        for _ in n_current..n_savepoint {
            let i_offset = if self.jfd.is_some() && self.journal_off > 0 {
                self.journal_off
            } else {
                self.journal_hdr_sz()
            };
            let mut a_wal_data = [0u32; WAL_SAVEPOINT_NDATA];
            if self.pager_use_wal() {
                if let Some(wal) = self.wal.as_ref() {
                    wal.savepoint(&mut a_wal_data);
                }
            }
            self.a_savepoint.push(PagerSavepoint {
                i_offset,
                i_hdr_offset: 0,
                in_savepoint: bitvec_create(self.db_size),
                n_orig: self.db_size,
                i_sub_rec: self.n_sub_rec,
                b_truncate_on_release: true,
                a_wal_data,
            });
        }
        debug_assert!(self.a_savepoint.len() == n_savepoint);
        SQLITE_OK
    }

    /// `sqlite3PagerOpenSavepoint`.
    pub(crate) fn open_savepoint(&mut self, n_savepoint: i32) -> i32 {
        debug_assert!(self.e_state >= PAGER_WRITER_LOCKED);
        debug_assert!(self.assert_pager_state());

        if n_savepoint > self.a_savepoint.len() as i32 && self.use_journal {
            self.pager_open_savepoint(n_savepoint as usize)
        } else {
            SQLITE_OK
        }
    }

    /// `sqlite3PagerSavepoint`: reverte (`SAVEPOINT_ROLLBACK`) ou libera (`SAVEPOINT_RELEASE`)
    /// um savepoint. Não precisa ser o mais recente. `i_savepoint` 0 é o mais externo, o
    /// primeiro criado; `nSavepoint - 1` o mais recente; maior que isso é um no-op. Negativo
    /// reverte a transação atual sem terminá-la nem destravar o banco (diferente de
    /// `rollback`): só restaura o conteúdo original. Todos os savepoints de índice maior que
    /// `i_savepoint` são destruídos; num release, o próprio `i_savepoint` também.
    pub(crate) fn savepoint(&mut self, op: i32, i_savepoint: i32) -> i32 {
        let mut rc = self.err_code;

        debug_assert!(op == SAVEPOINT_RELEASE || op == SAVEPOINT_ROLLBACK);
        debug_assert!(i_savepoint >= 0 || op == SAVEPOINT_ROLLBACK);

        if rc == SQLITE_OK && i_savepoint < self.a_savepoint.len() as i32 {
            // Quantos savepoints seguem ativos depois da operação, e liberação dos recursos dos
            // que a operação destrói. O C lê o savepoint `nNew` (o liberado) depois de baixar
            // `nSavepoint`; aqui os campos são lidos antes de encurtar o vetor.
            let n_new = (i_savepoint + if op == SAVEPOINT_RELEASE { 0 } else { 1 }) as usize;
            let (rel_truncate, rel_sub_rec) = if op == SAVEPOINT_RELEASE {
                let r = &self.a_savepoint[n_new];
                (r.b_truncate_on_release, r.i_sub_rec)
            } else {
                (false, 0)
            };
            self.a_savepoint.truncate(n_new); // `sqlite3BitvecDestroy` é o drop dos bitvecs

            // Trunca o sub-journal para só ter as partes em uso.
            if op == SAVEPOINT_RELEASE {
                if rel_truncate && self.sjfd.is_some() {
                    // Só trunca se o sub-journal é em memória.
                    if is_mem_journal(&self.sjfd) {
                        let sz = (self.page_size + 4) * rel_sub_rec as i64;
                        rc = file_of!(self, sjfd).truncate(sz);
                        debug_assert!(rc == SQLITE_OK);
                    }
                    self.n_sub_rec = rel_sub_rec;
                }
            }
            // Senão é um rollback: reproduz o savepoint pedido. Num arquivo temporário o
            // journal pode não ter sido aberto ainda; então não houve mudança no banco e a
            // reprodução pode ser pulada.
            else if self.pager_use_wal() || self.jfd.is_some() {
                let sp = if n_new == 0 { None } else { Some(n_new - 1) };
                rc = self.pager_playback_savepoint(sp);
                debug_assert!(rc != crate::consts::SQLITE_DONE);
            }
        }

        rc
    }

    // ------------------------------------------------------------------
    // Mover e renumerar páginas
    // ------------------------------------------------------------------

    /// `sqlite3PagerMovepage`: move a página `pg` para a posição `pgno` do arquivo (auto-vacuum).
    /// Não pode haver referências à página que estava em `pgno` (`pPgOld`), embora ela possa
    /// estar no cache; se não estava no journal de rollback, esta rotina não a põe lá. As
    /// referências a `pg` continuam válidas; atualizar os metadados do `pExtra` é com o
    /// chamador. Exige transação ativa. Com `is_commit`, a página está sendo movida numa
    /// reorganização logo antes do commit e é garantido que não será gravada de novo.
    pub(crate) fn movepage(&mut self, pg: PgId, pgno: u32, is_commit: bool) -> i32 {
        let mut need_sync_pgno: u32 = 0; // valor antigo de pg.pgno, se precisa de sync

        debug_assert!(self.pcache.page_ref_count(pg) > 0);
        debug_assert!(
            self.e_state == PAGER_WRITER_CACHEMOD || self.e_state == PAGER_WRITER_DBMOD
        );
        debug_assert!(self.assert_pager_state());

        // Para poder reverter, um banco em memória precisa pôr no journal a página de origem.
        debug_assert!(self.temp_file || !self.mem_db);
        if self.temp_file {
            let rc = self.write(pg);
            if rc != SQLITE_OK {
                return rc;
            }
        }

        // Se a página movida está suja e o último savepoint não a salvou, grava o conteúdo atual
        // no sub-journal agora. Necessário para: BEGIN; <journal da página X, e a modifica em
        // memória>; SAVEPOINT one; <move X para Y>; ROLLBACK TO one. Sem isso não haveria como
        // restaurar o conteúdo de X.
        if (self.pcache.page_flags(pg) & PGHDR_DIRTY) != 0 {
            let rc = self.subjournal_page_if_required(pg);
            if rc != SQLITE_OK {
                return rc;
            }
        }

        // Se o journal precisa de sync antes de gravar em `pg.pgno`, guarda esse número. Com
        // `is_commit` não precisa lembrar: o chamador prometeu não gravar nela.
        if (self.pcache.page_flags(pg) & PGHDR_NEED_SYNC) != 0 && !is_commit {
            need_sync_pgno = self.pcache.page_pgno(pg);
            debug_assert!(
                self.journal_mode == PAGER_JOURNALMODE_OFF
                    || self.page_in_journal(pg)
                    || need_sync_pgno > self.db_orig_size
            );
            debug_assert!(self.pcache.page_flags(pg) & PGHDR_DIRTY != 0);
        }

        // Se o cache tem uma página com número `pgno`, tira-a da cadeia de hash. Se ela tinha
        // PGHDR_NEED_SYNC, o flag passa à página movida para lá.
        *self.pcache.page_flags_mut(pg) &= !PGHDR_NEED_SYNC;
        let p_pg_old = self.lookup(pgno);
        if let Some(old) = p_pg_old {
            if self.pcache.page_ref_count(old) > 1 {
                self.unref_not_null(old);
                return SQLITE_CORRUPT_BKPT;
            }
            let old_need_sync = self.pcache.page_flags(old) & PGHDR_NEED_SYNC;
            *self.pcache.page_flags_mut(pg) |= old_need_sync;
            if self.temp_file {
                // Não descarta páginas de banco em memória: pode ser preciso reverter depois.
                // Só tira a página do caminho.
                let new_pgno = self.db_size + 1;
                self.pcache.move_page(old, new_pgno);
            } else {
                self.pcache.drop_page(old);
            }
        }

        let orig_pgno = self.pcache.page_pgno(pg);
        self.pcache.move_page(pg, pgno);
        self.pcache.make_dirty(pg);

        // Num banco em memória, garante que a página original continua existindo, para o caso
        // de a transação precisar reverter. Usa `pPgOld`, que já está alocada.
        if self.temp_file {
            if let Some(old) = p_pg_old {
                self.pcache.move_page(old, orig_pgno);
                self.unref_not_null(old);
            }
        }

        if need_sync_pgno != 0 {
            // O journal precisa de sync antes de qualquer dado ir à página `need_sync_pgno` do
            // arquivo. Hoje não existe tal página no cache e o bit "está no journal" está
            // ligado; é preciso carregá-la e ligar PGHDR_NEED_SYNC. Se carregar falha
            // (alocação ou E/S), desliga o bit em pInJournal: senão, se a página fosse
            // carregada e gravada de novo na transação, poderia ir ao arquivo do banco antes de
            // ser sincronizada no journal. Assim pode entrar no journal duas vezes, o que não é
            // problema.
            match self.get(need_sync_pgno, 0) {
                Err(rc) => {
                    if need_sync_pgno <= self.db_orig_size {
                        debug_assert!(!self.tmp_space.is_empty());
                        bitvec_clear(self.in_journal.as_deref_mut(), need_sync_pgno);
                    }
                    return rc;
                }
                Ok(pg_hdr) => {
                    *self.pcache.page_flags_mut(pg_hdr) |= PGHDR_NEED_SYNC;
                    self.pcache.make_dirty(pg_hdr);
                    self.unref_not_null(pg_hdr);
                }
            }
        }

        SQLITE_OK
    }

    /// `sqlite3PagerRekey`: a página `pg` é suja e tem número diferente de `i_new`. Muda o
    /// número para `i_new` e o campo `flags` para `flags`.
    pub(crate) fn rekey(&mut self, pg: PgId, i_new: u32, flags: u16) {
        debug_assert!(self.pcache.page_pgno(pg) != i_new);
        *self.pcache.page_flags_mut(pg) = flags;
        self.pcache.move_page(pg, i_new);
    }

    // ------------------------------------------------------------------
    // Modos de trava e de journal
    // ------------------------------------------------------------------

    /// `sqlite3PagerLockingMode`: consulta ou muda o locking-mode. `e_mode` é
    /// `PAGER_LOCKINGMODE_QUERY` (-1), `_NORMAL` (0) ou `_EXCLUSIVE` (1); fora QUERY, o modo é
    /// mudado. Devolve o modo vigente (NORMAL ou EXCLUSIVE).
    pub(crate) fn locking_mode(&mut self, e_mode: i32) -> i32 {
        let heap = self.wal.as_ref().is_some_and(|w| w.heap_memory());
        debug_assert!(self.exclusive_mode || !heap);
        if e_mode >= 0 && !self.temp_file && !heap {
            self.exclusive_mode = e_mode != 0;
        }
        self.exclusive_mode as i32
    }

    /// `sqlite3PagerSetJournalMode`: muda o modo de journal se a mudança é permitida. Banco em
    /// memória só aceita OFF e MEMORY; temporário não aceita WAL. Devolve o modo vigente. Ao
    /// sair de TRUNCATE ou PERSIST para outro modo que não WAL, fora do modo exclusivo, apaga o
    /// arquivo de journal (só uma otimização: se não der, não é problema).
    pub(crate) fn set_journal_mode(&mut self, e_mode: i32) -> i32 {
        let e_old = self.journal_mode; // modo anterior
        let mut e_mode = e_mode;

        // Só chamada pelo opcode OP_JournalMode, cuja lógica nunca muda um temporário para WAL.
        debug_assert!(!self.temp_file || e_mode != PAGER_JOURNALMODE_WAL);

        // O modo de um banco em memória só pode mudar entre MEMORY e OFF.
        if self.mem_db {
            debug_assert!(e_old == PAGER_JOURNALMODE_MEMORY || e_old == PAGER_JOURNALMODE_OFF);
            if e_mode != PAGER_JOURNALMODE_MEMORY && e_mode != PAGER_JOURNALMODE_OFF {
                e_mode = e_old;
            }
        }

        if e_mode != e_old {
            // Muda o modo de journal.
            debug_assert!(self.e_state != PAGER_ERROR);
            self.journal_mode = e_mode;

            // De TRUNCATE ou PERSIST para qualquer outro menos WAL, fora do modo exclusivo,
            // apaga o journal.
            debug_assert!((crate::consts::PAGER_JOURNALMODE_TRUNCATE & 5) == 1);
            debug_assert!((crate::consts::PAGER_JOURNALMODE_PERSIST & 5) == 1);
            debug_assert!((crate::consts::PAGER_JOURNALMODE_DELETE & 5) == 0);
            debug_assert!((PAGER_JOURNALMODE_MEMORY & 5) == 4);
            debug_assert!((PAGER_JOURNALMODE_OFF & 5) == 0);
            debug_assert!((PAGER_JOURNALMODE_WAL & 5) == 5);

            debug_assert!(self.fd.is_some() || self.exclusive_mode);
            if !self.exclusive_mode && (e_old & 5) == 1 && (e_mode & 1) == 0 {
                // Gostaria de apagar o arquivo de journal; se não der, não é problema. Antes de
                // apagar, pega uma trava RESERVED no banco: assim o journal não é apagado
                // enquanto outro cliente o usa.
                os_close(&mut self.jfd);
                if self.e_lock >= RESERVED_LOCK {
                    self.vfs.delete(cstr(&self.z_journal), 0);
                } else {
                    let mut rc = SQLITE_OK;
                    let state = self.e_state;
                    debug_assert!(state == PAGER_OPEN || state == PAGER_READER);
                    if state == PAGER_OPEN {
                        rc = self.shared_lock();
                    }
                    if self.e_state == PAGER_READER {
                        debug_assert!(rc == SQLITE_OK);
                        rc = self.pager_lock_db(RESERVED_LOCK);
                    }
                    if rc == SQLITE_OK {
                        self.vfs.delete(cstr(&self.z_journal), 0);
                    }
                    if rc == SQLITE_OK && state == PAGER_READER {
                        self.pager_unlock_db(SHARED_LOCK);
                    } else if state == PAGER_OPEN {
                        self.pager_unlock();
                    }
                    debug_assert!(state == self.e_state);
                }
            } else if e_mode == PAGER_JOURNALMODE_OFF || e_mode == PAGER_JOURNALMODE_MEMORY {
                os_close(&mut self.jfd);
            }
        }

        // Devolve o novo modo de journal.
        self.journal_mode
    }

    /// `sqlite3PagerOkToChangeJournalMode`: verdadeiro se o estado permite mudar o modo de
    /// journal, o que só vale com o banco sem modificações.
    pub(crate) fn ok_to_change_journal_mode(&mut self) -> bool {
        debug_assert!(self.assert_pager_state());
        if self.e_state >= PAGER_WRITER_CACHEMOD {
            return false;
        }
        if self.jfd.is_some() && self.journal_off > 0 {
            return false;
        }
        true
    }

    /// `sqlite3PagerJournalSizeLimit`: consulta ou muda o limite de tamanho de journals
    /// persistentes. -1 é sem limite; menor que -1 é um no-op. Devolve o limite vigente.
    pub(crate) fn journal_size_limit(&mut self, i_limit: i64) -> i64 {
        if i_limit >= -1 {
            self.journal_size_limit = i_limit;
            if let Some(wal) = self.wal.as_mut() {
                wal.limit(i_limit);
            }
        }
        self.journal_size_limit
    }

    /// `sqlite3PagerClearCache`: salvo em banco em memória ou temporário, esvazia o cache.
    pub(crate) fn clear_cache(&mut self) {
        debug_assert!(!self.mem_db || self.temp_file);
        if !self.temp_file {
            self.pager_reset();
        }
    }

    /// `sqlite3PagerFilename`: caminho completo do banco (o `sqlite3_filename`, seguro para
    /// `sqlite3_uri_parameter` e afins). Com `null_if_mem_db`, um banco só em memória devolve
    /// a string vazia, por compatibilidade com o comportamento antigo ao informar o nome ao
    /// usuário; o btree usa `null_if_mem_db` falso para casar o cache compartilhado.
    pub(crate) fn filename(&self, null_if_mem_db: bool) -> &[u8] {
        static Z_FAKE: [u8; 4] = [0; 4];
        if null_if_mem_db && (self.mem_db || crate::memdb::is_memdb(&*self.vfs)) {
            &Z_FAKE[..]
        } else {
            self.z_filename.as_slice()
        }
    }

    /// `sqlite3PagerJrnlFile`: o arquivo de journal (o do WAL, se há WAL, senão o de rollback);
    /// `None` se não há arquivo aberto.
    pub(crate) fn jrnl_file(&mut self) -> Option<&mut (dyn VfsFile + '_)> {
        match self.wal.as_mut() {
            Some(w) => Some(w.wal_file()),
            None => match self.jfd.as_mut() {
                Some(f) => Some(&mut **f),
                None => None,
            },
        }
    }

    // ------------------------------------------------------------------
    // WAL
    // ------------------------------------------------------------------

    /// Os dois primeiros passos de `sqlite3PagerCheckpoint`: se o pager não tem WAL aberto mas o
    /// modo é WAL (banco de zero bytes que recebeu `PRAGMA journal_mode=WAL` e foi seguido de
    /// `sqlite3_wal_checkpoint()` sem transação), é preciso começar uma transação para
    /// inicializar o WAL. O C executa `PRAGMA table_list` (que começa transações em todos os
    /// bancos, inclusive os anexados); aqui o pager não alcança a conexão, então o chamador
    /// roda esse pragma quando esta função devolve verdadeiro, antes de chamar `checkpoint`.
    pub(crate) fn checkpoint_needs_wal_init(&self) -> bool {
        self.wal.is_none() && self.journal_mode == PAGER_JOURNALMODE_WAL
    }

    /// `sqlite3PagerCheckpoint`: chamada por `PRAGMA wal_checkpoint`, `wal_blocking_checkpoint`
    /// e pelas APIs `sqlite3_wal_checkpoint()` e `wal_blocking_checkpoint()`. `e_mode` é
    /// PASSIVE, FULL, RESTART ou TRUNCATE. `interrupt` é a consulta do sinal de interrupção da
    /// conexão.
    pub(crate) fn checkpoint(
        &mut self,
        interrupt: &mut dyn FnMut() -> i32,
        e_mode: i32,
        pn_log: Option<&mut i32>,
        pn_ckpt: Option<&mut i32>,
    ) -> i32 {
        let mut rc = SQLITE_OK;
        if self.wal.is_some() {
            let sz = self.page_size as usize;
            let sync_flags = self.wal_sync_flags;
            // PASSIVE não usa o busy-handler.
            let busy: Option<&mut dyn FnMut() -> i32> = match self.busy_handler.as_mut() {
                Some(b) if e_mode != SQLITE_CHECKPOINT_PASSIVE => Some(&mut **b),
                _ => None,
            };
            let (wal, fd) = wal_and_fd!(self);
            rc = wal.checkpoint(
                fd,
                interrupt,
                e_mode,
                busy,
                sync_flags,
                &mut self.tmp_space[..sz],
                pn_log,
                pn_ckpt,
            );
        }
        rc
    }

    /// `sqlite3PagerWalCallback`: o valor para o callback de `sqlite3_wal_hook` (0 sem WAL).
    pub(crate) fn wal_callback(&mut self) -> i32 {
        match self.wal.as_mut() {
            Some(w) => w.callback(),
            None => 0,
        }
    }

    /// `sqlite3PagerWalSupported`: o VFS do pager tem as primitivas que o WAL precisa?
    pub(crate) fn wal_supported(&self) -> bool {
        if self.no_lock {
            return false;
        }
        self.exclusive_mode || self.fd.as_ref().is_some_and(|f| f.i_version() >= 2)
    }

    /// `pagerExclusiveLock`: tenta a trava EXCLUSIVE no banco. Se obtiver uma PENDING em vez
    /// dela, solta-a de imediato.
    fn pager_exclusive_lock(&mut self) -> i32 {
        debug_assert!(self.e_lock >= SHARED_LOCK);
        let e_orig_lock = self.e_lock;
        let rc = self.pager_lock_db(EXCLUSIVE_LOCK);
        if rc != SQLITE_OK {
            // Se a tentativa falhou, solta a trava pendente que pode ter vindo no lugar.
            self.pager_unlock_db(e_orig_lock);
        }
        rc
    }

    /// `pagerOpenWal`: chama `Wal::open`. Com o pager em locking_mode=exclusive, pega antes a
    /// trava EXCLUSIVE do banco e o WAL usa memória do heap para o wal-index; senão usa a
    /// memória compartilhada normal.
    fn pager_open_wal(&mut self) -> i32 {
        let mut rc = SQLITE_OK;

        debug_assert!(self.wal.is_none() && !self.temp_file);
        debug_assert!(self.e_lock == SHARED_LOCK || self.e_lock == EXCLUSIVE_LOCK);

        // Já em modo exclusivo, o WAL usa memória do heap para o wal-index em vez da memória
        // compartilhada do VFS. Pega a trava exclusiva agora, antes de abrir o arquivo do WAL,
        // para garantir que é seguro.
        if self.exclusive_mode {
            rc = self.pager_exclusive_lock();
        }

        // Abre a conexão com o arquivo de log.
        if rc == SQLITE_OK {
            let vfs = self.vfs.clone();
            let opened = Wal::open(
                &vfs,
                file_of!(self, fd),
                &self.z_wal,
                self.exclusive_mode,
                self.journal_size_limit,
                wal_hooks(),
            );
            match opened {
                Ok(w) => self.wal = Some(w),
                Err(e) => rc = e,
            }
        }
        self.pager_fix_maplimit();

        rc
    }

    /// `sqlite3PagerOpenWal`: quem chama segura uma trava SHARED no banco. Se o pager é de um
    /// arquivo real (nem temporário nem em memória) e o WAL ainda não está aberto, tenta
    /// abri-lo. Em sucesso `SQLITE_OK`; em erro, ou se o VFS não tem as primitivas xShm,
    /// devolve o código de erro e `p_open` não muda. Se o pager é de temporário, ou o WAL já
    /// está aberto, `p_open` (se dado) recebe 1 e nada mais acontece.
    pub(crate) fn open_wal(&mut self, p_open: Option<&mut i32>) -> i32 {
        let mut rc = SQLITE_OK;

        debug_assert!(self.assert_pager_state());
        debug_assert!(self.e_state == PAGER_OPEN || p_open.is_some());
        debug_assert!(self.e_state == PAGER_READER || p_open.is_none());
        debug_assert!(p_open.is_some() || (!self.temp_file && self.wal.is_none()));

        if !self.temp_file && self.wal.is_none() {
            if !self.wal_supported() {
                return SQLITE_CANTOPEN;
            }

            // Fecha qualquer journal de rollback aberto antes.
            os_close(&mut self.jfd);

            rc = self.pager_open_wal();
            if rc == SQLITE_OK {
                self.journal_mode = PAGER_JOURNALMODE_WAL;
                self.e_state = PAGER_OPEN;
            }
        } else if let Some(p) = p_open {
            *p = 1;
        }

        rc
    }

    /// `sqlite3PagerCloseWal`: fecha a conexão com o log antes de passar de WAL para rollback.
    /// Antes, tenta a trava EXCLUSIVE do banco; se não conseguir, devolve `SQLITE_BUSY` e o log
    /// não é fechado. Se conseguir, a trava EXCLUSIVE não é solta ao retornar. `interrupt` é a
    /// consulta do sinal de interrupção da conexão, usada pelo checkpoint do fechamento.
    pub(crate) fn close_wal(&mut self, interrupt: &mut dyn FnMut() -> i32) -> i32 {
        let mut rc = SQLITE_OK;

        debug_assert!(self.journal_mode == PAGER_JOURNALMODE_WAL);

        // Se o log não está aberto mas existe no sistema de arquivos, pode precisar de
        // checkpoint antes de a conexão passar ao modo rollback: abre-o agora.
        if self.wal.is_none() {
            let mut logexists: i32 = 0;
            rc = self.pager_lock_db(SHARED_LOCK);
            if rc == SQLITE_OK {
                rc = self
                    .vfs
                    .access(cstr(&self.z_wal), SQLITE_ACCESS_EXISTS, &mut logexists);
            }
            if rc == SQLITE_OK && logexists != 0 {
                rc = self.pager_open_wal();
            }
        }

        // Faz checkpoint e fecha o log. Com a trava EXCLUSIVE no banco, o log e o resumo são
        // apagados.
        if rc == SQLITE_OK && self.wal.is_some() {
            rc = self.pager_exclusive_lock();
            if rc == SQLITE_OK {
                if let Some(wal) = self.wal.take() {
                    let sz = self.page_size as usize;
                    rc = wal.close(
                        file_of!(self, fd),
                        interrupt,
                        self.wal_sync_flags,
                        Some(&mut self.tmp_space[..sz]),
                    );
                }
                self.pager_fix_maplimit();
                if rc != SQLITE_OK && !self.exclusive_mode {
                    self.pager_unlock_db(SHARED_LOCK);
                }
            }
        }
        rc
    }
}

// ---------------------------------------------------------------------------
// RECONCILIAR (para o integrador)
// ---------------------------------------------------------------------------
//
// * `lib.rs` precisa de `pub mod pager_ext;` (já está lá; eu não toquei no lib.rs).
// * Acrescentado fora deste arquivo, todo pequeno: `PCache::page_is_init` (pcache.rs);
//   `pub(crate)` em `os_unix::uri_boolean` (movê-lo para `os` ou `util` quando conveniente, ele
//   é o `sqlite3_uri_boolean` público do C); `pub(crate)` em `pager::is_mem_journal` e
//   `pager::filename_blob`; consts `SQLITE_CORRUPT_BKPT`, `SQLITE_CANTOPEN_BKPT`,
//   `SQLITE_DEFAULT_SYNCHRONOUS` e `SQLITE_STMTJRNL_SPILL` em `consts/sqlite_int.rs`.
//   `SQLITE_STMTJRNL_SPILL` é o padrão de `sqlite3Config.nStmtSpill`; quando o módulo de
//   configuração global existir (`SQLITE_CONFIG_STMTJRNL_SPILL`), `open_sub_journal` e
//   `pager_open_journal` devem ler dele.
// * `wal.rs` real usado aqui: `Wal::open(&vfs, fd, wal_name, b_no_shm, mx_wal_size, hooks)` com
//   `WalHooks`; `Wal::close(self, fd, interrupt, sync_flags, buf)` consome o `Wal`;
//   `Wal::checkpoint(fd, interrupt, mode, busy, sync_flags, buf, pn_log, pn_ckpt)`;
//   `Wal::savepoint(&self, &mut [u32; 4])`; `Wal::limit`, `callback`, `heap_memory`,
//   `wal_file`, `exclusive_mode`, `begin_write_transaction`, `find_frame`. Nenhum método do
//   `Wal` que este arquivo chama diverge da API real.
// * `sqlite3PagerClose(pPager, db)` virou `close(self, Option<PagerCloseDb>)`; `sqlite3PagerCheckpoint`
//   e `sqlite3PagerCloseWal` recebem `interrupt: &mut dyn FnMut() -> i32` no lugar do `db`.
//   O btree (`sqlite3BtreeCheckpoint`) deve chamar `checkpoint_needs_wal_init()` e, se verdadeiro,
//   executar `PRAGMA table_list` na conexão antes de `checkpoint`.
// * `Pager::begin(ex_flag: bool, subj_in_memory: bool)`; `open_savepoint(n: i32)`;
//   `savepoint(op, i_savepoint)` com as constantes `SAVEPOINT_*` de `consts`.
// * `pager_open` escolhe `PCache::open` com tamanho de página padrão e deixa `set_pagesize`
//   recriar o cache com o tamanho final (no C o pcache só é aberto depois); o estado final é o
//   mesmo.
// * Os nomes `z_journal` e `z_wal` guardam `caminho-journal NUL NUL` e `caminho-wal NUL NUL`
//   (sem os parâmetros de URI do banco: no C eles apareceriam como um par chave/valor falso
//   dentro do buffer vizinho e nunca casam com nada). `z_filename` guarda
//   `caminho NUL parâmetros... NUL`, copiado do nome recebido.
//
// ---------------------------------------------------------------------------
// ADIADAS
// ---------------------------------------------------------------------------
//
// * `sqlite3_database_file_object(zName)` (chunk 012): no C recua pelo buffer do nome até achar
//   o ponteiro de volta para o `Pager`; em Rust os nomes são vetores soltos e o VFS não recebe
//   um ponteiro para o pager. Só serve a extensões de VFS externas (nenhuma do Debian a usa).
// * `sqlite3PagerWalWriteLock`, `sqlite3PagerWalDb` (`SQLITE_ENABLE_SETLK_TIMEOUT`),
//   `sqlite3PagerSnapshot*` (`SQLITE_ENABLE_SNAPSHOT`), `sqlite3PagerWalFramesize`
//   (`SQLITE_ENABLE_ZIPVFS`), `sqlite3PagerWalSystemErrno` (`SQLITE_USE_SEH`): desligadas no
//   Debian 13, não portadas.
// * `sqlite3PagerPagenumber`, `sqlite3PagerIswriteable`, `sqlite3PagerRefcount`,
//   `sqlite3PagerStats`: só `SQLITE_DEBUG`/`SQLITE_TEST` ou `NDEBUG`, somem.
