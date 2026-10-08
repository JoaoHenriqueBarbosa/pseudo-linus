// Mesclado das partes traduzidas de memjournal_c (as partes quebram funções grandes no meio).
#![allow(unused_imports, ambiguous_glob_reexports)]
use crate::prelude::*;

// ---- part_000.rs ----

// Modelo: o `MemJournal` do C é uma subclasse de `sqlite3_file` alocada no próprio handle. Aqui o
// `MemJournal` vive dentro de `Sqlite3File.p_methods` (`Box<dyn VfsFile>`), e a tabela
// `MemJournalMethods` é a implementação de `VfsFile` para ele. Quando o journal sofre spill para
// disco (`memjrnlCreateFile`), o C reabre o MESMO handle com o VFS real; aqui o arquivo real fica em
// `p_real` e todos os métodos passam a delegar a ele (equivale a trocar `pMethods`).

/// Nó da lista de blocos do journal em memória.
///
/// O C usa lista encadeada (`pNext`); aqui a lista é `MemJournal::chunks` e cada "ponteiro para
/// bloco" é um índice nela. O bloco tem sempre `n_chunk_size` bytes (no mínimo 8).
#[derive(Debug)]
pub struct FileChunk {
    /// Conteúdo deste bloco.
    pub z_chunk: Vec<u8>,
}

/// Tamanho padrão de alocação, em bytes, de cada `FileChunk`.
pub const MEMJOURNAL_DFLT_FILECHUNKSIZE: usize = 1024;

/// `sizeof(FileChunk)` do C em 64 bits (ponteiro `pNext` de 8 bytes mais `zChunk[8]`). Fixo de
/// propósito: o tamanho padrão do bloco (`1016`) precisa sair idêntico ao do C.
pub const FILE_CHUNK_STRUCT_SIZE: usize = 16;

/// `sizeof(MemJournal)` do C em 64 bits, usado por `journal_size`.
pub const MEM_JOURNAL_STRUCT_SIZE: i32 = 80;

/// Para o tamanho de bloco `n_chunk_size`, número de bytes que o C aloca por `FileChunk`.
#[inline]
pub fn file_chunk_size(n_chunk_size: usize) -> usize {
    FILE_CHUNK_STRUCT_SIZE + (n_chunk_size - 8)
}

/// Cursor de leitura ou escrita no journal. `p_chunk` é o índice do bloco em `MemJournal::chunks`.
#[derive(Debug, Clone, Default)]
pub struct FilePoint {
    /// Offset a partir do início do arquivo.
    pub i_offset: i64,
    /// Bloco para o qual o cursor aponta.
    pub p_chunk: Option<usize>,
}

/// Journal em memória: cada journal aberto com `journal_open` (n_spill != 0) é uma instância.
#[derive(Default)]
pub struct MemJournal {
    /// Tamanho dos blocos em memória.
    pub n_chunk_size: i32,
    /// Bytes de dados antes do spill para disco.
    pub n_spill: i32,
    /// Lista de blocos em memória (`pFirst` é `chunks[0]`).
    pub chunks: Vec<FileChunk>,
    /// Fim do arquivo.
    pub endpoint: FilePoint,
    /// Fim da última leitura (`xRead`).
    pub readpoint: FilePoint,
    /// Flags do `xOpen`.
    pub flags: i32,
    /// O VFS real por baixo.
    pub p_vfs: Option<VfsRef>,
    /// Nome do arquivo do journal.
    pub z_journal: Option<Vec<u8>>,
    /// Arquivo real, depois do spill. Com ele presente, os métodos delegam a ele.
    pub p_real: Option<Sqlite3File>,
}

