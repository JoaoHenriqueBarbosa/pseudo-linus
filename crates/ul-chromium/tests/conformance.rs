//! Conformidade do `--dump-dom` contra o oráculo: cada página de `testbench/corpus/headless/` sai idêntica ao golden
//! gerado pelo chromium do Debian (`testbench/oracle/gen-chromium.sh`), rodando o programa `chromium` inteiro.

use std::path::PathBuf;

use sysabi::testkit::TestKit;

mod common;

fn bench() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testbench")
}

#[test]
fn dump_dom_matches_oracle() {
    let corpus = bench().join("corpus/headless");
    let golden = bench().join("golden/chromium");
    let mut pages: Vec<_> = std::fs::read_dir(&corpus).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|x| x == "html")).collect();
    pages.sort();
    let known: Vec<String> = std::fs::read_to_string(corpus.join("known-divergences.txt")).unwrap_or_default().lines().map(str::to_owned).collect();
    let mut failures = Vec::new();
    for path in &pages {
        let file = path.file_name().unwrap().to_string_lossy().into_owned();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let expected = std::fs::read_to_string(golden.join(format!("{name}.dom"))).unwrap();
        let kit = common::with_image_fonts(TestKit::new().programs(ul_chromium::programs()).file(&format!("/pg/{file}"), std::fs::read(path).unwrap(), 0o644));
        let run = kit.run(&["chromium", "--headless", "--no-sandbox", "--disable-gpu", "--dump-dom", &format!("file:///pg/{file}")], b"");
        let actual = run.stdout_str();
        // Divergência conhecida que passou a casar é aviso para tirar da lista, não sucesso silencioso.
        if (actual != expected) != known.contains(&name) {
            failures.push(format!("--- {name}\nesperado:\n{expected}\nobtido:\n{actual}\nstderr: {}", run.stderr_str()));
        }
    }
    assert!(failures.is_empty(), "{} de {} páginas divergem do oráculo:\n{}", failures.len(), pages.len(), failures.join("\n"));
}
