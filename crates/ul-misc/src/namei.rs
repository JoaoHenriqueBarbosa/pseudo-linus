//! `namei` do util-linux 2.41 (pacote util-linux do Debian 13): segue um caminho componente a componente,
//! mostrando o tipo (e com `-m`/`-o`/`-l` as permissões e os donos) de cada um, expandindo os links
//! simbólicos que encontra (até 20, o `MAXSYMLINKS` do glibc) e parando no primeiro componente que
//! não existe.
//!
//! Porte direto do `misc-utils/namei.c`: a lista de componentes é montada antes de imprimir, o texto
//! de cada link entra na lista logo depois dele (com um nível a mais de recuo), e as larguras das
//! colunas de dono e grupo acumulam todos os nomes vistos até ali, inclusive nos argumentos
//! anteriores. `-Z` sempre mostra `?` (o sandbox não tem contexto de segurança).

use std::ffi::OsString;
use std::io::Write;

use sysabi::{Ctx, Errno, FileType, Stat, mode, sys};

use crate::util::io;
use crate::util::ul;
use crate::util::{Getopt, HasArg, LongOpt, display_width};

/// `MAXSYMLINKS` do `sys/param.h` da glibc.
const MAXSYMLINKS: usize = 20;

const NAMEI_NOLINKS: u32 = 1 << 1;
const NAMEI_MODES: u32 = 1 << 2;
const NAMEI_MNTS: u32 = 1 << 3;
const NAMEI_OWNERS: u32 = 1 << 4;
const NAMEI_VERTICAL: u32 = 1 << 5;
const NAMEI_CONTEXT: u32 = 1 << 6;

const LONGS: &[LongOpt] = &[
    LongOpt::new("help", HasArg::No, b'h' as i32),
    LongOpt::new("version", HasArg::No, b'V' as i32),
    LongOpt::new("mountpoints", HasArg::No, b'x' as i32),
    LongOpt::new("modes", HasArg::No, b'm' as i32),
    LongOpt::new("owners", HasArg::No, b'o' as i32),
    LongOpt::new("long", HasArg::No, b'l' as i32),
    LongOpt::new("nolinks", HasArg::No, b'n' as i32),
    LongOpt::new("vertical", HasArg::No, b'v' as i32),
    LongOpt::new("context", HasArg::No, b'Z' as i32),
];

fn usage_text(p: &str) -> String {
    format!(
        "\nUsage:\n {p} [options] <pathname>...\n\nFollow a pathname until a terminal point is found.\n\nOptions:\n \
-x, --mountpoints   show mount point directories with a 'D'\n \
-m, --modes         show the mode bits of each file\n \
-o, --owners        show owner and group name of each file\n \
-l, --long          use a long listing format (-m -o -v) \n \
-n, --nosymlinks    don't follow symlinks\n \
-v, --vertical      vertical align of modes and owners\n \
-Z, --context       print any security context of each file \n \
-h, --help          display this help\n \
-V, --version       display version\n\nFor more details see namei(1).\n"
    )
}

/// Um componente do caminho (`struct namei`).
struct Item {
    st: Stat,
    name: Vec<u8>,
    /// Caminho absoluto (ou relativo ao cwd) do alvo, se for link simbólico.
    abslink: Option<Vec<u8>>,
    /// Onde começa a parte relativa do alvo dentro de `abslink`.
    relstart: usize,
    level: usize,
    mountpoint: bool,
    /// errno do `lstat` quando o componente não existe.
    noent: Option<Errno>,
}

/// O que o `new_namei` precisa saber do item anterior.
#[derive(Copy, Clone)]
struct ParentInfo {
    mode: u32,
    dev: u64,
    ino: u64,
}

impl ParentInfo {
    fn of(item: &Item) -> ParentInfo {
        ParentInfo { mode: item.st.mode, dev: item.st.dev, ino: item.st.ino }
    }

    fn is_dir(&self) -> bool {
        self.mode & mode::S_IFMT == mode::S_IFDIR
    }

    fn is_lnk(&self) -> bool {
        self.mode & mode::S_IFMT == mode::S_IFLNK
    }
}

/// Cache de nomes de usuário ou grupo (`struct idcache`): a ordem de chegada e a maior largura.
#[derive(Default)]
struct IdCache {
    entries: Vec<(u32, String)>,
    width: usize,
}

