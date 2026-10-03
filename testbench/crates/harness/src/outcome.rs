use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Bytes, MemTree};

/// Entrada de um caso já resolvida: o que um candidato recebe pra executar.
#[derive(Clone, Debug)]
pub struct Invocation {
    pub case_id: String,
    /// `argv[0]` é o nome do programa (ex.: "grep"). Vazio quando o caso é `script`.
    pub argv: Vec<String>,
    /// Script bash (caso `script`).
    pub script: Option<String>,
    pub stdin: Vec<u8>,
    /// Arquivos do diretório de trabalho do caso.
    pub files: MemTree,
    /// Variáveis extras, por cima de [`crate::BASE_ENV`].
    pub env: BTreeMap<String, String>,
    pub faketime: Option<String>,
}

impl Invocation {
    pub fn program(&self) -> Option<&str> {
        self.argv.first().map(String::as_str)
    }

    pub fn args(&self) -> &[String] {
        self.argv.get(1..).unwrap_or(&[])
    }

    /// Ambiente completo que o caso enxerga.
    pub fn full_env(&self) -> BTreeMap<String, String> {
        let mut env: BTreeMap<String, String> =
            crate::BASE_ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        env.extend(self.env.clone());
        env
    }
}

/// O que um caso produziu.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub stdout: Bytes,
    pub stderr: Bytes,
    /// Código de saída, quando o processo terminou normalmente.
    pub exit: Option<i32>,
    /// Sinal que matou o processo, quando foi o caso.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<i32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub timed_out: bool,
    /// Retrato do diretório de trabalho depois da execução.
    pub files: MemTree,
    /// Preenchido quando o candidato não suporta o caso (conta como falha, categoria "unsupported").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported: Option<String>,
}

impl Outcome {
    pub fn unsupported(reason: impl Into<String>) -> Outcome {
        Outcome { unsupported: Some(reason.into()), ..Outcome::default() }
    }

    /// Saída "de programa" comum: stdout, stderr e exit, com os arquivos de entrada intactos.
    pub fn exited(stdout: impl Into<Bytes>, stderr: impl Into<Bytes>, exit: i32, files: MemTree) -> Outcome {
        Outcome { stdout: stdout.into(), stderr: stderr.into(), exit: Some(exit), files, ..Outcome::default() }
    }
}

/// Algo que sabe executar um caso: uma crate candidata embrulhada, um porte, ou um projeto de linha de base.
pub trait Candidate {
    /// Nome estável usado nos resultados (ex.: "jaq-core 3.1").
    fn name(&self) -> String;
    fn run(&self, inv: &Invocation) -> Outcome;
}
