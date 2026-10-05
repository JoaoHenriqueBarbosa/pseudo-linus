//! A interface entre o VFS e cada sistema de arquivos.
//!
//! O VFS ([`crate::Namespace`]) faz a resolução de caminho, as checagens de permissão e a ordem dos
//! errnos; o sistema de arquivos só guarda e devolve: procura um nome num diretório, cria, remove,
//! renomeia, lê e escreve. É a mesma divisão do Linux entre `fs/namei.c` e as `inode_operations` de cada
//! sistema de arquivos. Os erros que são do sistema de arquivos (ENOSPC, ENOTEMPTY, ENAMETOOLONG num
//! nome comprido) saem daqui.

use std::any::Any;
use std::sync::Arc;

use crate::mount::Loc;
use crate::types::*;

/// Tipo e conteúdo inicial de um nó novo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Regular,
    Directory,
    Symlink(Vec<u8>),
    Fifo,
    CharDev(u64),
    BlockDev(u64),
    Socket,
}

impl NodeKind {
    pub fn type_bits(&self) -> Mode {
        match self {
            NodeKind::Regular => S_IFREG,
            NodeKind::Directory => S_IFDIR,
            NodeKind::Symlink(_) => S_IFLNK,
            NodeKind::Fifo => S_IFIFO,
            NodeKind::CharDev(_) => S_IFCHR,
            NodeKind::BlockDev(_) => S_IFBLK,
            NodeKind::Socket => S_IFSOCK,
        }
    }
}

/// Nó a criar: o VFS já aplicou a umask e decidiu dono e grupo (herança de setgid do diretório).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewNode {
    pub kind: NodeKind,
    /// Bits de permissão (incluindo setuid, setgid e sticky).
    pub perm: Mode,
    pub uid: Uid,
    pub gid: Gid,
}

/// Mudança de atributos (`struct iattr`). O VFS já validou permissões e preencheu os carimbos.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SetAttr {
    /// Só os bits de permissão (o tipo não muda).
    pub mode: Option<Mode>,
    pub uid: Option<Uid>,
    pub gid: Option<Gid>,
    pub size: Option<u64>,
    pub atime: Option<TimeSpec>,
    pub mtime: Option<TimeSpec>,
    pub ctime: Option<TimeSpec>,
}

/// Onde uma escrita cai.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WritePos {
    At(u64),
    /// `O_APPEND`: no fim, atomicamente com a escrita.
    Append,
}

/// Objeto que não é um inode do VFS mas aparece num caminho, como o pipe atrás de `/proc/self/fd/0`.
/// O kernel implementa (pipes); o VFS só precisa do `stat` e de devolver o objeto ao kernel no `open`.
pub trait MagicObject: Send + Sync {
    fn stat(&self) -> Stat;
    fn as_any(&self) -> &dyn Any;
}

/// Pra onde um symlink leva.
#[derive(Clone)]
pub enum Link {
    /// Symlink comum: o texto é resolvido de novo pelo namei.
    Path(Vec<u8>),
    /// "Magic link" do procfs (`/proc/self/cwd`, `/proc/self/fd/3` de um arquivo): pula direto pro lugar.
    Jump(Loc),
    /// Magic link pra um objeto do kernel (pipe). Só vale como último componente.
    Object(Arc<dyn MagicObject>),
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Link::Path(p) => write!(f, "Path({:?})", String::from_utf8_lossy(p)),
            Link::Jump(l) => write!(f, "Jump({l:?})"),
            Link::Object(_) => write!(f, "Object"),
        }
    }
}

