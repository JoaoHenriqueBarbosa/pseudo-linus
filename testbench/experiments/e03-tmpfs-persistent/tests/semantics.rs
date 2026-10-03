//! Semântica Linux em cenários explícitos: hardlink, unlink de arquivo aberto, rename sobre
//! destino existente, `.`/`..`, buracos e truncamento, e isolamento de snapshot.
//!
//! Cada passo tem o resultado esperado escrito à mão. O mesmo cenário roda no tmpfs de verdade do
//! host (o que prova que a expectativa é a do Linux), no modelo de referência e em todos os flavors.

use e03_tmpfs_persistent::check::{Model, Op, OpResult, Ours, Out, RealFs, Target};
use e03_tmpfs_persistent::maps::Flavor;
use e03_tmpfs_persistent::{Errno, Kind, for_each_content, for_each_final, for_each_structure};

struct Scenario {
    name: &'static str,
    /// Roda no tmpfs do host (falso pra snapshot, que o Linux não tem).
    real: bool,
    /// Roda no modelo (falso pra `.` e `..`, que o modelo não interpreta).
    model: bool,
    steps: Vec<(Op, OpResult)>,
}

fn p(s: &str) -> String {
    s.to_string()
}

fn ok() -> OpResult {
    Ok(Out::Unit)
}

fn err(e: Errno) -> OpResult {
    Err(e)
}

fn file(nlink: u32, size: u64) -> OpResult {
    Ok(Out::Stat { kind: Kind::File, nlink, size })
}

fn dir(nlink: u32, entries: u64) -> OpResult {
    Ok(Out::Stat { kind: Kind::Dir, nlink, size: 40 + 20 * entries })
}

fn bytes(b: &[u8]) -> OpResult {
    Ok(Out::Bytes(b.to_vec()))
}

fn names(n: &[&str]) -> OpResult {
    Ok(Out::Names(n.iter().map(|s| s.as_bytes().to_vec()).collect()))
}

