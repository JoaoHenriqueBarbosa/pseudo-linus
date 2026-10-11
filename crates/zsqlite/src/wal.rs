//! Log de escrita antecipada, o WAL do `journal_mode=WAL` (wal.c do SQLite 3.46.1).
//!
//! Formato do arquivo `-wal`: cabeçalho de 32 bytes seguido de quadros (24 bytes de
//! cabeçalho mais uma página). O índice do WAL (o `-shm`) é memória compartilhada: no C
//! é um vetor de ponteiros `apWiData`; aqui cada página de 32 KB do índice é uma
//! [`WiPage`], ou um buffer no heap (modo `WAL_HEAPMEMORY_MODE`, e o índice de
//! reconstrução do modo `bShmUnreliable`), ou uma [`ShmRegion`] devolvida por
//! `VfsFile::shm_map` e lida e escrita só por `shm_read` e `shm_write`. Como a
//! memória compartilhada pertence ao arquivo do banco, quase todo método que toca o
//! índice recebe `db_fd: &mut dyn VfsFile` (o `pDbFd` do C, que não é guardado).
//!
//! Escolhas de modelagem (registradas para o pager):
//!
//! * Callbacks do C (`xBusyHandler`+`pBusyArg`, `xUndo`+`pUndoCtx`) viram
//!   `&mut dyn FnMut`. A consulta a `db->u1.isInterrupted` do checkpoint vira o
//!   callback `interrupt`, que devolve 0 (nada) ou o código a propagar
//!   (`SQLITE_INTERRUPT` ou `SQLITE_NOMEM`).
//! * `sqlite3_randomness` e `sqlite3_log` são globais no C; enquanto o módulo global
//!   não existe, entram em [`WalHooks`] no `Wal::open`.
//! * Acesso à memória compartilhada pode falhar aqui (o C desreferencia ponteiro e,
//!   sem SEH, não falha). Onde o C tem função `void` que mexe no índice, a tradução
//!   propaga o código do `shm_read`/`shm_write`.
//! * Fora de escopo por opção do Debian: `SQLITE_ENABLE_SNAPSHOT`,
//!   `SQLITE_ENABLE_SETLK_TIMEOUT` (`sqlite3WalWriteLock`, `sqlite3WalDb`),
//!   `SQLITE_ENABLE_ZIPVFS`, `SQLITE_USE_SEH` e os ramos de teste/depuração.

use crate::consts::{
    SQLITE_BIGENDIAN, SQLITE_BUSY, SQLITE_BUSY_RECOVERY, SQLITE_BUSY_SNAPSHOT, SQLITE_CANTOPEN,
    SQLITE_CHECKPOINT_PASSIVE, SQLITE_CHECKPOINT_RESTART, SQLITE_CHECKPOINT_TRUNCATE,
    SQLITE_CORRUPT, SQLITE_ERROR, SQLITE_FCNTL_CKPT_DONE, SQLITE_FCNTL_CKPT_START,
    SQLITE_FCNTL_PERSIST_WAL, SQLITE_FCNTL_SIZE_HINT, SQLITE_IOCAP_POWERSAFE_OVERWRITE,
    SQLITE_IOCAP_SEQUENTIAL, SQLITE_LOCK_EXCLUSIVE, SQLITE_MAX_PAGE_SIZE,
    SQLITE_NOTICE_RECOVER_WAL, SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_READONLY,
    SQLITE_OPEN_READWRITE, SQLITE_OPEN_WAL, SQLITE_PROTOCOL, SQLITE_READONLY,
    SQLITE_READONLY_CANTINIT, SQLITE_READONLY_RECOVERY, SQLITE_SHM_EXCLUSIVE, SQLITE_SHM_LOCK,
    SQLITE_SHM_NLOCK, SQLITE_SHM_SHARED, SQLITE_SHM_UNLOCK, WAL_SAVEPOINT_NDATA,
};
use crate::os::{os_file_control_hint, os_open, os_sync, FileControlArg, ShmRegion, VfsFile, VfsRef};
use crate::util::{get4byte, put4byte};

/// `SQLITE_CORRUPT_BKPT` e `SQLITE_CANTOPEN_BKPT` do C só acrescentam um `sqlite3_log`
/// de depuração ao código; o valor devolvido é o mesmo.
const SQLITE_CORRUPT_BKPT: i32 = SQLITE_CORRUPT;
const SQLITE_CANTOPEN_BKPT: i32 = SQLITE_CANTOPEN;

/// `Err(rc)` quando o código não é `SQLITE_OK` (ponte entre o estilo do C e `?`).
fn rc_result(rc: i32) -> Result<(), i32> {
    if rc == SQLITE_OK {
        Ok(())
    } else {
        Err(rc)
    }
}

/// Devolve o valor de um `Result<T, i32>` ou retorna o código de erro (em função que
/// devolve `i32`).
macro_rules! tri {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(rc) => return rc,
        }
    };
}

// ---------------------------------------------------------------------------
// Constantes de wal.c e de wal.h/pager.h usadas aqui.
// ---------------------------------------------------------------------------

/// Única versão do formato do WAL e do índice que esta biblioteca interpreta.
const WAL_MAX_VERSION: u32 = 3007000;
const WALINDEX_MAX_VERSION: u32 = 3007000;

/// Índices dos bytes de trava do índice.
const WAL_WRITE_LOCK: i32 = 0;
const WAL_ALL_BUT_WRITE: i32 = 1;
const WAL_CKPT_LOCK: i32 = 1;
const WAL_RECOVER_LOCK: i32 = 2;
/// Número de travas de leitor.
const WAL_NREADER: i32 = SQLITE_SHM_NLOCK - 3;

/// `WAL_READ_LOCK(I)`.
#[inline]
const fn wal_read_lock(i: i32) -> i32 {
    3 + i
}

const READMARK_NOT_USED: u32 = 0xffffffff;

/// Tamanho do cabeçalho completo do índice (duas cópias de `WalIndexHdr` e o `WalCkptInfo`).
const WALINDEX_HDR_SIZE: usize = 136;
/// Tamanho do cabeçalho de cada quadro no arquivo `-wal`.
const WAL_FRAME_HDRSIZE: usize = 24;
/// Tamanho do cabeçalho do arquivo `-wal`.
const WAL_HDRSIZE: usize = 32;
const WAL_MAGIC: u32 = 0x377f0682;

/// `Wal.exclusiveMode`.
const WAL_NORMAL_MODE: u8 = 0;
const WAL_EXCLUSIVE_MODE: u8 = 1;
const WAL_HEAPMEMORY_MODE: u8 = 2;

/// `Wal.readOnly`.
const WAL_RDONLY: u8 = 1;
const WAL_SHM_RDONLY: u8 = 2;

/// Parâmetros das tabelas hash do índice (alterar quebra o formato do `-shm`).
const HASHTABLE_NPAGE: u32 = 4096;
const HASHTABLE_HASH_1: u32 = 383;
const HASHTABLE_NSLOT: u32 = HASHTABLE_NPAGE * 2;
const HASHTABLE_NPAGE_ONE: u32 = HASHTABLE_NPAGE - (WALINDEX_HDR_SIZE / 4) as u32;
/// Tamanho de uma página do índice: tabela hash (u16) mais mapa de páginas (u32).
const WALINDEX_PGSZ: usize = 2 * HASHTABLE_NSLOT as usize + 4 * HASHTABLE_NPAGE as usize;
/// Deslocamento em bytes de `aHash` dentro de uma página do índice.
const HASH_OFF: usize = 4 * HASHTABLE_NPAGE as usize;

/// Deslocamentos dentro do `WalCkptInfo` (que começa no byte 96 da página 0).
const CKPT_BACKFILL_OFF: usize = 96;
const CKPT_READMARK_OFF: usize = 100;
const CKPT_BACKFILL_ATTEMPTED_OFF: usize = 128;

/// Valor devolvido por `wal_try_begin_read` quando é preciso tentar de novo.
const WAL_RETRY: i32 = -1;
const WAL_RETRY_PROTOCOL_LIMIT: i32 = 100;
/// Sem `SQLITE_ENABLE_SETLK_TIMEOUT` a máscara é zero.
const WAL_RETRY_BLOCKED_MASK: i32 = 0;

/// `MAX_SECTOR_SIZE` do pager.c.
const MAX_SECTOR_SIZE: i32 = 0x10000;

/// `WAL_SYNC_FLAGS(X)` do pager.h: flags de sync das gravações de commit.
#[inline]
pub(crate) fn wal_sync_flags(x: i32) -> i32 {
    x & 0x03
}

/// `CKPT_SYNC_FLAGS(X)` do pager.h: flags de sync das operações de checkpoint.
#[inline]
pub(crate) fn ckpt_sync_flags(x: i32) -> i32 {
    (x >> 2) & 0x03
}

/// `sqlite3SectorSize` (definida em pager.c no C): tamanho de setor limitado a
/// `[32, MAX_SECTOR_SIZE]` (abaixo de 32 vira 512).
pub(crate) fn sector_size(f: &mut dyn VfsFile) -> i32 {
    let ret = f.sector_size();
    if ret < 32 {
        512
    } else if ret > MAX_SECTOR_SIZE {
        MAX_SECTOR_SIZE
    } else {
        ret
    }
}

/// Funções globais de que o WAL precisa (`sqlite3_randomness` e `sqlite3_log`).
#[derive(Clone, Copy)]
pub struct WalHooks {
    /// `sqlite3_randomness`: enche o buffer com bytes pseudoaleatórios.
    pub randomness: fn(&mut [u8]),
    /// `sqlite3_log(código, mensagem)`.
    pub log: fn(i32, &[u8]),
}

/// Uma página suja entregue a `Wal::frames` (o `PgHdr` da lista `pList`; a ordem do
/// slice é a ordem da lista, e o último elemento é o que tem `pDirty == 0`).
pub struct WalPageRef<'a> {
    /// Número da página no banco.
    pub pgno: u32,
    /// Conteúdo da página (pelo menos `sz_page` bytes).
    pub data: &'a [u8],
    /// `PgHdr.flags`. O WAL só usa `PGHDR_WAL_APPEND` e o controla internamente, então
    /// o valor de entrada é ignorado.
    pub flags: u16,
}

/// Cópia do cabeçalho do índice (`WalIndexHdr`, 48 bytes, ordem de bytes nativa).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct WalIndexHdr {
    pub i_version: u32,
    pub unused: u32,
    pub i_change: u32,
    pub is_init: u8,
    pub big_end_cksum: u8,
    pub sz_page: u16,
    pub mx_frame: u32,
    pub n_page: u32,
    pub a_frame_cksum: [u32; 2],
    pub a_salt: [u32; 2],
    pub a_cksum: [u32; 2],
}

