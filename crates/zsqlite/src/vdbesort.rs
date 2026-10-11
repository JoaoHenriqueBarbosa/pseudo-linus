//! `VdbeSorter` (vdbesort.c): o ordenador externo por mesclagem que o VDBE usa em `CREATE INDEX`
//! e em `ORDER BY` sem índice.
//!
//! Algoritmo, interface e formato do PMA são os do C: os registros ficam em memória até passar de
//! `mx_pma_size`; aí a lista é ordenada e gravada num PMA do arquivo temporário; no `rewind` o
//! resto da lista vira o último PMA e uma árvore de `MergeEngine`/`IncrMerger`/`PmaReader`
//! devolve os registros em ordem. A ordem das linhas, a ordem dos PMAs e o desempate (o mais
//! antigo vence, a mesclagem de listas prefere a lista da esquerda) são idênticos ao C.
//!
//! # Threads
//!
//! O ordenador só usa threads quando o limite `SQLITE_LIMIT_WORKER_THREADS` (padrão 0, e o sqlite3
//! do Debian não o muda) é maior que zero. Aqui `nWorker` vale sempre 0 (`N_WORKER`): há uma única
//! `SortSubtask`, `b_use_threads` é sempre falso e o caminho de threads do C (`vdbeSorterRunThread`,
//! `vdbeSorterCreateThread`, `vdbeIncrBgPopulate`, `vdbeIncrMergerSetThreads`, `INCRINIT_TASK` e
//! `INCRINIT_ROOT`, o `pReader` multi-thread, `IncrMerger.bUseThread` com dois arquivos próprios)
//! não existe. É o mesmo que o C faz com `nWorker == 0`. Os campos `p_reader`, `b_use_threads`,
//! `b_done` e `IncrMerger.b_use_thread` ficam na estrutura (sempre `None`/`false`) para manter a
//! forma do C. `vdbeSorterJoinThread` e `vdbeSorterJoinAll` são macros que valem `SQLITE_OK` e
//! `rcin` no C com `nWorker == 0`: ficam como comentário no ponto de chamada. O `eMode` de
//! `vdbeMergeEngineInit` e `vdbePmaReaderIncrMergeInit` é sempre `INCRINIT_NORMAL` e não é
//! parâmetro.
//!
//! # Modelo (CONVENTIONS.md, v2)
//!
//! * Sem ponteiros: o `sqlite3_file *` compartilhado entre `SortSubtask.file`, `file2`,
//!   `PmaReader.pFd` e `IncrMerger.aFile[1]` vira um índice (`FileId`) no vetor `files` do próprio
//!   `VdbeSorter` (o ordenador é o dono de todos os arquivos temporários). Fechar o arquivo é pôr
//!   `None` na posição; `vdbe_sorter_reset` zera o vetor no fim.
//! * `SortSubtask.pSorter` não existe: as funções recebem `&mut VdbeSorter` e o índice da tarefa.
//!   `MergeEngine.pTask` e `IncrMerger.pTask` são índices em `a_task`.
//! * A árvore `MergeEngine` (dona dos `PmaReader`, que podem ser donos de um `IncrMerger`, que é
//!   dono de outro `MergeEngine`) é possuída por valor. Quem a percorre a retira do sorter
//!   (`Option::take`) enquanto trabalha e a devolve: assim os arquivos, as tarefas e o
//!   `KeyInfo` (campos do sorter) ficam livres para o empréstimo mutável.
//! * Cada `PmaReader` guarda a chave corrente como posição (`KeyLoc`: mapa, buffer ou `a_alloc`)
//!   mais `n_key`; `PmaReader::key()` devolve a fatia. O `aMap` do C (`xFetch`) é uma cópia
//!   (`Option<Vec<u8>>`), como o trait `VfsFile::fetch` devolve; com o `os_unix` o mapeamento está
//!   desligado e `a_map` fica `None`.
//! * A lista em memória (`SorterList`) é uma arena: `records` (o cabeçalho `SorterRecord` com
//!   `n_val` e o elo), `data` (os bytes de todos os registros, um depois do outro) e `p_list` (o
//!   índice da cabeça). O elo `u.pNext`/`u.iNext` é `p_next`. O modo de memória em bloco do C
//!   (`list.aMemory != 0`) vira `a_memory: bool` mais a contabilidade `i_memory`/`n_memory`: o
//!   limiar de descarga e o crescimento (`nNew`) usam os MESMOS números do C, com
//!   `sizeof(SorterRecord) == 16` (LP64) e `ROUND8`, de modo que os PMAs saiam nos mesmos pontos.
//!   Os registros liberados um a um (`vdbeSorterRecordFree`) são só descartados com a arena.
//! * `sqlite3 *db` é substituído pelo que o sorter usa depois do `init`: `p_vfs` (o `db->pVfs`) e
//!   `n_max_sorter_mmap` (o `db->nMaxSorterMmap`), copiados na criação.
//! * Sem OOM: `sqlite3FaultSim`, `sqlite3Malloc` que falha e `vdbeMergeEngineNew`/
//!   `vdbeIncrMergerNew` com `rc` viram funções que não falham.
//! * `sqlite3GlobalConfig.szPma` (`SQLITE_CONFIG_PMASZ`) e `bSmallMalloc` ainda não têm módulo
//!   de configuração: valem os padrões (`SORTER_PMASZ` = 250, `bSmallMalloc` = 0).
//!
//! Nomes: o `vdbeSorterRowkey` estático do C é `vdbe_sorter_rowkey_bytes` e o `vdbeSorterCompare`
//! estático é `vdbe_sorter_compare_generic`, porque `sqlite3VdbeSorterRowkey` e
//! `sqlite3VdbeSorterCompare` já ocupam `vdbe_sorter_rowkey` e `vdbe_sorter_compare`.

use std::rc::Rc;

use crate::btree::btree_get_page_size;
use crate::connection::Connection;
use crate::consts::{
    CURTYPE_SORTER, KEYINFO_ORDER_BIGNULL, MEM_BLOB, MEM_NULL, SQLITE_CANTOPEN, SQLITE_DONE,
    SQLITE_FCNTL_CHUNK_SIZE, SQLITE_FCNTL_MMAP_SIZE, SQLITE_FCNTL_SIZE_HINT, SQLITE_INTERNAL,
    SQLITE_IOERR_READ, SQLITE_IOERR_WRITE, SQLITE_MAX_MMAP_SIZE, SQLITE_NOMEM_BKPT, SQLITE_OK,
    SQLITE_OPEN_CREATE, SQLITE_OPEN_DELETEONCLOSE, SQLITE_OPEN_EXCLUSIVE, SQLITE_OPEN_READWRITE,
    SQLITE_OPEN_TEMP_JOURNAL,
};
use crate::mem::{mem_clear_and_resize, memcmp, KeyInfo, Mem, UnpackedRecord};
use crate::os::{os_close, os_file_control_hint, os_open, FileControlArg, VfsFile, VfsRef};
use crate::pcache1::heap_nearly_full;
use crate::record::{alloc_unpacked_record, record_compare, record_compare_with_skip, record_unpack};
use crate::util::{at, get_varint, get_varint32_nr, put_varint, varint_len};
use crate::vdbe_types::VdbeCursor;

/// `SQLITE_MAX_PMASZ`: máximo de dados em memória antes de descarregar um PMA de nível 0 (512 MiB).
const SQLITE_MAX_PMASZ: i64 = 1 << 29;

/// `SQLITE_SORTER_PMASZ`: o padrão de `sqlite3GlobalConfig.szPma` (em páginas).
const SORTER_PMASZ: u32 = 250;

/// O valor de `nWorker` em `sqlite3VdbeSorterInit`: threads de trabalho desligadas (ver o módulo).
const N_WORKER: i32 = 0;

/// `SORTER_TYPE_INTEGER`.
const SORTER_TYPE_INTEGER: u8 = 0x01;
/// `SORTER_TYPE_TEXT`.
const SORTER_TYPE_TEXT: u8 = 0x02;

/// Máximo de PMAs que um `MergeEngine` mescla.
const SORTER_MAX_MERGE_COUNT: i32 = 16;

/// `sizeof(SorterRecord)` em LP64: `int nVal` (4, mais 4 de preenchimento) e a união de 8 bytes.
const SORTER_RECORD_SIZE: i64 = 16;

/// `ROUND8(x)`.
#[inline]
fn round8(x: i64) -> i64 {
    (x + 7) & !7
}

/// Índice de um arquivo temporário em `VdbeSorter.files` (o `sqlite3_file *` do C).
pub type FileId = usize;

/// Posição de um arquivo (o slot do vetor `files`); `None` é o arquivo fechado.
type FileSlot = Option<Box<dyn VfsFile>>;

/// `sqlite3TempInMemory(db)` com `SQLITE_TEMP_STORE == 1` (o padrão do Debian). A função do C vive
/// em `main.c`; quando `crate::main` existir ela passa a ser a de lá.
#[inline]
fn temp_in_memory(db: &Connection) -> bool {
    db.temp_store == 2
}

/// O arquivo `fd` do vetor, se aberto.
#[inline]
fn file_mut(files: &mut [FileSlot], fd: Option<FileId>) -> Option<&mut (dyn VfsFile + 'static)> {
    files.get_mut(fd?)?.as_deref_mut()
}

/// `sqlite3OsRead` sobre o arquivo `fd` (arquivo ausente é erro de E/S).
fn read_file(files: &mut [FileSlot], fd: Option<FileId>, buf: &mut [u8], offset: i64) -> i32 {
    match file_mut(files, fd) {
        Some(f) => f.read(buf, offset),
        None => SQLITE_IOERR_READ,
    }
}

/// `sqlite3OsWrite` sobre o arquivo `fd` (arquivo ausente é erro de E/S).
fn write_file(files: &mut [FileSlot], fd: Option<FileId>, buf: &[u8], offset: i64) -> i32 {
    match file_mut(files, fd) {
        Some(f) => f.write(buf, offset),
        None => SQLITE_IOERR_WRITE,
    }
}

// ---------------------------------------------------------------------------------------------
// Tipos
// ---------------------------------------------------------------------------------------------

/// `SorterFile`: um arquivo temporário e a quantidade de dados nele.
#[derive(Clone, Copy, Default)]
pub struct SorterFile {
    /// `pFd`: o arquivo (índice em `VdbeSorter.files`), `None` se não aberto.
    pub p_fd: Option<FileId>,
    /// `iEof`: bytes de dados guardados no arquivo.
    pub i_eof: i64,
}

/// `SorterRecord`: o cabeçalho de um registro em memória. Os bytes ficam em `SorterList.data`.
pub struct SorterRecord {
    /// `nVal`: tamanho do registro em bytes.
    pub n_val: i32,
    /// `u.pNext`/`u.iNext`: o próximo registro da lista (índice em `SorterList.records`).
    pub p_next: Option<usize>,
    /// Onde os `n_val` bytes começam em `SorterList.data` (o `SRVAL(p)` do C).
    pub off: usize,
}

/// `SorterList`: a lista de registros em memória.
#[derive(Default)]
pub struct SorterList {
    /// `pList`: a cabeça da lista (índice em `records`).
    pub p_list: Option<usize>,
    /// `aMemory != 0`: o ordenador usa o modo de memória em bloco (ver o módulo).
    pub a_memory: bool,
    /// `szPMA`: tamanho de `pList` como PMA, em bytes.
    pub sz_pma: i64,
    /// Os cabeçalhos dos registros, na ordem de chegada.
    pub records: Vec<SorterRecord>,
    /// Os bytes dos registros, um depois do outro.
    pub data: Vec<u8>,
}

