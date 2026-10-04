//! Conformidade dos compressores e do zip/unzip contra o golden: os arquivos de casos deles em
//! `testbench/corpus/cases/archive/` e, do `listing.toml`, os casos argv cujos programas são os
//! deles. Rode com `--nocapture` pra ver as falhas com detalhe. `COMPRESS_CASE=<id>` filtra.

use harness::{Case, Outcome};
use pl_testing::TestkitCandidate;

const FILES: &[&str] = &["gzip.toml", "bzip2.toml", "xz.toml", "lzip.toml", "zstd.toml", "zip.toml", "unzip.toml", "listing.toml"];

const PROGRAMS: &[&str] = &[
    "gzip", "gunzip", "zcat", "bzip2", "bunzip2", "bzcat", "xz", "unxz", "xzcat", "lzma", "unlzma", "lzcat", "zstd",
    "unzstd", "zstdcat", "lzip", "zip", "unzip",
];

fn load() -> Vec<(Case, Outcome)> {
    let filter = std::env::var("COMPRESS_CASE").ok();
    // Ids dos arquivos que são dos compressores e do zip (o listing.toml é dividido com o tar).
    let mut ids = std::collections::BTreeSet::new();
    for path in harness::paths::case_files("archive").expect("casos") {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        if FILES.contains(&name.as_str()) {
            for c in harness::CaseFile::load(&path).expect("arquivo de casos").cases {
                ids.insert(c.id);
            }
        }
    }
    let (all, _) = harness::paths::load_tool("archive").expect("casos e golden");
    all.into_iter()
        .filter(|(c, _)| ids.contains(&c.id))
        .filter(|(c, _)| c.argv.first().is_some_and(|p| PROGRAMS.contains(&p.as_str())))
        .filter(|(c, _)| filter.as_deref().is_none_or(|f| c.id.contains(f)))
        .collect()
}

#[test]
fn compress_and_zip_cases() {
    let cases = load();
    let cand = TestkitCandidate::new("ul-archive compress (testkit)", ul_archive::programs());
    let (conf, all) = harness::score(&cand, &cases);
    for c in all.iter().filter(|c| !c.strict) {
        eprintln!("FALHA {}: {}", c.id, c.detail.join(" | "));
        // COMPRESS_DUMP=1 mostra a saída inteira (esperada e obtida) das falhas.
        if std::env::var("COMPRESS_DUMP").is_ok()
            && let Some((case, golden)) = cases.iter().find(|(k, _)| k.id == c.id)
        {
            use harness::Candidate;
            let got = cand.run(&case.invocation().expect("caso"));
            eprintln!("--- esperado stdout\n{}", String::from_utf8_lossy(golden.stdout.as_slice()));
            eprintln!("--- obtido stdout\n{}", String::from_utf8_lossy(got.stdout.as_slice()));
            eprintln!("--- esperado stderr\n{}", String::from_utf8_lossy(golden.stderr.as_slice()));
            eprintln!("--- obtido stderr\n{}", String::from_utf8_lossy(got.stderr.as_slice()));
        }
    }
    eprintln!(
        "compressores e zip: {}/{} estrito, {}/{} leniente",
        conf.strict_pass, conf.total, conf.lenient_pass, conf.total
    );
}
