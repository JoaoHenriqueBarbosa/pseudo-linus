//! Sandbox de um dono só: caminhos, fds, snapshot e restore sobre um [`Fs`].
//!
//! O fd guarda o ino, não um `Arc<Inode>`. Com estrutura persistente o `Arc<Inode>` é um valor
//! congelado: um fd que o segurasse não veria o que outro fd ou o próprio caminho escreveu depois.
//! O "arquivo desvinculado continua vivo enquanto aberto" fica com a tabela: o inode com
//! `nlink == 0` e fd aberto continua nela marcado como órfão, e sai no último `close`.
//!
//! Restore com fds abertos: o fd continua apontando pro ino; se o ino existe no estado restaurado,
//! o fd passa a ver a versão restaurada do arquivo, e se não existe, o fd dá `ESTALE`. Inos nunca
//! são reaproveitados dentro da vida de uma sandbox (o restore não volta o contador), senão um fd
//! antigo passaria a apontar pra outro arquivo.

use std::collections::HashMap;

use crate::errno::Errno;
use crate::fs::{
    self, Data, Fail, Fs, Ino, Kind, Name, ROOT_INO, Stat, Txn, create_at, link_at, mkdir_at, read_ino, readdir_ino,
    rename_at, resolve, resolve_parent, rmdir_at, stat_ino, truncate_ino, unlink_at, write_ino,
};
use crate::maps::{Flavor, IntMap, NameMap};

pub type Fd = usize;

pub struct Vfs<F: Flavor> {
    fs: Fs<F>,
    fds: Vec<Option<Ino>>,
    open: HashMap<Ino, u32>,
}

impl<F: Flavor> Default for Vfs<F> {
    fn default() -> Self {
        Vfs::new()
    }
}

fn errno<T>(r: Result<T, Fail>) -> Result<T, Errno> {
    r.map_err(Fail::errno)
}

impl<F: Flavor> Vfs<F> {
    pub fn new() -> Self {
        Vfs { fs: Fs::new(), fds: Vec::new(), open: HashMap::new() }
    }

    /// Sandbox nova a partir de uma imagem: O(1), compartilha a imagem inteira.
    pub fn from_image(image: &Fs<F>) -> Self {
        Vfs { fs: image.clone(), fds: Vec::new(), open: HashMap::new() }
    }

    pub fn fs(&self) -> &Fs<F> {
        &self.fs
    }

    pub fn into_fs(self) -> Fs<F> {
        self.fs
    }

    pub fn snapshot(&self) -> Fs<F> {
        self.fs.clone()
    }

    pub fn restore(&mut self, snap: &Fs<F>) {
        let next = self.fs.next_ino().max(snap.next_ino());
        let clock = self.fs.clock().max(snap.clock());
        self.fs = snap.clone();
        self.fs.bump_counters(next, clock);
        let stale: Vec<Ino> = self.fs.orphans().iter().copied().filter(|i| !self.open.contains_key(i)).collect();
        for ino in stale {
            self.fs.drop_orphan(ino);
        }
    }

    pub fn mkdir(&mut self, path: &[u8], mode: u32) -> Result<Ino, Errno> {
        let (dir, last) = errno(resolve_parent(&self.fs, path))?;
        errno(mkdir_at(&mut self.fs, dir, last, mode))
    }

    /// `open(O_CREAT | O_WRONLY)` seguido de `close`.
    pub fn create(&mut self, path: &[u8], mode: u32) -> Result<Ino, Errno> {
        let (dir, last) = errno(resolve_parent(&self.fs, path))?;
        errno(create_at(&mut self.fs, dir, last, mode, false))
    }

    /// `open(O_WRONLY)` + `pwrite` + `close`.
    pub fn write(&mut self, path: &[u8], off: u64, data: &[u8]) -> Result<usize, Errno> {
        let ino = errno(resolve(&self.fs, path))?;
        errno(write_ino(&mut self.fs, ino, off, data))
    }

