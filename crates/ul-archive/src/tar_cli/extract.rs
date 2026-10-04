//! `-x`: extração sobre o sistema de arquivos do sandbox, com a semântica do GNU tar 1.35 rodando como
//! root: arquivo existente é substituído (diretório vazio no caminho é removido), `-k` e
//! `--skip-old-files` preservam o que existe, permissões exatas (`-p` é o padrão do root), dono e grupo
//! restaurados pelo nome (ou número), mtime restaurado, diretórios com modo e data aplicados no fim
//! (como o GNU adia), diretórios intermediários criados conforme preciso.

use sysabi::{AtFlags, Errno, Fd, FileType, Mode, OFlags, SetTime, TimeSpec};

use super::member::{Kind, Member, Time};
use super::reader::{ReadError, Reader};
use super::transform::{self, Target};
use super::{Flow, R, Tar, names, quote};

/// Atributos a aplicar num diretório no fim.
struct DelayedDir {
    name: Vec<u8>,
    mode: u32,
    uid: Option<u32>,
    gid: Option<u32>,
    mtime: Option<Time>,
    atime: Option<Time>,
}

fn ts(t: Time) -> SetTime {
    SetTime::At(TimeSpec { sec: t.sec, nsec: t.nsec })
}

fn sys() -> std::sync::Arc<dyn sysabi::Syscalls> {
    sysabi::sys::current()
}

/// `‘x’` com as aspas do locale, como o tar cita alvos nas mensagens de link.
fn lq(s: &[u8]) -> Vec<u8> {
    quote::locale(s)
}

struct Extractor {
    delayed: Vec<DelayedDir>,
    umask: Mode,
}

impl Extractor {
    /// Cria os diretórios que faltam até o pai de `name` (modo 0777 menos a umask).
    fn make_parents(&self, name: &[u8]) -> bool {
        let mut made = false;
        let mut i = 0;
        while let Some(p) = name[i..].iter().position(|&c| c == b'/') {
            let end = i + p;
            i = end + 1;
            if end == 0 {
                continue;
            }
            let dir = &name[..end];
            match sys().mkdirat(Fd::CWD, dir, 0o777) {
                Ok(()) => made = true,
                Err(Errno::EEXIST) => {}
                Err(_) => {}
            }
        }
        made
    }
}

/// Dono a restaurar: pelo nome (se existe no sandbox), senão o número do arquivo.
fn owner_ids(t: &mut Tar, m: &Member) -> (Option<u32>, Option<u32>) {
    let same_owner = t.o.same_owner.unwrap_or(sys().geteuid() == 0);
    if !same_owner {
        return (None, None);
    }
    let mut uid = m.uid.max(0) as u32;
    let mut gid = m.gid.max(0) as u32;
    if !t.o.numeric_owner {
        if !m.uname.is_empty()
            && let Some(u) = t.owners.uid_of(&m.uname)
        {
            uid = u;
        }
        if !m.gname.is_empty()
            && let Some(g) = t.owners.gid_of(&m.gname)
        {
            gid = g;
        }
    }
    (Some(uid), Some(gid))
}

/// Modo final de um arquivo extraído.
fn final_mode(t: &Tar, m: &Member, umask: Mode) -> Mode {
    let same_perms = t.o.same_permissions.unwrap_or(sys().geteuid() == 0);
    let mode = m.mode & 0o7777;
    if same_perms { mode } else { mode & !umask & 0o777 }
}

/// O que fazer com o nome do membro antes de extrair: tira componentes, aplica `--transform`, remove o
/// prefixo inseguro. `None` quando não sobra nome.
pub fn target_name(t: &mut Tar, name: &[u8], link: bool, kind_target: Target) -> Option<Vec<u8>> {
    let stripped = names::strip_components(name, t.o.strip_components)?;
    let transformed = transform::apply_all(t, &stripped, kind_target);
    if t.o.absolute_names {
        return Some(transformed);
    }
    let (prefix, rest) = names::unsafe_prefix(&transformed);
    if !prefix.is_empty() {
        t.warn_prefix(&prefix, link);
    }
    if rest.is_empty() { None } else { Some(rest) }
}

