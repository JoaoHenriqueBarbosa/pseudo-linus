//! Subconjunto portável da suíte de testes do gawk 5.2.1 (`test/` do tarball).
//!
//! Os arquivos ficam em `corpus/upstream/gawk/` (gitignored: são GPLv3 e não entram no repositório).
//! Seleção, toda automática e reproduzível:
//! 1. testes de BASIC_TESTS, UNIX_TESTS e GAWK_EXT_TESTS do `test/Makefile.am`;
//! 2. que usam a regra padrão gerada pelo `Gentests` (sem alvo próprio no Makefile.am, sem `.sh`);
//! 3. que não exigem flag nem locale (fora de todas as listas NEED_* e de RUN_SHELL);
//! 4. com `.awk` e `.ok`; a fixture leva o `.awk`, o `.in` (como stdin) e os arquivos do diretório de
//!    teste citados literalmente no programa;
//! 5. e que o gawk do oráculo reproduz: stdout + stderr + "EXIT CODE: n" igual ao `.ok` byte a byte.
//!
//! Os que passam viram casos com o resultado do oráculo como golden (stdout, stderr, exit e árvore).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use harness::{Case, FileSpec, Oracle, Outcome};
use serde::{Deserialize, Serialize};

pub const VERSION: &str = "5.2.1";

pub fn upstream_dir() -> PathBuf {
    harness::paths::corpus_dir().join("upstream").join("gawk")
}

pub fn test_dir() -> PathBuf {
    upstream_dir().join(format!("gawk-{VERSION}")).join("test")
}

/// Baixa e extrai o `test/` do tarball se ainda não estiver lá.
pub fn ensure_upstream() -> Result<PathBuf> {
    let dir = test_dir();
    if dir.join("Makefile.am").exists() {
        return Ok(dir);
    }
    let up = upstream_dir();
    std::fs::create_dir_all(&up)?;
    let tarball = up.join(format!("gawk-{VERSION}.tar.xz"));
    if !tarball.exists() {
        let url = format!("https://ftp.gnu.org/gnu/gawk/gawk-{VERSION}.tar.xz");
        let st = Command::new("curl").args(["-sSfL", "-o"]).arg(&tarball).arg(&url).status()?;
        if !st.success() {
            bail!("download de {url} falhou");
        }
    }
    let st = Command::new("tar")
        .arg("xJf")
        .arg(&tarball)
        .arg("-C")
        .arg(&up)
        .arg(format!("gawk-{VERSION}/test"))
        .status()?;
    if !st.success() {
        bail!("extração do test/ do gawk falhou");
    }
    Ok(dir)
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Selection {
    pub listed: usize,
    pub selected: Vec<String>,
    /// motivo -> testes
    pub excluded: BTreeMap<String, Vec<String>>,
}

fn list_vars(makefile: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in makefile.lines() {
        if let Some(name) = &current {
            let cont = line.trim_end().ends_with('\\');
            let body = line.trim_end().trim_end_matches('\\');
            out.get_mut(name).expect("var").extend(body.split_whitespace().map(str::to_string));
            if !cont {
                current = None;
            }
            continue;
        }
        if let Some((name, rest)) = line.split_once('=') {
            let name = name.trim();
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                let cont = rest.trim_end().ends_with('\\');
                let body = rest.trim_end().trim_end_matches('\\');
                out.insert(name.to_string(), body.split_whitespace().map(str::to_string).collect());
                if cont {
                    current = Some(name.to_string());
                }
            }
        }
    }
    out
}

fn explicit_targets(makefile: &str) -> BTreeSet<String> {
    makefile
        .lines()
        .filter_map(|l| {
            let (name, _) = l.split_once(':')?;
            (!name.is_empty()
                && !l.starts_with('\t')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .then(|| name.to_string())
        })
        .collect()
}

pub fn select(dir: &Path) -> Result<Selection> {
    let makefile = std::fs::read_to_string(dir.join("Makefile.am"))?;
    let vars = list_vars(&makefile);
    let targets = explicit_targets(&makefile);
    let mut sel = Selection::default();
    let mut flagged: BTreeMap<String, String> = BTreeMap::new();
    for (var, names) in &vars {
        if var.starts_with("NEED_") || var == "RUN_SHELL" {
            for n in names {
                flagged.entry(n.clone()).or_insert_with(|| var.clone());
            }
        }
    }
    for list in ["BASIC_TESTS", "UNIX_TESTS", "GAWK_EXT_TESTS"] {
        for name in vars.get(list).into_iter().flatten() {
            sel.listed += 1;
            let reason = if let Some(v) = flagged.get(name) {
                Some(format!("precisa de flag ou ambiente ({v})"))
            } else if targets.contains(name) {
                Some("regra própria no Makefile.am".to_string())
            } else if dir.join(format!("{name}.sh")).exists() {
                Some("roda por script .sh".to_string())
            } else if !dir.join(format!("{name}.awk")).exists() || !dir.join(format!("{name}.ok")).exists() {
                Some("sem .awk ou .ok".to_string())
            } else {
                None
            };
            match reason {
                Some(r) => sel.excluded.entry(r).or_default().push(name.clone()),
                None => sel.selected.push(name.clone()),
            }
        }
    }
    Ok(sel)
}

/// Arquivos do diretório de teste citados literalmente no programa (além do próprio .awk).
fn referenced_files(dir: &Path, name: &str, program: &str, all_files: &BTreeSet<String>) -> Vec<String> {
    let mut out = BTreeSet::new();
    for token in program.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')) {
        if token.is_empty() || token == format!("{name}.awk") || token.ends_with(".ok") {
            continue;
        }
        if all_files.contains(token) && dir.join(token).is_file() {
            out.insert(token.to_string());
        }
    }
    out.into_iter().collect()
}