/// Despeja o conteúdo da memória num arquivo real em disco.
pub fn memjrnl_create_file(p: &mut MemJournal) -> i32 {
    let mut real = Sqlite3File::default();
    let p_vfs = p.p_vfs.clone().expect("memjrnl_create_file: sem VFS");
    let mut rc = os_open(&*p_vfs, p.z_journal.as_deref(), &mut real, p.flags, None);
    if rc == SQLITE_OK {
        let mut n_chunk = p.n_chunk_size as i64;
        let mut i_off: i64 = 0;
        for p_iter in p.chunks.iter() {
            if i_off + n_chunk > p.endpoint.i_offset {
                n_chunk = p.endpoint.i_offset - i_off;
            }
            rc = os_write(&mut real, &p_iter.z_chunk[..n_chunk as usize], i_off);
            if rc != 0 {
                break;
            }
            i_off += n_chunk;
        }
        if rc == SQLITE_OK {
            // Nenhum erro: libera os buffers em memória e zera o objeto (o memset do C),
            // ficando só o arquivo real.
            *p = MemJournal {
                p_real: Some(real),
                ..MemJournal::default()
            };
            return rc;
        }
    }
    // Erro ao criar ou escrever no arquivo: o original permanece intacto, para o SQLite usar os
    // dados em memória no rollback das mudanças do cache de páginas.
    os_close(&mut real);
    rc
}

impl VfsFile for MemJournal {
    /// `memjrnlRead`: lê dados do journal em memória.
    fn x_read(&mut self, z_buf: &mut [u8], i_ofst: i64) -> i32 {
        if let Some(real) = self.p_real.as_mut() {
            return os_read(real, z_buf, i_ofst);
        }
        let i_amt = z_buf.len() as i64;
        let mut n_read = i_amt;
        let mut z_out = 0usize;

        if i_amt + i_ofst > self.endpoint.i_offset {
            return SQLITE_IOERR_SHORT_READ;
        }
        debug_assert!(self.readpoint.i_offset == 0 || self.readpoint.p_chunk.is_some());
        let mut p_chunk: Option<usize>;
        if self.readpoint.i_offset != i_ofst || i_ofst == 0 {
            let mut i_off: i64 = 0;
            let mut cur = if self.chunks.is_empty() { None } else { Some(0usize) };
            while let Some(i) = cur {
                if (i_off + self.n_chunk_size as i64) > i_ofst {
                    break;
                }
                i_off += self.n_chunk_size as i64;
                cur = if i + 1 < self.chunks.len() { Some(i + 1) } else { None };
            }
            p_chunk = cur;
        } else {
            p_chunk = self.readpoint.p_chunk;
            debug_assert!(p_chunk.is_some());
        }

        let mut i_chunk_offset = (i_ofst % self.n_chunk_size as i64) as i64;
        loop {
            let i_space = self.n_chunk_size as i64 - i_chunk_offset;
            let n_copy = n_read.min(self.n_chunk_size as i64 - i_chunk_offset);
            let chunk = &self.chunks[p_chunk.expect("memjrnl_read: bloco ausente")];
            let src = i_chunk_offset as usize;
            z_buf[z_out..z_out + n_copy as usize]
                .copy_from_slice(&chunk.z_chunk[src..src + n_copy as usize]);
            z_out += n_copy as usize;
            n_read -= i_space;
            i_chunk_offset = 0;
            // while( nRead>=0 && (pChunk=pChunk->pNext)!=0 && nRead>0 )
            if !(n_read >= 0) {
                break;
            }
            let next = p_chunk.unwrap() + 1;
            p_chunk = if next < self.chunks.len() { Some(next) } else { None };
            if p_chunk.is_none() || !(n_read > 0) {
                break;
            }
        }
        self.readpoint.i_offset = if p_chunk.is_some() { i_ofst + i_amt } else { 0 };
        self.readpoint.p_chunk = p_chunk;

        SQLITE_OK
    }