pub fn run(t: &mut Tar) -> R<()> {
    let (mut r, child) = super::open_for_read(t)?;
    for dir in t.o.final_chdir.clone() {
        if let Err(e) = sys().chdir(&dir) {
            let mut m = quote::colon(&dir);
            m.extend_from_slice(format!(": Cannot open: {}", e.message()).as_bytes());
            return Err(t.fatal(m));
        }
    }
    if let Some(Some(top)) = t.o.one_top_level.clone() {
        let _ = sys().mkdirat(Fd::CWD, &top, 0o777);
    }
    let umask = {
        let s = sys();
        let old = s.umask(0);
        s.umask(old);
        old
    };
    let mut x = Extractor { delayed: Vec::new(), umask };
    let mut names = names::NameList::new(&t.o.names);
    let starting = t.o.starting_file.clone();
    let mut started = starting.is_none();
    let res = t.read_and(&mut r, &mut |t, r, m| {
        if !started {
            if starting.as_deref().is_some_and(|s| names::name_matches(s, &m.name, Default::default(), true, true)) {
                started = true;
            } else {
                t.skip_member(r, &m)?;
                return Ok(Flow::Continue);
            }
        }
        if !names.is_empty() {
            match names.find(&m.name) {
                Some(i) => {
                    names.items[i].found += 1;
                    if let Some(occ) = t.o.occurrence
                        && names.items[i].found != occ
                    {
                        t.skip_member(r, &m)?;
                        return Ok(Flow::Continue);
                    }
                }
                None => {
                    t.skip_member(r, &m)?;
                    return Ok(Flow::Continue);
                }
            }
        }
        if t.excluder.excluded(&m.name) {
            t.skip_member(r, &m)?;
            return Ok(Flow::Continue);
        }
        extract_member(t, &mut x, r, m)?;
        Ok(Flow::Continue)
    });
    // Os diretórios recebem modo, dono e data no fim, mesmo depois de erro fatal.
    apply_delayed(t, &mut x);
    super::compress::finish_read(t, child, res)?;
    super::read_totals(t, &r);
    super::report_unmatched(t, &names);
    Ok(())
}

fn apply_delayed(t: &mut Tar, x: &mut Extractor) {
    for d in x.delayed.drain(..).rev() {
        let s = sys();
        if d.uid.is_some() || d.gid.is_some() {
            let _ = s.fchownat(Fd::CWD, &d.name, d.uid, d.gid, AtFlags::empty());
        }
        if let Err(e) = s.fchmodat(Fd::CWD, &d.name, d.mode, AtFlags::empty()) {
            t.sys_error(&d.name, "Cannot change mode to", e);
        }
        if let Some(mt) = d.mtime {
            let at = d.atime.map(ts).unwrap_or(SetTime::Now);
            if let Err(e) = s.utimensat(Fd::CWD, &d.name, at, ts(mt), AtFlags::empty()) {
                t.sys_error(&d.name, "Cannot utime", e);
            }
        }
    }
}

/// Lê os dados do membro pro fd (ou descarta), tratando fim inesperado.
fn copy_data(t: &mut Tar, r: &mut Reader, m: &Member, fd: Option<Fd>) -> R<()> {
    let mut write_err: Option<Errno> = None;
    let res = if let (Some(map), Some(fd)) = (m.sparse.clone(), fd) {
        // Arquivo esparso: os trechos de dados vão pros deslocamentos do mapa.
        let mut map_iter = map.into_iter();
        let mut cur = map_iter.next();
        let mut written_in_cur = 0u64;
        let r2 = r.read_data(m.data_size(), |mut chunk| {
            while !chunk.is_empty() {
                let Some((off, len)) = cur else { break };
                let left = len - written_in_cur;
                let n = (left as usize).min(chunk.len());
                if write_err.is_none()
                    && let Err(e) = pwrite_all(fd, &chunk[..n], off + written_in_cur)
                {
                    write_err = Some(e);
                }
                written_in_cur += n as u64;
                chunk = &chunk[n..];
                if written_in_cur == len {
                    cur = map_iter.next();
                    written_in_cur = 0;
                }
            }
        });
        let _ = sys().ftruncate(fd, m.real_size);
        r2
    } else {
        r.read_data(m.data_size(), |chunk| {
            if let Some(fd) = fd
                && write_err.is_none()
                && let Err(e) = sysabi::sys::write_all(fd, chunk)
            {
                write_err = Some(e);
            }
        })
    };
    match res {
        Ok(()) => {}
        Err(ReadError::UnexpectedEof) => {
            if let Some(fd) = fd {
                let _ = sysabi::sys::close(fd);
            }
            return Err(t.fatal(b"Unexpected EOF in archive"));
        }
        Err(ReadError::Io(e)) => {
            let name = t.archive_name();
            let mut msg = quote::colon(&name);
            msg.extend_from_slice(format!(": Read error: {}", e.message()).as_bytes());
            return Err(t.fatal(msg));
        }
    }
    if let Some(e) = write_err {
        t.sys_error(&m.name, "Cannot write", e);
    }
    Ok(())
}

