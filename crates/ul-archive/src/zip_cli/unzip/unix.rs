//! A parte Unix da extração (unix/unix.c): atributos do arquivo de saída, mapeamento do nome do
//! membro pra caminho, criação dos diretórios do caminho, fechamento com permissões e datas, e os
//! diretórios e links simbólicos cujos atributos só se aplicam no fim.

use sysabi::{AtFlags, Errno, Fd, SetTime, TimeSpec};

use super::fileio::{fnfilter, FILNAMSIZ};
use super::{Uz, PK_OK, PK_WARN};

/// Retornos do `mapname`/`checkdir` (byte alto; o baixo leva um código PK).
pub const MPN_OK: i32 = 0;
pub const MPN_INF_TRUNC: i32 = 1 << 8;
pub const MPN_INF_SKIP: i32 = 2 << 8;
pub const MPN_ERR_SKIP: i32 = 3 << 8;
pub const MPN_ERR_TOOLONG: i32 = 4 << 8;
pub const MPN_NOMEM: i32 = 10 << 8;
pub const MPN_CREATED_DIR: i32 = 16 << 8;
pub const MPN_VOL_LABEL: i32 = 17 << 8;
pub const MPN_MASK: i32 = 0x7F00;

const S_ISUID: u32 = 0o4000;
const S_ISGID: u32 = 0o2000;
const S_ISVTX: u32 = 0o1000;
const S_IFMT: u32 = 0o170000;
const S_IFDIR: u32 = 0o040000;

/// Um diretório criado na extração, com o que aplicar nele no fim (`uxdirattr`).
pub struct DirAttr {
    pub fname: Vec<u8>,
    pub perms: u32,
    /// UID e GID a restaurar (`-X` com o bloco do campo extra).
    pub uidgid: Option<(u64, u64)>,
    pub atime: i64,
    pub mtime: i64,
}

/// Um link simbólico adiado (`slinkentry`): criado só depois de todos os membros, pra que um
/// membro posterior não seja escrito através dele.
pub struct Slink {
    pub fname: Vec<u8>,
    pub target: Vec<u8>,
    pub perms: u32,
    pub uidgid: Option<(u64, u64)>,
}

/// O texto do `strerror`.
pub fn strerror(e: Errno) -> String {
    e.message()
}

/// `perror(msg)`: direto no stderr, fora do controle de começo de linha do `Info`.
pub fn perror(msg: &str, e: Errno) {
    let _ = sysabi::sys::write_all(Fd::STDERR, format!("{msg}: {}\n", strerror(e)).as_bytes());
}

impl Uz {
    /// Tira setuid, setgid e sticky, a menos que `-K` peça pra manter (`filtattr`).
    pub fn filtattr(&self, perms: u32) -> u32 {
        let p = if self.o.k_flag { perms } else { perms & !(S_ISUID | S_ISGID | S_ISVTX) };
        p & 0xffff
    }

    /// A data DOS como instante, lida no fuso local (`dos_to_unix_time`, pelo `mktime`).
    pub fn dos_to_unix_time(&self, dos: u32) -> i64 {
        let f = |shift: u32, mask: u32| i64::from(dos >> shift & mask);
        crate::tz::mktime(f(25, 0x7f) + 1980, f(21, 0x0f) - 1, f(16, 0x1f), f(11, 0x1f), f(5, 0x3f), i64::from(dos << 1 & 0x3e), &self.tz)
    }

    /// Datas de modificação e acesso do membro (do bloco "UT"/"UX" do campo extra local, senão a
    /// data DOS) e o UID/GID a restaurar quando `-X` foi pedido (`get_extattribs`).
    pub fn get_extattribs(&self) -> (i64, i64, Option<(u64, u64)>) {
        use super::process::{ef_scan_for_izux, EB_UT_FL_ATIME, EB_UT_FL_MTIME, EB_UX2_VALID};
        let dos = self.lrec.last_mod_dos_datetime;
        let (flags, t, ids) = match &self.extra_field {
            Some(ef) => {
                let ef = &ef[..ef.len().min(usize::from(self.lrec.extra_field_length))];
                ef_scan_for_izux(ef, false, dos, true, true)
            }
            None => (0, Default::default(), [0, 0]),
        };
        let mtime = if flags & EB_UT_FL_MTIME != 0 { t.mtime } else { self.dos_to_unix_time(dos) };
        let atime = if flags & EB_UT_FL_ATIME != 0 { t.atime } else { mtime };
        let ids = (self.o.x_flag != 0 && flags & EB_UX2_VALID != 0).then_some((ids[0], ids[1]));
        (mtime, atime, ids)
    }
}

