//! `export` e `import` em tar (formato GNU, com nomes longos), em cima das primitivas de arquivo.
//!
//! - **export** percorre a árvore com `lstat` (não segue symlink), guarda dono, modo e mtime, e
//!   reconhece hardlinks pelo número do inode. Exportando `/`, pula os sistemas de arquivos virtuais
//!   (`/proc`, `/sys`, `/dev`) e os arquivos de controle das sessões. Dispositivos e sockets ficam de
//!   fora e são contados.
//! - **import** faz o que o GNU tar faz por padrão: tira a `/` do começo dos nomes, recusa `..`, troca
//!   arquivo existente, cria os diretórios que faltam e aplica a mtime dos diretórios no fim (criar um
//!   filho mudaria a mtime do pai).
//!
//! Os dois têm teto de bytes; passar dele é erro, nunca truncamento silencioso.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{Cursor, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use sysabi::{Errno, FileType, TimeSpec, mode};
use tar::{Archive, Builder, EntryType, Header};

use crate::backend::{BResult, BackendError, Sandbox, WriteOpts, join_path};
use crate::fsops;
use crate::session::SESSION_ROOT;

const MAX_DEPTH: usize = 512;
const SKIP_AT_ROOT: &[&[u8]] = &[b"/proc", b"/sys", b"/dev", SESSION_ROOT.as_bytes()];

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TarReport {
    pub files: u64,
    pub dirs: u64,
    pub symlinks: u64,
    pub hardlinks: u64,
    pub fifos: u64,
    /// Entradas que não entram (dispositivos, sockets, tipos de tar desconhecidos).
    pub skipped: u64,
    /// Bytes de conteúdo de arquivo.
    pub bytes: u64,
}

fn too_large(max: u64) -> BackendError {
    BackendError::Limit(format!("o tar passou do teto de {max} bytes"))
}

fn tar_err(e: std::io::Error) -> BackendError {
    match e.raw_os_error() {
        Some(n) => BackendError::os(Errno(n), "tar"),
        None => BackendError::Internal(format!("tar: {e}")),
    }
}

/// Exporta `root` (diretório ou arquivo). Os nomes no tar são relativos a `root`.
pub fn export(sb: &dyn Sandbox, root: &[u8], max_bytes: u64) -> BResult<(Vec<u8>, TarReport)> {
    let mut b = Builder::new(Vec::new());
    b.mode(tar::HeaderMode::Complete);
    let mut report = TarReport::default();
    let mut links: HashMap<u64, Vec<u8>> = HashMap::new();
    let st = sb.stat(root, false)?;
    if st.file_type() != FileType::Directory {
        let name = root.rsplit(|c| *c == b'/').next().unwrap_or(root).to_vec();
        add(sb, &mut b, root, &name, &st, &mut links, &mut report, max_bytes)?;
    } else {
        let mut stack: Vec<(Vec<u8>, Vec<u8>, usize)> = vec![(root.to_vec(), Vec::new(), 0)];
        let at_root = root == b"/";
        while let Some((dir, rel, depth)) = stack.pop() {
            if depth > MAX_DEPTH {
                return Err(BackendError::os(Errno::ELOOP, String::from_utf8_lossy(&dir)));
            }
            let mut entries = sb.read_dir(&dir)?;
            entries.sort_by(|a, b| a.name.cmp(&b.name));
            for e in entries.into_iter().rev() {
                let full = join_path(&dir, &e.name);
                if at_root && SKIP_AT_ROOT.contains(&full.as_slice()) {
                    continue;
                }
                let mut r = rel.clone();
                if !r.is_empty() {
                    r.push(b'/');
                }
                r.extend_from_slice(&e.name);
                let st = match sb.stat(&full, false) {
                    Ok(st) => st,
                    Err(BackendError::Os { errno, .. }) if errno == Errno::ENOENT => continue,
                    Err(err) => return Err(err),
                };
                add(sb, &mut b, &full, &r, &st, &mut links, &mut report, max_bytes)?;
                if st.file_type() == FileType::Directory {
                    stack.push((full, r, depth + 1));
                }
            }
        }
    }
    let data = b.into_inner().map_err(tar_err)?;
    if data.len() as u64 > max_bytes {
        return Err(too_large(max_bytes));
    }
    Ok((data, report))
}