impl SorterList {
    /// Os bytes do registro `i` (`SRVAL(p)` com `p->nVal`).
    #[inline]
    fn val(&self, i: usize) -> &[u8] {
        let r = &self.records[i];
        &self.data[r.off..r.off + r.n_val as usize]
    }

    /// Esvazia a lista (o `pList = 0` mais a liberação dos registros).
    fn clear(&mut self) {
        self.p_list = None;
        self.records.clear();
        self.data.clear();
    }
}

/// `SorterCompare`: a função de comparação de chaves do ordenador. No C ela recebe a
/// `SortSubtask`; aqui recebe só o que ela usa: o registro desempacotado da tarefa
/// (`pTask->pUnpacked`) e o `KeyInfo` do ordenador (`pTask->pSorter->pKeyInfo`).
pub type SorterCompare = fn(&mut UnpackedRecord, &KeyInfo, &mut bool, &[u8], &[u8]) -> i32;

/// `SortSubtask`: uma linha de controle da ordenação (aqui há exatamente uma, ver o módulo).
#[derive(Default)]
pub struct SortSubtask {
    /// `bDone`: a thread terminou mas ainda não foi juntada (sempre falso, sem threads).
    pub b_done: bool,
    /// `nPMA`: PMAs atualmente no arquivo.
    pub n_pma: i32,
    /// `pUnpacked`: espaço para desempacotar um registro.
    pub p_unpacked: Option<Box<UnpackedRecord>>,
    /// `list`: a lista para a thread gravar num PMA (sem uso sem threads).
    pub list: SorterList,
    /// `xCompare`: a função de comparação.
    pub x_compare: Option<SorterCompare>,
    /// `file`: o arquivo temporário dos PMAs de nível 0.
    pub file: SorterFile,
    /// `file2`: espaço para os outros PMAs.
    pub file2: SorterFile,
}

/// Onde está a chave corrente de um `PmaReader` (o `aKey` do C).
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum KeyLoc {
    /// Nenhuma chave.
    #[default]
    Null,
    /// A partir do deslocamento dado em `a_map`.
    Map(usize),
    /// A partir do deslocamento dado em `a_buffer`.
    Buffer(usize),
    /// O começo de `a_alloc`.
    Alloc,
}

/// `PmaReader`: lê os registros de um PMA em ordem. `p_fd == None` é o fim do arquivo.
#[derive(Default)]
pub struct PmaReader {
    /// `iReadOff`: deslocamento de leitura corrente.
    pub i_read_off: i64,
    /// `iEof`: um byte depois do fim deste PMA.
    pub i_eof: i64,
    /// `nAlloc`: bytes de `a_alloc`.
    pub n_alloc: i32,
    /// `nKey`: bytes da chave corrente.
    pub n_key: i32,
    /// `pFd`: o arquivo de onde se lê (`None` no EOF).
    pub p_fd: Option<FileId>,
    /// `aAlloc`: espaço para a chave quando nem o mapa nem o buffer a têm contígua.
    pub a_alloc: Vec<u8>,
    /// `aKey`: onde está a chave corrente.
    a_key: KeyLoc,
    /// `aBuffer`: o buffer de leitura.
    pub a_buffer: Vec<u8>,
    /// `nBuffer`: tamanho do buffer de leitura.
    pub n_buffer: i32,
    /// `aMap`: o arquivo inteiro mapeado (cópia, ver o módulo).
    pub a_map: Option<Vec<u8>>,
    /// `pIncr`: o mesclador incremental.
    pub p_incr: Option<Box<IncrMerger>>,
}

impl PmaReader {
    /// `n` bytes a partir de `loc`, sem nunca passar do fim da fonte.
    fn blob(&self, loc: KeyLoc, n: usize) -> &[u8] {
        let (off, src): (usize, &[u8]) = match loc {
            KeyLoc::Null => return &[],
            KeyLoc::Map(o) => (o, self.a_map.as_deref().unwrap_or(&[])),
            KeyLoc::Buffer(o) => (o, &self.a_buffer),
            KeyLoc::Alloc => (0, &self.a_alloc),
        };
        let s = src.get(off..).unwrap_or(&[]);
        &s[..n.min(s.len())]
    }

    /// A chave corrente (`aKey`, `nKey`).
    pub fn key(&self) -> &[u8] {
        self.blob(self.a_key, self.n_key.max(0) as usize)
    }
}

/// `MergeEngine`: combina dois ou mais PMAs numa sequência ordenada. `a_tree` tem `n_tree`
/// entradas (potência de 2); `a_tree[1]` é o índice do leitor com a menor chave.
pub struct MergeEngine {
    /// `nTree`: tamanho usado de `a_tree` e `a_readr` (potência de 2).
    pub n_tree: i32,
    /// `pTask`: a tarefa que usa este mesclador (índice em `a_task`).
    pub p_task: Option<usize>,
    /// `aTree`: o estado corrente da mesclagem incremental.
    pub a_tree: Vec<i32>,
    /// `aReadr`: os leitores de PMA.
    pub a_readr: Vec<PmaReader>,
}

/// `IncrMerger`: lê e mescla vários PMAs de uma vez, num trecho de `file2` da tarefa.
pub struct IncrMerger {
    /// `pTask`: a tarefa dona (índice em `a_task`).
    pub p_task: usize,
    /// `pMerger`: o mesclador de onde os dados vêm.
    pub p_merger: Box<MergeEngine>,
    /// `iStartOff`: deslocamento onde começa a gravar.
    pub i_start_off: i64,
    /// `mxSz`: máximo de bytes de dados.
    pub mx_sz: i32,
    /// `bEof`: verdadeiro quando a mesclagem acabou.
    pub b_eof: bool,
    /// `bUseThread`: usa thread de fundo (sempre falso, ver o módulo).
    pub b_use_thread: bool,
    /// `aFile`: `a_file[0]` para leitura, `[1]` para escrita.
    pub a_file: [SorterFile; 2],
}

/// `PmaWriter`: grava um PMA em blocos alinhados à página.
#[derive(Default)]
pub struct PmaWriter {
    /// `eFWErr`: diferente de zero em estado de erro.
    pub e_fw_err: i32,
    /// `aBuffer`: o buffer de escrita.
    pub a_buffer: Vec<u8>,
    /// `nBuffer`: tamanho do buffer.
    pub n_buffer: i32,
    /// `iBufStart`: primeiro byte do buffer a gravar.
    pub i_buf_start: i32,
    /// `iBufEnd`: último byte do buffer a gravar.
    pub i_buf_end: i32,
    /// `iWriteOff`: deslocamento no arquivo do começo do buffer.
    pub i_write_off: i64,
    /// `pFd`: o arquivo onde gravar.
    pub p_fd: Option<FileId>,
}

/// `VdbeSorter`: o ordenador de um cursor (`VdbeCursor.p_sorter`).
pub struct VdbeSorter {
    /// `mnPmaSize`: tamanho mínimo do PMA, em bytes.
    pub mn_pma_size: i32,
    /// `mxPmaSize`: tamanho máximo do PMA, em bytes (0 é sem limite).
    pub mx_pma_size: i32,
    /// `mxKeysize`: a maior chave serializada vista até agora.
    pub mx_keysize: i32,
    /// `pgsz`: tamanho da página do banco principal.
    pub pgsz: i32,
    /// `pReader`: de onde ler depois do `rewind` com threads (sempre `None`).
    pub p_reader: Option<Box<PmaReader>>,
    /// `pMerger`: de onde ler depois do `rewind` sem threads.
    pub p_merger: Option<Box<MergeEngine>>,
    /// `db->pVfs`, copiado no `init`.
    pub p_vfs: Option<VfsRef>,
    /// `db->nMaxSorterMmap`, copiado no `init`.
    pub n_max_sorter_mmap: i32,
    /// `pKeyInfo`: como comparar os registros (cópia do `KeyInfo` do cursor).
    pub p_key_info: Rc<KeyInfo>,
    /// `pUnpacked`: usado por `vdbe_sorter_compare`.
    pub p_unpacked: Option<Box<UnpackedRecord>>,
    /// `list`: os registros em memória.
    pub list: SorterList,
    /// `iMemory`: deslocamento do espaço livre em `list.aMemory`.
    pub i_memory: i32,
    /// `nMemory`: tamanho da alocação `list.aMemory`, em bytes.
    pub n_memory: i32,
    /// `bUsePMA`: verdadeiro se um ou mais PMAs foram criados.
    pub b_use_pma: bool,
    /// `bUseThreads`: usa threads de fundo (sempre falso, ver o módulo).
    pub b_use_threads: bool,
    /// `iPrev`: a thread anterior que descarregou um PMA.
    pub i_prev: u8,
    /// `nTask`: tamanho de `a_task`.
    pub n_task: u8,
    /// `typeMask`: `SORTER_TYPE_*` comum a todas as chaves vistas.
    pub type_mask: u8,
    /// `aTask`: as subtarefas.
    pub a_task: Vec<SortSubtask>,
    /// Os arquivos temporários (o dono dos `sqlite3_file` do C).
    pub files: Vec<FileSlot>,
}

// ---------------------------------------------------------------------------------------------
// PmaReader
// ---------------------------------------------------------------------------------------------

/// `vdbePmaReaderClear`: libera tudo do leitor e o zera. Os `files` entram para o `xUnfetch`.
fn vdbe_pma_reader_clear(files: &mut [FileSlot], reader: &mut PmaReader) {
    if let Some(map) = reader.a_map.take() {
        if let Some(f) = file_mut(files, reader.p_fd) {
            f.unfetch(0, Some(map));
        }
    }
    if let Some(incr) = reader.p_incr.take() {
        vdbe_incr_free(files, incr);
    }
    *reader = PmaReader::default();
}

/// `vdbePmaReadBlob`: lê os próximos `n_byte` bytes do PMA. Devolve onde os dados estão (válido
/// até a próxima chamada) ou o código de erro.
fn vdbe_pma_read_blob(files: &mut [FileSlot], p: &mut PmaReader, n_byte: i32) -> Result<KeyLoc, i32> {
    if p.a_map.is_some() {
        let loc = KeyLoc::Map(p.i_read_off as usize);
        p.i_read_off += n_byte as i64;
        return Ok(loc);
    }

    debug_assert!(!p.a_buffer.is_empty());
    if p.n_buffer <= 0 {
        return Err(SQLITE_INTERNAL);
    }

    // Se não há mais dados no buffer, lê os próximos `n_buffer` bytes do arquivo (ou o que sobrou
    // do PMA, se for menos).
    let i_buf = (p.i_read_off % p.n_buffer as i64) as i32;
    if i_buf == 0 {
        let n_read: i32 = if (p.i_eof - p.i_read_off) > p.n_buffer as i64 {
            p.n_buffer
        } else {
            (p.i_eof - p.i_read_off) as i32
        };
        debug_assert!(n_read > 0);
        let n_read = n_read.max(0) as usize;
        let rc = read_file(files, p.p_fd, &mut p.a_buffer[..n_read], p.i_read_off);
        if rc != SQLITE_OK {
            return Err(rc);
        }
    }
    let n_avail = p.n_buffer - i_buf;

    if n_byte <= n_avail {
        // Os dados pedidos estão no buffer: devolve a posição, sem cópia.
        p.i_read_off += n_byte as i64;
        Ok(KeyLoc::Buffer(i_buf as usize))
    } else {
        // Nem tudo está no buffer: copia a faixa para `a_alloc`.
        if p.n_alloc < n_byte {
            let mut n_new = 128i64.max(2 * p.n_alloc as i64);
            while n_byte as i64 > n_new {
                n_new *= 2;
            }
            p.a_alloc.resize(n_new as usize, 0);
            p.n_alloc = n_new as i32;
        }

        // Copia o que há no buffer para o começo de `a_alloc`.
        let (ib, na) = (i_buf as usize, n_avail as usize);
        p.a_alloc[..na].copy_from_slice(&p.a_buffer[ib..ib + na]);
        p.i_read_off += n_avail as i64;
        let mut n_rem = n_byte - n_avail;

        // Cada volta copia até `n_buffer` bytes para `a_alloc`.
        while n_rem > 0 {
            let n_copy = n_rem.min(p.n_buffer);
            let loc = vdbe_pma_read_blob(files, p, n_copy)?;
            if let KeyLoc::Buffer(o) = loc {
                let dst = (n_byte - n_rem) as usize;
                let n = n_copy as usize;
                p.a_alloc[dst..dst + n].copy_from_slice(&p.a_buffer[o..o + n]);
            }
            n_rem -= n_copy;
        }

        Ok(KeyLoc::Alloc)
    }
}