fn scenarios() -> Vec<Scenario> {
    use Op::*;
    vec![
        Scenario {
            name: "hardlink",
            real: true,
            model: true,
            steps: vec![
                (Create(p("/a")), ok()),
                (Write(p("/a"), 0, b"hello".to_vec()), ok()),
                (Link(p("/a"), p("/b")), ok()),
                (Stat(p("/a")), file(2, 5)),
                (Write(p("/b"), 5, b" world".to_vec()), ok()),
                (Read(p("/a"), 0, 100), bytes(b"hello world")),
                (Unlink(p("/a")), ok()),
                (Stat(p("/b")), file(1, 11)),
                (Link(p("/b"), p("/b")), err(Errno::EEXIST)),
                (Mkdir(p("/d")), ok()),
                (Link(p("/d"), p("/e")), err(Errno::EPERM)),
                (Link(p("/d"), p("/b")), err(Errno::EEXIST)),
                (Link(p("/nope"), p("/x")), err(Errno::ENOENT)),
                (Link(p("/b"), p("/nodir/x")), err(Errno::ENOENT)),
                (Link(p("/b"), p("/b/x")), err(Errno::ENOTDIR)),
                (Link(p("/b"), p("/d/b2")), ok()),
                (Stat(p("/d/b2")), file(2, 11)),
                (Stat(p("/d")), dir(2, 1)),
            ],
        },
        Scenario {
            name: "unlink de arquivo aberto",
            real: true,
            model: true,
            steps: vec![
                (Create(p("/f")), ok()),
                (Write(p("/f"), 0, b"abc".to_vec()), ok()),
                (Open(p("/f")), ok()),
                (Open(p("/f")), ok()),
                (Unlink(p("/f")), ok()),
                (Stat(p("/f")), err(Errno::ENOENT)),
                (FStat(0), file(0, 3)),
                (PWrite(0, 3, b"def".to_vec()), ok()),
                (PRead(1, 0, 10), bytes(b"abcdef")),
                (Close(0), ok()),
                (PRead(0, 0, 10), bytes(b"abcdef")),
                (Create(p("/f")), ok()),
                (Stat(p("/f")), file(1, 0)),
                (FStat(0), file(0, 6)),
                (Close(0), ok()),
                (Readdir(p("/")), names(&["f"])),
            ],
        },
        Scenario {
            name: "rename sobre destino existente",
            real: true,
            model: true,
            steps: vec![
                (Create(p("/x")), ok()),
                (Write(p("/x"), 0, b"X".to_vec()), ok()),
                (Create(p("/y")), ok()),
                (Write(p("/y"), 0, b"YY".to_vec()), ok()),
                (Open(p("/y")), ok()),
                (Rename(p("/x"), p("/y")), ok()),
                (Read(p("/y"), 0, 10), bytes(b"X")),
                (Stat(p("/x")), err(Errno::ENOENT)),
                (FStat(0), file(0, 2)),
                (PRead(0, 0, 10), bytes(b"YY")),
                (Close(0), ok()),
                (Mkdir(p("/d1")), ok()),
                (Mkdir(p("/d2")), ok()),
                (Create(p("/d1/f")), ok()),
                (Rename(p("/d1"), p("/d2")), ok()),
                (Stat(p("/d2")), dir(2, 1)),
                (Stat(p("/d1")), err(Errno::ENOENT)),
                (Mkdir(p("/d3")), ok()),
                (Create(p("/d3/g")), ok()),
                (Rename(p("/d2"), p("/d3")), err(Errno::ENOTEMPTY)),
                (Create(p("/file")), ok()),
                (Rename(p("/file"), p("/d3")), err(Errno::EISDIR)),
                (Rename(p("/d3"), p("/file")), err(Errno::ENOTDIR)),
                (Mkdir(p("/d3/sub")), ok()),
                (Stat(p("/d3")), dir(3, 2)),
                (Rename(p("/d3"), p("/d3/sub/inner")), err(Errno::EINVAL)),
                (Rename(p("/d3"), p("/d3/sub")), err(Errno::EINVAL)),
                (Rename(p("/d3/sub"), p("/d3")), err(Errno::ENOTEMPTY)),
                (Rename(p("/d3/g"), p("/d3")), err(Errno::ENOTEMPTY)),
                (Link(p("/file"), p("/file2")), ok()),
                (Rename(p("/file"), p("/file2")), ok()),
                (Stat(p("/file")), file(2, 0)),
                (Rename(p("/d3"), p("/d3")), ok()),
                (Rename(p("/nope"), p("/z")), err(Errno::ENOENT)),
                (Rename(p("/file"), p("/nope/z")), err(Errno::ENOENT)),
                (Rename(p("/file"), p("/file2/z")), err(Errno::ENOTDIR)),
                (Mkdir(p("/p")), ok()),
                (Mkdir(p("/q")), ok()),
                (Mkdir(p("/p/c")), ok()),
                (Stat(p("/p")), dir(3, 1)),
                (Rename(p("/p/c"), p("/q/c")), ok()),
                (Stat(p("/p")), dir(2, 0)),
                (Stat(p("/q")), dir(3, 1)),
                (Mkdir(p("/q/c/e")), ok()),
                (Rename(p("/q/c"), p("/p")), ok()),
                (Stat(p("/q")), dir(2, 0)),
                (Stat(p("/p")), dir(3, 1)),
                (Readdir(p("/p")), names(&["e"])),
            ],
        },
        Scenario {
            name: "ponto e ponto-ponto",
            real: true,
            model: false,
            steps: vec![
                (Mkdir(p("/p")), ok()),
                (Mkdir(p("/q")), ok()),
                (Mkdir(p("/p/c")), ok()),
                (Rename(p("/p/c"), p("/q/c")), ok()),
                (Stat(p("/q/c/..")), dir(3, 1)),
                (Stat(p("/q/c/../../p")), dir(2, 0)),
                (Create(p("/q/c/../f")), ok()),
                (Stat(p("/q/f")), file(1, 0)),
                (Stat(p("/q/f/")), err(Errno::ENOTDIR)),
                (Stat(p("/q/c/")), dir(2, 0)),
                (Rmdir(p("/q/c/.")), err(Errno::EINVAL)),
                (Rmdir(p("/q/c/..")), err(Errno::ENOTEMPTY)),
                (Mkdir(p("/q/.")), err(Errno::EEXIST)),
                (Unlink(p("/q/..")), err(Errno::EISDIR)),
                (Rename(p("/q/c/."), p("/x")), err(Errno::EBUSY)),
                (Unlink(p("/")), err(Errno::EISDIR)),
                (Mkdir(p("/")), err(Errno::EEXIST)),
                (Create(p("/q/.")), err(Errno::EISDIR)),
                (Open(p("/q")), err(Errno::EISDIR)),
            ],
        },
        Scenario {
            // No alvo real "/" é um diretório comum dentro do /dev/shm, então rmdir dele não dá
            // EBUSY; este cenário fica só pro modelo e pro nosso.
            name: "raiz",
            real: false,
            model: true,
            steps: vec![
                (Rmdir(p("/")), err(Errno::EBUSY)),
                (Unlink(p("/")), err(Errno::EISDIR)),
                (Mkdir(p("/")), err(Errno::EEXIST)),
                (Stat(p("/")), dir(2, 0)),
            ],
        },
        Scenario {
            name: "buracos e truncamento",
            real: true,
            model: true,
            steps: vec![
                (Create(p("/h")), ok()),
                (Write(p("/h"), 10_000, b"Z".to_vec()), ok()),
                (Stat(p("/h")), file(1, 10_001)),
                (Read(p("/h"), 9_998, 5), bytes(&[0, 0, b'Z'])),
                (Read(p("/h"), 0, 4), bytes(&[0, 0, 0, 0])),
                (Write(p("/h"), 0, vec![0xAA; 8192]), ok()),
                (Truncate(p("/h"), 4100), ok()),
                (Truncate(p("/h"), 8192), ok()),
                (Read(p("/h"), 4094, 8), bytes(&[0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 0, 0])),
                (Truncate(p("/h"), 0), ok()),
                (Read(p("/h"), 0, 10), bytes(b"")),
                (Write(p("/h"), 4096, b"q".to_vec()), ok()),
                (Read(p("/h"), 4090, 10), bytes(&[0, 0, 0, 0, 0, 0, b'q'])),
                (Truncate(p("/"), 0), err(Errno::EISDIR)),
            ],
        },
        Scenario {
            name: "erros de rmdir, unlink e criação",
            real: true,
            model: true,
            steps: vec![
                (Mkdir(p("/m")), ok()),
                (Create(p("/m/f")), ok()),
                (Rmdir(p("/m")), err(Errno::ENOTEMPTY)),
                (Unlink(p("/m")), err(Errno::EISDIR)),
                (Rmdir(p("/m/f")), err(Errno::ENOTDIR)),
                (Unlink(p("/m/f")), ok()),
                (Rmdir(p("/m")), ok()),
                (Mkdir(p("/m/x")), err(Errno::ENOENT)),
                (Create(p("/m")), ok()),
                (Mkdir(p("/m")), err(Errno::EEXIST)),
                (Create(p("/m/x")), err(Errno::ENOTDIR)),
                (Write(p("/nope"), 0, b"a".to_vec()), err(Errno::ENOENT)),
                (Read(p("/m"), 0, 1), bytes(b"")),
                (Readdir(p("/m")), err(Errno::ENOTDIR)),
                (Mkdir(p("/k")), ok()),
                (Write(p("/k"), 0, b"a".to_vec()), err(Errno::EISDIR)),
                (Read(p("/k"), 0, 1), err(Errno::EISDIR)),
                (Create(p("/k")), err(Errno::EISDIR)),
                (Stat(p("/")), dir(3, 2)),
            ],
        },
        Scenario {
            name: "snapshot e restore",
            real: false,
            model: true,
            steps: vec![
                (Create(p("/s")), ok()),
                (Write(p("/s"), 0, b"v1".to_vec()), ok()),
                (Snapshot, ok()),
                (Write(p("/s"), 0, b"v2".to_vec()), ok()),
                (Create(p("/t")), ok()),
                (Restore(0), ok()),
                (Read(p("/s"), 0, 10), bytes(b"v1")),
                (Stat(p("/t")), err(Errno::ENOENT)),
                (Open(p("/s")), ok()),
                (Snapshot, ok()),
                (Unlink(p("/s")), ok()),
                (PWrite(0, 0, b"V3".to_vec()), ok()),
                (Restore(1), ok()),
                (Read(p("/s"), 0, 10), bytes(b"v1")),
                (PRead(0, 0, 10), bytes(b"v1")),
                (Create(p("/n")), ok()),
                (Open(p("/n")), ok()),
                (Restore(0), ok()),
                (FStat(1), err(Errno::ESTALE)),
                (Close(1), ok()),
                (FStat(0), file(1, 2)),
                (Close(0), ok()),
                (Readdir(p("/")), names(&["s"])),
            ],
        },
    ]
}

