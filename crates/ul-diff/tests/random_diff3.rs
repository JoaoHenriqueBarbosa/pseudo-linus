//! Corpus aleatório de triplas (meu, velho, seu) contra o GNU diff3 do oráculo, em todos os formatos:
//! padrão, `-e`, `-E`, `-3`, `-x`, `-A`, `-m`, `-m -E`, `-m -3`. Precisa do docker.
//!
//! ```sh
//! cargo test -p ul-diff --test random_diff3 -- --ignored --nocapture
//! ```

mod fuzzgen;

use fuzzgen::{case_with_files, generate_triples, gnu_outcomes, score};
use harness::{Candidate, Case};
use pl_testing::TestkitCandidate;

const FLAG_SETS: &[(&str, &[&str])] = &[
    ("normal", &[]),
    ("e", &["-e"]),
    ("E", &["-E"]),
    ("3", &["-3"]),
    ("x", &["-x"]),
    ("A", &["-A"]),
    ("m", &["-m"]),
    ("mE", &["-m", "-E"]),
    ("m3", &["-m", "-3"]),
];

fn cases_for(label: &str, flags: &[&str], triples: &[[String; 3]]) -> Vec<Case> {
    triples
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut argv = vec!["diff3".to_string()];
            argv.extend(flags.iter().map(|s| s.to_string()));
            argv.extend(["mine".into(), "old".into(), "yours".into()]);
            case_with_files(format!("d3-{label}-{i:04}"), argv, &[("mine", &t[0]), ("old", &t[1]), ("yours", &t[2])])
        })
        .collect()
}

#[test]
#[ignore = "precisa do docker com o oráculo"]
fn random_triples_against_gnu() {
    let triples = generate_triples(33, 600);
    let cand = TestkitCandidate::new("ul-diff", ul_diff::programs());
    let mut report = String::new();
    let mut ok = true;
    for (label, flags) in FLAG_SETS {
        let cases = cases_for(label, flags, &triples);
        let gnu = gnu_outcomes(&format!("d3-{label}"), &cases);
        let s = score(&cases, &gnu, &cand);
        report.push_str(&format!("diff3 {label}: {}/{} iguais ao GNU\n", s.same, s.total));
        if !s.failing.is_empty() {
            ok = false;
            report.push_str(&format!("  divergentes: {}\n", s.failing.iter().take(20).cloned().collect::<Vec<_>>().join(" ")));
        }
    }
    eprint!("{report}");
    assert!(ok, "{report}");
}

/// Depuração: `UL_DIFF3_SHOW=m:0012 ... show_triples -- --ignored --nocapture`.
#[test]
#[ignore = "depuração manual"]
fn show_triples() {
    let Ok(spec) = std::env::var("UL_DIFF3_SHOW") else { return };
    let triples = generate_triples(33, 600);
    let cand = TestkitCandidate::new("ul-diff", ul_diff::programs());
    for item in spec.split(',') {
        let (label, idx) = item.split_once(':').expect("rótulo:índice");
        let flags = FLAG_SETS.iter().find(|(l, _)| *l == label).map(|(_, f)| *f).expect("rótulo");
        let i: usize = idx.parse().expect("índice");
        let cases = cases_for(label, flags, &triples);
        let gnu = gnu_outcomes(&format!("d3-{label}"), &cases);
        let ours = cand.run(&cases[i].invocation().expect("caso"));
        eprintln!("==== {item}\nmine {:?}\nold {:?}\nyours {:?}", triples[i][0], triples[i][1], triples[i][2]);
        eprintln!(
            "-- GNU (exit {:?}):\n{}stderr {:?}",
            gnu[i].exit,
            String::from_utf8_lossy(&gnu[i].stdout.0),
            String::from_utf8_lossy(&gnu[i].stderr.0)
        );
        eprintln!(
            "-- nosso (exit {:?}):\n{}stderr {:?}",
            ours.exit,
            String::from_utf8_lossy(&ours.stdout.0),
            String::from_utf8_lossy(&ours.stderr.0)
        );
    }
}
