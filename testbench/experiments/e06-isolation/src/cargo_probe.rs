//! Roda `cargo clippy`/`cargo build` no workspace das sondas e lê os diagnósticos do JSON do cargo.

use std::ffi::OsString;
use std::process::Command;
use std::time::Instant;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::layout;

/// Variáveis que um cargo externo (ou o shell) pode ter deixado e que mudariam o build das sondas.
const SCRUBBED_ENV: &[&str] = &[
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_TARGET_DIR",
    "CLIPPY_CONF_DIR",
    "CLIPPY_ARGS",
    "CARGO_MAKEFLAGS",
];

/// Um diagnóstico do compilador (ou do clippy) atribuído a um target.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Nome do target que gerou o diagnóstico (ex.: `forbid_consumer`).
    pub target: String,
    pub level: String,
    /// Código do lint ou do erro (`unsafe_code`, `E0453`, `clippy::disallowed_methods`).
    pub code: Option<String>,
    pub message: String,
    /// Arquivo do span primário, relativo à raiz do workspace das sondas.
    pub file: Option<String>,
    pub line: Option<u64>,
}

/// Resultado de uma invocação do cargo.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CargoRun {
    pub command: String,
    pub exit_code: Option<i32>,
    /// Campo `success` da mensagem `build-finished`.
    pub build_finished: Option<bool>,
    pub diagnostics: Vec<Diagnostic>,
    pub seconds: f64,
    /// Fim do stderr, pra diagnosticar falha que não veio como mensagem do compilador.
    pub stderr_tail: String,
}

impl CargoRun {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0)
    }

    /// Diagnósticos de um target (nome com `_`, como o cargo reporta).
    pub fn diagnostics_for<'a>(&'a self, target: &'a str) -> impl Iterator<Item = &'a Diagnostic> + 'a {
        self.diagnostics.iter().filter(move |d| d.target == target)
    }
}

pub fn cargo_bin() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

/// Argumentos comuns pra rodar `subcommand` num pacote do workspace das sondas.
pub fn probe_args(subcommand: &str, package: &str, feature: Option<&str>) -> Vec<String> {
    let mut args = vec![
        subcommand.to_string(),
        "--release".to_string(),
        "--manifest-path".to_string(),
        layout::probes_manifest().display().to_string(),
        "--target-dir".to_string(),
        layout::probes_target_dir().display().to_string(),
        "-p".to_string(),
        package.to_string(),
        "--message-format=json".to_string(),
    ];
    if let Some(f) = feature {
        args.push("--features".to_string());
        args.push(f.to_string());
    }
    args
}

/// Roda o cargo com `args` e coleta os diagnósticos.
pub fn run(args: &[String]) -> Result<CargoRun> {
    let cargo = cargo_bin();
    let mut cmd = Command::new(&cargo);
    cmd.args(args);
    for var in SCRUBBED_ENV {
        cmd.env_remove(var);
    }
    let command = format!("{} {}", cargo.to_string_lossy(), args.join(" "));
    let start = Instant::now();
    let out = cmd.output().with_context(|| format!("rodar {command}"))?;
    let seconds = start.elapsed().as_secs_f64();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let (diagnostics, build_finished) = parse_messages(&stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stderr_tail: String = {
        let lines: Vec<&str> = stderr.lines().collect();
        lines[lines.len().saturating_sub(15)..].join("\n")
    };
    Ok(CargoRun { command, exit_code: out.status.code(), build_finished, diagnostics, seconds, stderr_tail })
}

/// Lê as linhas JSON do `--message-format=json`. Mensagens de resumo do rustc (sem span e sem
/// código, como "aborting due to 1 previous error") são descartadas.
pub fn parse_messages(stdout: &str) -> (Vec<Diagnostic>, Option<bool>) {
    let mut diagnostics = Vec::new();
    let mut build_finished = None;
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        match v.get("reason").and_then(Value::as_str) {
            Some("build-finished") => build_finished = v.get("success").and_then(Value::as_bool),
            Some("compiler-message") => {
                let Some(msg) = v.get("message") else { continue };
                let code = msg.pointer("/code/code").and_then(Value::as_str).map(str::to_string);
                let spans = msg.get("spans").and_then(Value::as_array).cloned().unwrap_or_default();
                let primary = spans
                    .iter()
                    .find(|s| s.get("is_primary").and_then(Value::as_bool) == Some(true))
                    .or_else(|| spans.first());
                if code.is_none() && primary.is_none() {
                    continue;
                }
                diagnostics.push(Diagnostic {
                    target: v.pointer("/target/name").and_then(Value::as_str).unwrap_or_default().to_string(),
                    level: msg.get("level").and_then(Value::as_str).unwrap_or_default().to_string(),
                    code,
                    message: msg.get("message").and_then(Value::as_str).unwrap_or_default().to_string(),
                    file: primary.and_then(|s| s.get("file_name")).and_then(Value::as_str).map(str::to_string),
                    line: primary.and_then(|s| s.get("line_start")).and_then(Value::as_u64),
                });
            }
            _ => {}
        }
    }
    (diagnostics, build_finished)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_compiler_messages_and_skips_summaries() {
        let stdout = concat!(
            r#"{"reason":"compiler-message","target":{"name":"forbid_consumer"},"message":{"level":"error","message":"usage of an `unsafe` block","code":{"code":"unsafe_code"},"spans":[{"file_name":"forbid-consumer/src/a.rs","line_start":5,"is_primary":true}]}}"#,
            "\n",
            r#"{"reason":"compiler-message","target":{"name":"forbid_consumer"},"message":{"level":"error","message":"aborting due to 1 previous error","code":null,"spans":[]}}"#,
            "\n",
            "not json\n",
            r#"{"reason":"build-finished","success":false}"#,
            "\n"
        );
        let (diags, finished) = parse_messages(stdout);
        assert_eq!(finished, Some(false));
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("unsafe_code"));
        assert_eq!(diags[0].file.as_deref(), Some("forbid-consumer/src/a.rs"));
        assert_eq!(diags[0].line, Some(5));
        assert_eq!(diags[0].target, "forbid_consumer");
    }
}