fn pwrite_all(fd: Fd, mut data: &[u8], mut off: u64) -> Result<(), Errno> {
    let s = sys();
    while !data.is_empty() {
        let n = s.pwrite(fd, data, off)?;
        if n == 0 {
            return Err(Errno::EIO);
        }
        data = &data[n..];
        off += n as u64;
    }
    Ok(())
}

/// Remove o que está no caminho (arquivo, link, ou diretório vazio). `true` se removeu.
fn remove_existing(name: &[u8], recursive: bool) -> bool {
    let s = sys();
    match s.fstatat(Fd::CWD, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(st) if st.file_type() == FileType::Directory => {
            if s.unlinkat(Fd::CWD, name, AtFlags::REMOVEDIR).is_ok() {
                return true;
            }
            if recursive {
                return remove_tree(name);
            }
            false
        }
        Ok(_) => s.unlinkat(Fd::CWD, name, AtFlags::empty()).is_ok(),
        Err(_) => false,
    }
}

fn remove_tree(path: &[u8]) -> bool {
    let s = sys();
    if let Ok(entries) = sysabi::sys::read_dir(path) {
        for e in entries {
            let child = crate::sysutil::join(path, &e.name);
            if e.kind == FileType::Directory {
                remove_tree(&child);
            } else {
                let _ = s.unlinkat(Fd::CWD, &child, AtFlags::empty());
            }
        }
    }
    s.unlinkat(Fd::CWD, path, AtFlags::REMOVEDIR).is_ok()
}

/// Decide o que fazer quando já existe algo no caminho. `Ok(true)` = prosseguir (removido ou não
/// havia), `Ok(false)` = pular o membro.
fn handle_existing(t: &mut Tar, name: &[u8], m: &Member, is_dir: bool) -> R<bool> {
    let s = sys();
    let Ok(st) = s.fstatat(Fd::CWD, name, AtFlags::SYMLINK_NOFOLLOW) else { return Ok(true) };
    if is_dir && st.file_type() == FileType::Directory {
        return Ok(true);
    }
    if t.o.skip_old_files {
        if t.warning_enabled(b"existing-file") || t.o.verbose > 0 {
            let mut msg = quote::colon(names::trim_trailing_slashes(&m.name));
            msg.extend_from_slice(b": skipping existing file");
            t.msg(msg);
        }
        return Ok(false);
    }
    if t.o.keep_old_files {
        return Ok(true); // o erro sai na criação (EEXIST)
    }
    if t.o.keep_newer_files && st.file_type() != FileType::Directory {
        let cur = Time::new(st.mtime.sec, st.mtime.nsec);
        if cur >= m.mtime {
            let mut msg = b"Current ".to_vec();
            msg.extend_from_slice(&lq(name));
            msg.extend_from_slice(b" is newer or same age");
            t.msg(msg);
            return Ok(false);
        }
    }
    if st.file_type() != FileType::Directory
        && let Some(backup) = backup_name(t, name)
    {
        // `--backup`: o que existe vira cópia de segurança em vez de ser apagado.
        let _ = s.renameat2(Fd::CWD, name, Fd::CWD, &backup, sysabi::RenameFlags::empty());
        return Ok(true);
    }
    if t.o.unlink_first || !t.o.overwrite || st.file_type() == FileType::Directory || st.file_type() == FileType::Symlink {
        remove_existing(name, t.o.recursive_unlink);
    }
    Ok(true)
}

