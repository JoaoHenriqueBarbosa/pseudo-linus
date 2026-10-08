//! `backup.c`: a API de backup online (`sqlite3_backup_init/step/finish/remaining/pagecount`) e
//! `sqlite3BtreeCopyFile`, que o VACUUM usa.
//!
//! Desvios do C, decorrentes do modelo v2 (CONVENTIONS.md):
//!
//! - `pDestDb`/`pSrcDb` e `pDest`/`pSrc` viram os índices de `Sqlite3Backup` em `Connection.dbs`
//!   das duas conexões, que cada função recebe por parâmetro (`&mut Connection` distintos: o caso
//!   "origem e destino iguais" não se representa, `backup_init_same_db` devolve o erro do C);
//! - os mutexes somem (a conexão é de quem a possui por `&mut`);
//! - a lista `pPager->pBackup` é um encadeamento de `BackupNode` instalado em `Pager.backup`
//!   (`PagerBackup`). O pager não alcança a conexão destino, então cada nó só registra: o
//!   `Entry` compartilhado com o `Backup` guarda `iNext` e `rc` espelhados, e uma fila das
//!   páginas de origem que mudaram depois de copiadas. A fila é aplicada no destino no começo do
//!   próximo `backup_step` e em `backup_finish`, na mesma ordem em que o C copiava no ato
//!   (`iNext` só muda dentro de `backup_step` e do recomeço, que o `Entry` aplica na hora), e o
//!   destino só é observável pelo backup, que o mantém travado em escrita;
//! - `sqlite3ResetAllSchemasOfConnection(pDestDb)` roda logo depois que o passo termina, e não no
//!   meio dele (nada do passo lê o esquema do destino);
//! - a falha de alocação não existe.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::btree::{btree_get_page_size, btree_last_page, btree_set_page_size};
use crate::btree_cursor::{
    btree_begin_trans, btree_commit_phase_one, btree_commit_phase_two, btree_new_db,
    btree_rollback, BtDb,
};
use crate::btree_types::Btree;
use crate::btree_write::{btree_set_version, btree_txn_state, btree_update_meta};
use crate::build::{find_db_name, reset_all_schemas_of_connection};
use crate::connection::{Connection, Sqlite3Backup};
use crate::consts::{
    BTREE_SCHEMA_VERSION, BTS_PAGESIZE_FIXED, PAGER_GET_READONLY, PAGER_JOURNALMODE_WAL,
    PENDING_BYTE, SQLITE_BUSY, SQLITE_DONE, SQLITE_ERROR, SQLITE_FCNTL_OVERWRITE,
    SQLITE_IOERR_NOMEM, SQLITE_LOCKED, SQLITE_NOMEM, SQLITE_NOMEM_BKPT, SQLITE_NOTFOUND,
    SQLITE_OK, SQLITE_READONLY, SQLITE_TXN_NONE, SQLITE_TXN_WRITE, TRANS_WRITE,
};
use crate::main::{error, error_with_msg, leave_mutex_and_close_zombie};
use crate::os::{FileControlArg, VfsFile};
use crate::pager::PagerBackup;
use crate::prepare::{parse_object_init, parse_object_reset};
use crate::printf::PrintfArg;
use crate::util::put4byte;
use crate::vdbeaux2::with_bt_db;

/// Quantos backups registrados num pager (`isAttached`) ainda estão vivos, em todo o processo.
/// Com zero, todo nó que sobrou em algum `Pager.backup` está morto e a cadeia pode ser descartada
/// ao registrar o próximo (o C desencadeia o nó em `sqlite3_backup_finish`; um `Box<dyn
/// PagerBackup>` não permite desligar um nó do meio).
static LIVE_BACKUPS: AtomicUsize = AtomicUsize::new(0);

