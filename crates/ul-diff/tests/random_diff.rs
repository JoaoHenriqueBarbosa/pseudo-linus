//! Corpus aleatório do F08 (semente 30, 800 pares, famílias Tiny/Code/Text/Medium) contra o GNU diff do
//! oráculo, nos formatos normal, `-u`, `-c`, `-e` e `-y`, mais pares grandes e a linha de base do
//! imara-diff. Precisa do docker com a imagem do oráculo; as saídas do GNU ficam em cache no diretório
//! temporário do target.
//!
//! ```sh
//! cargo test -p ul-diff --test random_diff -- --ignored --nocapture
//! ```

mod fuzzgen;

use fuzzgen::{CODE_LINES, Family, Lcg, Pair, case_for, generate, gnu_outcomes, mutate, render, score};
use harness::{Candidate, Case};
use pl_testing::TestkitCandidate;

#[test]
#[ignore = "precisa do docker com o oráculo"]
fn random_corpus_against_gnu() {
    let pairs = generate(30, 800);
    let cand = TestkitCandidate::new("ul-diff", ul_diff::programs());
    let mut report = String::new();
    let mut all_ok = true;
    for (label, flags) in [("normal", &[][..]), ("u", &["-u"][..]), ("c", &["-c"][..]), ("e", &["-e"][..]), ("y", &["-y"][..])] {
        let cases: Vec<Case> =
            pairs.iter().enumerate().map(|(i, p)| case_for(format!("fuzz-{label}-{i:04}"), p, flags)).collect();
        let gnu = gnu_outcomes(&format!("random-{label}"), &cases);
        let s = score(&cases, &gnu, &cand);
        report.push_str(&format!("{label}: {}/{} iguais ao GNU\n", s.same, s.total));
        if !s.failing.is_empty() {
            all_ok = false;
            report.push_str(&format!("  divergentes: {}\n", s.failing.iter().take(30).cloned().collect::<Vec<_>>().join(" ")));
        }
    }
    eprint!("{report}");
    assert!(all_ok, "{report}");
}

/// Pares grandes: 20 mil linhas com 1% de edições, arquivos independentes (custo alto o bastante pra
/// heurística de custo do GNU entrar) e repetitivos, com e sem `--minimal`.
#[test]
#[ignore = "precisa do docker com o oráculo"]
fn large_pairs_against_gnu() {
    let mut rng = Lcg::new(77);
    let mut pairs = Vec::new();
    // 20k linhas quase únicas, 1% de edições.
    let a: Vec<String> = (0..20_000).map(|i| format!("line {i} {}", rng.below(1000))).collect();
    let mut b = a.clone();
    for _ in 0..200 {
        let at = rng.below(b.len() as u64) as usize;
        match rng.below(3) {
            0 => b[at] = format!("changed {}", rng.below(100_000)),
            1 => {
                b.remove(at);
            }
            _ => b.insert(at, format!("new {}", rng.below(100_000))),
        }
    }
    pairs.push(Pair { family: Family::Medium, a: render(&a, true), b: render(&b, true) });
    // Independentes com alfabeto pequeno: custo de milhares de passos.
    for (n, alpha) in [(6000u64, 20u64), (9000, 50), (12000, 400)] {
        let a: Vec<String> = (0..n).map(|_| format!("w{}", rng.below(alpha))).collect();
        let b: Vec<String> = (0..n).map(|_| format!("w{}", rng.below(alpha))).collect();
        pairs.push(Pair { family: Family::Medium, a: render(&a, true), b: render(&b, true) });
    }
    // Repetitivo de 3 mil linhas.
    let a: Vec<String> = (0..3000).map(|_| CODE_LINES[rng.below(4) as usize].to_string()).collect();
    let mut pick = |r: &mut Lcg| CODE_LINES[r.below(4) as usize].to_string();
    let b = mutate(&mut rng, &a, &mut pick, 60);
    pairs.push(Pair { family: Family::Code, a: render(&a, true), b: render(&b, true) });
    let cand = TestkitCandidate::new("ul-diff", ul_diff::programs());
    let mut report = String::new();
    let mut ok = true;
    for (label, flags) in [("normal", &[][..]), ("minimal", &["-d"][..]), ("u", &["-u"][..])] {
        let cases: Vec<Case> =
            pairs.iter().enumerate().map(|(i, p)| case_for(format!("large-{label}-{i}"), p, flags)).collect();
        let gnu = gnu_outcomes(&format!("large-{label}"), &cases);
        let started = std::time::Instant::now();
        let s = score(&cases, &gnu, &cand);
        report.push_str(&format!(
            "{label}: {}/{} iguais ({:.2}s) {:?}\n",
            s.same,
            s.total,
            started.elapsed().as_secs_f64(),
            s.failing
        ));
        ok &= s.failing.is_empty();
    }
    eprint!("{report}");
    assert!(ok, "{report}");
}

