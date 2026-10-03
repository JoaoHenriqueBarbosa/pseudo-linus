//! proptest de sequências de operações.
//!
//! 1. Modelo de referência x tmpfs de verdade do host: prova que o modelo (um `BTreeMap` de
//!    caminhos) tem a semântica do Linux, inclusive a ordem dos errnos e fds de arquivo removido.
//! 2. Cada flavor x modelo, agora com snapshot e restore no meio, mais os invariantes (`fsck`).

use e03_tmpfs_persistent::check::{Dump, MAX_LEN, MAX_OFF, Model, NAMES, Op, OpResult, Ours, RealFs, Target, pattern};
use e03_tmpfs_persistent::maps::Flavor;
use e03_tmpfs_persistent::{for_each_content, for_each_final, for_each_structure};
use proptest::prelude::*;
use proptest::test_runner::TestCaseError;

fn path() -> impl Strategy<Value = String> {
    prop::collection::vec(prop::sample::select(NAMES), 1..=3).prop_map(|c| c.iter().map(|n| format!("/{n}")).collect())
}

fn data() -> impl Strategy<Value = Vec<u8>> {
    (0..MAX_LEN, any::<u8>()).prop_map(|(len, seed)| pattern(len, seed))
}

fn op(snapshots: bool) -> BoxedStrategy<Op> {
    let base = prop_oneof![
        3 => path().prop_map(Op::Mkdir),
        3 => path().prop_map(Op::Create),
        3 => (path(), 0..MAX_OFF, data()).prop_map(|(p, o, d)| Op::Write(p, o, d)),
        1 => (path(), 0..MAX_OFF).prop_map(|(p, l)| Op::Truncate(p, l)),
        1 => (path(), 0..MAX_OFF, 0..MAX_LEN).prop_map(|(p, o, l)| Op::Read(p, o, l)),
        2 => path().prop_map(Op::Unlink),
        2 => path().prop_map(Op::Rmdir),
        3 => (path(), path()).prop_map(|(a, b)| Op::Rename(a, b)),
        2 => (path(), path()).prop_map(|(a, b)| Op::Link(a, b)),
        1 => path().prop_map(Op::Stat),
        1 => path().prop_map(Op::Readdir),
        2 => path().prop_map(Op::Open),
        1 => (0..8usize).prop_map(Op::Close),
        2 => (0..8usize, 0..MAX_OFF, data()).prop_map(|(s, o, d)| Op::PWrite(s, o, d)),
        1 => (0..8usize, 0..MAX_OFF, 0..MAX_LEN).prop_map(|(s, o, l)| Op::PRead(s, o, l)),
        1 => (0..8usize).prop_map(Op::FStat),
    ];
    if snapshots {
        prop_oneof![
            20 => base,
            1 => Just(Op::Snapshot),
            1 => (0..8usize).prop_map(Op::Restore),
        ]
        .boxed()
    } else {
        base.boxed()
    }
}

fn record(ops: &[Op], target: &mut dyn Target) -> (Vec<OpResult>, Dump) {
    let results = ops.iter().map(|op| target.apply(op)).collect();
    (results, target.dump())
}

fn check_flavor<F: Flavor>(key: &str, ops: &[Op], want: &(Vec<OpResult>, Dump)) -> Result<(), TestCaseError> {
    let mut ours = Ours::<F>::new();
    for (i, op) in ops.iter().enumerate() {
        let got = ours.apply(op);
        prop_assert_eq!(&got, &want.0[i], "{}: passo {} {:?}", key, i, op);
    }
    let dump = ours.dump();
    prop_assert_eq!(&dump, &want.1, "{}: retrato final", key);
    let problems = ours.vfs.fsck();
    prop_assert!(problems.is_empty(), "{}: fsck {:?}", key, problems);
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2048, ..ProptestConfig::default() })]

    #[test]
    fn model_matches_linux_tmpfs(ops in prop::collection::vec(op(false), 1..80)) {
        let mut real = RealFs::new().expect("diretório no tmpfs do host");
        let want = record(&ops, &mut real);
        let got = record(&ops, &mut Model::new());
        for (i, op) in ops.iter().enumerate() {
            prop_assert_eq!(&got.0[i], &want.0[i], "passo {} {:?}", i, op);
        }
        prop_assert_eq!(got.1, want.1);
    }

    #[test]
    fn every_flavor_matches_model(ops in prop::collection::vec(op(true), 1..80)) {
        let want = record(&ops, &mut Model::new());
        for_each_structure!(|meta, F| {
            check_flavor::<F>(meta.key, &ops, &want)?;
        });
        for_each_content!(|meta, F| {
            check_flavor::<F>(meta.key, &ops, &want)?;
        });
        for_each_final!(|meta, F| {
            check_flavor::<F>(meta.key, &ops, &want)?;
        });
    }

    #[test]
    fn every_flavor_matches_linux_tmpfs(ops in prop::collection::vec(op(false), 1..60)) {
        let mut real = RealFs::new().expect("diretório no tmpfs do host");
        let want = record(&ops, &mut real);
        for_each_structure!(|meta, F| {
            check_flavor::<F>(meta.key, &ops, &want)?;
        });
        for_each_final!(|meta, F| {
            check_flavor::<F>(meta.key, &ops, &want)?;
        });
    }
}