/// Um sistema de arquivos montável.
pub trait FileSystem: Send + Sync + Any {
    /// Nome do tipo, como em `/proc/mounts` ("tmpfs", "proc").
    fn fs_type(&self) -> &'static str;
    /// `st_dev` dos inodes.
    fn dev(&self) -> u64;
    fn root_ino(&self) -> Ino;
    fn statfs(&self) -> StatFs;
    /// `stat` de um inode; ESTALE se ele não existe mais.
    fn getattr(&self, cx: &Caller, ino: Ino) -> SysResult<Stat>;
    /// Procura `name` no diretório `dir`. ENOENT se não existe; ENAMETOOLONG se o nome passa de
    /// NAME_MAX. O VFS já checou que `dir` é diretório e que dá pra atravessá-lo.
    fn lookup(&self, cx: &Caller, dir: Ino, name: &[u8]) -> SysResult<Ino>;
    /// O `..` de um diretório dentro deste sistema de arquivos (a raiz devolve ela mesma).
    fn parent(&self, dir: Ino) -> SysResult<Ino>;
    /// Diretório pai e nome com que o inode foi visto por último (pra `getcwd` e `/proc/self/fd`).
    fn name_of(&self, ino: Ino) -> Option<(Ino, Vec<u8>)>;
    /// Texto de um symlink (`readlink`).
    fn readlink(&self, cx: &Caller, ino: Ino) -> SysResult<Vec<u8>>;
    /// O que seguir um symlink faz. Padrão: resolver o texto.
    fn follow_link(&self, cx: &Caller, ino: Ino) -> SysResult<Link> {
        Ok(Link::Path(self.readlink(cx, ino)?))
    }
    fn create(&self, cx: &Caller, dir: Ino, name: &[u8], node: NewNode) -> SysResult<Ino>;
    /// Hardlink de `ino` em `dir/name`. ENOENT se `ino` já não tem nome nenhum.
    fn link(&self, cx: &Caller, ino: Ino, dir: Ino, name: &[u8]) -> SysResult<()>;
    fn unlink(&self, cx: &Caller, dir: Ino, name: &[u8]) -> SysResult<()>;
    /// ENOTEMPTY se o diretório tem entradas.
    fn rmdir(&self, cx: &Caller, dir: Ino, name: &[u8]) -> SysResult<()>;
    /// O VFS já validou ancestralidade, tipos e permissões; `flags` traz NOREPLACE e EXCHANGE.
    fn rename(&self, cx: &Caller, odir: Ino, oname: &[u8], ndir: Ino, nname: &[u8], flags: RenameFlags) -> SysResult<()>;
    fn setattr(&self, cx: &Caller, ino: Ino, attr: &SetAttr) -> SysResult<()>;
    /// Abre um arquivo regular ou diretório (o VFS já checou permissões).
    fn open(self: Arc<Self>, cx: &Caller, ino: Ino, flags: OFlags) -> SysResult<Box<dyn FileHandle>>;
    /// Marca o inode como em uso (fd aberto, cwd): ele não some da tabela enquanto houver uso, mesmo sem
    /// nenhum nome. Sistemas sem órfãos ignoram.
    fn pin(&self, _ino: Ino) {}
    fn unpin(&self, _ino: Ino) {}
    /// `touch_atime` (relatime) depois de `readlink` ou de ler um diretório.
    fn touch_atime(&self, _cx: &Caller, _ino: Ino) {}
    fn as_any(&self) -> &dyn Any;
}

/// Um arquivo ou diretório aberto num sistema de arquivos (a parte "privada" da `struct file`). O
/// deslocamento e as flags de status ficam no kernel, na descrição de arquivo aberto.
pub trait FileHandle: Send + Sync {
    fn read(&self, cx: &Caller, off: u64, buf: &mut [u8]) -> SysResult<usize>;
    /// Devolve os bytes escritos e o deslocamento logo depois deles. Quem chama só move o deslocamento
    /// da descrição de arquivo quando escreveu mais que 0 bytes: no Linux uma escrita de 0 bytes não mexe
    /// no `f_pos`, nem com O_APPEND (o `new_sync_write` só grava o `ki_pos` com retorno positivo).
    fn write(&self, cx: &Caller, pos: WritePos, buf: &[u8]) -> SysResult<(usize, u64)>;
    /// Próximo lote de entradas a partir do cookie (0 = início); devolve o cookie seguinte. Vazio no
    /// fim.
    fn readdir(&self, _cx: &Caller, _cookie: u64, _max: usize) -> SysResult<(Vec<DirEntry>, u64)> {
        Err(Errno::ENOTDIR)
    }
    /// Tamanho usado pelo `SEEK_END`.
    fn size(&self, cx: &Caller) -> SysResult<u64>;
    /// `SEEK_DATA` (`hole == false`) e `SEEK_HOLE`.
    fn seek_data(&self, cx: &Caller, off: u64, hole: bool) -> SysResult<u64> {
        let size = self.size(cx)?;
        if off >= size {
            return Err(Errno::ENXIO);
        }
        Ok(if hole { size } else { off })
    }
    /// `SEEK_END` permitido (o seq_file do procfs recusa com EINVAL).
    fn seek_end_allowed(&self) -> bool {
        true
    }
    /// A `f_op->fallocate` do sistema de arquivos. Quem chama já fez as checagens do `vfs_fallocate`
    /// (modo, `offset`/`len`, abertura pra escrita, tipo do arquivo e `s_maxbytes`). Sem a operação, o
    /// Linux responde EOPNOTSUPP, que é o padrão aqui.
    fn fallocate(&self, _cx: &Caller, _mode: FallocFlags, _offset: u64, _len: u64) -> SysResult<()> {
        Err(Errno::EOPNOTSUPP)
    }
}
