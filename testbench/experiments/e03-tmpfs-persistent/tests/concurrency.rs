//! Correção das três estratégias de trava com escritores concorrentes.
//!
//! Oito threads, cada uma no seu diretório da mesma sandbox, criam, escrevem e removem arquivos
//! guardando o que esperam ver. Uma nona thread tira snapshots o tempo todo e confere os
//! invariantes de cada um (snapshot nunca pode pegar meia operação). No fim, o estado exportado
//! tem que bater com o esperado de cada thread.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use e03_tmpfs_persistent::concurrent::{LockedSandbox, RcuSandbox, SharedSandbox, ShardedSandbox};
use e03_tmpfs_persistent::flavors::{FinalHand, FinalHandOrd, FinalImblOrd, HandRadix, ImblHamt, RpdsHamt};
use e03_tmpfs_persistent::maps::Flavor;
use e03_tmpfs_persistent::workload::Rng;
use e03_tmpfs_persistent::{Errno, Fs, Vfs};

const THREADS: usize = 8;
const OPS: usize = 1500;

fn worker<F: Flavor, S: SharedSandbox<F>>(sb: &S, t: usize) -> BTreeMap<String, Vec<u8>> {
    let dir = format!("/w{t}");
    sb.mkdir(dir.as_bytes()).expect("mkdir");
    let mut rng = Rng::new(1000 + t as u64);
    let mut expect: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for i in 0..OPS {
        let name = format!("{dir}/f{}", rng.below(24));
        match rng.below(10) {
            0..=1 => {
                sb.create(name.as_bytes()).expect("create");
                expect.entry(name).or_default();
            }
            2..=7 => {
                let off = rng.below(10_000);
                let len = rng.below(6000) as usize;
                let data: Vec<u8> = (0..len).map(|k| (k + i + t) as u8).collect();
                let r = sb.write(name.as_bytes(), off, &data);
                match expect.get_mut(&name) {
                    Some(content) => {
                        r.expect("escrita em arquivo existente");
                        if len > 0 {
                            let end = off as usize + len;
                            if end > content.len() {
                                content.resize(end, 0);
                            }
                            content[off as usize..end].copy_from_slice(&data);
                        }
                    }
                    None => assert_eq!(r, Err(Errno::ENOENT)),
                }
            }
            8 => {
                let r = sb.unlink(name.as_bytes());
                match expect.remove(&name) {
                    Some(_) => r.expect("unlink"),
                    None => assert_eq!(r, Err(Errno::ENOENT)),
                }
            }
            _ => {
                let r = sb.stat(name.as_bytes());
                match expect.get(&name) {
                    Some(c) => assert_eq!(r.expect("stat").size, c.len() as u64),
                    None => assert_eq!(r.map(|_| ()), Err(Errno::ENOENT)),
                }
            }
        }
    }
    expect
}

fn exercise<F: Flavor, S: SharedSandbox<F>>(label: &str) {
    let image = S::image_from_fs(&Fs::<F>::new());
    let sb = S::from_image(&image);
    let stop = AtomicBool::new(false);
    let (expected, snapshots) = std::thread::scope(|scope| {
        let checker = scope.spawn(|| {
            let mut n = 0;
            while !stop.load(Ordering::Relaxed) {
                let fs = S::image_to_fs(&sb.snapshot());
                let problems = Vfs::from_image(&fs).fsck();
                assert!(problems.is_empty(), "{label}: snapshot inconsistente: {problems:?}");
                n += 1;
            }
            n
        });
        let workers: Vec<_> = (0..THREADS).map(|t| scope.spawn({ let sb = &sb; move || worker::<F, S>(sb, t) })).collect();
        let expected: Vec<_> = workers.into_iter().map(|h| h.join().expect("worker")).collect();
        stop.store(true, Ordering::Relaxed);
        (expected, checker.join().expect("checker"))
    });
    assert!(snapshots > 0, "{label}: nenhum snapshot conferido");
    let vfs = Vfs::from_image(&sb.export());
    assert!(vfs.fsck().is_empty(), "{label}: {:?}", vfs.fsck());
    for (t, expect) in expected.iter().enumerate() {
        let dir = format!("/w{t}");
        let names: Vec<String> = vfs
            .readdir(dir.as_bytes())
            .expect("readdir")
            .into_iter()
            .map(|(n, _)| format!("{dir}/{}", String::from_utf8_lossy(&n)))
            .collect();
        assert_eq!(names, expect.keys().cloned().collect::<Vec<_>>(), "{label}: nomes em {dir}");
        for (name, content) in expect {
            assert_eq!(&vfs.read(name.as_bytes(), 0, usize::MAX >> 1).expect("read"), content, "{label}: {name}");
        }
    }
}

fn all_strategies<F: Flavor>(key: &str) {
    exercise::<F, LockedSandbox<F>>(&format!("{key} / RwLock"));
    exercise::<F, RcuSandbox<F>>(&format!("{key} / arc-swap"));
    exercise::<F, ShardedSandbox<F>>(&format!("{key} / fatias"));
}

#[test]
fn imbl_hamt() {
    all_strategies::<ImblHamt>("imbl-hamt");
}

#[test]
fn rpds_hamt() {
    all_strategies::<RpdsHamt>("rpds-hamt");
}

#[test]
fn hand_radix() {
    all_strategies::<HandRadix>("hand-radix");
}

#[test]
fn final_hand() {
    all_strategies::<FinalHand>("final-hand");
}

#[test]
fn final_hand_ord() {
    all_strategies::<FinalHandOrd>("final-hand-ord");
}

#[test]
fn final_imbl_ord() {
    all_strategies::<FinalImblOrd>("final-imbl-ord");
}
