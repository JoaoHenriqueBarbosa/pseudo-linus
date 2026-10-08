//! Journal de rollback em memória (memjournal.c).
//!
//! O journal em memória serve aos bancos `:memory:`, ao `journal_mode=MEMORY` e
//! guarda temporariamente journals pequenos (por exemplo os de statement) que não
//! precisam sobreviver a uma queda de energia. Quando o conteúdo passa de
//! `nSpill` bytes (ou quando `journal_create` é chamado) ele transborda para um
//! arquivo de verdade aberto pelo VFS subjacente.
//!
//! Modelagem:
//!
//! * A lista encadeada de `FileChunk` do C é um `Vec<Vec<u8>>`: cada elemento é
//!   um bloco de `n_chunk_size` bytes. O ponteiro `endpoint.pChunk` do C é sempre
//!   o último bloco, então vem de `chunks.len() - 1` e não é guardado. O ponteiro
//!   `readpoint.pChunk` é um índice (`read_chunk`).
//! * No C, depois do transbordo o próprio objeto de arquivo vira o arquivo real
//!   (a tabela de métodos é trocada). Aqui o `MemJournal` guarda
//!   `real: Option<Box<dyn VfsFile>>` e, com `real` preenchido, todo método
//!   delega a ele. É também o caso de `nSpill == 0`: o arquivo nasce real.
//! * `sqlite3JournalIsInMemory` (`pMethods == &MemJournalMethods`) é `real.is_none()`.

use crate::consts::{
    SQLITE_ERROR, SQLITE_IOERR_SHORT_READ, SQLITE_NOTFOUND, SQLITE_OK, SQLITE_OPEN_MAIN_JOURNAL,
};
use crate::os::{os_open, FileControlArg, ShmRegion, VfsFile, VfsRef};

/// `MEMJOURNAL_DFLT_FILECHUNKSIZE`: bytes alocados por `FileChunk` por padrão.
const MEMJOURNAL_DFLT_FILECHUNKSIZE: i32 = 1024;

/// `sizeof(FileChunk)` num alvo de 64 bits: o ponteiro `pNext` (8) mais `zChunk[8]`.
const FILE_CHUNK_STRUCT_SIZE: i32 = 16;

/// Tamanho do bloco quando `nSpill < 0`:
/// `8 + MEMJOURNAL_DFLT_FILECHUNKSIZE - sizeof(FileChunk)`.
const DFLT_CHUNK_SIZE: i32 = 8 + MEMJOURNAL_DFLT_FILECHUNKSIZE - FILE_CHUNK_STRUCT_SIZE;

/// Cursor de leitura: o `FilePoint` do C, com o bloco como índice.
#[derive(Debug, Clone, Copy, Default)]
struct ReadPoint {
    /// Deslocamento a partir do começo do arquivo.
    offset: i64,
    /// Bloco para onde o cursor aponta (`None` é o ponteiro nulo).
    chunk: Option<usize>,
}

/// Arquivo de journal (a estrutura `MemJournal`, subclasse de `sqlite3_file`).
pub struct MemJournal {
    /// Tamanho de cada bloco em memória.
    n_chunk_size: i32,
    /// Bytes de dados antes de descarregar em disco.
    n_spill: i32,
    /// Lista de blocos em memória (`pFirst` e os `pNext`).
    chunks: Vec<Vec<u8>>,
    /// `endpoint.iOffset`: o fim do arquivo.
    end_offset: i64,
    /// `readpoint`: onde terminou a última leitura.
    readpoint: ReadPoint,
    /// Flags do `xOpen`.
    flags: i32,
    /// O VFS "de verdade" por baixo.
    vfs: Option<VfsRef>,
    /// Nome do arquivo de journal (`zJournal`), sem o NUL.
    journal_name: Option<Vec<u8>>,
    /// O arquivo real, depois do transbordo (ou desde o início com `nSpill == 0`).
    real: Option<Box<dyn VfsFile>>,
}

