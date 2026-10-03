//! Lê `Vec<Case>` em JSON no stdin, roda cada caso em `/work/case` e escreve `Vec<Outcome>` no stdout.

use std::io::Read;
use std::path::Path;

use anyhow::Result;
use harness::{CASE_DIR, Case, Outcome};

fn main() -> Result<()> {
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input)?;
    let cases: Vec<Case> = serde_json::from_slice(&input)?;
    let mut outcomes: Vec<Outcome> = Vec::with_capacity(cases.len());
    for case in &cases {
        let outcome = harness::real::run_case(case, Path::new(CASE_DIR)).unwrap_or_else(|e| {
            Outcome::unsupported(format!("oracle-agent: {e:#}"))
        });
        outcomes.push(outcome);
    }
    serde_json::to_writer(std::io::stdout().lock(), &outcomes)?;
    Ok(())
}