    /// `open(O_WRONLY)` + `ftruncate` + `close`.
    pub fn truncate(&mut self, path: &[u8], len: u64) -> Result<(), Errno> {
        let ino = errno(resolve(&self.fs, path))?;
        errno(truncate_ino(&mut self.fs, ino, len))
    }

    pub fn read(&self, path: &[u8], off: u64, len: usize) -> Result<Vec<u8>, Errno> {
        let ino = errno(resolve(&self.fs, path))?;
        errno(read_ino(&self.fs, ino, off, len))
    }

    pub fn unlink(&mut self, path: &[u8]) -> Result<(), Errno> {
        let (dir, last) = errno(resolve_parent(&self.fs, path))?;
        let Vfs { fs, open, .. } = self;
        errno(unlink_at(fs, dir, last, &|ino| open.contains_key(&ino)))
    }

    pub fn rmdir(&mut self, path: &[u8]) -> Result<(), Errno> {
        let (dir, last) = errno(resolve_parent(&self.fs, path))?;
        errno(rmdir_at(&mut self.fs, dir, last))
    }

    pub fn rename(&mut self, from: &[u8], to: &[u8]) -> Result<(), Errno> {
        let (sdir, slast) = errno(resolve_parent(&self.fs, from))?;
        let (ddir, dlast) = errno(resolve_parent(&self.fs, to))?;
        let Vfs { fs, open, .. } = self;
        errno(rename_at(fs, sdir, slast, ddir, dlast, &|ino| open.contains_key(&ino)))
    }

    pub fn link(&mut self, target: &[u8], new: &[u8]) -> Result<(), Errno> {
        let ino = errno(resolve(&self.fs, target))?;
        let (dir, last) = errno(resolve_parent(&self.fs, new))?;
        errno(link_at(&mut self.fs, ino, dir, last))
    }

    pub fn stat(&self, path: &[u8]) -> Result<Stat, Errno> {
        let ino = errno(resolve(&self.fs, path))?;
        errno(stat_ino(&self.fs, ino))
    }

    pub fn readdir(&self, path: &[u8]) -> Result<Vec<(Name, Ino)>, Errno> {
        let ino = errno(resolve(&self.fs, path))?;
        errno(readdir_ino(&self.fs, ino))
    }

    /// `open(O_RDWR)`, com `O_CREAT` opcional. Diretório dá `EISDIR`.
    pub fn open(&mut self, path: &[u8], create: bool) -> Result<Fd, Errno> {
        let ino = if create {
            self.create(path, 0o644)?
        } else {
            let ino = errno(resolve(&self.fs, path))?;
            if errno(stat_ino(&self.fs, ino))?.kind == Kind::Dir {
                return Err(Errno::EISDIR);
            }
            ino
        };
        *self.open.entry(ino).or_insert(0) += 1;
        let fd = match self.fds.iter().position(Option::is_none) {
            Some(i) => {
                self.fds[i] = Some(ino);
                i
            }
            None => {
                self.fds.push(Some(ino));
                self.fds.len() - 1
            }
        };
        Ok(fd)
    }

    fn fd_ino(&self, fd: Fd) -> Result<Ino, Errno> {
        let ino = self.fds.get(fd).copied().flatten().ok_or(Errno::EBADF)?;
        if !self.fs.contains(ino) {
            return Err(Errno::ESTALE);
        }
        Ok(ino)
    }

    pub fn close(&mut self, fd: Fd) -> Result<(), Errno> {
        let ino = self.fds.get_mut(fd).and_then(Option::take).ok_or(Errno::EBADF)?;
        let count = self.open.get_mut(&ino).expect("contagem de abertura");
        *count -= 1;
        if *count == 0 {
            self.open.remove(&ino);
            self.fs.drop_orphan(ino);
        }
        Ok(())
    }

    pub fn pwrite(&mut self, fd: Fd, off: u64, data: &[u8]) -> Result<usize, Errno> {
        let ino = self.fd_ino(fd)?;
        errno(write_ino(&mut self.fs, ino, off, data))
    }