impl MemJournal {
    /// O objeto zerado do `memset(p, 0, sizeof(MemJournal))`.
    fn blank() -> MemJournal {
        MemJournal {
            n_chunk_size: 0,
            n_spill: 0,
            chunks: Vec::new(),
            end_offset: 0,
            readpoint: ReadPoint::default(),
            flags: 0,
            vfs: None,
            journal_name: None,
            real: None,
        }
    }

    /// Bloco seguinte a `i` na lista (`pChunk->pNext`).
    fn next_chunk(&self, i: usize) -> Option<usize> {
        if i + 1 < self.chunks.len() {
            Some(i + 1)
        } else {
            None
        }
    }

    /// `memjrnlCreateFile`: descarrega o conteúdo da memória para um arquivo de
    /// verdade. Em erro o estado em memória fica intacto, de modo que o SQLite
    /// continua podendo reverter as mudanças do cache de páginas a partir dele.
    fn create_file(&mut self) -> i32 {
        // `copy.pVfs` nulo só ocorreria com `nSpill < 0`, onde o C nunca chega aqui.
        let Some(vfs) = self.vfs.clone() else {
            return SQLITE_ERROR;
        };
        let mut out_flags = 0;
        let mut file = match os_open(&*vfs, self.journal_name.as_deref(), self.flags, &mut out_flags) {
            Ok(f) => f,
            Err(rc) => return rc,
        };
        let mut rc = SQLITE_OK;
        let mut n_chunk = self.n_chunk_size as i64;
        let mut off: i64 = 0;
        for chunk in &self.chunks {
            if off + n_chunk > self.end_offset {
                n_chunk = self.end_offset - off;
            }
            rc = file.write(&chunk[..n_chunk as usize], off);
            if rc != SQLITE_OK {
                break;
            }
            off += n_chunk;
        }
        if rc != SQLITE_OK {
            // Erro ao criar ou escrever: fecha o arquivo e mantém o original.
            file.close();
            return rc;
        }
        // Nenhum erro: libera os buffers em memória e o arquivo real assume.
        *self = MemJournal::blank();
        self.real = Some(file);
        SQLITE_OK
    }
}

impl VfsFile for MemJournal {
    fn i_version(&self) -> i32 {
        match &self.real {
            Some(r) => r.i_version(),
            None => 1,
        }
    }

    fn is_in_memory_journal(&self) -> bool {
        journal_is_in_memory(self)
    }

    /// `memjrnlClose`: libera os blocos (`memjrnlFreeChunks`).
    fn close(&mut self) -> i32 {
        if let Some(r) = self.real.as_mut() {
            return r.close();
        }
        self.chunks.clear();
        SQLITE_OK
    }

    /// `memjrnlRead`.
    fn read(&mut self, buf: &mut [u8], offset: i64) -> i32 {
        if let Some(r) = self.real.as_mut() {
            return r.read(buf, offset);
        }
        let i_amt = buf.len() as i64;
        let i_ofst = offset;
        let n = self.n_chunk_size as i64;

        if i_amt + i_ofst > self.end_offset {
            buf.fill(0);
            return SQLITE_IOERR_SHORT_READ;
        }
        debug_assert!(self.readpoint.offset == 0 || self.readpoint.chunk.is_some());
        let start: Option<usize>;
        if self.readpoint.offset != i_ofst || i_ofst == 0 {
            let mut i_off: i64 = 0;
            let mut c = if self.chunks.is_empty() { None } else { Some(0) };
            while let Some(i) = c {
                if i_off + n <= i_ofst {
                    i_off += n;
                    c = self.next_chunk(i);
                } else {
                    break;
                }
            }
            start = c;
        } else {
            start = self.readpoint.chunk;
            debug_assert!(start.is_some());
        }
        // O C dereferenciaria um bloco nulo aqui (`ALWAYS(pChunk)`).
        let Some(first) = start else {
            buf.fill(0);
            return SQLITE_IOERR_SHORT_READ;
        };

        let mut cur: Option<usize> = Some(first);
        let mut ci = first;
        let mut n_read = i_amt;
        let mut chunk_offset = (i_ofst % n) as usize;
        let mut out_pos = 0usize;
        loop {
            let i_space = n - chunk_offset as i64;
            let n_copy = n_read.min(n - chunk_offset as i64) as usize;
            buf[out_pos..out_pos + n_copy]
                .copy_from_slice(&self.chunks[ci][chunk_offset..chunk_offset + n_copy]);
            out_pos += n_copy;
            n_read -= i_space;
            chunk_offset = 0;
            // while( nRead>=0 && (pChunk=pChunk->pNext)!=0 && nRead>0 )
            if n_read < 0 {
                break;
            }
            cur = self.next_chunk(ci);
            match cur {
                None => break,
                Some(x) => ci = x,
            }
            if n_read <= 0 {
                break;
            }
        }
        self.readpoint.offset = if cur.is_some() { i_ofst + i_amt } else { 0 };
        self.readpoint.chunk = cur;
        SQLITE_OK
    }