/// `vdbePmaReadVarint`: lê um varint do fluxo de dados do leitor.
fn vdbe_pma_read_varint(files: &mut [FileSlot], p: &mut PmaReader) -> Result<u64, i32> {
    if let Some(map) = p.a_map.as_deref() {
        let (n, v) = get_varint(map.get(p.i_read_off as usize..).unwrap_or(&[]));
        p.i_read_off += n as i64;
        return Ok(v);
    }
    if p.n_buffer <= 0 {
        return Err(SQLITE_INTERNAL);
    }
    let i_buf = (p.i_read_off % p.n_buffer as i64) as i32;
    if i_buf != 0 && (p.n_buffer - i_buf) >= 9 {
        let (n, v) = get_varint(&p.a_buffer[i_buf as usize..]);
        p.i_read_off += n as i64;
        Ok(v)
    } else {
        let mut a_varint = [0u8; 16];
        let mut i = 0usize;
        loop {
            let loc = vdbe_pma_read_blob(files, p, 1)?;
            let b = at(p.blob(loc, 1), 0);
            a_varint[i & 0xf] = b;
            i += 1;
            if (b & 0x80) == 0 {
                break;
            }
        }
        Ok(get_varint(&a_varint).1)
    }
}

/// `vdbeSorterMapFile`: tenta mapear o arquivo inteiro. Devolve o código e o mapa (`None` se o
/// mapeamento não foi tentado, porque o arquivo é grande ou o VFS não mapeia).
fn vdbe_sorter_map_file(
    files: &mut [FileSlot],
    n_max_sorter_mmap: i32,
    file: SorterFile,
) -> (i32, Option<Vec<u8>>) {
    let mut rc = SQLITE_OK;
    let mut map = None;
    if file.i_eof <= n_max_sorter_mmap as i64 {
        if let Some(f) = file_mut(files, file.p_fd) {
            if f.i_version() >= 3 {
                rc = f.fetch(0, file.i_eof as i32, &mut map);
            }
        }
    }
    (rc, map)
}

/// `vdbePmaReaderSeek`: liga o leitor ao arquivo `file` (se ainda não estiver) e o posiciona em
/// `i_off`.
fn vdbe_pma_reader_seek(sorter: &mut VdbeSorter, reader: &mut PmaReader, file: SorterFile, i_off: i64) -> i32 {
    debug_assert!(reader.p_incr.as_ref().map_or(true, |i| !i.b_eof));

    if let Some(map) = reader.a_map.take() {
        if let Some(f) = file_mut(&mut sorter.files, reader.p_fd) {
            f.unfetch(0, Some(map));
        }
    }
    reader.i_read_off = i_off;
    reader.i_eof = file.i_eof;
    reader.p_fd = file.p_fd;

    let (mut rc, map) = vdbe_sorter_map_file(&mut sorter.files, sorter.n_max_sorter_mmap, file);
    reader.a_map = map;
    if rc == SQLITE_OK && reader.a_map.is_none() {
        let pgsz = sorter.pgsz;
        let i_buf = (reader.i_read_off % pgsz as i64) as i32;
        if reader.a_buffer.is_empty() {
            reader.a_buffer = vec![0u8; pgsz as usize];
            reader.n_buffer = pgsz;
        }
        if rc == SQLITE_OK && i_buf != 0 {
            let mut n_read = pgsz - i_buf;
            if (reader.i_read_off + n_read as i64) > reader.i_eof {
                n_read = (reader.i_eof - reader.i_read_off) as i32;
            }
            let ib = i_buf as usize;
            let nr = n_read.max(0) as usize;
            rc = read_file(&mut sorter.files, reader.p_fd, &mut reader.a_buffer[ib..ib + nr], reader.i_read_off);
        }
    }

    rc
}

/// `vdbePmaReaderNext`: avança o leitor para a próxima chave do PMA.
fn vdbe_pma_reader_next(sorter: &mut VdbeSorter, reader: &mut PmaReader) -> i32 {
    let mut rc = SQLITE_OK;

    if reader.i_read_off >= reader.i_eof {
        let mut b_eof = true;
        let mut incr_opt = reader.p_incr.take();
        if let Some(incr) = incr_opt.as_mut() {
            rc = vdbe_incr_swap(sorter, incr);
            if rc == SQLITE_OK && !incr.b_eof {
                rc = vdbe_pma_reader_seek(sorter, reader, incr.a_file[0], incr.i_start_off);
                b_eof = false;
            }
        }
        reader.p_incr = incr_opt;

        if b_eof {
            // Condição de EOF.
            vdbe_pma_reader_clear(&mut sorter.files, reader);
            return rc;
        }
    }

    let mut n_rec: u64 = 0;
    if rc == SQLITE_OK {
        match vdbe_pma_read_varint(&mut sorter.files, reader) {
            Ok(v) => n_rec = v,
            Err(e) => rc = e,
        }
    }
    if rc == SQLITE_OK {
        reader.n_key = n_rec as i32;
        match vdbe_pma_read_blob(&mut sorter.files, reader, n_rec as i32) {
            Ok(loc) => reader.a_key = loc,
            Err(e) => rc = e,
        }
    }

    rc
}

/// `vdbePmaReaderInit`: inicializa o leitor para varrer o PMA de `file` a partir de `i_start`.
/// Deixa o leitor na primeira chave (ou no EOF se o PMA for vazio). `pn_byte` recebe o tamanho
/// do PMA.
fn vdbe_pma_reader_init(
    sorter: &mut VdbeSorter,
    file: SorterFile,
    i_start: i64,
    reader: &mut PmaReader,
    pn_byte: &mut i64,
) -> i32 {
    debug_assert!(file.i_eof > i_start);
    debug_assert!(reader.a_alloc.is_empty() && reader.n_alloc == 0);
    debug_assert!(reader.a_buffer.is_empty());
    debug_assert!(reader.a_map.is_none());

    let mut rc = vdbe_pma_reader_seek(sorter, reader, file, i_start);
    if rc == SQLITE_OK {
        match vdbe_pma_read_varint(&mut sorter.files, reader) {
            Ok(n_byte) => {
                reader.i_eof = reader.i_read_off + n_byte as i64;
                *pn_byte += n_byte as i64;
            }
            Err(e) => rc = e,
        }
    }

    if rc == SQLITE_OK {
        rc = vdbe_pma_reader_next(sorter, reader);
    }
    rc
}

// ---------------------------------------------------------------------------------------------
// Comparação de chaves
// ---------------------------------------------------------------------------------------------

/// O primeiro byte do `KeyInfo.aSortFlags` (a ordem do primeiro campo).
#[inline]
fn sort_flags0(key_info: &KeyInfo) -> u8 {
    key_info.a_sort_flags.first().copied().unwrap_or(0)
}

/// `vdbeSorterCompareTail`: o `vdbeSorterCompare` supondo que o primeiro campo já se mostrou
/// igual nas duas chaves.
fn vdbe_sorter_compare_tail(
    r2: &mut UnpackedRecord,
    key_info: &KeyInfo,
    pb_key2_cached: &mut bool,
    p_key1: &[u8],
    p_key2: &[u8],
) -> i32 {
    if !*pb_key2_cached {
        record_unpack(key_info, p_key2, r2);
        *pb_key2_cached = true;
    }
    record_compare_with_skip(p_key1, r2, true)
}

/// `vdbeSorterCompare`: compara a chave 1 com a chave 2 usando as colações do `KeyInfo`. Se
/// `*pb_key2_cached` é verdadeiro, `r2` já contém a chave 2 desempacotada; senão ela é
/// desempacotada e a flag fica verdadeira.
fn vdbe_sorter_compare_generic(
    r2: &mut UnpackedRecord,
    key_info: &KeyInfo,
    pb_key2_cached: &mut bool,
    p_key1: &[u8],
    p_key2: &[u8],
) -> i32 {
    if !*pb_key2_cached {
        record_unpack(key_info, p_key2, r2);
        *pb_key2_cached = true;
    }
    record_compare(p_key1, r2)
}

/// `vdbeSorterCompareText`: versão otimizada que supõe que o primeiro campo das duas chaves é
/// TEXT com a colação BINARY.
fn vdbe_sorter_compare_text(
    r2: &mut UnpackedRecord,
    key_info: &KeyInfo,
    pb_key2_cached: &mut bool,
    p_key1: &[u8],
    p_key2: &[u8],
) -> i32 {
    let v1 = p_key1.get(at(p_key1, 0) as usize..).unwrap_or(&[]);
    let v2 = p_key2.get(at(p_key2, 0) as usize..).unwrap_or(&[]);

    let n1 = get_varint32_nr(p_key1.get(1..).unwrap_or(&[])) as i32;
    let n2 = get_varint32_nr(p_key2.get(1..).unwrap_or(&[])) as i32;
    let n = ((n1.min(n2) - 13) / 2).max(0) as usize;
    let mut res = memcmp(&v1[..n.min(v1.len())], &v2[..n.min(v2.len())]);
    if res == 0 {
        res = n1 - n2;
    }

    if res == 0 {
        if key_info.n_key_field > 1 {
            res = vdbe_sorter_compare_tail(r2, key_info, pb_key2_cached, p_key1, p_key2);
        }
    } else {
        debug_assert!(sort_flags0(key_info) & KEYINFO_ORDER_BIGNULL == 0);
        if sort_flags0(key_info) != 0 {
            res = -res;
        }
    }

    res
}

