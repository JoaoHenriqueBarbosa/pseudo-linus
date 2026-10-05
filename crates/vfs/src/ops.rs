//! As syscalls de arquivo, com a ordem de errnos do Linux 6.12 (man pages de cada syscall e o teste
//! diferencial contra o kernel do host). Os comentários citam a função do kernel que tem o mesmo
//! comportamento, como referência (ver `PROVENANCE.md`).

use std::sync::Arc;

use crate::fs::{FileHandle, Link, MagicObject, NewNode, NodeKind, SetAttr};
use crate::mount::{Loc, MountFlags, Namespace};
use crate::namei::{self, LastType, Parent, Resolved, Walker};
use crate::perm;
use crate::types::*;

/// De onde um caminho relativo parte (`dirfd`).
#[derive(Clone, Debug)]
pub enum Start {
    /// `AT_FDCWD`.
    Cwd,
    /// Um fd aberto no VFS (qualquer tipo; diretório quando o caminho é relativo).
    Dir(Loc),
    /// O `dirfd` é inválido ou não é do VFS: o erro só aparece se o caminho for relativo.
    Bad(Errno),
}

/// O que um `open` devolve ao kernel.
pub enum Opened {
    /// Arquivo regular ou diretório.
    File { loc: Loc, stat: Stat, handle: Box<dyn FileHandle> },
    /// `O_PATH`.
    Path { loc: Loc, stat: Stat },
    /// FIFO: o kernel faz o encontro de leitor e escritor.
    Fifo { loc: Loc, stat: Stat },
    /// Dispositivo de caractere: o kernel escolhe o driver por `rdev`.
    CharDev { loc: Loc, stat: Stat },
    /// Objeto do kernel atrás de um magic link.
    Object(Arc<dyn MagicObject>),
}

impl std::fmt::Debug for Opened {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Opened::File { loc, .. } => write!(f, "Opened::File({loc:?})"),
            Opened::Path { loc, .. } => write!(f, "Opened::Path({loc:?})"),
            Opened::Fifo { loc, .. } => write!(f, "Opened::Fifo({loc:?})"),
            Opened::CharDev { loc, stat } => write!(f, "Opened::CharDev({loc:?}, rdev {:#x})", stat.rdev),
            Opened::Object(_) => write!(f, "Opened::Object"),
        }
    }
}

/// Um executável resolvido pelo `execve`: onde está e os primeiros bytes (cabeçalho de builtin ou `#!`).
#[derive(Clone, Debug)]
pub struct ExecFile {
    pub loc: Loc,
    pub stat: Stat,
    pub head: Vec<u8>,
}

/// Quantos bytes o `execve` lê do começo do arquivo (`BINPRM_BUF_SIZE`).
pub const BINPRM_BUF_SIZE: usize = 256;

/// Caller interno do kernel: root, raiz e cwd em `root`.
pub fn kernel_caller(root: Loc) -> Caller {
    Caller {
        cred: Arc::new(Cred::root()),
        cwd: root.clone(),
        root,
        umask: 0,
        now: TimeSpec::default(),
        pid: 0,
        tid: 0,
        fsize_limit: u64::MAX,
    }
}

/// `getname`: caminho vazio é ENOENT; PATH_MAX conta o NUL.
fn check_path(path: &[u8]) -> SysResult<()> {
    if path.is_empty() {
        return Err(Errno::ENOENT);
    }
    if path.len() >= PATH_MAX {
        return Err(Errno::ENAMETOOLONG);
    }
    Ok(())
}

/// Dono, grupo e modo de um nó novo (`inode_init_owner`): grupo do diretório se ele tem setgid, e
/// diretórios herdam o setgid.
fn new_node(cx: &Caller, dir: &Stat, kind: NodeKind, mut perm: Mode) -> NewNode {
    let uid = cx.cred.uid;
    let gid = if dir.mode & S_ISGID != 0 {
        if kind == NodeKind::Directory {
            perm |= S_ISGID;
        } else if perm & (S_ISGID | 0o010) == (S_ISGID | 0o010) && !cx.cred.in_group(dir.gid) && !cx.cred.is_root() {
            perm &= !S_ISGID;
        }
        dir.gid
    } else {
        cx.cred.gid
    };
    NewNode { kind, perm, uid, gid }
}

/// `may_create`: diretório morto dá ENOENT; precisa de escrita e busca.
fn may_create(cx: &Caller, dir: &Stat) -> SysResult<()> {
    if dir.nlink == 0 {
        return Err(Errno::ENOENT);
    }
    perm::permission(&cx.cred, dir, MAY_WRITE | MAY_EXEC)
}

/// `may_delete`.
fn may_delete(cx: &Caller, dir: &Stat, victim: &Stat, isdir: bool, victim_is_fs_root: bool) -> SysResult<()> {
    perm::permission(&cx.cred, dir, MAY_WRITE | MAY_EXEC)?;
    if perm::sticky_denies(&cx.cred, dir, victim) {
        return Err(Errno::EPERM);
    }
    if isdir {
        if !is_dir(victim.mode) {
            return Err(Errno::ENOTDIR);
        }
        if victim_is_fs_root {
            return Err(Errno::EBUSY);
        }
    } else if is_dir(victim.mode) {
        return Err(Errno::EISDIR);
    }
    if dir.nlink == 0 {
        return Err(Errno::ENOENT);
    }
    Ok(())
}