    /// `memjrnlWrite`: escreve dados no arquivo. `memjrnlFreeChunks` some: soltar o `Vec` basta.
    fn x_write(&mut self, z_buf: &[u8], i_ofst: i64) -> i32 {
        if let Some(real) = self.p_real.as_mut() {
            return os_write(real, z_buf, i_ofst);
        }
        let i_amt = z_buf.len() as i64;
        let mut n_write = i_amt;
        let mut z_write = 0usize;

        // Se o arquivo deve ser criado agora, cria e escreve os dados novos nele.
        if self.n_spill > 0 && (i_amt + i_ofst) > self.n_spill as i64 {
            let mut rc = memjrnl_create_file(self);
            if rc == SQLITE_OK {
                rc = os_write(self.p_real.as_mut().expect("spill sem arquivo real"), z_buf, i_ofst);
            }
            return rc;
        }

        // O conteúdo desta escrita fica em memória. O journal em memória só recebe appends; a
        // exceção é a otimização de escrita atômica, que reescreve os primeiros 28 bytes.
        debug_assert!(i_ofst <= self.endpoint.i_offset);
        if i_ofst > 0 && i_ofst != self.endpoint.i_offset {
            self.x_truncate(i_ofst);
        }
        if i_ofst == 0 && !self.chunks.is_empty() {
            debug_assert!(self.n_chunk_size as i64 > i_amt);
            self.chunks[0].z_chunk[..i_amt as usize].copy_from_slice(z_buf);
        } else {
            while n_write > 0 {
                let mut p_chunk = self.endpoint.p_chunk;
                let i_chunk_offset = self.endpoint.i_offset % self.n_chunk_size as i64;
                let i_space = n_write.min(self.n_chunk_size as i64 - i_chunk_offset);

                debug_assert!(p_chunk.is_some() || i_chunk_offset == 0);
                if i_chunk_offset == 0 {
                    // Novo bloco para estender o arquivo (sempre vai ao fim da lista).
                    debug_assert!(p_chunk.is_some() == !self.chunks.is_empty());
                    self.chunks.push(FileChunk {
                        z_chunk: vec![0u8; self.n_chunk_size as usize],
                    });
                    p_chunk = Some(self.chunks.len() - 1);
                    self.endpoint.p_chunk = p_chunk;
                }

                debug_assert!(p_chunk.is_some());
                let dst = i_chunk_offset as usize;
                let n = i_space as usize;
                self.chunks[p_chunk.unwrap()].z_chunk[dst..dst + n]
                    .copy_from_slice(&z_buf[z_write..z_write + n]);
                z_write += n;
                n_write -= i_space;
                self.endpoint.i_offset += i_space;
            }
        }

        SQLITE_OK
    }

    /// `memjrnlTruncate`: trunca o arquivo em memória.
    fn x_truncate(&mut self, size: i64) -> i32 {
        if let Some(real) = self.p_real.as_mut() {
            return os_truncate(real, size);
        }
        debug_assert!(
            self.endpoint.p_chunk.is_none() || self.endpoint.p_chunk == Some(self.chunks.len() - 1)
        );
        if size < self.endpoint.i_offset {
            let mut p_iter: Option<usize> = None;
            if size == 0 {
                self.chunks.clear();
            } else {
                let mut i_off = self.n_chunk_size as i64;
                let mut i = 0usize;
                while i < self.chunks.len() && i_off < size {
                    i_off += self.n_chunk_size as i64;
                    i += 1;
                }
                if i < self.chunks.len() {
                    p_iter = Some(i);
                    self.chunks.truncate(i + 1);
                }
            }

            self.endpoint.p_chunk = p_iter;
            self.endpoint.i_offset = size;
            self.readpoint.p_chunk = None;
            self.readpoint.i_offset = 0;
        }
        SQLITE_OK
    }

    /// `memjrnlClose`: fecha o arquivo.
    fn x_close(&mut self) {
        if let Some(real) = self.p_real.as_mut() {
            os_close(real);
            return;
        }
        self.chunks.clear();
    }

    /// `memjrnlSync`: sem arquivo real, sincronizar o journal em memória é um no-op; com ele,
    /// chama o `xSync` do arquivo real.
    fn x_sync(&mut self, flags: i32) -> i32 {
        if let Some(real) = self.p_real.as_mut() {
            return real.p_methods.as_mut().unwrap().x_sync(flags);
        }
        SQLITE_OK
    }