#[allow(clippy::too_many_arguments)]
fn add(
    sb: &dyn Sandbox,
    b: &mut Builder<Vec<u8>>,
    full: &[u8],
    rel: &[u8],
    st: &sysabi::Stat,
    links: &mut HashMap<u64, Vec<u8>>,
    report: &mut TarReport,
    max_bytes: u64,
) -> BResult<()> {
    let path = Path::new(OsStr::from_bytes(rel));
    // mtime com a fração de segundo (o campo do cabeçalho só guarda segundos): extensão PAX.
    let pax_mtime = format!("{}.{:09}", st.mtime.sec.max(0), st.mtime.nsec.min(999_999_999));
    b.append_pax_extensions([("mtime", pax_mtime.as_bytes())]).map_err(tar_err)?;
    let mut h = Header::new_gnu();
    h.set_mode(st.mode & 0o7777);
    h.set_uid(u64::from(st.uid));
    h.set_gid(u64::from(st.gid));
    h.set_mtime(st.mtime.sec.max(0) as u64);
    h.set_size(0);
    match st.file_type() {
        FileType::Directory => {
            h.set_entry_type(EntryType::Directory);
            b.append_data(&mut h, path, std::io::empty()).map_err(tar_err)?;
            report.dirs += 1;
        }
        FileType::Symlink => {
            let target = sb.readlink(full)?;
            h.set_entry_type(EntryType::Symlink);
            b.append_link(&mut h, path, Path::new(OsStr::from_bytes(&target))).map_err(tar_err)?;
            report.symlinks += 1;
        }
        FileType::Regular => {
            if st.nlink > 1 {
                if let Some(first) = links.get(&st.ino) {
                    h.set_entry_type(EntryType::Link);
                    b.append_link(&mut h, path, Path::new(OsStr::from_bytes(first))).map_err(tar_err)?;
                    report.hardlinks += 1;
                    return Ok(());
                }
                links.insert(st.ino, rel.to_vec());
            }
            report.bytes += st.size;
            if report.bytes > max_bytes {
                return Err(too_large(max_bytes));
            }
            let data = sb.read_file(full, 0, st.size as usize)?;
            h.set_entry_type(EntryType::Regular);
            h.set_size(data.len() as u64);
            b.append_data(&mut h, path, data.as_slice()).map_err(tar_err)?;
            report.files += 1;
        }
        FileType::Fifo => {
            h.set_entry_type(EntryType::Fifo);
            b.append_data(&mut h, path, std::io::empty()).map_err(tar_err)?;
            report.fifos += 1;
        }
        FileType::CharDevice | FileType::BlockDevice | FileType::Socket => report.skipped += 1,
    }
    Ok(())
}

/// Nome de entrada saneado como o GNU tar: sem `/` no começo, sem `.`; `..` é recusado.
fn sanitize(name: &[u8]) -> BResult<Option<Vec<u8>>> {
    let mut out: Vec<u8> = Vec::with_capacity(name.len());
    for c in Path::new(OsStr::from_bytes(name)).components() {
        match c {
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
            Component::ParentDir => {
                return Err(BackendError::os(
                    Errno::EPERM,
                    format!("{}: o nome contém '..'", String::from_utf8_lossy(name)),
                ));
            }
            Component::Normal(s) => {
                if !out.is_empty() {
                    out.push(b'/');
                }
                out.extend_from_slice(s.as_bytes());
            }
        }
    }
    Ok((!out.is_empty()).then_some(out))
}

