//! `-d`/`--diff`/`--compare` (compara o arquivo com o sistema de arquivos e reporta "x: Mod time
//! differs", "Size differs", "Contents differ"... no stdout, saída 1 se houve diferença), `--delete`
//! (reescreve o arquivo sem os membros pedidos) e `--test-label`.

use sysabi::{AtFlags, Errno, Fd, FileType, OFlags};

use super::member::{Kind, Member};
use super::reader::{ReadError, Reader, Source, Status};
use super::writer::{Sink, Writer};
use super::{Flow, R, Tar, compress, names, quote};

use sysabi::sys::current as sys;

fn report(t: &mut Tar, name: &[u8], what: &[u8]) {
    let mut line = quote::colon(name);
    line.extend_from_slice(b": ");
    line.extend_from_slice(what);
    t.stdlis(&line);
    if t.exit == 0 {
        t.exit = 1;
    }
}

fn warn_stat(t: &mut Tar, name: &[u8], what: &str, e: Errno) {
    let mut m = quote::colon(name);
    m.extend_from_slice(format!(": Warning: {what}: {}", e.message()).as_bytes());
    t.msg(m);
    if t.exit == 0 {
        t.exit = 1;
    }
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
    let mut names = names::NameList::new(&t.o.names);
    let res = t.read_and(&mut r, &mut |t, r, m| {
        if !names.items.is_empty() {
            match names.find(&m.name) {
                Some(i) => names.items[i].found += 1,
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
        if t.o.verbose > 0 {
            // No `-d`, `-v` mostra só o nome; a linha longa é do `-vv`.
            let mut line = t.block_prefix(m.main_block);
            if t.o.verbose > 1 {
                let q = t.o.quoting.clone();
                let shown = m.name.clone();
                line.extend_from_slice(&t.lister.line(&m, &shown, &q));
            } else {
                line.extend_from_slice(&quote::quote_with(&m.name, &t.o.quoting, false));
            }
            t.stdlis(&line);
        }
        compare_member(t, r, &m)?;
        Ok(Flow::Continue)
    });
    compress::finish_read(t, child, res)?;
    super::read_totals(t, &r);
    super::report_unmatched(t, &names);
    Ok(())
}

/// `-W`: relê o arquivo recém-criado e compara com o disco ("Verify x" com `-v`).
pub fn verify(t: &mut Tar, name: &[u8]) -> R<()> {
    let (mut r, child) = super::compress::open_read(t, name)?;
    let res = t.read_and(&mut r, &mut |t, r, m| {
        if t.o.verbose > 0 {
            let mut line = b"Verify ".to_vec();
            line.extend_from_slice(&quote::quote_with(&m.name, &t.o.quoting, false));
            t.stdlis(&line);
        }
        compare_member(t, r, &m)?;
        Ok(Flow::Continue)
    });
    compress::finish_read(t, child, res)
}

fn compare_member(t: &mut Tar, r: &mut Reader, m: &Member) -> R<()> {
    let Some(path) = super::extract::target_name(t, &m.name, false, super::transform::Target::Name) else {
        return t.skip_member(r, m);
    };
    let path = names::trim_trailing_slashes(&path).to_vec();
    let s = sys();
    let st = s.fstatat(Fd::CWD, &path, AtFlags::SYMLINK_NOFOLLOW);
    let kind = m.kind();
    match kind {
        Kind::Regular | Kind::Contiguous => {
            let st = match st {
                Ok(st) => st,
                Err(e) => {
                    warn_stat(t, &path, "Cannot stat", e);
                    return t.skip_member(r, m);
                }
            };
            if st.file_type() != FileType::Regular {
                report(t, &path, b"File type differs");
                return t.skip_member(r, m);
            }
            attr_diffs(t, &path, m, &st);
            if st.mtime.sec != m.mtime.sec {
                report(t, &path, b"Mod time differs");
            }
            let size = if m.sparse.is_some() { m.real_size } else { m.size };
            if st.size != size {
                report(t, &path, b"Size differs");
                return t.skip_member(r, m);
            }
            // Conteúdo, em fluxo, contra o arquivo.
            let fd = match super::open(&path, OFlags::RDONLY, 0) {
                Ok(fd) => fd,
                Err(e) => {
                    warn_stat(t, &path, "Cannot open", e);
                    return t.skip_member(r, m);
                }
            };
            let mut differs = false;
            let mut buf = vec![0u8; 64 * 1024];
            let res = r.read_data(m.data_size(), |chunk| {
                if differs {
                    return;
                }
                let mut off = 0;
                while off < chunk.len() {
                    let want = (chunk.len() - off).min(buf.len());
                    let n = sysabi::sys::read(fd, &mut buf[..want]).unwrap_or_default();
                    if n == 0 || buf[..n] != chunk[off..off + n] {
                        differs = true;
                        return;
                    }
                    off += n;
                }
            });
            let _ = s.close(fd);
            match res {
                Ok(()) => {}
                Err(ReadError::UnexpectedEof) => return Err(t.fatal(b"Unexpected EOF in archive")),
                Err(ReadError::Io(e)) => return Err(t.fatal(format!("Read error: {}", e.message()))),
            }
            if differs {
                report(t, &path, b"Contents differ");
            }
            Ok(())
        }
        Kind::Directory => {
            t.skip_member(r, m)?;
            match st {
                Ok(st) if st.file_type() == FileType::Directory => {
                    if st.mode & 0o7777 != m.mode & 0o7777 {
                        report(t, &path, b"Mode differs");
                    }
                }
                Ok(_) => report(t, &path, b"File type differs"),
                Err(e) => warn_stat(t, &path, "Cannot stat", e),
            }
            Ok(())
        }
        Kind::Symlink => {
            t.skip_member(r, m)?;
            match s.readlinkat(Fd::CWD, &path) {
                Ok(target) => {
                    if target != m.linkname {
                        report(t, &path, b"Symlink differs");
                    }
                }
                Err(Errno::EINVAL) => report(t, &path, b"File type differs"),
                Err(e) => warn_stat(t, &path, "Cannot readlink", e),
            }
            Ok(())
        }
        Kind::HardLink => {
            t.skip_member(r, m)?;
            let Some(target) = super::extract::target_name(t, &m.linkname, true, super::transform::Target::Hardlink)
            else {
                return Ok(());
            };
            match (st, s.fstatat(Fd::CWD, &target, AtFlags::SYMLINK_NOFOLLOW)) {
                (Ok(a), Ok(b)) => {
                    if a.ino != b.ino || a.dev != b.dev {
                        let mut what = b"Not linked to ".to_vec();
                        what.extend_from_slice(&quote::colon(&target));
                        report(t, &path, &what);
                    }
                }
                (Err(e), _) => warn_stat(t, &path, "Cannot stat", e),
                (_, Err(e)) => warn_stat(t, &target, "Cannot stat", e),
            }
            Ok(())
        }
        Kind::CharDev | Kind::BlockDev | Kind::Fifo => {
            t.skip_member(r, m)?;
            match st {
                Ok(st) => {
                    let want = match kind {
                        Kind::CharDev => FileType::CharDevice,
                        Kind::BlockDev => FileType::BlockDevice,
                        _ => FileType::Fifo,
                    };
                    if st.file_type() != want {
                        report(t, &path, b"File type differs");
                    } else {
                        if kind != Kind::Fifo {
                            let major = (((st.rdev >> 8) & 0xfff) | ((st.rdev >> 32) & !0xfff)) as u32;
                            let minor = ((st.rdev & 0xff) | ((st.rdev >> 12) & !0xff)) as u32;
                            if major != m.devmajor || minor != m.devminor {
                                report(t, &path, b"Device number differs");
                            }
                        }
                        if st.mode & 0o7777 != m.mode & 0o7777 {
                            report(t, &path, b"Mode differs");
                        }
                    }
                }
                Err(e) => warn_stat(t, &path, "Cannot stat", e),
            }
            Ok(())
        }
        _ => t.skip_member(r, m),
    }
}

fn attr_diffs(t: &mut Tar, path: &[u8], m: &Member, st: &sysabi::Stat) {
    if st.mode & 0o7777 != m.mode & 0o7777 {
        report(t, path, b"Mode differs");
    }
    if st.uid as i64 != m.uid {
        report(t, path, b"Uid differs");
    }
    if st.gid as i64 != m.gid {
        report(t, path, b"Gid differs");
    }
}

/// `--delete`: o arquivo é lido inteiro, os membros que casam saem, e o resto é regravado (no mesmo
/// arquivo, truncado, ou no stdout com `-f -`).
pub fn delete(t: &mut Tar) -> R<()> {
    let name = t.archive_name();
    let stdin = name == b"-";
    let fd = if stdin {
        Fd::STDIN
    } else {
        match super::open(&name, OFlags::RDWR, 0) {
            Ok(fd) => fd,
            Err(e) => {
                let mut m = quote::colon(&name);
                m.extend_from_slice(format!(": Cannot open: {}", e.message()).as_bytes());
                return Err(t.fatal(m));
            }
        }
    };
    if t.o.compression.is_some() {
        let p = t.prog.clone();
        super::usage_error(&p, b"Cannot update compressed archives");
        t.exit = 2;
        return Err(super::Fatal);
    }
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
    let mut names = names::NameList::new(&t.o.names);
    let mut keep: Vec<(u64, u64)> = Vec::new();
    let mut r = Reader::new(Source::Mem { data: data.clone(), pos: 0 });
    let mut first = true;
    loop {
        match r.read_header() {
            Status::Member(m) => {
                first = false;
                let start = m.start_offset;
                let matched = names.find(&m.name).inspect(|&i| {
                    names.items[i].found += 1;
                });
                if r.skip_data(m.data_size()).is_err() {
                    return Err(t.fatal("Unexpected EOF in archive"));
                }
                if matched.is_none() {
                    keep.push((start, r.offset));
                }
            }
            Status::ZeroBlock | Status::EndOfFile => {
                if first && data.is_empty() {
                    t.error("This does not look like a tar archive");
                }
                break;
            }
            Status::Failure => {
                if first {
                    t.error("This does not look like a tar archive");
                    break;
                }
                t.error("Skipping to next header");
            }
            Status::Error(_) => return Err(t.fatal("Unexpected EOF in archive")),
        }
    }
    let record = t.o.record_size.unwrap_or(t.o.blocking_factor * 512);
    let mut w = Writer::new(Sink::Mem(Vec::new()), record);
    for (a, b) in keep {
        w.write_raw(&data[a as usize..(b as usize).min(data.len())]);
    }
    let out = match w.finish(true) {
        Ok(Some(v)) => v,
        _ => Vec::new(),
    };
    let s = sys();
    if stdin {
        let _ = sysabi::sys::write_all(Fd::STDOUT, &out);
    } else {
        let _ = s.lseek(fd, 0, sysabi::Whence::Set);
        if let Err(e) = sysabi::sys::write_all(fd, &out) {
            let mut m = quote::colon(&name);
            m.extend_from_slice(format!(": Cannot write: {}", e.message()).as_bytes());
            return Err(t.fatal(m));
        }
        let _ = s.ftruncate(fd, out.len() as u64);
        let _ = s.close(fd);
    }
    super::report_unmatched(t, &names);
    Ok(())
}

/// `--test-label`: mostra o rótulo do volume; com operandos, sai 0 se algum casa e 1 se não.
pub fn test_label(t: &mut Tar) -> R<()> {
    let (mut r, child) = super::open_for_read(t)?;
    let label = match r.read_header() {
        Status::Member(m) if m.kind() == Kind::Volume => Some(m.name.clone()),
        _ => None,
    };
    compress::finish_read(t, child, Ok(()))?;
    let names: Vec<Vec<u8>> = t.o.names.iter().map(|n| n.name.clone()).collect();
    match label {
        Some(l) => {
            if names.is_empty() {
                let line = quote::quote_with(&l, &t.o.quoting, false);
                t.stdlis(&line);
            } else if !names.iter().any(|n| ul_common::fnmatch::fnmatch::<ul_common::fnmatch::Bytes>(n, &l, ul_common::fnmatch::Flags::TRAILING_BACKSLASH_LITERAL)) {
                t.exit = 1;
            }
        }
        None => {
            if !names.is_empty() {
                t.exit = 1;
            }
        }
    }
    Ok(())
}