impl Uz {
    /// `checkdir(path, ROOT)`: guarda o diretório de extração com a barra final, criando um nível
    /// se não existir e for permitido criar.
    pub fn checkdir_root(&mut self, pathcomp: &[u8]) -> i32 {
        if !self.x.rootpath.is_empty() {
            return MPN_OK;
        }
        let mut root = pathcomp.to_vec();
        if root.is_empty() {
            return MPN_OK;
        }
        if root.last() == Some(&b'/') {
            root.pop();
        }
        if !root.is_empty() && !stat_mode(&root).is_ok_and(is_dir_mode) {
            if !self.x.create_dirs {
                return MPN_INF_SKIP;
            }
            if let Err(e) = super::sys().mkdirat(Fd::CWD, &root, 0o777) {
                let mut m = b"checkdir:  cannot create extraction directory: ".to_vec();
                m.extend(fnfilter(&root));
                m.extend_from_slice(format!("\n           {}\n", strerror(e)).as_bytes());
                self.info(0x1, m);
                return MPN_ERR_SKIP;
            }
        }
        root.push(b'/');
        self.x.rootpath = root;
        MPN_OK
    }

    /// `checkdir(NULL, INIT)`: começa o caminho do membro pelo diretório de extração, a menos que o
    /// nome novo seja absoluto.
    fn checkdir_init(&mut self) {
        self.x.buildpath = if self.x.renamed_fullpath { Vec::new() } else { self.x.rootpath.clone() };
    }

    /// `checkdir(comp, APPEND_DIR)`: acrescenta um diretório ao caminho e garante que ele existe,
    /// criando quando permitido.
    fn checkdir_append_dir(&mut self, comp: &[u8]) -> i32 {
        self.x.buildpath.extend_from_slice(comp);
        let too_long = self.x.buildpath.len() > FILNAMSIZ - 3;
        let path = self.x.buildpath.clone();
        match stat_mode(&path) {
            Err(_) => {
                if !self.x.create_dirs {
                    return MPN_INF_SKIP;
                }
                if too_long {
                    self.path_too_long(&path);
                    return MPN_ERR_TOOLONG;
                }
                if let Err(e) = super::sys().mkdirat(Fd::CWD, &path, 0o777) {
                    let mut m = b"checkdir error:  cannot create ".to_vec();
                    m.extend(fnfilter(&path));
                    m.extend_from_slice(format!("\n                 {}\n                 unable to process ", strerror(e)).as_bytes());
                    m.extend(fnfilter(&self.filename));
                    m.extend_from_slice(b".\n");
                    self.info(0x1, m);
                    return MPN_ERR_SKIP;
                }
                self.x.created_dir = true;
            }
            Ok(mode) if !is_dir_mode(mode) => {
                let mut m = b"checkdir error:  ".to_vec();
                m.extend(fnfilter(&path));
                m.extend_from_slice(b" exists but is not directory\n                 unable to process ");
                m.extend(fnfilter(&self.filename));
                m.extend_from_slice(b".\n");
                self.info(0x1, m);
                return MPN_ERR_SKIP;
            }
            Ok(_) => {}
        }
        if too_long {
            self.path_too_long(&path);
            return MPN_ERR_TOOLONG;
        }
        self.x.buildpath.push(b'/');
        MPN_OK
    }

    fn path_too_long(&mut self, path: &[u8]) {
        let mut m = b"checkdir error:  path too long: ".to_vec();
        m.extend(fnfilter(path));
        m.push(b'\n');
        self.info(0x1, m);
    }

    /// `checkdir(name, APPEND_NAME)`: acrescenta o nome do arquivo sem checar existência, cortando
    /// no limite de `FILNAMSIZ`.
    fn checkdir_append_name(&mut self, name: &[u8]) -> i32 {
        for &c in name {
            self.x.buildpath.push(c);
            if self.x.buildpath.len() >= FILNAMSIZ {
                self.x.buildpath.pop();
                let mut m = b"checkdir warning:  path too long; truncating\n                   ".to_vec();
                m.extend(fnfilter(&self.filename));
                m.extend_from_slice(b"\n                -> ");
                m.extend(fnfilter(&self.x.buildpath));
                m.push(b'\n');
                self.info(0x201, m);
                return MPN_INF_TRUNC;
            }
        }
        MPN_OK
    }

    /// `checkdir(filename, GETPATH)`: o caminho montado vira o nome do membro.
    fn checkdir_getpath(&mut self) {
        self.filename = std::mem::take(&mut self.x.buildpath);
    }

