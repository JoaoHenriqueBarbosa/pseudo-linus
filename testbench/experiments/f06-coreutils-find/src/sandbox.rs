//! Monta o VFS do shim pra um caso da bancada: raiz mínima (`/etc/passwd`, `/etc/group`, `/tmp`,
//! `/root`) e a fixture em `/work/case`, com o relógio do caso (o `faketime` dele, ou o relógio do
//! host lido aqui, no "kernel", nunca pelo programa).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use harness::{Entry, MemTree};
use sysio::vfs::{NodeKind, TreeEntry, Vfs, epoch};

/// Primeiras linhas do `/etc/passwd` e do `/etc/group` do oráculo (Debian 13).
const PASSWD: &str = "root:x:0:0:root:/root:/bin/bash\n\
daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin\n\
bin:x:2:2:bin:/bin:/usr/sbin/nologin\n\
sys:x:3:3:sys:/dev:/usr/sbin/nologin\n\
nobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n";
const GROUP: &str = "root:x:0:\ndaemon:x:1:\nbin:x:2:\nsys:x:3:\nadm:x:4:\nnogroup:x:65534:\n";

pub struct Sandbox {
    pub vfs: Arc<Mutex<Vfs>>,
    pub now: SystemTime,
    pub case_dir: PathBuf,
}

impl Sandbox {
    pub fn for_case(files: &MemTree, faketime: Option<&str>) -> Sandbox {
        let now = faketime.and_then(parse_faketime).unwrap_or_else(SystemTime::now);
        let fixture_time = epoch(harness::FIXTURE_MTIME);
        let case_dir = PathBuf::from(harness::CASE_DIR);
        let mut vfs = Vfs::new(now);
        let tmp = vfs.ensure_dir(Path::new("/tmp"), 0o1777, now);
        vfs.inode_mut(tmp).perm = 0o1777;
        let etc = vfs.ensure_dir(Path::new("/etc"), 0o755, now);
        for (name, text) in [("passwd", PASSWD), ("group", GROUP)] {
            let ino = vfs.create_file(etc, OsStr::new(name), 0o644, now).expect("criar /etc");
            if let NodeKind::File(d) = &mut vfs.inode_mut(ino).kind {
                *d = text.as_bytes().to_vec();
            }
        }
        vfs.ensure_dir(Path::new("/root"), 0o700, now);
        let entries = files.entries.iter().map(|(rel, e)| (rel.as_str(), to_tree_entry(e)));
        vfs.load_tree(&case_dir, entries, fixture_time, now);
        Sandbox { vfs: Arc::new(Mutex::new(vfs)), now, case_dir }
    }

    /// Retrato do diretório do caso no formato da bancada.
    pub fn snapshot(&self) -> MemTree {
        let vfs = sysio::proc::lock(&self.vfs);
        let mut tree = MemTree::new();
        for (rel, entry) in vfs.snapshot(&self.case_dir) {
            let e = match entry {
                TreeEntry::Dir { mode } => Entry::dir(mode),
                TreeEntry::File { mode, data } => Entry::file(data, mode),
                TreeEntry::Symlink { target } => Entry::symlink(target),
            };
            tree.entries.insert(rel, e);
        }
        tree
    }
}

fn to_tree_entry(e: &Entry) -> TreeEntry {
    match e {
        Entry::Dir { mode } => TreeEntry::Dir { mode: *mode },
        Entry::File { mode, data, .. } => TreeEntry::File {
            mode: *mode,
            data: data.as_ref().map(|b| b.0.clone()).unwrap_or_default(),
        },
        Entry::Symlink { target } => TreeEntry::Symlink { target: target.clone() },
    }
}

/// "AAAA-MM-DD HH:MM:SS" em UTC (o TZ dos casos) pra `SystemTime`.
pub fn parse_faketime(s: &str) -> Option<SystemTime> {
    let (date, time) = s.trim().split_once(' ')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.split(':').map(|x| x.parse::<i64>());
    let (hh, mm, ss) = (t.next()?.ok()?, t.next()?.ok()?, t.next().unwrap_or(Ok(0)).ok()?);
    let days = days_from_civil(y, m, day);
    let secs = days * 86_400 + hh * 3600 + mm * 60 + ss;
    u64::try_from(secs).ok().map(epoch)
}

/// Algoritmo de Howard Hinnant: dias desde 1970-01-01 no calendário gregoriano proléptico.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faketime_parses_as_utc() {
        assert_eq!(parse_faketime("2026-01-15 12:00:00"), Some(epoch(harness::FIXTURE_MTIME)));
        assert_eq!(parse_faketime("1970-01-01 00:00:00"), Some(epoch(0)));
    }

    #[test]
    fn sandbox_roundtrip_keeps_fixture() {
        let mut tree = MemTree::new();
        tree.insert("a/b.txt", Entry::file("x", 0o600));
        tree.insert("l", Entry::symlink("a/b.txt"));
        let sb = Sandbox::for_case(&tree, None);
        assert_eq!(sb.snapshot(), tree);
    }
}
