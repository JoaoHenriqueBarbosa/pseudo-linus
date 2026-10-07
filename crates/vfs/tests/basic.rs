//! Cenários básicos do VFS sobre o tmpfs, com os errnos do Linux escritos à mão.

use std::sync::Arc;

use vfs::tmpfs::{Tmpfs, TmpfsLimits};
use vfs::*;

fn setup() -> (Arc<Namespace>, Caller) {
    let now = TimeSpec { sec: 1_768_478_400, nsec: 0 };
    let fs = Tmpfs::new(makedev(0, 30), 0o755, now, TmpfsLimits::default());
    let ns = Namespace::new(fs, MountFlags::RELATIME, "tmpfs", "");
    let mut cx = ops::kernel_caller(ns.root.root());
    cx.now = now;
    cx.umask = 0o022;
    cx.pid = 1;
    (ns, cx)
}

fn write_file(ns: &Namespace, cx: &Caller, path: &str, data: &[u8]) {
    let o = ns.open(cx, &Start::Cwd, path.as_bytes(), OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC, 0o644).unwrap();
    match o {
        Opened::File { handle, .. } => {
            let (n, _) = handle.write(cx, WritePos::At(0), data).unwrap();
            assert_eq!(n, data.len());
        }
        other => panic!("{other:?}"),
    }
}

fn read_file(ns: &Namespace, cx: &Caller, path: &str) -> Result<Vec<u8>, Errno> {
    match ns.open(cx, &Start::Cwd, path.as_bytes(), OFlags::RDONLY, 0)? {
        Opened::File { handle, .. } => {
            let mut buf = vec![0u8; 1 << 16];
            let n = handle.read(cx, 0, &mut buf)?;
            buf.truncate(n);
            Ok(buf)
        }
        other => panic!("{other:?}"),
    }
}