/// Lê um `u32` nativo de `b[off..off + 4]`.
#[inline]
fn ne32(b: &[u8], off: usize) -> u32 {
    u32::from_ne_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

impl WalIndexHdr {
    /// Tamanho em bytes (`sizeof(WalIndexHdr)`).
    const SIZE: usize = 48;
    /// `offsetof(WalIndexHdr, aCksum)`.
    const CKSUM_OFF: usize = 40;

    fn to_bytes(&self) -> [u8; 48] {
        let mut b = [0u8; 48];
        b[0..4].copy_from_slice(&self.i_version.to_ne_bytes());
        b[4..8].copy_from_slice(&self.unused.to_ne_bytes());
        b[8..12].copy_from_slice(&self.i_change.to_ne_bytes());
        b[12] = self.is_init;
        b[13] = self.big_end_cksum;
        b[14..16].copy_from_slice(&self.sz_page.to_ne_bytes());
        b[16..20].copy_from_slice(&self.mx_frame.to_ne_bytes());
        b[20..24].copy_from_slice(&self.n_page.to_ne_bytes());
        b[24..28].copy_from_slice(&self.a_frame_cksum[0].to_ne_bytes());
        b[28..32].copy_from_slice(&self.a_frame_cksum[1].to_ne_bytes());
        b[32..36].copy_from_slice(&self.a_salt[0].to_ne_bytes());
        b[36..40].copy_from_slice(&self.a_salt[1].to_ne_bytes());
        b[40..44].copy_from_slice(&self.a_cksum[0].to_ne_bytes());
        b[44..48].copy_from_slice(&self.a_cksum[1].to_ne_bytes());
        b
    }

    fn from_bytes(b: &[u8; 48]) -> WalIndexHdr {
        WalIndexHdr {
            i_version: ne32(b, 0),
            unused: ne32(b, 4),
            i_change: ne32(b, 8),
            is_init: b[12],
            big_end_cksum: b[13],
            sz_page: u16::from_ne_bytes([b[14], b[15]]),
            mx_frame: ne32(b, 16),
            n_page: ne32(b, 20),
            a_frame_cksum: [ne32(b, 24), ne32(b, 28)],
            a_salt: [ne32(b, 32), ne32(b, 36)],
            a_cksum: [ne32(b, 40), ne32(b, 44)],
        }
    }

    /// Os 8 bytes de sal como estão na memória (o `memcpy(.., hdr.aSalt, 8)` do C).
    fn salt_bytes(&self) -> [u8; 8] {
        let mut s = [0u8; 8];
        s[0..4].copy_from_slice(&self.a_salt[0].to_ne_bytes());
        s[4..8].copy_from_slice(&self.a_salt[1].to_ne_bytes());
        s
    }
}

/// Uma página de 32 KB do índice do WAL.
enum WiPage {
    /// Memória do heap (modo heap, índice de reconstrução de `bShmUnreliable`, e o
    /// buffer privado da recuperação).
    Heap(Vec<u8>),
    /// Região da memória compartilhada do VFS.
    Shm(ShmRegion),
}

/// Localização da tabela hash e do mapa de páginas dentro de uma página do índice
/// (`WalHashLoc`; os ponteiros do C viram deslocamentos em bytes).
struct WalHashLoc {
    /// Página do índice onde a tabela mora.
    pg: usize,
    /// Deslocamento de `aHash`.
    hash_off: usize,
    /// Deslocamento de `aPgno[0]`.
    pgno_off: usize,
    /// Um a menos que o número do primeiro quadro indexado.
    i_zero: u32,
}

/// Segmento do iterador (`struct WalSegment`): cópia ordenada de uma página do índice.
#[derive(Default)]
struct WalSegment {
    /// Próxima posição de `a_index` ainda não devolvida.
    i_next: usize,
    /// `i0, i1, i2...` tais que `a_pgno[iN]` cresce.
    a_index: Vec<u16>,
    /// Números de página do segmento (cópia de `aPgno`).
    a_pgno: Vec<u32>,
    /// Número de entradas válidas de `a_index`.
    n_entry: usize,
    /// Número do quadro associado a `a_pgno[0]`.
    i_zero: u32,
}

/// Iterador sobre todos os quadros do WAL em ordem de página (`WalIterator`).
struct WalIterator {
    /// Último resultado devolvido.
    i_prior: u32,
    a_segment: Vec<WalSegment>,
}

/// Estado do escritor de quadros (`WalWriter`, sem o ponteiro para o `Wal` e o arquivo).
struct WalWriter {
    /// Faz fsync ao cruzar este deslocamento.
    i_sync_point: i64,
    /// Flags do fsync.
    sync_flags: i32,
    /// Tamanho de uma página.
    sz_page: usize,
}

/// Conexão com um arquivo de log (`struct Wal`).
pub struct Wal {
    /// O VFS que abriu o arquivo do WAL.
    vfs: VfsRef,
    /// Arquivo `-wal`.
    wal_fd: Box<dyn VfsFile>,
    /// Valor para o callback do log (ou 0).
    i_callback: u32,
    /// Trunca o WAL a este tamanho no reinício.
    mx_wal_size: i64,
    /// Conteúdo do índice em memória (`apWiData`; `nWiData` é `len()`).
    wi_data: Vec<Option<WiPage>>,
    /// Tamanho de página do banco.
    sz_page: u32,
    /// Trava de leitura em uso, ou -1.
    read_lock: i16,
    /// `WAL_NORMAL_MODE`, `WAL_EXCLUSIVE_MODE` ou `WAL_HEAPMEMORY_MODE`.
    exclusive_mode: u8,
    /// Em transação de escrita.
    write_lock: bool,
    /// Segura a trava de checkpoint.
    ckpt_lock: bool,
    /// `WAL_RDWR` (0), `WAL_RDONLY` ou `WAL_SHM_RDONLY` (bits).
    read_only: u8,
    /// Truncar o WAL no commit.
    truncate_on_commit: bool,
    /// Fazer fsync do cabeçalho do WAL.
    sync_header: bool,
    /// Preencher transações até o limite do setor.
    pad_to_sector_boundary: bool,
    /// A memória compartilhada é só leitura e não confiável.
    b_shm_unreliable: bool,
    /// Cabeçalho do índice para a transação corrente.
    hdr: WalIndexHdr,
    /// Ignora quadros anteriores a este.
    min_frame: u32,
    /// No commit, recalcula checksums a partir daqui.
    i_re_cksum: u32,
    /// Nome do arquivo do WAL (`sqlite3_filename`: caminho, NUL e pares de URI).
    wal_name: Vec<u8>,
    /// Contador de checkpoints do cabeçalho do WAL.
    n_ckpt: u32,
    /// `sqlite3_randomness` e `sqlite3_log`.
    hooks: WalHooks,
}

/// `walFrameOffset`: deslocamento do cabeçalho do quadro `i_frame` no arquivo.
#[inline]
fn wal_frame_offset(i_frame: u32, sz_page: i32) -> i64 {
    WAL_HDRSIZE as i64 + (i_frame.wrapping_sub(1) as i64) * ((sz_page as usize + WAL_FRAME_HDRSIZE) as i64)
}

/// `walChecksumBytes`: gera ou estende um checksum de 8 bytes sobre `a[..n_byte]`.
/// `native_cksum` falso lê as palavras na ordem de bytes oposta à nativa.
fn wal_checksum_bytes(native_cksum: bool, a: &[u8], n_byte: usize, a_in: Option<[u32; 2]>) -> [u32; 2] {
    let (mut s1, mut s2) = match a_in {
        Some(v) => (v[0], v[1]),
        None => (0u32, 0u32),
    };
    debug_assert!(n_byte >= 8 && n_byte % 8 == 0 && n_byte <= 65536);
    let mut i = 0usize;
    while i < n_byte {
        let mut x0 = ne32(a, i);
        let mut x1 = ne32(a, i + 4);
        if !native_cksum {
            x0 = x0.swap_bytes();
            x1 = x1.swap_bytes();
        }
        s1 = s1.wrapping_add(x0).wrapping_add(s2);
        s2 = s2.wrapping_add(x1).wrapping_add(s1);
        i += 8;
    }
    [s1, s2]
}

/// `walHash`: hash de um número de página, entre 0 e `HASHTABLE_NSLOT - 1`.
#[inline]
fn wal_hash(i_page: u32) -> usize {
    debug_assert!(i_page > 0);
    (i_page.wrapping_mul(HASHTABLE_HASH_1) & (HASHTABLE_NSLOT - 1)) as usize
}

/// `walNextHash`: próxima posição depois de uma colisão.
#[inline]
fn wal_next_hash(i_prior_hash: usize) -> usize {
    (i_prior_hash + 1) & (HASHTABLE_NSLOT as usize - 1)
}

/// `walFramePage`: página do índice cuja tabela hash indexa o quadro `i_frame`.
#[inline]
fn wal_frame_page(i_frame: u32) -> u32 {
    i_frame
        .wrapping_add(HASHTABLE_NPAGE)
        .wrapping_sub(HASHTABLE_NPAGE_ONE)
        .wrapping_sub(1)
        / HASHTABLE_NPAGE
}

/// `walMerge`: funde duas listas ordenadas de índices. `left` e `right` são posições
/// de início dentro de `list`; o resultado fica a partir de `left` e `right` passa a
/// apontar para ele.
fn wal_merge(
    content: &[u32],
    list: &mut [u16],
    left: usize,
    n_left: usize,
    right: &mut usize,
    n_right: &mut usize,
    tmp: &mut [u16],
) {
    let mut i_left = 0usize;
    let mut i_right = 0usize;
    let mut i_out = 0usize;
    let n_r = *n_right;
    let a_right = *right;
    debug_assert!(n_left > 0 && n_r > 0);
    while i_right < n_r || i_left < n_left {
        let logpage: u16;
        if i_left < n_left
            && (i_right >= n_r
                || content[list[left + i_left] as usize] < content[list[a_right + i_right] as usize])
        {
            logpage = list[left + i_left];
            i_left += 1;
        } else {
            logpage = list[a_right + i_right];
            i_right += 1;
        }
        let dbpage = content[logpage as usize];
        tmp[i_out] = logpage;
        i_out += 1;
        if i_left < n_left && content[list[left + i_left] as usize] == dbpage {
            i_left += 1;
        }
    }
    *right = left;
    *n_right = i_out;
    list[left..left + i_out].copy_from_slice(&tmp[..i_out]);
}

/// `walMergesort`: ordena `list` por `content[list[i]]`, removendo chaves repetidas
/// (fica o maior valor de `list`).
fn wal_mergesort(content: &[u32], buffer: &mut [u16], list: &mut [u16], pn_list: &mut usize) {
    let n_list = *pn_list;
    let mut n_merge = 0usize;
    let mut a_merge = 0usize;
    // (n_list, a_list) de cada sublista.
    let mut a_sub = [(0usize, 0usize); 13];
    let mut i_sub = 0usize;
    debug_assert!(n_list <= HASHTABLE_NPAGE as usize);

    for i_list in 0..n_list {
        n_merge = 1;
        a_merge = i_list;
        i_sub = 0;
        while (i_list & (1usize << i_sub)) != 0 {
            let (p_n, p_a) = a_sub[i_sub];
            wal_merge(content, list, p_a, p_n, &mut a_merge, &mut n_merge, buffer);
            i_sub += 1;
        }
        a_sub[i_sub] = (n_merge, a_merge);
    }

    i_sub += 1;
    while i_sub < a_sub.len() {
        if (n_list & (1usize << i_sub)) != 0 {
            let (p_n, p_a) = a_sub[i_sub];
            wal_merge(content, list, p_a, p_n, &mut a_merge, &mut n_merge, buffer);
        }
        i_sub += 1;
    }
    debug_assert!(n_list == 0 || a_merge == 0);
    *pn_list = n_merge;
}

/// `walIteratorNext`: menor página ainda não devolvida; devolve (página, quadro onde
/// ela foi escrita por último) ou `None` quando acabou (o `1` do C).
fn wal_iterator_next(p: &mut WalIterator) -> Option<(u32, u32)> {
    let i_min = p.i_prior;
    let mut i_ret = 0xFFFFFFFFu32;
    let mut i_frame = 0u32;
    debug_assert!(i_min < 0xffffffff);
    for i in (0..p.a_segment.len()).rev() {
        let seg = &mut p.a_segment[i];
        while seg.i_next < seg.n_entry {
            let i_pg = seg.a_pgno[seg.a_index[seg.i_next] as usize];
            if i_pg > i_min {
                if i_pg < i_ret {
                    i_ret = i_pg;
                    i_frame = seg.i_zero + seg.a_index[seg.i_next] as u32;
                }
                break;
            }
            seg.i_next += 1;
        }
    }
    p.i_prior = i_ret;
    if i_ret == 0xFFFFFFFF {
        None
    } else {
        Some((i_ret, i_frame))
    }
}

impl Wal {
    // -----------------------------------------------------------------------
    // Acesso ao índice (apWiData).
    // -----------------------------------------------------------------------

    /// Lê `buf.len()` bytes da página `pg` do índice a partir de `off`.
    fn wi_read(&mut self, db_fd: &mut dyn VfsFile, pg: usize, off: usize, buf: &mut [u8]) -> Result<(), i32> {
        match self.wi_data.get(pg) {
            Some(Some(WiPage::Heap(v))) => {
                if off + buf.len() > v.len() {
                    return Err(SQLITE_CORRUPT_BKPT);
                }
                buf.copy_from_slice(&v[off..off + buf.len()]);
                Ok(())
            }
            Some(Some(WiPage::Shm(r))) => {
                let r = *r;
                rc_result(db_fd.shm_read(&r, off, buf))
            }
            _ => Err(SQLITE_ERROR),
        }
    }

    /// Escreve `data` na página `pg` do índice a partir de `off`.
    fn wi_write(&mut self, db_fd: &mut dyn VfsFile, pg: usize, off: usize, data: &[u8]) -> Result<(), i32> {
        match self.wi_data.get_mut(pg) {
            Some(Some(WiPage::Heap(v))) => {
                if off + data.len() > v.len() {
                    return Err(SQLITE_CORRUPT_BKPT);
                }
                v[off..off + data.len()].copy_from_slice(data);
                Ok(())
            }
            Some(Some(WiPage::Shm(r))) => {
                let r = *r;
                rc_result(db_fd.shm_write(&r, off, data))
            }
            _ => Err(SQLITE_ERROR),
        }
    }

    fn wi_get32(&mut self, db_fd: &mut dyn VfsFile, pg: usize, off: usize) -> Result<u32, i32> {
        let mut b = [0u8; 4];
        self.wi_read(db_fd, pg, off, &mut b)?;
        Ok(u32::from_ne_bytes(b))
    }

    fn wi_put32(&mut self, db_fd: &mut dyn VfsFile, pg: usize, off: usize, v: u32) -> Result<(), i32> {
        self.wi_write(db_fd, pg, off, &v.to_ne_bytes())
    }

    fn wi_get16(&mut self, db_fd: &mut dyn VfsFile, pg: usize, off: usize) -> Result<u16, i32> {
        let mut b = [0u8; 2];
        self.wi_read(db_fd, pg, off, &mut b)?;
        Ok(u16::from_ne_bytes(b))
    }

    fn wi_put16(&mut self, db_fd: &mut dyn VfsFile, pg: usize, off: usize, v: u16) -> Result<(), i32> {
        self.wi_write(db_fd, pg, off, &v.to_ne_bytes())
    }

    /// `memset(.., 0, n)` numa página do índice.
    fn wi_zero(&mut self, db_fd: &mut dyn VfsFile, pg: usize, off: usize, n: usize) -> Result<(), i32> {
        let z = vec![0u8; n];
        self.wi_write(db_fd, pg, off, &z)
    }

    /// `walIndexHdr(pWal)[which]`: cópia de uma das duas cópias do cabeçalho.
    fn wal_index_hdr(&mut self, db_fd: &mut dyn VfsFile, which: usize) -> Result<WalIndexHdr, i32> {
        let mut b = [0u8; WalIndexHdr::SIZE];
        self.wi_read(db_fd, 0, which * WalIndexHdr::SIZE, &mut b)?;
        Ok(WalIndexHdr::from_bytes(&b))
    }

    /// `walCkptInfo(pWal)->nBackfill`.
    fn ckpt_backfill(&mut self, db_fd: &mut dyn VfsFile) -> Result<u32, i32> {
        self.wi_get32(db_fd, 0, CKPT_BACKFILL_OFF)
    }

    fn set_ckpt_backfill(&mut self, db_fd: &mut dyn VfsFile, v: u32) -> Result<(), i32> {
        self.wi_put32(db_fd, 0, CKPT_BACKFILL_OFF, v)
    }

    /// `walCkptInfo(pWal)->aReadMark[i]`.
    fn ckpt_read_mark(&mut self, db_fd: &mut dyn VfsFile, i: i32) -> Result<u32, i32> {
        self.wi_get32(db_fd, 0, CKPT_READMARK_OFF + 4 * i as usize)
    }

    fn set_ckpt_read_mark(&mut self, db_fd: &mut dyn VfsFile, i: i32, v: u32) -> Result<(), i32> {
        self.wi_put32(db_fd, 0, CKPT_READMARK_OFF + 4 * i as usize, v)
    }

    fn set_ckpt_backfill_attempted(&mut self, db_fd: &mut dyn VfsFile, v: u32) -> Result<(), i32> {
        self.wi_put32(db_fd, 0, CKPT_BACKFILL_ATTEMPTED_OFF, v)
    }

    /// `walIndexPageRealloc`: garante a página `i_page` do índice. Devolve o código; a
    /// página fica em `wi_data[i_page]` (ou `None` em erro, ou no caso 3 do C).
    fn wal_index_page_realloc(&mut self, db_fd: &mut dyn VfsFile, i_page: usize) -> i32 {
        let mut rc = SQLITE_OK;
        if self.wi_data.len() <= i_page {
            self.wi_data.resize_with(i_page + 1, || None);
        }
        debug_assert!(self.wi_data[i_page].is_none());
        if self.exclusive_mode == WAL_HEAPMEMORY_MODE {
            self.wi_data[i_page] = Some(WiPage::Heap(vec![0u8; WALINDEX_PGSZ]));
        } else {
            let mut pp: Option<ShmRegion> = None;
            rc = db_fd.shm_map(i_page as i32, WALINDEX_PGSZ as i32, self.write_lock as i32, &mut pp);
            self.wi_data[i_page] = pp.map(WiPage::Shm);
            if rc != SQLITE_OK && (rc & 0xff) == SQLITE_READONLY {
                self.read_only |= WAL_SHM_RDONLY;
                if rc == SQLITE_READONLY {
                    rc = SQLITE_OK;
                }
            }
        }
        rc
    }

    /// `walIndexPage`.
    fn wal_index_page(&mut self, db_fd: &mut dyn VfsFile, i_page: usize) -> i32 {
        if self.wi_data.len() <= i_page || self.wi_data[i_page].is_none() {
            return self.wal_index_page_realloc(db_fd, i_page);
        }
        SQLITE_OK
    }

    /// A página `i_page` do índice está mapeada?
    fn has_page(&self, i_page: usize) -> bool {
        matches!(self.wi_data.get(i_page), Some(Some(_)))
    }

    /// `walShmBarrier`.
    fn wal_shm_barrier(&mut self, db_fd: &mut dyn VfsFile) {
        if self.exclusive_mode != WAL_HEAPMEMORY_MODE {
            db_fd.shm_barrier();
        }
    }

    /// `walIndexWriteHdr`: grava `self.hdr` no índice (com o checksum atualizado).
    fn wal_index_write_hdr(&mut self, db_fd: &mut dyn VfsFile) -> Result<(), i32> {
        debug_assert!(self.write_lock);
        self.hdr.is_init = 1;
        self.hdr.i_version = WALINDEX_MAX_VERSION;
        let b = self.hdr.to_bytes();
        self.hdr.a_cksum = wal_checksum_bytes(true, &b, WalIndexHdr::CKSUM_OFF, None);
        let b = self.hdr.to_bytes();
        self.wi_write(db_fd, 0, WalIndexHdr::SIZE, &b)?;
        self.wal_shm_barrier(db_fd);
        self.wi_write(db_fd, 0, 0, &b)
    }

    /// `walEncodeFrame`: monta o cabeçalho de um quadro em `a_frame` (24 bytes) e
    /// avança a cadeia de checksums em `hdr.aFrameCksum`.
    fn wal_encode_frame(&mut self, i_page: u32, n_truncate: u32, a_data: &[u8], a_frame: &mut [u8]) {
        put4byte(&mut a_frame[0..], i_page);
        put4byte(&mut a_frame[4..], n_truncate);
        if self.i_re_cksum == 0 {
            a_frame[8..16].copy_from_slice(&self.hdr.salt_bytes());
            let native = self.native_cksum();
            let mut ck = self.hdr.a_frame_cksum;
            ck = wal_checksum_bytes(native, &a_frame[..8], 8, Some(ck));
            ck = wal_checksum_bytes(native, a_data, self.sz_page as usize, Some(ck));
            self.hdr.a_frame_cksum = ck;
            put4byte(&mut a_frame[16..], ck[0]);
            put4byte(&mut a_frame[20..], ck[1]);
        } else {
            a_frame[8..24].fill(0);
        }
    }

    /// `hdr.bigEndCksum == SQLITE_BIGENDIAN`.
    fn native_cksum(&self) -> bool {
        self.hdr.big_end_cksum == SQLITE_BIGENDIAN as u8
    }

    /// `walDecodeFrame`: `frame` é o cabeçalho de 24 bytes seguido dos dados da página.
    /// Se o quadro é válido devolve (página, tamanho do banco do commit ou 0). A cadeia
    /// `hdr.aFrameCksum` é atualizada mesmo quando o quadro é inválido, como no C.
    fn wal_decode_frame(&mut self, frame: &[u8]) -> Option<(u32, u32)> {
        if self.hdr.salt_bytes() != frame[8..16] {
            return None;
        }
        let pgno = get4byte(&frame[0..]);
        if pgno == 0 {
            return None;
        }
        let native = self.native_cksum();
        let mut ck = self.hdr.a_frame_cksum;
        ck = wal_checksum_bytes(native, &frame[..8], 8, Some(ck));
        ck = wal_checksum_bytes(native, &frame[WAL_FRAME_HDRSIZE..], self.sz_page as usize, Some(ck));
        self.hdr.a_frame_cksum = ck;
        if ck[0] != get4byte(&frame[16..]) || ck[1] != get4byte(&frame[20..]) {
            return None;
        }
        Some((pgno, get4byte(&frame[4..])))
    }

    // -----------------------------------------------------------------------
    // Travas.
    // -----------------------------------------------------------------------

    /// `walLockShared`.
    fn wal_lock_shared(&mut self, db_fd: &mut dyn VfsFile, lock_idx: i32) -> i32 {
        if self.exclusive_mode != 0 {
            return SQLITE_OK;
        }
        db_fd.shm_lock(lock_idx, 1, SQLITE_SHM_LOCK | SQLITE_SHM_SHARED)
    }

    /// `walUnlockShared`.
    fn wal_unlock_shared(&mut self, db_fd: &mut dyn VfsFile, lock_idx: i32) {
        if self.exclusive_mode != 0 {
            return;
        }
        let _ = db_fd.shm_lock(lock_idx, 1, SQLITE_SHM_UNLOCK | SQLITE_SHM_SHARED);
    }

    /// `walLockExclusive`.
    fn wal_lock_exclusive(&mut self, db_fd: &mut dyn VfsFile, lock_idx: i32, n: i32) -> i32 {
        if self.exclusive_mode != 0 {
            return SQLITE_OK;
        }
        db_fd.shm_lock(lock_idx, n, SQLITE_SHM_LOCK | SQLITE_SHM_EXCLUSIVE)
    }

    /// `walUnlockExclusive`.
    fn wal_unlock_exclusive(&mut self, db_fd: &mut dyn VfsFile, lock_idx: i32, n: i32) {
        if self.exclusive_mode != 0 {
            return;
        }
        let _ = db_fd.shm_lock(lock_idx, n, SQLITE_SHM_UNLOCK | SQLITE_SHM_EXCLUSIVE);
    }

    /// `walBusyLock`: trava exclusiva repetida enquanto `busy` pedir nova tentativa.
    fn wal_busy_lock(
        &mut self,
        db_fd: &mut dyn VfsFile,
        busy: &mut Option<&mut dyn FnMut() -> i32>,
        lock_idx: i32,
        n: i32,
    ) -> i32 {
        loop {
            let rc = self.wal_lock_exclusive(db_fd, lock_idx, n);
            if rc != SQLITE_BUSY {
                return rc;
            }
            match busy.as_deref_mut() {
                Some(f) => {
                    if f() == 0 {
                        return rc;
                    }
                }
                None => return rc,
            }
        }
    }

    // -----------------------------------------------------------------------
    // Tabelas hash do índice.
    // -----------------------------------------------------------------------

    /// `walFramePgno`: número de página associado ao quadro `i_frame`.
    fn wal_frame_pgno(&mut self, db_fd: &mut dyn VfsFile, i_frame: u32) -> Result<u32, i32> {
        let i_hash = wal_frame_page(i_frame);
        if i_hash == 0 {
            self.wi_get32(db_fd, 0, WALINDEX_HDR_SIZE + 4 * (i_frame.wrapping_sub(1) as usize))
        } else {
            let idx = i_frame.wrapping_sub(1).wrapping_sub(HASHTABLE_NPAGE_ONE) % HASHTABLE_NPAGE;
            self.wi_get32(db_fd, i_hash as usize, 4 * idx as usize)
        }
    }

    /// `walHashGet`: localização da tabela hash e do mapa de páginas da página `i_hash`.
    fn wal_hash_get(&mut self, db_fd: &mut dyn VfsFile, i_hash: u32) -> Result<WalHashLoc, i32> {
        let rc = self.wal_index_page(db_fd, i_hash as usize);
        debug_assert!(rc == SQLITE_OK || i_hash > 0);
        if rc != SQLITE_OK {
            return Err(rc);
        }
        if !self.has_page(i_hash as usize) {
            return Err(SQLITE_ERROR);
        }
        let (pgno_off, i_zero) = if i_hash == 0 {
            (WALINDEX_HDR_SIZE, 0)
        } else {
            (0, HASHTABLE_NPAGE_ONE + (i_hash - 1) * HASHTABLE_NPAGE)
        };
        Ok(WalHashLoc { pg: i_hash as usize, hash_off: HASH_OFF, pgno_off, i_zero })
    }

    /// `walCleanupHash`: apaga da tabela hash as entradas de quadros maiores que
    /// `hdr.mxFrame` (chamada quando `mxFrame` diminui por rollback ou savepoint).
    fn wal_cleanup_hash(&mut self, db_fd: &mut dyn VfsFile) -> Result<(), i32> {
        debug_assert!(self.write_lock);
        if self.hdr.mx_frame == 0 {
            return Ok(());
        }
        // A página do quadro mx_frame já está mapeada; erro aqui é "defesa em
        // profundidade" no C e a função só retorna.
        let loc = match self.wal_hash_get(db_fd, wal_frame_page(self.hdr.mx_frame)) {
            Ok(l) => l,
            Err(_) => return Ok(()),
        };
        let i_limit = self.hdr.mx_frame - loc.i_zero;
        debug_assert!(i_limit > 0);
        let mut tbl = vec![0u8; 2 * HASHTABLE_NSLOT as usize];
        self.wi_read(db_fd, loc.pg, loc.hash_off, &mut tbl)?;
        for i in 0..HASHTABLE_NSLOT as usize {
            let v = u16::from_ne_bytes([tbl[2 * i], tbl[2 * i + 1]]);
            if v as u32 > i_limit {
                self.wi_put16(db_fd, loc.pg, loc.hash_off + 2 * i, 0)?;
            }
        }
        let start = loc.pgno_off + 4 * i_limit as usize;
        let n_byte = loc.hash_off.saturating_sub(start);
        self.wi_zero(db_fd, loc.pg, start, n_byte)
    }

    /// `walIndexAppend`: registra no índice que a página `i_page` do banco está no
    /// quadro `i_frame`.
    fn wal_index_append(&mut self, db_fd: &mut dyn VfsFile, i_frame: u32, i_page: u32) -> i32 {
        match self.wal_index_append_inner(db_fd, i_frame, i_page) {
            Ok(()) => SQLITE_OK,
            Err(rc) => rc,
        }
    }

    fn wal_index_append_inner(&mut self, db_fd: &mut dyn VfsFile, i_frame: u32, i_page: u32) -> Result<(), i32> {
        let loc = self.wal_hash_get(db_fd, wal_frame_page(i_frame))?;
        let idx = i_frame - loc.i_zero;
        debug_assert!(idx <= HASHTABLE_NSLOT / 2 + 1);

        // Primeira entrada da tabela: zera a tabela inteira e o mapa de páginas.
        if idx == 1 {
            let n_byte = loc.hash_off + 2 * HASHTABLE_NSLOT as usize - loc.pgno_off;
            self.wi_zero(db_fd, loc.pg, loc.pgno_off, n_byte)?;
        }

        // Entrada já preenchida: restos de um escritor que saiu no meio da transação.
        let pgno_slot = loc.pgno_off + 4 * (idx as usize - 1);
        if self.wi_get32(db_fd, loc.pg, pgno_slot)? != 0 {
            self.wal_cleanup_hash(db_fd)?;
            debug_assert!(self.wi_get32(db_fd, loc.pg, pgno_slot)? == 0);
        }

        // Grava a entrada do mapa e o slot da tabela hash.
        let mut n_collide = idx as i32;
        let mut i_key = wal_hash(i_page);
        while self.wi_get16(db_fd, loc.pg, loc.hash_off + 2 * i_key)? != 0 {
            let nc = n_collide;
            n_collide -= 1;
            if nc == 0 {
                return Err(SQLITE_CORRUPT_BKPT);
            }
            i_key = wal_next_hash(i_key);
        }
        self.wi_put32(db_fd, loc.pg, pgno_slot, i_page)?;
        self.wi_put16(db_fd, loc.pg, loc.hash_off + 2 * i_key, idx as u16)
    }

    // -----------------------------------------------------------------------
    // Recuperação do índice a partir do arquivo -wal.
    // -----------------------------------------------------------------------

    /// `walIndexRecover`: reconstrói o índice lendo o arquivo `-wal`.
    fn wal_index_recover(&mut self, db_fd: &mut dyn VfsFile) -> i32 {
        let mut rc: i32;
        let mut a_frame_cksum = [0u32; 2];

        debug_assert!(self.write_lock);
        let i_lock = WAL_ALL_BUT_WRITE + self.ckpt_lock as i32;
        rc = self.wal_lock_exclusive(db_fd, i_lock, wal_read_lock(0) - i_lock);
        if rc != SQLITE_OK {
            return rc;
        }

        self.hdr = WalIndexHdr::default();

        let mut n_size: i64 = 0;
        rc = self.wal_fd.file_size(&mut n_size);

        'recovery_error: {
            if rc != SQLITE_OK {
                break 'recovery_error;
            }

            if n_size > WAL_HDRSIZE as i64 {
                'finished: {
                    let mut a_buf = [0u8; WAL_HDRSIZE];
                    rc = self.wal_fd.read(&mut a_buf, 0);
                    if rc != SQLITE_OK {
                        break 'recovery_error;
                    }

                    // Tamanho de página que não é potência de dois, maior que o máximo
                    // ou menor que 512, ou magic inválido: o arquivo não tem dados válidos.
                    let magic = get4byte(&a_buf[0..]);
                    let sz_page = get4byte(&a_buf[8..]);
                    if (magic & 0xFFFFFFFE) != WAL_MAGIC
                        || (sz_page & sz_page.wrapping_sub(1)) != 0
                        || sz_page > SQLITE_MAX_PAGE_SIZE as u32
                        || sz_page < 512
                    {
                        break 'finished;
                    }
                    self.hdr.big_end_cksum = (magic & 0x00000001) as u8;
                    self.sz_page = sz_page;
                    self.n_ckpt = get4byte(&a_buf[12..]);
                    self.hdr.a_salt = [ne32(&a_buf, 16), ne32(&a_buf, 20)];

                    // Confere o checksum do cabeçalho do WAL.
                    self.hdr.a_frame_cksum =
                        wal_checksum_bytes(self.native_cksum(), &a_buf, WAL_HDRSIZE - 2 * 4, None);
                    if self.hdr.a_frame_cksum[0] != get4byte(&a_buf[24..])
                        || self.hdr.a_frame_cksum[1] != get4byte(&a_buf[28..])
                    {
                        break 'finished;
                    }

                    // Confere a versão do formato.
                    let version = get4byte(&a_buf[4..]);
                    if version != WAL_MAX_VERSION {
                        rc = SQLITE_CANTOPEN_BKPT;
                        break 'finished;
                    }

                    // Buffer para ler quadros e cópia privada de uma página do índice.
                    let sz_frame = sz_page as usize + WAL_FRAME_HDRSIZE;
                    let mut a_frame = vec![0u8; sz_frame];
                    let mut a_private = vec![0u8; WALINDEX_PGSZ];

                    // Lê todos os quadros do log.
                    let i_last_frame = ((n_size - WAL_HDRSIZE as i64) / sz_frame as i64) as u32;
                    let mut i_pg = 0u32;
                    while i_pg <= wal_frame_page(i_last_frame) {
                        let i_last = i_last_frame.min(HASHTABLE_NPAGE_ONE + i_pg * HASHTABLE_NPAGE);
                        let i_first = 1 + if i_pg == 0 { 0 } else { HASHTABLE_NPAGE_ONE + (i_pg - 1) * HASHTABLE_NPAGE };
                        rc = self.wal_index_page(db_fd, i_pg as usize);
                        if !self.has_page(i_pg as usize) {
                            break;
                        }
                        // A página compartilhada sai de cena enquanto o buffer privado
                        // recebe as entradas.
                        let shared = self.wi_data[i_pg as usize].take();
                        self.wi_data[i_pg as usize] = Some(WiPage::Heap(std::mem::take(&mut a_private)));

                        let mut i_frame = i_first;
                        while i_frame <= i_last {
                            let i_offset = wal_frame_offset(i_frame, sz_page as i32);

                            // Lê e decodifica o próximo quadro do log.
                            rc = self.wal_fd.read(&mut a_frame, i_offset);
                            if rc != SQLITE_OK {
                                break;
                            }
                            let (pgno, n_truncate) = match self.wal_decode_frame(&a_frame) {
                                Some(v) => v,
                                None => break,
                            };
                            rc = self.wal_index_append(db_fd, i_frame, pgno);
                            if rc != SQLITE_OK {
                                break;
                            }

                            // nTruncate diferente de zero marca um registro de commit.
                            if n_truncate != 0 {
                                self.hdr.mx_frame = i_frame;
                                self.hdr.n_page = n_truncate;
                                self.hdr.sz_page = ((sz_page & 0xff00) | (sz_page >> 16)) as u16;
                                a_frame_cksum = self.hdr.a_frame_cksum;
                            }
                            i_frame += 1;
                        }

                        if let Some(WiPage::Heap(v)) = std::mem::replace(&mut self.wi_data[i_pg as usize], shared) {
                            a_private = v;
                        }
                        let n_hdr = if i_pg == 0 { WALINDEX_HDR_SIZE } else { 0 };
                        if let Err(e) = self.wi_write(db_fd, i_pg as usize, n_hdr, &a_private[n_hdr..]) {
                            if rc == SQLITE_OK {
                                rc = e;
                            }
                            break;
                        }
                        if i_frame <= i_last {
                            break;
                        }
                        i_pg += 1;
                    }
                }
            }

            // finished:
            if rc == SQLITE_OK {
                self.hdr.a_frame_cksum = a_frame_cksum;
                if let Err(e) = self.wal_index_write_hdr(db_fd) {
                    rc = e;
                    break 'recovery_error;
                }

                // Reinicia o cabeçalho de checkpoint (seguro porque as travas aqui
                // seguradas excluem os outros escritores e checkpointers) e ajusta as
                // marcas de leitura de 1 a N.
                let mx_frame = self.hdr.mx_frame;
                let r = (|| -> Result<(), i32> {
                    self.set_ckpt_backfill(db_fd, 0)?;
                    self.set_ckpt_backfill_attempted(db_fd, mx_frame)?;
                    self.set_ckpt_read_mark(db_fd, 0, 0)
                })();
                if let Err(e) = r {
                    rc = e;
                    break 'recovery_error;
                }
                for i in 1..WAL_NREADER {
                    rc = self.wal_lock_exclusive(db_fd, wal_read_lock(i), 1);
                    if rc == SQLITE_OK {
                        let mark = if i == 1 && mx_frame != 0 { mx_frame } else { READMARK_NOT_USED };
                        let r = self.set_ckpt_read_mark(db_fd, i, mark);
                        self.wal_unlock_exclusive(db_fd, wal_read_lock(i), 1);
                        if let Err(e) = r {
                            rc = e;
                            break 'recovery_error;
                        }
                    } else if rc != SQLITE_BUSY {
                        break 'recovery_error;
                    }
                }

                // Mais de um quadro recuperado: avisa pelo log, para ajudar a achar
                // aplicações que encerram sem fazer checkpoint.
                if self.hdr.n_page != 0 {
                    let mut msg: Vec<u8> = b"recovered ".to_vec();
                    msg.extend_from_slice(self.hdr.mx_frame.to_string().as_bytes());
                    msg.extend_from_slice(b" frames from WAL file ");
                    msg.extend_from_slice(self.wal_path());
                    (self.hooks.log)(SQLITE_NOTICE_RECOVER_WAL, &msg);
                }
            }
        }

        // recovery_error:
        self.wal_unlock_exclusive(db_fd, i_lock, wal_read_lock(0) - i_lock);
        rc
    }

    /// Caminho do arquivo do WAL, sem os pares de URI que seguem o primeiro NUL.
    fn wal_path(&self) -> &[u8] {
        let n = self.wal_name.iter().position(|&c| c == 0).unwrap_or(self.wal_name.len());
        &self.wal_name[..n]
    }

    /// `walIndexClose`: fecha o índice.
    fn wal_index_close(&mut self, db_fd: &mut dyn VfsFile, is_delete: bool) {
        // No modo heap (e com `bShmUnreliable`) as páginas são do heap; nos demais
        // casos são regiões que o `shm_unmap` invalida, então somem do mesmo jeito.
        for p in self.wi_data.iter_mut() {
            *p = None;
        }
        if self.exclusive_mode != WAL_HEAPMEMORY_MODE {
            let _ = db_fd.shm_unmap(is_delete as i32);
        }
    }

    /// `sqlite3WalOpen`: abre a conexão com o arquivo `wal_name`. O arquivo do banco já
    /// deve estar aberto em `db_fd`, com uma trava SHARED. `wal_name` é o
    /// `sqlite3_filename` (caminho, NUL, pares de URI). `b_no_shm` roda o índice na
    /// memória do heap.
    pub fn open(
        vfs: &VfsRef,
        db_fd: &mut dyn VfsFile,
        wal_name: &[u8],
        b_no_shm: bool,
        mx_wal_size: i64,
        hooks: WalHooks,
    ) -> Result<Wal, i32> {
        debug_assert!(!wal_name.is_empty() && wal_name[0] != 0);

        // Abre o arquivo do WAL.
        let flags = SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE | SQLITE_OPEN_WAL;
        let mut out_flags = 0i32;
        let wal_fd = match os_open(&**vfs, Some(wal_name), flags, &mut out_flags) {
            Ok(f) => f,
            Err(rc) => {
                // walIndexClose(pRet, 0)
                if !b_no_shm {
                    let _ = db_fd.shm_unmap(0);
                }
                return Err(rc);
            }
        };

        let mut ret = Wal {
            vfs: vfs.clone(),
            wal_fd,
            i_callback: 0,
            mx_wal_size,
            wi_data: Vec::new(),
            sz_page: 0,
            read_lock: -1,
            exclusive_mode: if b_no_shm { WAL_HEAPMEMORY_MODE } else { WAL_NORMAL_MODE },
            write_lock: false,
            ckpt_lock: false,
            read_only: 0,
            truncate_on_commit: false,
            sync_header: true,
            pad_to_sector_boundary: true,
            b_shm_unreliable: false,
            hdr: WalIndexHdr::default(),
            min_frame: 0,
            i_re_cksum: 0,
            wal_name: wal_name.to_vec(),
            n_ckpt: 0,
            hooks,
        };
        if (out_flags & SQLITE_OPEN_READONLY) != 0 {
            ret.read_only = WAL_RDONLY;
        }

        let i_dc = db_fd.device_characteristics();
        if (i_dc & SQLITE_IOCAP_SEQUENTIAL) != 0 {
            ret.sync_header = false;
        }
        if (i_dc & SQLITE_IOCAP_POWERSAFE_OVERWRITE) != 0 {
            ret.pad_to_sector_boundary = false;
        }
        Ok(ret)
    }

    /// `sqlite3WalLimit`: muda o tamanho a que o WAL é truncado a cada reinício.
    pub fn limit(&mut self, i_limit: i64) {
        self.mx_wal_size = i_limit;
    }

    // -----------------------------------------------------------------------
    // Iterador e checkpoint.
    // -----------------------------------------------------------------------

    /// `walIteratorInit`: iterador sobre as páginas do WAL depois do quadro
    /// `n_backfill`, em ordem crescente. O chamador segura a trava de checkpoint. Os
    /// números de página são copiados do índice na criação (sob a trava de checkpoint
    /// e com a leitura 0 presa, o conteúdo até `mxFrame` não muda).
    fn wal_iterator_init(&mut self, db_fd: &mut dyn VfsFile, n_backfill: u32) -> Result<WalIterator, i32> {
        debug_assert!(self.ckpt_lock && self.hdr.mx_frame > 0);
        let i_last = self.hdr.mx_frame;
        let n_segment = wal_frame_page(i_last) + 1;
        let mut p = WalIterator {
            i_prior: 0,
            a_segment: (0..n_segment).map(|_| WalSegment::default()).collect(),
        };
        let mut a_tmp = vec![0u16; HASHTABLE_NPAGE as usize];
        let mut i = wal_frame_page(n_backfill.wrapping_add(1));
        while i < n_segment {
            let loc = self.wal_hash_get(db_fd, i)?;
            let n_entry = if i + 1 == n_segment {
                (i_last - loc.i_zero) as usize
            } else {
                (loc.hash_off - loc.pgno_off) / 4
            };
            let mut raw = vec![0u8; n_entry * 4];
            self.wi_read(db_fd, loc.pg, loc.pgno_off, &mut raw)?;
            let a_pgno: Vec<u32> = raw.chunks_exact(4).map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]])).collect();
            let mut a_index: Vec<u16> = (0..n_entry).map(|j| j as u16).collect();
            let mut n = n_entry;
            wal_mergesort(&a_pgno, &mut a_tmp, &mut a_index, &mut n);
            let seg = &mut p.a_segment[i as usize];
            seg.i_zero = loc.i_zero + 1;
            seg.n_entry = n;
            seg.a_index = a_index;
            seg.a_pgno = a_pgno;
            i += 1;
        }
        Ok(p)
    }

    /// `walPagesize`: tamanho de página do banco segundo o cabeçalho em cache.
    fn wal_pagesize(&self) -> i32 {
        (self.hdr.sz_page & 0xfe00) as i32 + (((self.hdr.sz_page & 0x0001) as i32) << 16)
    }

    /// `walRestartHdr`: atualiza as estruturas compartilhadas para que o próximo
    /// escritor grave a partir do início do log. `salt1` vira `aSalt[1]` (aleatório).
    fn wal_restart_hdr(&mut self, db_fd: &mut dyn VfsFile, salt1: u32) -> Result<(), i32> {
        self.n_ckpt = self.n_ckpt.wrapping_add(1);
        self.hdr.mx_frame = 0;
        // aSalt[0] é guardado como bytes big-endian.
        let s0 = u32::from_be_bytes(self.hdr.a_salt[0].to_ne_bytes()).wrapping_add(1);
        self.hdr.a_salt[0] = u32::from_ne_bytes(s0.to_be_bytes());
        self.hdr.a_salt[1] = salt1;
        self.wal_index_write_hdr(db_fd)?;
        self.set_ckpt_backfill(db_fd, 0)?;
        self.set_ckpt_backfill_attempted(db_fd, 0)?;
        self.set_ckpt_read_mark(db_fd, 1, 0)?;
        for i in 2..WAL_NREADER {
            self.set_ckpt_read_mark(db_fd, i, READMARK_NOT_USED)?;
        }
        Ok(())
    }

    /// `walCheckpoint`: copia do WAL para o banco o que os leitores deixam.
    fn wal_checkpoint(
        &mut self,
        db_fd: &mut dyn VfsFile,
        interrupt: &mut dyn FnMut() -> i32,
        e_mode: i32,
        busy: &mut Option<&mut dyn FnMut() -> i32>,
        sync_flags: i32,
        z_buf: &mut [u8],
    ) -> i32 {
        let mut rc = SQLITE_OK;
        let sz_page = self.wal_pagesize();
        let sz = sz_page as usize;
        let n_backfill_now = tri!(self.ckpt_backfill(db_fd));
        if n_backfill_now < self.hdr.mx_frame {
            let mut p_iter: Option<WalIterator> = None;

            debug_assert!(e_mode != SQLITE_CHECKPOINT_PASSIVE || busy.is_none());

            // mxSafeFrame: último quadro que pode ir para o banco sem pisar em página
            // usada por leitor ativo.
            let mut mx_safe_frame = self.hdr.mx_frame;
            let mx_page = self.hdr.n_page;
            for i in 1..WAL_NREADER {
                let y = tri!(self.ckpt_read_mark(db_fd, i));
                if mx_safe_frame > y {
                    debug_assert!(y <= self.hdr.mx_frame);
                    rc = self.wal_busy_lock(db_fd, busy, wal_read_lock(i), 1);
                    if rc == SQLITE_OK {
                        let i_mark = if i == 1 { mx_safe_frame } else { READMARK_NOT_USED };
                        let r = self.set_ckpt_read_mark(db_fd, i, i_mark);
                        self.wal_unlock_exclusive(db_fd, wal_read_lock(i), 1);
                        if let Err(e) = r {
                            return e;
                        }
                    } else if rc == SQLITE_BUSY {
                        mx_safe_frame = y;
                        *busy = None;
                    } else {
                        return rc;
                    }
                }
            }

            // Aloca o iterador.
            let n_backfill_cur = tri!(self.ckpt_backfill(db_fd));
            if n_backfill_cur < mx_safe_frame {
                match self.wal_iterator_init(db_fd, n_backfill_cur) {
                    Ok(it) => {
                        p_iter = Some(it);
                        rc = SQLITE_OK;
                    }
                    Err(e) => rc = e,
                }
            }

            if p_iter.is_some() && {
                rc = self.wal_busy_lock(db_fd, busy, wal_read_lock(0), 1);
                rc == SQLITE_OK
            } {
                let mut iter = match p_iter.take() {
                    Some(it) => it,
                    None => return SQLITE_ERROR,
                };
                let n_backfill = match self.ckpt_backfill(db_fd) {
                    Ok(v) => v,
                    Err(e) => {
                        self.wal_unlock_exclusive(db_fd, wal_read_lock(0), 1);
                        return e;
                    }
                };
                tri_set(&mut rc, self.set_ckpt_backfill_attempted(db_fd, mx_safe_frame));
                if rc != SQLITE_OK {
                    self.wal_unlock_exclusive(db_fd, wal_read_lock(0), 1);
                    return rc;
                }

                // Sincroniza o WAL em disco.
                rc = os_sync(&mut *self.wal_fd, ckpt_sync_flags(sync_flags));

                // Se o banco pode crescer, dá a dica do tamanho final ao VFS.
                if rc == SQLITE_OK {
                    let n_req = mx_page as i64 * sz_page as i64;
                    let mut n_size: i64 = 0;
                    os_file_control_hint(Some(&mut *db_fd), SQLITE_FCNTL_CKPT_START, &mut FileControlArg::None);
                    rc = db_fd.file_size(&mut n_size);
                    if rc == SQLITE_OK && n_size < n_req {
                        if (n_size + 65536 + self.hdr.mx_frame as i64 * sz_page as i64) < n_req {
                            // O banco final seria maior que o atual mais o WAL mais a
                            // página do byte pendente: há corrupção em algum lugar.
                            rc = SQLITE_CORRUPT_BKPT;
                        } else {
                            os_file_control_hint(
                                Some(&mut *db_fd),
                                SQLITE_FCNTL_SIZE_HINT,
                                &mut FileControlArg::Int64(n_req),
                            );
                        }
                    }
                }

                // Percorre o WAL copiando os dados para o arquivo do banco.
                while rc == SQLITE_OK {
                    let (i_dbpage, i_frame) = match wal_iterator_next(&mut iter) {
                        Some(v) => v,
                        None => break,
                    };
                    let r = interrupt();
                    if r != 0 {
                        rc = r;
                        break;
                    }
                    if i_frame <= n_backfill || i_frame > mx_safe_frame || i_dbpage > mx_page {
                        continue;
                    }
                    let i_offset = wal_frame_offset(i_frame, sz_page) + WAL_FRAME_HDRSIZE as i64;
                    rc = self.wal_fd.read(&mut z_buf[..sz], i_offset);
                    if rc != SQLITE_OK {
                        break;
                    }
                    let i_offset = (i_dbpage as i64 - 1) * sz_page as i64;
                    rc = db_fd.write(&z_buf[..sz], i_offset);
                    if rc != SQLITE_OK {
                        break;
                    }
                }
                os_file_control_hint(Some(&mut *db_fd), SQLITE_FCNTL_CKPT_DONE, &mut FileControlArg::None);

                // Se algum trabalho foi feito...
                if rc == SQLITE_OK {
                    let live_mx = match self.wal_index_hdr(db_fd, 0) {
                        Ok(h) => h.mx_frame,
                        Err(e) => {
                            self.wal_unlock_exclusive(db_fd, wal_read_lock(0), 1);
                            return e;
                        }
                    };
                    if mx_safe_frame == live_mx {
                        let sz_db = self.hdr.n_page as i64 * sz_page as i64;
                        rc = db_fd.truncate(sz_db);
                        if rc == SQLITE_OK {
                            rc = os_sync(db_fd, ckpt_sync_flags(sync_flags));
                        }
                    }
                    if rc == SQLITE_OK {
                        tri_set(&mut rc, self.set_ckpt_backfill(db_fd, mx_safe_frame));
                    }
                }

                // Solta a trava de leitor usada durante o backfill.
                self.wal_unlock_exclusive(db_fd, wal_read_lock(0), 1);
            }

            if rc == SQLITE_BUSY {
                // Não relata falha de checkpoint só porque há leitores ativos.
                rc = SQLITE_OK;
            }
        }

        // RESTART ou TRUNCATE com o WAL inteiro copiado: espera os leitores saírem do
        // WAL, para que o próximo escritor o reinicie.
        if rc == SQLITE_OK && e_mode != SQLITE_CHECKPOINT_PASSIVE {
            debug_assert!(self.write_lock);
            let n_backfill = tri!(self.ckpt_backfill(db_fd));
            if n_backfill < self.hdr.mx_frame {
                rc = SQLITE_BUSY;
            } else if e_mode >= SQLITE_CHECKPOINT_RESTART {
                let mut b = [0u8; 4];
                (self.hooks.randomness)(&mut b);
                let salt1 = u32::from_ne_bytes(b);
                debug_assert!(n_backfill == self.hdr.mx_frame);
                rc = self.wal_busy_lock(db_fd, busy, wal_read_lock(1), WAL_NREADER - 1);
                if rc == SQLITE_OK {
                    if e_mode == SQLITE_CHECKPOINT_TRUNCATE {
                        // Atualiza também o cabeçalho do índice, para que ele acompanhe o
                        // sistema de arquivos (o log passa a ter zero quadros válidos).
                        rc = match self.wal_restart_hdr(db_fd, salt1) {
                            Ok(()) => self.wal_fd.truncate(0),
                            Err(e) => e,
                        };
                    }
                    self.wal_unlock_exclusive(db_fd, wal_read_lock(1), WAL_NREADER - 1);
                }
            }
        }
        rc
    }

    /// `walLimitSize`: se o WAL é maior que `n_max` bytes, trunca para exatamente
    /// `n_max`. Erros são ignorados (só vão para o log).
    fn wal_limit_size(&mut self, n_max: i64) {
        let mut sz: i64 = 0;
        let mut rx = self.wal_fd.file_size(&mut sz);
        if rx == SQLITE_OK && sz > n_max {
            rx = self.wal_fd.truncate(n_max);
        }
        if rx != 0 {
            let mut msg: Vec<u8> = b"cannot limit WAL size: ".to_vec();
            msg.extend_from_slice(self.wal_path());
            (self.hooks.log)(rx, &msg);
        }
    }

    /// `sqlite3WalClose`: fecha a conexão. Com `buf` (o `zBuf` do C, que também dá o
    /// `nBuf`), tenta a trava EXCLUSIVE do banco; obtida, faz checkpoint e apaga o WAL
    /// e o índice. A trava EXCLUSIVE não é solta.
    pub fn close(
        mut self,
        db_fd: &mut dyn VfsFile,
        interrupt: &mut dyn FnMut() -> i32,
        sync_flags: i32,
        buf: Option<&mut [u8]>,
    ) -> i32 {
        let mut rc = SQLITE_OK;
        let mut is_delete = false;

        if let Some(buf) = buf {
            rc = db_fd.lock(SQLITE_LOCK_EXCLUSIVE);
            if rc == SQLITE_OK {
                if self.exclusive_mode == WAL_NORMAL_MODE {
                    self.exclusive_mode = WAL_EXCLUSIVE_MODE;
                }
                rc = self.checkpoint(db_fd, interrupt, SQLITE_CHECKPOINT_PASSIVE, None, sync_flags, buf, None, None);
                if rc == SQLITE_OK {
                    let mut arg = FileControlArg::Int(-1);
                    os_file_control_hint(Some(&mut *db_fd), SQLITE_FCNTL_PERSIST_WAL, &mut arg);
                    let b_persist = match arg {
                        FileControlArg::Int(v) => v,
                        _ => -1,
                    };
                    if b_persist != 1 {
                        // Checkpoint concluído e sincronizado, fora do modo WAL
                        // persistente: tenta apagar o arquivo do WAL.
                        is_delete = true;
                    } else if self.mx_wal_size >= 0 {
                        // Modo persistente com journal_size_limit não negativo: trunca
                        // a zero bytes (truncar ao limite poderia deixar WAL corrompido).
                        self.wal_limit_size(0);
                    }
                }
            }
        }

        self.wal_index_close(db_fd, is_delete);
        self.wal_fd.close();
        if is_delete {
            let _ = self.vfs.delete(self.wal_path(), 0);
        }
        rc
    }

    // -----------------------------------------------------------------------
    // Transação de leitura.
    // -----------------------------------------------------------------------

    /// `walIndexTryHdr`: tenta ler o cabeçalho do índice. `Ok(false)` é sucesso (o `0`
    /// do C), `Ok(true)` é problema (leitura suja, cabeçalho zerado, checksum errado).
    /// Se o cabeçalho lido difere de `self.hdr`, atualiza `self.hdr` e `*changed = 1`.
    fn wal_index_try_hdr(&mut self, db_fd: &mut dyn VfsFile, changed: &mut i32) -> Result<bool, i32> {
        debug_assert!(self.has_page(0));
        // Lê a cópia [0] e depois a [1]; as escritas são na ordem inversa.
        let h1 = self.wal_index_hdr(db_fd, 0)?;
        self.wal_shm_barrier(db_fd);
        let h2 = self.wal_index_hdr(db_fd, 1)?;

        if h1 != h2 {
            return Ok(true); // leitura suja
        }
        if h1.is_init == 0 {
            return Ok(true); // cabeçalho malformado, provavelmente zeros
        }
        let a_cksum = wal_checksum_bytes(true, &h1.to_bytes(), WalIndexHdr::CKSUM_OFF, None);
        if a_cksum != h1.a_cksum {
            return Ok(true); // checksum não confere
        }

        if self.hdr != h1 {
            *changed = 1;
            self.hdr = h1;
            self.sz_page = (self.hdr.sz_page & 0xfe00) as u32 + (((self.hdr.sz_page & 0x0001) as u32) << 16);
        }
        Ok(false)
    }

    /// `walIndexReadHdr`: lê o cabeçalho do índice para `self.hdr`; se parecer
    /// corrompido, reconstrói o índice a partir do WAL. `*changed = 1` se `self.hdr`
    /// mudou.
    fn wal_index_read_hdr(&mut self, db_fd: &mut dyn VfsFile, changed: &mut i32) -> i32 {
        let mut rc = self.wal_index_page(db_fd, 0);
        if rc != SQLITE_OK {
            if rc == SQLITE_READONLY_CANTINIT {
                // A memória compartilhada abre mas não é gravável, e não dá para
                // confirmar que outro escritor a mantém aberta: o conteúdo não é confiável.
                debug_assert!(!self.has_page(0));
                debug_assert!(!self.write_lock);
                debug_assert!((self.read_only & WAL_SHM_RDONLY) != 0);
                self.b_shm_unreliable = true;
                self.exclusive_mode = WAL_HEAPMEMORY_MODE;
                *changed = 1;
            } else {
                return rc;
            }
        }
        debug_assert!(self.has_page(0) || !self.write_lock);

        // Primeira página mapeada: tenta ler o cabeçalho sem trava.
        let mut bad_hdr = if self.has_page(0) {
            match self.wal_index_try_hdr(db_fd, changed) {
                Ok(b) => b,
                Err(e) => return e,
            }
        } else {
            true
        };

        // A primeira tentativa pode ter perdido uma corrida com um escritor: pega a
        // trava de escrita e tenta de novo.
        if bad_hdr {
            if !self.b_shm_unreliable && (self.read_only & WAL_SHM_RDONLY) != 0 {
                rc = self.wal_lock_shared(db_fd, WAL_WRITE_LOCK);
                if rc == SQLITE_OK {
                    self.wal_unlock_shared(db_fd, WAL_WRITE_LOCK);
                    rc = SQLITE_READONLY_RECOVERY;
                }
            } else {
                let b_write_lock = self.write_lock;
                if b_write_lock || {
                    rc = self.wal_lock_exclusive(db_fd, WAL_WRITE_LOCK, 1);
                    rc == SQLITE_OK
                } {
                    self.write_lock = true;
                    rc = self.wal_index_page(db_fd, 0);
                    if rc == SQLITE_OK {
                        bad_hdr = match self.wal_index_try_hdr(db_fd, changed) {
                            Ok(b) => b,
                            Err(e) => {
                                rc = e;
                                true
                            }
                        };
                        if bad_hdr && rc == SQLITE_OK {
                            // Mesmo com a trava de escrita o cabeçalho segue
                            // malformado: está corrompido e precisa ser reconstruído.
                            rc = self.wal_index_recover(db_fd);
                            *changed = 1;
                        }
                    }
                    if !b_write_lock {
                        self.write_lock = false;
                        self.wal_unlock_exclusive(db_fd, WAL_WRITE_LOCK, 1);
                    }
                }
            }
        }

        // Cabeçalho lido: confere a versão do índice.
        if !bad_hdr && self.hdr.i_version != WALINDEX_MAX_VERSION {
            rc = SQLITE_CANTOPEN_BKPT;
        }
        if self.b_shm_unreliable {
            if rc != SQLITE_OK {
                self.wal_index_close(db_fd, false);
                self.b_shm_unreliable = false;
                debug_assert!(!self.wi_data.is_empty() && self.wi_data[0].is_none());
                // walIndexRecover pode devolver SHORT_READ se um escritor truncou o
                // WAL por baixo: ele consertou o SHM, então tenta de novo.
                if rc == crate::consts::SQLITE_IOERR_SHORT_READ {
                    rc = WAL_RETRY;
                }
            }
            self.exclusive_mode = WAL_NORMAL_MODE;
        }
        rc
    }

    /// `walBeginShmUnreliable`: abre transação de leitura quando a memória compartilhada
    /// é só leitura e não há como saber se um escritor a mantém em dia com o WAL. O
    /// índice em heap já foi construído a partir do `-wal`.
    fn wal_begin_shm_unreliable(&mut self, db_fd: &mut dyn VfsFile, changed: &mut i32) -> i32 {
        let mut rc: i32;
        debug_assert!(self.b_shm_unreliable);
        debug_assert!((self.read_only & WAL_SHM_RDONLY) != 0);
        debug_assert!(self.has_page(0));

        'out: {
            // WAL_READ_LOCK(0) impede checkpoint dos escritores (mas não a recuperação).
            rc = self.wal_lock_shared(db_fd, wal_read_lock(0));
            if rc != SQLITE_OK {
                if rc == SQLITE_BUSY {
                    rc = WAL_RETRY;
                }
                break 'out;
            }
            self.read_lock = 0;

            // Um escritor separado pode ter se ligado à memória compartilhada,
            // tornando-a confiável de novo: `shm_map` devolve READONLY em vez de
            // READONLY_CANTINIT.
            let mut dummy: Option<ShmRegion> = None;
            rc = db_fd.shm_map(0, WALINDEX_PGSZ as i32, 0, &mut dummy);
            debug_assert!(rc != SQLITE_OK);
            if rc != SQLITE_READONLY_CANTINIT {
                rc = if rc == SQLITE_READONLY { WAL_RETRY } else { rc };
                break 'out;
            }

            // A memória compartilhada segue não confiável: o índice em heap vale.
            self.hdr = match self.wal_index_hdr(db_fd, 0) {
                Ok(h) => h,
                Err(e) => {
                    rc = e;
                    break 'out;
                }
            };

            // Confere se um escritor não mexeu no WAL por baixo e saiu.
            let mut sz_wal: i64 = 0;
            rc = self.wal_fd.file_size(&mut sz_wal);
            if rc != SQLITE_OK {
                break 'out;
            }
            if sz_wal < WAL_HDRSIZE as i64 {
                // WAL pequeno demais para ter cabeçalho: se mxFrame é 0 dá para ler só o
                // banco, mas o cache de páginas não é confiável.
                *changed = 1;
                rc = if self.hdr.mx_frame == 0 { SQLITE_OK } else { WAL_RETRY };
                break 'out;
            }

            // Os sais do início do WAL ainda batem?
            let mut a_buf = [0u8; WAL_HDRSIZE];
            rc = self.wal_fd.read(&mut a_buf, 0);
            if rc != SQLITE_OK {
                break 'out;
            }
            if self.hdr.salt_bytes() != a_buf[16..24] {
                // Um escritor reiniciou o WAL: o índice em memória precisa ser refeito.
                rc = WAL_RETRY;
                break 'out;
            }

            debug_assert!(self.sz_page & (self.sz_page - 1) == 0);
            debug_assert!(self.sz_page >= 512 && self.sz_page <= 65536);
            let sz_frame = self.sz_page as usize + WAL_FRAME_HDRSIZE;
            let mut a_frame = vec![0u8; sz_frame];

            // Alguma transação completa foi anexada desde que o índice em heap foi
            // criado? Então ele é descartado e o chamador tenta de novo.
            let a_save_cksum = self.hdr.a_frame_cksum;
            let mut i_offset = wal_frame_offset(self.hdr.mx_frame.wrapping_add(1), self.sz_page as i32);
            while i_offset + sz_frame as i64 <= sz_wal {
                rc = self.wal_fd.read(&mut a_frame, i_offset);
                if rc != SQLITE_OK {
                    break;
                }
                let n_truncate = match self.wal_decode_frame(&a_frame) {
                    Some((_, n)) => n,
                    None => break,
                };
                if n_truncate != 0 {
                    rc = WAL_RETRY;
                    break;
                }
                i_offset += sz_frame as i64;
            }
            self.hdr.a_frame_cksum = a_save_cksum;
        }

        if rc != SQLITE_OK {
            for p in self.wi_data.iter_mut() {
                *p = None;
            }
            self.b_shm_unreliable = false;
            self.end_read_transaction(db_fd);
            *changed = 1;
        }
        rc
    }

    /// `walTryBeginRead`: tenta abrir uma transação de leitura. Pode falhar por corrida
    /// ou condição transitória; nesse caso devolve `WAL_RETRY`. `use_wal` força o uso do
    /// WAL (cabeçalho já carregado); `cnt` conta as tentativas.
    fn wal_try_begin_read(&mut self, db_fd: &mut dyn VfsFile, changed: &mut i32, use_wal: bool, cnt: &mut i32) -> i32 {
        let mut rc = SQLITE_OK;

        debug_assert!(self.read_lock < 0);
        debug_assert!((self.read_only & WAL_SHM_RDONLY) == 0 || !use_wal);

        // Evita girar para sempre em caso de erro de protocolo. Depois de 5 tentativas
        // começa a dormir, cada vez mais.
        *cnt += 1;
        if *cnt > 5 {
            let mut n_delay: i32 = 1;
            let c = *cnt & !WAL_RETRY_BLOCKED_MASK;
            if c > WAL_RETRY_PROTOCOL_LIMIT {
                return SQLITE_PROTOCOL;
            }
            if *cnt >= 10 {
                n_delay = (c - 9) * (c - 9) * 39;
            }
            self.vfs.sleep(n_delay);
            *cnt &= !WAL_RETRY_BLOCKED_MASK;
        }

        if !use_wal {
            debug_assert!(rc == SQLITE_OK);
            if !self.b_shm_unreliable {
                rc = self.wal_index_read_hdr(db_fd, changed);
            }
            if rc == SQLITE_BUSY {
                // Sem recuperação rodando em outra thread/processo, BUSY vira
                // WAL_RETRY; se há recuperação, vira BUSY_RECOVERY.
                if !self.has_page(0) {
                    // `shm_map` devolveu BUSY: condição transitória.
                    rc = WAL_RETRY;
                } else {
                    rc = self.wal_lock_shared(db_fd, WAL_RECOVER_LOCK);
                    if rc == SQLITE_OK {
                        self.wal_unlock_shared(db_fd, WAL_RECOVER_LOCK);
                        rc = WAL_RETRY;
                    } else if rc == SQLITE_BUSY {
                        rc = SQLITE_BUSY_RECOVERY;
                    }
                }
            }
            if rc != SQLITE_OK {
                return rc;
            } else if self.b_shm_unreliable {
                return self.wal_begin_shm_unreliable(db_fd, changed);
            }
        }

        debug_assert!(self.has_page(0));
        let n_backfill = tri!(self.ckpt_backfill(db_fd));
        if !use_wal && n_backfill == self.hdr.mx_frame {
            // O WAL foi todo copiado para o banco (ou está vazio) e pode ser ignorado.
            rc = self.wal_lock_shared(db_fd, wal_read_lock(0));
            self.wal_shm_barrier(db_fd);
            if rc == SQLITE_OK {
                let live = tri!(self.wal_index_hdr(db_fd, 0));
                if live != self.hdr {
                    // Quadros podem ter sido anexados antes de READ_LOCK(0): um
                    // checkpointer pode ter começado a copiá-los e caído no meio.
                    self.wal_unlock_shared(db_fd, wal_read_lock(0));
                    return WAL_RETRY;
                }
                self.read_lock = 0;
                return SQLITE_OK;
            } else if rc != SQLITE_BUSY {
                return rc;
            }
        }

        // O leitor vai usar o WAL: escolhe a marca aReadMark[] mais próxima de
        // `hdr.mxFrame` sem passar dela, e a trava.
        let mut mx_read_mark = 0u32;
        let mut mx_i = 0i32;
        let mx_frame = self.hdr.mx_frame;
        for i in 1..WAL_NREADER {
            let this_mark = tri!(self.ckpt_read_mark(db_fd, i));
            if mx_read_mark <= this_mark && this_mark <= mx_frame {
                debug_assert!(this_mark != READMARK_NOT_USED);
                mx_read_mark = this_mark;
                mx_i = i;
            }
        }
        if (self.read_only & WAL_SHM_RDONLY) == 0 && (mx_read_mark < mx_frame || mx_i == 0) {
            for i in 1..WAL_NREADER {
                rc = self.wal_lock_exclusive(db_fd, wal_read_lock(i), 1);
                if rc == SQLITE_OK {
                    let r = self.set_ckpt_read_mark(db_fd, i, mx_frame);
                    mx_read_mark = mx_frame;
                    mx_i = i;
                    self.wal_unlock_exclusive(db_fd, wal_read_lock(i), 1);
                    if let Err(e) = r {
                        return e;
                    }
                    break;
                } else if rc != SQLITE_BUSY {
                    return rc;
                }
            }
        }
        if mx_i == 0 {
            debug_assert!(rc == SQLITE_BUSY || (self.read_only & WAL_SHM_RDONLY) != 0);
            return if rc == SQLITE_BUSY { WAL_RETRY } else { SQLITE_READONLY_CANTINIT };
        }

        rc = self.wal_lock_shared(db_fd, wal_read_lock(mx_i));
        if rc != SQLITE_OK {
            debug_assert!((rc & 0xFF) != SQLITE_BUSY || rc == SQLITE_BUSY);
            return if (rc & 0xFF) == SQLITE_BUSY { WAL_RETRY } else { rc };
        }

        // Com a trava de leitura obtida, confere que a marca e o cabeçalho do índice não
        // mudaram desde que foram lidos (o WAL pode ter sido reiniciado, ou quadros
        // posteriores copiados para o banco). Antes, `minFrame` passa ao primeiro quadro
        // ainda não copiado: os anteriores podem ser lidos direto do banco.
        self.min_frame = tri!(self.ckpt_backfill(db_fd)).wrapping_add(1);
        self.wal_shm_barrier(db_fd);
        let mark_now = tri!(self.ckpt_read_mark(db_fd, mx_i));
        let live = tri!(self.wal_index_hdr(db_fd, 0));
        if mark_now != mx_read_mark || live != self.hdr {
            self.wal_unlock_shared(db_fd, wal_read_lock(mx_i));
            return WAL_RETRY;
        }
        debug_assert!(mx_read_mark <= self.hdr.mx_frame);
        self.read_lock = mx_i as i16;
        rc
    }

    /// `sqlite3WalBeginReadTransaction`: abre uma transação de leitura (um retrato do
    /// WAL e do índice naquele instante). `*changed = 1` se o conteúdo do banco mudou
    /// desde a leitura anterior (o pager descarta o cache).
    pub fn begin_read_transaction(&mut self, db_fd: &mut dyn VfsFile, changed: &mut i32) -> i32 {
        let mut cnt = 0i32;
        debug_assert!(!self.ckpt_lock);
        loop {
            let rc = self.wal_try_begin_read(db_fd, changed, false, &mut cnt);
            if rc != WAL_RETRY {
                return rc;
            }
        }
    }

    /// `sqlite3WalEndReadTransaction`: termina a leitura, soltando a trava.
    pub fn end_read_transaction(&mut self, db_fd: &mut dyn VfsFile) {
        self.end_write_transaction(db_fd);
        if self.read_lock >= 0 {
            self.wal_unlock_shared(db_fd, wal_read_lock(self.read_lock as i32));
            self.read_lock = -1;
        }
    }

    /// `walFindFrame`: procura a página `pgno` no WAL. `*pi_read` recebe o quadro que a
    /// contém, ou zero.
    fn wal_find_frame(&mut self, db_fd: &mut dyn VfsFile, pgno: u32, pi_read: &mut u32) -> i32 {
        let mut i_read = 0u32;
        let i_last = self.hdr.mx_frame;

        // Só roda dentro de uma transação de leitura.
        debug_assert!(self.read_lock >= 0);

        // Sem quadros, ou com a leitura 0 (WAL ignorado): nada a procurar.
        if i_last == 0 || (self.read_lock == 0 && !self.b_shm_unreliable) {
            *pi_read = 0;
            return SQLITE_OK;
        }

        // Procura nas tabelas hash, da última para a primeira. A condição interna é
        // mais rígida que o necessário com acesso exclusivo: `aPgno[iFrame] == pgno`
        // descarta colisões e `iFrame <= iLast` descarta entradas que escritores
        // concorrentes anexaram depois da abertura desta transação.
        let i_min_hash = wal_frame_page(self.min_frame) as i32;
        let mut i_hash = wal_frame_page(i_last) as i32;
        while i_hash >= i_min_hash {
            let loc = match self.wal_hash_get(db_fd, i_hash as u32) {
                Ok(l) => l,
                Err(rc) => return rc,
            };
            let mut n_collide = HASHTABLE_NSLOT as i32;
            let mut i_key = wal_hash(pgno);
            loop {
                let i_h = tri!(self.wi_get16(db_fd, loc.pg, loc.hash_off + 2 * i_key)) as u32;
                if i_h == 0 {
                    break;
                }
                let i_frame = i_h + loc.i_zero;
                if i_frame <= i_last && i_frame >= self.min_frame {
                    let pg = tri!(self.wi_get32(db_fd, loc.pg, loc.pgno_off + 4 * (i_h as usize - 1)));
                    if pg == pgno {
                        debug_assert!(i_frame > i_read);
                        i_read = i_frame;
                    }
                }
                let nc = n_collide;
                n_collide -= 1;
                if nc == 0 {
                    *pi_read = 0;
                    return SQLITE_CORRUPT_BKPT;
                }
                i_key = wal_next_hash(i_key);
            }
            if i_read != 0 {
                break;
            }
            i_hash -= 1;
        }

        *pi_read = i_read;
        SQLITE_OK
    }

    /// `sqlite3WalFindFrame`: o quadro do WAL que contém a página `pgno` (zero se a
    /// página não está no WAL), dentro da transação de leitura aberta.
    pub fn find_frame(&mut self, db_fd: &mut dyn VfsFile, pgno: u32, pi_read: &mut u32) -> i32 {
        self.wal_find_frame(db_fd, pgno, pi_read)
    }

    /// `sqlite3WalReadFrame`: lê o conteúdo do quadro `i_read` para `out` (no máximo uma
    /// página; `out.len()` é o `nOut` do C).
    pub fn read_frame(&mut self, i_read: u32, out: &mut [u8]) -> i32 {
        let mut sz = self.hdr.sz_page as i32;
        sz = (sz & 0xfe00) + ((sz & 0x0001) << 16);
        let i_offset = wal_frame_offset(i_read, sz) + WAL_FRAME_HDRSIZE as i64;
        let n = out.len().min(sz as usize);
        self.wal_fd.read(&mut out[..n], i_offset)
    }

    /// `sqlite3WalDbsize`: tamanho do banco em páginas (zero se desconhecido).
    pub fn dbsize(&self) -> u32 {
        if self.read_lock >= 0 {
            return self.hdr.n_page;
        }
        0
    }

    // -----------------------------------------------------------------------
    // Transação de escrita.
    // -----------------------------------------------------------------------

    /// `sqlite3WalBeginWriteTransaction`: começa a escrever. Uma transação de leitura
    /// precisa estar aberta. Se outro escritor mexeu no banco desde então, devolve
    /// `SQLITE_BUSY_SNAPSHOT`.
    pub fn begin_write_transaction(&mut self, db_fd: &mut dyn VfsFile) -> i32 {
        debug_assert!(self.read_lock >= 0);
        debug_assert!(!self.write_lock && self.i_re_cksum == 0);

        if self.read_only != 0 {
            return SQLITE_READONLY;
        }

        // Um escritor por vez.
        let mut rc = self.wal_lock_exclusive(db_fd, WAL_WRITE_LOCK, 1);
        if rc != SQLITE_OK {
            return rc;
        }
        self.write_lock = true;

        // Se outra conexão escreveu desde que a leitura começou, escrever aqui criaria
        // uma bifurcação: proibido.
        match self.wal_index_hdr(db_fd, 0) {
            Ok(live) => {
                if self.hdr != live {
                    rc = SQLITE_BUSY_SNAPSHOT;
                }
            }
            Err(e) => rc = e,
        }

        if rc != SQLITE_OK {
            self.wal_unlock_exclusive(db_fd, WAL_WRITE_LOCK, 1);
            self.write_lock = false;
        }
        rc
    }

    /// `sqlite3WalEndWriteTransaction`: termina a escrita (o commit já foi feito; só
    /// solta a trava).
    pub fn end_write_transaction(&mut self, db_fd: &mut dyn VfsFile) -> i32 {
        if self.write_lock {
            self.wal_unlock_exclusive(db_fd, WAL_WRITE_LOCK, 1);
            self.write_lock = false;
            self.i_re_cksum = 0;
            self.truncate_on_commit = false;
        }
        SQLITE_OK
    }

    /// `sqlite3WalUndo`: desfaz a escrita não confirmada no log, voltando o ponteiro de
    /// escrita ao início da transação. `undo` é chamado com o número de página de cada
    /// quadro escrito desde então; se devolver diferente de `SQLITE_OK`, não é chamado
    /// de novo e o código é devolvido.
    ///
    /// Em duas fases, porque o `xUndo` do pager (`pagerUndoCallback`) usa o próprio `Wal`
    /// (`readDbPage`) e não pode rodar com ele emprestado: `undo_begin` restaura o cabeçalho
    /// e devolve o número de página de cada quadro desfeito, na ordem do C, junto do
    /// `mxFrame` anterior; o pager chama o callback para cada um e termina com `undo_end`.
    pub fn undo_begin(&mut self, db_fd: &mut dyn VfsFile) -> Result<(Vec<u32>, u32), i32> {
        let mut pgnos = Vec::new();
        let mut i_max = 0;
        if self.write_lock {
            i_max = self.hdr.mx_frame;

            // Restaura o cache do cabeçalho ao estado anterior à escrita.
            self.hdr = self.wal_index_hdr(db_fd, 0)?;

            let mut i_frame = self.hdr.mx_frame.wrapping_add(1);
            while i_frame <= i_max {
                let pgno = self.wal_frame_pgno(db_fd, i_frame)?;
                debug_assert!(pgno != 1);
                pgnos.push(pgno);
                i_frame += 1;
            }
        }
        Ok((pgnos, i_max))
    }

    /// Segunda fase de `sqlite3WalUndo`: limpa o hash se quadros foram desfeitos. Devolve o
    /// código de erro da limpeza, ou `SQLITE_OK`.
    pub fn undo_end(&mut self, db_fd: &mut dyn VfsFile, i_max: u32) -> i32 {
        if self.write_lock && i_max != self.hdr.mx_frame {
            if let Err(e) = self.wal_cleanup_hash(db_fd) {
                return e;
            }
        }
        SQLITE_OK
    }

    /// `sqlite3WalSavepoint`: preenche `a_wal_data` com os valores que permitem voltar
    /// a posição de escrita ao ponto atual (`savepoint_undo`).
    pub fn savepoint(&self, a_wal_data: &mut [u32; WAL_SAVEPOINT_NDATA]) {
        debug_assert!(self.write_lock);
        a_wal_data[0] = self.hdr.mx_frame;
        a_wal_data[1] = self.hdr.a_frame_cksum[0];
        a_wal_data[2] = self.hdr.a_frame_cksum[1];
        a_wal_data[3] = self.n_ckpt;
    }

    /// `sqlite3WalSavepointUndo`: volta a posição de escrita ao ponto guardado em
    /// `a_wal_data` por `savepoint`.
    pub fn savepoint_undo(&mut self, db_fd: &mut dyn VfsFile, a_wal_data: &mut [u32; WAL_SAVEPOINT_NDATA]) -> i32 {
        let mut rc = SQLITE_OK;

        debug_assert!(self.write_lock);
        debug_assert!(a_wal_data[3] != self.n_ckpt || a_wal_data[0] <= self.hdr.mx_frame);

        if a_wal_data[3] != self.n_ckpt {
            // O savepoint abriu logo depois do início da transação e em seguida o
            // escritor deu a volta para o começo do log: ajusta os valores.
            a_wal_data[0] = 0;
            a_wal_data[3] = self.n_ckpt;
        }

        if a_wal_data[0] < self.hdr.mx_frame {
            self.hdr.mx_frame = a_wal_data[0];
            self.hdr.a_frame_cksum[0] = a_wal_data[1];
            self.hdr.a_frame_cksum[1] = a_wal_data[2];
            if let Err(e) = self.wal_cleanup_hash(db_fd) {
                rc = e;
            }
        }
        rc
    }

    /// `walRestartLog`: antes de escrever quadros, vê se dá para sobrescrever o início do
    /// log em vez de anexar (reinício). Se der, `hdr.mxFrame` vira 0.
    fn wal_restart_log(&mut self, db_fd: &mut dyn VfsFile) -> i32 {
        let mut rc = SQLITE_OK;

        if self.read_lock == 0 {
            let n_backfill = tri!(self.ckpt_backfill(db_fd));
            debug_assert!(n_backfill == self.hdr.mx_frame);
            if n_backfill > 0 {
                let mut b = [0u8; 4];
                (self.hooks.randomness)(&mut b);
                let salt1 = u32::from_ne_bytes(b);
                rc = self.wal_lock_exclusive(db_fd, wal_read_lock(1), WAL_NREADER - 1);
                if rc == SQLITE_OK {
                    // Todos os leitores estão na WAL_READ_LOCK(0) (ninguém usa o WAL):
                    // os quadros da transação sobrescrevem o começo do log. Atualiza o
                    // cabeçalho do índice (assim `undo` não tem caso especial).
                    let r = self.wal_restart_hdr(db_fd, salt1);
                    self.wal_unlock_exclusive(db_fd, wal_read_lock(1), WAL_NREADER - 1);
                    if let Err(e) = r {
                        return e;
                    }
                } else if rc != SQLITE_BUSY {
                    return rc;
                }
            }
            self.wal_unlock_shared(db_fd, wal_read_lock(0));
            self.read_lock = -1;
            let mut cnt = 0i32;
            loop {
                let mut not_used = 0i32;
                rc = self.wal_try_begin_read(db_fd, &mut not_used, true, &mut cnt);
                if rc != WAL_RETRY {
                    break;
                }
            }
            debug_assert!((rc & 0xff) != SQLITE_BUSY);
        }
        rc
    }

    /// `walWriteToLog`: grava `content` no arquivo do WAL a partir de `i_offset`, com
    /// fsync ao cruzar `w.i_sync_point`: grava a parte anterior, sincroniza, grava o resto.
    fn wal_write_to_log(&mut self, w: &WalWriter, content: &[u8], mut i_offset: i64) -> i32 {
        let i_amt = content.len() as i64;
        let mut rest = content;
        if i_offset < w.i_sync_point && i_offset + i_amt >= w.i_sync_point {
            let i_first_amt = (w.i_sync_point - i_offset) as usize;
            let rc = self.wal_fd.write(&rest[..i_first_amt], i_offset);
            if rc != SQLITE_OK {
                return rc;
            }
            i_offset += i_first_amt as i64;
            rest = &rest[i_first_amt..];
            debug_assert!(wal_sync_flags(w.sync_flags) != 0);
            let rc = os_sync(&mut *self.wal_fd, wal_sync_flags(w.sync_flags));
            if rest.is_empty() || rc != SQLITE_OK {
                return rc;
            }
        }
        self.wal_fd.write(rest, i_offset)
    }

    /// `walWriteOneFrame`: grava um quadro do WAL (cabeçalho e página).
    fn wal_write_one_frame(&mut self, w: &WalWriter, page: &WalPageRef<'_>, n_truncate: u32, i_offset: i64) -> i32 {
        let mut a_frame = [0u8; WAL_FRAME_HDRSIZE];
        let data = &page.data[..w.sz_page];
        self.wal_encode_frame(page.pgno, n_truncate, data, &mut a_frame);
        let rc = self.wal_write_to_log(w, &a_frame, i_offset);
        if rc != SQLITE_OK {
            return rc;
        }
        self.wal_write_to_log(w, data, i_offset + WAL_FRAME_HDRSIZE as i64)
    }

    /// `walRewriteChecksums`: no commit de transação que sobrescreveu quadros, refaz os
    /// checksums de todos os quadros escritos a partir do primeiro sobrescrito, até
    /// `i_last`.
    fn wal_rewrite_checksums(&mut self, i_last: u32) -> i32 {
        let sz_page = self.sz_page as usize;
        let mut rc: i32;
        let mut a_buf = vec![0u8; sz_page + WAL_FRAME_HDRSIZE];
        let mut a_frame = [0u8; WAL_FRAME_HDRSIZE];

        // Valores de checksum de entrada do primeiro quadro: se ele é o quadro 1 (a
        // transação reiniciou o WAL) vêm do cabeçalho do WAL; senão, do cabeçalho do
        // quadro anterior.
        debug_assert!(self.i_re_cksum > 0);
        let i_cksum_off: i64 = if self.i_re_cksum == 1 {
            24
        } else {
            wal_frame_offset(self.i_re_cksum - 1, sz_page as i32) + 16
        };
        rc = self.wal_fd.read(&mut a_buf[..8], i_cksum_off);
        self.hdr.a_frame_cksum[0] = get4byte(&a_buf[0..]);
        self.hdr.a_frame_cksum[1] = get4byte(&a_buf[4..]);

        let mut i_read = self.i_re_cksum;
        self.i_re_cksum = 0;
        while rc == SQLITE_OK && i_read <= i_last {
            let i_off = wal_frame_offset(i_read, sz_page as i32);
            rc = self.wal_fd.read(&mut a_buf, i_off);
            if rc == SQLITE_OK {
                let i_pgno = get4byte(&a_buf[0..]);
                let n_db_size = get4byte(&a_buf[4..]);
                self.wal_encode_frame(i_pgno, n_db_size, &a_buf[WAL_FRAME_HDRSIZE..], &mut a_frame);
                rc = self.wal_fd.write(&a_frame, i_off);
            }
            i_read += 1;
        }
        rc
    }

    /// `sqlite3WalFrames`: grava um conjunto de quadros no log. O chamador segura a trava
    /// de escrita (`begin_write_transaction`). `pages` é a lista de páginas sujas, na
    /// ordem em que são gravadas; `n_truncate` é o tamanho do banco depois do commit (0
    /// se `is_commit` é falso).
    pub fn frames(
        &mut self,
        db_fd: &mut dyn VfsFile,
        sz_page: i32,
        pages: &[WalPageRef<'_>],
        n_truncate: u32,
        is_commit: bool,
        sync_flags: i32,
    ) -> i32 {
        let mut rc: i32;
        let n_pages = pages.len();
        let mut p_last: Option<usize> = None; // último quadro gravado
        let mut n_extra: u32 = 0; // cópias extras da última página
        let mut i_first: u32 = 0; // primeiro quadro que pode ser sobrescrito

        debug_assert!(n_pages > 0);
        debug_assert!(self.write_lock);
        debug_assert!(is_commit == (n_truncate != 0));

        let live = tri!(self.wal_index_hdr(db_fd, 0));
        if self.hdr != live {
            i_first = live.mx_frame.wrapping_add(1);
        }

        // Dá para gravar no início do log em vez de anexar?
        rc = self.wal_restart_log(db_fd);
        if rc != SQLITE_OK {
            return rc;
        }

        // Primeiro quadro do log: grava o cabeçalho do WAL no início do arquivo.
        let mut i_frame = self.hdr.mx_frame;
        if i_frame == 0 {
            let mut a_wal_hdr = [0u8; WAL_HDRSIZE];
            put4byte(&mut a_wal_hdr[0..], WAL_MAGIC | SQLITE_BIGENDIAN as u32);
            put4byte(&mut a_wal_hdr[4..], WAL_MAX_VERSION);
            put4byte(&mut a_wal_hdr[8..], sz_page as u32);
            put4byte(&mut a_wal_hdr[12..], self.n_ckpt);
            if self.n_ckpt == 0 {
                let mut b = [0u8; 8];
                (self.hooks.randomness)(&mut b);
                self.hdr.a_salt = [ne32(&b, 0), ne32(&b, 4)];
            }
            a_wal_hdr[16..24].copy_from_slice(&self.hdr.salt_bytes());
            let a_cksum = wal_checksum_bytes(true, &a_wal_hdr, WAL_HDRSIZE - 2 * 4, None);
            put4byte(&mut a_wal_hdr[24..], a_cksum[0]);
            put4byte(&mut a_wal_hdr[28..], a_cksum[1]);

            self.sz_page = sz_page as u32;
            self.hdr.big_end_cksum = SQLITE_BIGENDIAN as u8;
            self.hdr.a_frame_cksum = a_cksum;
            self.truncate_on_commit = true;

            rc = self.wal_fd.write(&a_wal_hdr, 0);
            if rc != SQLITE_OK {
                return rc;
            }

            // Sincroniza o cabeçalho (salvo SQLITE_IOCAP_SEQUENTIAL ou synchronous=OFF);
            // senão uma escrita fora de ordem após o reinício do WAL poderia corromper o
            // banco.
            if self.sync_header {
                rc = os_sync(&mut *self.wal_fd, ckpt_sync_flags(sync_flags));
                if rc != SQLITE_OK {
                    return rc;
                }
            }
        }
        if self.sz_page != sz_page as u32 {
            return SQLITE_CORRUPT_BKPT;
        }

        // Prepara o escritor.
        let mut w = WalWriter { i_sync_point: 0, sync_flags, sz_page: sz_page as usize };
        let mut i_offset = wal_frame_offset(i_frame.wrapping_add(1), sz_page);
        let sz_frame = (sz_page as usize + WAL_FRAME_HDRSIZE) as i64;

        // Grava todos os quadros exatamente uma vez.
        let mut append = vec![false; n_pages];
        for k in 0..n_pages {
            let p = &pages[k];
            let is_last = k + 1 == n_pages;

            // Página já gravada no WAL por esta transação: sobrescreve o quadro e marca
            // que os checksums terão de ser refeitos no commit.
            if i_first != 0 && (!is_last || !is_commit) {
                let mut i_write = 0u32;
                let _ = self.wal_find_frame(db_fd, p.pgno, &mut i_write);
                if i_write >= i_first {
                    let i_off = wal_frame_offset(i_write, sz_page) + WAL_FRAME_HDRSIZE as i64;
                    if self.i_re_cksum == 0 || i_write < self.i_re_cksum {
                        self.i_re_cksum = i_write;
                    }
                    rc = self.wal_fd.write(&p.data[..sz_page as usize], i_off);
                    if rc != SQLITE_OK {
                        return rc;
                    }
                    append[k] = false;
                    continue;
                }
            }

            i_frame = i_frame.wrapping_add(1);
            debug_assert!(i_offset == wal_frame_offset(i_frame, sz_page));
            let n_db_size = if is_commit && is_last { n_truncate } else { 0 };
            rc = self.wal_write_one_frame(&w, p, n_db_size, i_offset);
            if rc != SQLITE_OK {
                return rc;
            }
            p_last = Some(k);
            i_offset += sz_frame;
            append[k] = true;
        }

        // Refaz os checksums dentro do arquivo, se preciso.
        if is_commit && self.i_re_cksum != 0 {
            rc = self.wal_rewrite_checksums(i_frame);
            if rc != SQLITE_OK {
                return rc;
            }
        }

        // Fim de transação: pode haver preenchimento (padding) e fsync, só com
        // synchronous=FULL. Com padding, o último quadro é repetido (com a marca de
        // commit) até cruzar o limite do setor; só a parte antes do limite é
        // sincronizada, e o que passa dele é gravado depois do sync.
        if is_commit && wal_sync_flags(sync_flags) != 0 {
            let mut b_sync = true;
            if self.pad_to_sector_boundary {
                let sector = sector_size(&mut *self.wal_fd) as i64;
                w.i_sync_point = ((i_offset + sector - 1) / sector) * sector;
                b_sync = w.i_sync_point == i_offset;
                while i_offset < w.i_sync_point {
                    let last = match p_last {
                        Some(l) => l,
                        None => return SQLITE_CORRUPT_BKPT,
                    };
                    rc = self.wal_write_one_frame(&w, &pages[last], n_truncate, i_offset);
                    if rc != SQLITE_OK {
                        return rc;
                    }
                    i_offset += sz_frame;
                    n_extra += 1;
                }
            }
            if b_sync {
                debug_assert!(rc == SQLITE_OK);
                rc = os_sync(&mut *self.wal_fd, wal_sync_flags(sync_flags));
            }
        }

        // Fecha a primeira transação do WAL com journal_size_limit: trunca o WAL ao
        // limite, se possível.
        if is_commit && self.truncate_on_commit && self.mx_wal_size >= 0 {
            let mut sz = self.mx_wal_size;
            let end = wal_frame_offset(i_frame.wrapping_add(n_extra).wrapping_add(1), sz_page);
            if end > self.mx_wal_size {
                sz = end;
            }
            self.wal_limit_size(sz);
            self.truncate_on_commit = false;
        }

        // Anexa os dados ao índice. Não precisa travar o índice: a trava de escrita
        // garante que não há outros escritores e que nada em uso por leitor é
        // sobrescrito.
        i_frame = self.hdr.mx_frame;
        for k in 0..n_pages {
            if rc != SQLITE_OK {
                break;
            }
            if !append[k] {
                continue;
            }
            i_frame = i_frame.wrapping_add(1);
            rc = self.wal_index_append(db_fd, i_frame, pages[k].pgno);
        }
        debug_assert!(p_last.is_some() || n_extra == 0);
        while rc == SQLITE_OK && n_extra > 0 {
            let last = match p_last {
                Some(l) => l,
                None => return SQLITE_CORRUPT_BKPT,
            };
            i_frame = i_frame.wrapping_add(1);
            n_extra -= 1;
            rc = self.wal_index_append(db_fd, i_frame, pages[last].pgno);
        }

        if rc == SQLITE_OK {
            // Atualiza a cópia privada do cabeçalho.
            let sp = sz_page as u32;
            self.hdr.sz_page = ((sp & 0xff00) | (sp >> 16)) as u16;
            self.hdr.mx_frame = i_frame;
            if is_commit {
                self.hdr.i_change = self.hdr.i_change.wrapping_add(1);
                self.hdr.n_page = n_truncate;
            }
            // Commit: atualiza também o cabeçalho do índice.
            if is_commit {
                if let Err(e) = self.wal_index_write_hdr(db_fd) {
                    return e;
                }
                self.i_callback = i_frame;
            }
        }
        rc
    }

    /// `sqlite3WalCheckpoint`: implementa `sqlite3_wal_checkpoint()` e afins. Pega a
    /// trava de checkpoint e copia o que puder do WAL para o banco. `busy` (o
    /// `xBusy`+`pBusyArg`) faz o checkpoint ser bloqueante. `interrupt` consulta o
    /// sinal de interrupção da conexão (0 = seguir; senão, o código a devolver). `buf` é
    /// o buffer de uma página (`zBuf`, com `nBuf = buf.len()`). `pn_log` e `pn_ckpt`
    /// recebem o número de quadros no WAL e o de quadros copiados.
    #[allow(clippy::too_many_arguments)]
    pub fn checkpoint(
        &mut self,
        db_fd: &mut dyn VfsFile,
        interrupt: &mut dyn FnMut() -> i32,
        e_mode: i32,
        busy: Option<&mut dyn FnMut() -> i32>,
        sync_flags: i32,
        buf: &mut [u8],
        pn_log: Option<&mut i32>,
        pn_ckpt: Option<&mut i32>,
    ) -> i32 {
        let mut rc: i32;
        let mut is_changed = 0i32; // um novo cabeçalho do índice foi carregado
        let mut e_mode2 = e_mode; // modo passado a wal_checkpoint
        let mut busy2 = busy; // busy handler de e_mode2

        debug_assert!(!self.ckpt_lock);
        debug_assert!(!self.write_lock);
        debug_assert!(e_mode != SQLITE_CHECKPOINT_PASSIVE || busy2.is_none());

        if self.read_only != 0 {
            return SQLITE_READONLY;
        }

        // Todas as chamadas pegam a trava exclusiva de "checkpoint" do banco. Com outro
        // checkpoint em curso a trava falha com BUSY, sem chamar o busy handler.
        rc = self.wal_lock_exclusive(db_fd, WAL_CKPT_LOCK, 1);
        if rc == SQLITE_OK {
            self.ckpt_lock = true;

            // FULL, RESTART e TRUNCATE também pegam a trava de "escritor"; com o busy
            // handler, ele é chamado e a trava repetida até o handler devolver 0 ou a
            // trava ser obtida.
            if e_mode != SQLITE_CHECKPOINT_PASSIVE {
                rc = self.wal_busy_lock(db_fd, &mut busy2, WAL_WRITE_LOCK, 1);
                if rc == SQLITE_OK {
                    self.write_lock = true;
                } else if rc == SQLITE_BUSY {
                    e_mode2 = SQLITE_CHECKPOINT_PASSIVE;
                    busy2 = None;
                    rc = SQLITE_OK;
                }
            }
        }

        // Lê o cabeçalho do índice.
        if rc == SQLITE_OK {
            rc = self.wal_index_read_hdr(db_fd, &mut is_changed);
            if is_changed != 0 && db_fd.i_version() >= 3 {
                let _ = db_fd.unfetch(0, None);
            }
        }

        // Copia dados do log para o arquivo do banco.
        if rc == SQLITE_OK {
            if self.hdr.mx_frame != 0 && self.wal_pagesize() as usize != buf.len() {
                rc = SQLITE_CORRUPT_BKPT;
            } else {
                rc = self.wal_checkpoint(db_fd, interrupt, e_mode2, &mut busy2, sync_flags, buf);
            }

            // Sem erro, preenche as saídas.
            if rc == SQLITE_OK || rc == SQLITE_BUSY {
                if let Some(p) = pn_log {
                    *p = self.hdr.mx_frame as i32;
                }
                match self.ckpt_backfill(db_fd) {
                    Ok(v) => {
                        if let Some(p) = pn_ckpt {
                            *p = v as i32;
                        }
                    }
                    Err(e) => rc = e,
                }
            }
        }

        if is_changed != 0 {
            // Um novo cabeçalho foi carregado antes do checkpoint: o cache do pager está
            // desatualizado. Zera o cabeçalho em cache para que o próximo retrato saiba
            // que o cache precisa ser reiniciado.
            self.hdr = WalIndexHdr::default();
        }

        // Solta as travas.
        self.end_write_transaction(db_fd);
        if self.ckpt_lock {
            self.wal_unlock_exclusive(db_fd, WAL_CKPT_LOCK, 1);
            self.ckpt_lock = false;
        }
        if rc == SQLITE_OK && e_mode != e_mode2 {
            SQLITE_BUSY
        } else {
            rc
        }
    }

    /// `sqlite3WalCallback`: valor para o callback do `sqlite3_wal_hook`, o número de
    /// quadros no WAL no último commit desde a chamada anterior (0 se não houve commit).
    pub fn callback(&mut self) -> i32 {
        let ret = self.i_callback;
        self.i_callback = 0;
        ret as i32
    }

    /// `sqlite3WalExclusiveMode`: entra ou sai de `locking_mode=EXCLUSIVE`. `op == 0`
    /// tenta sair (pega a trava de leitura; devolve 1 se saiu); `op > 0` entra (solta a
    /// trava de leitura; devolve 1); `op < 0` é ensaio do caso `op == 1`.
    pub fn exclusive_mode(&mut self, db_fd: &mut dyn VfsFile, op: i32) -> i32 {
        let rc: i32;
        debug_assert!(!self.write_lock);
        debug_assert!(self.exclusive_mode != WAL_HEAPMEMORY_MODE || op == -1);
        debug_assert!(self.read_lock >= 0 || (op <= 0 && self.exclusive_mode == 0));

        if op == 0 {
            if self.exclusive_mode != WAL_NORMAL_MODE {
                self.exclusive_mode = WAL_NORMAL_MODE;
                if self.wal_lock_shared(db_fd, wal_read_lock(self.read_lock as i32)) != SQLITE_OK {
                    self.exclusive_mode = WAL_EXCLUSIVE_MODE;
                }
                rc = (self.exclusive_mode == WAL_NORMAL_MODE) as i32;
            } else {
                // Já em locking_mode=NORMAL.
                rc = 0;
            }
        } else if op > 0 {
            debug_assert!(self.exclusive_mode == WAL_NORMAL_MODE);
            debug_assert!(self.read_lock >= 0);
            self.wal_unlock_shared(db_fd, wal_read_lock(self.read_lock as i32));
            self.exclusive_mode = WAL_EXCLUSIVE_MODE;
            rc = 1;
        } else {
            rc = (self.exclusive_mode == WAL_NORMAL_MODE) as i32;
        }
        rc
    }

    /// `sqlite3WalHeapMemory`: o índice do WAL está em memória do heap?
    pub fn heap_memory(&self) -> bool {
        self.exclusive_mode == WAL_HEAPMEMORY_MODE
    }

    /// `sqlite3WalFile`: o arquivo do WAL.
    pub fn wal_file(&mut self) -> &mut dyn VfsFile {
        &mut *self.wal_fd
    }
}

