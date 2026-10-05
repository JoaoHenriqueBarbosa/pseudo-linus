//! Tipos comuns do VFS: credenciais, o contexto de quem chama e as constantes do Linux.

use std::sync::Arc;

pub use sysabi::mode::*;
pub use sysabi::{
    AccessMode, AtFlags, DirEntry, Errno, FileType, Gid, Mode, OFlags, Pid, RenameFlags, SetTime, Stat, StatFs,
    TimeSpec, Uid,
};

use crate::mount::Loc;

pub type SysResult<T> = Result<T, Errno>;

/// Número de inode dentro de um sistema de arquivos.
pub type Ino = u64;

/// `PATH_MAX`: tamanho máximo de um caminho contando o NUL (então 4095 bytes úteis).
pub const PATH_MAX: usize = 4096;
/// `NAME_MAX`: tamanho máximo de um componente.
pub const NAME_MAX: usize = 255;
/// Symlinks seguidos numa resolução: 40 resolvem, o 41º dá ELOOP.
pub const MAXSYMLINKS: u32 = 40;
/// Tamanho de página e de bloco do tmpfs.
pub const PAGE_SIZE: u64 = 4096;
/// `MAX_LFS_FILESIZE` no x86_64.
pub const MAX_FILE_SIZE: u64 = i64::MAX as u64;

/// Pedidos de permissão (`MAY_*` do kernel).
pub const MAY_EXEC: u32 = 1;
pub const MAY_WRITE: u32 = 2;
pub const MAY_READ: u32 = 4;

/// Credenciais de um processo. O pseudo-linus não separa uid real, efetivo e de FS (não há setuid), então
/// um só uid e um só gid valem pra tudo. uid 0 tem todas as capabilities, como o root do Linux.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cred {
    pub uid: Uid,
    pub gid: Gid,
    /// Grupos suplementares.
    pub groups: Vec<Gid>,
}

impl Cred {
    pub fn root() -> Cred {
        Cred { uid: 0, gid: 0, groups: vec![0] }
    }

    /// Tem as capabilities do root (CAP_DAC_OVERRIDE, CAP_FOWNER, CAP_CHOWN, CAP_MKNOD...).
    pub fn is_root(&self) -> bool {
        self.uid == 0
    }

    /// `in_group_p`: o gid principal ou um dos suplementares.
    pub fn in_group(&self, gid: Gid) -> bool {
        self.gid == gid || self.groups.contains(&gid)
    }
}

/// Quem está chamando uma operação de arquivo: o equivalente às partes do `current` que o VFS do Linux
/// consulta (credenciais, `fs_struct`, umask, rlimit de tamanho de arquivo) mais o relógio do sandbox.
#[derive(Clone, Debug)]
pub struct Caller {
    pub cred: Arc<Cred>,
    /// Raiz do processo (o `..` não passa dela).
    pub root: Loc,
    pub cwd: Loc,
    pub umask: Mode,
    /// Instante usado nos carimbos de tempo.
    pub now: TimeSpec,
    /// pid do processo (pra `/proc/self`).
    pub pid: Pid,
    /// tid da thread (pra `/proc/thread-self`); igual ao pid na thread principal.
    pub tid: Pid,
    /// `RLIMIT_FSIZE` corrente, em bytes.
    pub fsize_limit: u64,
}

/// Divide `dev_t` como o glibc (`major`/`minor`).
pub fn dev_major(dev: u64) -> u32 {
    (((dev >> 32) & 0xffff_f000) | ((dev >> 8) & 0x0000_0fff)) as u32
}

pub fn dev_minor(dev: u64) -> u32 {
    (((dev >> 12) & 0xffff_ff00) | (dev & 0x0000_00ff)) as u32
}

/// `makedev` do glibc.
pub fn makedev(major: u32, minor: u32) -> u64 {
    let (ma, mi) = (major as u64, minor as u64);
    ((ma & 0xffff_f000) << 32) | ((ma & 0x0000_0fff) << 8) | ((mi & 0xffff_ff00) << 12) | (mi & 0x0000_00ff)
}

pub fn is_dir(mode: Mode) -> bool {
    mode & S_IFMT == S_IFDIR
}

pub fn is_reg(mode: Mode) -> bool {
    mode & S_IFMT == S_IFREG
}

pub fn is_lnk(mode: Mode) -> bool {
    mode & S_IFMT == S_IFLNK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makedev_roundtrip_matches_glibc() {
        let d = makedev(1, 3);
        assert_eq!(d, 0x103);
        assert_eq!((dev_major(d), dev_minor(d)), (1, 3));
        let d = makedev(136, 300);
        assert_eq!((dev_major(d), dev_minor(d)), (136, 300));
    }
}