    /// `memjrnlWrite`.
    fn write(&mut self, buf: &[u8], offset: i64) -> i32 {
        if let Some(r) = self.real.as_mut() {
            return r.write(buf, offset);
        }
        let i_amt = buf.len() as i64;
        let i_ofst = offset;
        let mut n_write = buf.len();
        let mut pos = 0usize;

        // Se o arquivo deve ser criado agora, cria e grava os dados novos nele.
        if self.n_spill > 0 && (i_amt + i_ofst) > self.n_spill as i64 {
            let rc = self.create_file();
            if rc != SQLITE_OK {
                return rc;
            }
            return match self.real.as_mut() {
                Some(r) => r.write(buf, offset),
                None => SQLITE_ERROR,
            };
        }

        // O conteúdo desta escrita fica em memória. Um journal em memória só
        // recebe appends; a exceção é a otimização de escrita atômica, em que os
        // primeiros 28 bytes podem ser regravados no commit.
        debug_assert!(i_ofst <= self.end_offset);
        if i_ofst > 0 && i_ofst != self.end_offset {
            self.truncate(i_ofst);
        }
        if i_ofst == 0 && !self.chunks.is_empty() {
            debug_assert!(self.n_chunk_size as i64 > i_amt);
            self.chunks[0][..buf.len()].copy_from_slice(buf);
        } else {
            let n = self.n_chunk_size as i64;
            while n_write > 0 {
                let chunk_offset = (self.end_offset % n) as usize;
                let i_space = n_write.min(n as usize - chunk_offset);

                debug_assert!(!self.chunks.is_empty() || chunk_offset == 0);
                if chunk_offset == 0 {
                    // Um bloco novo é preciso para estender o arquivo.
                    self.chunks.push(vec![0u8; n as usize]);
                }
                let last = self.chunks.len() - 1;
                self.chunks[last][chunk_offset..chunk_offset + i_space]
                    .copy_from_slice(&buf[pos..pos + i_space]);
                pos += i_space;
                n_write -= i_space;
                self.end_offset += i_space as i64;
            }
        }
        SQLITE_OK
    }

    /// `memjrnlTruncate`.
    fn truncate(&mut self, size: i64) -> i32 {
        if let Some(r) = self.real.as_mut() {
            return r.truncate(size);
        }
        if size < self.end_offset {
            if size == 0 {
                self.chunks.clear();
            } else {
                let n = self.n_chunk_size as i64;
                let mut i_off = n;
                let mut idx = 0usize;
                while idx < self.chunks.len() && i_off < size {
                    i_off += n;
                    idx += 1;
                }
                if idx < self.chunks.len() {
                    self.chunks.truncate(idx + 1);
                }
            }
            self.end_offset = size;
            self.readpoint.chunk = None;
            self.readpoint.offset = 0;
        }
        SQLITE_OK
    }

    /// `memjrnlSync`: sincronizar um journal em memória não faz nada; com o
    /// arquivo real criado, vale o `xSync` dele.
    fn sync(&mut self, flags: i32) -> i32 {
        match self.real.as_mut() {
            Some(r) => r.sync(flags),
            None => SQLITE_OK,
        }
    }