fn names(ns: &Namespace, cx: &Caller, path: &str) -> Vec<String> {
    match ns.open(cx, &Start::Cwd, path.as_bytes(), OFlags::RDONLY | OFlags::DIRECTORY, 0).unwrap() {
        Opened::File { handle, .. } => {
            let (ents, _) = handle.readdir(cx, 0, 1000).unwrap();
            ents.into_iter().map(|e| String::from_utf8(e.name).unwrap()).collect()
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn create_read_and_readdir_newest_first() {
    let (ns, cx) = setup();
    ns.mkdir(&cx, &Start::Cwd, b"/d", 0o777).unwrap();
    write_file(&ns, &cx, "/d/a", b"aaa");
    write_file(&ns, &cx, "/d/b", b"bb");
    write_file(&ns, &cx, "/d/c", b"c");
    assert_eq!(read_file(&ns, &cx, "/d/b").unwrap(), b"bb");
    assert_eq!(names(&ns, &cx, "/d"), [".", "..", "c", "b", "a"]);
    // rename põe a entrada como a mais nova.
    ns.rename(&cx, &Start::Cwd, b"/d/a", &Start::Cwd, b"/d/z", RenameFlags::empty()).unwrap();
    assert_eq!(names(&ns, &cx, "/d"), [".", "..", "z", "c", "b"]);
    let st = ns.stat(&cx, &Start::Cwd, b"/d", AtFlags::empty()).unwrap().stat();
    assert_eq!(st.mode, S_IFDIR | 0o755, "umask 022 aplicada");
    assert_eq!(st.size, 40 + 3 * 20);
    assert_eq!(st.nlink, 2);
}

#[test]
fn linux_errnos_for_common_mistakes() {
    let (ns, cx) = setup();
    write_file(&ns, &cx, "/f", b"x");
    ns.mkdir(&cx, &Start::Cwd, b"/e", 0o755).unwrap();
    write_file(&ns, &cx, "/e/x", b"");
    let s = &Start::Cwd;
    assert_eq!(ns.mkdir(&cx, s, b"/f", 0o755), Err(Errno::EEXIST));
    assert_eq!(read_file(&ns, &cx, "/missing").unwrap_err(), Errno::ENOENT);
    assert_eq!(read_file(&ns, &cx, "/f/x").unwrap_err(), Errno::ENOTDIR);
    assert_eq!(ns.stat(&cx, s, b"/f/", AtFlags::empty()).unwrap_err(), Errno::ENOTDIR);
    assert_eq!(ns.unlink(&cx, s, b"/e", AtFlags::REMOVEDIR), Err(Errno::ENOTEMPTY));
    assert_eq!(ns.unlink(&cx, s, b"/e", AtFlags::empty()), Err(Errno::EISDIR));
    assert_eq!(ns.unlink(&cx, s, b"/f", AtFlags::REMOVEDIR), Err(Errno::ENOTDIR));
    assert_eq!(ns.unlink(&cx, s, b"/f/", AtFlags::empty()), Err(Errno::ENOTDIR));
    assert_eq!(ns.unlink(&cx, s, b"/", AtFlags::REMOVEDIR), Err(Errno::EBUSY));
    assert_eq!(ns.unlink(&cx, s, b"/e/.", AtFlags::REMOVEDIR), Err(Errno::EINVAL));
    assert_eq!(ns.unlink(&cx, s, b"/e/..", AtFlags::REMOVEDIR), Err(Errno::ENOTEMPTY));
    let long = format!("/{}", "a".repeat(256));
    assert_eq!(ns.stat(&cx, s, long.as_bytes(), AtFlags::empty()).unwrap_err(), Errno::ENAMETOOLONG);
    assert_eq!(ns.stat(&cx, s, b"", AtFlags::empty()).unwrap_err(), Errno::ENOENT);
    assert_eq!(ns.rename(&cx, s, b"/e", s, b"/e/sub", RenameFlags::empty()), Err(Errno::EINVAL));
    assert!(matches!(ns.open(&cx, s, b"/e", OFlags::WRONLY, 0), Err(Errno::EISDIR)));
    assert!(matches!(ns.open(&cx, s, b"/new/", OFlags::WRONLY | OFlags::CREAT, 0o644), Err(Errno::EISDIR)));
    assert!(matches!(ns.open(&cx, s, b"/f", OFlags::WRONLY | OFlags::CREAT | OFlags::EXCL, 0o644), Err(Errno::EEXIST)));
}

#[test]
fn symlinks_follow_limits_and_dangling_creation() {
    let (ns, cx) = setup();
    let s = &Start::Cwd;
    ns.mkdir(&cx, s, b"/d", 0o755).unwrap();
    write_file(&ns, &cx, "/d/target", b"hello");
    ns.symlink(&cx, b"d/target", s, b"/rel").unwrap();
    ns.symlink(&cx, b"/d", s, b"/abs").unwrap();
    assert_eq!(read_file(&ns, &cx, "/rel").unwrap(), b"hello");
    assert_eq!(read_file(&ns, &cx, "/abs/target").unwrap(), b"hello");
    assert_eq!(ns.readlink(&cx, s, b"/rel").unwrap(), b"d/target");
    let lst = ns.stat(&cx, s, b"/rel", AtFlags::SYMLINK_NOFOLLOW).unwrap().stat();
    assert_eq!(lst.mode, S_IFLNK | 0o777);
    assert_eq!(lst.size, 8);
    // Cadeia de 40 resolve; 41 dá ELOOP.
    ns.symlink(&cx, b"/d/target", s, b"/l0").unwrap();
    for i in 1..=41 {
        ns.symlink(&cx, format!("/l{}", i - 1).as_bytes(), s, format!("/l{i}").as_bytes()).unwrap();
    }
    assert_eq!(read_file(&ns, &cx, "/l39").unwrap(), b"hello");
    assert_eq!(read_file(&ns, &cx, "/l40").unwrap_err(), Errno::ELOOP);
    // Symlink pendurado com O_CREAT cria o alvo.
    ns.symlink(&cx, b"/d/created", s, b"/dangling").unwrap();
    write_file(&ns, &cx, "/dangling", b"new");
    assert_eq!(read_file(&ns, &cx, "/d/created").unwrap(), b"new");
    // O_NOFOLLOW no symlink: ELOOP.
    assert!(matches!(ns.open(&cx, s, b"/rel", OFlags::RDONLY | OFlags::NOFOLLOW, 0), Err(Errno::ELOOP)));
    // Barra no fim segue o symlink.
    let st = ns.stat(&cx, s, b"/abs/", AtFlags::SYMLINK_NOFOLLOW).unwrap().stat();
    assert!(is_dir(st.mode));
}

#[test]
fn snapshot_restore_and_orphans() {
    let fs = Tmpfs::new(makedev(0, 31), 0o755, TimeSpec::default(), TmpfsLimits::default());
    let ns = Namespace::new(fs.clone(), MountFlags::empty(), "tmpfs", "");
    let cx = ops::kernel_caller(ns.root.root());
    write_file(&ns, &cx, "/a", b"one");
    let snap = fs.snapshot();
    write_file(&ns, &cx, "/a", b"two");
    write_file(&ns, &cx, "/b", b"bee");
    assert_eq!(read_file(&ns, &cx, "/a").unwrap(), b"two");
    fs.restore(&snap);
    assert_eq!(read_file(&ns, &cx, "/a").unwrap(), b"one");
    assert_eq!(read_file(&ns, &cx, "/b").unwrap_err(), Errno::ENOENT);
    // Arquivo aberto e removido continua legível; some no fechamento.
    let h = match ns.open(&cx, &Start::Cwd, b"/a", OFlags::RDWR, 0).unwrap() {
        Opened::File { handle, loc, .. } => (handle, loc),
        _ => unreachable!(),
    };
    ns.unlink(&cx, &Start::Cwd, b"/a", AtFlags::empty()).unwrap();
    let st = ns.stat_loc(&cx, &h.1).unwrap();
    assert_eq!(st.nlink, 0);
    let mut buf = [0u8; 8];
    assert_eq!(h.0.read(&cx, 0, &mut buf).unwrap(), 3);
    let ino = h.1.ino;
    drop(h);
    assert_eq!(fs.getattr(&cx, ino).unwrap_err(), Errno::ESTALE);
}

#[test]
fn sparse_files_and_truncate() {
    let (ns, cx) = setup();
    let h = match ns.open(&cx, &Start::Cwd, b"/s", OFlags::RDWR | OFlags::CREAT, 0o600).unwrap() {
        Opened::File { handle, loc, .. } => (handle, loc),
        _ => unreachable!(),
    };
    h.0.write(&cx, WritePos::At(10_000), b"end").unwrap();
    let st = ns.stat_loc(&cx, &h.1).unwrap();
    assert_eq!(st.size, 10_003);
    assert_eq!(st.blocks, 8, "só a página escrita conta");
    assert_eq!(h.0.seek_data(&cx, 0, false).unwrap(), 8192);
    assert_eq!(h.0.seek_data(&cx, 0, true).unwrap(), 0);
    let mut buf = vec![1u8; 10_003];
    assert_eq!(h.0.read(&cx, 0, &mut buf).unwrap(), 10_003);
    assert!(buf[..10_000].iter().all(|b| *b == 0));
    assert_eq!(&buf[10_000..], b"end");
    ns.truncate_loc(&cx, &h.1, 10_001, false).unwrap();
    ns.truncate_loc(&cx, &h.1, 20_000, false).unwrap();
    let mut buf = vec![1u8; 3];
    assert_eq!(h.0.read(&cx, 10_000, &mut buf).unwrap(), 3);
    assert_eq!(buf, [b'e', 0, 0], "truncar e estender zera o que foi cortado");
}

#[test]
fn permissions_for_non_root() {
    let (ns, root) = setup();
    let s = &Start::Cwd;
    ns.mkdir(&root, s, b"/locked", 0o700).unwrap();
    write_file(&ns, &root, "/locked/f", b"secret");
    write_file(&ns, &root, "/zero", b"z");
    ns.chmod(&root, s, b"/zero", 0, AtFlags::empty()).unwrap();
    assert_eq!(read_file(&ns, &root, "/zero").unwrap(), b"z", "root lê modo 000");
    let mut user = root.clone();
    user.cred = Arc::new(Cred::new(1000, 1000, vec![1000]));
    assert_eq!(read_file(&ns, &user, "/locked/f").unwrap_err(), Errno::EACCES);
    assert_eq!(read_file(&ns, &user, "/zero").unwrap_err(), Errno::EACCES);
    assert_eq!(ns.chmod(&user, s, b"/zero", 0o777, AtFlags::empty()), Err(Errno::EPERM));
    assert_eq!(ns.mkdir(&user, s, b"/u", 0o755), Err(Errno::EACCES));
    // Sticky em /tmp.
    ns.mkdir(&root, s, b"/tmp", 0o777).unwrap();
    ns.chmod(&root, s, b"/tmp", 0o1777, AtFlags::empty()).unwrap();
    write_file(&ns, &root, "/tmp/rootfile", b"");
    assert_eq!(ns.unlink(&user, s, b"/tmp/rootfile", AtFlags::empty()), Err(Errno::EPERM));
}
