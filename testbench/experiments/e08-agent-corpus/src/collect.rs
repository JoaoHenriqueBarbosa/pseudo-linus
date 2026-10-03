//! Extrai os comandos `Bash` dos transcripts locais do Claude Code.
//!
//! Só leitura. Os comandos extraídos vão pra `testbench/corpus/agent/commands.jsonl`, que é gitignored,
//! e **nunca são executados**: a bancada só parseia.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MinedCommand {
    pub command: String,
    /// Diretório do projeto no ~/.claude/projects (ex.: "-home-john-projects-x").
    pub project: String,
    pub session: String,
    /// Veio de um subagente (transcript em `subagents/`).
    pub subagent: bool,
}

/// Diretório padrão dos transcripts; `CLAUDE_PROJECTS_DIR` sobrescreve.
pub fn transcripts_dir() -> PathBuf {
    std::env::var_os("CLAUDE_PROJECTS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            home.join(".claude/projects")
        })
}

pub struct Collected {
    pub commands: Vec<MinedCommand>,
    pub files: usize,
    pub bad_lines: usize,
}

pub fn collect(root: &Path) -> Result<Collected> {
    let mut commands = Vec::new();
    let mut files = 0;
    let mut bad_lines = 0;
    for entry in walkdir::WalkDir::new(root).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !entry.file_type().is_file() || path.extension().is_none_or(|x| x != "jsonl") {
            continue;
        }
        files += 1;
        let rel = path.strip_prefix(root).unwrap_or(path);
        let project = rel
            .components()
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .unwrap_or_default();
        let session = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let subagent = rel.components().any(|c| c.as_os_str() == "subagents");
        let reader = std::io::BufReader::new(std::fs::File::open(path)?);
        for line in reader.split(b'\n') {
            let line = line?;
            // Filtro barato antes do parse completo: só linhas com chamada da tool Bash.
            if !contains(&line, br#""name":"Bash""#) || !contains(&line, br#""tool_use""#) {
                continue;
            }
            let Ok(value) = serde_json::from_slice::<serde_json::Value>(&line) else {
                bad_lines += 1;
                continue;
            };
            let Some(content) = value.pointer("/message/content").and_then(|c| c.as_array()) else {
                continue;
            };
            for item in content {
                let is_bash = item.get("type").and_then(|t| t.as_str()) == Some("tool_use")
                    && item.get("name").and_then(|n| n.as_str()) == Some("Bash");
                if !is_bash {
                    continue;
                }
                if let Some(cmd) = item.pointer("/input/command").and_then(|c| c.as_str()) {
                    commands.push(MinedCommand {
                        command: cmd.to_string(),
                        project: project.clone(),
                        session: session.clone(),
                        subagent,
                    });
                }
            }
        }
    }
    Ok(Collected { commands, files, bad_lines })
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    memchr::memmem::find(haystack, needle).is_some()
}