/// Nome da cópia de segurança (`--backup`, `--suffix`), como o `backupfile` do gnulib: simples
/// (`x~`), numerada (`x.~N~`) ou "existing" (numerada se já há numeradas).
fn backup_name(t: &Tar, name: &[u8]) -> Option<Vec<u8>> {
    let control = match (&t.o.backup, &t.o.suffix) {
        (Some(Some(c)), _) => c.clone(),
        (Some(None), _) | (None, Some(_)) => {
            crate::sysutil::getenv("VERSION_CONTROL").filter(|v| !v.is_empty()).unwrap_or_else(|| b"existing".to_vec())
        }
        (None, None) => return None,
    };
    let suffix = t
        .o
        .suffix
        .clone()
        .or_else(|| crate::sysutil::getenv("SIMPLE_BACKUP_SUFFIX").filter(|v| !v.is_empty()))
        .unwrap_or_else(|| b"~".to_vec());
    let simple = || {
        let mut n = name.to_vec();
        n.extend_from_slice(&suffix);
        n
    };
    // Maior número de cópia numerada que já existe.
    let highest = || -> u64 {
        let (dir, base) = match name.iter().rposition(|&c| c == b'/') {
            Some(p) => (name[..p].to_vec(), name[p + 1..].to_vec()),
            None => (b".".to_vec(), name.to_vec()),
        };
        let mut best = 0;
        if let Ok(entries) = sysabi::sys::read_dir(&dir) {
            for e in entries {
                let mut prefix = base.clone();
                prefix.extend_from_slice(b".~");
                if let Some(rest) = e.name.strip_prefix(prefix.as_slice())
                    && let Some(num) = rest.strip_suffix(b"~")
                    && let Some(n) = std::str::from_utf8(num).ok().and_then(|s| s.parse::<u64>().ok())
                {
                    best = best.max(n);
                }
            }
        }
        best
    };
    let numbered = |n: u64| {
        let mut v = name.to_vec();
        v.extend_from_slice(format!(".~{n}~").as_bytes());
        v
    };
    match control.as_slice() {
        b"none" | b"off" => None,
        b"simple" | b"never" => Some(simple()),
        b"numbered" | b"t" => Some(numbered(highest() + 1)),
        _ => {
            let h = highest();
            if h > 0 { Some(numbered(h + 1)) } else { Some(simple()) }
        }
    }
}

