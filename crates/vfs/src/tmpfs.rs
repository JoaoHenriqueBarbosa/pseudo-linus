//! tmpfs persistente, como medido no E03.
//!
//! - Tabela de inodes: `imbl::OrdMap<Ino, Arc<Inode>>`. Diretório: `OrdMap<nome, entrada>` mais um índice
//!   por ordem de criação (`OrdMap<seq, nome>`), porque o readdir do tmpfs do Linux 6.12 sai do mais novo
//!   pro mais antigo (a lista `d_children` recebe cada dentry novo na cabeça). Rename põe a entrada como
//!   a mais nova do diretório de destino, como o `d_move`.
//! - Conteúdo: `imbl::Vector` de blocos de 4 KiB (`Option<Arc<Vec<u8>>>`; `None` é buraco). Um bloco pode
//!   ser mais curto que 4 KiB: o que falta até o fim do arquivo lê como zero. Mudar 1 byte depois de um
//!   snapshot copia um bloco e o caminho na árvore, nunca o arquivo.
//! - Snapshot e restore trocam a raiz do estado: O(1), e o restore custa o que mudou. O contador de inodes
//!   não volta no restore (senão um fd aberto antes passaria a ver outro arquivo com o mesmo número).
//! - O fd guarda o número do inode. Um inode sem nome e ainda em uso (fd aberto, cwd) fica na tabela como
//!   órfão e sai no último uso.
//! - Tamanho de diretório: `40 + 20 * entradas` (o `BOGO_DIRENT_SIZE` do shmem); `nlink` de diretório =
//!   `2 + subdiretórios`; `st_blocks` conta as páginas alocadas (8 setores de 512 bytes por página).
//! - Cota (`size=` e `nr_inodes=` do tmpfs): ENOSPC quando acaba.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use imbl::{OrdMap, Vector};
use parking_lot::RwLock;

use crate::fs::{FileHandle, FileSystem, NewNode, NodeKind, SetAttr, WritePos};
use crate::types::*;

/// `TMPFS_MAGIC` do `statfs`.
pub const TMPFS_MAGIC: u64 = 0x0102_1994;
/// `BOGO_DIRENT_SIZE` do shmem.
const BOGO_DIRENT_SIZE: u64 = 20;
const BLOCK: usize = PAGE_SIZE as usize;
/// Inode da raiz.
pub const ROOT_INO: Ino = 1;
/// Os seqs de entrada de diretório ficam abaixo disto (sobra espaço no cookie do readdir).
const SEQ_SPACE: u64 = 1 << 62;

type Name = Arc<[u8]>;
type Block = Option<Arc<Vec<u8>>>;

#[derive(Clone, Copy, Debug)]
struct Slot {
    ino: Ino,
    seq: u64,
    kind: FileType,
}

#[derive(Clone, Debug, Default)]
struct DirData {
    entries: OrdMap<Name, Slot>,
    order: OrdMap<u64, Name>,
    next_seq: u64,
}

#[derive(Clone, Debug, Default)]
struct FileData {
    blocks: Vector<Block>,
    size: u64,
    /// Blocos alocados (os `Some`).
    alloc: u64,
}

#[derive(Clone, Debug)]
enum Data {
    File(FileData),
    Dir(DirData),
    Symlink(Name),
    Special,
}

#[derive(Clone, Debug)]
struct Inode {
    mode: Mode,
    uid: Uid,
    gid: Gid,
    nlink: u64,
    rdev: u64,
    atime: TimeSpec,
    mtime: TimeSpec,
    ctime: TimeSpec,
    btime: TimeSpec,
    /// Diretório pai e nome com que o inode foi visto por último (exatos pra diretórios).
    parent: Ino,
    name: Name,
    data: Data,
}

impl Inode {
    fn kind(&self) -> FileType {
        FileType::from_mode(self.mode)
    }
}

/// O estado persistente (o que um snapshot guarda).
#[derive(Clone, Debug, Default)]
struct State {
    inodes: OrdMap<Ino, Arc<Inode>>,
    /// Inodes sem nome que estavam em uso quando perderam o último nome.
    orphans: OrdMap<Ino, ()>,
    used_blocks: u64,
    used_inodes: u64,
}

struct Inner {
    state: State,
    next_ino: Ino,
    /// Usos vivos por inode (fds, cwd). Fica fora do snapshot.
    pins: HashMap<Ino, u32>,
}

/// Limites do tmpfs (`size=`, `nr_inodes=`). O padrão é sem limite.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TmpfsLimits {
    /// Páginas de 4 KiB; `None` = sem limite (o statfs mostra o tamanho padrão).
    pub max_blocks: Option<u64>,
    pub max_inodes: Option<u64>,
}

/// Retrato do estado de um tmpfs.
#[derive(Clone)]
pub struct TmpfsSnapshot {
    state: State,
    next_ino: Ino,
}

impl std::fmt::Debug for TmpfsSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TmpfsSnapshot({} inodes)", self.state.inodes.len())
    }
}

/// Um tmpfs.
pub struct Tmpfs {
    inner: RwLock<Inner>,
    dev: u64,
    limits: TmpfsLimits,
}

impl std::fmt::Debug for Tmpfs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Tmpfs(dev {:#x})", self.dev)
    }
}

fn name(b: &[u8]) -> Name {
    Arc::from(b)
}

fn dir_size(d: &DirData) -> u64 {
    2 * BOGO_DIRENT_SIZE + BOGO_DIRENT_SIZE * d.entries.len() as u64
}

fn blocks_for(size: u64) -> usize {
    size.div_ceil(PAGE_SIZE) as usize
}

impl Tmpfs {
    /// tmpfs vazio com a raiz no modo dado (ex.: 0o755, 0o1777).
    pub fn new(dev: u64, root_mode: Mode, now: TimeSpec, limits: TmpfsLimits) -> Arc<Tmpfs> {
        let root = Inode {
            mode: S_IFDIR | (root_mode & 0o7777),
            uid: 0,
            gid: 0,
            nlink: 2,
            rdev: 0,
            atime: now,
            mtime: now,
            ctime: now,
            btime: now,
            parent: ROOT_INO,
            name: name(b"/"),
            data: Data::Dir(DirData::default()),
        };
        let mut state = State::default();
        state.inodes.insert(ROOT_INO, Arc::new(root));
        state.used_inodes = 1;
        Arc::new(Tmpfs {
            inner: RwLock::new(Inner { state, next_ino: ROOT_INO + 1, pins: HashMap::new() }),
            dev,
            limits,
        })
    }

