//! Sandbox compartilhada entre threads, com três estratégias de trava.
//!
//! - [`LockedSandbox`]: um `RwLock` na raiz. Escrita trava tudo; leitura é paralela.
//! - [`RcuSandbox`]: `arc-swap` com RCU. Leitura sem trava nenhuma; escrita clona a raiz (O(1)),
//!   aplica a operação (cópia de caminho, porque a versão antiga continua publicada) e publica
//!   com compare-and-swap, refazendo se outra thread publicou antes.
//! - [`ShardedSandbox`]: a tabela de inodes dividida em [`SHARDS`] fatias por `ino % SHARDS`, cada
//!   uma com seu `RwLock`. A resolução de caminho lê cada diretório com trava de leitura momentânea
//!   (como o ref-walk do Linux); a mutação trava em ordem crescente só as fatias dos inodes que
//!   toca e revalida ao executar. Snapshot trava todas as fatias e clona cada raiz: O(SHARDS).
//!
//! Só as operações usadas pelo teste de vazão e pela verificação estão expostas aqui; a semântica é
//! a mesma do [`crate::vfs::Vfs`] porque as operações são as mesmas funções de [`crate::fs`].

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, RwLockWriteGuard};

use arc_swap::ArcSwap;

use crate::errno::Errno;
use crate::fs::{
    Data, Fail, Fs, Ino, Inode, Last, ROOT_INO, Stat, Table, Txn, components, create_at, mkdir_at, read_ino,
    resolve, resolve_parent, stat_ino, unlink_at, write_ino,
};
use crate::maps::{Flavor, IntMap, NameMap};

pub trait SharedSandbox<F: Flavor>: Send + Sync + Sized {
    const LABEL: &'static str;
    /// Imagem de onde a sandbox nasce (e o tipo do snapshot dela).
    type Image: Send + Sync;

    fn image_from_fs(fs: &Fs<F>) -> Self::Image;
    /// Imagem como um `Fs` (pra verificação; pode custar O(n)).
    fn image_to_fs(image: &Self::Image) -> Fs<F>;
    fn from_image(image: &Self::Image) -> Self;
    fn snapshot(&self) -> Self::Image;
    /// Estado inteiro como um `Fs` (pra verificação; pode custar O(n)).
    fn export(&self) -> Fs<F> {
        Self::image_to_fs(&self.snapshot())
    }

    fn create(&self, path: &[u8]) -> Result<Ino, Errno>;
    fn mkdir(&self, path: &[u8]) -> Result<Ino, Errno>;
    fn write(&self, path: &[u8], off: u64, data: &[u8]) -> Result<usize, Errno>;
    fn read(&self, path: &[u8], off: u64, len: usize) -> Result<Vec<u8>, Errno>;
    fn stat(&self, path: &[u8]) -> Result<Stat, Errno>;
    fn unlink(&self, path: &[u8]) -> Result<(), Errno>;
    /// Quantas vezes uma operação teve de ser refeita (CAS perdido ou trava faltando).
    fn retries(&self) -> u64;
}

fn never_open(_: Ino) -> bool {
    false
}

// ---------------------------------------------------------------------------------------------

pub struct LockedSandbox<F: Flavor> {
    fs: RwLock<Fs<F>>,
}

impl<F: Flavor> LockedSandbox<F> {
    fn mutate<R>(&self, op: impl FnOnce(&mut Fs<F>) -> Result<R, Fail>) -> Result<R, Errno> {
        let mut guard = self.fs.write().expect("trava envenenada");
        op(&mut guard).map_err(Fail::errno)
    }

    fn inspect<R>(&self, op: impl FnOnce(&Fs<F>) -> Result<R, Fail>) -> Result<R, Errno> {
        let guard = self.fs.read().expect("trava envenenada");
        op(&guard).map_err(Fail::errno)
    }
}

impl<F: Flavor> SharedSandbox<F> for LockedSandbox<F> {
    const LABEL: &'static str = "RwLock na raiz";
    type Image = Fs<F>;