impl IdCache {
    fn get(&self, id: u32) -> Option<&str> {
        self.entries.iter().find(|(i, _)| *i == id).map(|(_, n)| n.as_str())
    }

    /// `add_id`: o nome (ou o número, quando não há nome) entra uma vez só.
    fn add(&mut self, id: u32, lookup: impl FnOnce() -> Option<String>) {
        if self.get(id).is_some() {
            return;
        }
        let name = lookup().filter(|n| display_width(n) > 0).unwrap_or_else(|| id.to_string());
        let w = display_width(&name);
        self.width = self.width.max(w);
        self.entries.push((id, name));
    }
}

struct Namei {
    flags: u32,
    ucache: IdCache,
    gcache: IdCache,
}

/// `xstrmode`.
fn xstrmode(m: u32) -> Vec<u8> {
    let mut s = Vec::with_capacity(10);
    s.push(match FileType::from_mode(m) {
        FileType::Directory => b'd',
        FileType::Symlink => b'l',
        FileType::CharDevice => b'c',
        FileType::BlockDevice => b'b',
        FileType::Socket => b's',
        FileType::Fifo => b'p',
        FileType::Regular => b'-',
    });
    let bit = |mask: u32, ch: u8| if m & mask != 0 { ch } else { b'-' };
    s.push(bit(0o400, b'r'));
    s.push(bit(0o200, b'w'));
    s.push(if m & mode::S_ISUID != 0 { if m & 0o100 != 0 { b's' } else { b'S' } } else { bit(0o100, b'x') });
    s.push(bit(0o040, b'r'));
    s.push(bit(0o020, b'w'));
    s.push(if m & mode::S_ISGID != 0 { if m & 0o010 != 0 { b's' } else { b'S' } } else { bit(0o010, b'x') });
    s.push(bit(0o004, b'r'));
    s.push(bit(0o002, b'w'));
    s.push(if m & mode::S_ISVTX != 0 { if m & 0o001 != 0 { b't' } else { b'T' } } else { bit(0o001, b'x') });
    s
}

impl Namei {
    /// `readlink_to_namei`: o alvo do link, já combinado com o diretório do próprio link quando relativo.
    fn readlink_to_namei(&self, item: &mut Item, path: &[u8]) -> Result<(), String> {
        let sym = match sys::current().readlinkat(sysabi::Fd::CWD, path) {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => return Err(format!("failed to read symlink: {}: {}", io::lossy(path), Errno::EINVAL.message())),
            Err(e) => return Err(format!("failed to read symlink: {}: {}", io::lossy(path), e.message())),
        };
        if sym.first() != Some(&b'/') {
            if let Some(p) = path.iter().rposition(|&b| b == b'/') {
                // alvo relativo: vira absoluto a partir do diretório do link
                let mut abs = path[..p].to_vec();
                abs.push(b'/');
                abs.extend_from_slice(&sym);
                item.relstart = p + 1;
                item.abslink = Some(abs);
                return Ok(());
            }
        }
        item.abslink = Some(sym);
        item.relstart = 0;
        Ok(())
    }

    /// `dotdot_stat`: o `stat` de `<dirname>/..`.
    fn dotdot_stat(dirname: &[u8]) -> Result<Stat, String> {
        let mut path = dirname.to_vec();
        path.extend_from_slice(b"/..");
        sys::stat(&path).map_err(|e| format!("stat of {} failed: {}", io::lossy(&path), e.message()))
    }

    /// `new_namei`.
    fn new_namei(&mut self, parent: Option<ParentInfo>, path: &[u8], fname: &[u8], level: usize) -> Result<Item, String> {
        let mut item = Item {
            st: Stat::default(),
            name: fname.to_vec(),
            abslink: None,
            relstart: 0,
            level,
            mountpoint: false,
            noent: None,
        };
        match sys::lstat(path) {
            Err(e) => {
                item.noent = Some(e);
                return Ok(item);
            }
            Ok(st) => item.st = st,
        }

        if item.st.file_type() == FileType::Symlink {
            self.readlink_to_namei(&mut item, path)?;
        }
        if self.flags & NAMEI_OWNERS != 0 {
            let (uid, gid) = (item.st.uid, item.st.gid);
            self.ucache.add(uid, || sysio::users::passwd_by_uid(uid).map(|p| p.name));
            self.gcache.add(gid, || sysio::users::group_by_gid(gid).map(|g| g.name));
        }

        if self.flags & NAMEI_MNTS != 0 && item.st.file_type() == FileType::Directory {
            let sb: Option<(u64, u64)> = match parent {
                Some(p) if p.is_dir() => Some((p.dev, p.ino)),
                Some(p) if p.is_lnk() => {
                    let s = Self::dotdot_stat(path)?;
                    Some((s.dev, s.ino))
                }
                None => {
                    let s = Self::dotdot_stat(path)?;
                    Some((s.dev, s.ino))
                }
                Some(_) => None,
            };
            if let Some((dev, ino)) = sb {
                if dev != item.st.dev || ino == item.st.ino {
                    // diretório em outro dispositivo, ou a raiz
                    item.mountpoint = true;
                }
            }
        }
        Ok(item)
    }