    /// `memjrnlFileSize`.
    fn file_size(&mut self, size: &mut i64) -> i32 {
        if let Some(r) = self.real.as_mut() {
            return r.file_size(size);
        }
        *size = self.end_offset;
        SQLITE_OK
    }

    /// `xLock` é nulo na tabela do journal em memória: nunca é chamado.
    fn lock(&mut self, lock_type: i32) -> i32 {
        match self.real.as_mut() {
            Some(r) => r.lock(lock_type),
            None => SQLITE_OK,
        }
    }

    /// `xUnlock` é nulo na tabela do journal em memória: nunca é chamado.
    fn unlock(&mut self, lock_type: i32) -> i32 {
        match self.real.as_mut() {
            Some(r) => r.unlock(lock_type),
            None => SQLITE_OK,
        }
    }

    /// `xCheckReservedLock` é nulo na tabela do journal em memória.
    fn check_reserved_lock(&mut self, res_out: &mut i32) -> i32 {
        match self.real.as_mut() {
            Some(r) => r.check_reserved_lock(res_out),
            None => {
                *res_out = 0;
                SQLITE_OK
            }
        }
    }

    /// `xFileControl` é nulo na tabela do journal em memória.
    fn file_control(&mut self, op: i32, arg: &mut FileControlArg) -> i32 {
        match self.real.as_mut() {
            Some(r) => r.file_control(op, arg),
            None => SQLITE_NOTFOUND,
        }
    }

    fn sector_size(&mut self) -> i32 {
        match self.real.as_mut() {
            Some(r) => r.sector_size(),
            None => crate::consts::SQLITE_DEFAULT_SECTOR_SIZE as i32,
        }
    }

    /// `xDeviceCharacteristics` é nulo na tabela do journal em memória.
    fn device_characteristics(&mut self) -> i32 {
        match self.real.as_mut() {
            Some(r) => r.device_characteristics(),
            None => 0,
        }
    }

    fn shm_map(&mut self, i_page: i32, pgsz: i32, b_extend: i32, pp: &mut Option<ShmRegion>) -> i32 {
        match self.real.as_mut() {
            Some(r) => r.shm_map(i_page, pgsz, b_extend, pp),
            None => {
                *pp = None;
                crate::consts::SQLITE_IOERR_SHMMAP
            }
        }
    }

    fn shm_read(&mut self, region: &ShmRegion, offset: usize, buf: &mut [u8]) -> i32 {
        match self.real.as_mut() {
            Some(r) => r.shm_read(region, offset, buf),
            None => crate::consts::SQLITE_IOERR_SHMMAP,
        }
    }

    fn shm_write(&mut self, region: &ShmRegion, offset: usize, data: &[u8]) -> i32 {
        match self.real.as_mut() {
            Some(r) => r.shm_write(region, offset, data),
            None => crate::consts::SQLITE_IOERR_SHMMAP,
        }
    }

    fn shm_lock(&mut self, offset: i32, n: i32, flags: i32) -> i32 {
        match self.real.as_mut() {
            Some(r) => r.shm_lock(offset, n, flags),
            None => crate::consts::SQLITE_IOERR_SHMLOCK,
        }
    }

    fn shm_barrier(&mut self) {
        if let Some(r) = self.real.as_mut() {
            r.shm_barrier();
        }
    }

    fn shm_unmap(&mut self, delete_flag: i32) -> i32 {
        match self.real.as_mut() {
            Some(r) => r.shm_unmap(delete_flag),
            None => SQLITE_OK,
        }
    }

    fn fetch(&mut self, ofst: i64, amt: i32, pp: &mut Option<Vec<u8>>) -> i32 {
        match self.real.as_mut() {
            Some(r) => r.fetch(ofst, amt, pp),
            None => {
                *pp = None;
                SQLITE_OK
            }
        }
    }

    fn unfetch(&mut self, ofst: i64, p: Option<Vec<u8>>) -> i32 {
        match self.real.as_mut() {
            Some(r) => r.unfetch(ofst, p),
            None => SQLITE_OK,
        }
    }
}