/// `vdbeSorterCompareInt`: versão otimizada que supõe que o primeiro campo das duas chaves é um
/// INTEGER.
fn vdbe_sorter_compare_int(
    r2: &mut UnpackedRecord,
    key_info: &KeyInfo,
    pb_key2_cached: &mut bool,
    p_key1: &[u8],
    p_key2: &[u8],
) -> i32 {
    /// `aLen`: bytes do inteiro por tipo serial.
    static A_LEN: [u8; 10] = [0, 1, 2, 3, 4, 6, 8, 0, 0, 0];

    let s1 = at(p_key1, 1) as i32;
    let s2 = at(p_key2, 1) as i32;
    let v1 = p_key1.get(at(p_key1, 0) as usize..).unwrap_or(&[]);
    let v2 = p_key2.get(at(p_key2, 0) as usize..).unwrap_or(&[]);
    let res: i32;

    debug_assert!((s1 > 0 && s1 < 7) || s1 == 8 || s1 == 9);
    debug_assert!((s2 > 0 && s2 < 7) || s2 == 8 || s2 == 9);

    if s1 == s2 {
        // Os dois valores têm o mesmo tamanho: compara como memcmp().
        let n = A_LEN.get(s1 as usize).copied().unwrap_or(0) as usize;
        let mut r = 0;
        for i in 0..n {
            r = at(v1, i) as i32 - at(v2, i) as i32;
            if r != 0 {
                if ((at(v1, 0) ^ at(v2, 0)) & 0x80) != 0 {
                    r = if at(v1, 0) & 0x80 != 0 { -1 } else { 1 };
                }
                break;
            }
        }
        res = r;
    } else if s1 > 7 && s2 > 7 {
        res = s1 - s2;
    } else {
        let mut r = if s2 > 7 {
            1
        } else if s1 > 7 {
            -1
        } else {
            s1 - s2
        };
        debug_assert!(r != 0);

        if r > 0 {
            if at(v1, 0) & 0x80 != 0 {
                r = -1;
            }
        } else if at(v2, 0) & 0x80 != 0 {
            r = 1;
        }
        res = r;
    }

    if res == 0 {
        if key_info.n_key_field > 1 {
            return vdbe_sorter_compare_tail(r2, key_info, pb_key2_cached, p_key1, p_key2);
        }
        res
    } else if sort_flags0(key_info) != 0 {
        debug_assert!(sort_flags0(key_info) & KEYINFO_ORDER_BIGNULL == 0);
        -res
    } else {
        res
    }
}

/// `vdbeSorterGetCompare`: a função de comparação para os valores que o sorter coletou.
fn vdbe_sorter_get_compare(type_mask: u8) -> SorterCompare {
    if type_mask == SORTER_TYPE_INTEGER {
        vdbe_sorter_compare_int
    } else if type_mask == SORTER_TYPE_TEXT {
        vdbe_sorter_compare_text
    } else {
        vdbe_sorter_compare_generic
    }
}

// ---------------------------------------------------------------------------------------------
// Criação, reset e fechamento
// ---------------------------------------------------------------------------------------------

/// `sqlite3VdbeSorterInit`: inicializa o cursor temporário recém-aberto como cursor de ordenação.
///
/// Normalmente o número de campos comparados é `pCsr->pKeyInfo->nKeyField`; se `n_field` não é
/// zero e o sorter garante ordenação estável (sempre, sem threads), `n_field` o substitui: no
/// `CREATE INDEX` as chaves chegam na ordem da chave primária, que forma o fim do registro, e
/// então os campos da chave primária nunca precisam ser comparados.
///
/// Devolve `SQLITE_OK` ou um código de erro.
pub fn vdbe_sorter_init(db: &Connection, n_field: i32, csr: &mut VdbeCursor) -> i32 {
    let n_worker = N_WORKER;

    debug_assert!(csr.p_key_info.is_some());
    debug_assert!(!csr.is_ephemeral);
    debug_assert!(csr.e_cur_type == CURTYPE_SORTER);
    let Some(csr_key_info) = csr.p_key_info.as_ref() else {
        return SQLITE_INTERNAL;
    };

    let Some(pgsz) = db.dbs.first().and_then(|d| d.bt.as_ref()).map(btree_get_page_size) else {
        return SQLITE_INTERNAL;
    };

    // Cópia do `KeyInfo` do cursor (o `db = 0` do C não existe aqui).
    let mut key_info: KeyInfo = (**csr_key_info).clone();
    if n_field != 0 && n_worker == 0 {
        key_info.n_key_field = n_field as u16;
    }

    let n_task = (n_worker + 1) as u8;
    let mut sorter = VdbeSorter {
        mn_pma_size: 0,
        mx_pma_size: 0,
        mx_keysize: 0,
        pgsz,
        p_reader: None,
        p_merger: None,
        p_vfs: db.p_vfs.clone(),
        n_max_sorter_mmap: db.n_max_sorter_mmap,
        p_key_info: Rc::new(key_info),
        p_unpacked: None,
        list: SorterList::default(),
        i_memory: 0,
        n_memory: 0,
        b_use_pma: false,
        b_use_threads: n_task > 1,
        i_prev: (n_worker - 1) as u8,
        n_task,
        type_mask: 0,
        a_task: (0..n_task).map(|_| SortSubtask::default()).collect(),
        files: Vec::new(),
    };

    if !temp_in_memory(db) {
        let sz_pma: u32 = SORTER_PMASZ;
        sorter.mn_pma_size = sz_pma.wrapping_mul(pgsz as u32) as i32;

        let mut mx_cache: i64 = db.dbs[0].schema.cache_size as i64;
        if mx_cache < 0 {
            // Um valor negativo C indica um cache de abs(C) KiB.
            mx_cache *= -1024;
        } else {
            mx_cache *= pgsz as i64;
        }
        mx_cache = mx_cache.min(SQLITE_MAX_PMASZ);
        sorter.mx_pma_size = sorter.mn_pma_size.max(mx_cache as i32);

        // `bSmallMalloc == 0`: aloca a memória em bloco.
        debug_assert!(sorter.i_memory == 0);
        sorter.n_memory = pgsz;
        sorter.list.a_memory = true;
    }

    let ki = &sorter.p_key_info;
    let coll0_is_default = match ki.a_coll.first() {
        None | Some(None) => true,
        Some(Some(c)) => db.p_dflt_coll.as_ref().map_or(false, |d| Rc::ptr_eq(c, d)),
    };
    if ki.n_all_field < 13 && coll0_is_default && (sort_flags0(ki) & KEYINFO_ORDER_BIGNULL) == 0 {
        sorter.type_mask = SORTER_TYPE_INTEGER | SORTER_TYPE_TEXT;
    }

    csr.p_sorter = Some(Box::new(sorter));
    SQLITE_OK
}

/// `vdbeMergeEngineFree`: libera o mesclador e todos os seus leitores.
fn vdbe_merge_engine_free(files: &mut [FileSlot], mut merger: Box<MergeEngine>) {
    for reader in merger.a_readr.iter_mut() {
        vdbe_pma_reader_clear(files, reader);
    }
}

/// `vdbeIncrFree`: libera o `IncrMerger` e o mesclador dentro dele.
fn vdbe_incr_free(files: &mut [FileSlot], incr: Box<IncrMerger>) {
    // Sem threads não há arquivos próprios para fechar nem thread para juntar.
    vdbe_merge_engine_free(files, incr.p_merger);
}

/// `vdbeSortSubtaskCleanup`: libera os recursos da tarefa e a zera. `pSorter` volta a ser
/// preenchido pelo chamador no C; aqui a tarefa não aponta para o sorter.
fn vdbe_sort_subtask_cleanup(files: &mut [FileSlot], task: &mut SortSubtask) {
    for fd in [task.file.p_fd, task.file2.p_fd].into_iter().flatten() {
        if let Some(slot) = files.get_mut(fd) {
            os_close(slot);
        }
    }
    *task = SortSubtask::default();
}

/// `sqlite3VdbeSorterReset`: devolve o sorter ao estado vazio inicial.
pub fn vdbe_sorter_reset(sorter: &mut VdbeSorter) {
    // vdbeSorterJoinAll(pSorter, SQLITE_OK): sem threads é a identidade.
    debug_assert!(sorter.b_use_threads || sorter.p_reader.is_none());
    if let Some(mut reader) = sorter.p_reader.take() {
        vdbe_pma_reader_clear(&mut sorter.files, &mut reader);
    }
    if let Some(merger) = sorter.p_merger.take() {
        vdbe_merge_engine_free(&mut sorter.files, merger);
    }
    for task in sorter.a_task.iter_mut() {
        vdbe_sort_subtask_cleanup(&mut sorter.files, task);
    }
    sorter.list.clear();
    sorter.list.sz_pma = 0;
    sorter.b_use_pma = false;
    sorter.i_memory = 0;
    sorter.mx_keysize = 0;
    sorter.p_unpacked = None;
    // Nenhum `FileId` sobrevive: todos os leitores e tarefas foram liberados acima.
    sorter.files.clear();
}

/// `sqlite3VdbeSorterClose`: libera tudo o que as rotinas `sqlite3VdbeSorterXXX` alocaram no cursor.
pub fn vdbe_sorter_close(csr: &mut VdbeCursor) {
    debug_assert!(csr.e_cur_type == CURTYPE_SORTER);
    if let Some(mut sorter) = csr.p_sorter.take() {
        vdbe_sorter_reset(&mut sorter);
    }
}

// ---------------------------------------------------------------------------------------------
// Arquivos temporários, MergeEngine
// ---------------------------------------------------------------------------------------------

/// `vdbeSorterExtendFile` (`SQLITE_MAX_MMAP_SIZE > 0`): tenta estender o arquivo temporário para
/// `n_byte` bytes e garantir que o VFS o mapeou, se o VFS mapear.
fn vdbe_sorter_extend_file(files: &mut [FileSlot], n_max_sorter_mmap: i32, fd: Option<FileId>, n_byte: i64) {
    if n_byte <= n_max_sorter_mmap as i64 {
        if let Some(f) = file_mut(files, fd) {
            if f.i_version() >= 3 {
                let mut p: Option<Vec<u8>> = None;
                os_file_control_hint(Some(&mut *f), SQLITE_FCNTL_CHUNK_SIZE, &mut FileControlArg::Int(4 * 1024));
                os_file_control_hint(Some(&mut *f), SQLITE_FCNTL_SIZE_HINT, &mut FileControlArg::Int64(n_byte));
                f.fetch(0, n_byte as i32, &mut p);
                if p.is_some() {
                    f.unfetch(0, p);
                }
            }
        }
    }
}

/// `vdbeSorterOpenTempFile`: abre um arquivo temporário e o guarda em `files`. Devolve o índice
/// ou o código de erro.
fn vdbe_sorter_open_temp_file(
    vfs: Option<&VfsRef>,
    files: &mut Vec<FileSlot>,
    n_max_sorter_mmap: i32,
    n_extend: i64,
) -> Result<FileId, i32> {
    let Some(vfs) = vfs else {
        return Err(SQLITE_CANTOPEN);
    };
    let mut out_flags = 0;
    let file = os_open(
        &**vfs,
        None,
        SQLITE_OPEN_TEMP_JOURNAL
            | SQLITE_OPEN_READWRITE
            | SQLITE_OPEN_CREATE
            | SQLITE_OPEN_EXCLUSIVE
            | SQLITE_OPEN_DELETEONCLOSE,
        &mut out_flags,
    )?;
    files.push(Some(file));
    let fd = files.len() - 1;
    let max: i64 = SQLITE_MAX_MMAP_SIZE as i64;
    os_file_control_hint(file_mut(files, Some(fd)), SQLITE_FCNTL_MMAP_SIZE, &mut FileControlArg::Int64(max));
    if n_extend > 0 {
        vdbe_sorter_extend_file(files, n_max_sorter_mmap, Some(fd), n_extend);
    }
    Ok(fd)
}