fn run(target: &mut dyn Target, sc: &Scenario, who: &str) {
    for (i, (op, want)) in sc.steps.iter().enumerate() {
        let got = target.apply(op);
        assert_eq!(&got, want, "{who}, cenário '{}', passo {i}: {op:?}", sc.name);
    }
}

#[test]
fn expectations_hold_on_linux_tmpfs() {
    for sc in scenarios().iter().filter(|s| s.real) {
        let mut real = RealFs::new().expect("diretório no tmpfs do host");
        run(&mut real, sc, "tmpfs do host");
    }
}

#[test]
fn model_matches_expectations() {
    for sc in scenarios().iter().filter(|s| s.model) {
        run(&mut Model::new(), sc, "modelo");
    }
}

fn run_flavor<F: Flavor>(key: &str) {
    for sc in scenarios() {
        let mut ours = Ours::<F>::new();
        run(&mut ours, &sc, key);
        let problems = ours.vfs.fsck();
        assert!(problems.is_empty(), "{key}, cenário '{}': {problems:?}", sc.name);
    }
}

#[test]
fn every_flavor_matches_expectations() {
    for_each_structure!(|meta, F| {
        run_flavor::<F>(meta.key);
    });
    for_each_content!(|meta, F| {
        run_flavor::<F>(meta.key);
    });
    for_each_final!(|meta, F| {
        run_flavor::<F>(meta.key);
    });
}