/// Resultado do `filename_create`.
struct Creation {
    dir: Loc,
    dir_st: Stat,
    name: Vec<u8>,
}

impl Namespace {
    /// Ponto de partida de um caminho (`path_init`): absoluto ignora o dirfd; relativo precisa de um
    /// dirfd válido que seja diretório.
    fn start_loc(&self, cx: &Caller, start: &Start, path: &[u8]) -> SysResult<Loc> {
        if path.first() == Some(&b'/') {
            return Ok(cx.root.clone());
        }
        match start {
            Start::Cwd => Ok(cx.cwd.clone()),
            Start::Dir(l) => {
                let st = l.fs().getattr(cx, l.ino)?;
                if !is_dir(st.mode) {
                    return Err(Errno::ENOTDIR);
                }
                Ok(l.clone())
            }
            Start::Bad(e) => Err(*e),
        }
    }

    /// O alvo de um caminho vazio com `AT_EMPTY_PATH`: o próprio dirfd.
    fn empty_path_target(&self, cx: &Caller, start: &Start) -> SysResult<Resolved> {
        let l = match start {
            Start::Cwd => cx.cwd.clone(),
            Start::Dir(l) => l.clone(),
            Start::Bad(e) => return Err(*e),
        };
        let st = l.fs().getattr(cx, l.ino)?;
        Ok(Resolved::Loc(l, st))
    }

    /// Resolve um caminho inteiro (o `filename_lookup` das syscalls que só leem).
    pub fn resolve(&self, cx: &Caller, start: &Start, path: &[u8], follow: bool) -> SysResult<Resolved> {
        check_path(path)?;
        let s = self.start_loc(cx, start, path)?;
        let mut w = Walker::new(self, cx);
        let p = w.walk_parent(&s, path)?;
        w.resolve_last(p, follow, false)
    }

    /// Resolve o diretório pai e o último componente (`path_parentat`).
    pub fn resolve_parent(&self, cx: &Caller, start: &Start, path: &[u8]) -> SysResult<Parent> {
        check_path(path)?;
        let s = self.start_loc(cx, start, path)?;
        Walker::new(self, cx).walk_parent(&s, path)
    }

    /// `filename_create`: pai resolvido, último componente normal e inexistente. EEXIST vem antes de
    /// EROFS, que vem antes do ENOENT de barra no fim sem `want_dir`.
    fn filename_create(&self, cx: &Caller, start: &Start, path: &[u8], want_dir: bool) -> SysResult<Creation> {
        check_path(path)?;
        let s = self.start_loc(cx, start, path)?;
        let w = &mut Walker::new(self, cx);
        let p = w.walk_parent(&s, path)?;
        if p.ltype != LastType::Norm {
            return Err(Errno::EEXIST);
        }
        let ro = p.dir.mnt.read_only();
        match w.lookup_nomount(&p.dir, &p.last) {
            Ok(_) => return Err(Errno::EEXIST),
            Err(Errno::ENOENT) => {}
            Err(e) => return Err(e),
        }
        if p.trailing && !want_dir {
            return Err(Errno::ENOENT);
        }
        if ro {
            return Err(Errno::EROFS);
        }
        let dir_st = w.getattr(&p.dir)?;
        Ok(Creation { dir: p.dir, dir_st, name: p.last })
    }

    // ------------------------------------------------------------------------------------------
    // open
    // ------------------------------------------------------------------------------------------

