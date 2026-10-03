//! O protótipo git sobre `gix-*` é nosso: tem que bater com o golden e passar no `git fsck`.

use f12_net_sqlite_git::git::GitCandidate;
use f12_net_sqlite_git::git::experiments as gx;

#[test]
fn prototype_matches_golden() {
    let cases = gx::load_cases().unwrap();
    let (conf, cmps) = harness::score(&GitCandidate, &cases);
    let failing: Vec<_> = cmps.iter().filter(|c| !c.strict).map(|c| (c.id.clone(), c.detail.clone())).collect();
    assert_eq!(conf.strict_pass, conf.total, "{failing:#?}");
}

#[test]
fn exported_repository_passes_fsck_and_reads_real_packs() {
    let Ok(oracle) = harness::Oracle::locate() else {
        eprintln!("oráculo indisponível; teste pulado");
        return;
    };
    let fsck = gx::fsck_validation(&oracle).unwrap();
    assert_eq!(fsck["fsck_strict_exit"], 0, "{fsck:#}");
    assert_eq!(fsck["checks_equal"], fsck["checks_total"], "{:#}", fsck["checks"]);
    let packed = gx::read_real_packed_repo(&oracle).unwrap();
    assert_eq!(packed["objects_equal_type_size_and_cat_file_p"], packed["objects_total"], "{packed:#}");
    assert!(packed["pack_delta_entries_seen_by_gix_pack"].as_u64().unwrap() > 0, "o pack precisa ter deltas");
}