/// `vdbeSortAllocUnpacked`: aloca o `UnpackedRecord` da tarefa se ainda não existe.
fn vdbe_sort_alloc_unpacked(task: &mut SortSubtask, key_info: &Rc<KeyInfo>) {
    if task.p_unpacked.is_none() {
        let mut r = alloc_unpacked_record(Rc::clone(key_info));
        r.n_field = key_info.n_key_field;
        r.err_code = 0;
        task.p_unpacked = Some(Box::new(r));
    }
}

/// O `pTask->pUnpacked` que o C supõe alocado (`vdbeSortAllocUnpacked` já rodou).
fn unpacked(task: &mut SortSubtask) -> &mut UnpackedRecord {
    task.p_unpacked.as_deref_mut().expect("vdbesort: pUnpacked ausente (invariante do C)")
}

/// `vdbeMergeEngineNew`: um `MergeEngine` para até `n_reader` leitores (arredondado para a
/// próxima potência de dois, no mínimo 2).
fn vdbe_merge_engine_new(n_reader: i32) -> Box<MergeEngine> {
    let mut n: i32 = 2;
    debug_assert!(n_reader <= SORTER_MAX_MERGE_COUNT);
    while n < n_reader {
        n += n;
    }
    Box::new(MergeEngine {
        n_tree: n,
        p_task: None,
        a_tree: vec![0; n as usize],
        a_readr: (0..n).map(|_| PmaReader::default()).collect(),
    })
}

// ---------------------------------------------------------------------------------------------
// Ordenação da lista em memória
// ---------------------------------------------------------------------------------------------

/// `vdbeSorterMerge`: mescla as duas listas ordenadas `p1` e `p2` numa só e devolve a cabeça.
/// Em empate a lista da esquerda (`p1`) vem primeiro.
fn vdbe_sorter_merge(
    task: &mut SortSubtask,
    key_info: &KeyInfo,
    list: &mut SorterList,
    mut p1: usize,
    mut p2: usize,
) -> usize {
    let x_compare = task.x_compare.expect("vdbesort: xCompare ausente (invariante do C)");
    let mut p_final: Option<usize> = None;
    // `pp`: o elo onde o próximo nó entra (`None` é `pFinal`).
    let mut pp: Option<usize> = None;
    let mut b_cached = false;

    macro_rules! append {
        ($n:expr) => {
            match pp {
                None => p_final = Some($n),
                Some(prev) => list.records[prev].p_next = Some($n),
            }
        };
    }

    loop {
        let res = x_compare(unpacked(task), key_info, &mut b_cached, list.val(p1), list.val(p2));

        if res <= 0 {
            append!(p1);
            pp = Some(p1);
            match list.records[p1].p_next {
                Some(n) => p1 = n,
                None => {
                    list.records[p1].p_next = Some(p2);
                    break;
                }
            }
        } else {
            append!(p2);
            pp = Some(p2);
            b_cached = false;
            match list.records[p2].p_next {
                Some(n) => p2 = n,
                None => {
                    list.records[p2].p_next = Some(p1);
                    break;
                }
            }
        }
    }
    p_final.expect("vdbesort: lista mesclada vazia (invariante do C)")
}

/// `vdbeSorterSort`: ordena a lista ligada `list.p_list`. Devolve `SQLITE_OK` ou o `errCode` do
/// registro desempacotado.
fn vdbe_sorter_sort(
    task: &mut SortSubtask,
    key_info: &Rc<KeyInfo>,
    type_mask: u8,
    list: &mut SorterList,
) -> i32 {
    vdbe_sort_alloc_unpacked(task, key_info);

    let mut p = list.p_list;
    task.x_compare = Some(vdbe_sorter_get_compare(type_mask));
    let mut a_slot: [Option<usize>; 64] = [None; 64];

    while let Some(mut cur) = p {
        let p_next = list.records[cur].p_next;
        list.records[cur].p_next = None;
        let mut i = 0;
        while i < a_slot.len() {
            let Some(slot) = a_slot[i] else { break };
            cur = vdbe_sorter_merge(task, key_info, list, cur, slot);
            a_slot[i] = None;
            i += 1;
        }
        if i < a_slot.len() {
            a_slot[i] = Some(cur);
        }
        p = p_next;
    }

    let mut p: Option<usize> = None;
    for slot in a_slot.iter().flatten() {
        p = Some(match p {
            Some(prev) => vdbe_sorter_merge(task, key_info, list, prev, *slot),
            None => *slot,
        });
    }
    list.p_list = p;

    let err = unpacked(task).err_code as i32;
    debug_assert!(err == SQLITE_OK || err == crate::consts::SQLITE_NOMEM);
    err
}

// ---------------------------------------------------------------------------------------------
// PmaWriter
// ---------------------------------------------------------------------------------------------

/// `vdbePmaWriterInit`: inicializa o gravador de PMA.
fn vdbe_pma_writer_init(fd: Option<FileId>, p: &mut PmaWriter, n_buf: i32, i_start: i64) {
    *p = PmaWriter::default();
    if n_buf <= 0 {
        p.e_fw_err = SQLITE_NOMEM_BKPT;
    } else {
        p.a_buffer = vec![0u8; n_buf as usize];
        p.i_buf_start = (i_start % n_buf as i64) as i32;
        p.i_buf_end = p.i_buf_start;
        p.i_write_off = i_start - p.i_buf_start as i64;
        p.n_buffer = n_buf;
        p.p_fd = fd;
    }
}

/// `vdbePmaWriteBlob`: grava os bytes de `data` no PMA (o erro fica em `e_fw_err`).
fn vdbe_pma_write_blob(files: &mut [FileSlot], p: &mut PmaWriter, data: &[u8]) {
    let n_data = data.len() as i32;
    let mut n_rem = n_data;
    while n_rem > 0 && p.e_fw_err == 0 {
        let mut n_copy = n_rem;
        if n_copy > (p.n_buffer - p.i_buf_end) {
            n_copy = p.n_buffer - p.i_buf_end;
        }

        let src = (n_data - n_rem) as usize;
        let (e, n) = (p.i_buf_end as usize, n_copy as usize);
        p.a_buffer[e..e + n].copy_from_slice(&data[src..src + n]);
        p.i_buf_end += n_copy;
        if p.i_buf_end == p.n_buffer {
            let (s, e) = (p.i_buf_start as usize, p.i_buf_end as usize);
            p.e_fw_err = write_file(files, p.p_fd, &p.a_buffer[s..e], p.i_write_off + p.i_buf_start as i64);
            p.i_buf_start = 0;
            p.i_buf_end = 0;
            p.i_write_off += p.n_buffer as i64;
        }
        debug_assert!(p.i_buf_end < p.n_buffer);

        n_rem -= n_copy;
    }
}

/// `vdbePmaWriterFinish`: descarrega o que está no buffer e encerra o gravador. `pi_eof` recebe
/// o deslocamento logo depois do último byte gravado.
fn vdbe_pma_writer_finish(files: &mut [FileSlot], p: &mut PmaWriter, pi_eof: &mut i64) -> i32 {
    if p.e_fw_err == 0 && !p.a_buffer.is_empty() && p.i_buf_end > p.i_buf_start {
        let (s, e) = (p.i_buf_start as usize, p.i_buf_end as usize);
        p.e_fw_err = write_file(files, p.p_fd, &p.a_buffer[s..e], p.i_write_off + p.i_buf_start as i64);
    }
    *pi_eof = p.i_write_off + p.i_buf_end as i64;
    let rc = p.e_fw_err;
    *p = PmaWriter::default();
    rc
}

/// `vdbePmaWriteVarint`: grava `i_val` como varint no PMA.
fn vdbe_pma_write_varint(files: &mut [FileSlot], p: &mut PmaWriter, i_val: u64) {
    let mut a_byte = [0u8; 10];
    let n_byte = put_varint(&mut a_byte, i_val) as usize;
    vdbe_pma_write_blob(files, p, &a_byte[..n_byte]);
}

/// `vdbeSorterListToPMA`: grava a lista em memória `list` num PMA de nível 0 do arquivo
/// temporário da tarefa `i_task`. O PMA é: um varint com o total de bytes de conteúdo (sem ele
/// mesmo), seguido dos registros em ordem crescente, cada um um varint com o tamanho e a chave.
fn vdbe_sorter_list_to_pma(sorter: &mut VdbeSorter, i_task: usize, list: &mut SorterList) -> i32 {
    let mut rc = SQLITE_OK;
    let mut writer = PmaWriter::default();

    debug_assert!(list.sz_pma > 0);

    // Se o primeiro arquivo temporário de PMA não foi aberto, abre agora.
    if sorter.a_task[i_task].file.p_fd.is_none() {
        match vdbe_sorter_open_temp_file(sorter.p_vfs.as_ref(), &mut sorter.files, sorter.n_max_sorter_mmap, 0) {
            Ok(fd) => sorter.a_task[i_task].file.p_fd = Some(fd),
            Err(e) => rc = e,
        }
        debug_assert!(sorter.a_task[i_task].file.i_eof == 0);
        debug_assert!(sorter.a_task[i_task].n_pma == 0);
    }

    // Tenta fazer o arquivo ser mapeado em memória.
    if rc == SQLITE_OK {
        let file = sorter.a_task[i_task].file;
        vdbe_sorter_extend_file(&mut sorter.files, sorter.n_max_sorter_mmap, file.p_fd, file.i_eof + list.sz_pma + 9);
    }

    // Ordena a lista.
    if rc == SQLITE_OK {
        rc = vdbe_sorter_sort(&mut sorter.a_task[i_task], &sorter.p_key_info, sorter.type_mask, list);
    }

    if rc == SQLITE_OK {
        let task = &mut sorter.a_task[i_task];
        vdbe_pma_writer_init(task.file.p_fd, &mut writer, sorter.pgsz, task.file.i_eof);
        task.n_pma += 1;
        vdbe_pma_write_varint(&mut sorter.files, &mut writer, list.sz_pma as u64);
        let mut p = list.p_list;
        while let Some(i) = p {
            let n_val = list.records[i].n_val;
            vdbe_pma_write_varint(&mut sorter.files, &mut writer, n_val as u64);
            vdbe_pma_write_blob(&mut sorter.files, &mut writer, list.val(i));
            p = list.records[i].p_next;
        }
        list.clear();
        rc = vdbe_pma_writer_finish(&mut sorter.files, &mut writer, &mut task.file.i_eof);
    }

    rc
}

// ---------------------------------------------------------------------------------------------
// MergeEngine: passo, IncrMerger
// ---------------------------------------------------------------------------------------------

