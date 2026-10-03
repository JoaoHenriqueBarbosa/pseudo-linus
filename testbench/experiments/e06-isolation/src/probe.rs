//! Tipos das sondas de syscall que o subprocesso devolve em JSON pro processo principal.

use std::io;

use serde::{Deserialize, Serialize};

/// Resultado de uma chamada: sucesso, ou o errno que voltou.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallResult {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errno: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errno_name: Option<String>,
    /// Detalhe livre (bytes lidos, mensagem de erro que não é errno).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

impl CallResult {
    pub fn ok(detail: impl Into<String>) -> CallResult {
        CallResult { ok: true, errno: None, errno_name: None, detail: detail.into() }
    }

    pub fn errno(code: i32) -> CallResult {
        CallResult { ok: false, errno: Some(code), errno_name: Some(errno_name(code)), detail: String::new() }
    }

    pub fn failed(detail: impl Into<String>) -> CallResult {
        CallResult { ok: false, errno: None, errno_name: None, detail: detail.into() }
    }

    pub fn from_io<T>(r: &io::Result<T>, ok_detail: impl FnOnce(&T) -> String) -> CallResult {
        match r {
            Ok(v) => CallResult::ok(ok_detail(v)),
            Err(e) => match e.raw_os_error() {
                Some(code) => CallResult::errno(code),
                None => CallResult::failed(e.to_string()),
            },
        }
    }

    pub fn from_rustix<T>(r: &rustix::io::Result<T>) -> CallResult {
        match r {
            Ok(_) => CallResult::ok(""),
            Err(e) => CallResult::errno(e.raw_os_error()),
        }
    }
}

/// Nome simbólico do errno (`EACCES`, `EPERM`...).
pub fn errno_name(code: i32) -> String {
    format!("{:?}", nix::errno::Errno::from_raw(code))
}

/// O que se espera da chamada.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Expect {
    /// A chamada precisa funcionar.
    Allowed,
    /// A chamada precisa falhar com este errno.
    Errno(i32),
}

/// Papel da sonda no veredito.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Decide o veredito da hipótese (critério do `hypotheses.toml`).
    Criterion,
    /// Comportamento registrado pro design, não entra no veredito.
    Finding,
    /// Controle do próprio experimento: se falhar, é bug nosso.
    Control,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Probe {
    /// Quem fez a chamada: `restricted`, `neighbor`, `child_of_restricted`, `main`...
    pub thread: String,
    pub action: String,
    pub target: String,
    pub role: Role,
    pub expect: Expect,
    pub result: CallResult,
}

impl Probe {
    pub fn new(thread: &str, action: &str, target: impl Into<String>, role: Role, expect: Expect, result: CallResult) -> Probe {
        Probe { thread: thread.to_string(), action: action.to_string(), target: target.into(), role, expect, result }
    }

    pub fn matches(&self) -> bool {
        match self.expect {
            Expect::Allowed => self.result.ok,
            Expect::Errno(code) => !self.result.ok && self.result.errno == Some(code),
        }
    }
}

/// Sondas de um papel que não bateram com o esperado.
pub fn mismatches(probes: &[Probe], role: Role) -> Vec<&Probe> {
    probes.iter().filter(|p| p.role == role && !p.matches()).collect()
}
