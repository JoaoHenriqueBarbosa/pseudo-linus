//! Parâmetros e resultados dos métodos públicos do JSON-RPC.
//!
//! Os parâmetros recusam campo desconhecido (`deny_unknown_fields`): um agente que erra o nome de um
//! campo recebe `invalid_params` em vez de ver o campo ignorado em silêncio.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::CpuMax;

/// Bytes como texto base64 nos frames do IPC e nas respostas binárias.
pub mod b64 {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn encode(b: &[u8]) -> String {
        STANDARD.encode(b)
    }

    pub fn decode(s: &str) -> Result<Vec<u8>, String> {
        STANDARD.decode(s.as_bytes()).map_err(|e| format!("base64 inválido: {e}"))
    }

    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&encode(v))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        decode(&s).map_err(serde::de::Error::custom)
    }
}

/// Como bytes saem numa resposta.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Encoding {
    /// Texto; bytes que não são UTF-8 viram U+FFFD e o campo `*_lossy` fica verdadeiro.
    #[default]
    Utf8,
    Base64,
}

/// Bytes codificados como pedido.
pub fn encode_bytes(b: &[u8], enc: Encoding) -> (String, bool) {
    match enc {
        Encoding::Base64 => (b64::encode(b), false),
        Encoding::Utf8 => match std::str::from_utf8(b) {
            Ok(s) => (s.to_string(), false),
            Err(_) => (String::from_utf8_lossy(b).into_owned(), true),
        },
    }
}

/// Limites pedidos pra uma sandbox nova (o que faltar vem do padrão da configuração).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LimitsRequest {
    pub mem_bytes: Option<u64>,
    pub max_procs: Option<u32>,
    pub fs_bytes: Option<u64>,
    pub nofile: Option<u64>,
    /// Peso da sandbox dentro do grupo do usuário (1 a 10000).
    pub cpu_weight: Option<u32>,
    /// Teto de banda da própria sandbox, abaixo do teto do usuário.
    pub cpu_max: Option<CpuMax>,
}