pub fn build_cases(dir: &Path, names: &[String]) -> Result<Vec<Case>> {
    let all_files: BTreeSet<String> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let mut cases = Vec::new();
    for name in names {
        let awk_path = dir.join(format!("{name}.awk"));
        let program = std::fs::read(&awk_path)?;
        let program_text = String::from_utf8_lossy(&program).into_owned();
        let mut files = BTreeMap::new();
        files.insert(format!("{name}.awk"), spec_of(&program));
        let mut size = program.len();
        for f in referenced_files(dir, name, &program_text, &all_files) {
            let data = std::fs::read(dir.join(&f))?;
            size += data.len();
            files.insert(f, spec_of(&data));
        }
        if size > 2 << 20 {
            continue;
        }
        let stdin_path = dir.join(format!("{name}.in"));
        let stdin = if stdin_path.exists() { Some(std::fs::read(&stdin_path)?) } else { None };
        let mut env = BTreeMap::new();
        env.insert("AWKPATH".to_string(), ".".to_string());
        cases.push(Case {
            id: format!("gawk-{name}"),
            argv: vec!["gawk".into(), "-f".into(), format!("{name}.awk")],
            script: None,
            stdin: None,
            stdin_b64: stdin.map(|s| b64(&s)),
            files,
            env,
            tags: vec!["upstream:gawk".into()],
            faketime: None,
            timeout_ms: Some(10_000),
        });
    }
    Ok(cases)
}

fn b64(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn spec_of(data: &[u8]) -> FileSpec {
    match std::str::from_utf8(data) {
        Ok(s) => FileSpec::Text(s.to_string()),
        Err(_) => FileSpec::Table(harness::case::FileTable { content_b64: Some(b64(data)), ..Default::default() }),
    }
}

/// Saída "mesclada" como o Makefile do gawk grava em `_teste`: stdout, stderr e a linha de exit.
pub fn merged(o: &Outcome) -> Vec<u8> {
    let mut m = o.stdout.0.clone();
    m.extend_from_slice(&o.stderr.0);
    match o.exit {
        Some(0) => {}
        Some(code) => m.extend_from_slice(format!("EXIT CODE: {code}\n").as_bytes()),
        None => m.extend_from_slice(format!("EXIT CODE: {}\n", 128 + o.signal.unwrap_or(0)).as_bytes()),
    }
    m
}

#[derive(Serialize, Deserialize)]
struct Cache {
    key: String,
    outcomes: Vec<Outcome>,
}

/// Casos válidos com o golden do oráculo, e os nomes dos testes que o oráculo não reproduz.
pub type Validated = (Vec<(Case, Outcome)>, Vec<String>);

/// Roda os casos no oráculo (com cache) e devolve só os que reproduzem o `.ok`.
pub fn validate(dir: &Path, cases: Vec<Case>, cache: &Path) -> Result<Validated> {
    let key = harness::memtree::sha256_hex(serde_json::to_string(&cases)?.as_bytes());
    let outcomes: Vec<Outcome> = match std::fs::read_to_string(cache)
        .ok()
        .and_then(|t| serde_json::from_str::<Cache>(&t).ok())
        .filter(|c| c.key == key)
    {
        Some(c) => c.outcomes,
        None => {
            let oracle = Oracle::locate().context("oráculo indisponível pra validar a suíte do gawk")?;
            let mut all = Vec::new();
            for chunk in cases.chunks(60) {
                all.extend(oracle.run(chunk)?);
            }
            std::fs::write(cache, serde_json::to_string(&Cache { key, outcomes: all.clone() })?)?;
            all
        }
    };
    let mut ok = Vec::new();
    let mut mismatched = Vec::new();
    for (mut case, outcome) in cases.into_iter().zip(outcomes) {
        let name = case.id.trim_start_matches("gawk-").to_string();
        let expected = std::fs::read(dir.join(format!("{name}.ok")))?;
        if merged(&outcome) == expected {
            if outcome.stderr.is_empty() && outcome.exit == Some(0) {
                case.tags.push("output-only".into());
            }
            ok.push((case, outcome));
        } else {
            mismatched.push(name);
        }
    }
    Ok((ok, mismatched))
}
