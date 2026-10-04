//! Conformidade dos programas contra o golden da bancada, rodando sobre o testkit do `sysabi`.
//!
//! `cargo test -p ul-textproc --test conformance -- --nocapture` mostra os placares e as falhas.
//!
//! Casos com a tag `order-insensitive` dependem da ordem do readdir (o oráculo roda em overlayfs;
//! o tmpfs do pseudo-linus lista do mais novo pro mais antigo): comparam as linhas ordenadas, como
//! o F02 fazia.

use std::collections::BTreeSet;

use harness::{Candidate, Case, Invocation, Outcome};
use pl_testing::TestkitCandidate;

fn sorted_lines(b: &[u8]) -> Vec<u8> {
    let mut lines: Vec<&[u8]> = b.split_inclusive(|&c| c == b'\n').collect();
    lines.sort();
    lines.concat()
}

struct Sorting {
    inner: TestkitCandidate,
    unordered: BTreeSet<String>,
}

impl Candidate for Sorting {
    fn name(&self) -> String {
        self.inner.name()
    }

    fn run(&self, inv: &Invocation) -> Outcome {
        let mut out = self.inner.run(inv);
        if self.unordered.contains(&inv.case_id) {
            out.stdout = sorted_lines(out.stdout.as_slice()).into();
        }
        out
    }
}

/// Placar de uma ferramenta; devolve (estrito, leniente, total).
pub fn score(tool: &str, filter: impl Fn(&Case) -> bool) -> (usize, usize, usize) {
    let (cases, missing) = harness::paths::load_tool(tool).expect("casos e golden");
    let mut unordered = BTreeSet::new();
    let cases: Vec<(Case, Outcome)> = cases
        .into_iter()
        .filter(|(c, _)| filter(c))
        .map(|(c, mut g)| {
            if c.tags.iter().any(|t| t == "order-insensitive") {
                g.stdout = sorted_lines(g.stdout.as_slice()).into();
                unordered.insert(c.id.clone());
            }
            (c, g)
        })
        .collect();
    let cand = Sorting { inner: TestkitCandidate::new("ul-textproc (testkit)", ul_textproc::programs()), unordered };
    let (conf, comparisons) = harness::score(&cand, &cases);
    eprintln!(
        "{tool}: {}/{} estrito ({:.1}%), {}/{} leniente ({:.1}%), {} sem golden",
        conf.strict_pass,
        conf.total,
        100.0 * conf.strict_rate(),
        conf.lenient_pass,
        conf.total,
        100.0 * conf.lenient_rate(),
        missing
    );
    for c in comparisons.iter().filter(|c| !c.strict) {
        let kind = if c.lenient { "ESTRITO" } else { "FALHA" };
        eprintln!("  {kind} {}: {}", c.id, c.detail.join(" | "));
    }
    (conf.strict_pass, conf.lenient_pass, conf.total)
}

/// As sondas `grep -n` e `grep -ob` do corpus de regex (F01), pelo grep de verdade.
#[test]
fn regex_corpus_grep_probes() {
    let (_, lenient, total) = score("regex", |c| c.argv.first().is_some_and(|a| a == "grep"));
    assert_eq!(lenient, total, "sondas de grep do corpus de regex");
}

#[test]
fn grep_corpus() {
    let (_, lenient, total) = score("grep", |_| true);
    assert_eq!(lenient, total, "grep: meta é 100% leniente");
}