    /// tmpfs novo a partir de um snapshot (sandbox derivada): O(1).
    pub fn from_snapshot(dev: u64, snap: &TmpfsSnapshot, limits: TmpfsLimits) -> Arc<Tmpfs> {
        let mut state = snap.state.clone();
        // Órfãos do snapshot não têm dono aqui.
        let orphans: Vec<Ino> = state.orphans.keys().copied().collect();
        for ino in orphans {
            Self::drop_inode(&mut state, ino);
        }
        state.orphans = OrdMap::new();
        Arc::new(Tmpfs {
            inner: RwLock::new(Inner { state, next_ino: snap.next_ino, pins: HashMap::new() }),
            dev,
            limits,
        })
    }

    pub fn limits(&self) -> TmpfsLimits {
        self.limits
    }

    /// Retrato do estado (O(1)).
    pub fn snapshot(&self) -> TmpfsSnapshot {
        let g = self.inner.read();
        TmpfsSnapshot { state: g.state.clone(), next_ino: g.next_ino }
    }

    /// Volta ao snapshot. O contador de inodes não volta; órfãos sem uso somem.
    pub fn restore(&self, snap: &TmpfsSnapshot) {
        let old = {
            let mut g = self.inner.write();
            let old = std::mem::replace(&mut g.state, snap.state.clone());
            g.next_ino = g.next_ino.max(snap.next_ino);
            let orphans: Vec<Ino> = g.state.orphans.keys().copied().collect();
            for ino in orphans {
                if !g.pins.contains_key(&ino) {
                    g.state.orphans.remove(&ino);
                    Self::drop_inode(&mut g.state, ino);
                }
            }
            old
        };
        // Solta a versão abandonada fora da trava (pode ser O(o que mudou)).
        drop(old);
    }

    /// Bytes de conteúdo em uso (páginas alocadas) e inodes.
    pub fn usage(&self) -> (u64, u64) {
        let g = self.inner.read();
        (g.state.used_blocks * PAGE_SIZE, g.state.used_inodes)
    }

    fn drop_inode(state: &mut State, ino: Ino) {
        if let Some(inode) = state.inodes.remove(&ino) {
            if let Data::File(f) = &inode.data {
                state.used_blocks = state.used_blocks.saturating_sub(f.alloc);
            }
            state.used_inodes = state.used_inodes.saturating_sub(1);
        }
    }

    fn get(state: &State, ino: Ino) -> SysResult<&Arc<Inode>> {
        state.inodes.get(&ino).ok_or(Errno::ESTALE)
    }

    fn get_mut(state: &mut State, ino: Ino) -> SysResult<&mut Inode> {
        state.inodes.get_mut(&ino).map(Arc::make_mut).ok_or(Errno::ESTALE)
    }

    fn dir(state: &State, ino: Ino) -> SysResult<&DirData> {
        match &Self::get(state, ino)?.data {
            Data::Dir(d) => Ok(d),
            _ => Err(Errno::ENOTDIR),
        }
    }

    fn stat_of(&self, ino: Ino, i: &Inode) -> Stat {
        let (size, blocks) = match &i.data {
            Data::File(f) => (f.size, f.alloc * (PAGE_SIZE / 512)),
            Data::Dir(d) => (dir_size(d), 0),
            Data::Symlink(t) => (t.len() as u64, 0),
            Data::Special => (0, 0),
        };
        Stat {
            dev: self.dev,
            ino,
            mode: i.mode,
            nlink: i.nlink,
            uid: i.uid,
            gid: i.gid,
            rdev: i.rdev,
            size,
            blksize: PAGE_SIZE,
            blocks,
            atime: i.atime,
            mtime: i.mtime,
            ctime: i.ctime,
            btime: Some(i.btime),
        }
    }

    /// Insere `child` em `dir` com um seq novo (a entrada mais nova do diretório).
    fn dir_insert(state: &mut State, dir: Ino, nm: Name, child: Ino, kind: FileType) -> SysResult<()> {
        let d = match &mut Self::get_mut(state, dir)?.data {
            Data::Dir(d) => d,
            _ => return Err(Errno::ENOTDIR),
        };
        let seq = d.next_seq;
        d.next_seq += 1;
        if let Some(old) = d.entries.insert(nm.clone(), Slot { ino: child, seq, kind }) {
            d.order.remove(&old.seq);
        }
        d.order.insert(seq, nm);
        Ok(())
    }

    fn dir_remove(state: &mut State, dir: Ino, nm: &[u8]) -> SysResult<Slot> {
        let d = match &mut Self::get_mut(state, dir)?.data {
            Data::Dir(d) => d,
            _ => return Err(Errno::ENOTDIR),
        };
        let slot = d.entries.remove(nm).ok_or(Errno::ENOENT)?;
        d.order.remove(&slot.seq);
        Ok(slot)
    }

    fn touch_dir(state: &mut State, dir: Ino, now: TimeSpec) -> SysResult<()> {
        let i = Self::get_mut(state, dir)?;
        i.mtime = now;
        i.ctime = now;
        Ok(())
    }

    /// Tira um nome de um inode: `nlink--`; sem nome e sem uso, some; sem nome e em uso, vira órfão.
    fn drop_link(g: &mut Inner, ino: Ino, now: TimeSpec, by: u64) -> SysResult<()> {
        let left = {
            let i = Self::get_mut(&mut g.state, ino)?;
            i.nlink = i.nlink.saturating_sub(by);
            i.ctime = now;
            i.nlink
        };
        if left == 0 {
            if g.pins.contains_key(&ino) {
                g.state.orphans.insert(ino, ());
            } else {
                Self::drop_inode(&mut g.state, ino);
            }
        }
        Ok(())
    }

    fn alloc_ino(g: &mut Inner) -> Ino {
        let i = g.next_ino;
        g.next_ino += 1;
        i
    }

    fn check_inode_quota(&self, g: &Inner) -> SysResult<()> {
        if let Some(max) = self.limits.max_inodes
            && g.state.used_inodes >= max
        {
            return Err(Errno::ENOSPC);
        }
        Ok(())
    }

    // ---- conteúdo ----