    pub fn pread(&self, fd: Fd, off: u64, len: usize) -> Result<Vec<u8>, Errno> {
        let ino = self.fd_ino(fd)?;
        errno(read_ino(&self.fs, ino, off, len))
    }

    pub fn fstat(&self, fd: Fd) -> Result<Stat, Errno> {
        let ino = self.fd_ino(fd)?;
        errno(stat_ino(&self.fs, ino))
    }

    pub fn ftruncate(&mut self, fd: Fd, len: u64) -> Result<(), Errno> {
        let ino = self.fd_ino(fd)?;
        errno(truncate_ino(&mut self.fs, ino, len))
    }

    pub fn open_count(&self) -> usize {
        self.open.len()
    }

    /// Confere os invariantes da árvore inteira. Devolve a lista de violações (vazia se está tudo
    /// certo): todo nome aponta pra inode existente, `nlink` de arquivo = número de nomes, `nlink`
    /// de diretório = 2 + subdiretórios, `..` certo, nenhum inode inalcançável além dos órfãos
    /// abertos, e o conjunto de órfãos é exatamente o dos arquivos com `nlink == 0`.
    pub fn fsck(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut names: HashMap<Ino, u32> = HashMap::new();
        let mut stack = vec![ROOT_INO];
        let mut seen_dirs = 0usize;
        while let Some(dir) = stack.pop() {
            seen_dirs += 1;
            let Ok(inode) = fs::get(&self.fs, dir) else {
                problems.push(format!("diretório {dir} referenciado mas ausente"));
                continue;
            };
            let Data::Dir { entries, .. } = &inode.data else {
                problems.push(format!("ino {dir} na pilha de diretórios não é diretório"));
                continue;
            };
            let mut subdirs = 0;
            let mut children = Vec::new();
            entries.for_each(&mut |name, ino| children.push((name.clone(), ino)));
            for (name, ino) in children {
                match self.fs.inode(ino).ok().flatten() {
                    None => problems.push(format!("{}/{} aponta pro ino ausente {ino}", dir, String::from_utf8_lossy(&name))),
                    Some(child) => match &child.data {
                        Data::Dir { parent, .. } => {
                            subdirs += 1;
                            if *parent != dir {
                                problems.push(format!("diretório {ino}: '..' = {parent}, esperado {dir}"));
                            }
                            *names.entry(ino).or_insert(0) += 1;
                            stack.push(ino);
                        }
                        Data::File(_) => *names.entry(ino).or_insert(0) += 1,
                    },
                }
            }
            if inode.nlink != 2 + subdirs {
                problems.push(format!("diretório {dir}: nlink {} != 2 + {subdirs}", inode.nlink));
            }
        }
        let mut total = 0usize;
        let mut orphans_found = std::collections::BTreeSet::new();
        self.fs.table().for_each(&mut |ino, inode| {
            total += 1;
            if inode.kind() == Kind::File {
                let n = names.get(&ino).copied().unwrap_or(0);
                if n != inode.nlink {
                    problems.push(format!("arquivo {ino}: nlink {} != {n} nomes", inode.nlink));
                }
                if n == 0 {
                    orphans_found.insert(ino);
                    if !self.open.contains_key(&ino) {
                        problems.push(format!("arquivo {ino} sem nome e sem fd continua na tabela"));
                    }
                }
            } else if ino != ROOT_INO && !names.contains_key(&ino) {
                problems.push(format!("diretório {ino} inalcançável"));
            }
        });
        if &orphans_found != self.fs.orphans() {
            problems.push(format!("órfãos {:?} != marcados {:?}", orphans_found, self.fs.orphans()));
        }
        let reachable_files = names.len() - (seen_dirs - 1);
        if total != seen_dirs + reachable_files + orphans_found.len() {
            problems.push(format!(
                "tabela com {total} inodes, alcançáveis {seen_dirs} diretórios + {reachable_files} arquivos + {} órfãos",
                orphans_found.len()
            ));
        }
        problems
    }
}