    /// Transforma o nome do membro no caminho de saída (`mapname`): tira `./`, recusa `../` (a menos
    /// de `-:`), descarta caracteres de controle (a menos de `-^`), cria os diretórios do caminho e
    /// trata o membro que é só um diretório. O retorno junta um `MPN_*` e um código PK.
    pub fn mapname(&mut self, renamed: bool) -> i32 {
        if self.pinfo.vollabel {
            return MPN_VOL_LABEL;
        }
        self.x.create_dirs = !self.o.fflag || renamed;
        self.x.created_dir = false;
        self.x.renamed_fullpath = renamed && self.filename.first() == Some(&b'/');
        self.checkdir_init();
        let name = self.filename.clone();
        let start = if self.o.jflag { name.iter().rposition(|&c| c == b'/').map_or(0, |i| i + 1) } else { 0 };
        let mut error = MPN_OK;
        let mut comp: Vec<u8> = Vec::new();
        let mut lastsemi: Option<usize> = None;
        let mut killed_ddot = false;
        for &c in &name[start..] {
            match c {
                b'/' => {
                    if comp == b"." {
                        comp.clear();
                    } else if self.o.ddotflag == 0 && comp == b".." {
                        comp.clear();
                        killed_ddot = true;
                    }
                    if !comp.is_empty() {
                        error = self.checkdir_append_dir(&comp);
                        if error & MPN_MASK > MPN_INF_TRUNC {
                            return error;
                        }
                    }
                    comp.clear();
                    lastsemi = None;
                }
                b';' => {
                    lastsemi = Some(comp.len());
                    comp.push(c);
                }
                _ => {
                    if self.o.cflxflag != 0 || (0x20..0x7f).contains(&c) || (128..=254).contains(&c) {
                        comp.push(c);
                    }
                }
            }
        }
        if killed_ddot && self.o.qflag == 0 {
            let mut m = b"warning:  skipped \"../\" path component(s) in ".to_vec();
            m.extend(fnfilter(&self.filename));
            m.push(b'\n');
            self.info(0, m);
            if error & !MPN_MASK == 0 {
                error = (error & MPN_MASK) | PK_WARN;
            }
        }
        if self.filename.last() == Some(&b'/') {
            self.checkdir_getpath();
            if !self.x.created_dir {
                return (error & !MPN_MASK) | MPN_INF_SKIP;
            }
            self.created_dir_attribs();
            return (error & !MPN_MASK) | MPN_CREATED_DIR;
        }
        // Sem `-V`, um ";123" no fim (versão do VMS) sai.
        if !self.o.v_flag
            && let Some(i) = lastsemi
                && comp[i + 1..].iter().all(u8::is_ascii_digit) {
                    comp.truncate(i);
                }
        if comp == b"." {
            comp = b"_".to_vec();
        } else if comp == b".." {
            comp = b"__".to_vec();
        }
        if comp.is_empty() {
            let mut m = b"mapname:  conversion of ".to_vec();
            m.extend(fnfilter(&self.filename));
            m.extend_from_slice(b" failed\n");
            self.info(0x1, m);
            return (error & !MPN_MASK) | MPN_ERR_SKIP;
        }
        self.checkdir_append_name(&comp);
        self.checkdir_getpath();
        error
    }

    /// O diretório do membro acabou de ser criado: anuncia e já põe permissões aproximadas
    /// (garantindo leitura e escrita pro dono), preservando o setgid herdado do pai.
    fn created_dir_attribs(&mut self) {
        use super::process::UNIX;
        if self.o.qflag == 0 {
            let mut m = b"   creating: ".to_vec();
            m.extend(fnfilter(&self.filename));
            m.push(b'\n');
            self.info(0, m);
        }
        self.pinfo.file_attr = self.filtattr(self.pinfo.file_attr);
        if self.pinfo.hostnum != UNIX || !(self.o.x_flag != 0 || self.o.k_flag) {
            match stat_mode(&self.filename) {
                Ok(mode) => self.pinfo.file_attr |= mode & S_ISGID,
                Err(e) => perror("Could not read directory attributes", e),
            }
        }
        if let Err(e) = super::sys().fchmodat(Fd::CWD, &self.filename, self.pinfo.file_attr | 0o700, AtFlags::empty()) {
            perror("chmod (directory attributes) error", e);
        }
    }
}

impl Uz {
    /// `"warning:  cannot set UID u and/or GID g for NOME\n          erro\n"`.
    fn warn_uidgid(&mut self, (uid, gid): (u64, u64), name: &[u8], e: Errno) {
        let mut m = format!("warning:  cannot set UID {uid} and/or GID {gid} for ").into_bytes();
        m.extend(fnfilter(name));
        m.extend_from_slice(format!("\n          {}\n", strerror(e)).as_bytes());
        self.info(0x201, m);
    }

