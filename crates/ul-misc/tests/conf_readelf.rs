//! Conformidade do `readelf` contra o golden da bancada (binutils 2.44 do Debian 13). Os casos de
//! notas (`-n`) usam o `true` real do Debian como fixture, porque o do kernel tem build-id próprio.

#[test]
fn conformance_readelf_notes() {
    let cand = pl_testing::TestkitCandidate::new("readelf (testkit)", ul_misc::programs());
    let ids = ["readelf-notes", "readelf-notes-long"];
    let report = pl_testing::score_ids("binutils", &cand, &ids);
    report.print();
    assert_eq!(report.comparisons.len(), ids.len(), "casos sem golden: rode `oracle gen --tool binutils`");
    for id in ids {
        let c = report.comparisons.iter().find(|c| c.id == id).expect("caso no corpus");
        assert!(c.strict, "caso {id} divergiu do oráculo: {:?}", c.detail);
    }
}
