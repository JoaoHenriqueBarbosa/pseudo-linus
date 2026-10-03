//! Painel de conformidade do pseudo-linus: roda a bancada inteira (todos os casos com golden de
//! `testbench/corpus/cases/*`) sobre o kernel real com a tabela completa de programas e escreve
//! `docs/conformance.md`.
//!
//! Uso: `cargo run -p pl-conformance --release [-- [--testkit] [--details N] [ferramenta ...]]`

use std::fmt::Write as _;

use pl_testing::{KernelCandidate, Report, TestkitCandidate, score_tool};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let use_testkit = args.iter().any(|a| a == "--testkit");
    let details: usize = args
        .iter()
        .position(|a| a == "--details")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let mut tools: Vec<String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && !(i > &0 && args[i - 1] == "--details"))
        .map(|(_, a)| a.clone())
        .collect();
    if tools.is_empty() {
        tools = harness::paths::tools().expect("ferramentas do corpus");
    }

    let programs = userland::all_programs();
    let dups = userland::duplicates(&programs);
    let candidate: Box<dyn harness::Candidate> = if use_testkit {
        Box::new(TestkitCandidate::new("pseudo-linus (testkit)", programs.clone()))
    } else {
        Box::new(KernelCandidate::new("pseudo-linus (kernel)", programs.clone()))
    };

    let started = std::time::Instant::now();
    let mut reports: Vec<Report> = Vec::new();
    for tool in &tools {
        let t0 = std::time::Instant::now();
        let r = score_tool(tool, candidate.as_ref());
        let c = &r.conformance;
        println!(
            "{tool:<12} estrito {:>4}/{:<4} ({:5.1}%)  leniente {:>4}/{:<4} ({:5.1}%)  {:>4} sem suporte  {:.1}s",
            c.strict_pass,
            c.total,
            100.0 * c.strict_rate(),
            c.lenient_pass,
            c.total,
            100.0 * c.lenient_rate(),
            c.unsupported,
            t0.elapsed().as_secs_f64()
        );
        if details > 0 {
            for f in r.comparisons.iter().filter(|c| !c.strict).take(details) {
                println!("    FALHA {}: {}", f.id, f.detail.join(" | "));
            }
        }
        reports.push(r);
    }

    let total: usize = reports.iter().map(|r| r.conformance.total).sum();
    let strict: usize = reports.iter().map(|r| r.conformance.strict_pass).sum();
    let lenient: usize = reports.iter().map(|r| r.conformance.lenient_pass).sum();
    println!(
        "TOTAL        estrito {strict}/{total} ({:.1}%)  leniente {lenient}/{total} ({:.1}%)  em {:.0}s",
        pct(strict, total),
        pct(lenient, total),
        started.elapsed().as_secs_f64()
    );

    if !use_testkit && tools.len() > 1 {
        write_doc(&reports, &programs, &dups, strict, lenient, total);
    }
}

fn pct(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 }
}

fn write_doc(reports: &[Report], programs: &[sysabi::Program], dups: &[String], strict: usize, lenient: usize, total: usize) {
    let mut doc = String::new();
    let _ = writeln!(doc, "# Conformidade do pseudo-linus\n");
    let _ = writeln!(
        doc,
        "Gerado por `cargo run -p pl-conformance --release`: todos os casos da bancada com golden, rodados no kernel real com a tabela completa de programas ({} programas). Estrito: stdout, stderr, exit e arquivos iguais ao Debian 13; leniente: stderr pode divergir.\n",
        programs.len()
    );
    let _ = writeln!(doc, "**Total: {strict}/{total} estrito ({:.1}%), {lenient}/{total} leniente ({:.1}%).**\n", pct(strict, total), pct(lenient, total));
    let _ = writeln!(doc, "| Ferramenta | Casos | Estrito | Leniente | Sem suporte |");
    let _ = writeln!(doc, "|---|---|---|---|---|");
    for r in reports {
        let c = &r.conformance;
        let _ = writeln!(
            doc,
            "| {} | {} | {:.1}% | {:.1}% | {} |",
            r.tool,
            c.total,
            100.0 * c.strict_rate(),
            100.0 * c.lenient_rate(),
            c.unsupported
        );
    }
    if !dups.is_empty() {
        let _ = writeln!(doc, "\nProgramas exportados por mais de um crate (vale o primeiro): {}.", dups.join(", "));
    }
    let path = harness::paths::repo_root().join("docs/conformance.md");
    if let Err(e) = std::fs::write(&path, doc) {
        eprintln!("não gravei {}: {e}", path.display());
    } else {
        println!("gravado {}", path.display());
    }
}
