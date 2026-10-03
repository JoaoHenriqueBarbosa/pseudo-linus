//! Cliente do oráculo: roda casos dentro do container Debian pinado (`testbench/oracle/Dockerfile`).
//!
//! O binário `oracle-agent` é compilado no host (mesma glibc 2.41 do container) e montado read-only.
//! Antes de usar, rode `cargo run -p oracle -- build`, que monta a imagem e compila o agente.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::{Case, Outcome, paths};

pub struct Oracle {
    pub image: String,
    pub agent: PathBuf,
}

impl Oracle {
    /// Tag da imagem derivada do conteúdo do Dockerfile, pra que mudar o Dockerfile force rebuild.
    pub fn image_tag() -> Result<String> {
        let dockerfile = std::fs::read(Self::dockerfile_dir().join("Dockerfile"))?;
        let hash = crate::memtree::sha256_hex(&dockerfile);
        Ok(format!("pseudo-linus-oracle:{}", &hash[..12]))
    }

    pub fn dockerfile_dir() -> PathBuf {
        paths::root().join("oracle")
    }

    /// Sempre o target do workspace da infraestrutura, mesmo quando chamado de um experimento
    /// (que tem target próprio).
    pub fn agent_path() -> PathBuf {
        paths::root().join("target").join("release").join("oracle-agent")
    }

    /// Localiza imagem e agente já prontos. Não compila nada (evita cargo aninhado dentro de `cargo test`).
    pub fn locate() -> Result<Oracle> {
        let image = Self::image_tag()?;
        let agent = Self::agent_path();
        if !agent.exists() {
            bail!("oracle-agent não encontrado em {}; rode `cargo run -p oracle -- build`", agent.display());
        }
        let ok = Command::new("docker")
            .args(["image", "inspect", &image])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("docker não disponível")?
            .success();
        if !ok {
            bail!("imagem {image} não existe; rode `cargo run -p oracle -- build`");
        }
        Ok(Oracle { image, agent })
    }

    /// Roda os casos num container novo (um container por chamada, casos em sequência dentro dele).
    pub fn run(&self, cases: &[Case]) -> Result<Vec<Outcome>> {
        let input = serde_json::to_vec(cases)?;
        let mount = format!("{}:/agent/oracle-agent:ro", self.agent.display());
        let mut child = Command::new("docker")
            .args(["run", "--rm", "-i", "--network", "none", "-v", &mount, &self.image, "/agent/oracle-agent"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("docker run")?;
        let mut stdin = child.stdin.take().expect("stdin");
        let writer = std::thread::spawn(move || stdin.write_all(&input));
        let output = child.wait_with_output()?;
        writer.join().expect("writer").context("enviando casos pro agente")?;
        if !output.status.success() {
            bail!("oracle-agent falhou: {}", String::from_utf8_lossy(&output.stderr));
        }
        let outcomes: Vec<Outcome> = serde_json::from_slice(&output.stdout).context("saída do oracle-agent")?;
        if outcomes.len() != cases.len() {
            bail!("oracle-agent devolveu {} resultados pra {} casos", outcomes.len(), cases.len());
        }
        Ok(outcomes)
    }

    /// Atalho pra um script avulso (útil em experimentos de interoperabilidade).
    pub fn run_script(&self, id: &str, script: &str, files: crate::MemTree) -> Result<Outcome> {
        let mut case = Case {
            id: id.to_string(),
            argv: Vec::new(),
            script: Some(script.to_string()),
            stdin: None,
            stdin_b64: None,
            files: Default::default(),
            env: Default::default(),
            tags: Vec::new(),
            faketime: None,
            timeout_ms: None,
        };
        for (path, entry) in files.entries {
            case.files.insert(path, entry_to_spec(&entry)?);
        }
        Ok(self.run(&[case])?.remove(0))
    }
}

fn entry_to_spec(entry: &crate::Entry) -> Result<crate::FileSpec> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use crate::case::{FileSpec, FileTable};
    Ok(match entry {
        crate::Entry::File { mode, data: Some(d), .. } => FileSpec::Table(FileTable {
            content_b64: Some(STANDARD.encode(d.as_slice())),
            mode: Some(*mode),
            ..FileTable::default()
        }),
        crate::Entry::File { size, .. } => bail!("arquivo de {size} bytes sem conteúdo"),
        crate::Entry::Dir { mode } => FileSpec::Table(FileTable { dir: true, mode: Some(*mode), ..FileTable::default() }),
        crate::Entry::Symlink { target } => {
            FileSpec::Table(FileTable { symlink: Some(target.clone()), ..FileTable::default() })
        }
    })
}