fn extract_member(t: &mut Tar, x: &mut Extractor, r: &mut Reader, m: Member) -> R<()> {
    let kind = m.kind();
    let Some(name) = target_name(t, &m.name, false, Target::Name) else {
        return t.skip_member(r, &m);
    };
    let name = match t.o.one_top_level.clone() {
        Some(top) => {
            let top = top.unwrap_or_else(|| default_top_level(t));
            if name.starts_with(&top) && (name.len() == top.len() || name[top.len()] == b'/') {
                name
            } else {
                crate::sysutil::join(&top, &name)
            }
        }
        None => name,
    };
    // Listagem verbosa: o nome do arquivo.
    if t.o.verbose > 0 {
        let mut line = t.block_prefix(m.main_block);
        let shown = transform::display_name(t, &m.name);
        if t.o.verbose > 1 {
            let q = t.o.quoting.clone();
            line.extend_from_slice(&t.lister.line(&m, &shown, &q));
        } else {
            line.extend_from_slice(&quote::quote_with(&shown, &t.o.quoting, false));
        }
        t.stdlis(&line);
    }
    if t.o.to_stdout {
        if matches!(kind, Kind::Regular | Kind::Contiguous) {
            t.out.flush();
            return copy_data(t, r, &m, Some(Fd::STDOUT));
        }
        return t.skip_member(r, &m);
    }
    let fname = names::trim_trailing_slashes(&name).to_vec();
    let umask = x.umask;
    match kind {
        Kind::Directory => {
            t.skip_member(r, &m)?;
            if !handle_existing(t, &fname, &m, true)? {
                return Ok(());
            }
            let s = sys();
            let mut created = s.mkdirat(Fd::CWD, &fname, 0o700 | (m.mode & 0o777));
            if created == Err(Errno::ENOENT) {
                x.make_parents(&fname);
                created = s.mkdirat(Fd::CWD, &fname, 0o700 | (m.mode & 0o777));
            }
            match created {
                Ok(()) | Err(Errno::EEXIST) => {}
                Err(e) => {
                    t.sys_error(&fname, "Cannot mkdir", e);
                    return Ok(());
                }
            }
            if t.o.overwrite_dir == Some(false) && created == Err(Errno::EEXIST) {
                return Ok(());
            }
            let (uid, gid) = owner_ids(t, &m);
            x.delayed.push(DelayedDir {
                name: fname,
                mode: final_mode(t, &m, umask),
                uid,
                gid,
                mtime: if t.o.touch { None } else { Some(m.mtime) },
                atime: m.atime,
            });
            Ok(())
        }
        Kind::Symlink => {
            t.skip_member(r, &m)?;
            if !handle_existing(t, &fname, &m, false)? {
                return Ok(());
            }
            let target = transform::apply_all(t, &m.linkname, Target::Symlink);
            let s = sys();
            let mut res = s.symlinkat(&target, Fd::CWD, &fname);
            if res == Err(Errno::ENOENT) && x.make_parents(&fname) {
                res = s.symlinkat(&target, Fd::CWD, &fname);
            }
            match res {
                Ok(()) => {
                    let (uid, gid) = owner_ids(t, &m);
                    if uid.is_some() {
                        let _ = s.fchownat(Fd::CWD, &fname, uid, gid, AtFlags::SYMLINK_NOFOLLOW);
                    }
                    if !t.o.touch {
                        let at = m.atime.map(ts).unwrap_or(SetTime::Now);
                        let _ = s.utimensat(Fd::CWD, &fname, at, ts(m.mtime), AtFlags::SYMLINK_NOFOLLOW);
                    }
                }
                Err(e) => {
                    let mut msg = quote::colon(&fname);
                    msg.extend_from_slice(b": Cannot create symlink to ");
                    msg.extend_from_slice(&lq(&target));
                    msg.extend_from_slice(format!(": {}", e.message()).as_bytes());
                    t.error(msg);
                }
            }
            Ok(())
        }
        Kind::HardLink => {
            t.skip_member(r, &m)?;
            let Some(target) = target_name(t, &m.linkname, true, Target::Hardlink) else { return Ok(()) };
            let s = sys();
            // Já é o mesmo arquivo: nada a fazer.
            if let (Ok(a), Ok(b)) = (
                s.fstatat(Fd::CWD, &fname, AtFlags::SYMLINK_NOFOLLOW),
                s.fstatat(Fd::CWD, &target, AtFlags::SYMLINK_NOFOLLOW),
            ) && a.ino == b.ino
                && a.dev == b.dev
            {
                return Ok(());
            }
            if !handle_existing(t, &fname, &m, false)? {
                return Ok(());
            }
            let mut res = s.linkat(Fd::CWD, &target, Fd::CWD, &fname, AtFlags::empty());
            if res == Err(Errno::ENOENT) && x.make_parents(&fname) {
                res = s.linkat(Fd::CWD, &target, Fd::CWD, &fname, AtFlags::empty());
            }
            if let Err(e) = res {
                let mut msg = quote::colon(&fname);
                msg.extend_from_slice(b": Cannot hard link to ");
                msg.extend_from_slice(&lq(&target));
                msg.extend_from_slice(format!(": {}", e.message()).as_bytes());
                t.error(msg);
            }
            Ok(())
        }
        Kind::CharDev | Kind::BlockDev | Kind::Fifo => {
            t.skip_member(r, &m)?;
            if !handle_existing(t, &fname, &m, false)? {
                return Ok(());
            }
            let ftype = match kind {
                Kind::CharDev => sysabi::mode::S_IFCHR,
                Kind::BlockDev => sysabi::mode::S_IFBLK,
                _ => sysabi::mode::S_IFIFO,
            };
            let dev = ((m.devmajor as u64) << 8) | (m.devminor as u64 & 0xff) | (((m.devminor as u64) & !0xff) << 12);
            let s = sys();
            let mode = final_mode(t, &m, umask);
            let mut res = s.mknodat(Fd::CWD, &fname, ftype | (mode & 0o777), dev);
            if res == Err(Errno::ENOENT) && x.make_parents(&fname) {
                res = s.mknodat(Fd::CWD, &fname, ftype | (mode & 0o777), dev);
            }
            match res {
                Ok(()) => set_attrs_path(t, &fname, &m, umask),
                Err(e) => t.sys_error(&fname, "Cannot mknod", e),
            }
            Ok(())
        }
        Kind::Volume | Kind::Multivolume => t.skip_member(r, &m),
        Kind::Regular | Kind::Contiguous | Kind::Other(_) => {
            if let Kind::Other(c) = kind {
                let mut msg = quote::colon(&fname);
                msg.extend_from_slice(format!(": Unknown file type '{}', extracted as normal file", c as char).as_bytes());
                t.msg(msg);
            }
            if !handle_existing(t, &fname, &m, false)? {
                return t.skip_member(r, &m);
            }
            let flags = OFlags::WRONLY
                | OFlags::CREAT
                | if t.o.overwrite && !t.o.keep_old_files { OFlags::TRUNC } else { OFlags::EXCL };
            let mode = final_mode(t, &m, umask);
            let mut fd = super::open(&fname, flags | OFlags::NOFOLLOW, mode & 0o777);
            if fd == Err(Errno::ENOENT) && x.make_parents(&fname) {
                fd = super::open(&fname, flags | OFlags::NOFOLLOW, mode & 0o777);
            }
            let fd = match fd {
                Ok(fd) => fd,
                Err(e) => {
                    t.sys_error(&fname, "Cannot open", e);
                    return t.skip_member(r, &m);
                }
            };
            let res = copy_data(t, r, &m, Some(fd));
            if res.is_ok() {
                set_attrs_fd(t, fd, &fname, &m, umask);
            }
            let _ = sysabi::sys::close(fd);
            res
        }
    }
}

