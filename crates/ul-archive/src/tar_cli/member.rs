//! Metadados de um membro do arquivo, independentes do formato em que foi gravado.

use super::header::kind;

/// Instante com nanossegundos.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Time {
    pub sec: i64,
    pub nsec: u32,
}

impl Time {
    pub fn new(sec: i64, nsec: u32) -> Time {
        Time { sec, nsec }
    }
}

/// Um membro do arquivo.
#[derive(Clone, Debug, Default)]
pub struct Member {
    /// Nome como está no arquivo (depois de juntar prefixo, nome longo do GNU e `path` do pax).
    pub name: Vec<u8>,
    pub linkname: Vec<u8>,
    pub typeflag: u8,
    /// Bits do campo mode (normalmente só as permissões e os bits especiais).
    pub mode: u32,
    pub uid: i64,
    pub gid: i64,
    pub uname: Vec<u8>,
    pub gname: Vec<u8>,
    /// Tamanho dos dados gravados depois do cabeçalho.
    pub size: u64,
    /// Tamanho real do arquivo (difere de `size` em arquivo esparso).
    pub real_size: u64,
    pub mtime: Time,
    pub atime: Option<Time>,
    pub ctime: Option<Time>,
    pub devmajor: u32,
    pub devminor: u32,
    /// Mapa de um arquivo esparso: (deslocamento, tamanho) de cada trecho com dados.
    pub sparse: Option<Vec<(u64, u64)>>,
    /// Bloco (ordinal) do primeiro cabeçalho do membro, inclusive cabeçalhos estendidos.
    pub header_block: u64,
    /// Bloco do cabeçalho principal (o que o `-R` informa).
    pub main_block: u64,
    /// Deslocamento, em bytes, do primeiro cabeçalho do membro (pra `--delete` e `-A` copiarem o
    /// membro inteiro).
    pub start_offset: u64,
    /// Formato detectado pela magia do cabeçalho principal.
    pub magic: super::header::Magic,
}

/// Tipo lógico do membro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Regular,
    HardLink,
    Symlink,
    CharDev,
    BlockDev,
    Directory,
    Fifo,
    Contiguous,
    Volume,
    Multivolume,
    Other(u8),
}

impl Member {
    pub fn kind(&self) -> Kind {
        match self.typeflag {
            kind::REG | kind::AREG | kind::GNU_SPARSE => {
                if self.name.ends_with(b"/") {
                    Kind::Directory
                } else {
                    Kind::Regular
                }
            }
            kind::LNK => Kind::HardLink,
            kind::SYM => Kind::Symlink,
            kind::CHR => Kind::CharDev,
            kind::BLK => Kind::BlockDev,
            kind::DIR | kind::GNU_DUMPDIR => Kind::Directory,
            kind::FIFO => Kind::Fifo,
            kind::CONT => Kind::Contiguous,
            kind::GNU_VOLHDR => Kind::Volume,
            kind::GNU_MULTIVOL => Kind::Multivolume,
            other => Kind::Other(other),
        }
    }

    /// Bytes de dados que seguem o cabeçalho no arquivo. Links e dispositivos não têm dados mesmo que o
    /// campo `size` diga o contrário (o leitor ignora o campo nesses tipos).
    pub fn data_size(&self) -> u64 {
        match self.kind() {
            Kind::HardLink | Kind::Symlink | Kind::CharDev | Kind::BlockDev | Kind::Fifo => 0,
            _ => self.size,
        }
    }

    /// Blocos de dados que seguem o cabeçalho.
    pub fn data_blocks(&self) -> u64 {
        self.data_size().div_ceil(512)
    }

    pub fn is_sparse(&self) -> bool {
        self.sparse.is_some()
    }
}