/// `vdbeMergeEngineStep`: avança o mesclador para a próxima entrada. `pb_eof` fica verdadeiro
/// quando não há próxima (todas as entradas acabaram).
fn vdbe_merge_engine_step(sorter: &mut VdbeSorter, merger: &mut MergeEngine, pb_eof: &mut bool) -> i32 {
    let i_prev = merger.a_tree[1] as usize; /* Índice do PmaReader a avançar */
    let i_task = merger.p_task.expect("vdbesort: MergeEngine sem tarefa (invariante do C)");

    // Avança o PmaReader corrente.
    let rc = vdbe_pma_reader_next(sorter, &mut merger.a_readr[i_prev]);

    // Atualiza o conteúdo de a_tree[].
    if rc == SQLITE_OK {
        let mut b_cached = false;

        // Os dois primeiros PmaReaders a comparar: o que acabou de avançar e o vizinho dele.
        let mut i1 = i_prev & 0xFFFE;
        let mut i2 = i_prev | 0x0001;

        let mut i = (merger.n_tree as usize + i_prev) / 2;
        while i > 0 {
            // Compara os leitores 1 e 2 e guarda o resultado em i_res.
            let i_res: i32;
            {
                let r1 = &merger.a_readr[i1];
                let r2 = &merger.a_readr[i2];
                if r1.p_fd.is_none() {
                    i_res = 1;
                } else if r2.p_fd.is_none() {
                    i_res = -1;
                } else {
                    let task = &mut sorter.a_task[i_task];
                    let x_compare = task.x_compare.expect("vdbesort: xCompare ausente (invariante do C)");
                    i_res = x_compare(unpacked(task), &sorter.p_key_info, &mut b_cached, r1.key(), r2.key());
                }
            }

            // Se o leitor 1 tem o menor valor, a_tree[i] recebe o índice dele e o leitor 2 passa
            // a ser o próximo a comparar; senão o contrário. Em empate vale o PMA mais antigo
            // (a_readr[] é ordenado do mais antigo ao mais novo, então menor índice é mais
            // antigo).
            if i_res < 0 || (i_res == 0 && i1 < i2) {
                merger.a_tree[i] = i1 as i32;
                i2 = merger.a_tree[i ^ 0x0001] as usize;
                b_cached = false;
            } else {
                if merger.a_readr[i1].p_fd.is_some() {
                    b_cached = false;
                }
                merger.a_tree[i] = i2 as i32;
                i1 = merger.a_tree[i ^ 0x0001] as usize;
            }
            i /= 2;
        }
        *pb_eof = merger.a_readr[merger.a_tree[1] as usize].p_fd.is_none();
    }

    if rc == SQLITE_OK {
        unpacked(&mut sorter.a_task[i_task]).err_code as i32
    } else {
        rc
    }
}

/// `vdbeSorterFlushPMA`: descarrega `list` num novo PMA. Sem threads é sempre a thread principal
/// (`aTask[0]`).
fn vdbe_sorter_flush_pma(sorter: &mut VdbeSorter) -> i32 {
    sorter.b_use_pma = true;
    let mut list = std::mem::take(&mut sorter.list);
    let rc = vdbe_sorter_list_to_pma(sorter, 0, &mut list);
    sorter.list = list;
    rc
}

/// `sqlite3VdbeSorterWrite`: acrescenta um registro ao sorter.
pub fn vdbe_sorter_write(csr: &mut VdbeCursor, val: &Mem) -> i32 {
    debug_assert!(csr.e_cur_type == CURTYPE_SORTER);
    let Some(sorter) = csr.p_sorter.as_deref_mut() else {
        return SQLITE_INTERNAL;
    };
    let mut rc = SQLITE_OK;

    // O tipo serial do primeiro campo do registro.
    let t = get_varint32_nr(val.z.get(1..).unwrap_or(&[]));
    if t > 0 && t < 10 && t != 7 {
        sorter.type_mask &= SORTER_TYPE_INTEGER;
    } else if t > 10 && (t & 0x01) != 0 {
        sorter.type_mask &= SORTER_TYPE_TEXT;
    } else {
        sorter.type_mask = 0;
    }

    // Decide se o conteúdo da memória deve ser descarregado num PMA antes de seguir. No modo de
    // alocação em bloco (aMemory != 0), descarrega se (a) já há um valor em memória e (b) o novo
    // não cabe. Com alocação separada por registro, descarrega se a memória da lista passa de
    // (tamanho de página * tamanho de cache) ou, passando de (tamanho de página * 10), se o heap
    // está quase cheio.
    let key = val.bytes();
    let n_val = key.len() as i64;
    let n_req: i64 = n_val + SORTER_RECORD_SIZE;
    let n_pma: i64 = n_val + varint_len(n_val as u64) as i64;
    if sorter.mx_pma_size != 0 {
        let b_flush = if sorter.list.a_memory {
            sorter.i_memory != 0 && (sorter.i_memory as i64 + n_req) > sorter.mx_pma_size as i64
        } else {
            sorter.list.sz_pma > sorter.mx_pma_size as i64
                || (sorter.list.sz_pma > sorter.mn_pma_size as i64 && heap_nearly_full())
        };
        if b_flush {
            rc = vdbe_sorter_flush_pma(sorter);
            sorter.list.sz_pma = 0;
            sorter.i_memory = 0;
            debug_assert!(rc != SQLITE_OK || sorter.list.p_list.is_none());
        }
    }

    sorter.list.sz_pma += n_pma;
    if n_pma > sorter.mx_keysize as i64 {
        sorter.mx_keysize = n_pma as i32;
    }

    if sorter.list.a_memory {
        let n_min = (sorter.i_memory as i64 + n_req) as i32;

        if n_min > sorter.n_memory {
            let mut n_new: i64 = 2 * sorter.n_memory as i64;
            while n_new < n_min as i64 {
                n_new *= 2;
            }
            if n_new > sorter.mx_pma_size as i64 {
                n_new = sorter.mx_pma_size as i64;
            }
            if n_new < n_min as i64 {
                n_new = n_min as i64;
            }
            sorter.n_memory = n_new as i32;
        }

        sorter.i_memory += round8(n_req) as i32;
    }

    let list = &mut sorter.list;
    let off = list.data.len();
    list.data.extend_from_slice(key);
    list.records.push(SorterRecord { n_val: key.len() as i32, p_next: list.p_list, off });
    list.p_list = Some(list.records.len() - 1);

    rc
}

/// `vdbeIncrPopulate`: lê chaves de `pIncr->pMerger` e preenche `a_file[1]`. O formato dos dados
/// é o dos PMAs comuns, sem o varint de número de bytes no começo.
fn vdbe_incr_populate(sorter: &mut VdbeSorter, incr: &mut IncrMerger) -> i32 {
    let mut rc = SQLITE_OK;
    let i_start = incr.i_start_off;
    let mut writer = PmaWriter::default();
    debug_assert!(!incr.b_eof);

    vdbe_pma_writer_init(incr.a_file[1].p_fd, &mut writer, sorter.pgsz, i_start);
    while rc == SQLITE_OK {
        let reader = &incr.p_merger.a_readr[incr.p_merger.a_tree[1] as usize];
        let n_key = reader.n_key;
        let i_eof = writer.i_write_off + writer.i_buf_end as i64;

        // Sai se o arquivo de saída está cheio ou a entrada acabou.
        if reader.p_fd.is_none() {
            break;
        }
        if (i_eof + n_key as i64 + varint_len(n_key as u64) as i64) > (i_start + incr.mx_sz as i64) {
            break;
        }

        // Grava a próxima chave na saída.
        vdbe_pma_write_varint(&mut sorter.files, &mut writer, n_key as u64);
        vdbe_pma_write_blob(&mut sorter.files, &mut writer, reader.key());
        debug_assert!(incr.p_merger.p_task == Some(incr.p_task));
        let mut dummy = false;
        rc = vdbe_merge_engine_step(sorter, &mut incr.p_merger, &mut dummy);
    }

    let rc2 = vdbe_pma_writer_finish(&mut sorter.files, &mut writer, &mut incr.a_file[1].i_eof);
    if rc == SQLITE_OK {
        rc = rc2;
    }
    rc
}

/// `vdbeIncrSwap`: chamada quando o `PmaReader` do `pIncr` terminou de ler `a_file[0]`. Reabastece
/// `a_file[0]` lendo chaves de `pIncr->pMerger` (sem threads, o próprio chamador faz o trabalho).
fn vdbe_incr_swap(sorter: &mut VdbeSorter, incr: &mut IncrMerger) -> i32 {
    let rc = vdbe_incr_populate(sorter, incr);
    incr.a_file[0] = incr.a_file[1];
    if incr.a_file[0].i_eof == incr.i_start_off {
        incr.b_eof = true;
    }
    rc
}

/// `vdbeIncrMergerNew`: um `IncrMerger` novo para ler de `merger`.
fn vdbe_incr_merger_new(sorter: &mut VdbeSorter, i_task: usize, merger: Box<MergeEngine>) -> Box<IncrMerger> {
    let mx_sz = (sorter.mx_keysize + 9).max(sorter.mx_pma_size / 2);
    sorter.a_task[i_task].file2.i_eof += mx_sz as i64;
    Box::new(IncrMerger {
        p_task: i_task,
        p_merger: merger,
        i_start_off: 0,
        mx_sz,
        b_eof: false,
        b_use_thread: false,
        a_file: [SorterFile::default(); 2],
    })
}

/// `vdbeMergeEngineCompare`: recalcula `a_tree[i_out]` comparando as próximas chaves dos dois
/// leitores que o alimentam. Nenhum deles avança.
fn vdbe_merge_engine_compare(sorter: &mut VdbeSorter, merger: &mut MergeEngine, i_out: usize) {
    let n_half = merger.n_tree as usize / 2;
    debug_assert!(i_out < merger.n_tree as usize && i_out > 0);

    let (i1, i2) = if i_out >= n_half {
        let i1 = (i_out - n_half) * 2;
        (i1, i1 + 1)
    } else {
        (merger.a_tree[i_out * 2] as usize, merger.a_tree[i_out * 2 + 1] as usize)
    };

    let p1 = &merger.a_readr[i1];
    let p2 = &merger.a_readr[i2];

    let i_res = if p1.p_fd.is_none() {
        i2
    } else if p2.p_fd.is_none() {
        i1
    } else {
        let i_task = merger.p_task.expect("vdbesort: MergeEngine sem tarefa (invariante do C)");
        let task = &mut sorter.a_task[i_task];
        let x_compare = task.x_compare.expect("vdbesort: xCompare ausente (invariante do C)");
        let mut b_cached = false;
        let res = x_compare(unpacked(task), &sorter.p_key_info, &mut b_cached, p1.key(), p2.key());
        if res <= 0 {
            i1
        } else {
            i2
        }
    };

    merger.a_tree[i_out] = i_res as i32;
}

/// `vdbeMergeEngineInit`: inicializa o mesclador. Depois disso a primeira chave mesclada pode ser
/// lida de forma usual.
fn vdbe_merge_engine_init(sorter: &mut VdbeSorter, i_task: usize, merger: &mut MergeEngine) -> i32 {
    // O MergeEngine é atribuído a uma única thread.
    debug_assert!(merger.p_task.is_none());
    merger.p_task = Some(i_task);

    let n_tree = merger.n_tree as usize;
    for i in 0..n_tree {
        let rc = vdbe_pma_reader_incr_init(sorter, &mut merger.a_readr[i]);
        if rc != SQLITE_OK {
            return rc;
        }
    }

    for i in (1..n_tree).rev() {
        vdbe_merge_engine_compare(sorter, merger, i);
    }
    unpacked(&mut sorter.a_task[i_task]).err_code as i32
}