/// Se o resultado é erro, grava o código em `rc`.
fn tri_set(rc: &mut i32, r: Result<(), i32>) {
    if let Err(e) = r {
        *rc = e;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consts::SQLITE_IOERR_SHORT_READ;
    use crate::os::Vfs;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    type Shared = Arc<Mutex<Vec<u8>>>;

    struct MemFile {
        data: Shared,
    }

    impl VfsFile for MemFile {
        fn close(&mut self) -> i32 {
            SQLITE_OK
        }
        fn read(&mut self, buf: &mut [u8], offset: i64) -> i32 {
            let d = self.data.lock().unwrap();
            let off = offset as usize;
            let avail = d.len().saturating_sub(off).min(buf.len());
            buf[..avail].copy_from_slice(&d[off..off + avail]);
            if avail < buf.len() {
                buf[avail..].fill(0);
                return SQLITE_IOERR_SHORT_READ;
            }
            SQLITE_OK
        }
        fn write(&mut self, buf: &[u8], offset: i64) -> i32 {
            let mut d = self.data.lock().unwrap();
            let end = offset as usize + buf.len();
            if d.len() < end {
                d.resize(end, 0);
            }
            d[offset as usize..end].copy_from_slice(buf);
            SQLITE_OK
        }
        fn truncate(&mut self, size: i64) -> i32 {
            self.data.lock().unwrap().resize(size as usize, 0);
            SQLITE_OK
        }
        fn sync(&mut self, _flags: i32) -> i32 {
            SQLITE_OK
        }
        fn file_size(&mut self, size: &mut i64) -> i32 {
            *size = self.data.lock().unwrap().len() as i64;
            SQLITE_OK
        }
        fn lock(&mut self, _t: i32) -> i32 {
            SQLITE_OK
        }
        fn unlock(&mut self, _t: i32) -> i32 {
            SQLITE_OK
        }
        fn check_reserved_lock(&mut self, r: &mut i32) -> i32 {
            *r = 0;
            SQLITE_OK
        }
        fn file_control(&mut self, _op: i32, _arg: &mut FileControlArg) -> i32 {
            crate::consts::SQLITE_NOTFOUND
        }
        fn device_characteristics(&mut self) -> i32 {
            0
        }
    }

    struct MemVfs {
        files: Mutex<HashMap<Vec<u8>, Shared>>,
    }

    impl Vfs for MemVfs {
        fn name(&self) -> &[u8] {
            b"wal-test-mem"
        }
        fn max_pathname(&self) -> i32 {
            512
        }
        fn open(&self, name: Option<&[u8]>, flags: i32, out_flags: &mut i32) -> Result<Box<dyn VfsFile>, i32> {
            let key = name.unwrap_or(b"").to_vec();
            let mut files = self.files.lock().unwrap();
            let data = files.entry(key).or_default().clone();
            *out_flags = flags;
            Ok(Box::new(MemFile { data }))
        }
        fn delete(&self, name: &[u8], _sync_dir: i32) -> i32 {
            self.files.lock().unwrap().remove(name);
            SQLITE_OK
        }
        fn access(&self, name: &[u8], _flags: i32, res_out: &mut i32) -> i32 {
            *res_out = self.files.lock().unwrap().contains_key(name) as i32;
            SQLITE_OK
        }
        fn full_pathname(&self, name: &[u8], _n_out: i32, out: &mut Vec<u8>) -> i32 {
            out.extend_from_slice(name);
            SQLITE_OK
        }
        fn randomness(&self, out: &mut [u8]) -> i32 {
            out.fill(0x5a);
            SQLITE_OK
        }
        fn sleep(&self, micro: i32) -> i32 {
            micro
        }
        fn current_time(&self, out: &mut f64) -> i32 {
            *out = 2.0;
            SQLITE_OK
        }
    }

    fn test_random(out: &mut [u8]) {
        for (i, b) in out.iter_mut().enumerate() {
            *b = 0x11u8.wrapping_mul(i as u8 + 1);
        }
    }

    fn test_log(_code: i32, _msg: &[u8]) {}

    #[test]
    fn header_roundtrip_and_checksum() {
        let h = WalIndexHdr {
            i_version: WALINDEX_MAX_VERSION,
            unused: 0,
            i_change: 7,
            is_init: 1,
            big_end_cksum: 0,
            sz_page: 512,
            mx_frame: 9,
            n_page: 3,
            a_frame_cksum: [1, 2],
            a_salt: [3, 4],
            a_cksum: [5, 6],
        };
        assert_eq!(WalIndexHdr::from_bytes(&h.to_bytes()), h);
        let a = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let c1 = wal_checksum_bytes(true, &a, 16, None);
        let first = wal_checksum_bytes(true, &a[..8], 8, None);
        let c2 = wal_checksum_bytes(true, &a[8..], 8, Some(first));
        assert_eq!(c1, c2);
    }

    #[test]
    fn mergesort_dedups_keeping_larger_index() {
        // content[i] é a página do quadro i; a página 7 aparece em 1 e 4.
        let content = [9u32, 7, 3, 5, 7, 1];
        let mut list: Vec<u16> = (0..content.len() as u16).collect();
        let mut tmp = vec![0u16; 16];
        let mut n = content.len();
        wal_mergesort(&content, &mut tmp, &mut list, &mut n);
        let sorted: Vec<u32> = list[..n].iter().map(|&i| content[i as usize]).collect();
        assert_eq!(sorted, vec![1, 3, 5, 7, 9]);
        assert!(list[..n].contains(&4) && !list[..n].contains(&1));
    }

    #[test]
    fn heap_mode_full_cycle() {
        let vfs: VfsRef = Arc::new(MemVfs { files: Mutex::new(HashMap::new()) });
        let db_data: Shared = Arc::new(Mutex::new(Vec::new()));
        let mut db = MemFile { data: db_data.clone() };
        let hooks = WalHooks { randomness: test_random, log: test_log };
        let mut wal = Wal::open(&vfs, &mut db, b"t.db-wal", true, -1, hooks).unwrap();
        assert!(wal.heap_memory());

        let mut intr = || 0i32;
        let mut changed = 0;
        assert_eq!(wal.begin_read_transaction(&mut db, &mut changed), SQLITE_OK);
        assert_eq!(wal.begin_write_transaction(&mut db), SQLITE_OK);

        let p1 = vec![0xABu8; 512];
        let p2 = vec![0xCDu8; 512];
        let pages = [
            WalPageRef { pgno: 1, data: &p1, flags: 0 },
            WalPageRef { pgno: 2, data: &p2, flags: 0 },
        ];
        assert_eq!(wal.frames(&mut db, 512, &pages, 2, true, 0), SQLITE_OK);
        assert_eq!(wal.callback(), 2);
        assert_eq!(wal.end_write_transaction(&mut db), SQLITE_OK);
        wal.end_read_transaction(&mut db);

        assert_eq!(wal.begin_read_transaction(&mut db, &mut changed), SQLITE_OK);
        assert_eq!(wal.dbsize(), 2);
        let mut i_read = 0u32;
        assert_eq!(wal.find_frame(&mut db, 2, &mut i_read), SQLITE_OK);
        assert_eq!(i_read, 2);
        let mut out = vec![0u8; 512];
        assert_eq!(wal.read_frame(i_read, &mut out), SQLITE_OK);
        assert_eq!(out, p2);
        assert_eq!(wal.find_frame(&mut db, 9, &mut i_read), SQLITE_OK);
        assert_eq!(i_read, 0);
        wal.end_read_transaction(&mut db);

        let mut buf = vec![0u8; 512];
        let (mut n_log, mut n_ckpt) = (0i32, 0i32);
        let rc = wal.checkpoint(
            &mut db,
            &mut intr,
            SQLITE_CHECKPOINT_PASSIVE,
            None,
            0,
            &mut buf,
            Some(&mut n_log),
            Some(&mut n_ckpt),
        );
        assert_eq!(rc, SQLITE_OK);
        assert_eq!((n_log, n_ckpt), (2, 2));
        {
            let d = db_data.lock().unwrap();
            assert_eq!(&d[..512], &p1[..]);
            assert_eq!(&d[512..1024], &p2[..]);
        }
        assert_eq!(wal.close(&mut db, &mut intr, 0, Some(&mut buf[..])), SQLITE_OK);
    }
}