/// Apaga tudo da raiz menos os sistemas de arquivos virtuais: prepara uma sandbox nova pra receber o
/// tar de um snapshot persistido, de modo que o que tinha sido apagado continue apagado. (Os programas
/// embutidos são arquivos comuns reconhecidos pelo conteúdo, então voltam pelo próprio tar.)
pub fn wipe_root(sb: &dyn Sandbox) -> BResult<()> {
    for e in sb.read_dir(b"/")? {
        let full = join_path(b"/", &e.name);
        if SKIP_AT_ROOT.contains(&full.as_slice()) || full.as_slice() == b"/run" {
            if full.as_slice() == b"/run" {
                // /run fica, mas vazio (as sessões não sobrevivem à recuperação).
                for c in sb.read_dir(b"/run")? {
                    fsops::remove(sb, &join_path(b"/run", &c.name), true, true)?;
                }
            }
            continue;
        }
        match fsops::remove(sb, &full, true, true) {
            Ok(()) => {}
            // Ponto de montagem (o /work é um tmpfs à parte): fica, mas vazio.
            Err(BackendError::Os { errno, .. }) if errno == Errno::EBUSY => {
                for c in sb.read_dir(&full)? {
                    fsops::remove(sb, &join_path(&full, &c.name), true, true)?;
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Importa um tar em `dest` (criado se faltar).
pub fn import(sb: &dyn Sandbox, dest: &[u8], data: &[u8], max_bytes: u64) -> BResult<TarReport> {
    fsops::mkdir_p(sb, dest, 0o755)?;
    let mut ar = Archive::new(Cursor::new(data));
    let mut report = TarReport::default();
    let mut dir_times: Vec<(Vec<u8>, TimeSpec)> = Vec::new();
    for entry in ar.entries().map_err(tar_err)? {
        let mut entry = entry.map_err(tar_err)?;
        let raw_name = entry.path_bytes().into_owned();
        let Some(rel) = sanitize(&raw_name)? else { continue };
        let full = join_path(dest, &rel);
        let h = entry.header().clone();
        let perm = h.mode().map_err(tar_err)? & 0o7777;
        let mtime = h.mtime().map_err(tar_err)? as i64;
        let ids = (h.uid().ok(), h.gid().ok());
        if let Some(p) = fsops::parent(&full) {
            fsops::mkdir_p(sb, p, 0o755)?;
        }
        let remove_existing = |sb: &dyn Sandbox| match sb.stat(&full, false) {
            Ok(st) if st.file_type() == FileType::Directory => Ok(()),
            Ok(_) => sb.unlink(&full),
            Err(BackendError::Os { errno, .. }) if errno == Errno::ENOENT => Ok(()),
            Err(e) => Err(e),
        };
        let mut nsec = 0u32;
        let mut mtime = mtime;
        if let Ok(Some(exts)) = entry.pax_extensions() {
            for x in exts.flatten() {
                if x.key_bytes() == b"mtime"
                    && let Ok(v) = std::str::from_utf8(x.value_bytes())
                {
                    let (s, f) = v.split_once('.').unwrap_or((v, ""));
                    if let Ok(s) = s.parse::<i64>() {
                        mtime = s;
                        let digits: String = f.chars().take(9).collect();
                        nsec = format!("{digits:0<9}").parse().unwrap_or(0);
                    }
                }
            }
        }
        let ts = TimeSpec { sec: mtime, nsec };
        match h.entry_type() {
            EntryType::Directory => {
                fsops::mkdir_p(sb, &full, 0o755)?;
                sb.chmod(&full, perm)?;
                dir_times.push((full.clone(), ts));
                report.dirs += 1;
            }
            EntryType::Regular | EntryType::Continuous | EntryType::GNUSparse => {
                let size = entry.size();
                report.bytes += size;
                if report.bytes > max_bytes {
                    return Err(too_large(max_bytes));
                }
                let mut content = Vec::new();
                content.try_reserve(size as usize).map_err(|_| too_large(max_bytes))?;
                entry.read_to_end(&mut content).map_err(tar_err)?;
                remove_existing(sb)?;
                sb.write_file(&full, &content, WriteOpts { append: false, exclusive: false, mode: perm })?;
                sb.chmod(&full, perm)?;
                sb.set_times(&full, ts, ts, true)?;
                report.files += 1;
            }
            EntryType::Symlink => {
                let target = entry.link_name_bytes().map(|t| t.into_owned()).unwrap_or_default();
                remove_existing(sb)?;
                sb.symlink(&target, &full)?;
                let _ = sb.set_times(&full, ts, ts, false);
                report.symlinks += 1;
            }
            EntryType::Link => {
                let target = entry.link_name_bytes().map(|t| t.into_owned()).unwrap_or_default();
                let Some(trel) = sanitize(&target)? else { continue };
                remove_existing(sb)?;
                sb.link(&join_path(dest, &trel), &full)?;
                report.hardlinks += 1;
            }
            EntryType::Fifo => {
                remove_existing(sb)?;
                sb.mknod(&full, mode::S_IFIFO | perm, 0)?;
                report.fifos += 1;
            }
            EntryType::XGlobalHeader | EntryType::XHeader | EntryType::GNULongName | EntryType::GNULongLink => {}
            _ => {
                report.skipped += 1;
                continue;
            }
        }
        if let (Some(uid), Some(gid)) = ids
            && h.entry_type() != EntryType::Link
        {
            let follow = h.entry_type() != EntryType::Symlink;
            let _ = sb.chown(&full, uid as u32, gid as u32, follow);
        }
    }
    for (dir, ts) in dir_times.into_iter().rev() {
        sb.set_times(&dir, ts, ts, true)?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::test_sandbox;

    const W: WriteOpts = WriteOpts { append: false, exclusive: false, mode: 0o640 };

    #[test]
    fn subsecond_mtime_survives_the_round_trip() {
        let a = test_sandbox();
        fsops::write(&*a, b"/work/p/b.txt", b"conteudo", W, true).unwrap();
        let t = TimeSpec { sec: 1_700_000_000, nsec: 123_456_789 };
        a.set_times(b"/work/p/b.txt", t, t, true).unwrap();
        let (tar, _) = export(&*a, b"/work/p", 1 << 20).unwrap();
        let b = test_sandbox();
        import(&*b, b"/work/q", &tar, 1 << 20).unwrap();
        let st = b.stat(b"/work/q/b.txt", false).unwrap();
        assert_eq!(st.mtime, t, "mtime perdeu a fração de segundo");
    }

    #[test]
    fn round_trip_between_sandboxes() {
        let a = test_sandbox();
        fsops::write(&*a, b"/work/p/a.txt", b"conteudo", W, true).unwrap();
        fsops::write(&*a, b"/work/p/sub/b.bin", &[0, 1, 2, 255], W, true).unwrap();
        a.symlink(b"a.txt", b"/work/p/ln").unwrap();
        let long = format!("/work/p/{}", "n".repeat(150));
        fsops::write(&*a, long.as_bytes(), b"longo", W, false).unwrap();
        let t = TimeSpec { sec: 1_700_000_000, nsec: 0 };
        a.set_times(b"/work/p/a.txt", t, t, true).unwrap();
        let (tar, rep) = export(&*a, b"/work/p", 1 << 20).unwrap();
        assert_eq!((rep.files, rep.symlinks, rep.dirs), (3, 1, 1));

        let b = test_sandbox();
        let rep = import(&*b, b"/work/q", &tar, 1 << 20).unwrap();
        assert_eq!((rep.files, rep.symlinks, rep.dirs), (3, 1, 1));
        assert_eq!(b.read_file(b"/work/q/a.txt", 0, 100).unwrap(), b"conteudo");
        assert_eq!(b.read_file(b"/work/q/sub/b.bin", 0, 100).unwrap(), [0, 1, 2, 255]);
        assert_eq!(b.readlink(b"/work/q/ln").unwrap(), b"a.txt");
        let st = b.stat(b"/work/q/a.txt", false).unwrap();
        assert_eq!((st.perm(), st.mtime.sec), (0o640, 1_700_000_000));
        let long_q = format!("/work/q/{}", "n".repeat(150));
        assert_eq!(b.read_file(long_q.as_bytes(), 0, 100).unwrap(), b"longo");
    }

    #[test]
    fn limits_and_dotdot() {
        let a = test_sandbox();
        fsops::write(&*a, b"/work/big", &vec![7u8; 5000], W, false).unwrap();
        assert!(matches!(export(&*a, b"/work", 1000), Err(BackendError::Limit(_))));
        let (tar, _) = export(&*a, b"/work", 1 << 20).unwrap();
        let b = test_sandbox();
        assert!(matches!(import(&*b, b"/x", &tar, 1000), Err(BackendError::Limit(_))));

        // Tar com `../fora`: recusado.
        let mut builder = Builder::new(Vec::new());
        let mut h = Header::new_gnu();
        h.set_size(1);
        h.set_mode(0o644);
        h.set_entry_type(EntryType::Regular);
        {
            let name = b"../fora";
            let gnu = h.as_gnu_mut().unwrap();
            gnu.name[..name.len()].copy_from_slice(name);
        }
        h.set_cksum();
        builder.append(&h, &b"x"[..]).unwrap();
        let evil = builder.into_inner().unwrap();
        let e = import(&*b, b"/work/dest", &evil, 1 << 20).unwrap_err();
        assert!(e.to_string().contains("'..'"), "{e}");
        assert!(b.stat(b"/work/fora", false).is_err());
    }

    #[test]
    fn wipe_and_import_restores_deletions_too() {
        let a = test_sandbox();
        a.unlink(b"/usr/bin/echo").unwrap();
        fsops::write(&*a, b"/work/novo", b"n", W, false).unwrap();
        let (tar, _) = export(&*a, b"/", 1 << 24).unwrap();
        let b = test_sandbox();
        fsops::write(&*b, b"/work/lixo", b"x", W, false).unwrap();
        wipe_root(&*b).unwrap();
        import(&*b, b"/", &tar, 1 << 24).unwrap();
        assert!(b.stat(b"/usr/bin/echo", false).is_err(), "o apagado continua apagado");
        assert!(b.stat(b"/bin/echo", false).is_ok());
        assert!(b.stat(b"/work/lixo", false).is_err());
        assert_eq!(b.read_file(b"/work/novo", 0, 10).unwrap(), b"n");
        assert_eq!(b.stat(b"/root", false).unwrap().perm(), 0o700);
    }

    #[test]
    fn export_of_root_skips_virtual_fs() {
        let a = test_sandbox();
        fsops::write(&*a, b"/dev/x", b"", W, false).unwrap();
        fsops::write(&*a, b"/work/keep", b"k", W, false).unwrap();
        let (tar, _) = export(&*a, b"/", 1 << 24).unwrap();
        let mut ar = Archive::new(Cursor::new(tar));
        let names: Vec<String> =
            ar.entries().unwrap().map(|e| String::from_utf8_lossy(&e.unwrap().path_bytes()).into_owned()).collect();
        assert!(names.iter().any(|n| n == "work/keep"), "{names:?}");
        assert!(!names.iter().any(|n| n.starts_with("dev")), "{names:?}");
    }
}