/// `vdbePmaReaderIncrMergeInit`: o leitor é incremental (`p_incr != None`). Abre e inicializa os
/// campos de arquivo do `IncrMerger` e todos os leitores da subárvore, e carrega os dados no
/// buffer do leitor, que fica apontando para a primeira chave.
fn vdbe_pma_reader_incr_merge_init(sorter: &mut VdbeSorter, reader: &mut PmaReader) -> i32 {
    let Some(mut incr) = reader.p_incr.take() else {
        return SQLITE_INTERNAL;
    };
    let i_task = incr.p_task;

    let mut rc = vdbe_merge_engine_init(sorter, i_task, &mut incr.p_merger);

    // Prepara os arquivos de `pIncr`: sem threads é só um trecho de `file2` da tarefa.
    if rc == SQLITE_OK {
        let mx_sz = incr.mx_sz;
        if sorter.a_task[i_task].file2.p_fd.is_none() {
            debug_assert!(sorter.a_task[i_task].file2.i_eof > 0);
            let n_extend = sorter.a_task[i_task].file2.i_eof;
            match vdbe_sorter_open_temp_file(sorter.p_vfs.as_ref(), &mut sorter.files, sorter.n_max_sorter_mmap, n_extend) {
                Ok(fd) => sorter.a_task[i_task].file2.p_fd = Some(fd),
                Err(e) => rc = e,
            }
            sorter.a_task[i_task].file2.i_eof = 0;
        }
        if rc == SQLITE_OK {
            let file2 = &mut sorter.a_task[i_task].file2;
            incr.a_file[1].p_fd = file2.p_fd;
            incr.i_start_off = file2.i_eof;
            file2.i_eof += mx_sz as i64;
        }
    }

    reader.p_incr = Some(incr);

    if rc == SQLITE_OK {
        rc = vdbe_pma_reader_next(sorter, reader);
    }

    rc
}

/// `vdbePmaReaderIncrInit`: se o leitor não é incremental não faz nada; senão inicializa a mescla
/// incremental com a thread corrente.
fn vdbe_pma_reader_incr_init(sorter: &mut VdbeSorter, reader: &mut PmaReader) -> i32 {
    if reader.p_incr.is_some() {
        vdbe_pma_reader_incr_merge_init(sorter, reader)
    } else {
        SQLITE_OK
    }
}

/// `vdbeMergeEngineLevel0`: um `MergeEngine` novo que mescla `n_pma` PMAs de nível 0 de
/// `pTask->file`, a partir do deslocamento `*pi_offset`, que ao fim aponta para depois do último
/// PMA.
fn vdbe_merge_engine_level0(
    sorter: &mut VdbeSorter,
    i_task: usize,
    n_pma: i32,
    pi_offset: &mut i64,
) -> Result<Box<MergeEngine>, i32> {
    let mut new = vdbe_merge_engine_new(n_pma);
    let mut i_off = *pi_offset;
    let mut rc = SQLITE_OK;

    let mut i = 0usize;
    while (i as i32) < n_pma && rc == SQLITE_OK {
        let mut n_dummy: i64 = 0;
        let file = sorter.a_task[i_task].file;
        rc = vdbe_pma_reader_init(sorter, file, i_off, &mut new.a_readr[i], &mut n_dummy);
        i_off = new.a_readr[i].i_eof;
        i += 1;
    }

    *pi_offset = i_off;
    if rc != SQLITE_OK {
        vdbe_merge_engine_free(&mut sorter.files, new);
        return Err(rc);
    }
    Ok(new)
}

/// `vdbeSorterTreeDepth`: a profundidade de uma árvore de `n_pma` PMAs com fanout
/// `SORTER_MAX_MERGE_COUNT`, sem contar as folhas (`<= 16`: 0, `<= 256`: 1, `<= 65536`: 2).
fn vdbe_sorter_tree_depth(n_pma: i32) -> i32 {
    let mut n_depth = 0;
    let mut n_div: i64 = SORTER_MAX_MERGE_COUNT as i64;
    while n_div < n_pma as i64 {
        n_div *= SORTER_MAX_MERGE_COUNT as i64;
        n_depth += 1;
    }
    n_depth
}

/// `vdbeSorterAddToTree`: `p_root` é a raiz de uma árvore de mescla incremental de profundidade
/// `n_depth`; `p_leaf` é a `i_seq`-ésima folha a acrescentar, contando de zero.
fn vdbe_sorter_add_to_tree(
    sorter: &mut VdbeSorter,
    i_task: usize,
    n_depth: i32,
    i_seq: i32,
    p_root: &mut MergeEngine,
    p_leaf: Box<MergeEngine>,
) {
    let p_incr = vdbe_incr_merger_new(sorter, i_task, p_leaf);

    let mut n_div: i32 = 1;
    for _ in 1..n_depth {
        n_div *= SORTER_MAX_MERGE_COUNT;
    }

    let mut p: &mut MergeEngine = p_root;
    let mut i = 1;
    while i < n_depth {
        let i_iter = ((i_seq / n_div) % SORTER_MAX_MERGE_COUNT) as usize;
        let p_readr = &mut p.a_readr[i_iter];

        let incr = p_readr.p_incr.get_or_insert_with(|| {
            let p_new = vdbe_merge_engine_new(SORTER_MAX_MERGE_COUNT);
            vdbe_incr_merger_new(sorter, i_task, p_new)
        });
        p = &mut *incr.p_merger;
        n_div /= SORTER_MAX_MERGE_COUNT;
        i += 1;
    }

    p.a_readr[(i_seq % SORTER_MAX_MERGE_COUNT) as usize].p_incr = Some(p_incr);
}

/// `vdbeSorterMergeTreeBuild`: chamada no `rewind` de um sorter que já gravou dois ou mais PMAs
/// de nível 0. Monta a árvore de `MergeEngine`/`IncrMerger`/`PmaReader` que mescla
/// incrementalmente todos os PMAs do disco. Devolve o código e a raiz.
fn vdbe_sorter_merge_tree_build(sorter: &mut VdbeSorter) -> (i32, Option<Box<MergeEngine>>) {
    let mut p_main: Option<Box<MergeEngine>> = None;
    let mut rc = SQLITE_OK;

    // Sem threads há uma tarefa só e a raiz da tarefa é a raiz da árvore.
    debug_assert!(!sorter.b_use_threads && sorter.n_task == 1);

    let mut i_task = 0usize;
    while rc == SQLITE_OK && i_task < sorter.n_task as usize {
        let n_pma = sorter.a_task[i_task].n_pma;
        debug_assert!(n_pma > 0);
        let n_depth = vdbe_sorter_tree_depth(n_pma);
        let mut i_read_off: i64 = 0;
        let mut p_root: Option<Box<MergeEngine>> = None;

        if n_pma <= SORTER_MAX_MERGE_COUNT {
            match vdbe_merge_engine_level0(sorter, i_task, n_pma, &mut i_read_off) {
                Ok(m) => p_root = Some(m),
                Err(e) => rc = e,
            }
        } else {
            let mut root = vdbe_merge_engine_new(SORTER_MAX_MERGE_COUNT);
            let mut i_seq = 0;
            let mut i = 0;
            while i < n_pma && rc == SQLITE_OK {
                // `n_reader`: PMAs de nível 0 a mesclar.
                let n_reader = (n_pma - i).min(SORTER_MAX_MERGE_COUNT);
                match vdbe_merge_engine_level0(sorter, i_task, n_reader, &mut i_read_off) {
                    Ok(p_merger) => {
                        vdbe_sorter_add_to_tree(sorter, i_task, n_depth, i_seq, &mut root, p_merger);
                        i_seq += 1;
                    }
                    Err(e) => rc = e,
                }
                i += SORTER_MAX_MERGE_COUNT;
            }
            p_root = Some(root);
        }

        if rc == SQLITE_OK {
            debug_assert!(p_main.is_none());
            p_main = p_root;
        } else if let Some(root) = p_root {
            vdbe_merge_engine_free(&mut sorter.files, root);
        }
        i_task += 1;
    }

    if rc != SQLITE_OK {
        if let Some(main) = p_main.take() {
            vdbe_merge_engine_free(&mut sorter.files, main);
        }
    }
    (rc, p_main)
}

/// `vdbeSorterSetupMerge`: chamada no `rewind` de um sorter que gravou PMAs em arquivos
/// temporários. Prepara `p_merger` para iterar por todos os registros do sorter.
fn vdbe_sorter_setup_merge(sorter: &mut VdbeSorter) -> i32 {
    let (mut rc, p_main) = vdbe_sorter_merge_tree_build(sorter);
    if rc == SQLITE_OK {
        if let Some(mut main) = p_main {
            rc = vdbe_merge_engine_init(sorter, 0, &mut main);
            sorter.p_merger = Some(main);
        }
    }
    rc
}

/// `sqlite3VdbeSorterRewind`: chamada depois de todos os `vdbe_sorter_write`, para se preparar
/// para iterar os registros em ordem. `pb_eof` fica 1 se o sorter está vazio.
pub fn vdbe_sorter_rewind(csr: &mut VdbeCursor, pb_eof: &mut i32) -> i32 {
    debug_assert!(csr.e_cur_type == CURTYPE_SORTER);
    let Some(sorter) = csr.p_sorter.as_deref_mut() else {
        return SQLITE_INTERNAL;
    };
    let mut rc;

    // Se nenhum dado foi para o disco, não vai agora: ordena a lista em memória; o VDBE lê dela.
    if !sorter.b_use_pma {
        if sorter.list.p_list.is_some() {
            *pb_eof = 0;
            rc = vdbe_sorter_sort(&mut sorter.a_task[0], &sorter.p_key_info, sorter.type_mask, &mut sorter.list);
        } else {
            *pb_eof = 1;
            rc = SQLITE_OK;
        }
        return rc;
    }

    // Grava a lista em memória num PMA. Quando `vdbe_sorter_write` descarrega a memória, cria
    // logo depois uma lista com uma chave só, então a lista nunca está vazia aqui.
    debug_assert!(sorter.list.p_list.is_some());
    rc = vdbe_sorter_flush_pma(sorter);

    // vdbeSorterJoinAll(pSorter, rc): sem threads é a identidade.

    // Sem erro, monta a estrutura que mescla incrementalmente os PMAs restantes.
    debug_assert!(sorter.p_reader.is_none());
    if rc == SQLITE_OK {
        rc = vdbe_sorter_setup_merge(sorter);
        *pb_eof = 0;
    }

    rc
}

/// `sqlite3VdbeSorterNext`: avança para o próximo elemento. Devolve `SQLITE_OK`, `SQLITE_DONE` no
/// fim dos dados, ou um erro.
pub fn vdbe_sorter_next(csr: &mut VdbeCursor) -> i32 {
    debug_assert!(csr.e_cur_type == CURTYPE_SORTER);
    let Some(sorter) = csr.p_sorter.as_deref_mut() else {
        return SQLITE_INTERNAL;
    };
    debug_assert!(sorter.b_use_pma || (sorter.p_reader.is_none() && sorter.p_merger.is_none()));
    if sorter.b_use_pma {
        debug_assert!(sorter.p_reader.is_none() || sorter.p_merger.is_none());
        debug_assert!(!sorter.b_use_threads);
        let Some(mut merger) = sorter.p_merger.take() else {
            return SQLITE_INTERNAL;
        };
        debug_assert!(merger.p_task == Some(0));
        let mut res = false;
        let mut rc = vdbe_merge_engine_step(sorter, &mut merger, &mut res);
        sorter.p_merger = Some(merger);
        if rc == SQLITE_OK && res {
            rc = SQLITE_DONE;
        }
        rc
    } else {
        match sorter.list.p_list {
            Some(i_free) => {
                sorter.list.p_list = sorter.list.records[i_free].p_next;
                sorter.list.records[i_free].p_next = None;
                if sorter.list.p_list.is_some() {
                    SQLITE_OK
                } else {
                    SQLITE_DONE
                }
            }
            None => SQLITE_DONE,
        }
    }
}

