//! `-c` (criação), `-r` (acréscimo), `-u` (atualização), `-A` (concatenação) e a leitura do `-T`.
//!
//! A árvore é percorrida como o GNU tar: o diretório antes do conteúdo, entradas na ordem do `getdents`
//! (ou por nome/inode com `--sort`), links físicos viram membros `link to` a partir da segunda vez que o
//! mesmo inode aparece, o próprio arquivo de saída é pulado ("archive cannot contain itself").

use std::collections::HashMap;

use sysabi::{AtFlags, Errno, Fd, FileType, OFlags, Stat};

use super::args::{NameArg, SortOrder};
use super::header::{Format, kind};
use super::member::{Member, Time};
use super::modespec::ModeSpec;
use super::names::{self, Excluder};
use super::reader::{Reader, Source, Status};
use super::transform::{self, Target};
use super::writer::{HeaderError, Sink, Writer};
use super::{Fatal, R, Tar, compress, date, quote};

const MAX_DEPTH: usize = 4096;
const CACHEDIR_SIGNATURE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55";

fn sys() -> std::sync::Arc<dyn sysabi::Syscalls> {
    sysabi::sys::current()
}

/// Desfaz os escapes de um nome lido do `-T` (`\\`, `\n`, `\t`, octal).
fn unquote(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'\\' && i + 1 < s.len() {
            let c = s[i + 1];
            i += 2;
            match c {
                b'\\' => out.push(b'\\'),
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                b'r' => out.push(b'\r'),
                b'a' => out.push(0x07),
                b'b' => out.push(0x08),
                b'f' => out.push(0x0c),
                b'v' => out.push(0x0b),
                b'0'..=b'7' => {
                    let mut v = (c - b'0') as u32;
                    let mut k = 0;
                    while k < 2 && i < s.len() && (b'0'..=b'7').contains(&s[i]) {
                        v = v * 8 + (s[i] - b'0') as u32;
                        i += 1;
                        k += 1;
                    }
                    out.push(v as u8);
                }
                other => {
                    out.push(b'\\');
                    out.push(other);
                }
            }
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

/// Expande os `-T ARQUIVO` no lugar em que apareceram.
pub fn load_files_from(t: &mut Tar) -> R<()> {
    if !t.o.names.iter().any(|n| n.list_file) {
        return Ok(());
    }
    let mut out: Vec<NameArg> = Vec::new();
    for n in std::mem::take(&mut t.o.names) {
        if !n.list_file {
            out.push(n);
            continue;
        }
        let data = match crate::sysutil::read_path(&n.name) {
            Ok(d) => d,
            Err(e) => {
                let mut m = quote::colon(&n.name);
                m.extend_from_slice(format!(": Cannot open: {}", e.message()).as_bytes());
                return Err(t.fatal(m));
            }
        };
        let sep = if t.o.null { 0u8 } else { b'\n' };
        let mut chdir = n.chdir.clone();
        for line in data.split(|&c| c == sep) {
            let line = if !t.o.null { line.strip_suffix(b"\r").unwrap_or(line) } else { line };
            if line.is_empty() {
                continue;
            }
            if !t.o.null && !t.o.verbatim_files_from && line.first() == Some(&b'-') {
                if let Some(dir) = super::args::files_from_option(line) {
                    chdir.push(dir);
                    continue;
                }
            }
            let name = if t.o.unquote && !t.o.null { unquote(line) } else { line.to_vec() };
            out.push(NameArg {
                name,
                chdir: chdir.clone(),
                flags: n.flags,
                recursion: n.recursion,
                from_file: true,
                list_file: false,
            });
        }
    }
    t.o.names = out;
    Ok(())
}

/// Monta as exclusões da linha de comando (lê os arquivos de `-X`).
pub fn build_excluder(t: &mut Tar) -> R<Excluder> {
    let mut excluder = Excluder { patterns: t.o.excludes.clone(), vcs: t.o.exclude_vcs, backups: t.o.exclude_backups };
    for (file, flags) in t.o.exclude_from.clone() {
        match crate::sysutil::read_path(&file) {
            Ok(data) => {
                for line in data.split(|&c| c == b'\n') {
                    if !line.is_empty() {
                        excluder.patterns.push(super::args::Exclude { pattern: line.to_vec(), flags });
                    }
                }
            }
            Err(e) => {
                let mut m = quote::colon(&file);
                m.extend_from_slice(format!(": Cannot open: {}", e.message()).as_bytes());
                return Err(t.fatal(m));
            }
        }
    }
    Ok(excluder)
}

/// Dono/grupo fixos (`--owner`, `--group`): nome guardado e número.
#[derive(Clone)]
struct IdOverride {
    name: Vec<u8>,
    id: Option<u32>,
}

fn parse_id_override(t: &mut Tar, spec: &[u8], user: bool) -> IdOverride {
    if let Some(p) = spec.iter().position(|&c| c == b':') {
        let id = std::str::from_utf8(&spec[p + 1..]).ok().and_then(|s| s.parse().ok());
        return IdOverride { name: spec[..p].to_vec(), id };
    }
    let found = if user { t.owners.uid_of(spec) } else { t.owners.gid_of(spec) };
    if let Some(id) = found {
        return IdOverride { name: spec.to_vec(), id: Some(id) };
    }
    let digits = spec.strip_prefix(b"+").unwrap_or(spec);
    let id = std::str::from_utf8(digits).ok().and_then(|s| s.parse().ok());
    IdOverride { name: spec.to_vec(), id }
}

/// Estado da criação.
pub struct Creator {
    pub w: Writer,
    links: HashMap<(u64, u64), Vec<u8>>,
    archive_id: Option<(u64, u64)>,
    excluder: Excluder,
    initial_cwd: Vec<u8>,
    cur_chdir: Vec<Vec<u8>>,
    pub format: Format,
    mtime: Option<Time>,
    owner: Option<IdOverride>,
    group: Option<IdOverride>,
    mode: Option<ModeSpec>,
    newer: Option<Time>,
    umask: u32,
    top_dev: u64,
    /// Arquivos e diretórios a apagar no fim (`--remove-files`).
    to_remove: Vec<(Vec<u8>, bool)>,
    /// Membros já no arquivo (pro `-u`): nome -> mtime mais recente.
    pub existing: Option<HashMap<Vec<u8>, Time>>,
    pax: super::writer::PaxConfig,
}

fn st_time(st: &Stat) -> Time {
    Time::new(st.mtime.sec, st.mtime.nsec)
}

fn dev_major(rdev: u64) -> u32 {
    (((rdev >> 8) & 0xfff) | ((rdev >> 32) & !0xfff)) as u32
}

fn dev_minor(rdev: u64) -> u32 {
    ((rdev & 0xff) | ((rdev >> 12) & !0xff)) as u32
}

impl Creator {
    pub fn new(t: &mut Tar, w: Writer, archive_id: Option<(u64, u64)>) -> R<Creator> {
        let format = t.o.format.unwrap_or(Format::Gnu);
        let excluder = t.excluder.clone();
        let tz = crate::tz::local();
        let mtime = match t.o.mtime.clone() {
            None => None,
            Some(spec) => Some(date_or_file(t, &spec, &tz)?),
        };
        let newer = match t.o.newer.clone() {
            None => None,
            Some(spec) => Some(date_or_file(t, &spec, &tz)?),
        };
        let owner = t.o.owner.clone().map(|s| parse_id_override(t, &s, true));
        let group = t.o.group.clone().map(|s| parse_id_override(t, &s, false));
        let mode = match t.o.mode_changes.clone() {
            None => None,
            Some(s) => match ModeSpec::parse(&s) {
                Some(m) => Some(m),
                None => return Err(t.fatal("Invalid mode given on option")),
            },
        };
        let pax = match super::writer::PaxConfig::parse(&t.o.pax_options) {
            Ok(p) => p,
            Err(m) => return Err(t.fatal(m)),
        };
        let s = sys();
        let umask = s.umask(0);
        s.umask(umask);
        Ok(Creator {
            pax,
            w,
            links: HashMap::new(),
            archive_id,
            excluder,
            initial_cwd: s.getcwd().unwrap_or_else(|_| b"/".to_vec()),
            cur_chdir: Vec::new(),
            format,
            mtime,
            owner,
            group,
            mode,
            newer,
            umask,
            top_dev: 0,
            to_remove: Vec::new(),
            existing: None,
        })
    }

    /// Vai pro diretório de um operando (sequência de `-C` a partir do diretório inicial).
    fn enter(&mut self, t: &mut Tar, chdir: &[Vec<u8>]) -> R<()> {
        if self.cur_chdir.as_slice() == chdir {
            return Ok(());
        }
        let s = sys();
        let _ = s.chdir(&self.initial_cwd);
        self.cur_chdir.clear();
        for d in chdir {
            if let Err(e) = s.chdir(d) {
                let mut m = quote::colon(d);
                m.extend_from_slice(format!(": Cannot open: {}", e.message()).as_bytes());
                return Err(t.fatal(m));
            }
            self.cur_chdir.push(d.clone());
        }
        Ok(())
    }

    /// Nome guardado: sem prefixo inseguro (com aviso), `/` no fim de diretório, `--transform`.
    fn stored_name(&self, t: &mut Tar, path: &[u8], is_dir: bool) -> Vec<u8> {
        let mut name = if t.o.absolute_names {
            path.to_vec()
        } else {
            let (prefix, rest) = names::unsafe_prefix(path);
            if !prefix.is_empty() {
                t.warn_prefix(&prefix, false);
            }
            rest
        };
        if is_dir && !name.ends_with(b"/") {
            name.push(b'/');
        }
        transform::apply_all(t, &name, Target::Name)
    }

    fn base_member(&self, t: &mut Tar, name: Vec<u8>, st: &Stat, typeflag: u8) -> Member {
        let is_dir = typeflag == kind::DIR;
        let mut mode = st.mode & 0o7777;
        if let Some(ms) = &self.mode {
            mode = ms.apply(mode, is_dir, self.umask);
        }
        let mut uid = st.uid as i64;
        let mut gid = st.gid as i64;
        let mut uname = if t.o.numeric_owner { Vec::new() } else { t.owners.uname(st.uid).unwrap_or_default() };
        let mut gname = if t.o.numeric_owner { Vec::new() } else { t.owners.gname(st.gid).unwrap_or_default() };
        if let Some(o) = &self.owner {
            if let Some(id) = o.id {
                uid = id as i64;
            }
            uname = if t.o.numeric_owner { Vec::new() } else { o.name.clone() };
        }
        if let Some(g) = &self.group {
            if let Some(id) = g.id {
                gid = id as i64;
            }
            gname = if t.o.numeric_owner { Vec::new() } else { g.name.clone() };
        }
        let mut mtime = st_time(st);
        if let Some(m) = self.mtime {
            if !t.o.clamp_mtime || mtime > m {
                mtime = m;
            }
        }
        let pax = self.format == Format::Pax;
        Member {
            name,
            typeflag,
            mode,
            uid,
            gid,
            uname,
            gname,
            mtime,
            atime: pax.then(|| Time::new(st.atime.sec, st.atime.nsec)),
            ctime: pax.then(|| Time::new(st.ctime.sec, st.ctime.nsec)),
            ..Member::default()
        }
    }

    /// Escreve os cabeçalhos; em erro de formato avisa e devolve `false`.
    fn write_headers(&mut self, t: &mut Tar, m: &Member) -> bool {
        if matches!(self.format, Format::Ustar | Format::V7) && m.linkname.len() > 100 {
            // O GNU avisa e mesmo assim grava o membro com o alvo cortado em 100 bytes.
            let mut msg = quote::colon(&m.linkname);
            msg.extend_from_slice(b": link name is too long; not dumped");
            t.error(msg);
        }
        match super::writer::headers(m, self.format, &self.pax) {
            Ok(h) => {
                self.w.write_raw(&h);
                true
            }
            Err(e) => {
                let mut msg = quote::colon(&m.name);
                match e {
                    HeaderError::CannotSplit => msg.extend_from_slice(b": file name is too long (cannot be split); not dumped"),
                    HeaderError::TooLong(n) => {
                        msg.extend_from_slice(format!(": file name is too long (max {n}); not dumped").as_bytes())
                    }
                    HeaderError::Range { field, value, max } => {
                        msg = format!("value {value} out of {field} range 0..{max}").into_bytes();
                    }
                }
                t.error(msg);
                false
            }
        }
    }

    fn verbose(&mut self, t: &mut Tar, shown: &[u8], m: &Member) {
        if t.o.verbose == 0 {
            return;
        }
        let mut line = t.block_prefix(self.w.blocks);
        if t.o.verbose > 1 {
            let q = t.o.quoting.clone();
            line.extend_from_slice(&t.lister.line(m, shown, &q));
        } else {
            line.extend_from_slice(&quote::quote_with(shown, &t.o.quoting, false));
        }
        t.stdlis(&line);
    }

    /// Um operando da linha de comando.
    pub fn dump_operand(&mut self, t: &mut Tar, arg: &NameArg) -> R<()> {
        self.enter(t, &arg.chdir)?;
        let path = names::trim_trailing_slashes(&arg.name).to_vec();
        let path = if path.is_empty() { arg.name.clone() } else { path };
        self.top_dev = 0;
        self.dump(t, &path, true, 0, arg.recursion)
    }

    fn dump(&mut self, t: &mut Tar, path: &[u8], top: bool, depth: usize, recursion: bool) -> R<()> {
        sysabi::sys::checkpoint();
        t.checkpoint(self.w.records, true);
        let s = sys();
        let flags = if t.o.dereference { AtFlags::empty() } else { AtFlags::SYMLINK_NOFOLLOW };
        let st = match s.fstatat(Fd::CWD, path, flags) {
            Ok(st) => st,
            Err(e) => {
                t.sys_error(path, "Cannot stat", e);
                return Ok(());
            }
        };
        if self.excluder.excluded(path) {
            return Ok(());
        }
        if top {
            self.top_dev = st.dev;
        }
        if self.archive_id == Some((st.dev, st.ino)) && st.file_type() != FileType::Directory {
            let mut m = quote::colon(path);
            m.extend_from_slice(b": archive cannot contain itself; not dumped");
            t.msg(m);
            return Ok(());
        }
        let is_dir = st.file_type() == FileType::Directory;
        if let Some(newer) = self.newer
            && !is_dir
        {
            let ct = Time::new(st.ctime.sec, st.ctime.nsec);
            let fresh = st_time(&st) >= newer || (!t.o.newer_mtime_only && ct >= newer);
            if !fresh {
                if t.o.verbose > 0 {
                    let mut m = quote::colon(path);
                    m.extend_from_slice(b": file is unchanged; not dumped");
                    t.msg(m);
                }
                return Ok(());
            }
        }
        // `-u`: só o que é mais novo que a cópia do arquivo.
        let stored = self.stored_name(t, path, is_dir);
        if let Some(existing) = &self.existing
            && !is_dir
            && let Some(old) = existing.get(&stored)
            && st_time(&st) <= *old
        {
            return Ok(());
        }
        let mut shown = path.to_vec();
        if is_dir && !shown.ends_with(b"/") {
            shown.push(b'/');
        }
        match st.file_type() {
            FileType::Directory => {
                if !top && t.o.one_file_system && st.dev != self.top_dev {
                    let mut m = quote::colon(&shown);
                    m.extend_from_slice(b": file is on a different filesystem; not dumped");
                    t.msg(m);
                    return Ok(());
                }
                let m = self.base_member(t, stored, &st, kind::DIR);
                // No `-u` o GNU não acrescenta diretórios, só o conteúdo mais novo.
                if self.existing.is_none() {
                    if !self.write_headers(t, &m) {
                        return Ok(());
                    }
                    self.verbose(t, &shown, &m);
                }
                if !recursion {
                    return Ok(());
                }
                if depth >= MAX_DEPTH {
                    t.sys_error(path, "Cannot open", Errno::ELOOP);
                    return Ok(());
                }
                let entries = match sysabi::sys::read_dir(path) {
                    Ok(e) => e,
                    Err(e) => {
                        t.sys_error(path, "Cannot open", e);
                        return Ok(());
                    }
                };
                let mut entries = entries;
                match t.o.sort {
                    SortOrder::None => {}
                    SortOrder::Name => entries.sort_by(|a, b| a.name.cmp(&b.name)),
                    SortOrder::Inode => entries.sort_by_key(|e| e.ino),
                }
                // Marcas de cache (`--exclude-caches*`, `--exclude-tag*`).
                let mut tag: Option<(Vec<u8>, u8)> = None;
                if t.o.exclude_caches > 0 {
                    let tagpath = crate::sysutil::join(path, b"CACHEDIR.TAG");
                    if sysabi::sys::read_file(&tagpath).is_ok_and(|d| d.starts_with(CACHEDIR_SIGNATURE)) {
                        tag = Some((b"CACHEDIR.TAG".to_vec(), t.o.exclude_caches));
                    }
                }
                for (name, kind) in t.o.exclude_tags.clone() {
                    if tag.is_none() && entries.iter().any(|e| e.name == name) {
                        tag = Some((name, kind));
                    }
                }
                if let Some((tagname, level)) = tag {
                    let mut m = quote::colon(&shown);
                    m.extend_from_slice(b": contains a cache directory tag ");
                    m.extend_from_slice(&quote::escape(&tagname));
                    m.extend_from_slice(b"; contents not dumped");
                    t.msg(m);
                    if level == 1 {
                        let child = join_path(path, &tagname);
                        self.dump(t, &child, false, depth + 1, recursion)?;
                    }
                    return Ok(());
                }
                for e in entries {
                    let child = join_path(path, &e.name);
                    self.dump(t, &child, false, depth + 1, recursion)?;
                }
                if t.o.remove_files {
                    self.to_remove.push((path.to_vec(), true));
                }
                Ok(())
            }
            FileType::Regular => {
                // O GNU guarda o nome de todo arquivo comum como possível alvo de link físico e avisa
                // também dessa remoção de prefixo.
                if !t.o.absolute_names {
                    let (prefix, _) = names::unsafe_prefix(path);
                    if !prefix.is_empty() {
                        t.warn_prefix(&prefix, true);
                    }
                }
                let key = (st.dev, st.ino);
                if !t.o.hard_dereference
                    && let Some(first) = self.links.get(&key).cloned()
                {
                    let mut m = self.base_member(t, stored, &st, kind::LNK);
                    m.linkname = first;
                    if self.write_headers(t, &m) {
                        self.verbose(t, &shown, &m);
                        if t.o.remove_files {
                            self.to_remove.push((path.to_vec(), false));
                        }
                    }
                    return Ok(());
                }
                let mut m = self.base_member(t, stored.clone(), &st, kind::REG);
                m.size = st.size;
                let fd = match super::open(path, OFlags::RDONLY, 0) {
                    Ok(fd) => fd,
                    Err(e) => {
                        t.sys_error(path, "Cannot open", e);
                        return Ok(());
                    }
                };
                if !self.write_headers(t, &m) {
                    let _ = s.close(fd);
                    return Ok(());
                }
                self.verbose(t, &shown, &m);
                self.copy_file(t, fd, path, st.size);
                let _ = s.close(fd);
                if !t.o.hard_dereference {
                    self.links.insert(key, stored);
                }
                if t.o.remove_files {
                    self.to_remove.push((path.to_vec(), false));
                }
                Ok(())
            }
            FileType::Symlink => {
                let target = match s.readlinkat(Fd::CWD, path) {
                    Ok(tg) => tg,
                    Err(e) => {
                        t.sys_error(path, "Cannot readlink", e);
                        return Ok(());
                    }
                };
                let mut m = self.base_member(t, stored, &st, kind::SYM);
                m.linkname = transform::apply_all(t, &target, Target::Symlink);
                if self.write_headers(t, &m) {
                    self.verbose(t, &shown, &m);
                    if t.o.remove_files {
                        self.to_remove.push((path.to_vec(), false));
                    }
                }
                Ok(())
            }
            FileType::CharDevice | FileType::BlockDevice | FileType::Fifo => {
                let tf = match st.file_type() {
                    FileType::CharDevice => kind::CHR,
                    FileType::BlockDevice => kind::BLK,
                    _ => kind::FIFO,
                };
                let mut m = self.base_member(t, stored, &st, tf);
                if tf != kind::FIFO {
                    m.devmajor = dev_major(st.rdev);
                    m.devminor = dev_minor(st.rdev);
                }
                if self.write_headers(t, &m) {
                    self.verbose(t, &shown, &m);
                }
                Ok(())
            }
            FileType::Socket => {
                let mut m = quote::colon(path);
                m.extend_from_slice(b": socket ignored");
                t.msg(m);
                Ok(())
            }
        }
    }

    /// Copia o conteúdo de um arquivo pros blocos, completando com zeros se ele encolheu.
    fn copy_file(&mut self, t: &mut Tar, fd: Fd, path: &[u8], size: u64) {
        let mut left = size;
        let mut buf = vec![0u8; 64 * 1024];
        let mut pending: Vec<u8> = Vec::new();
        while left > 0 {
            sysabi::sys::checkpoint();
            let want = (left.min(buf.len() as u64)) as usize;
            let n = match sysabi::sys::read(fd, &mut buf[..want]) {
                Ok(0) => break,
                Ok(n) => n,
                Err(Errno::EINTR) => continue,
                Err(e) => {
                    t.sys_error(path, "Read error", e);
                    break;
                }
            };
            pending.extend_from_slice(&buf[..n]);
            left -= n as u64;
            let whole = pending.len() / 512 * 512;
            if whole > 0 {
                let rest = pending.split_off(whole);
                self.w.write_data(&pending);
                pending = rest;
            }
        }
        if left > 0 {
            let mut m = quote::colon(path);
            let s = if left == 1 { "" } else { "s" };
            m.extend_from_slice(format!(": File shrank by {left} byte{s}; padding with zeros").as_bytes());
            t.error(m);
            pending.resize(pending.len() + left as usize, 0);
        }
        if !pending.is_empty() {
            self.w.write_data(&pending);
        }
    }

    /// `--remove-files`: apaga o que foi arquivado.
    fn remove_files(&mut self, t: &mut Tar) {
        let s = sys();
        let _ = s.chdir(&self.initial_cwd);
        for (p, is_dir) in std::mem::take(&mut self.to_remove) {
            let flags = if is_dir { AtFlags::REMOVEDIR } else { AtFlags::empty() };
            if let Err(e) = s.unlinkat(Fd::CWD, &p, flags) {
                let what = if is_dir { "Cannot rmdir" } else { "Cannot unlink" };
                t.sys_error(&p, what, e);
            }
        }
    }
}

fn join_path(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = dir.to_vec();
    if !p.ends_with(b"/") {
        p.push(b'/');
    }
    p.extend_from_slice(name);
    p
}

/// `--mtime`/`--newer`: data, ou nome de arquivo (começando com `/` ou `.`) cuja data vale.
fn date_or_file(t: &mut Tar, spec: &[u8], tz: &jiff::tz::TimeZone) -> R<Time> {
    if spec.first().is_some_and(|&c| c == b'/' || c == b'.') {
        return match sysabi::sys::stat(spec) {
            Ok(st) => Ok(st_time(&st)),
            Err(e) => {
                let mut m = quote::colon(spec);
                m.extend_from_slice(format!(": Cannot stat: {}", e.message()).as_bytes());
                Err(t.fatal(m))
            }
        };
    }
    match date::parse(spec, tz) {
        Some(tm) => Ok(tm),
        None => {
            let mut m = b"Substituting -9223372036854775807 for unknown date format ".to_vec();
            m.extend_from_slice(&quote::locale(spec));
            t.msg(m);
            Ok(Time::new(i64::MIN, 0))
        }
    }
}

/// Abre o arquivo de saída do `-c`.
fn open_output(t: &mut Tar, name: &[u8]) -> R<Fd> {
    if name == b"-" {
        if sys().isatty(Fd::STDOUT) {
            return Err(t.fatal("Refusing to write archive contents to terminal (missing -f option?)"));
        }
        return Ok(Fd::STDOUT);
    }
    match super::open(name, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, 0o666) {
        Ok(fd) => Ok(fd),
        Err(e) => {
            let mut m = quote::colon(name);
            m.extend_from_slice(format!(": Cannot open: {}", e.message()).as_bytes());
            Err(t.fatal(m))
        }
    }
}

fn record_size(t: &Tar) -> usize {
    t.o.record_size.unwrap_or(t.o.blocking_factor * 512)
}

fn archive_identity(fd: Fd) -> Option<(u64, u64)> {
    match sys().fstat(fd) {
        Ok(st) if st.file_type() == FileType::Regular => Some((st.dev, st.ino)),
        _ => None,
    }
}

/// Rótulo de volume (`-V`).
fn write_label(t: &mut Tar, c: &mut Creator) {
    if let Some(label) = t.o.label.clone() {
        // O GNU grava o rótulo só com nome, data e tipo (modo, dono e tamanho ficam vazios).
        let now = sys().clock_gettime(sysabi::Clock::Realtime).map(|t| t.sec).unwrap_or(0);
        let mut b = super::header::zero_block();
        super::header::put_bytes(&mut b, super::header::NAME, &label);
        super::header::put_octal(&mut b, super::header::MTIME, now.max(0) as u64);
        b[super::header::TYPEFLAG] = kind::GNU_VOLHDR;
        match c.format {
            Format::Gnu | Format::OldGnu => super::header::put_bytes(&mut b, (257, 8), b"ustar  \0"),
            Format::Ustar | Format::Pax => {
                super::header::put_bytes(&mut b, super::header::MAGIC, b"ustar\0");
                super::header::put_bytes(&mut b, super::header::VERSION, b"00");
            }
            Format::V7 => {}
        }
        super::header::put_checksum(&mut b);
        c.w.write_block(&b);
    }
}

pub fn run(t: &mut Tar) -> R<()> {
    let name = t.archive_name();
    let comp = compress::write_compression(t, &name);
    let fd = open_output(t, &name)?;
    let archive_id = archive_identity(fd);
    let (sink, prog) = compress::sink_for(t, fd, &comp)?;
    let w = Writer::new(sink, record_size(t));
    let mut c = Creator::new(t, w, archive_id)?;
    write_label(t, &mut c);
    let names = t.o.names.clone();
    let mut result = Ok(());
    for n in &names {
        if let Err(e) = c.dump_operand(t, n) {
            result = Err(e);
            break;
        }
    }
    let _ = sys().chdir(&c.initial_cwd);
    let w = std::mem::replace(&mut c.w, Writer::new(Sink::Mem(Vec::new()), 512));
    let total = w.blocks * 512;
    let final_records = (w.blocks + 2).div_ceil(w.record_blocks());
    let res = compress::finish_write(t, w, fd, prog, &name);
    t.checkpoint(final_records, true);
    if fd != Fd::STDOUT {
        let _ = sys().close(fd);
    }
    result?;
    res?;
    if t.o.verify && name != b"-" && comp.is_none() {
        super::compare::verify(t, &name)?;
    }
    if t.o.remove_files {
        c.remove_files(t);
    }
    if t.o.totals {
        let rec = record_size(t) as u64;
        print_totals(t, "written", total.div_ceil(rec) * rec);
    }
    Ok(())
}

/// `--totals`: "Total bytes written: N (H, R/s)". A taxa sai do relógio monotônico do sandbox.
pub fn print_totals(t: &mut Tar, what: &str, bytes: u64) {
    let human = human_size(bytes);
    let now = sys().clock_gettime(sysabi::Clock::Monotonic).unwrap_or_default();
    let elapsed = (now.sec - t.start_time.sec) as f64 + (now.nsec as f64 - t.start_time.nsec as f64) / 1e9;
    let rate = if elapsed > 0.0 { human_size((bytes as f64 / elapsed) as u64) } else { human_size(bytes) };
    let msg = format!("Total bytes {what}: {bytes} ({human}, {rate}/s)\n");
    t.out.flush();
    crate::sysutil::eprint(msg);
}

/// Tamanho no estilo do `human_readable` com `--block-size=1` e sufixo binário ("10KiB", "1.5MiB").
pub fn human_size(n: u64) -> String {
    const UNITS: [&str; 7] = ["", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    if n < 1024 {
        return n.to_string();
    }
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if v < 10.0 {
        let r = (v * 10.0).ceil() / 10.0;
        if r < 10.0 {
            return format!("{r:.1}{}", UNITS[u]);
        }
        return format!("{}{}", r as u64, UNITS[u]);
    }
    format!("{}{}", v.ceil() as u64, UNITS[u])
}

/// `-r` e `-u`: acha o fim do arquivo existente e escreve a partir dali.
pub fn append(t: &mut Tar, update: bool) -> R<()> {
    let name = t.archive_name();
    if name == b"-" {
        return Err(t.fatal("Cannot update a stdin archive"));
    }
    if t.o.compression.is_some() {
        let p = t.prog.clone();
        super::usage_error(&p, b"Cannot update compressed archives");
        t.exit = 2;
        return Err(Fatal);
    }
    let fd = match super::open(&name, OFlags::RDWR | OFlags::CREAT, 0o666) {
        Ok(fd) => fd,
        Err(e) => {
            let mut m = quote::colon(&name);
            m.extend_from_slice(format!(": Cannot open: {}", e.message()).as_bytes());
            return Err(t.fatal(m));
        }
    };
    let s = sys();
    // Lê o arquivo inteiro pra achar o fim (e, no `-u`, as datas dos membros).
    let data = match crate::sysutil::read_fd(fd) {
        Ok(d) => d,
        Err(e) => {
            let mut m = quote::colon(&name);
            m.extend_from_slice(format!(": Read error: {}", e.message()).as_bytes());
            return Err(t.fatal(m));
        }
    };
    if crate::codec::Format::sniff(&data).is_some() {
        return Err(t.fatal("Cannot update compressed archives"));
    }
    let mut existing: HashMap<Vec<u8>, Time> = HashMap::new();
    let mut r = Reader::new(Source::Mem { data: data.clone(), pos: 0 });
    let end: u64 = loop {
        let before = r.offset;
        match r.read_header() {
            Status::Member(m) => {
                let e = existing.entry(m.name.clone()).or_insert(m.mtime);
                if m.mtime > *e {
                    *e = m.mtime;
                }
                if r.skip_data(m.data_size()).is_err() {
                    break r.offset.min(data.len() as u64);
                }
            }
            Status::ZeroBlock | Status::EndOfFile | Status::Error(_) => break before,
            Status::Failure => {
                if before == 0 && !data.is_empty() {
                    let _ = s.close(fd);
                    t.error("This does not look like a tar archive");
                    return Err(t.fatal("Exiting with failure status due to previous errors"));
                }
                break before;
            }
        }
    };
    let rec = record_size(t) as u64;
    let rec_start = end - end % rec;
    let pending = data[rec_start as usize..end as usize].to_vec();
    if s.lseek(fd, rec_start as i64, sysabi::Whence::Set).is_err() {
        return Err(t.fatal("Cannot seek"));
    }
    let w = Writer::with_pending(Sink::Fd(fd), rec as usize, pending, end / 512);
    let mut c = Creator::new(t, w, archive_identity(fd))?;
    if update {
        c.existing = Some(existing);
    }
    let names = t.o.names.clone();
    let mut result = Ok(());
    for n in &names {
        if let Err(e) = c.dump_operand(t, n) {
            result = Err(e);
            break;
        }
    }
    let _ = s.chdir(&c.initial_cwd);
    let res = compress::finish_write(t, c.w, fd, None, &name);
    let _ = s.close(fd);
    result?;
    res
}

/// `-A`: acrescenta os membros de outros arquivos.
pub fn catenate(t: &mut Tar) -> R<()> {
    let name = t.archive_name();
    let fd = match super::open(&name, OFlags::RDWR | OFlags::CREAT, 0o666) {
        Ok(fd) => fd,
        Err(e) => {
            let mut m = quote::colon(&name);
            m.extend_from_slice(format!(": Cannot open: {}", e.message()).as_bytes());
            return Err(t.fatal(m));
        }
    };
    let s = sys();
    let data = crate::sysutil::read_fd(fd).unwrap_or_default();
    let end = archive_end(&data);
    let rec = record_size(t) as u64;
    let rec_start = end - end % rec;
    let pending = data[rec_start as usize..end as usize].to_vec();
    let _ = s.lseek(fd, rec_start as i64, sysabi::Whence::Set);
    let mut w = Writer::with_pending(Sink::Fd(fd), rec as usize, pending, end / 512);
    for n in t.o.names.clone() {
        let src = match crate::sysutil::read_path(&n.name) {
            Ok(d) => d,
            Err(e) => {
                t.sys_error(&n.name, "Cannot open", e);
                continue;
            }
        };
        if !src.is_empty() && !super::header::checksum_ok(&first_block(&src)) && !super::header::is_zero(&first_block(&src)) {
            let mut m = quote::colon(&n.name);
            m.extend_from_slice(b": This does not look like a tar archive");
            t.error(m);
            continue;
        }
        // Como o GNU: o arquivo de origem entra inteiro (registros completos, com os zeros do fim dele).
        let mut whole = src;
        let pad = (512 - whole.len() % 512) % 512;
        whole.resize(whole.len() + pad, 0);
        w.write_raw(&whole);
    }
    let res = compress::finish_write(t, w, fd, None, &name);
    let _ = s.close(fd);
    res
}

fn first_block(d: &[u8]) -> [u8; 512] {
    let mut b = [0u8; 512];
    let n = d.len().min(512);
    b[..n].copy_from_slice(&d[..n]);
    b
}

/// Deslocamento do fim lógico de um arquivo tar na memória (o primeiro bloco de zeros).
fn archive_end(data: &[u8]) -> u64 {
    let mut r = Reader::new(Source::Mem { data: data.to_vec(), pos: 0 });
    let mut end = 0u64;
    loop {
        let before = r.offset;
        match r.read_header() {
            Status::Member(m) => {
                if r.skip_data(m.data_size()).is_err() {
                    return r.offset.min(data.len() as u64);
                }
                end = r.offset;
            }
            _ => return before.max(end).min(data.len() as u64),
        }
    }
}