fn default_top_level(t: &Tar) -> Vec<u8> {
    let name = t.archive_name();
    let base = crate::sysutil::basename(&name).to_vec();
    for suffix in [&b".tar.gz"[..], b".tgz", b".tar.bz2", b".tbz2", b".tar.xz", b".txz", b".tar.zst", b".tar.lz", b".tar.lzma", b".tar"] {
        if let Some(stem) = base.strip_suffix(suffix) {
            return stem.to_vec();
        }
    }
    base
}

fn set_attrs_fd(t: &mut Tar, fd: Fd, name: &[u8], m: &Member, umask: Mode) {
    let s = sys();
    let (uid, gid) = owner_ids(t, m);
    if uid.is_some() {
        let _ = s.fchownat(fd, b"", uid, gid, AtFlags::EMPTY_PATH).or_else(|_| s.fchownat(Fd::CWD, name, uid, gid, AtFlags::empty()));
    }
    let mode = final_mode(t, m, umask);
    if let Err(e) = s.fchmod(fd, mode) {
        t.sys_error(name, "Cannot change mode to", e);
    }
    if !t.o.touch {
        let at = m.atime.map(ts).unwrap_or(SetTime::Now);
        if let Err(e) = s.futimens(fd, at, ts(m.mtime)) {
            t.sys_error(name, "Cannot utime", e);
        }
    }
}

fn set_attrs_path(t: &mut Tar, name: &[u8], m: &Member, umask: Mode) {
    let s = sys();
    let (uid, gid) = owner_ids(t, m);
    if uid.is_some() {
        let _ = s.fchownat(Fd::CWD, name, uid, gid, AtFlags::empty());
    }
    let _ = s.fchmodat(Fd::CWD, name, final_mode(t, m, umask), AtFlags::empty());
    if !t.o.touch {
        let at = m.atime.map(ts).unwrap_or(SetTime::Now);
        let _ = s.utimensat(Fd::CWD, name, at, ts(m.mtime), AtFlags::empty());
    }
}