    fn read_data(f: &FileData, off: u64, buf: &mut [u8]) -> usize {
        if off >= f.size || buf.is_empty() {
            return 0;
        }
        let n = (buf.len() as u64).min(f.size - off) as usize;
        let mut done = 0;
        while done < n {
            let pos = off + done as u64;
            let bi = (pos / PAGE_SIZE) as usize;
            let in_off = (pos % PAGE_SIZE) as usize;
            let take = (BLOCK - in_off).min(n - done);
            let out = &mut buf[done..done + take];
            match f.blocks.get(bi) {
                Some(Some(b)) => {
                    // Bloco curto: o que passa do fim dele lê como zero.
                    let have = b.len().saturating_sub(in_off).min(take);
                    if have > 0 {
                        out[..have].copy_from_slice(&b[in_off..in_off + have]);
                    }
                    out[have..].fill(0);
                }
                _ => out.fill(0),
            }
            done += take;
        }
        n
    }

    /// Escreve; devolve quantos bytes couberam na cota (pode ser menos que `data.len()`).
    fn write_data(f: &mut FileData, off: u64, data: &[u8], budget: Option<u64>, used: &mut u64) -> usize {
        let end = off + data.len() as u64;
        let need = blocks_for(end);
        while f.blocks.len() < need {
            f.blocks.push_back(None);
        }
        let mut done = 0usize;
        while done < data.len() {
            let pos = off + done as u64;
            let bi = (pos / PAGE_SIZE) as usize;
            let in_off = (pos % PAGE_SIZE) as usize;
            let take = (BLOCK - in_off).min(data.len() - done);
            let slot = f.blocks.get_mut(bi).expect("bloco estendido acima");
            if slot.is_none() {
                if let Some(max) = budget
                    && *used >= max
                {
                    break;
                }
                *slot = Some(Arc::new(Vec::new()));
                f.alloc += 1;
                *used += 1;
            }
            let b = Arc::make_mut(slot.as_mut().expect("alocado acima"));
            if b.len() < in_off + take {
                b.resize(in_off + take, 0);
            }
            b[in_off..in_off + take].copy_from_slice(&data[done..done + take]);
            done += take;
        }
        let written_end = off + done as u64;
        if written_end > f.size {
            f.size = written_end;
        }
        // Blocos de buraco criados além do que foi escrito (cota esgotada) não mudam o tamanho. Páginas
        // alocadas além do fim por `fallocate` com KEEP_SIZE ficam.
        let keep = blocks_for(f.size);
        while f.blocks.len() > keep && matches!(f.blocks.back(), Some(None)) {
            f.blocks.pop_back();
        }
        done
    }

    fn truncate_data(f: &mut FileData, size: u64, used: &mut u64) {
        if size < f.size {
            let keep = blocks_for(size);
            while f.blocks.len() > keep {
                if let Some(Some(_)) = f.blocks.pop_back() {
                    f.alloc -= 1;
                    *used = used.saturating_sub(1);
                }
            }
            let rem = (size % PAGE_SIZE) as usize;
            if rem != 0
                && let Some(slot) = f.blocks.get_mut(keep - 1)
                && let Some(b) = slot
                && b.len() > rem
            {
                Arc::make_mut(b).truncate(rem);
            }
        } else {
            let need = blocks_for(size);
            while f.blocks.len() < need {
                f.blocks.push_back(None);
            }
        }
        f.size = size;
    }

    /// relatime: atualiza o atime se ele não é mais novo que mtime e ctime, ou se tem mais de um dia.
    fn relatime_needed(i: &Inode, now: TimeSpec) -> bool {
        if i.atime == now {
            return false;
        }
        i.mtime >= i.atime || i.ctime >= i.atime || now.sec - i.atime.sec >= 24 * 60 * 60
    }

    fn touch_atime_now(&self, ino: Ino, now: TimeSpec) {
        let needed = {
            let g = self.inner.read();
            match g.state.inodes.get(&ino) {
                Some(i) => Self::relatime_needed(i, now),
                None => false,
            }
        };
        if needed {
            let mut g = self.inner.write();
            if let Ok(i) = Self::get_mut(&mut g.state, ino)
                && Self::relatime_needed(i, now)
            {
                i.atime = now;
            }
        }
    }

    fn file_read(&self, cx: &Caller, ino: Ino, off: u64, buf: &mut [u8]) -> SysResult<usize> {
        let n = {
            let g = self.inner.read();
            match &Self::get(&g.state, ino)?.data {
                Data::File(f) => Self::read_data(f, off, buf),
                Data::Dir(_) => return Err(Errno::EISDIR),
                _ => return Err(Errno::EINVAL),
            }
        };
        self.touch_atime_now(ino, cx.now);
        Ok(n)
    }

    fn file_write(&self, cx: &Caller, ino: Ino, pos: WritePos, buf: &[u8]) -> SysResult<(usize, u64)> {
        let mut g = self.inner.write();
        let max_blocks = self.limits.max_blocks;
        let Inner { state, .. } = &mut *g;
        let mut used = state.used_blocks;
        let i = Self::get_mut(state, ino)?;
        let f = match &mut i.data {
            Data::File(f) => f,
            Data::Dir(_) => return Err(Errno::EISDIR),
            _ => return Err(Errno::EINVAL),
        };
        let off = match pos {
            WritePos::At(o) => o,
            WritePos::Append => f.size,
        };
        if buf.is_empty() {
            return Ok((0, off));
        }
        // generic_write_check_limits: RLIMIT_FSIZE (o kernel manda SIGXFSZ) e MAX_LFS_FILESIZE.
        let mut len = buf.len() as u64;
        if off >= cx.fsize_limit {
            return Err(Errno::EFBIG);
        }
        len = len.min(cx.fsize_limit - off);
        if off >= MAX_FILE_SIZE {
            return Err(Errno::EFBIG);
        }
        len = len.min(MAX_FILE_SIZE - off);
        let data = &buf[..len as usize];
        let n = Self::write_data(f, off, data, max_blocks, &mut used);
        if n == 0 {
            return Err(Errno::ENOSPC);
        }
        i.mtime = cx.now;
        i.ctime = cx.now;
        // file_remove_privs: quem não tem CAP_FSETID perde setuid e setgid ao escrever.
        if let Some(m) = crate::perm::drop_suidgid(&cx.cred, i.mode, i.gid) {
            i.mode = (i.mode & S_IFMT) | m;
        }
        state.used_blocks = used;
        Ok((n, off + n as u64))
    }

