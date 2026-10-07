//! Lê `Vec<Case>` em JSON no stdin, roda cada caso em `/work/case` e escreve `Vec<Outcome>` no stdout.

use std::io::Read;
use std::net::{SocketAddrV4, TcpListener};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use harness::{CASE_DIR, Case, Outcome};

/// Diretório (montado pelo harness) com as wheels que o espelho do PyPI serve.
const WHEELS_DIR: &str = "/agent/wheels";

/// Sobe o espelho do PyPI em 127.0.0.80:443, uma thread por conexão. Só roda pros casos com a tag
/// `mirror`, pra que o listener não apareça no `ss` nem em `/proc/net/tcp` dos outros casos.
fn start_mirror() -> Result<()> {
    let mirror = Arc::new(mirror::Mirror::from_dir(Path::new(WHEELS_DIR)).context("carregando wheels do espelho")?);
    let listener = TcpListener::bind(SocketAddrV4::new(mirror::ADDR, mirror::PORT)).context("bind do espelho")?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mirror = Arc::clone(&mirror);
            std::thread::spawn(move || {
                let _ = mirror.serve_tls(stream);
            });
        }
    });
    Ok(())
}

fn main() -> Result<()> {
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input)?;
    let cases: Vec<Case> = serde_json::from_slice(&input)?;
    let mut outcomes: Vec<Outcome> = Vec::with_capacity(cases.len());
    let mut mirror_up = false;
    for case in &cases {
        if !mirror_up && case.tags.iter().any(|t| t == "mirror") {
            start_mirror()?;
            mirror_up = true;
        }
        let outcome = harness::real::run_case(case, Path::new(CASE_DIR)).unwrap_or_else(|e| {
            Outcome::unsupported(format!("oracle-agent: {e:#}"))
        });
        outcomes.push(outcome);
    }
    serde_json::to_writer(std::io::stdout().lock(), &outcomes)?;
    Ok(())
}