/// A parte do backup que o pager enxerga: o que `sqlite3BackupUpdate` e `sqlite3BackupRestart`
/// leem e escrevem.
struct Entry {
    /// Espelho de `p->iNext`.
    i_next: AtomicU32,
    /// Espelho de `p->rc`.
    rc: AtomicI32,
    /// Falso depois de `backup_finish` (o nó some da lista do pager).
    alive: AtomicBool,
    /// Páginas de origem alteradas depois de copiadas: `(iPage, aData)`, na ordem do evento.
    events: Mutex<Vec<(u32, Vec<u8>)>>,
}

impl Entry {
    fn lock_events(&self) -> MutexGuard<'_, Vec<(u32, Vec<u8>)>> {
        self.events.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// `sqlite3_backup`: o objeto de um backup. O estado do C está em `st`; `entry` existe depois que
/// o backup foi registrado no pager de origem.
pub struct Backup {
    st: Sqlite3Backup,
    /// Falso só no backup interno de `btree_copy_file` (`pDestDb == 0`).
    has_dest_db: bool,
    entry: Option<Arc<Entry>>,
}

/// Um nó da lista de backups do pager de origem (`sqlite3_backup.pNext`).
struct BackupNode {
    entry: Arc<Entry>,
    next: Option<Box<dyn PagerBackup>>,
}

impl PagerBackup for BackupNode {
    fn restart(&mut self) {
        backup_restart(&self.entry);
        if let Some(next) = self.next.as_mut() {
            next.restart();
        }
    }