    fn seek(&self, ino: Ino, off: u64, hole: bool) -> SysResult<u64> {
        let g = self.inner.read();
        let f = match &Self::get(&g.state, ino)?.data {
            Data::File(f) => f,
            _ => return Err(Errno::EINVAL),
        };
        if off >= f.size {
            return Err(Errno::ENXIO);
        }
        let mut bi = (off / PAGE_SIZE) as usize;
        let nb = f.blocks.len();
        if hole {
            while bi < nb && matches!(f.blocks.get(bi), Some(Some(_))) {
                bi += 1;
            }
            let at = (bi as u64 * PAGE_SIZE).max(off);
            Ok(at.min(f.size))
        } else {
            while bi < nb && matches!(f.blocks.get(bi), Some(None)) {
                bi += 1;
            }
            if bi >= nb {
                return Err(Errno::ENXIO);
            }
            // Páginas reservadas além do fim (KEEP_SIZE) não são dado visível.
            let at = (bi as u64 * PAGE_SIZE).max(off);
            if at >= f.size {
                return Err(Errno::ENXIO);
            }
            Ok(at)
        }
    }

    /// `shmem_fallocate` (Linux 6.12). O VFS já validou o modo (só sobra o que o tmpfs não faz),
    /// `offset`/`len` e `s_maxbytes`.
    fn file_fallocate(&self, cx: &Caller, ino: Ino, mode: FallocFlags, offset: u64, len: u64) -> SysResult<()> {
        if mode.bits() & !(FallocFlags::KEEP_SIZE | FallocFlags::PUNCH_HOLE).bits() != 0 {
            return Err(Errno::EOPNOTSUPP);
        }
        let end = offset.checked_add(len).ok_or(Errno::EFBIG)?;
        let mut g = self.inner.write();
        let max_blocks = self.limits.max_blocks;
        let Inner { state, .. } = &mut *g;
        let mut used = state.used_blocks;
        let i = Self::get_mut(state, ino)?;
        let f = match &mut i.data {
            Data::File(f) => f,
            Data::Dir(_) => return Err(Errno::EISDIR),
            _ => return Err(Errno::ENODEV),
        };
        if mode.contains(FallocFlags::PUNCH_HOLE) {
            // shmem_truncate_range(offset, end - 1): páginas inteiras no intervalo saem (inclusive as
            // reservadas além do fim), as parciais nas pontas ficam alocadas e zeradas.
            Self::punch_data(f, offset, end, &mut used);
        } else {
            // inode_newsize_ok vale mesmo com KEEP_SIZE: RLIMIT_FSIZE (o kernel manda SIGXFSZ) e
            // s_maxbytes.
            if end > cx.fsize_limit || end > MAX_FILE_SIZE {
                return Err(Errno::EFBIG);
            }
            let first = (offset / PAGE_SIZE) as usize;
            let last = blocks_for(end);
            if let Some(max) = max_blocks
                && (last - first) as u64 > max
            {
                // "Evita uma tempestade de swap se len é impossível de satisfazer."
                return Err(Errno::ENOSPC);
            }
            while f.blocks.len() < last {
                f.blocks.push_back(None);
            }
            // Páginas alocadas por esta chamada, pra desfazer se a cota acabar no meio (o
            // `shmem_undo_range` com `fallocend`): as que já existiam ficam.
            let mut fresh: Vec<usize> = Vec::new();
            for bi in first..last {
                let slot = f.blocks.get_mut(bi).expect("bloco estendido acima");
                if slot.is_some() {
                    continue;
                }
                if let Some(max) = max_blocks
                    && used >= max
                {
                    for b in fresh {
                        *f.blocks.get_mut(b).expect("bloco desta chamada") = None;
                        f.alloc -= 1;
                        used -= 1;
                    }
                    let keep = blocks_for(f.size);
                    while f.blocks.len() > keep && matches!(f.blocks.back(), Some(None)) {
                        f.blocks.pop_back();
                    }
                    state.used_blocks = used;
                    return Err(Errno::ENOSPC);
                }
                *slot = Some(Arc::new(Vec::new()));
                f.alloc += 1;
                used += 1;
                fresh.push(bi);
            }
            if !mode.contains(FallocFlags::KEEP_SIZE) && end > f.size {
                f.size = end;
            }
        }
        // file_modified: mtime e ctime, e quem não tem CAP_FSETID perde setuid e setgid.
        i.mtime = cx.now;
        i.ctime = cx.now;
        if let Some(m) = crate::perm::drop_suidgid(&cx.cred, i.mode, i.gid) {
            i.mode = (i.mode & S_IFMT) | m;
        }
        state.used_blocks = used;
        Ok(())
    }

    /// Libera as páginas inteiras de `[start, end)` e zera as partes cobertas das páginas das pontas.
    fn punch_data(f: &mut FileData, start: u64, end: u64, used: &mut u64) {
        let nb = f.blocks.len() as u64;
        let mut pos = start;
        while pos < end {
            let bi = pos / PAGE_SIZE;
            if bi >= nb {
                break;
            }
            let in_off = (pos % PAGE_SIZE) as usize;
            let take = ((PAGE_SIZE - in_off as u64).min(end - pos)) as usize;
            let slot = f.blocks.get_mut(bi as usize).expect("bloco dentro do vetor");
            if in_off == 0 && take == BLOCK {
                if slot.take().is_some() {
                    f.alloc -= 1;
                    *used = used.saturating_sub(1);
                }
            } else if let Some(b) = slot
                && b.len() > in_off
            {
                let stop = (in_off + take).min(b.len());
                Arc::make_mut(b)[in_off..stop].fill(0);
            }
            pos += take as u64;
        }
        let keep = blocks_for(f.size);
        while f.blocks.len() > keep && matches!(f.blocks.back(), Some(None)) {
            f.blocks.pop_back();
        }
    }