    fn image_from_fs(fs: &Fs<F>) -> Fs<F> {
        fs.clone()
    }
    fn image_to_fs(image: &Fs<F>) -> Fs<F> {
        image.clone()
    }
    fn from_image(image: &Fs<F>) -> Self {
        LockedSandbox { fs: RwLock::new(image.clone()) }
    }
    fn snapshot(&self) -> Fs<F> {
        self.fs.read().expect("trava envenenada").clone()
    }
    fn create(&self, path: &[u8]) -> Result<Ino, Errno> {
        self.mutate(|fs| {
            let (dir, last) = resolve_parent(fs, path)?;
            create_at(fs, dir, last, 0o644, false)
        })
    }
    fn mkdir(&self, path: &[u8]) -> Result<Ino, Errno> {
        self.mutate(|fs| {
            let (dir, last) = resolve_parent(fs, path)?;
            mkdir_at(fs, dir, last, 0o755)
        })
    }
    fn write(&self, path: &[u8], off: u64, data: &[u8]) -> Result<usize, Errno> {
        self.mutate(|fs| {
            let ino = resolve(fs, path)?;
            write_ino(fs, ino, off, data)
        })
    }
    fn read(&self, path: &[u8], off: u64, len: usize) -> Result<Vec<u8>, Errno> {
        self.inspect(|fs| read_ino(fs, resolve(fs, path)?, off, len))
    }
    fn stat(&self, path: &[u8]) -> Result<Stat, Errno> {
        self.inspect(|fs| stat_ino(fs, resolve(fs, path)?))
    }
    fn unlink(&self, path: &[u8]) -> Result<(), Errno> {
        self.mutate(|fs| {
            let (dir, last) = resolve_parent(fs, path)?;
            unlink_at(fs, dir, last, &never_open)
        })
    }
    fn retries(&self) -> u64 {
        0
    }
}

// ---------------------------------------------------------------------------------------------

pub struct RcuSandbox<F: Flavor> {
    cur: ArcSwap<Fs<F>>,
    retries: AtomicU64,
}