/// `vdbeSorterRowkey`: a chave corrente (`nKey` é o tamanho da fatia).
fn vdbe_sorter_rowkey_bytes(sorter: &VdbeSorter) -> &[u8] {
    if sorter.b_use_pma {
        match sorter.p_merger.as_deref() {
            Some(m) => m.a_readr[m.a_tree[1] as usize].key(),
            None => &[],
        }
    } else {
        match sorter.list.p_list {
            Some(i) => sorter.list.val(i),
            None => &[],
        }
    }
}

/// `sqlite3VdbeSorterRowkey`: copia a chave corrente do sorter para a célula `out`.
pub fn vdbe_sorter_rowkey(csr: &VdbeCursor, out: &mut Mem) -> i32 {
    debug_assert!(csr.e_cur_type == CURTYPE_SORTER);
    let Some(sorter) = csr.p_sorter.as_deref() else {
        return SQLITE_INTERNAL;
    };
    let key = vdbe_sorter_rowkey_bytes(sorter);
    let n_key = key.len();
    if mem_clear_and_resize(out, n_key as i32) != SQLITE_OK {
        return SQLITE_NOMEM_BKPT;
    }
    out.n = n_key as i32;
    out.set_type_flag(MEM_BLOB);
    if out.z.len() < n_key {
        out.z.resize(n_key, 0);
    }
    out.z[..n_key].copy_from_slice(key);

    SQLITE_OK
}

/// `sqlite3VdbeSorterCompare`: compara a chave da célula `val` com a chave em que o cursor está.
/// Ignora o rowid no fim de cada registro. Se a chave do cursor tem algum NULL, ela é considerada
/// menor que `val`, mesmo que `val` também tenha. `res` recebe negativo, zero ou positivo se a
/// chave de `val` é menor, igual ou maior que a corrente do sorter. É o núcleo do
/// `OP_SorterCompare`, que verifica unicidade ao montar um UNIQUE INDEX.
pub fn vdbe_sorter_compare(csr: &mut VdbeCursor, val: &Mem, n_key_col: i32, res: &mut i32) -> i32 {
    debug_assert!(csr.e_cur_type == CURTYPE_SORTER);
    let Some(key_info) = csr.p_key_info.clone() else {
        return SQLITE_INTERNAL;
    };
    let Some(sorter) = csr.p_sorter.as_deref_mut() else {
        return SQLITE_INTERNAL;
    };

    let mut r2 = match sorter.p_unpacked.take() {
        Some(r2) => r2,
        None => {
            let mut r2 = alloc_unpacked_record(Rc::clone(&key_info));
            r2.n_field = n_key_col as u16;
            Box::new(r2)
        }
    };

    record_unpack(&key_info, vdbe_sorter_rowkey_bytes(sorter), &mut r2);
    for i in 0..n_key_col.max(0) as usize {
        if r2.a_mem.get(i).map_or(false, |m| m.flags & MEM_NULL != 0) {
            *res = -1;
            sorter.p_unpacked = Some(r2);
            return SQLITE_OK;
        }
    }

    *res = record_compare(val.bytes(), &mut r2);
    sorter.p_unpacked = Some(r2);
    SQLITE_OK
}

// ---------------------------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::consts::SQLITE_IOERR_SHORT_READ;
    use crate::mem::ENC_UTF8;
    use crate::os::Vfs;

    /// Arquivo temporário em memória.
    struct MemFile {
        data: Vec<u8>,
    }

    impl VfsFile for MemFile {
        fn close(&mut self) -> i32 {
            SQLITE_OK
        }
        fn read(&mut self, buf: &mut [u8], offset: i64) -> i32 {
            let off = offset as usize;
            let avail = self.data.len().saturating_sub(off).min(buf.len());
            buf[..avail].copy_from_slice(&self.data[off..off + avail]);
            if avail < buf.len() {
                buf[avail..].fill(0);
                return SQLITE_IOERR_SHORT_READ;
            }
            SQLITE_OK
        }
        fn write(&mut self, buf: &[u8], offset: i64) -> i32 {
            let off = offset as usize;
            if self.data.len() < off + buf.len() {
                self.data.resize(off + buf.len(), 0);
            }
            self.data[off..off + buf.len()].copy_from_slice(buf);
            SQLITE_OK
        }
        fn truncate(&mut self, size: i64) -> i32 {
            self.data.truncate(size as usize);
            SQLITE_OK
        }
        fn sync(&mut self, _flags: i32) -> i32 {
            SQLITE_OK
        }
        fn file_size(&mut self, size: &mut i64) -> i32 {
            *size = self.data.len() as i64;
            SQLITE_OK
        }
        fn lock(&mut self, _lock_type: i32) -> i32 {
            SQLITE_OK
        }
        fn unlock(&mut self, _lock_type: i32) -> i32 {
            SQLITE_OK
        }
        fn check_reserved_lock(&mut self, res_out: &mut i32) -> i32 {
            *res_out = 0;
            SQLITE_OK
        }
        fn file_control(&mut self, _op: i32, _arg: &mut FileControlArg) -> i32 {
            crate::consts::SQLITE_NOTFOUND
        }
        fn device_characteristics(&mut self) -> i32 {
            0
        }
    }

    /// VFS que só sabe abrir arquivos temporários em memória.
    struct MemVfs;

    impl Vfs for MemVfs {
        fn name(&self) -> &[u8] {
            b"sorter-test"
        }
        fn max_pathname(&self) -> i32 {
            512
        }
        fn open(&self, _: Option<&[u8]>, _: i32, _: &mut i32) -> Result<Box<dyn VfsFile>, i32> {
            Ok(Box::new(MemFile { data: Vec::new() }))
        }
        fn access(&self, _: &[u8], _: i32, res_out: &mut i32) -> i32 {
            *res_out = 0;
            SQLITE_OK
        }
        fn full_pathname(&self, name: &[u8], _: i32, out: &mut Vec<u8>) -> i32 {
            out.extend_from_slice(name);
            SQLITE_OK
        }
        fn randomness(&self, out: &mut [u8]) -> i32 {
            out.fill(0);
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

    fn key_info() -> Rc<KeyInfo> {
        Rc::new(KeyInfo {
            enc: ENC_UTF8,
            n_key_field: 1,
            n_all_field: 1,
            a_sort_flags: vec![0],
            a_coll: vec![None],
        })
    }

    /// Um cursor-sorter já inicializado, com o limiar de PMA dado (`mx_pma_size == 0` é só memória).
    fn sorter_cursor(mx_pma_size: i32) -> VdbeCursor {
        let ki = key_info();
        let sorter = VdbeSorter {
            mn_pma_size: mx_pma_size,
            mx_pma_size,
            mx_keysize: 0,
            pgsz: 4096,
            p_reader: None,
            p_merger: None,
            p_vfs: Some(Arc::new(MemVfs)),
            n_max_sorter_mmap: 0x7fffffff,
            p_key_info: Rc::clone(&ki),
            p_unpacked: None,
            list: SorterList { a_memory: mx_pma_size != 0, ..SorterList::default() },
            i_memory: 0,
            n_memory: 4096,
            b_use_pma: false,
            b_use_threads: false,
            i_prev: 255,
            n_task: 1,
            type_mask: SORTER_TYPE_INTEGER | SORTER_TYPE_TEXT,
            a_task: vec![SortSubtask::default()],
            files: Vec::new(),
        };
        let mut csr = VdbeCursor::default();
        csr.e_cur_type = CURTYPE_SORTER;
        csr.p_key_info = Some(ki);
        csr.p_sorter = Some(Box::new(sorter));
        csr
    }

    fn record_mem(rec: &[u8]) -> Mem {
        Mem { flags: MEM_BLOB, n: rec.len() as i32, z: rec.to_vec(), ..Mem::default() }
    }

    /// Escreve os registros, ordena e devolve as chaves na ordem de saída.
    fn run(csr: &mut VdbeCursor, records: &[Vec<u8>]) -> Vec<Vec<u8>> {
        for r in records {
            assert_eq!(vdbe_sorter_write(csr, &record_mem(r)), SQLITE_OK);
        }
        let mut eof = 0;
        assert_eq!(vdbe_sorter_rewind(csr, &mut eof), SQLITE_OK);
        let mut out = Vec::new();
        if eof == 0 {
            loop {
                let mut m = Mem::default();
                assert_eq!(vdbe_sorter_rowkey(csr, &mut m), SQLITE_OK);
                out.push(m.bytes().to_vec());
                let rc = vdbe_sorter_next(csr);
                if rc == SQLITE_DONE {
                    break;
                }
                assert_eq!(rc, SQLITE_OK);
            }
        }
        vdbe_sorter_close(csr);
        out
    }

    /// Registro de um campo inteiro de um byte (tipo serial 1).
    fn int_record(v: i8) -> Vec<u8> {
        vec![2, 1, v as u8]
    }

    fn pseudo_ints(n: i64) -> Vec<i8> {
        (0..n).map(|i| (((i * 7919 + 13) % 200) - 100) as i8).collect()
    }

    #[test]
    fn sorts_integers_in_memory() {
        let mut csr = sorter_cursor(0);
        let vals = pseudo_ints(100);
        let recs: Vec<Vec<u8>> = vals.iter().map(|v| int_record(*v)).collect();
        let out = run(&mut csr, &recs);
        let mut want = vals.clone();
        want.sort();
        let got: Vec<i8> = out.iter().map(|r| r[2] as i8).collect();
        assert_eq!(got, want);
        assert!(csr.p_sorter.is_none());
    }

    #[test]
    fn empty_sorter_is_eof() {
        let mut csr = sorter_cursor(0);
        assert!(run(&mut csr, &[]).is_empty());
    }

    #[test]
    fn sorts_integers_through_pmas() {
        // Um PMA por registro: 300 PMAs, o que exige a árvore de mescla de profundidade 2.
        let mut csr = sorter_cursor(1);
        let vals = pseudo_ints(300);
        let recs: Vec<Vec<u8>> = vals.iter().map(|v| int_record(*v)).collect();
        let out = run(&mut csr, &recs);
        let mut want = vals.clone();
        want.sort();
        let got: Vec<i8> = out.iter().map(|r| r[2] as i8).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn sorts_integers_through_few_pmas() {
        // Poucos PMAs (menos de 16): um MergeEngine só, sem IncrMerger.
        let mut csr = sorter_cursor(200);
        let vals = pseudo_ints(60);
        let recs: Vec<Vec<u8>> = vals.iter().map(|v| int_record(*v)).collect();
        let out = run(&mut csr, &recs);
        let mut want = vals.clone();
        want.sort();
        let got: Vec<i8> = out.iter().map(|r| r[2] as i8).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn sorts_text() {
        let words: [&[u8]; 6] = [b"pear", b"apple", b"fig", b"banana", b"apricot", b"cherry"];
        let recs: Vec<Vec<u8>> = words
            .iter()
            .map(|w| {
                let mut r = vec![2u8, (13 + 2 * w.len()) as u8];
                r.extend_from_slice(w);
                r
            })
            .collect();
        for mx in [0, 1] {
            let mut csr = sorter_cursor(mx);
            let out = run(&mut csr, &recs);
            let got: Vec<&[u8]> = out.iter().map(|r| &r[2..]).collect();
            let mut want: Vec<&[u8]> = words.to_vec();
            want.sort();
            assert_eq!(got, want);
        }
    }
}
