//! `fs.*` diretos, em cima das primitivas de arquivo da sandbox: leitura com teto de resposta, escrita
//! com criação dos pais, listagem e `stat` no formato da API, `mkdir -p` e remoção recursiva.
//!
//! Caminho relativo é resolvido a partir do diretório de trabalho da sandbox, como se um processo
//! estivesse nele.

use sysabi::{Errno, FileType, Mode, Stat};

use crate::api::FileInfo;
use crate::backend::{BResult, BackendError, Sandbox, WriteOpts, join_path};

/// Profundidade máxima da remoção recursiva (o `rm -r` do GNU não tem, mas o host precisa de teto).
const MAX_DEPTH: usize = 512;

pub fn kind_name(t: FileType) -> &'static str {
    match t {
        FileType::Regular => "file",
        FileType::Directory => "dir",
        FileType::Symlink => "symlink",
        FileType::Fifo => "fifo",
        FileType::CharDevice => "char",
        FileType::BlockDevice => "block",
        FileType::Socket => "socket",
    }
}

pub fn abs(workdir: &str, path: &str) -> Vec<u8> {
    join_path(workdir.as_bytes(), path.as_bytes())
}

pub fn file_info(sb: &dyn Sandbox, name: &[u8], full: &[u8], st: &Stat) -> FileInfo {
    let symlink_target = (st.file_type() == FileType::Symlink)
        .then(|| sb.readlink(full).ok().map(|t| String::from_utf8_lossy(&t).into_owned()))
        .flatten();
    FileInfo {
        name: String::from_utf8_lossy(name).into_owned(),
        kind: kind_name(st.file_type()).to_string(),
        size: st.size,
        mode: st.mode & 0o7777,
        uid: st.uid,
        gid: st.gid,
        nlink: st.nlink,
        mtime: st.mtime.sec,
        mtime_nsec: st.mtime.nsec,
        symlink_target,
    }
}

fn basename(p: &[u8]) -> &[u8] {
    let t = p.strip_suffix(b"/").unwrap_or(p);
    t.rsplit(|b| *b == b'/').next().filter(|s| !s.is_empty()).unwrap_or(b"/")
}

pub fn stat(sb: &dyn Sandbox, path: &[u8], follow: bool) -> BResult<FileInfo> {
    let st = sb.stat(path, follow)?;
    Ok(file_info(sb, basename(path), path, &st))
}

