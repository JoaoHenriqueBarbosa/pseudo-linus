//! Permissões: `generic_permission` do Linux com as capabilities do root.
//!
//! O root (uid 0) tem CAP_DAC_OVERRIDE e CAP_DAC_READ_SEARCH: lê e escreve qualquer coisa e atravessa
//! qualquer diretório; executar um arquivo regular ainda exige pelo menos um bit `x` (é assim que `cat`
//! de um arquivo modo 000 funciona como root e `./script` sem `x` dá "Permission denied" mesmo pro root).

use crate::types::*;

/// Checa `mask` (`MAY_READ | MAY_WRITE | MAY_EXEC`) contra o modo e o dono do inode.
pub fn permission(cred: &Cred, st: &Stat, mask: u32) -> SysResult<()> {
    if acl_permission_check(cred, st, mask) {
        return Ok(());
    }
    if cred.is_root() {
        if is_dir(st.mode) {
            // CAP_DAC_OVERRIDE cobre tudo num diretório.
            return Ok(());
        }
        // CAP_DAC_OVERRIDE: ler e escrever sempre; executar só se algum bit x existir.
        if mask & MAY_EXEC == 0 || st.mode & 0o111 != 0 {
            return Ok(());
        }
    }
    Err(Errno::EACCES)
}

/// A parte sem capabilities: escolhe a tríade do dono, do grupo ou dos outros.
fn acl_permission_check(cred: &Cred, st: &Stat, mask: u32) -> bool {
    let mode = st.mode;
    let bits = if cred.uid == st.uid {
        (mode >> 6) & 7
    } else if cred.in_group(st.gid) {
        (mode >> 3) & 7
    } else {
        mode & 7
    };
    (mask & !bits) == 0
}

/// `inode_permission`: EROFS pra escrita em montagem só de leitura (arquivo, diretório, symlink), depois
/// a checagem normal.
pub fn inode_permission(cred: &Cred, st: &Stat, mask: u32, read_only_mount: bool) -> SysResult<()> {
    if mask & MAY_WRITE != 0 && read_only_mount {
        let t = st.mode & S_IFMT;
        if t == S_IFREG || t == S_IFDIR || t == S_IFLNK {
            return Err(Errno::EROFS);
        }
    }
    permission(cred, st, mask)
}

/// `inode_owner_or_capable`: dono do inode ou CAP_FOWNER.
pub fn owner_or_capable(cred: &Cred, st: &Stat) -> bool {
    cred.uid == st.uid || cred.is_root()
}

/// `setattr_should_drop_suidgid`: o modo novo de um arquivo regular depois de escrita ou truncamento por
/// quem não tem CAP_FSETID (setuid sempre cai; setgid cai se tem x de grupo ou se quem escreve não é do
/// grupo do arquivo). `None` se nada muda.
pub fn drop_suidgid(cred: &Cred, mode: Mode, file_gid: Gid) -> Option<Mode> {
    if cred.is_root() || !is_reg(mode) {
        return None;
    }
    let mut m = mode;
    if m & S_ISUID != 0 {
        m &= !S_ISUID;
    }
    if m & S_ISGID != 0 && (m & 0o010 != 0 || !cred.in_group(file_gid)) {
        m &= !S_ISGID;
    }
    (m != mode).then_some(m & 0o7777)
}

/// `check_sticky`: num diretório com sticky bit só o dono do arquivo, o dono do diretório ou quem tem
/// CAP_FOWNER remove ou renomeia. `true` quer dizer que NÃO pode (EPERM).
pub fn sticky_denies(cred: &Cred, dir: &Stat, victim: &Stat) -> bool {
    if dir.mode & S_ISVTX == 0 {
        return false;
    }
    if cred.uid == victim.uid || cred.uid == dir.uid {
        return false;
    }
    !cred.is_root()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(mode: Mode, uid: Uid, gid: Gid) -> Stat {
        Stat { mode, uid, gid, ..Stat::default() }
    }

    #[test]
    fn root_bypasses_everything_but_exec_without_x() {
        let root = Cred::root();
        let f = st(S_IFREG, 1000, 1000);
        assert!(permission(&root, &f, MAY_READ | MAY_WRITE).is_ok());
        assert_eq!(permission(&root, &f, MAY_EXEC), Err(Errno::EACCES));
        let fx = st(S_IFREG | 0o010, 1000, 1000);
        assert!(permission(&root, &fx, MAY_EXEC).is_ok());
        let d = st(S_IFDIR, 1000, 1000);
        assert!(permission(&root, &d, MAY_EXEC | MAY_WRITE | MAY_READ).is_ok());
    }

    #[test]
    fn owner_triad_wins_even_if_others_have_more() {
        let user = Cred { uid: 1000, gid: 1000, groups: vec![1000] };
        let f = st(S_IFREG | 0o077, 1000, 1000);
        assert_eq!(permission(&user, &f, MAY_READ), Err(Errno::EACCES));
        let g = st(S_IFREG | 0o040, 0, 1000);
        assert!(permission(&user, &g, MAY_READ).is_ok());
        let o = st(S_IFREG | 0o004, 0, 0);
        assert!(permission(&user, &o, MAY_READ).is_ok());
        assert_eq!(permission(&user, &o, MAY_WRITE), Err(Errno::EACCES));
    }

    #[test]
    fn read_only_mount_gives_erofs_for_writes_only() {
        let root = Cred::root();
        let f = st(S_IFREG | 0o644, 0, 0);
        assert_eq!(inode_permission(&root, &f, MAY_WRITE, true), Err(Errno::EROFS));
        assert!(inode_permission(&root, &f, MAY_READ, true).is_ok());
        let fifo = st(S_IFIFO | 0o644, 0, 0);
        assert!(inode_permission(&root, &fifo, MAY_WRITE, true).is_ok());
    }
}