    /// Troca o arquivo provisório pelo link de verdade (`set_deferred_symlink`), conferindo antes
    /// que o provisório ainda tem exatamente o alvo gravado.
    pub fn set_deferred_symlink(&mut self, s: &Slink) {
        let target_c = &s.target[..s.target.iter().position(|&c| c == 0).unwrap_or(s.target.len())];
        let valid = match sysabi::sys::read_file(&s.fname) {
            Ok(data) => data.len() == s.target.len() && data[..data.iter().position(|&c| c == 0).unwrap_or(data.len())] == *target_c,
            Err(_) => false,
        };
        if !valid {
            let mut m = b"warning:  deferred symlink (".to_vec();
            m.extend(fnfilter(&s.fname));
            m.extend_from_slice(b") failed:\n          invalid placeholder file\n");
            self.info(0x201, m);
            return;
        }
        let sys = super::sys();
        let _ = sys.unlinkat(Fd::CWD, &s.fname, AtFlags::empty());
        if self.o.qflag == 0 {
            let mut m = b"  ".to_vec();
            m.extend(super::extract::pad22(&fnfilter(&s.fname)));
            m.extend_from_slice(b" -> ");
            m.extend(fnfilter(target_c));
            m.push(b'\n');
            self.info(0, m);
        }
        if let Err(e) = sys.symlinkat(target_c, Fd::CWD, &s.fname) {
            perror("symlink error", e);
        }
        if let Some(ids) = s.uidgid
            && ids.0 <= u64::from(u32::MAX) && ids.1 <= u64::from(u32::MAX)
                && let Err(e) = sys.fchownat(Fd::CWD, &s.fname, Some(ids.0 as u32), Some(ids.1 as u32), AtFlags::SYMLINK_NOFOLLOW) {
                    self.warn_uidgid(ids, &s.fname, e);
                }
    }

    /// Dono, datas e permissões finais de um diretório criado (`set_direc_attribs`).
    pub fn set_direc_attribs(&mut self, d: &DirAttr) -> i32 {
        let sys = super::sys();
        let mut errval = PK_OK;
        if let Some(ids) = d.uidgid
            && ids.0 <= u64::from(u32::MAX) && ids.1 <= u64::from(u32::MAX)
                && let Err(e) = sys.fchownat(Fd::CWD, &d.fname, Some(ids.0 as u32), Some(ids.1 as u32), AtFlags::empty()) {
                    self.warn_uidgid(ids, &d.fname, e);
                    errval = PK_WARN;
                }
        if self.o.d_flag <= 0
            && let Err(e) = set_times(&d.fname, d.atime, d.mtime, AtFlags::empty()) {
                let mut m = b"warning:  cannot set modif./access times for ".to_vec();
                m.extend(fnfilter(&d.fname));
                m.extend_from_slice(format!("\n          {}\n", strerror(e)).as_bytes());
                self.info(0x201, m);
                if errval == PK_OK {
                    errval = PK_WARN;
                }
            }
        if let Err(e) = sys.fchmodat(Fd::CWD, &d.fname, d.perms, AtFlags::empty()) {
            let mut m = b"warning:  cannot set permissions for ".to_vec();
            m.extend(fnfilter(&d.fname));
            m.extend_from_slice(format!("\n          {}\n", strerror(e)).as_bytes());
            self.info(0x201, m);
            if errval == PK_OK {
                errval = PK_WARN;
            }
        }
        errval
    }

    /// Guarda o que aplicar no diretório recém-criado quando a extração acabar (`defer_dir_attribs`).
    pub fn defer_dir_attribs(&self) -> DirAttr {
        let (mtime, atime, uidgid) = self.get_extattribs();
        DirAttr { fname: self.filename.clone(), perms: self.pinfo.file_attr, uidgid, atime, mtime }
    }
}

/// `utime(path, {atime, mtime})`.
pub fn set_times(path: &[u8], atime: i64, mtime: i64, flags: AtFlags) -> Result<(), Errno> {
    let at = |s: i64| SetTime::At(TimeSpec { sec: s, nsec: 0 });
    super::sys().utimensat(Fd::CWD, path, at(atime), at(mtime), flags)
}

/// `stat` que devolve o modo, ou o erro.
pub fn stat_mode(path: &[u8]) -> Result<u32, Errno> {
    sysabi::sys::stat(path).map(|s| s.mode)
}

/// O modo é de diretório.
pub fn is_dir_mode(mode: u32) -> bool {
    mode & S_IFMT == S_IFDIR
}