    /// `openat(2)`: `build_open_flags` + `path_openat` + `do_open`.
    pub fn open(&self, cx: &Caller, start: &Start, path: &[u8], flags: OFlags, mode: Mode) -> SysResult<Opened> {
        // build_open_flags roda antes do getname: EINVAL de flags vem antes de ENOENT e ENAMETOOLONG.
        let mut flags = flags;
        if flags.contains(OFlags::PATH) {
            flags &= OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::PATH | OFlags::CLOEXEC;
        }
        if flags.contains(OFlags::TMPFILE) {
            // O_TMPFILE ainda não existe no pseudo-linus (o tmpfs do Linux aceita).
            return Err(Errno::ENOTSUP);
        }
        let creat = flags.contains(OFlags::CREAT);
        if creat && flags.contains(OFlags::DIRECTORY) {
            return Err(Errno::EINVAL);
        }
        check_path(path)?;
        let excl = creat && flags.contains(OFlags::EXCL);
        let nofollow = flags.contains(OFlags::NOFOLLOW) || excl;
        let opath = flags.contains(OFlags::PATH);
        let mut acc_mode = match flags.bits() & OFlags::ACCMODE {
            0 => MAY_READ,
            1 => MAY_WRITE,
            _ => MAY_READ | MAY_WRITE,
        };
        if flags.contains(OFlags::TRUNC) {
            acc_mode |= MAY_WRITE;
        }
        if opath {
            acc_mode = 0;
        }
        let must_dir = flags.contains(OFlags::DIRECTORY);
        let s = self.start_loc(cx, start, path)?;
        let mut w = Walker::new(self, cx);
        let mut p = w.walk_parent(&s, path)?;
        let mut created = false;
        let target = if !creat {
            w.resolve_last(p, !nofollow, must_dir)?
        } else {
            loop {
                if p.ltype != LastType::Norm {
                    break w.resolve_last(p, true, false)?;
                }
                if p.trailing {
                    return Err(Errno::EISDIR);
                }
                let dir = p.dir.clone();
                match w.lookup_child(&dir, &p.last) {
                    Ok(child) => {
                        let st = w.getattr(&child)?;
                        if is_lnk(st.mode) && !nofollow {
                            match w.pick_link(&child)? {
                                Link::Path(t) => {
                                    if t.is_empty() {
                                        return Err(Errno::ENOENT);
                                    }
                                    p = w.walk_parent(&dir, &t)?;
                                    continue;
                                }
                                Link::Jump(l) => {
                                    let l = self.follow_mounts(l);
                                    let st = w.getattr(&l)?;
                                    break Resolved::Loc(l, st);
                                }
                                Link::Object(o) => break Resolved::Object(o),
                            }
                        }
                        break Resolved::Loc(child, st);
                    }
                    Err(Errno::ENOENT) => {
                        // lookup_open: o arquivo não existe, cria. EROFS e EACCES só aparecem aqui.
                        if dir.mnt.read_only() {
                            return Err(Errno::EROFS);
                        }
                        let dst = w.getattr(&dir)?;
                        may_create(cx, &dst)?;
                        let node = new_node(cx, &dst, NodeKind::Regular, mode & 0o7777 & !cx.umask);
                        let ino = dir.fs().create(cx, dir.ino, &p.last, node)?;
                        created = true;
                        let loc = Loc { mnt: dir.mnt.clone(), ino };
                        let st = w.getattr(&loc)?;
                        break Resolved::Loc(loc, st);
                    }
                    Err(e) => return Err(e),
                }
            }
        };
        let (loc, st) = match target {
            Resolved::Object(o) => {
                if excl {
                    return Err(Errno::EEXIST);
                }
                return Ok(Opened::Object(o));
            }
            Resolved::Loc(l, st) => (l, st),
        };
        // do_open
        if creat {
            if excl && !created {
                return Err(Errno::EEXIST);
            }
            if is_dir(st.mode) {
                return Err(Errno::EISDIR);
            }
        }
        if must_dir && !is_dir(st.mode) {
            return Err(Errno::ENOTDIR);
        }
        if opath {
            return Ok(Opened::Path { loc, stat: st });
        }
        if !created {
            self.may_open(cx, &loc, &st, acc_mode, flags)?;
            if flags.contains(OFlags::TRUNC) && is_reg(st.mode) {
                self.truncate_loc(cx, &loc, 0, true)?;
            }
        }
        let st = if created || flags.contains(OFlags::TRUNC) { loc.fs().getattr(cx, loc.ino)? } else { st };
        match st.mode & S_IFMT {
            S_IFREG | S_IFDIR => {
                let handle = loc.fs().clone().open(cx, loc.ino, flags)?;
                Ok(Opened::File { loc, stat: st, handle })
            }
            S_IFIFO => Ok(Opened::Fifo { loc, stat: st }),
            S_IFCHR => Ok(Opened::CharDev { loc, stat: st }),
            S_IFLNK => Err(Errno::ELOOP),
            _ => Err(Errno::ENXIO),
        }
    }

    /// `may_open`.
    fn may_open(&self, cx: &Caller, loc: &Loc, st: &Stat, acc_mode: u32, flags: OFlags) -> SysResult<()> {
        match st.mode & S_IFMT {
            S_IFLNK => return Err(Errno::ELOOP),
            S_IFDIR if acc_mode & MAY_WRITE != 0 => return Err(Errno::EISDIR),
            S_IFBLK | S_IFCHR if loc.mnt.flags.contains(MountFlags::NODEV) => return Err(Errno::EACCES),
            _ => {}
        }
        perm::inode_permission(&cx.cred, st, acc_mode, loc.mnt.read_only())?;
        if flags.contains(OFlags::NOATIME) && !perm::owner_or_capable(&cx.cred, st) {
            return Err(Errno::EPERM);
        }
        Ok(())
    }

    // ------------------------------------------------------------------------------------------
    // stat, access, readlink, statfs
    // ------------------------------------------------------------------------------------------