    /// `memjrnlFileSize`: tamanho do arquivo em bytes.
    fn x_file_size(&mut self, p_size: &mut i64) -> i32 {
        if let Some(real) = self.p_real.as_mut() {
            return os_file_size(real, p_size);
        }
        *p_size = self.endpoint.i_offset;
        SQLITE_OK
    }

    /// Substitui a comparação `pMethods==&MemJournalMethods` do C (`journal_is_in_memory`):
    /// verdadeiro enquanto o journal não sofreu spill para o arquivo real.
    fn is_mem_journal(&self) -> bool {
        self.p_real.is_none()
    }
}

/// Abre um arquivo de journal (`sqlite3JournalOpen`).
///
/// Com `n_spill` 0, o journal é sempre criado e acessado pelo VFS real. Menor que zero, todo o
/// conteúdo fica na memória. Positivo, o journal nasce em memória e vai para disco quando passa de
/// `n_spill` bytes ou quando `journal_create` é chamado.
pub fn journal_open(
    p_vfs: Option<VfsRef>,
    z_name: Option<&[u8]>,
    p_jfd: &mut Sqlite3File,
    flags: i32,
    n_spill: i32,
) -> i32 {
    debug_assert!(z_name.is_some() || n_spill < 0 || (flags & SQLITE_OPEN_EXCLUSIVE) != 0);

    // Zera o handle (o memset do C). Com n_spill 0 inicializa pelo `os_open` do VFS real, e nada
    // deste módulo roda nas chamadas feitas depois no handle.
    *p_jfd = Sqlite3File::default();
    if n_spill == 0 {
        return os_open(&*p_vfs.expect("journal_open: sem VFS"), z_name, p_jfd, flags, None);
    }

    let mut p = MemJournal::default();
    if n_spill > 0 {
        p.n_chunk_size = n_spill;
    } else {
        p.n_chunk_size = (8 + MEMJOURNAL_DFLT_FILECHUNKSIZE - FILE_CHUNK_STRUCT_SIZE) as i32;
        debug_assert!(MEMJOURNAL_DFLT_FILECHUNKSIZE == file_chunk_size(p.n_chunk_size as usize));
    }

    p.n_spill = n_spill;
    p.flags = flags;
    p.z_journal = z_name.map(|z| z.to_vec());
    p.p_vfs = p_vfs;
    p_jfd.p_methods = Some(Box::new(p));
    SQLITE_OK
}


// ---- part_001.rs ----

/// Abre um arquivo de journal em memória (`sqlite3MemJournalOpen`).
pub fn mem_journal_open(p_jfd: &mut Sqlite3File) {
    journal_open(None, None, p_jfd, 0, -1);
}

// `sqlite3JournalCreate` só existe sob `SQLITE_ENABLE_ATOMIC_WRITE` ou
// `SQLITE_ENABLE_BATCH_ATOMIC_WRITE`, nenhum dos dois ligado no Debian 13: fora do porte (no C,
// `sqliteInt.h` a define como macro que devolve `SQLITE_OK`).

/// O handle recebido está aberto num arquivo de journal. Verdadeiro se este "journal" está hoje
/// na memória do heap, falso caso contrário (`sqlite3JournalIsInMemory`).
pub fn journal_is_in_memory(p: &Sqlite3File) -> i32 {
    match p.p_methods.as_ref() {
        Some(m) => m.is_mem_journal() as i32,
        None => 0,
    }
}

/// Bytes necessários para um `JournalFile` que usa o VFS `p_vfs` para criar os arquivos em disco
/// (`sqlite3JournalSize`). O `sizeof(MemJournal)` do C em 64 bits é fixo em 80.
pub fn journal_size(p_vfs: &dyn Vfs) -> i32 {
    max(p_vfs.sz_os_file(), MEM_JOURNAL_STRUCT_SIZE)
}