impl<F: Flavor> RcuSandbox<F> {
    fn mutate<R>(&self, mut op: impl FnMut(&mut Fs<F>) -> Result<R, Fail>) -> Result<R, Errno> {
        loop {
            let cur = self.cur.load_full();
            let mut next = (*cur).clone();
            let r = op(&mut next).map_err(Fail::errno)?;
            let prev = self.cur.compare_and_swap(&cur, Arc::new(next));
            if Arc::ptr_eq(&prev, &cur) {
                return Ok(r);
            }
            self.retries.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn inspect<R>(&self, op: impl FnOnce(&Fs<F>) -> Result<R, Fail>) -> Result<R, Errno> {
        let guard = self.cur.load();
        op(&guard).map_err(Fail::errno)
    }
}

impl<F: Flavor> SharedSandbox<F> for RcuSandbox<F> {
    const LABEL: &'static str = "arc-swap (RCU)";
    type Image = Fs<F>;

    fn image_from_fs(fs: &Fs<F>) -> Fs<F> {
        fs.clone()
    }
    fn image_to_fs(image: &Fs<F>) -> Fs<F> {
        image.clone()
    }
    fn from_image(image: &Fs<F>) -> Self {
        RcuSandbox { cur: ArcSwap::from_pointee(image.clone()), retries: AtomicU64::new(0) }
    }
    fn snapshot(&self) -> Fs<F> {
        (**self.cur.load()).clone()
    }
    fn create(&self, path: &[u8]) -> Result<Ino, Errno> {
        self.mutate(|fs| {
            let (dir, last) = resolve_parent(fs, path)?;
            create_at(fs, dir, last, 0o644, false)
        })
    }
    fn mkdir(&self, path: &[u8]) -> Result<Ino, Errno> {
        self.mutate(|fs| {
            let (dir, last) = resolve_parent(fs, path)?;
            mkdir_at(fs, dir, last, 0o755)
        })
    }
    fn write(&self, path: &[u8], off: u64, data: &[u8]) -> Result<usize, Errno> {
        self.mutate(|fs| {
            let ino = resolve(fs, path)?;
            write_ino(fs, ino, off, data)
        })
    }
    fn read(&self, path: &[u8], off: u64, len: usize) -> Result<Vec<u8>, Errno> {
        self.inspect(|fs| read_ino(fs, resolve(fs, path)?, off, len))
    }
    fn stat(&self, path: &[u8]) -> Result<Stat, Errno> {
        self.inspect(|fs| stat_ino(fs, resolve(fs, path)?))
    }
    fn unlink(&self, path: &[u8]) -> Result<(), Errno> {
        self.mutate(|fs| {
            let (dir, last) = resolve_parent(fs, path)?;
            unlink_at(fs, dir, last, &never_open)
        })
    }
    fn retries(&self) -> u64 {
        self.retries.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------------------------

pub const SHARDS: usize = 64;

fn shard_of(ino: Ino) -> usize {
    (ino % SHARDS as u64) as usize
}

pub struct ShardedImage<F: Flavor> {
    shards: Vec<Table<F>>,
    next_ino: Ino,
    clock: u64,
}

pub struct ShardedSandbox<F: Flavor> {
    shards: Vec<RwLock<Table<F>>>,
    next_ino: AtomicU64,
    clock: AtomicU64,
    retries: AtomicU64,
}

struct ShardTxn<'a, F: Flavor> {
    owner: &'a ShardedSandbox<F>,
    guards: Vec<(usize, RwLockWriteGuard<'a, Table<F>>)>,
    prealloc: Option<Ino>,
    mutated: bool,
}

impl<F: Flavor> ShardTxn<'_, F> {
    fn slot(&self, ino: Ino) -> Result<usize, Fail> {
        let shard = shard_of(ino);
        self.guards.binary_search_by_key(&shard, |(s, _)| *s).map_err(|_| Fail::NeedLock(ino))
    }
}

impl<F: Flavor> Txn<F> for ShardTxn<'_, F> {
    fn inode(&self, ino: Ino) -> Result<Option<&Arc<Inode<F>>>, Fail> {
        let i = self.slot(ino)?;
        Ok(self.guards[i].1.get(ino))
    }

    fn inode_mut(&mut self, ino: Ino) -> Result<Option<&mut Inode<F>>, Fail> {
        let i = self.slot(ino)?;
        self.mutated = true;
        Ok(self.guards[i].1.get_mut(ino).map(Arc::make_mut))
    }

    fn insert(&mut self, ino: Ino, inode: Inode<F>) -> Result<(), Fail> {
        let i = self.slot(ino)?;
        self.mutated = true;
        self.guards[i].1.insert(ino, Arc::new(inode));
        Ok(())
    }

    fn remove(&mut self, ino: Ino) -> Result<(), Fail> {
        let i = self.slot(ino)?;
        self.mutated = true;
        self.guards[i].1.remove(ino);
        Ok(())
    }

    fn alloc_ino(&mut self) -> Ino {
        self.prealloc.take().expect("operação que cria inode sem ino pré-alocado")
    }

    fn now(&mut self) -> u64 {
        self.owner.clock.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn set_orphan(&mut self, ino: Ino, _orphan: bool) {
        unreachable!("a sandbox fatiada não tem fds; ino {ino} não pode virar órfão")
    }
}

impl<F: Flavor> ShardedSandbox<F> {
    fn read_inode(&self, ino: Ino) -> Option<Arc<Inode<F>>> {
        self.shards[shard_of(ino)].read().expect("trava envenenada").get(ino).cloned()
    }

    fn lookup(&self, dir: Ino, name: &[u8]) -> Result<Ino, Errno> {
        let inode = self.read_inode(dir).ok_or(Errno::ENOENT)?;
        match &inode.data {
            Data::Dir { entries, parent } => match name {
                b"." => Ok(dir),
                b".." => Ok(*parent),
                _ => entries.get(name).ok_or(Errno::ENOENT),
            },
            Data::File(_) => Err(Errno::ENOTDIR),
        }
    }

    fn walk(&self, comps: &[&[u8]]) -> Result<Ino, Errno> {
        let mut cur = ROOT_INO;
        for c in comps {
            cur = self.lookup(cur, c)?;
        }
        Ok(cur)
    }

    fn resolve(&self, path: &[u8]) -> Result<Ino, Errno> {
        let comps = components(path)?;
        self.walk(&comps)
    }

    fn resolve_parent<'p>(&self, path: &'p [u8]) -> Result<(Ino, Last<'p>), Errno> {
        let comps = components(path)?;
        let Some((last, init)) = comps.split_last() else {
            return Ok((ROOT_INO, Last::Root));
        };
        let dir = self.walk(init)?;
        self.read_inode(dir).ok_or(Errno::ENOENT)?.entries()?;
        Ok((
            dir,
            match *last {
                b"." => Last::Dot,
                b".." => Last::DotDot,
                n => Last::Name(n),
            },
        ))
    }

    /// Roda `op` com as fatias de `hint` travadas; se ela pedir outra fatia, trava mais e refaz.
    fn run<R>(
        &self,
        hint: &[Ino],
        prealloc: Option<Ino>,
        mut op: impl FnMut(&mut ShardTxn<'_, F>) -> Result<R, Fail>,
    ) -> Result<R, Errno> {
        let mut want: Vec<usize> = hint.iter().map(|&i| shard_of(i)).collect();
        want.sort_unstable();
        want.dedup();
        loop {
            let guards = want.iter().map(|&s| (s, self.shards[s].write().expect("trava envenenada"))).collect();
            let mut tx = ShardTxn { owner: self, guards, prealloc, mutated: false };
            match op(&mut tx) {
                Ok(r) => return Ok(r),
                Err(Fail::Errno(e)) => return Err(e),
                Err(Fail::NeedLock(ino)) => {
                    assert!(!tx.mutated, "operação pediu trava depois de mutar");
                    drop(tx);
                    let s = shard_of(ino);
                    if let Err(pos) = want.binary_search(&s) {
                        want.insert(pos, s);
                    }
                    self.retries.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}

impl<F: Flavor> SharedSandbox<F> for ShardedSandbox<F> {
    const LABEL: &'static str = "64 fatias com RwLock";
    type Image = ShardedImage<F>;

    fn image_from_fs(fs: &Fs<F>) -> ShardedImage<F> {
        let mut shards: Vec<Table<F>> = (0..SHARDS).map(|_| Table::<F>::default()).collect();
        fs.table().for_each(&mut |ino, inode| shards[shard_of(ino)].insert(ino, Arc::clone(inode)));
        ShardedImage { shards, next_ino: fs.next_ino(), clock: fs.clock() }
    }

    fn from_image(image: &ShardedImage<F>) -> Self {
        ShardedSandbox {
            shards: image.shards.iter().map(|t| RwLock::new(t.clone())).collect(),
            next_ino: AtomicU64::new(image.next_ino),
            clock: AtomicU64::new(image.clock),
            retries: AtomicU64::new(0),
        }
    }

    fn snapshot(&self) -> ShardedImage<F> {
        let guards: Vec<_> = self.shards.iter().map(|s| s.read().expect("trava envenenada")).collect();
        ShardedImage {
            shards: guards.iter().map(|g| (**g).clone()).collect(),
            next_ino: self.next_ino.load(Ordering::SeqCst),
            clock: self.clock.load(Ordering::SeqCst),
        }
    }

    fn image_to_fs(image: &ShardedImage<F>) -> Fs<F> {
        let mut table = Table::<F>::default();
        for shard in &image.shards {
            shard.for_each(&mut |ino, inode| table.insert(ino, Arc::clone(inode)));
        }
        Fs::from_parts(table, image.next_ino, image.clock)
    }

    fn create(&self, path: &[u8]) -> Result<Ino, Errno> {
        let (dir, last) = self.resolve_parent(path)?;
        let ino = self.next_ino.fetch_add(1, Ordering::Relaxed);
        self.run(&[dir, ino], Some(ino), |t| create_at(t, dir, last, 0o644, false))
    }

    fn mkdir(&self, path: &[u8]) -> Result<Ino, Errno> {
        let (dir, last) = self.resolve_parent(path)?;
        let ino = self.next_ino.fetch_add(1, Ordering::Relaxed);
        self.run(&[dir, ino], Some(ino), |t| mkdir_at(t, dir, last, 0o755))
    }

    fn write(&self, path: &[u8], off: u64, data: &[u8]) -> Result<usize, Errno> {
        let ino = self.resolve(path)?;
        self.run(&[ino], None, |t| write_ino(t, ino, off, data))
    }

    fn read(&self, path: &[u8], off: u64, len: usize) -> Result<Vec<u8>, Errno> {
        let ino = self.resolve(path)?;
        let inode = self.read_inode(ino).ok_or(Errno::ENOENT)?;
        let view = Single(ino, inode);
        read_ino(&view, ino, off, len).map_err(Fail::errno)
    }

    fn stat(&self, path: &[u8]) -> Result<Stat, Errno> {
        let ino = self.resolve(path)?;
        let inode = self.read_inode(ino).ok_or(Errno::ENOENT)?;
        let view = Single(ino, inode);
        stat_ino(&view, ino).map_err(Fail::errno)
    }

    fn unlink(&self, path: &[u8]) -> Result<(), Errno> {
        let (dir, last) = self.resolve_parent(path)?;
        let mut hint = vec![dir];
        if let Last::Name(name) = last
            && let Ok(child) = self.lookup(dir, name)
        {
            hint.push(child);
        }
        self.run(&hint, None, |t| unlink_at(t, dir, last, &never_open))
    }

    fn retries(&self) -> u64 {
        self.retries.load(Ordering::Relaxed)
    }
}

/// Visão de leitura de um único inode já clonado (leitura sem trava na sandbox fatiada).
struct Single<F: Flavor>(Ino, Arc<Inode<F>>);

impl<F: Flavor> Txn<F> for Single<F> {
    fn inode(&self, ino: Ino) -> Result<Option<&Arc<Inode<F>>>, Fail> {
        Ok((ino == self.0).then_some(&self.1))
    }
    fn inode_mut(&mut self, _: Ino) -> Result<Option<&mut Inode<F>>, Fail> {
        unreachable!("visão de leitura")
    }
    fn insert(&mut self, _: Ino, _: Inode<F>) -> Result<(), Fail> {
        unreachable!("visão de leitura")
    }
    fn remove(&mut self, _: Ino) -> Result<(), Fail> {
        unreachable!("visão de leitura")
    }
    fn alloc_ino(&mut self) -> Ino {
        unreachable!("visão de leitura")
    }
    fn now(&mut self) -> u64 {
        unreachable!("visão de leitura")
    }
    fn set_orphan(&mut self, _: Ino, _: bool) {
        unreachable!("visão de leitura")
    }
}
