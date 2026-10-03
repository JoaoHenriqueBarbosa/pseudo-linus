//! Gera `corpus/cases/regex/*.toml` a partir das suítes do GNU grep (em `corpus/upstream/grep/`) e de
//! `data/agent_style.toml`. Os arquivos gerados são derivados mecânicos; a fonte editável é a
//! especificação e as suítes upstream.
//!
//! `cargo run --release --bin f01-gen-cases`

use anyhow::Result;
use f01_regex::corpus::{ProbeSet, RegexEntry, load_agent_style, load_upstream, probes_for, spec_path, upstream_dir, upstream_suites};
use harness::CaseFile;

const HEADER: &str = "# Gerado por testbench/experiments/f01-regex (binário f01-gen-cases). Não edite à mão:\n\
# edite experiments/f01-regex/data/agent_style.toml ou regenere a partir de corpus/upstream/grep.\n\n";

fn write(name: &str, source: &str, entries: &[RegexEntry], set: ProbeSet) -> Result<usize> {
    let mut cases = Vec::new();
    for e in entries {
        for (id, probe, tags) in probes_for(e, set) {
            cases.push(probe.to_case(id, tags));
        }
    }
    let file = CaseFile { tool: Some("regex".into()), source: Some(source.into()), cases };
    let n = file.cases.len();
    let dir = harness::paths::cases_dir("regex");
    std::fs::create_dir_all(&dir)?;
    let text = format!("{HEADER}{}", toml::to_string(&file)?);
    let path = dir.join(format!("{name}.toml"));
    std::fs::write(&path, text)?;
    // Confere que o harness lê de volta.
    CaseFile::load(&path)?;
    println!("{}: {} regexes, {n} sondas", path.display(), entries.len());
    Ok(n)
}

fn main() -> Result<()> {
    for (suite, file, dialect) in upstream_suites() {
        let entries = load_upstream(&upstream_dir().join(file), suite, dialect)?;
        write(&format!("upstream-{suite}"), &format!("upstream:grep-3.11/tests/{file}"), &entries, ProbeSet::Upstream)?;
    }
    let agent = load_agent_style(&spec_path())?;
    write("agent-style", "agent-style", &agent, ProbeSet::Full)?;
    Ok(())
}
