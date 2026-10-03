//! Os utilitários originais (sem porte) rodando no container do oráculo, sobre a mesma fixture.
//!
//! O multicall `uu-original` (pacote `original/` deste workspace) é montado em `/usr/local/bin` com
//! links `cat`, `sort`, `ls`, `head`, `wc`, `find`, `xargs`. Esse diretório vem antes de `/usr/bin`
//! no PATH fixo dos casos, então o mesmo argv (e o mesmo script) cai no uutils em vez do GNU, sem
//! mexer no ambiente. Comandos que o multicall não tem (`echo`, `rm`) continuam sendo os do Debian.
//! Rodar no container, e não no host, deixa usuário, grupo, `/etc/passwd` e FS iguais aos do golden.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use harness::{Case, Oracle, Outcome};

pub const LINKS: &[&str] = &["cat", "sort", "ls", "head", "wc", "find", "xargs"];

fn experiment_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Compila o multicall original (no-op se já estiver em dia) e devolve o caminho do binário.
pub fn build_original() -> Result<PathBuf> {
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "-q", "-p", "uu-original", "--manifest-path"])
        .arg(experiment_dir().join("Cargo.toml"))
        .status()
        .context("cargo build -p uu-original")?;
    if !status.success() {
        bail!("falhou o build do uu-original");
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| experiment_dir().join("target"));
    Ok(target.join("release").join("uu-original"))
}

/// Diretório com o binário e os links, pronto pra montar no container.
fn prepare_bin_dir(binary: &Path) -> Result<PathBuf> {
    let dir = harness::paths::scratch_dir("f06-coreutils-find").join("uu-bin");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    std::fs::copy(binary, dir.join("uu-original"))?;
    for link in LINKS {
        std::os::unix::fs::symlink("uu-original", dir.join(link))?;
    }
    Ok(dir)
}

/// Roda `uu-original --demo-global-state` no host (num diretório de rascunho com `ok.txt`): `wc` e
/// `cat` originais chamados em sequência no mesmo processo.
pub fn global_state_demo() -> Result<serde_json::Value> {
    let binary = build_original()?;
    let dir = harness::paths::scratch_dir("f06-coreutils-find").join("global-state-demo");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("ok.txt"), "conteúdo\n")?;
    let out = Command::new(&binary).arg("--demo-global-state").current_dir(&dir).env_clear().output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let last = stdout.lines().last().unwrap_or_default();
    let codes: serde_json::Value = serde_json::from_str(last).unwrap_or(serde_json::Value::Null);
    Ok(serde_json::json!({
        "sequence": ["wc nope.txt", "cat ok.txt"],
        "exit_codes": codes,
        "expected_exit_codes": {"wc_missing_exit": 1, "cat_ok_exit": 0},
        "stderr": String::from_utf8_lossy(&out.stderr),
        "expected_stderr": "wc: nope.txt: No such file or directory\n",
    }))
}

/// Roda os casos no container com o uutils original na frente do PATH.
pub fn run(cases: &[Case]) -> Result<Vec<Outcome>> {
    let binary = build_original()?;
    let bin_dir = prepare_bin_dir(&binary)?;
    let oracle = Oracle::locate()?;
    let agent = format!("{}:/agent/oracle-agent:ro", oracle.agent.display());
    let bins = format!("{}:/usr/local/bin:ro", bin_dir.display());
    let input = serde_json::to_vec(cases)?;
    let mut child = Command::new("docker")
        .args(["run", "--rm", "-i", "--network", "none", "-v", &agent, "-v", &bins, &oracle.image])
        .arg("/agent/oracle-agent")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("docker run")?;
    let mut stdin = child.stdin.take().expect("stdin");
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output()?;
    writer.join().expect("writer").context("enviando casos")?;
    if !output.status.success() {
        bail!("oracle-agent falhou: {}", String::from_utf8_lossy(&output.stderr));
    }
    let outcomes: Vec<Outcome> = serde_json::from_slice(&output.stdout).context("saída do oracle-agent")?;
    if outcomes.len() != cases.len() {
        bail!("{} resultados pra {} casos", outcomes.len(), cases.len());
    }
    Ok(outcomes)
}
