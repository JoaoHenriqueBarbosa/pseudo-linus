//! VFS do pseudo-linus.
//!
//! O que mora aqui é o que no Linux fica em `fs/`: a resolução de caminho (`namei.c`), as regras de
//! permissão (`generic_permission` com o bypass do root via CAP_DAC_OVERRIDE e CAP_DAC_READ_SEARCH),
//! a semântica das syscalls de arquivo com a mesma ordem de errnos do kernel (`open.c`, `namei.c`,
//! `stat.c`, `utimes.c`, `attr.c`), as montagens e os sistemas de arquivos:
//!
//! - [`tmpfs`]: o tmpfs persistente medido no E03 (`imbl::OrdMap` pra inodes e diretórios,
//!   `imbl::Vector` de blocos de 4 KiB pro conteúdo), com snapshot e restore O(1), órfãos abertos e
//!   readdir do mais novo pro mais antigo, como o tmpfs do Linux 6.12;
//! - [`procfs`]: o `/proc`, alimentado pelo kernel através de [`procfs::ProcProvider`].
//!
//! O kernel (crate `kernel`) é quem tem processos, fds, pipes e dispositivos: ele chama as operações
//! de [`Namespace`] passando um [`Caller`] (credenciais, raiz, cwd, umask, relógio, pid) e recebe um
//! [`Opened`] pra montar a descrição de arquivo aberto.

pub mod fs;
pub mod mount;
pub mod namei;
pub mod ops;
pub mod perm;
pub mod procfs;
pub mod tmpfs;
pub mod types;

pub use fs::{FileHandle, FileSystem, MagicObject, NewNode, NodeKind, SetAttr, WritePos};
pub use mount::{Loc, Mount, MountFlags, Namespace, PinnedLoc};
pub use ops::{ExecFile, Opened, Start};
pub use types::*;