    /// Lote de entradas de diretório. Cookie 0 = ".", 1 = "..", 2 em diante = entradas com seq menor que
    /// `SEQ_SPACE - (cookie - 2)` (do mais novo pro mais antigo).
    fn readdir_batch(&self, cx: &Caller, ino: Ino, cookie: u64, max: usize) -> SysResult<(Vec<DirEntry>, u64)> {
        let out = {
            let g = self.inner.read();
            let i = Self::get(&g.state, ino)?;
            let d = match &i.data {
                Data::Dir(d) => d,
                _ => return Err(Errno::ENOTDIR),
            };
            if i.nlink == 0 {
                // iterate_dir num diretório morto (IS_DEADDIR).
                return Err(Errno::ENOENT);
            }
            let mut out = Vec::new();
            let mut c = cookie;
            if c == 0 && out.len() < max {
                out.push(DirEntry { ino, kind: FileType::Directory, name: b".".to_vec() });
                c = 1;
            }
            if c == 1 && out.len() < max {
                out.push(DirEntry { ino: i.parent, kind: FileType::Directory, name: b"..".to_vec() });
                c = 2;
            }
            if c >= 2 {
                let bound = SEQ_SPACE.saturating_sub(c - 2);
                for (seq, nm) in d.order.range(..bound).rev() {
                    if out.len() >= max {
                        break;
                    }
                    let slot = d.entries.get(nm).expect("índice de ordem coerente");
                    out.push(DirEntry { ino: slot.ino, kind: slot.kind, name: nm.to_vec() });
                    c = 2 + (SEQ_SPACE - *seq);
                }
            }
            (out, c)
        };
        self.touch_atime_now(ino, cx.now);
        Ok(out)
    }
}