/// Depuração: `UL_DIFF_SHOW=e:0002,y:0010 cargo test ... show_cases -- --ignored --nocapture` mostra a
/// entrada, a saída do GNU e a nossa.
#[test]
#[ignore = "depuração manual"]
fn show_cases() {
    let Ok(spec) = std::env::var("UL_DIFF_SHOW") else { return };
    let pairs = generate(30, 800);
    let cand = TestkitCandidate::new("ul-diff", ul_diff::programs());
    for item in spec.split(',') {
        let (label, idx) = item.split_once(':').expect("rótulo:índice");
        let flags: &[&str] = match label {
            "u" => &["-u"],
            "c" => &["-c"],
            "e" => &["-e"],
            "y" => &["-y"],
            _ => &[],
        };
        let i: usize = idx.parse().expect("índice");
        let cases: Vec<Case> =
            pairs.iter().enumerate().map(|(i, p)| case_for(format!("fuzz-{label}-{i:04}"), p, flags)).collect();
        let gnu = gnu_outcomes(&format!("random-{label}"), &cases);
        let ours = cand.run(&cases[i].invocation().expect("caso"));
        eprintln!("==== {item}\n-- a:\n{:?}\n-- b:\n{:?}", pairs[i].a, pairs[i].b);
        eprintln!(
            "-- GNU (exit {:?}) stdout:\n{}\nstderr: {:?}",
            gnu[i].exit,
            String::from_utf8_lossy(&gnu[i].stdout.0),
            String::from_utf8_lossy(&gnu[i].stderr.0)
        );
        eprintln!(
            "-- nosso (exit {:?}) stdout:\n{}\nstderr: {:?}",
            ours.exit,
            String::from_utf8_lossy(&ours.stdout.0),
            String::from_utf8_lossy(&ours.stderr.0)
        );
    }
}

/// Linha de base do F08: imara-diff Myers + `postprocess_no_heuristic` com o nosso formatador.
#[test]
#[ignore = "precisa do docker com o oráculo"]
fn imara_baseline() {
    use imara_diff::{Algorithm, Diff, NoSliderHeuristic, Token};
    use ul_diff::diff::format::{Look, Paint, Printer, build_script, format_normal, format_unified};
    use ul_diff::diff::text::{Normalize, intern, split_lines};
    let pairs = generate(30, 800);
    for (label, flags) in [("normal", &[][..]), ("u", &["-u"][..])] {
        let cases: Vec<Case> =
            pairs.iter().enumerate().map(|(i, p)| case_for(format!("fuzz-{label}-{i:04}"), p, flags)).collect();
        let gnu = gnu_outcomes(&format!("random-{label}"), &cases);
        let mut same = 0;
        for (pair, g) in pairs.iter().zip(&gnu) {
            let la = split_lines(pair.a.as_bytes());
            let lb = split_lines(pair.b.as_bytes());
            let it = intern(&la, &lb, &Normalize::default());
            let mut out = Vec::new();
            if it.a != it.b {
                let before: Vec<Token> = it.a.iter().map(|&t| Token(t)).collect();
                let after: Vec<Token> = it.b.iter().map(|&t| Token(t)).collect();
                let mut d = Diff::default();
                d.compute_with(Algorithm::Myers, &before, &after, it.classes as u32);
                d.postprocess_with(&before, &after, NoSliderHeuristic);
                let c0: Vec<bool> = (0..la.len() as u32).map(|i| d.is_removed(i)).collect();
                let c1: Vec<bool> = (0..lb.len() as u32).map(|j| d.is_added(j)).collect();
                let s = build_script(&c0, &c1);
                let look = Look::default();
                let mut p = Printer { out: &mut out, look: &look };
                if label == "u" {
                    p.control("--- a\t2026-01-15 12:00:00.000000000 +0000", Paint::None);
                    p.control("+++ b\t2026-01-15 12:00:00.000000000 +0000", Paint::None);
                    format_unified(&mut p, &s, &la, &lb, 3, None);
                } else {
                    format_normal(&mut p, &s, &la, &lb);
                }
            }
            if out == g.stdout.0 {
                same += 1;
            }
        }
        eprintln!("imara-diff {label}: {same}/{} iguais ao GNU", pairs.len());
    }
}

/// Interoperabilidade: o `patch` do GNU aplica os nossos diffs (normal, `-u`, `-c`) e chega ao segundo
/// arquivo, em 200 pares do corpus aleatório.
#[test]
#[ignore = "precisa do docker com o oráculo"]
fn gnu_patch_applies_our_diffs() {
    let pairs = generate(30, 200);
    let cand = TestkitCandidate::new("ul-diff", ul_diff::programs());
    let oracle = harness::Oracle::locate().expect("oráculo");
    let mut report = String::new();
    let mut ok = true;
    for (label, flags) in [("normal", &[][..]), ("u", &["-u"][..]), ("c", &["-c"][..])] {
        let mut cases = Vec::new();
        for (i, p) in pairs.iter().enumerate() {
            let ours = cand.run(&case_for(format!("x-{i}"), p, flags).invocation().expect("caso"));
            let patch_text = String::from_utf8_lossy(&ours.stdout.0).into_owned();
            let mut c = fuzzgen::case_with_files(
                format!("interop-{label}-{i:04}"),
                Vec::new(),
                &[("a", &p.a), ("b", &p.b), ("fix.patch", &patch_text)],
            );
            c.script = Some("patch -s a fix.patch && cmp -s a b && echo OK".into());
            cases.push(c);
        }
        let outs = oracle.run(&cases).expect("oráculo");
        let good = outs.iter().filter(|o| o.stdout.0 == b"OK\n").count();
        report.push_str(&format!("patch do GNU com diff {label}: {good}/{} reconstroem o segundo arquivo\n", cases.len()));
        ok &= good == cases.len();
    }
    eprint!("{report}");
    assert!(ok, "{report}");
}
