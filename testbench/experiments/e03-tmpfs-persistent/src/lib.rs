//! E03: modelo mínimo, mas real, do tmpfs persistente do design v2.
//!
//! O estado de uma sandbox é um [`fs::Fs`]: tabela de inodes `ino -> Arc<Inode>` num mapa persistente,
//! diretórios como mapas persistentes `nome -> ino`, conteúdo de arquivo em blocos de 4 KiB com cópia
//! na escrita. Snapshot é `Fs::clone()`. Tudo é genérico sobre um [`maps::Flavor`], que escolhe a
//! estrutura da tabela, a dos diretórios e a do conteúdo; cada candidato do experimento é um flavor.
//!
//! - [`errno`]: erros com os números do Linux.
//! - [`radix`]: trie de raiz 64 persistente feita à mão (só `Arc` e `Arc::make_mut`).
//! - [`maps`]: traits de mapa (tabela e diretório) e as implementações sobre imbl, im, rpds,
//!   immutable-chunkmap e as estruturas à mão.
//! - [`content`]: conteúdo de arquivo (blocos COW em várias estruturas, ou `Arc<Vec<u8>>`).
//! - [`fs`]: inodes, operações (genéricas sobre [`fs::Txn`]) e o estado persistente.
//! - [`vfs`]: sandbox de um dono só, com caminhos, fds, snapshot e restore.
//! - [`concurrent`]: sandbox compartilhada entre threads, com três estratégias de trava.
//! - [`flavors`]: os candidatos.
//! - [`check`]: modelo de referência, alvo real (tmpfs do host) e comparação diferencial.
//! - [`workload`]: gerador determinístico de imagens e de alterações.
//! - [`bench`]: utilitários de medição.
//! - [`measure`]: cenários de tempo (binário principal).
//! - [`memory`]: cenários de memória (binário `e03-mem`, com allocator contador).

pub mod bench;
pub mod check;
pub mod concurrent;
pub mod content;
pub mod errno;
pub mod flavors;
pub mod fs;
pub mod maps;
pub mod measure;
pub mod memory;
pub mod radix;
pub mod vfs;
pub mod workload;

pub use errno::Errno;
pub use fs::{Fs, Ino, Kind, Stat};
pub use maps::Flavor;
pub use vfs::Vfs;
