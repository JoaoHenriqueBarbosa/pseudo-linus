use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

use crate::memtree::{Entry, MemTree};
use crate::outcome::Invocation;

/// Um arquivo `corpus/cases/<tool>/<nome>.toml`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaseFile {
    /// Ferramenta principal do arquivo (informativo; cada caso pode ter argv próprio).
    #[serde(default)]
    pub tool: Option<String>,
    /// Origem dos casos (ex.: "manual", "agent-style", "upstream:jq.test").
    #[serde(default)]
    pub source: Option<String>,
    #[serde(rename = "case", default)]
    pub cases: Vec<Case>,
}

/// Um caso: ou `argv` (roda o programa direto, sem shell), ou `script` (roda `bash -c`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Case {
    pub id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdin_b64: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, FileSpec>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Data e hora fixas pro `faketime` (ex.: "2026-01-15 12:00:00").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faketime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

/// Conteúdo de um arquivo de fixture: string direta ou tabela.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileSpec {
    Text(String),
    Table(FileTable),
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FileTable {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub content_b64: Option<String>,
    #[serde(default)]
    pub mode: Option<u32>,
    #[serde(default)]
    pub symlink: Option<String>,
    #[serde(default)]
    pub dir: bool,
}

impl FileSpec {
    pub fn to_entry(&self) -> Result<Entry> {
        Ok(match self {
            FileSpec::Text(s) => Entry::file(s.as_bytes().to_vec(), 0o644),
            FileSpec::Table(t) => {
                if let Some(target) = &t.symlink {
                    Entry::symlink(target.clone())
                } else if t.dir {
                    Entry::dir(t.mode.unwrap_or(0o755))
                } else {
                    let data = match (&t.content, &t.content_b64) {
                        (Some(c), None) => c.as_bytes().to_vec(),
                        (None, Some(b)) => STANDARD.decode(b.as_bytes())?,
                        (None, None) => Vec::new(),
                        (Some(_), Some(_)) => bail!("content e content_b64 ao mesmo tempo"),
                    };
                    Entry::file(data, t.mode.unwrap_or(0o644))
                }
            }
        })
    }
}

impl Case {
    pub fn stdin_bytes(&self) -> Result<Vec<u8>> {
        Ok(match (&self.stdin, &self.stdin_b64) {
            (Some(s), None) => s.as_bytes().to_vec(),
            (None, Some(b)) => STANDARD.decode(b.as_bytes())?,
            (None, None) => Vec::new(),
            (Some(_), Some(_)) => bail!("{}: stdin e stdin_b64 ao mesmo tempo", self.id),
        })
    }

    pub fn fixture(&self) -> Result<MemTree> {
        let mut tree = MemTree::new();
        for (path, spec) in &self.files {
            tree.insert(path, spec.to_entry().with_context(|| format!("{}: {path}", self.id))?);
        }
        Ok(tree)
    }

    pub fn validate(&self) -> Result<()> {
        match (self.argv.is_empty(), self.script.is_some()) {
            (false, false) | (true, true) => Ok(()),
            (true, false) => bail!("{}: caso sem argv nem script", self.id),
            (false, true) => bail!("{}: caso com argv e script ao mesmo tempo", self.id),
        }
    }

    pub fn invocation(&self) -> Result<Invocation> {
        self.validate()?;
        Ok(Invocation {
            case_id: self.id.clone(),
            argv: self.argv.clone(),
            script: self.script.clone(),
            stdin: self.stdin_bytes()?,
            files: self.fixture()?,
            env: self.env.clone(),
            faketime: self.faketime.clone(),
        })
    }
}

impl CaseFile {
    pub fn load(path: &Path) -> Result<CaseFile> {
        let text = std::fs::read_to_string(path).with_context(|| format!("lendo {}", path.display()))?;
        let file: CaseFile = toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
        let mut seen = std::collections::BTreeSet::new();
        for case in &file.cases {
            case.validate()?;
            if !seen.insert(case.id.clone()) {
                bail!("{}: id repetido {}", path.display(), case.id);
            }
        }
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tool_and_script_cases() {
        let text = r#"
tool = "grep"
[[case]]
id = "a"
argv = ["grep", "-n", "x", "in.txt"]
tags = ["bre"]
[case.files]
"in.txt" = "x\ny\n"
"bin.dat" = { content_b64 = "AAE=" }
"link" = { symlink = "in.txt" }
"sub" = { dir = true, mode = 0o700 }

[[case]]
id = "b"
script = "printf 'a\\n' | grep a"
stdin = "ignored"
"#;
        let file: CaseFile = toml::from_str(text).unwrap();
        assert_eq!(file.cases.len(), 2);
        let inv = file.cases[0].invocation().unwrap();
        assert_eq!(inv.files.read("in.txt"), Some(&b"x\ny\n"[..]));
        assert_eq!(inv.files.read("bin.dat"), Some(&[0u8, 1][..]));
        assert!(matches!(inv.files.get("sub"), Some(Entry::Dir { mode: 0o700 })));
        assert!(file.cases[1].invocation().unwrap().script.is_some());
    }
}