    /// `add_namei`: os componentes de `orgpath` a partir de `start`, com o `parent` do primeiro.
    fn add_namei(&mut self, parent: Option<ParentInfo>, orgpath: &[u8], start: usize, level: usize) -> Result<Vec<Item>, String> {
        let buf = orgpath.to_vec();
        let mut items: Vec<Item> = Vec::new();
        let mut fpos = start.min(buf.len());
        let mut prev = parent;

        // root directory
        if buf.get(fpos) == Some(&b'/') {
            while buf.get(fpos) == Some(&b'/') {
                fpos += 1; // eat extra '/'
            }
            let it = self.new_namei(prev, b"/", b"/", level)?;
            prev = Some(ParentInfo::of(&it));
            items.push(it);
        }

        while fpos < buf.len() {
            let rel_end = buf[fpos..].iter().position(|&b| b == b'/');
            let comp_end = rel_end.map_or(buf.len(), |p| fpos + p);
            let it = self.new_namei(prev, &buf[..comp_end], &buf[fpos..comp_end], level)?;
            prev = Some(ParentInfo::of(&it));
            items.push(it);
            match rel_end {
                None => break,
                Some(_) => {
                    fpos = comp_end + 1;
                    while buf.get(fpos) == Some(&b'/') {
                        fpos += 1; // eat extra '/'
                    }
                }
            }
        }
        Ok(items)
    }

    /// `follow_symlinks`: `Ok(true)` quando estourou o limite de links.
    fn follow_symlinks(&mut self, items: &mut Vec<Item>) -> Result<bool, String> {
        let mut symcount = 0;
        let mut i = 0;
        while i < items.len() {
            if items[i].noent.is_some() || items[i].st.file_type() != FileType::Symlink {
                i += 1;
                continue;
            }
            symcount += 1;
            if symcount > MAXSYMLINKS {
                // drop the rest of the list
                items.truncate(i + 1);
                return Ok(true);
            }
            let abslink = items[i].abslink.clone().unwrap_or_default();
            let relstart = items[i].relstart;
            let level = items[i].level + 1;
            let parent = ParentInfo::of(&items[i]);
            let new = self.add_namei(Some(parent), &abslink, relstart, level)?;
            let at = i + 1;
            for (k, it) in new.into_iter().enumerate() {
                items.insert(at + k, it);
            }
            i += 1;
        }
        Ok(false)
    }