impl FileSystem for Tmpfs {
    fn fs_type(&self) -> &'static str {
        "tmpfs"
    }

    fn dev(&self) -> u64 {
        self.dev
    }

    fn root_ino(&self) -> Ino {
        ROOT_INO
    }

    fn statfs(&self) -> StatFs {
        let g = self.inner.read();
        // Sem `size=`, o tmpfs do Linux usa metade da RAM; o pseudo-linus mostra 1 GiB e 262144 inodes.
        let blocks = self.limits.max_blocks.unwrap_or(262_144);
        let files = self.limits.max_inodes.unwrap_or(262_144);
        let bfree = blocks.saturating_sub(g.state.used_blocks);
        StatFs {
            fs_type: TMPFS_MAGIC,
            bsize: PAGE_SIZE,
            blocks,
            bfree,
            bavail: bfree,
            files,
            ffree: files.saturating_sub(g.state.used_inodes),
            namelen: NAME_MAX as u64,
            frsize: PAGE_SIZE,
            flags: 0,
        }
    }

    fn getattr(&self, _cx: &Caller, ino: Ino) -> SysResult<Stat> {
        let g = self.inner.read();
        let i = Self::get(&g.state, ino)?;
        Ok(self.stat_of(ino, i))
    }

    fn lookup(&self, _cx: &Caller, dir: Ino, nm: &[u8]) -> SysResult<Ino> {
        if nm.len() > NAME_MAX {
            return Err(Errno::ENAMETOOLONG);
        }
        let g = self.inner.read();
        let d = Self::dir(&g.state, dir)?;
        d.entries.get(nm).map(|s| s.ino).ok_or(Errno::ENOENT)
    }

    fn parent(&self, dir: Ino) -> SysResult<Ino> {
        let g = self.inner.read();
        Ok(Self::get(&g.state, dir)?.parent)
    }

    fn name_of(&self, ino: Ino) -> Option<(Ino, Vec<u8>)> {
        let g = self.inner.read();
        let i = g.state.inodes.get(&ino)?;
        if ino == ROOT_INO {
            return None;
        }
        Some((i.parent, i.name.to_vec()))
    }

    fn readlink(&self, _cx: &Caller, ino: Ino) -> SysResult<Vec<u8>> {
        let g = self.inner.read();
        match &Self::get(&g.state, ino)?.data {
            Data::Symlink(t) => Ok(t.to_vec()),
            _ => Err(Errno::EINVAL),
        }
    }

    fn create(&self, cx: &Caller, dir: Ino, nm: &[u8], node: NewNode) -> SysResult<Ino> {
        if nm.len() > NAME_MAX {
            return Err(Errno::ENAMETOOLONG);
        }
        let mut g = self.inner.write();
        {
            let d = Self::dir(&g.state, dir)?;
            if d.entries.contains_key(nm) {
                return Err(Errno::EEXIST);
            }
        }
        if Self::get(&g.state, dir)?.nlink == 0 {
            return Err(Errno::ENOENT);
        }
        self.check_inode_quota(&g)?;
        let now = cx.now;
        let ino = Self::alloc_ino(&mut g);
        let (data, nlink, rdev) = match &node.kind {
            NodeKind::Regular => (Data::File(FileData::default()), 1, 0),
            NodeKind::Directory => (Data::Dir(DirData::default()), 2, 0),
            NodeKind::Symlink(t) => {
                if t.len() >= PAGE_SIZE as usize {
                    return Err(Errno::ENAMETOOLONG);
                }
                (Data::Symlink(name(t)), 1, 0)
            }
            NodeKind::CharDev(d) | NodeKind::BlockDev(d) => (Data::Special, 1, *d),
            NodeKind::Fifo | NodeKind::Socket => (Data::Special, 1, 0),
        };
        let mode = node.kind.type_bits() | (node.perm & 0o7777);
        let inode = Inode {
            mode,
            uid: node.uid,
            gid: node.gid,
            nlink,
            rdev,
            atime: now,
            mtime: now,
            ctime: now,
            btime: now,
            parent: dir,
            name: name(nm),
            data,
        };
        let kind = inode.kind();
        let state = &mut g.state;
        state.inodes.insert(ino, Arc::new(inode));
        state.used_inodes += 1;
        Self::dir_insert(state, dir, name(nm), ino, kind)?;
        let di = Self::get_mut(state, dir)?;
        di.mtime = now;
        di.ctime = now;
        if kind == FileType::Directory {
            di.nlink += 1;
        }
        Ok(ino)
    }

    fn link(&self, cx: &Caller, ino: Ino, dir: Ino, nm: &[u8]) -> SysResult<()> {
        if nm.len() > NAME_MAX {
            return Err(Errno::ENAMETOOLONG);
        }
        let mut g = self.inner.write();
        if Self::dir(&g.state, dir)?.entries.contains_key(nm) {
            return Err(Errno::EEXIST);
        }
        let kind = {
            let i = Self::get(&g.state, ino)?;
            if i.nlink == 0 {
                return Err(Errno::ENOENT);
            }
            i.kind()
        };
        let now = cx.now;
        let state = &mut g.state;
        {
            let i = Self::get_mut(state, ino)?;
            i.nlink += 1;
            i.ctime = now;
        }
        Self::dir_insert(state, dir, name(nm), ino, kind)?;
        Self::touch_dir(state, dir, now)?;
        g.state.orphans.remove(&ino);
        Ok(())
    }

    fn unlink(&self, cx: &Caller, dir: Ino, nm: &[u8]) -> SysResult<()> {
        let mut g = self.inner.write();
        let slot = Self::dir_remove(&mut g.state, dir, nm)?;
        Self::touch_dir(&mut g.state, dir, cx.now)?;
        Self::drop_link(&mut g, slot.ino, cx.now, 1)
    }

    fn rmdir(&self, cx: &Caller, dir: Ino, nm: &[u8]) -> SysResult<()> {
        let mut g = self.inner.write();
        let victim = Self::dir(&g.state, dir)?.entries.get(nm).copied().ok_or(Errno::ENOENT)?;
        match &Self::get(&g.state, victim.ino)?.data {
            Data::Dir(d) => {
                if !d.entries.is_empty() {
                    return Err(Errno::ENOTEMPTY);
                }
            }
            _ => return Err(Errno::ENOTDIR),
        }
        Self::dir_remove(&mut g.state, dir, nm)?;
        {
            let di = Self::get_mut(&mut g.state, dir)?;
            di.nlink = di.nlink.saturating_sub(1);
            di.mtime = cx.now;
            di.ctime = cx.now;
        }
        Self::drop_link(&mut g, victim.ino, cx.now, 2)
    }

    fn rename(&self, cx: &Caller, odir: Ino, oname: &[u8], ndir: Ino, nname: &[u8], flags: RenameFlags) -> SysResult<()> {
        if nname.len() > NAME_MAX {
            return Err(Errno::ENAMETOOLONG);
        }
        let now = cx.now;
        let mut g = self.inner.write();
        let old = Self::dir(&g.state, odir)?.entries.get(oname).copied().ok_or(Errno::ENOENT)?;
        let new = Self::dir(&g.state, ndir)?.entries.get(nname).copied();
        let old_is_dir = old.kind == FileType::Directory;
        if flags.contains(RenameFlags::EXCHANGE) {
            let new = new.ok_or(Errno::ENOENT)?;
            let new_is_dir = new.kind == FileType::Directory;
            Self::dir_remove(&mut g.state, odir, oname)?;
            Self::dir_remove(&mut g.state, ndir, nname)?;
            // __d_move com exchange: o dentry do destino volta pra cabeça da lista primeiro e o da origem
            // depois, então o nome novo fica como a entrada mais nova.
            Self::dir_insert(&mut g.state, odir, name(oname), new.ino, new.kind)?;
            Self::dir_insert(&mut g.state, ndir, name(nname), old.ino, old.kind)?;
            if odir != ndir && old_is_dir != new_is_dir {
                let (gain, lose) = if old_is_dir { (ndir, odir) } else { (odir, ndir) };
                Self::get_mut(&mut g.state, gain)?.nlink += 1;
                let l = Self::get_mut(&mut g.state, lose)?;
                l.nlink = l.nlink.saturating_sub(1);
            }
            for (ino, parent, nm) in [(old.ino, ndir, nname), (new.ino, odir, oname)] {
                let i = Self::get_mut(&mut g.state, ino)?;
                i.parent = parent;
                i.name = name(nm);
                i.ctime = now;
            }
            Self::touch_dir(&mut g.state, odir, now)?;
            Self::touch_dir(&mut g.state, ndir, now)?;
            return Ok(());
        }
        if let Some(n) = new {
            if flags.contains(RenameFlags::NOREPLACE) {
                return Err(Errno::EEXIST);
            }
            if let Data::Dir(d) = &Self::get(&g.state, n.ino)?.data
                && !d.entries.is_empty()
            {
                return Err(Errno::ENOTEMPTY);
            }
        }
        Self::dir_remove(&mut g.state, odir, oname)?;
        if let Some(n) = new {
            Self::dir_remove(&mut g.state, ndir, nname)?;
            if old_is_dir {
                // O diretório substituído some (nlink 2 -> 0); o pai do destino perde um subdiretório e
                // ganha outro, o de origem perde um.
                Self::drop_link(&mut g, n.ino, now, 2)?;
                let l = Self::get_mut(&mut g.state, odir)?;
                l.nlink = l.nlink.saturating_sub(1);
            } else {
                Self::drop_link(&mut g, n.ino, now, 1)?;
            }
        } else if old_is_dir && odir != ndir {
            // Sem destino: o subdiretório muda de pai (no mesmo diretório o nlink não muda).
            let l = Self::get_mut(&mut g.state, odir)?;
            l.nlink = l.nlink.saturating_sub(1);
            Self::get_mut(&mut g.state, ndir)?.nlink += 1;
        }
        Self::dir_insert(&mut g.state, ndir, name(nname), old.ino, old.kind)?;
        {
            let i = Self::get_mut(&mut g.state, old.ino)?;
            i.parent = ndir;
            i.name = name(nname);
            i.ctime = now;
        }
        Self::touch_dir(&mut g.state, odir, now)?;
        if ndir != odir {
            Self::touch_dir(&mut g.state, ndir, now)?;
        }
        Ok(())
    }

    fn setattr(&self, _cx: &Caller, ino: Ino, a: &SetAttr) -> SysResult<()> {
        let mut g = self.inner.write();
        let state = &mut g.state;
        let mut used = state.used_blocks;
        let i = Self::get_mut(state, ino)?;
        if let Some(size) = a.size {
            match &mut i.data {
                // Estender por truncate cria buraco: não consome páginas da cota.
                Data::File(f) => Self::truncate_data(f, size, &mut used),
                Data::Dir(_) => return Err(Errno::EISDIR),
                _ => return Err(Errno::EINVAL),
            }
        }
        if let Some(m) = a.mode {
            i.mode = (i.mode & S_IFMT) | (m & 0o7777);
        }
        if let Some(u) = a.uid {
            i.uid = u;
        }
        if let Some(gid) = a.gid {
            i.gid = gid;
        }
        if let Some(t) = a.atime {
            i.atime = t;
        }
        if let Some(t) = a.mtime {
            i.mtime = t;
        }
        if let Some(t) = a.ctime {
            i.ctime = t;
        }
        state.used_blocks = used;
        Ok(())
    }

    fn open(self: Arc<Self>, _cx: &Caller, ino: Ino, _flags: OFlags) -> SysResult<Box<dyn FileHandle>> {
        {
            let g = self.inner.read();
            match &Self::get(&g.state, ino)?.data {
                Data::File(_) | Data::Dir(_) => {}
                _ => return Err(Errno::ENXIO),
            }
        }
        self.pin(ino);
        Ok(Box::new(TmpfsHandle { fs: self, ino }))
    }

    fn pin(&self, ino: Ino) {
        *self.inner.write().pins.entry(ino).or_insert(0) += 1;
    }

    fn unpin(&self, ino: Ino) {
        let mut g = self.inner.write();
        let gone = match g.pins.get_mut(&ino) {
            Some(n) => {
                *n -= 1;
                *n == 0
            }
            None => false,
        };
        if gone {
            g.pins.remove(&ino);
            let dead = g.state.inodes.get(&ino).is_some_and(|i| i.nlink == 0);
            if dead {
                g.state.orphans.remove(&ino);
                Self::drop_inode(&mut g.state, ino);
            }
        }
    }

    fn touch_atime(&self, cx: &Caller, ino: Ino) {
        self.touch_atime_now(ino, cx.now);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Arquivo ou diretório aberto num tmpfs: só o número do inode (ver o E03).
struct TmpfsHandle {
    fs: Arc<Tmpfs>,
    ino: Ino,
}

impl Drop for TmpfsHandle {
    fn drop(&mut self) {
        self.fs.unpin(self.ino);
    }
}

impl FileHandle for TmpfsHandle {
    fn read(&self, cx: &Caller, off: u64, buf: &mut [u8]) -> SysResult<usize> {
        self.fs.file_read(cx, self.ino, off, buf)
    }

    fn write(&self, cx: &Caller, pos: WritePos, buf: &[u8]) -> SysResult<(usize, u64)> {
        self.fs.file_write(cx, self.ino, pos, buf)
    }

    fn readdir(&self, cx: &Caller, cookie: u64, max: usize) -> SysResult<(Vec<DirEntry>, u64)> {
        self.fs.readdir_batch(cx, self.ino, cookie, max)
    }

    fn size(&self, cx: &Caller) -> SysResult<u64> {
        Ok(self.fs.getattr(cx, self.ino)?.size)
    }

    fn seek_data(&self, _cx: &Caller, off: u64, hole: bool) -> SysResult<u64> {
        self.fs.seek(self.ino, off, hole)
    }

    fn fallocate(&self, cx: &Caller, mode: FallocFlags, offset: u64, len: u64) -> SysResult<()> {
        self.fs.file_fallocate(cx, self.ino, mode, offset, len)
    }
}

#[cfg(test)]
mod fallocate_tests {
    use super::*;
    use crate::mount::{Mount, MountFlags};

    const T0: TimeSpec = TimeSpec { sec: 1_768_478_400, nsec: 0 };

    fn setup(limits: TmpfsLimits) -> (Arc<Tmpfs>, Caller, Ino, Box<dyn FileHandle>) {
        let fs = Tmpfs::new(0x2a, 0o755, T0, limits);
        let mnt = Arc::new(Mount {
            id: 1,
            fs: fs.clone(),
            parent: None,
            flags: MountFlags::empty(),
            source: "tmpfs".to_string(),
            fs_options: String::new(),
        });
        let root = mnt.root();
        let cx = Caller {
            cred: Arc::new(Cred::root()),
            root: root.clone(),
            cwd: root,
            umask: 0o022,
            now: T0,
            pid: 1,
            tid: 1,
            fsize_limit: u64::MAX,
        };
        let node = NewNode { kind: NodeKind::Regular, perm: 0o644, uid: 0, gid: 0 };
        let ino = fs.create(&cx, ROOT_INO, b"f", node).expect("cria");
        let h = fs.clone().open(&cx, ino, OFlags::RDWR).expect("abre");
        (fs, cx, ino, h)
    }

    fn st(fs: &Tmpfs, cx: &Caller, ino: Ino) -> Stat {
        fs.getattr(cx, ino).expect("stat")
    }

    const PS: u64 = PAGE_SIZE;

    #[test]
    fn mode_zero_allocates_and_extends() {
        let (fs, cx, ino, h) = setup(TmpfsLimits::default());
        h.fallocate(&cx, FallocFlags::empty(), 100, 3 * PS).unwrap();
        let s = st(&fs, &cx, ino);
        assert_eq!(s.size, 100 + 3 * PS);
        // Páginas 0 a 3: 4 páginas, 32 setores.
        assert_eq!(s.blocks, 32);
        assert_eq!(fs.usage().0, 4 * PS);
        let mut buf = vec![0xffu8; 200];
        assert_eq!(h.read(&cx, 0, &mut buf).unwrap(), 200);
        assert!(buf.iter().all(|b| *b == 0));
        // Reservar de novo o que já existe não muda nada.
        h.fallocate(&cx, FallocFlags::empty(), 0, PS).unwrap();
        assert_eq!(st(&fs, &cx, ino).blocks, 32);
        // Não encolhe.
        assert_eq!(st(&fs, &cx, ino).size, 100 + 3 * PS);
    }

    #[test]
    fn keep_size_allocates_past_eof_without_growing() {
        let (fs, cx, ino, h) = setup(TmpfsLimits::default());
        h.write(&cx, WritePos::At(0), b"abc").unwrap();
        h.fallocate(&cx, FallocFlags::KEEP_SIZE, 0, 8 * PS).unwrap();
        let s = st(&fs, &cx, ino);
        assert_eq!(s.size, 3);
        assert_eq!(s.blocks, 64);
        // As páginas além do fim sobrevivem a uma escrita e não viram dado no SEEK_DATA.
        h.write(&cx, WritePos::At(3), b"d").unwrap();
        assert_eq!(st(&fs, &cx, ino).blocks, 64);
        assert_eq!(h.seek_data(&cx, 0, false).unwrap(), 0);
        assert_eq!(h.seek_data(&cx, 0, true).unwrap(), 4);
        // Estender por truncate expõe zeros nelas.
        fs.setattr(&cx, ino, &SetAttr { size: Some(2 * PS), ..SetAttr::default() }).unwrap();
        let mut buf = [0xffu8; 8];
        h.read(&cx, PS, &mut buf).unwrap();
        assert_eq!(buf, [0; 8]);
        // Encolher devolve as páginas além do novo fim.
        fs.setattr(&cx, ino, &SetAttr { size: Some(1), ..SetAttr::default() }).unwrap();
        assert_eq!(st(&fs, &cx, ino).blocks, 8);
        assert_eq!(fs.usage().0, PS);
    }

    #[test]
    fn punch_hole_frees_whole_pages_and_zeroes_partial_ones() {
        let (fs, cx, ino, h) = setup(TmpfsLimits::default());
        let data = vec![0xaau8; (4 * PS) as usize];
        h.write(&cx, WritePos::At(0), &data).unwrap();
        assert_eq!(st(&fs, &cx, ino).blocks, 32);
        let mode = FallocFlags::PUNCH_HOLE | FallocFlags::KEEP_SIZE;
        // De 10 na página 0 até 10 na página 3: libera as páginas 1 e 2.
        h.fallocate(&cx, mode, 10, 3 * PS).unwrap();
        let s = st(&fs, &cx, ino);
        assert_eq!(s.size, 4 * PS);
        assert_eq!(s.blocks, 16);
        let mut all = vec![0u8; (4 * PS) as usize];
        h.read(&cx, 0, &mut all).unwrap();
        assert!(all[..10].iter().all(|b| *b == 0xaa));
        assert!(all[10..(3 * PS + 10) as usize].iter().all(|b| *b == 0));
        assert!(all[(3 * PS + 10) as usize..].iter().all(|b| *b == 0xaa));
        assert_eq!(h.seek_data(&cx, 0, true).unwrap(), PS);
        assert_eq!(h.seek_data(&cx, PS, false).unwrap(), 3 * PS);
        // Furar além do fim não muda o tamanho.
        h.fallocate(&cx, mode, 3 * PS, 10 * PS).unwrap();
        let s = st(&fs, &cx, ino);
        assert_eq!(s.size, 4 * PS);
        assert_eq!(s.blocks, 8);
        assert_eq!(fs.usage().0, PS);
    }

    #[test]
    fn unsupported_modes_are_eopnotsupp() {
        let (_fs, cx, _ino, h) = setup(TmpfsLimits::default());
        for m in [
            FallocFlags::COLLAPSE_RANGE,
            FallocFlags::ZERO_RANGE,
            FallocFlags::ZERO_RANGE | FallocFlags::KEEP_SIZE,
            FallocFlags::INSERT_RANGE,
            FallocFlags::UNSHARE_RANGE,
        ] {
            assert_eq!(h.fallocate(&cx, m, 0, PS), Err(Errno::EOPNOTSUPP), "{m:?}");
        }
    }

    #[test]
    fn generic_validation_order() {
        let v = |m: FallocFlags, o: i64, l: i64| m.validate(o, l);
        assert_eq!(v(FallocFlags::empty(), 0, 0), Err(Errno::EINVAL));
        assert_eq!(v(FallocFlags::empty(), -1, 10), Err(Errno::EINVAL));
        assert_eq!(v(FallocFlags::empty(), 0, -5), Err(Errno::EINVAL));
        assert_eq!(v(FallocFlags::from_bits_retain(0x80), 0, 1), Err(Errno::EOPNOTSUPP));
        assert_eq!(v(FallocFlags::PUNCH_HOLE, 0, 1), Err(Errno::EOPNOTSUPP));
        assert_eq!(v(FallocFlags::PUNCH_HOLE | FallocFlags::ZERO_RANGE | FallocFlags::KEEP_SIZE, 0, 1), Err(Errno::EOPNOTSUPP));
        assert_eq!(v(FallocFlags::COLLAPSE_RANGE | FallocFlags::KEEP_SIZE, 0, 1), Err(Errno::EINVAL));
        assert_eq!(v(FallocFlags::INSERT_RANGE | FallocFlags::KEEP_SIZE, 0, 1), Err(Errno::EINVAL));
        assert_eq!(v(FallocFlags::UNSHARE_RANGE | FallocFlags::ZERO_RANGE, 0, 1), Err(Errno::EINVAL));
        assert_eq!(v(FallocFlags::UNSHARE_RANGE | FallocFlags::KEEP_SIZE, 0, 1), Ok(()));
        assert_eq!(v(FallocFlags::PUNCH_HOLE | FallocFlags::KEEP_SIZE, 0, 1), Ok(()));
    }

    #[test]
    fn enospc_respects_limits_and_undoes_partial_allocation() {
        let (fs, cx, ino, h) = setup(TmpfsLimits { max_blocks: Some(4), max_inodes: None });
        // Pedido maior que o tmpfs inteiro: ENOSPC antes de alocar.
        assert_eq!(h.fallocate(&cx, FallocFlags::empty(), 0, 5 * PS), Err(Errno::ENOSPC));
        h.write(&cx, WritePos::At(0), b"x").unwrap();
        // Cabe no limite, mas não com a página já usada: desfaz o que esta chamada alocou.
        assert_eq!(h.fallocate(&cx, FallocFlags::empty(), PS, 4 * PS), Err(Errno::ENOSPC));
        let s = st(&fs, &cx, ino);
        assert_eq!((s.size, s.blocks), (1, 8));
        assert_eq!(fs.usage().0, PS);
        assert_eq!(fs.statfs().bfree, 3);
        // A página existente conta como já alocada.
        h.fallocate(&cx, FallocFlags::empty(), 0, 4 * PS).unwrap();
        let s = st(&fs, &cx, ino);
        assert_eq!((s.size, s.blocks), (4 * PS, 32));
        assert_eq!(fs.statfs().bfree, 0);
    }

    #[test]
    fn efbig_from_rlimit_and_updates_times() {
        let (fs, mut cx, ino, h) = setup(TmpfsLimits::default());
        cx.fsize_limit = 2 * PS;
        assert_eq!(h.fallocate(&cx, FallocFlags::KEEP_SIZE, PS, 2 * PS), Err(Errno::EFBIG));
        assert_eq!(st(&fs, &cx, ino).blocks, 0);
        // PUNCH_HOLE não passa pelo inode_newsize_ok.
        let later = TimeSpec { sec: T0.sec + 60, nsec: 0 };
        cx.now = later;
        h.fallocate(&cx, FallocFlags::PUNCH_HOLE | FallocFlags::KEEP_SIZE, PS, 2 * PS).unwrap();
        let s = st(&fs, &cx, ino);
        assert_eq!((s.mtime, s.ctime), (later, later));
    }
}