/// `sqlite3JournalOpen`: abre um arquivo de journal.
///
/// Com `n_spill == 0` o arquivo é sempre criado e acessado pelo VFS subjacente e
/// nada deste módulo roda nas chamadas seguintes. Com `n_spill < 0` todo o
/// conteúdo fica na memória. Com `n_spill > 0` o journal nasce em memória e é
/// descarregado em disco quando passa de `n_spill` bytes ou quando
/// `journal_create` é chamado.
pub fn journal_open(
    vfs: Option<VfsRef>,
    name: Option<&[u8]>,
    flags: i32,
    n_spill: i32,
) -> Result<MemJournal, i32> {
    debug_assert!(
        name.is_some() || n_spill < 0 || (flags & crate::consts::SQLITE_OPEN_EXCLUSIVE) != 0
    );

    let mut p = MemJournal::blank();
    if n_spill == 0 {
        // O `pVfs` nulo faria o C desreferenciar um ponteiro nulo.
        let Some(vfs) = vfs else {
            return Err(SQLITE_ERROR);
        };
        let mut out_flags = 0;
        p.real = Some(os_open(&*vfs, name, flags, &mut out_flags)?);
        return Ok(p);
    }

    if n_spill > 0 {
        p.n_chunk_size = n_spill;
    } else {
        p.n_chunk_size = DFLT_CHUNK_SIZE;
        debug_assert!(
            MEMJOURNAL_DFLT_FILECHUNKSIZE == FILE_CHUNK_STRUCT_SIZE + (p.n_chunk_size - 8)
        );
    }

    p.n_spill = n_spill;
    p.flags = flags;
    p.journal_name = name.map(|n| n.to_vec());
    p.vfs = vfs;
    Ok(p)
}

/// `sqlite3MemJournalOpen`: abre um journal só em memória.
pub fn mem_journal_open() -> MemJournal {
    // `journal_open` com `nSpill < 0` não tem caminho de erro.
    match journal_open(None, None, 0, -1) {
        Ok(p) => p,
        Err(_) => MemJournal::blank(),
    }
}

/// `sqlite3JournalCreate` (sob `SQLITE_ENABLE_ATOMIC_WRITE` e
/// `SQLITE_ENABLE_BATCH_ATOMIC_WRITE` no C): se o argumento é um journal que não
/// é só em memória (aberto com `nSpill` positivo ou como
/// `SQLITE_OPEN_MAIN_JOURNAL`) e o arquivo ainda não foi criado, cria agora.
pub fn journal_create(p: &mut MemJournal) -> i32 {
    let mut rc = SQLITE_OK;
    if p.real.is_none() && (p.n_spill > 0 || (p.flags & SQLITE_OPEN_MAIN_JOURNAL) != 0) {
        rc = p.create_file();
    }
    rc
}

/// `sqlite3JournalIsInMemory`: verdadeiro se o "arquivo" de journal está
/// guardado na memória do heap.
pub fn journal_is_in_memory(p: &MemJournal) -> bool {
    p.real.is_none()
}

