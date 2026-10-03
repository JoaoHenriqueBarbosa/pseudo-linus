//! Formato de `results/<experimento>.json`, lido pelo runner pra montar o relatório.

use serde::{Deserialize, Serialize};

use crate::compare::Conformance;
use crate::paths;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Confirmed,
    Refuted,
    Partial,
    Inconclusive,
}

impl Verdict {
    pub fn label_pt(self) -> &'static str {
        match self {
            Verdict::Confirmed => "Confirmada",
            Verdict::Refuted => "Refutada",
            Verdict::Partial => "Parcial",
            Verdict::Inconclusive => "Inconclusiva",
        }
    }
}

/// Veredito de encaixe de um candidato.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Usa como dependência, do jeito que está.
    Fits,
    /// Usa, mas com fork ou camada nossa relevante por cima.
    FitsWithWork,
    /// Não serve.
    DoesNotFit,
    /// Não é candidato a dependência: referência ou linha de base.
    Reference,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HypothesisVerdict {
    pub id: String,
    pub verdict: Verdict,
    /// Uma ou duas frases com o número que decidiu.
    pub summary: String,
    #[serde(default)]
    pub evidence: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CandidateResult {
    pub name: String,
    pub version: String,
    /// Papel que o candidato disputa (ex.: "jq", "regex", "tmpfs").
    pub role: String,
    /// Categoria de acoplamento do depscan: "a", "b" ou "c".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conformance: Option<Conformance>,
    pub fit: Fit,
    pub notes: String,
    #[serde(default)]
    pub metrics: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostInfo {
    pub kernel: String,
    pub cpus: usize,
    pub rustc: String,
}

impl HostInfo {
    pub fn collect() -> HostInfo {
        let kernel = std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
        let rustc = std::process::Command::new("rustc")
            .arg("--version")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        HostInfo { kernel, cpus, rustc }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExperimentResult {
    /// Id do experimento, igual ao nome do diretório (ex.: "e01-exec-models").
    pub experiment: String,
    pub title: String,
    pub generated_at: String,
    pub host: HostInfo,
    pub hypotheses: Vec<HypothesisVerdict>,
    #[serde(default)]
    pub candidates: Vec<CandidateResult>,
    #[serde(default)]
    pub metrics: serde_json::Value,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl ExperimentResult {
    pub fn new(experiment: &str, title: &str) -> ExperimentResult {
        ExperimentResult {
            experiment: experiment.to_string(),
            title: title.to_string(),
            generated_at: humantime::format_rfc3339_seconds(std::time::SystemTime::now()).to_string(),
            host: HostInfo::collect(),
            hypotheses: Vec::new(),
            candidates: Vec::new(),
            metrics: serde_json::Value::Null,
            notes: Vec::new(),
        }
    }

    pub fn hypothesis(&mut self, id: &str, verdict: Verdict, summary: impl Into<String>, evidence: serde_json::Value) {
        self.hypotheses.push(HypothesisVerdict { id: id.to_string(), verdict, summary: summary.into(), evidence });
    }

    /// Grava em `results/<experimento>.json`.
    pub fn write(&self) -> anyhow::Result<std::path::PathBuf> {
        let dir = paths::results_dir();
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", self.experiment));
        std::fs::write(&path, serde_json::to_string_pretty(self)? + "\n")?;
        Ok(path)
    }

    pub fn load_all() -> anyhow::Result<Vec<ExperimentResult>> {
        let dir = paths::results_dir();
        let mut out = Vec::new();
        if !dir.exists() {
            return Ok(out);
        }
        let mut files: Vec<_> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        for path in files {
            let text = std::fs::read_to_string(&path)?;
            out.push(serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?);
        }
        Ok(out)
    }
}