/// Limites efetivos de uma sandbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxLimits {
    pub mem_bytes: u64,
    pub max_procs: u32,
    pub fs_bytes: u64,
    pub nofile: u64,
    pub cpu_weight: u32,
    pub cpu_max: Option<CpuMax>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SandboxCreateParams {
    /// Imagem base; hoje só existe `default` (Debian 13 com o userland do pseudo-linus).
    pub image: Option<String>,
    pub limits: LimitsRequest,
    pub hostname: Option<String>,
    /// Ambiente base de todo `exec` e sessão da sandbox (por cima do padrão).
    pub env: BTreeMap<String, String>,
    /// Diretório de trabalho padrão (cwd de `exec` e de sessão nova). Padrão: `/root`.
    pub workdir: Option<String>,
    /// Rótulos livres do cliente, devolvidos no `sandbox.list`.
    pub labels: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxInfo {
    pub sandbox_id: String,
    pub owner: String,
    pub image: String,
    pub hostname: String,
    pub workdir: String,
    pub limits: SandboxLimits,
    pub labels: BTreeMap<String, String>,
    pub created_at: u64,
    pub last_used_at: u64,
    /// `active`, `recovering` (subindo de novo a partir de um snapshot persistido depois da queda do
    /// worker) ou `lost`.
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lost_reason: Option<String>,
    /// Snapshot persistido de onde a sandbox foi recuperada depois de uma queda.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovered_from: Option<String>,
    pub worker: usize,
    pub sessions: Vec<String>,
    pub snapshots: Vec<SnapshotInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotInfo {
    pub snapshot_id: String,
    pub name: String,
    pub created_at: u64,
    pub persisted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persisted_bytes: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxRef {
    pub sandbox_id: String,
}

/// Parâmetros de `exec` e `exec.stream`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ExecParams {
    pub sandbox_id: String,
    /// Linha de comando pro `bash -c`. Exatamente um de `command` e `argv`.
    pub command: Option<String>,
    /// Programa e argumentos, sem shell (busca no PATH como o `execvp`).
    pub argv: Option<Vec<String>>,
    pub cwd: Option<String>,
    /// Variáveis acrescentadas ao ambiente da sandbox.
    pub env: BTreeMap<String, String>,
    /// Começa de um ambiente vazio (só `env`).
    pub clear_env: bool,
    pub stdin: Option<String>,
    pub stdin_base64: Option<String>,
    /// Stdin em fluxo: depois do `stdin`/`stdin_base64` (que vão primeiro), o processo lê os pedaços
    /// mandados por `exec.stdin` com este id, até um com `eof`. Id escolhido pelo cliente (até 64
    /// letras, dígitos, `-` e `_`), único entre os execs em andamento da sandbox.
    pub stdin_id: Option<String>,
    pub timeout_ms: Option<u64>,
    /// Limite de saída guardada por fluxo.
    pub output_limit_bytes: Option<u64>,
    pub encoding: Encoding,
}

/// Parâmetros de `exec.stdin`: um pedaço do stdin em fluxo de um `exec` com `stdin_id`. A resposta
/// só vem quando o pedaço entrou no pipe do processo (ou o processo já terminou).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ExecStdinParams {
    pub sandbox_id: String,
    pub stdin_id: String,
    pub data: Option<String>,
    pub data_base64: Option<String>,
    /// Fecha o stdin do processo depois deste pedaço.
    pub eof: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecResult {
    /// Código de saída, quando o processo saiu normalmente.
    pub exit_code: Option<i32>,
    /// Sinal que matou o processo.
    pub signal: Option<i32>,
    pub signal_name: Option<String>,
    /// O `$?` que o bash veria: o código de saída, ou 128 + sinal.
    pub status: i32,
    pub timed_out: bool,
    pub cancelled: bool,
    pub duration_ms: u64,
    pub encoding: Encoding,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// Total produzido em cada fluxo, inclusive o que passou do limite e foi descartado.
    pub stdout_bytes: u64,
    pub stderr_bytes: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stdout_lossy: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stderr_lossy: bool,
    /// A saída passou do teto de descarte e o host fechou o pipe (o escritor leva SIGPIPE).
    #[serde(default, skip_serializing_if = "is_false")]
    pub output_closed: bool,
    /// Processos em segundo plano ainda seguravam a saída quando o comando terminou; o host parou de
    /// ler depois do tempo de escoamento.
    #[serde(default, skip_serializing_if = "is_false")]
    pub background_detached: bool,
    /// A saída foi entregue em notificações `exec.output` (streaming); `stdout`/`stderr` vêm vazios.
    #[serde(default, skip_serializing_if = "is_false")]
    pub streamed: bool,
    /// Só em `session.exec`: cwd do shell depois do comando.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Só em `session.exec`: o shell foi morto (timeout) e uma sessão nova subiu com o cwd e as
    /// variáveis exportadas do último comando que terminou.
    #[serde(default, skip_serializing_if = "is_false")]
    pub session_reset: bool,
    /// Só em `session.exec`: o shell saiu (`exit`) e a sessão acabou.
    #[serde(default, skip_serializing_if = "is_false")]
    pub session_closed: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SessionOpenParams {
    pub sandbox_id: String,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SessionExecParams {
    pub session_id: String,
    pub command: String,
    pub stdin: Option<String>,
    pub stdin_base64: Option<String>,
    pub timeout_ms: Option<u64>,
    pub output_limit_bytes: Option<u64>,
    pub encoding: Encoding,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRef {
    pub session_id: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FsReadParams {
    pub sandbox_id: String,
    pub path: String,
    pub offset: u64,
    /// Bytes a ler (padrão: até o fim, limitado pelo teto de resposta).
    pub length: Option<u64>,
    pub encoding: Encoding,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FsWriteParams {
    pub sandbox_id: String,
    pub path: String,
    pub data: Option<String>,
    pub data_base64: Option<String>,
    /// Modo de arquivo novo (padrão 0644, com a umask 022 já aplicada).
    pub mode: Option<u32>,
    pub append: bool,
    /// Falha se o arquivo já existe.
    pub exclusive: bool,
    /// Cria os diretórios que faltarem (`mkdir -p` do pai).
    pub create_parents: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FsPathParams {
    pub sandbox_id: String,
    pub path: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FsStatParams {
    pub sandbox_id: String,
    pub path: String,
    /// Segue o symlink final (`stat`); falso faz `lstat`. Padrão: falso.
    pub follow: bool,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FsMkdirParams {
    pub sandbox_id: String,
    pub path: String,
    pub parents: bool,
    pub mode: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct FsRemoveParams {
    pub sandbox_id: String,
    pub path: String,
    pub recursive: bool,
    /// Não reclama se o caminho não existe (`rm -f`).
    pub force: bool,
}

/// Uma entrada de `fs.list` e o resultado de `fs.stat`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileInfo {
    pub name: String,
    /// `file`, `dir`, `symlink`, `fifo`, `char`, `block` ou `socket`.
    #[serde(rename = "type")]
    pub kind: String,
    pub size: u64,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub nlink: u64,
    pub mtime: i64,
    pub mtime_nsec: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symlink_target: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SnapshotParams {
    pub sandbox_id: String,
    pub name: Option<String>,
    /// Grava o snapshot em disco (tar), o que permite recuperar a sandbox se o worker cair.
    pub persist: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRef {
    pub sandbox_id: String,
    pub snapshot_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KillParams {
    pub sandbox_id: String,
    pub pid: i32,
    /// Número ou nome (`TERM`, `SIGKILL`); padrão SIGTERM.
    #[serde(default)]
    pub signal: Option<serde_json::Value>,
    /// Manda pro grupo de processos `pid` (`kill -- -pid`).
    #[serde(default)]
    pub group: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessInfo {
    pub pid: i32,
    pub ppid: i32,
    pub pgid: i32,
    pub sid: i32,
    pub state: String,
    pub comm: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ExportParams {
    pub sandbox_id: String,
    /// Diretório (ou arquivo) a exportar; padrão `/`.
    pub path: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ImportParams {
    pub sandbox_id: String,
    /// Diretório de destino (criado se faltar); padrão `/`.
    pub path: Option<String>,
    pub data_base64: String,
}

/// Uso corrente de uma sandbox.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxUsage {
    pub procs: u32,
    pub mem_bytes: u64,
    pub fs_bytes: u64,
    pub cpu_ns: u64,
}