pub fn list(sb: &dyn Sandbox, path: &[u8]) -> BResult<Vec<FileInfo>> {
    let entries = sb.read_dir(path)?;
    let mut out = Vec::with_capacity(entries.len());
    for e in entries {
        let full = join_path(path, &e.name);
        match sb.stat(&full, false) {
            Ok(st) => out.push(file_info(sb, &e.name, &full, &st)),
            // Sumiu entre o readdir e o stat (outro processo apagou): fica de fora, como no `ls`.
            Err(BackendError::Os { errno, .. }) if errno == Errno::ENOENT => {}
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

/// Lê até `max` bytes. Devolve os dados, o tamanho do arquivo e se chegou ao fim.
pub fn read(sb: &dyn Sandbox, path: &[u8], offset: u64, max: usize) -> BResult<(Vec<u8>, u64, bool)> {
    let st = sb.stat(path, true)?;
    match st.file_type() {
        FileType::Directory => return Err(BackendError::os(Errno::EISDIR, String::from_utf8_lossy(path))),
        FileType::Regular => {}
        _ => return Err(BackendError::os(Errno::EINVAL, String::from_utf8_lossy(path))),
    }
    let data = sb.read_file(path, offset, max)?;
    let eof = offset.saturating_add(data.len() as u64) >= st.size;
    Ok((data, st.size, eof))
}

/// `mkdir -p`: cria cada componente que falta; componente existente que não é diretório é ENOTDIR.
pub fn mkdir_p(sb: &dyn Sandbox, path: &[u8], mode: Mode) -> BResult<()> {
    let mut cur = Vec::with_capacity(path.len());
    for comp in path.split(|b| *b == b'/').filter(|c| !c.is_empty()) {
        cur.push(b'/');
        cur.extend_from_slice(comp);
        match sb.mkdir(&cur, mode) {
            Ok(()) => {}
            Err(BackendError::Os { errno, .. }) if errno == Errno::EEXIST => {
                let st = sb.stat(&cur, true)?;
                if st.file_type() != FileType::Directory {
                    return Err(BackendError::os(Errno::ENOTDIR, String::from_utf8_lossy(&cur)));
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

pub fn parent(path: &[u8]) -> Option<&[u8]> {
    let t = path.strip_suffix(b"/").unwrap_or(path);
    let i = t.iter().rposition(|b| *b == b'/')?;
    Some(if i == 0 { b"/" } else { &t[..i] })
}

pub fn write(sb: &dyn Sandbox, path: &[u8], data: &[u8], opts: WriteOpts, create_parents: bool) -> BResult<()> {
    if create_parents && let Some(p) = parent(path) {
        mkdir_p(sb, p, 0o755)?;
    }
    sb.write_file(path, data, opts)
}

/// `rm` (com `recursive`, `rm -r`). Não segue symlinks: remove o link.
pub fn remove(sb: &dyn Sandbox, path: &[u8], recursive: bool, force: bool) -> BResult<()> {
    let st = match sb.stat(path, false) {
        Ok(st) => st,
        Err(BackendError::Os { errno, .. }) if errno == Errno::ENOENT && force => return Ok(()),
        Err(e) => return Err(e),
    };
    if st.file_type() != FileType::Directory {
        return sb.unlink(path);
    }
    if !recursive {
        return sb.rmdir(path);
    }
    remove_tree(sb, path, 0)
}

fn remove_tree(sb: &dyn Sandbox, dir: &[u8], depth: usize) -> BResult<()> {
    if depth > MAX_DEPTH {
        return Err(BackendError::os(Errno::ELOOP, String::from_utf8_lossy(dir)));
    }
    for e in sb.read_dir(dir)? {
        let full = join_path(dir, &e.name);
        if e.kind == FileType::Directory {
            remove_tree(sb, &full, depth + 1)?;
        } else {
            match sb.unlink(&full) {
                Ok(()) => {}
                Err(BackendError::Os { errno, .. }) if errno == Errno::ENOENT => {}
                Err(e) => return Err(e),
            }
        }
    }
    sb.rmdir(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::test_sandbox;

    const W: WriteOpts = WriteOpts { append: false, exclusive: false, mode: 0o644 };

    #[test]
    fn write_read_list_stat_remove() {
        let sb = test_sandbox();
        let sb = &*sb;
        write(sb, b"/work/a/b/c.txt", b"hello", W, true).unwrap();
        write(sb, b"/work/a/b/c.txt", b" world", WriteOpts { append: true, ..W }, false).unwrap();
        let (d, size, eof) = read(sb, b"/work/a/b/c.txt", 0, 1 << 20).unwrap();
        assert_eq!((d.as_slice(), size, eof), (&b"hello world"[..], 11, true));
        let (d, _, eof) = read(sb, b"/work/a/b/c.txt", 6, 3).unwrap();
        assert_eq!((d.as_slice(), eof), (&b"wor"[..], false));
        sb.symlink(b"b/c.txt", b"/work/a/link").unwrap();
        let names: Vec<_> = list(sb, b"/work/a").unwrap().into_iter().map(|f| (f.name, f.kind, f.symlink_target)).collect();
        assert!(names.contains(&("link".into(), "symlink".into(), Some("b/c.txt".into()))), "{names:?}");
        assert_eq!(stat(sb, b"/work/a/link", true).unwrap().kind, "file");
        assert_eq!(stat(sb, b"/work/a/b/", false).unwrap().name, "b");
        let e = write(sb, b"/work/a/b/c.txt", b"x", WriteOpts { exclusive: true, ..W }, false).unwrap_err();
        assert_eq!(e.to_string(), "/work/a/b/c.txt: File exists");
        assert!(remove(sb, b"/work/a", false, false).is_err());
        remove(sb, b"/work/a", true, false).unwrap();
        assert!(sb.stat(b"/work/a", false).is_err());
        remove(sb, b"/work/a", true, true).unwrap();
        let e = remove(sb, b"/work/a", true, false).unwrap_err();
        assert_eq!(e.to_string(), "/work/a: No such file or directory");
    }

    #[test]
    fn mkdir_p_over_file_is_enotdir() {
        let sb = test_sandbox();
        write(&*sb, b"/work/f", b"", W, false).unwrap();
        let e = mkdir_p(&*sb, b"/work/f/g", 0o755).unwrap_err();
        assert_eq!(e.to_string(), "/work/f: Not a directory");
        mkdir_p(&*sb, b"/work/x/y/z", 0o755).unwrap();
        mkdir_p(&*sb, b"/work/x/y/z", 0o755).unwrap();
        assert_eq!(read(&*sb, b"/work/x", 0, 10).unwrap_err().to_string(), "/work/x: Is a directory");
    }

    #[test]
    fn parents() {
        assert_eq!(parent(b"/a/b"), Some(&b"/a"[..]));
        assert_eq!(parent(b"/a"), Some(&b"/"[..]));
        assert_eq!(parent(b"/a/b/"), Some(&b"/a"[..]));
        assert_eq!(basename(b"/a/b/"), b"b");
        assert_eq!(basename(b"/"), b"/");
    }
}