    /// `fstatat(2)` / `statx`.
    pub fn stat(&self, cx: &Caller, start: &Start, path: &[u8], flags: AtFlags) -> SysResult<Resolved> {
        if !(AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT | AtFlags::EMPTY_PATH).contains(flags) {
            return Err(Errno::EINVAL);
        }
        if path.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
            return self.empty_path_target(cx, start);
        }
        self.resolve(cx, start, path, !flags.contains(AtFlags::SYMLINK_NOFOLLOW))
    }

    /// `faccessat2(2)`.
    pub fn access(&self, cx: &Caller, start: &Start, path: &[u8], mode: AccessMode, flags: AtFlags) -> SysResult<()> {
        let allowed = AtFlags::REMOVEDIR /* AT_EACCESS */ | AtFlags::SYMLINK_NOFOLLOW | AtFlags::EMPTY_PATH;
        if !allowed.contains(flags) {
            return Err(Errno::EINVAL);
        }
        let r = if path.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
            self.empty_path_target(cx, start)?
        } else {
            self.resolve(cx, start, path, !flags.contains(AtFlags::SYMLINK_NOFOLLOW))?
        };
        if mode.is_empty() {
            return Ok(());
        }
        let mut mask = 0;
        if mode.contains(AccessMode::R_OK) {
            mask |= MAY_READ;
        }
        if mode.contains(AccessMode::W_OK) {
            mask |= MAY_WRITE;
        }
        if mode.contains(AccessMode::X_OK) {
            mask |= MAY_EXEC;
        }
        match r {
            Resolved::Object(o) => perm::permission(&cx.cred, &o.stat(), mask),
            Resolved::Loc(l, st) => {
                if mask & MAY_EXEC != 0 && is_reg(st.mode) && l.mnt.flags.contains(MountFlags::NOEXEC) {
                    return Err(Errno::EACCES);
                }
                perm::inode_permission(&cx.cred, &st, mask, l.mnt.read_only())
            }
        }
    }

    /// `readlinkat(2)`.
    pub fn readlink(&self, cx: &Caller, start: &Start, path: &[u8]) -> SysResult<Vec<u8>> {
        let r = if path.is_empty() {
            match start {
                Start::Dir(l) => {
                    let st = l.fs().getattr(cx, l.ino)?;
                    if !is_lnk(st.mode) {
                        return Err(Errno::ENOENT);
                    }
                    Resolved::Loc(l.clone(), st)
                }
                _ => return Err(Errno::ENOENT),
            }
        } else {
            self.resolve(cx, start, path, false)?
        };
        match r {
            Resolved::Loc(l, st) if is_lnk(st.mode) => {
                let t = l.fs().readlink(cx, l.ino)?;
                l.fs().touch_atime(cx, l.ino);
                Ok(t)
            }
            _ => Err(Errno::EINVAL),
        }
    }

    /// `statfs(2)`.
    pub fn statfs(&self, cx: &Caller, start: &Start, path: &[u8]) -> SysResult<StatFs> {
        match self.resolve(cx, start, path, true)? {
            Resolved::Loc(l, _) => Ok(self.statfs_loc(&l)),
            Resolved::Object(_) => Err(Errno::ENOSYS),
        }
    }

    /// `statfs` de um lugar já resolvido (`fstatfs`), com as flags da montagem (`ST_*`).
    pub fn statfs_loc(&self, l: &Loc) -> StatFs {
        let mut s = l.fs().statfs();
        // ST_VALID (0x20) sempre; as outras espelham as flags de montagem.
        let mut f = 0x20u64;
        let mf = l.mnt.flags;
        if mf.contains(MountFlags::RDONLY) {
            f |= 1;
        }
        if mf.contains(MountFlags::NOSUID) {
            f |= 2;
        }
        if mf.contains(MountFlags::NODEV) {
            f |= 4;
        }
        if mf.contains(MountFlags::NOEXEC) {
            f |= 8;
        }
        if mf.contains(MountFlags::NOATIME) {
            f |= 0x400;
        }
        if mf.contains(MountFlags::NODIRATIME) {
            f |= 0x800;
        }
        if mf.contains(MountFlags::RELATIME) {
            f |= 0x1000;
        }
        s.flags = f;
        s
    }

    // ------------------------------------------------------------------------------------------
    // criação
    // ------------------------------------------------------------------------------------------

    /// `mkdirat(2)`.
    pub fn mkdir(&self, cx: &Caller, start: &Start, path: &[u8], mode: Mode) -> SysResult<()> {
        let c = self.filename_create(cx, start, path, true)?;
        may_create(cx, &c.dir_st)?;
        let perm = mode & (0o777 | S_ISVTX) & !cx.umask;
        let node = new_node(cx, &c.dir_st, NodeKind::Directory, perm);
        c.dir.fs().create(cx, c.dir.ino, &c.name, node)?;
        Ok(())
    }

    /// `mknodat(2)`.
    pub fn mknod(&self, cx: &Caller, start: &Start, path: &[u8], mode: Mode, dev: u64) -> SysResult<()> {
        check_path(path)?;
        let kind = match mode & S_IFMT {
            0 | S_IFREG => NodeKind::Regular,
            S_IFCHR => NodeKind::CharDev(dev),
            S_IFBLK => NodeKind::BlockDev(dev),
            S_IFIFO => NodeKind::Fifo,
            S_IFSOCK => NodeKind::Socket,
            S_IFDIR => return Err(Errno::EPERM),
            _ => return Err(Errno::EINVAL),
        };
        let c = self.filename_create(cx, start, path, false)?;
        may_create(cx, &c.dir_st)?;
        if matches!(kind, NodeKind::CharDev(_) | NodeKind::BlockDev(_)) && !cx.cred.is_root() {
            return Err(Errno::EPERM);
        }
        let perm = mode & 0o7777 & !cx.umask;
        let node = new_node(cx, &c.dir_st, kind, perm);
        c.dir.fs().create(cx, c.dir.ino, &c.name, node)?;
        Ok(())
    }

    /// `symlinkat(2)`.
    pub fn symlink(&self, cx: &Caller, target: &[u8], start: &Start, path: &[u8]) -> SysResult<()> {
        check_path(target)?;
        let c = self.filename_create(cx, start, path, false)?;
        may_create(cx, &c.dir_st)?;
        let node = new_node(cx, &c.dir_st, NodeKind::Symlink(target.to_vec()), 0o777);
        c.dir.fs().create(cx, c.dir.ino, &c.name, node)?;
        Ok(())
    }

    /// `linkat(2)`.
    pub fn link(&self, cx: &Caller, ostart: &Start, old: &[u8], nstart: &Start, new: &[u8], flags: AtFlags) -> SysResult<()> {
        if !(AtFlags::SYMLINK_FOLLOW | AtFlags::EMPTY_PATH).contains(flags) {
            return Err(Errno::EINVAL);
        }
        if flags.contains(AtFlags::EMPTY_PATH) && !cx.cred.is_root() {
            return Err(Errno::ENOENT);
        }
        let src = if old.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
            self.empty_path_target(cx, ostart)?
        } else {
            self.resolve(cx, ostart, old, flags.contains(AtFlags::SYMLINK_FOLLOW))?
        };
        let c = self.filename_create(cx, nstart, new, false)?;
        let (sloc, sst) = match src {
            Resolved::Loc(l, st) => (l, st),
            Resolved::Object(_) => return Err(Errno::EXDEV),
        };
        if sloc.mnt.id != c.dir.mnt.id {
            return Err(Errno::EXDEV);
        }
        // may_linkat (protected_hardlinks = 1 no Debian).
        if !perm::owner_or_capable(&cx.cred, &sst) {
            let ok = is_reg(sst.mode)
                && sst.mode & S_ISUID == 0
                && sst.mode & (S_ISGID | 0o010) != (S_ISGID | 0o010)
                && perm::permission(&cx.cred, &sst, MAY_READ | MAY_WRITE).is_ok();
            if !ok {
                return Err(Errno::EPERM);
            }
        }
        may_create(cx, &c.dir_st)?;
        if is_dir(sst.mode) {
            return Err(Errno::EPERM);
        }
        c.dir.fs().link(cx, sloc.ino, c.dir.ino, &c.name)
    }

    // ------------------------------------------------------------------------------------------
    // remoção e rename
    // ------------------------------------------------------------------------------------------

    /// `unlinkat(2)` (com `AT_REMOVEDIR`, o `rmdir`).
    pub fn unlink(&self, cx: &Caller, start: &Start, path: &[u8], flags: AtFlags) -> SysResult<()> {
        if !AtFlags::REMOVEDIR.contains(flags) {
            return Err(Errno::EINVAL);
        }
        let p = self.resolve_parent(cx, start, path)?;
        let w = Walker::new(self, cx);
        if flags.contains(AtFlags::REMOVEDIR) {
            match p.ltype {
                LastType::DotDot => return Err(Errno::ENOTEMPTY),
                LastType::Dot => return Err(Errno::EINVAL),
                LastType::Root => return Err(Errno::EBUSY),
                LastType::Norm => {}
            }
            if p.dir.mnt.read_only() {
                return Err(Errno::EROFS);
            }
            let victim = w.lookup_nomount(&p.dir, &p.last)?;
            let vst = w.getattr(&victim)?;
            let dst = w.getattr(&p.dir)?;
            may_delete(cx, &dst, &vst, true, victim.is_mount_root())?;
            if self.is_mountpoint(&victim) {
                return Err(Errno::EBUSY);
            }
            p.dir.fs().rmdir(cx, p.dir.ino, &p.last)
        } else {
            if p.ltype != LastType::Norm {
                return Err(Errno::EISDIR);
            }
            if p.dir.mnt.read_only() {
                return Err(Errno::EROFS);
            }
            let victim = w.lookup_nomount(&p.dir, &p.last)?;
            let vst = w.getattr(&victim)?;
            if p.trailing {
                return Err(if is_dir(vst.mode) { Errno::EISDIR } else { Errno::ENOTDIR });
            }
            let dst = w.getattr(&p.dir)?;
            may_delete(cx, &dst, &vst, false, false)?;
            if self.is_mountpoint(&victim) {
                return Err(Errno::EBUSY);
            }
            p.dir.fs().unlink(cx, p.dir.ino, &p.last)
        }
    }

    /// `renameat2(2)`: `do_renameat2` + `vfs_rename`.
    pub fn rename(&self, cx: &Caller, ostart: &Start, old: &[u8], nstart: &Start, new: &[u8], flags: RenameFlags) -> SysResult<()> {
        let known = RenameFlags::NOREPLACE | RenameFlags::EXCHANGE | RenameFlags::WHITEOUT;
        if !known.contains(flags) {
            return Err(Errno::EINVAL);
        }
        if flags.contains(RenameFlags::NOREPLACE | RenameFlags::EXCHANGE) {
            return Err(Errno::EINVAL);
        }
        if flags.contains(RenameFlags::WHITEOUT) {
            if flags.intersects(RenameFlags::NOREPLACE | RenameFlags::EXCHANGE) {
                return Err(Errno::EINVAL);
            }
            if !cx.cred.is_root() {
                return Err(Errno::EPERM);
            }
            // Whiteout é coisa de overlayfs; o tmpfs do pseudo-linus não cria.
            return Err(Errno::EINVAL);
        }
        let exchange = flags.contains(RenameFlags::EXCHANGE);
        let noreplace = flags.contains(RenameFlags::NOREPLACE);
        let op = self.resolve_parent(cx, ostart, old)?;
        let np = self.resolve_parent(cx, nstart, new)?;
        if op.dir.mnt.id != np.dir.mnt.id {
            return Err(Errno::EXDEV);
        }
        if op.ltype != LastType::Norm {
            return Err(Errno::EBUSY);
        }
        if np.ltype != LastType::Norm {
            return Err(if noreplace { Errno::EEXIST } else { Errno::EBUSY });
        }
        if op.dir.mnt.read_only() {
            return Err(Errno::EROFS);
        }
        let w = Walker::new(self, cx);
        let oloc = w.lookup_nomount(&op.dir, &op.last)?;
        let ost = w.getattr(&oloc)?;
        let nloc = match w.lookup_nomount(&np.dir, &np.last) {
            Ok(l) => Some(l),
            Err(Errno::ENOENT) => None,
            Err(e) => return Err(e),
        };
        let nst = match &nloc {
            Some(l) => Some(w.getattr(l)?),
            None => None,
        };
        if noreplace && nloc.is_some() {
            return Err(Errno::EEXIST);
        }
        if exchange {
            match &nst {
                None => return Err(Errno::ENOENT),
                Some(st) if !is_dir(st.mode) && np.trailing => return Err(Errno::ENOTDIR),
                _ => {}
            }
        }
        if !is_dir(ost.mode) && (op.trailing || (!exchange && np.trailing)) {
            return Err(Errno::ENOTDIR);
        }
        // lock_rename: a origem não pode ser ancestral do destino; o destino não pode ser ancestral da
        // origem.
        if op.dir != np.dir {
            if is_dir(ost.mode) && self.is_ancestor(cx, &oloc, &np.dir)? {
                return Err(Errno::EINVAL);
            }
            if let (Some(nl), Some(st)) = (&nloc, &nst)
                && is_dir(st.mode)
                && self.is_ancestor(cx, nl, &op.dir)?
            {
                return Err(if exchange { Errno::EINVAL } else { Errno::ENOTEMPTY });
            }
        }
        // vfs_rename
        if let Some(nl) = &nloc
            && nl.ino == oloc.ino
        {
            return Ok(());
        }
        let is_dir_old = is_dir(ost.mode);
        let odst = w.getattr(&op.dir)?;
        let ndst = if np.dir == op.dir { odst.clone() } else { w.getattr(&np.dir)? };
        may_delete(cx, &odst, &ost, is_dir_old, oloc.is_mount_root())?;
        match &nst {
            None => may_create(cx, &ndst)?,
            Some(st) => {
                let isdir = if exchange { is_dir(st.mode) } else { is_dir_old };
                may_delete(cx, &ndst, st, isdir, nloc.as_ref().is_some_and(|l| l.is_mount_root()))?;
            }
        }
        if op.dir != np.dir {
            if is_dir_old {
                perm::permission(&cx.cred, &ost, MAY_WRITE)?;
            }
            if exchange
                && let Some(st) = &nst
                && is_dir(st.mode)
            {
                perm::permission(&cx.cred, st, MAY_WRITE)?;
            }
        }
        if self.is_mountpoint(&oloc) || nloc.as_ref().is_some_and(|l| self.is_mountpoint(l)) {
            return Err(Errno::EBUSY);
        }
        let fflags = flags & (RenameFlags::NOREPLACE | RenameFlags::EXCHANGE);
        op.dir.fs().rename(cx, op.dir.ino, &op.last, np.dir.ino, &np.last, fflags)
    }

    /// Diz se o diretório `anc` é `loc` ou um ancestral dele (mesma montagem).
    fn is_ancestor(&self, _cx: &Caller, anc: &Loc, loc: &Loc) -> SysResult<bool> {
        if anc.mnt.id != loc.mnt.id {
            return Ok(false);
        }
        let fs = loc.fs();
        let root = fs.root_ino();
        let mut cur = loc.ino;
        for _ in 0..PATH_MAX {
            if cur == anc.ino {
                return Ok(true);
            }
            if cur == root {
                return Ok(false);
            }
            let p = fs.parent(cur)?;
            if p == cur {
                return Ok(false);
            }
            cur = p;
        }
        Ok(false)
    }

    // ------------------------------------------------------------------------------------------
    // atributos
    // ------------------------------------------------------------------------------------------

    fn resolve_attr_target(&self, cx: &Caller, start: &Start, path: &[u8], flags: AtFlags) -> SysResult<(Loc, Stat)> {
        let r = if path.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
            self.empty_path_target(cx, start)?
        } else {
            self.resolve(cx, start, path, !flags.contains(AtFlags::SYMLINK_NOFOLLOW))?
        };
        match r {
            Resolved::Loc(l, st) => Ok((l, st)),
            // Atributos do inode de um pipe: o pipefs aceita e ninguém vê. Mantemos sem efeito.
            Resolved::Object(_) => Err(Errno::ENOTSUP),
        }
    }

    /// `fchmodat2(2)`.
    pub fn chmod(&self, cx: &Caller, start: &Start, path: &[u8], mode: Mode, flags: AtFlags) -> SysResult<()> {
        if !(AtFlags::SYMLINK_NOFOLLOW | AtFlags::EMPTY_PATH).contains(flags) {
            return Err(Errno::EINVAL);
        }
        let (l, st) = self.resolve_attr_target(cx, start, path, flags)?;
        self.chmod_loc(cx, &l, &st, mode)
    }

    /// `chmod_common` + `notify_change` + `setattr_prepare`.
    pub fn chmod_loc(&self, cx: &Caller, l: &Loc, st: &Stat, mode: Mode) -> SysResult<()> {
        if l.mnt.read_only() {
            return Err(Errno::EROFS);
        }
        if is_lnk(st.mode) {
            return Err(Errno::ENOTSUP);
        }
        if !perm::owner_or_capable(&cx.cred, st) {
            return Err(Errno::EPERM);
        }
        let mut m = mode & 0o7777;
        if m & S_ISGID != 0 && !cx.cred.in_group(st.gid) && !cx.cred.is_root() {
            m &= !S_ISGID;
        }
        l.fs().setattr(cx, l.ino, &SetAttr { mode: Some(m), ctime: Some(cx.now), ..SetAttr::default() })
    }

    /// `fchownat(2)`.
    pub fn chown(&self, cx: &Caller, start: &Start, path: &[u8], uid: Option<Uid>, gid: Option<Gid>, flags: AtFlags) -> SysResult<()> {
        if !(AtFlags::SYMLINK_NOFOLLOW | AtFlags::EMPTY_PATH).contains(flags) {
            return Err(Errno::EINVAL);
        }
        let (l, st) = self.resolve_attr_target(cx, start, path, flags)?;
        self.chown_loc(cx, &l, &st, uid, gid)
    }

    /// `chown_common` + `chown_ok`/`chgrp_ok`.
    pub fn chown_loc(&self, cx: &Caller, l: &Loc, st: &Stat, uid: Option<Uid>, gid: Option<Gid>) -> SysResult<()> {
        if l.mnt.read_only() {
            return Err(Errno::EROFS);
        }
        let root = cx.cred.is_root();
        let owner = cx.cred.uid == st.uid;
        if let Some(u) = uid
            && !root
            && !(owner && u == st.uid)
        {
            return Err(Errno::EPERM);
        }
        if let Some(g) = gid
            && !root
            && !(owner && (g == st.gid || cx.cred.in_group(g)))
        {
            return Err(Errno::EPERM);
        }
        let mut attr = SetAttr { uid, gid, ctime: Some(cx.now), ..SetAttr::default() };
        if !is_dir(st.mode) && !is_lnk(st.mode) {
            let mut m = st.mode & 0o7777;
            m &= !S_ISUID;
            if m & S_ISGID != 0 && (m & 0o010 != 0 || (!cx.cred.in_group(st.gid) && !root)) {
                m &= !S_ISGID;
            }
            if m != st.mode & 0o7777 {
                attr.mode = Some(m);
            }
        }
        l.fs().setattr(cx, l.ino, &attr)
    }

    /// `utimensat(2)`. Os dois `UTIME_OMIT` voltam 0 sem nem resolver o caminho, como o kernel.
    pub fn utimens(&self, cx: &Caller, start: &Start, path: &[u8], atime: SetTime, mtime: SetTime, flags: AtFlags) -> SysResult<()> {
        if atime == SetTime::Omit && mtime == SetTime::Omit {
            return Ok(());
        }
        for t in [atime, mtime] {
            if let SetTime::At(ts) = t
                && ts.nsec >= 1_000_000_000
            {
                return Err(Errno::EINVAL);
            }
        }
        if !(AtFlags::SYMLINK_NOFOLLOW | AtFlags::EMPTY_PATH).contains(flags) {
            return Err(Errno::EINVAL);
        }
        let (l, st) = self.resolve_attr_target(cx, start, path, flags)?;
        self.utimens_loc(cx, &l, &st, atime, mtime)
    }

    /// `vfs_utimes`: hora explícita exige ser dono (EPERM); "agora" basta permissão de escrita (EACCES).
    pub fn utimens_loc(&self, cx: &Caller, l: &Loc, st: &Stat, atime: SetTime, mtime: SetTime) -> SysResult<()> {
        if atime == SetTime::Omit && mtime == SetTime::Omit {
            return Ok(());
        }
        if l.mnt.read_only() {
            return Err(Errno::EROFS);
        }
        let explicit = matches!(atime, SetTime::At(_)) || matches!(mtime, SetTime::At(_));
        if !perm::owner_or_capable(&cx.cred, st) {
            if explicit {
                return Err(Errno::EPERM);
            }
            perm::inode_permission(&cx.cred, st, MAY_WRITE, false)?;
        }
        let pick = |t: SetTime| match t {
            SetTime::Now => Some(cx.now),
            SetTime::Omit => None,
            SetTime::At(ts) => Some(ts),
        };
        let attr = SetAttr { atime: pick(atime), mtime: pick(mtime), ctime: Some(cx.now), ..SetAttr::default() };
        l.fs().setattr(cx, l.ino, &attr)
    }

    /// Truncamento (`do_truncate`): `ftruncate` e `O_TRUNC` sempre atualizam mtime e ctime.
    pub fn truncate_loc(&self, cx: &Caller, l: &Loc, len: u64, _from_open: bool) -> SysResult<()> {
        if len > MAX_FILE_SIZE {
            return Err(Errno::EFBIG);
        }
        if len > cx.fsize_limit {
            return Err(Errno::EFBIG);
        }
        let st = l.fs().getattr(cx, l.ino)?;
        // "Remove suid, sgid, and file capabilities on truncate too".
        let mode = perm::drop_suidgid(&cx.cred, st.mode, st.gid);
        let attr = SetAttr { size: Some(len), mode, mtime: Some(cx.now), ctime: Some(cx.now), ..SetAttr::default() };
        l.fs().setattr(cx, l.ino, &attr)
    }

    // ------------------------------------------------------------------------------------------
    // cwd, exec, caminhos
    // ------------------------------------------------------------------------------------------

    /// Alvo de `chdir(2)`: diretório com permissão de busca.
    pub fn chdir_target(&self, cx: &Caller, start: &Start, path: &[u8]) -> SysResult<Loc> {
        match self.resolve(cx, start, path, true)? {
            Resolved::Loc(l, st) => {
                if !is_dir(st.mode) {
                    return Err(Errno::ENOTDIR);
                }
                perm::permission(&cx.cred, &st, MAY_EXEC)?;
                Ok(l)
            }
            Resolved::Object(_) => Err(Errno::ENOTDIR),
        }
    }

    /// `fchdir(2)` de um lugar já aberto.
    pub fn fchdir_check(&self, cx: &Caller, l: &Loc) -> SysResult<()> {
        let st = l.fs().getattr(cx, l.ino)?;
        if !is_dir(st.mode) {
            return Err(Errno::ENOTDIR);
        }
        perm::permission(&cx.cred, &st, MAY_EXEC)
    }

    /// `getcwd(2)`: ENOENT se o cwd foi removido.
    pub fn getcwd(&self, cx: &Caller) -> SysResult<Vec<u8>> {
        let st = cx.cwd.fs().getattr(cx, cx.cwd.ino)?;
        if st.nlink == 0 {
            return Err(Errno::ENOENT);
        }
        namei::d_path(&cx.cwd, &cx.root)
    }

    /// Caminho de um arquivo aberto como `/proc/self/fd` mostra: com " (deleted)" se ele não tem mais
    /// nome.
    pub fn fd_path(&self, cx: &Caller, l: &Loc) -> Vec<u8> {
        let deleted = match l.fs().getattr(cx, l.ino) {
            Ok(st) => st.nlink == 0,
            Err(_) => true,
        };
        let mut p = namei::d_path(l, &cx.root).unwrap_or_else(|_| {
            l.fs().name_of(l.ino).map(|(_, n)| [b"/".as_slice(), &n].concat()).unwrap_or_else(|| b"/".to_vec())
        });
        if deleted {
            p.extend_from_slice(b" (deleted)");
        }
        p
    }

    /// Abre um executável pro `execve` (`do_open_execat`): arquivo regular, montagem sem noexec,
    /// permissão de execução; devolve os primeiros [`BINPRM_BUF_SIZE`] bytes.
    pub fn exec_open(&self, cx: &Caller, start: &Start, path: &[u8]) -> SysResult<ExecFile> {
        let (loc, st) = match self.resolve(cx, start, path, true)? {
            Resolved::Loc(l, st) => (l, st),
            Resolved::Object(_) => return Err(Errno::EACCES),
        };
        if !is_reg(st.mode) || loc.mnt.flags.contains(MountFlags::NOEXEC) {
            return Err(Errno::EACCES);
        }
        perm::inode_permission(&cx.cred, &st, MAY_EXEC, false)?;
        let h = loc.fs().clone().open(cx, loc.ino, OFlags::RDONLY)?;
        let mut head = vec![0u8; BINPRM_BUF_SIZE];
        let n = h.read(cx, 0, &mut head)?;
        head.truncate(n);
        Ok(ExecFile { loc, stat: st, head })
    }

    /// `stat` de um lugar.
    pub fn stat_loc(&self, cx: &Caller, l: &Loc) -> SysResult<Stat> {
        l.fs().getattr(cx, l.ino)
    }
}