/// `sqlite3JournalSize`: no C é o `MAX(pVfs->szOsFile, sizeof(MemJournal))`
/// necessário para pré-alocar o objeto de arquivo. Em Rust o objeto é um valor
/// próprio e o `szOsFile` não existe; devolve o tamanho do próprio `MemJournal`.
pub fn journal_size() -> i32 {
    std::mem::size_of::<MemJournal>() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::{Vfs, VfsRef};
    use std::sync::{Arc, Mutex};

    /// Arquivo real de teste: guarda os bytes num `Vec` compartilhado.
    struct VecFile(Arc<Mutex<Vec<u8>>>);

    impl VfsFile for VecFile {
        fn close(&mut self) -> i32 {
            SQLITE_OK
        }
        fn read(&mut self, buf: &mut [u8], offset: i64) -> i32 {
            let d = self.0.lock().unwrap();
            let o = offset as usize;
            buf.copy_from_slice(&d[o..o + buf.len()]);
            SQLITE_OK
        }
        fn write(&mut self, buf: &[u8], offset: i64) -> i32 {
            let mut d = self.0.lock().unwrap();
            let o = offset as usize;
            if d.len() < o + buf.len() {
                d.resize(o + buf.len(), 0);
            }
            d[o..o + buf.len()].copy_from_slice(buf);
            SQLITE_OK
        }
        fn truncate(&mut self, size: i64) -> i32 {
            self.0.lock().unwrap().truncate(size as usize);
            SQLITE_OK
        }
        fn sync(&mut self, _: i32) -> i32 {
            SQLITE_OK
        }
        fn file_size(&mut self, size: &mut i64) -> i32 {
            *size = self.0.lock().unwrap().len() as i64;
            SQLITE_OK
        }
        fn lock(&mut self, _: i32) -> i32 {
            SQLITE_OK
        }
        fn unlock(&mut self, _: i32) -> i32 {
            SQLITE_OK
        }
        fn check_reserved_lock(&mut self, r: &mut i32) -> i32 {
            *r = 0;
            SQLITE_OK
        }
        fn file_control(&mut self, _: i32, _: &mut FileControlArg) -> i32 {
            SQLITE_NOTFOUND
        }
        fn device_characteristics(&mut self) -> i32 {
            0
        }
    }

    struct VecVfs(Arc<Mutex<Vec<u8>>>);

    impl Vfs for VecVfs {
        fn name(&self) -> &[u8] {
            b"memjournal-test"
        }
        fn max_pathname(&self) -> i32 {
            512
        }
        fn open(&self, _: Option<&[u8]>, _: i32, _: &mut i32) -> Result<Box<dyn VfsFile>, i32> {
            Ok(Box::new(VecFile(self.0.clone())))
        }
        fn access(&self, _: &[u8], _: i32, r: &mut i32) -> i32 {
            *r = 0;
            SQLITE_OK
        }
        fn full_pathname(&self, n: &[u8], _: i32, out: &mut Vec<u8>) -> i32 {
            out.extend_from_slice(n);
            SQLITE_OK
        }
        fn randomness(&self, _: &mut [u8]) -> i32 {
            SQLITE_OK
        }
        fn sleep(&self, m: i32) -> i32 {
            m
        }
        fn current_time(&self, o: &mut f64) -> i32 {
            *o = 0.0;
            SQLITE_OK
        }
    }

    #[test]
    fn append_read_truncate_in_memory() {
        let mut j = mem_journal_open();
        let data: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
        assert_eq!(j.write(&data, 0), SQLITE_OK);
        let mut sz = 0;
        j.file_size(&mut sz);
        assert_eq!(sz, 3000);
        let mut out = vec![0u8; 3000];
        assert_eq!(j.read(&mut out, 0), SQLITE_OK);
        assert_eq!(out, data);
        let mut part = vec![0u8; 100];
        assert_eq!(j.read(&mut part, 1500), SQLITE_OK);
        assert_eq!(&part[..], &data[1500..1600]);
        assert_eq!(j.read(&mut part, 2950), SQLITE_IOERR_SHORT_READ);
        j.truncate(1000);
        j.file_size(&mut sz);
        assert_eq!(sz, 1000);
        assert_eq!(j.write(&data[..10], 1000), SQLITE_OK);
        let mut out = vec![0u8; 1010];
        assert_eq!(j.read(&mut out, 0), SQLITE_OK);
        assert_eq!(&out[..1000], &data[..1000]);
        assert!(journal_is_in_memory(&j));
    }

    #[test]
    fn spill_to_real_file() {
        let store = Arc::new(Mutex::new(Vec::new()));
        let vfs: VfsRef = Arc::new(VecVfs(store.clone()));
        let mut j = journal_open(Some(vfs), Some(b"j"), 0, 64).unwrap();
        let data: Vec<u8> = (0..100u8).collect();
        assert_eq!(j.write(&data[..40], 0), SQLITE_OK);
        assert!(journal_is_in_memory(&j));
        assert_eq!(j.write(&data[40..], 40), SQLITE_OK);
        assert!(!journal_is_in_memory(&j));
        assert_eq!(&store.lock().unwrap()[..], &data[..]);
    }
}