    fn update(&mut self, pgno: u32, data: &[u8]) {
        backup_update(&self.entry, pgno, data);
        if let Some(next) = self.next.as_mut() {
            next.update(pgno, data);
        }
    }
}

/// Um lado da cópia: o `Btree` e o que as rotinas de transação leem da conexão dele.
struct Side<'a, 'b> {
    bt: &'a mut Btree,
    bdb: &'a mut BtDb<'b>,
}

/// O erro que `findBtree` escreve em `pErrorDb`: código e a mensagem (argumento do `"%s"`).
type FindError = (i32, Option<Vec<u8>>);

/// `findBtree`: o índice em `db.dbs` do banco `z_db` ("main", "temp", ...). O "temp" pode ser aberto
/// aqui. `Ok(None)` é o `pBt` nulo; o erro vai para o chamador, que o escreve na conexão destino.
fn find_btree(db: &mut Connection, z_db: &[u8]) -> Result<Option<usize>, FindError> {
    let i = find_db_name(db, Some(z_db));
    if i == 1 {
        let mut parse = parse_object_init(db);
        let mut failed = None;
        if crate::build3::open_temp_database(db, &mut parse) != 0 {
            failed = Some((parse.rc, parse.z_err_msg.take()));
        }
        parse_object_reset(db, &mut parse);
        if let Some(e) = failed {
            return Err(e);
        }
    }
    if i < 0 {
        let mut msg = b"unknown database ".to_vec();
        msg.extend_from_slice(z_db);
        return Err((SQLITE_ERROR, Some(msg)));
    }
    Ok(db.dbs.get(i as usize).and_then(|slot| slot.bt.as_ref()).map(|_| i as usize))
}

/// `setDestPgsz`: tenta pôr no destino o tamanho de página da origem.
fn set_dest_pgsz(src: &Btree, dest: &mut Btree) -> i32 {
    btree_set_page_size(dest, btree_get_page_size(src), 0, 0)
}

/// `checkReadTransaction`: não pode haver transação aberta no `Btree` do destino.
fn check_read_transaction(db: &mut Connection, i_db: usize) -> i32 {
    let state = btree_txn_state(db.dbs[i_db].bt.as_ref());
    if state != SQLITE_TXN_NONE {
        error_with_msg(db, SQLITE_ERROR, b"destination database is in use", &[]);
        return SQLITE_ERROR;
    }
    SQLITE_OK
}

/// Escreve em `dest_db` o erro de `findBtree`.
fn report_find_error(dest_db: &mut Connection, (code, msg): FindError) {
    error_with_msg(dest_db, code, b"%s", &[PrintfArg::Text(msg)]);
}

/// `sqlite3_backup_init`: cria o backup que copia `z_src_db` de `src_db` para `z_dest_db` de
/// `dest_db`. Sem sucesso devolve `None` e deixa o erro em `dest_db`.
pub fn backup_init(
    dest_db: &mut Connection,
    z_dest_db: &[u8],
    src_db: &mut Connection,
    z_src_db: &[u8],
) -> Option<Backup> {
    // As duas buscas rodam sempre; um erro da segunda sobrescreve o da primeira, como no C.
    let src = find_btree(src_db, z_src_db).unwrap_or_else(|e| {
        report_find_error(dest_db, e);
        None
    });
    let dest = find_btree(dest_db, z_dest_db).unwrap_or_else(|e| {
        report_find_error(dest_db, e);
        None
    });
    let (src_idx, dest_idx) = match (src, dest) {
        (Some(s), Some(d)) => (s, d),
        _ => return None,
    };
    if check_read_transaction(dest_db, dest_idx) != SQLITE_OK {
        return None;
    }
    if let Some(bt) = src_db.dbs[src_idx].bt.as_mut() {
        bt.n_backup += 1;
    }
    Some(Backup {
        st: Sqlite3Backup {
            dest_db_index: dest_idx as i32,
            src_db_index: src_idx as i32,
            i_next: 1,
            ..Sqlite3Backup::default()
        },
        has_dest_db: true,
        entry: None,
    })
}

/// O ramo de `sqlite3_backup_init` com `pSrcDb == pDestDb`, que a posse por `&mut` não deixa
/// formar: só registra o erro.
pub fn backup_init_same_db(db: &mut Connection) -> Option<Backup> {
    error_with_msg(db, SQLITE_ERROR, b"source and destination must be distinct", &[]);
    None
}

/// `isFatalError`: todo erro é fatal num backup, menos `SQLITE_BUSY` e `SQLITE_LOCKED`.
fn is_fatal_error(rc: i32) -> bool {
    rc != SQLITE_OK && rc != SQLITE_BUSY && rc != SQLITE_LOCKED
}

/// `backupOnePage`: copia para o destino a página `i_src_pg` da origem, cujos bytes são
/// `src_data` (o tamanho de página da origem é `src_data.len()`). `src_last_page` só é lido
/// quando `b_update` é falso.
fn backup_one_page(
    dest: &mut Btree,
    src_last_page: u32,
    i_src_pg: u32,
    src_data: &[u8],
    b_update: bool,
) -> i32 {
    let n_src_pgsz = src_data.len() as i32;
    let n_dest_pgsz = btree_get_page_size(dest);
    let n_copy = n_src_pgsz.min(n_dest_pgsz) as usize;
    let i_end = i_src_pg as i64 * n_src_pgsz as i64;
    let mut rc = SQLITE_OK;

    debug_assert!(n_src_pgsz == n_dest_pgsz || !dest.bt.pager.is_memdb());

    // Uma volta por página de destino que a página de origem cobre; `i_off` é o deslocamento do
    // byte inicial da página de destino.
    let mut i_off = i_end - n_src_pgsz as i64;
    while rc == SQLITE_OK && i_off < i_end {
        let i_dest = (i_off / n_dest_pgsz as i64) as u32 + 1;
        if i_dest != dest.bt.pending_byte_page() {
            match dest.bt.pager.get(i_dest, 0) {
                Err(e) => rc = e,
                Ok(pg) => {
                    rc = dest.bt.pager.write(pg);
                    if rc == SQLITE_OK {
                        let in_off = (i_off % n_src_pgsz as i64) as usize;
                        let out_off = (i_off % n_dest_pgsz as i64) as usize;
                        // Copia os dados e invalida a análise da página no btree (`isInit` é o
                        // primeiro byte do extra da página, o truque que o pager também usa).
                        let data = dest.bt.pager.page_data_mut(pg);
                        data[out_off..out_off + n_copy]
                            .copy_from_slice(&src_data[in_off..in_off + n_copy]);
                        if i_off == 0 && !b_update {
                            put4byte(&mut data[out_off + 28..], src_last_page);
                        }
                        dest.bt.pager.page_extra_mut(pg).is_init = false;
                    }
                    dest.bt.pager.unref_not_null(pg);
                }
            }
        }
        i_off += n_dest_pgsz as i64;
    }
    rc
}

/// `backupTruncateFile`: corta o arquivo para `i_size` bytes se ele for maior.
fn backup_truncate_file(file: &mut dyn VfsFile, i_size: i64) -> i32 {
    let mut i_current = 0i64;
    let mut rc = file.file_size(&mut i_current);
    if rc == SQLITE_OK && i_current > i_size {
        rc = file.truncate(i_size);
    }
    rc
}

/// `attachBackupObject`: registra o backup no pager de origem para receber os avisos de página
/// alterada e de cache invalidado.
fn attach_backup_object(p: &mut Backup, src_bt: &mut Btree) {
    let entry = Arc::new(Entry {
        i_next: AtomicU32::new(p.st.i_next),
        rc: AtomicI32::new(p.st.rc),
        alive: AtomicBool::new(true),
        events: Mutex::new(Vec::new()),
    });
    let old = src_bt.bt.pager.backup.take();
    // Sem nenhum backup vivo no processo, o que havia na lista são só nós mortos.
    let next = if LIVE_BACKUPS.load(Ordering::SeqCst) == 0 { None } else { old };
    LIVE_BACKUPS.fetch_add(1, Ordering::SeqCst);
    src_bt.bt.pager.backup = Some(Box::new(BackupNode { entry: Arc::clone(&entry), next }));
    p.entry = Some(entry);
    p.st.is_attached = true;
}

/// Aplica no destino as páginas que a origem alterou depois de copiadas (`sqlite3BackupUpdate`,
/// que no C copiava no ato) e traz de volta o `iNext` que um recomeço possa ter zerado.
fn apply_pager_events(p: &mut Backup, dest: &mut Btree) {
    let events = match p.entry.as_ref() {
        Some(e) => {
            p.st.i_next = e.i_next.load(Ordering::SeqCst);
            std::mem::take(&mut *e.lock_events())
        }
        None => return,
    };
    for (i_page, data) in events {
        if !is_fatal_error(p.st.rc) {
            debug_assert!(p.has_dest_db && p.st.b_dest_locked);
            let rc = backup_one_page(dest, 0, i_page, &data, true);
            debug_assert!(rc != SQLITE_BUSY && rc != SQLITE_LOCKED);
            if rc != SQLITE_OK {
                p.st.rc = rc;
            }
        }
    }
}

/// O arquivo do pager do destino (`sqlite3PagerFile`), que o C afirma não ser nulo.
fn dest_file(bt: &mut Btree) -> &mut dyn VfsFile {
    match bt.bt.pager.fd.as_deref_mut() {
        Some(f) => f,
        None => unreachable!("o pager do destino tem arquivo aberto"),
    }
}

/// O corpo de `sqlite3_backup_step`: copia `n_page` páginas (todas, se negativo) da origem para o
/// destino. `reset_schemas` sobe quando o destino termina e o esquema dele precisa ser zerado.
fn step_core(
    p: &mut Backup,
    src: Side<'_, '_>,
    dest: Side<'_, '_>,
    n_page: i32,
    reset_schemas: &mut bool,
) -> i32 {
    let Side { bt: src_bt, bdb: src_bdb } = src;
    let Side { bt: dest_bt, bdb: dest_bdb } = dest;

    apply_pager_events(p, dest_bt);
    let mut rc = p.st.rc;
    if is_fatal_error(rc) {
        return rc;
    }

    let mut n_src_page: i32;
    let mut b_close_trans = false;

    // Com a origem numa transação de escrita, devolve BUSY de imediato.
    rc = if p.has_dest_db && src_bt.bt.in_transaction == TRANS_WRITE {
        SQLITE_BUSY
    } else {
        SQLITE_OK
    };

    // Sem transação de leitura na origem, abre uma aqui e a fecha antes de sair.
    if rc == SQLITE_OK && btree_txn_state(Some(&*src_bt)) == SQLITE_TXN_NONE {
        rc = btree_begin_trans(src_bt, 0, None, src_bdb);
        b_close_trans = true;
    }

    // No primeiro passo, tenta pôr no destino o tamanho de página da origem (importante no
    // ZipVFS, onde um arquivo de um tamanho de página não se cria escrevendo com outro).
    if !p.st.b_dest_locked && rc == SQLITE_OK && set_dest_pgsz(src_bt, dest_bt) == SQLITE_NOMEM {
        rc = SQLITE_NOMEM;
    }

    // Trava o destino, se ainda não está.
    if rc == SQLITE_OK && !p.st.b_dest_locked {
        let mut schema = 0i32;
        rc = btree_begin_trans(dest_bt, 2, Some(&mut schema), dest_bdb);
        if rc == SQLITE_OK {
            p.st.i_dest_schema = schema as u32;
            p.st.b_dest_locked = true;
        }
    }

    // Destino em WAL (ou em memória) com tamanho de página diferente não aceita backup.
    let pgsz_src = btree_get_page_size(src_bt);
    let pgsz_dest = btree_get_page_size(dest_bt);
    let dest_mode = dest_bt.bt.pager.journal_mode;
    if rc == SQLITE_OK
        && (dest_mode == PAGER_JOURNALMODE_WAL || dest_bt.bt.pager.is_memdb())
        && pgsz_src != pgsz_dest
    {
        rc = SQLITE_READONLY;
    }

    // Com a trava de leitura na origem, pergunta quantas páginas ela tem.
    n_src_page = btree_last_page(src_bt) as i32;
    debug_assert!(n_src_page >= 0);
    let mut ii = 0;
    while (n_page < 0 || ii < n_page) && p.st.i_next <= n_src_page as u32 && rc == SQLITE_OK {
        let i_src_pg = p.st.i_next;
        if i_src_pg != src_bt.bt.pending_byte_page() {
            match src_bt.bt.pager.get(i_src_pg, PAGER_GET_READONLY) {
                Err(e) => rc = e,
                Ok(pg) => {
                    rc = backup_one_page(
                        dest_bt,
                        n_src_page as u32,
                        i_src_pg,
                        src_bt.bt.pager.page_data(pg),
                        false,
                    );
                    src_bt.bt.pager.unref_not_null(pg);
                }
            }
        }
        p.st.i_next += 1;
        ii += 1;
    }
    if rc == SQLITE_OK {
        p.st.n_pagecount = n_src_page as u32;
        p.st.n_remaining = (n_src_page as u32 + 1).wrapping_sub(p.st.i_next);
        if p.st.i_next > n_src_page as u32 {
            rc = SQLITE_DONE;
        } else if !p.st.is_attached {
            attach_backup_object(p, src_bt);
        }
    }

    // Atualiza a versão do esquema no destino, para ela mudar de fato quando origem e destino
    // têm a mesma.
    if rc == SQLITE_DONE {
        if n_src_page == 0 {
            rc = btree_new_db(dest_bt);
            n_src_page = 1;
        }
        if rc == SQLITE_OK || rc == SQLITE_DONE {
            rc = btree_update_meta(
                dest_bt,
                BTREE_SCHEMA_VERSION as i32,
                p.st.i_dest_schema.wrapping_add(1),
            );
        }
        if rc == SQLITE_OK {
            if p.has_dest_db {
                *reset_schemas = true;
            }
            if dest_mode == PAGER_JOURNALMODE_WAL {
                rc = btree_set_version(dest_bt, 2, dest_bdb);
            }
        }
        if rc == SQLITE_OK {
            // Número final de páginas do destino. Com página de origem menor que a do destino,
            // arredonda para cima; o `OsTruncate` abaixo acerta o tamanho do arquivo, mas o
            // `PagerTruncateImage` é indispensável para o `PagerCommitPhaseOne` gravar no journal
            // as páginas além do corte antes de o arquivo encolher.
            debug_assert!(pgsz_src == btree_get_page_size(src_bt));
            debug_assert!(pgsz_dest == btree_get_page_size(dest_bt));
            let n_dest_truncate: i32;
            if pgsz_src < pgsz_dest {
                let ratio = pgsz_dest / pgsz_src;
                let mut t = (n_src_page + ratio - 1) / ratio;
                if t == dest_bt.bt.pending_byte_page() as i32 {
                    t -= 1;
                }
                n_dest_truncate = t;
            } else {
                n_dest_truncate = n_src_page * (pgsz_src / pgsz_dest);
            }
            debug_assert!(n_dest_truncate > 0);

            if pgsz_src < pgsz_dest {
                // Origem com página menor: o destino pode precisar encolher, e os dados das
                // páginas logo depois da página do byte pendente da origem podem precisar ser
                // copiados para o destino.
                let i_size = pgsz_src as i64 * n_src_page as i64;

                // Garante que tudo o que recria o banco original está no journal do destino e
                // que o journal foi sincronizado; só então o arquivo pode ser mexido à vontade.
                let n_dst_page = dest_bt.bt.pager.pagecount();
                let mut i_pg = n_dest_truncate as u32;
                while rc == SQLITE_OK && i_pg as i64 <= n_dst_page as i64 {
                    if i_pg != dest_bt.bt.pending_byte_page() {
                        match dest_bt.bt.pager.get(i_pg, 0) {
                            Err(e) => rc = e,
                            Ok(pg) => {
                                rc = dest_bt.bt.pager.write(pg);
                                dest_bt.bt.pager.unref_not_null(pg);
                            }
                        }
                    }
                    i_pg += 1;
                }
                if rc == SQLITE_OK {
                    rc = dest_bt.bt.pager.commit_phase_one(None, true);
                }

                // Grava as páginas extras e trunca o arquivo do banco.
                let i_end = (PENDING_BYTE + pgsz_dest as i64).min(i_size);
                let mut i_off = PENDING_BYTE + pgsz_src as i64;
                while rc == SQLITE_OK && i_off < i_end {
                    let i_src_pg = (i_off / pgsz_src as i64) as u32 + 1;
                    match src_bt.bt.pager.get(i_src_pg, 0) {
                        Err(e) => rc = e,
                        Ok(pg) => {
                            let data = &src_bt.bt.pager.page_data(pg)[..pgsz_src as usize];
                            rc = dest_file(dest_bt).write(data, i_off);
                            src_bt.bt.pager.unref_not_null(pg);
                        }
                    }
                    i_off += pgsz_src as i64;
                }
                if rc == SQLITE_OK {
                    rc = backup_truncate_file(dest_file(dest_bt), i_size);
                }

                // Sincroniza o arquivo do banco.
                if rc == SQLITE_OK {
                    rc = dest_bt.bt.pager.sync(None);
                }
            } else {
                dest_bt.bt.pager.truncate_image(n_dest_truncate as u32);
                rc = dest_bt.bt.pager.commit_phase_one(None, false);
            }

            // Termina o commit da transação no destino.
            if rc == SQLITE_OK {
                rc = btree_commit_phase_two(dest_bt, false, dest_bdb);
                if rc == SQLITE_OK {
                    rc = SQLITE_DONE;
                }
            }
        }
    }

    // Com `b_close_trans`, esta função abriu a leitura na origem e a fecha aqui. Confirmar uma
    // transação só de leitura não falha, então os retornos não importam.
    if b_close_trans {
        let rc1 = btree_commit_phase_one(src_bt, None, src_bdb);
        let rc2 = btree_commit_phase_two(src_bt, false, src_bdb);
        debug_assert!(rc1 | rc2 == SQLITE_OK);
    }
    if rc == SQLITE_IOERR_NOMEM {
        rc = SQLITE_NOMEM_BKPT;
    }
    p.st.rc = rc;
    if let Some(e) = p.entry.as_ref() {
        e.i_next.store(p.st.i_next, Ordering::SeqCst);
        e.rc.store(p.st.rc, Ordering::SeqCst);
    }
    rc
}

/// `sqlite3_backup_step`: copia até `n_page` páginas (todas, se negativo). Devolve `SQLITE_OK`
/// enquanto faltam páginas, `SQLITE_DONE` ao terminar, `SQLITE_BUSY`/`SQLITE_LOCKED` se tiver de
/// ser repetido, ou o erro fatal.
pub fn backup_step(
    p: &mut Backup,
    dest_db: &mut Connection,
    src_db: &mut Connection,
    n_page: i32,
) -> i32 {
    let (src_idx, dest_idx) = (p.st.src_db_index as usize, p.st.dest_db_index as usize);
    let mut reset_schemas = false;
    let rc = with_bt_db(src_db, src_idx, |src_bt, src_bdb| {
        with_bt_db(dest_db, dest_idx, |dest_bt, dest_bdb| {
            step_core(
                p,
                Side { bt: src_bt, bdb: src_bdb },
                Side { bt: dest_bt, bdb: dest_bdb },
                n_page,
                &mut reset_schemas,
            )
        })
    })
    .flatten()
    .unwrap_or(SQLITE_ERROR);
    if reset_schemas {
        reset_all_schemas_of_connection(dest_db);
    }
    rc
}

/// O corpo de `sqlite3_backup_finish` que mexe nos `Btree`: desliga o backup da origem e desfaz a
/// transação que sobrou no destino. Devolve o código do backup.
fn finish_core(p: &mut Backup, src_bt: &mut Btree, dest: Side<'_, '_>) -> i32 {
    let Side { bt: dest_bt, bdb: dest_bdb } = dest;
    apply_pager_events(p, dest_bt);

    // Desliga o backup do pager de origem.
    if p.has_dest_db {
        src_bt.n_backup -= 1;
    }
    if let Some(e) = p.entry.take() {
        e.alive.store(false, Ordering::SeqCst);
        LIVE_BACKUPS.fetch_sub(1, Ordering::SeqCst);
    }

    // Se ainda há transação aberta no destino, desfaz.
    btree_rollback(dest_bt, SQLITE_OK, false, dest_bdb);

    if p.st.rc == SQLITE_DONE {
        SQLITE_OK
    } else {
        p.st.rc
    }
}

/// `sqlite3_backup_finish`: libera tudo do backup. Devolve `SQLITE_OK` se ele terminou (ou se não
/// houve erro), e o erro fatal que o parou se houve; o mesmo código fica em `dest_db`.
pub fn backup_finish(
    p: Option<Backup>,
    dest_db: &mut Connection,
    src_db: &mut Connection,
) -> i32 {
    let mut p = match p {
        Some(p) => p,
        None => return SQLITE_OK,
    };
    let (src_idx, dest_idx) = (p.st.src_db_index as usize, p.st.dest_db_index as usize);
    let rc = with_bt_db(src_db, src_idx, |src_bt, _| {
        with_bt_db(dest_db, dest_idx, |dest_bt, dest_bdb| {
            finish_core(&mut p, src_bt, Side { bt: dest_bt, bdb: dest_bdb })
        })
    })
    .flatten()
    .unwrap_or(SQLITE_ERROR);

    error(dest_db, rc);

    // Uma conexão que já passou por `close` e só esperava o backup acaba de fechar aqui.
    leave_mutex_and_close_zombie(dest_db);
    leave_mutex_and_close_zombie(src_db);
    rc
}

/// `sqlite3_backup_remaining`: as páginas que faltam, como estavam no último `backup_step`.
pub fn backup_remaining(p: &Backup) -> i32 {
    p.st.n_remaining as i32
}

/// `sqlite3_backup_pagecount`: o total de páginas da origem, como estava no último `backup_step`.
pub fn backup_pagecount(p: &Backup) -> i32 {
    p.st.n_pagecount as i32
}

/// `backupUpdate`: depois que a página `i_page` da origem mudou, se ela já tinha sido copiada o
/// dado do destino ficou inválido e precisa ser refeito antes de o backup terminar. Aqui só
/// registra o evento; `apply_pager_events` o aplica no destino.
fn backup_update(entry: &Entry, i_page: u32, a_data: &[u8]) {
    if entry.alive.load(Ordering::SeqCst)
        && !is_fatal_error(entry.rc.load(Ordering::SeqCst))
        && i_page < entry.i_next.load(Ordering::SeqCst)
    {
        entry.lock_events().push((i_page, a_data.to_vec()));
    }
}

/// `sqlite3BackupRestart`: o pager viu o banco ser alterado por outra conexão, e não há como
/// saber quais páginas já copiadas continuam válidas: o processo recomeça.
fn backup_restart(entry: &Entry) {
    if entry.alive.load(Ordering::SeqCst) {
        entry.i_next.store(1, Ordering::SeqCst);
    }
}

/// `sqlite3BtreeCopyFile`: copia todo o conteúdo de `p_from` para `p_to`. As duas árvores têm
/// transação aberta (a de `p_to` de escrita). O arquivo de `p_to` pode encolher. Se algo falha, a
/// transação de `p_to` é desfeita; se dá certo, ela é confirmada antes de voltar.
pub fn btree_copy_file(p_to: &mut Btree, p_from: &mut Btree) -> i32 {
    debug_assert!(btree_txn_state(Some(&*p_to)) == SQLITE_TXN_WRITE);
    let mut rc = SQLITE_OK;
    'copy_finished: {
        if let Some(fd) = p_to.bt.pager.fd.as_deref_mut() {
            let n_byte = btree_get_page_size(p_from) as i64 * btree_last_page(p_from) as i64;
            rc = fd.file_control(SQLITE_FCNTL_OVERWRITE, &mut FileControlArg::Int64(n_byte));
            if rc == SQLITE_NOTFOUND {
                rc = SQLITE_OK;
            }
            if rc != SQLITE_OK {
                break 'copy_finished;
            }
        }

        // O backup interno não tem conexão destino (`pDestDb == 0`): é o que `backup_step` e
        // `finish_core` usam para saber que quem chama é esta função e não o usuário.
        let mut b = Backup {
            st: Sqlite3Backup { i_next: 1, ..Sqlite3Backup::default() },
            has_dest_db: false,
            entry: None,
        };

        // 0x7FFFFFFF é o limite duro de páginas de um arquivo: pedir tantas garante que a cópia
        // termina numa chamada só, com `b.st.rc` em `SQLITE_DONE` ou num erro.
        let (mut src_bdb, mut dest_bdb) = (BtDb::default(), BtDb::default());
        let mut reset_schemas = false;
        step_core(
            &mut b,
            Side { bt: &mut *p_from, bdb: &mut src_bdb },
            Side { bt: &mut *p_to, bdb: &mut dest_bdb },
            0x7FFF_FFFF,
            &mut reset_schemas,
        );
        debug_assert!(b.st.rc != SQLITE_OK && !reset_schemas);
        rc = finish_core(&mut b, p_from, Side { bt: &mut *p_to, bdb: &mut dest_bdb });
        if rc == SQLITE_OK {
            p_to.bt.bts_flags &= !BTS_PAGESIZE_FIXED;
        } else {
            p_to.bt.pager.clear_cache();
        }
        debug_assert!(btree_txn_state(Some(&*p_to)) != SQLITE_TXN_WRITE);
    }
    rc
}