    /// `print_namei`: `true` quando parou num componente inexistente.
    fn print_namei(&self, out: &mut impl Write, items: &[Item], path: &[u8]) -> bool {
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(b"f: ");
        buf.extend_from_slice(path);
        buf.push(b'\n');
        let mut failed = false;

        for nm in items {
            if let Some(e) = nm.noent {
                let mut blanks = 1;
                if self.flags & NAMEI_MODES != 0 {
                    blanks += 9;
                }
                if self.flags & NAMEI_OWNERS != 0 {
                    blanks += self.ucache.width + self.gcache.width + 2;
                }
                if self.flags & NAMEI_VERTICAL == 0 {
                    blanks += 1;
                }
                if self.flags & NAMEI_CONTEXT == 0 {
                    blanks += 1;
                }
                blanks += nm.level * 2;
                buf.extend(std::iter::repeat_n(b' ', blanks));
                buf.push(b' ');
                buf.extend_from_slice(&nm.name);
                buf.extend_from_slice(format!(" - {}\n", e.message()).as_bytes());
                failed = true;
                break;
            }

            let mut md = xstrmode(nm.st.mode);
            if nm.mountpoint {
                md[0] = b'D';
            }

            if self.flags & NAMEI_VERTICAL == 0 {
                for _ in 0..nm.level {
                    buf.extend_from_slice(b"  ");
                }
                buf.push(b' ');
            }

            if self.flags & NAMEI_MODES != 0 {
                buf.extend_from_slice(&md);
            } else {
                buf.push(md[0]);
            }

            if self.flags & NAMEI_OWNERS != 0 {
                for (cache, id) in [(&self.ucache, nm.st.uid), (&self.gcache, nm.st.gid)] {
                    let name = cache.get(id).unwrap_or("");
                    buf.push(b' ');
                    buf.extend_from_slice(name.as_bytes());
                    buf.extend(std::iter::repeat_n(b' ', cache.width.saturating_sub(name.len())));
                }
            }
            if self.flags & NAMEI_CONTEXT != 0 {
                buf.extend_from_slice(b" ?");
            }
            if self.flags & NAMEI_VERTICAL != 0 {
                for _ in 0..nm.level {
                    buf.extend_from_slice(b"  ");
                }
            }

            buf.push(b' ');
            buf.extend_from_slice(&nm.name);
            if nm.st.file_type() == FileType::Symlink {
                buf.extend_from_slice(b" -> ");
                if let Some(l) = &nm.abslink {
                    buf.extend_from_slice(&l[nm.relstart.min(l.len())..]);
                }
            }
            buf.push(b'\n');
        }
        let _ = out.write_all(&buf);
        failed
    }
}

pub fn main(_ctx: &mut Ctx, args: &[OsString]) -> i32 {
    io::run(|| run(args))
}

fn run(args: &[OsString]) -> i32 {
    let argv = io::args_bytes(args);
    let argv0 = io::argv0(args);
    let short = ul::short_name(args);

    let mut flags = 0u32;
    let mut g = Getopt::from_env(&argv[1..], "ZhVlmnovx", LONGS);
    while let Some(r) = g.next_opt() {
        let o = match r {
            Ok(o) => o,
            Err(e) => {
                io::eprint(format!("{}\n", e.message(&argv0)));
                ul::errtryhelp(&short);
                return 1;
            }
        };
        match o.short() {
            Some('l') => flags |= NAMEI_OWNERS | NAMEI_MODES | NAMEI_VERTICAL,
            Some('m') => flags |= NAMEI_MODES,
            Some('n') => flags |= NAMEI_NOLINKS,
            Some('o') => flags |= NAMEI_OWNERS,
            Some('x') => flags |= NAMEI_MNTS,
            Some('v') => flags |= NAMEI_VERTICAL,
            Some('Z') => flags |= NAMEI_CONTEXT,
            Some('h') => {
                let p = if short.is_empty() { "namei" } else { short.as_str() };
                let mut out = io::stdout();
                let _ = out.write_all(usage_text(p).as_bytes());
                return 0;
            }
            Some('V') => {
                ul::print_version(&short);
                return 0;
            }
            _ => {
                ul::errtryhelp(&short);
                return 1;
            }
        }
    }
    let paths = g.operands();
    if paths.is_empty() {
        ul::warnx(&short, "pathname argument is missing");
        ul::errtryhelp(&short);
        return 1;
    }

    let mut nm = Namei { flags, ucache: IdCache::default(), gcache: IdCache::default() };
    let mut out = io::stdout();
    let mut rc = 0;
    for path in &paths {
        if sys::stat(path).is_err() {
            rc = 1;
        }
        let mut items = match nm.add_namei(None, path, 0, 0) {
            Ok(i) => i,
            Err(m) => return fatal(&short, &m),
        };
        if !items.is_empty() {
            let mut sml = false;
            if nm.flags & NAMEI_NOLINKS == 0 {
                match nm.follow_symlinks(&mut items) {
                    Ok(over) => sml = over,
                    Err(m) => return fatal(&short, &m),
                }
            }
            if nm.print_namei(&mut out, &items, path) {
                rc = 1;
                continue;
            }
            if sml {
                rc = 1;
                ul::warnx(&short, format!("{}: exceeded limit of symlinks", io::lossy(path)));
                continue;
            }
        }
        sys::checkpoint();
    }
    if out.flush().is_err() {
        return 1;
    }
    rc
}

/// `err(EXIT_FAILURE, ...)`: o stdout sai antes da mensagem.
fn fatal(short: &str, msg: &str) -> i32 {
    ul::warnx(short, msg);
    1
}