/// Inode desvinculado e aberto some da tabela no último close (e não antes).
fn orphan_lifecycle<F: Flavor>() {
    use e03_tmpfs_persistent::Vfs;
    let mut v = Vfs::<F>::new();
    v.create(b"/f", 0o644).expect("create");
    v.write(b"/f", 0, b"dados").expect("write");
    let base = v.fs().inode_count();
    let a = v.open(b"/f", false).expect("open");
    let b = v.open(b"/f", false).expect("open");
    v.unlink(b"/f").expect("unlink");
    assert_eq!(v.fs().inode_count(), base, "órfão continua na tabela enquanto aberto");
    assert_eq!(v.fs().orphans().len(), 1);
    v.close(a).expect("close");
    assert_eq!(v.fs().inode_count(), base);
    assert_eq!(v.pread(b, 0, 10).expect("pread"), b"dados");
    v.close(b).expect("close");
    assert_eq!(v.fs().inode_count(), base - 1, "último close libera o inode");
    assert!(v.fs().orphans().is_empty());
    assert!(v.fsck().is_empty());
}

#[test]
fn orphan_inode_freed_on_last_close() {
    for_each_structure!(|_meta, F| {
        orphan_lifecycle::<F>();
    });
    for_each_final!(|_meta, F| {
        orphan_lifecycle::<F>();
    });
}
