//! Protocolo entre o supervisor e os workers.
//!
//! O worker é o mesmo binário (`pseudo-linusd worker`), filho do supervisor. O supervisor escreve no
//! stdin dele e lê do stdout; o stderr do worker vai pro log do supervisor. Cada mensagem é um quadro:
//! 4 bytes de tamanho (big endian) e um JSON. Bytes viajam em base64.
//!
//! Chamadas levam um id; o worker atende cada uma numa thread e responde com o mesmo id, em qualquer
//! ordem. Chamadas com streaming mandam eventos de saída com o id antes da resposta.

use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::api::{FileInfo, ProcessInfo, SandboxUsage, b64};
use crate::backend::{BackendInfo, SandboxSpec, UserSched};
use crate::exec::{ExecOutcome, Stream};
use crate::rpc::RpcError;
use crate::session::SessionOutcome;
use crate::tarball::TarReport;

/// Maior quadro aceito (o maior payload legítimo é um tar de import/export dentro do teto do serviço).
pub const MAX_FRAME: usize = 512 << 20;

/// Limites de uma execução, em forma serializável.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireLimits {
    pub timeout_ms: u64,
    pub output_limit: u64,
    pub max_discard: u64,
    pub drain_grace_ms: u64,
}

impl WireLimits {
    pub fn to_exec(self) -> crate::exec::ExecLimits {
        crate::exec::ExecLimits {
            timeout: std::time::Duration::from_millis(self.timeout_ms),
            output_limit: self.output_limit,
            max_discard: self.max_discard,
            drain_grace: std::time::Duration::from_millis(self.drain_grace_ms),
        }
    }
}

/// `exec` já resolvido pelo supervisor (ambiente completo, cwd absoluto, limites efetivos).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecCall {
    pub command: Option<String>,
    pub argv: Option<Vec<String>>,
    pub cwd: String,
    pub env: Vec<String>,
    #[serde(with = "b64")]
    pub stdin: Vec<u8>,
    /// Stdin em fluxo: os pedaços chegam depois por `Call::ExecStdin` com este id (o `stdin` acima vai
    /// primeiro).
    #[serde(default)]
    pub stdin_id: Option<String>,
    pub limits: WireLimits,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Call {
    Ping,
    CreateSandbox {
        sandbox_id: String,
        user: UserSched,
        spec: SandboxSpec,
    },
    /// Cria a sandbox e importa o tar de um snapshot persistido (recuperação depois de queda).
    RecoverSandbox {
        sandbox_id: String,
        user: UserSched,
        spec: SandboxSpec,
        tar_path: String,
        max_bytes: u64,
    },
    DestroySandbox {
        sandbox_id: String,
    },
    /// Reaplica peso e teto de CPU do grupo do usuário.
    UpdateUser {
        user: UserSched,
    },
    Exec {
        sandbox_id: String,
        exec: ExecCall,
        stream: bool,
    },
    /// Um pedaço do stdin em fluxo de um `Exec` da sandbox. Só responde quando o pedaço entrou no
    /// pipe do processo (ou o exec terminou), o que dá contrapressão até o cliente.
    ExecStdin {
        sandbox_id: String,
        stdin_id: String,
        #[serde(with = "b64")]
        data: Vec<u8>,
        eof: bool,
    },
    SessionOpen {
        sandbox_id: String,
        session_id: String,
        cwd: String,
        env: Vec<String>,
        workdir: String,
        /// Dump do shell de uma sessão recriada depois da queda do worker (vazio numa sessão nova).
        #[serde(default, with = "b64")]
        dump: Vec<u8>,
    },
    SessionExec {
        session_id: String,
        command: String,
        #[serde(with = "b64")]
        stdin: Vec<u8>,
        limits: WireLimits,
        stream: bool,
    },
    SessionClose {
        session_id: String,
    },
    FsRead {
        sandbox_id: String,
        path: String,
        offset: u64,
        max: u64,
    },
    FsWrite {
        sandbox_id: String,
        path: String,
        #[serde(with = "b64")]
        data: Vec<u8>,
        mode: u32,
        append: bool,
        exclusive: bool,
        create_parents: bool,
    },
    FsList {
        sandbox_id: String,
        path: String,
    },
    FsStat {
        sandbox_id: String,
        path: String,
        follow: bool,
    },
    FsMkdir {
        sandbox_id: String,
        path: String,
        parents: bool,
        mode: u32,
    },
    FsRemove {
        sandbox_id: String,
        path: String,
        recursive: bool,
        force: bool,
    },
    Snapshot {
        sandbox_id: String,
        snapshot_id: String,
        /// Grava também um tar da sandbox neste arquivo do host (snapshot persistido).
        persist_to: Option<String>,
        max_bytes: u64,
    },
    Restore {
        sandbox_id: String,
        snapshot_id: String,
    },
    DropSnapshot {
        sandbox_id: String,
        snapshot_id: String,
    },
    Ps {
        sandbox_id: String,
    },
    Kill {
        sandbox_id: String,
        pid: i32,
        signal: i32,
        group: bool,
    },
    Export {
        sandbox_id: String,
        path: String,
        max_bytes: u64,
    },
    Import {
        sandbox_id: String,
        path: String,
        #[serde(with = "b64")]
        data: Vec<u8>,
        max_bytes: u64,
    },
    Usage {
        sandbox_id: String,
    },
}

impl Call {
    /// Nome curto pro log.
    pub fn name(&self) -> &'static str {
        match self {
            Call::Ping => "ping",
            Call::CreateSandbox { .. } => "create_sandbox",
            Call::RecoverSandbox { .. } => "recover_sandbox",
            Call::DestroySandbox { .. } => "destroy_sandbox",
            Call::UpdateUser { .. } => "update_user",
            Call::Exec { .. } => "exec",
            Call::ExecStdin { .. } => "exec_stdin",
            Call::SessionOpen { .. } => "session_open",
            Call::SessionExec { .. } => "session_exec",
            Call::SessionClose { .. } => "session_close",
            Call::FsRead { .. } => "fs_read",
            Call::FsWrite { .. } => "fs_write",
            Call::FsList { .. } => "fs_list",
            Call::FsStat { .. } => "fs_stat",
            Call::FsMkdir { .. } => "fs_mkdir",
            Call::FsRemove { .. } => "fs_remove",
            Call::Snapshot { .. } => "snapshot",
            Call::Restore { .. } => "restore",
            Call::DropSnapshot { .. } => "drop_snapshot",
            Call::Ps { .. } => "ps",
            Call::Kill { .. } => "kill",
            Call::Export { .. } => "export",
            Call::Import { .. } => "import",
            Call::Usage { .. } => "usage",
        }
    }
}

// A mensagem vira JSON logo depois de montada; o tamanho da variante não importa.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ToWorker {
    Call { id: u64, call: Call },
    Cancel { id: u64 },
    /// Mata todas as sandboxes e sai.
    Shutdown,
}

/// Estado do worker no ping.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerStatus {
    pub sandboxes: u32,
    pub sessions: u32,
    pub calls_in_flight: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "r", rename_all = "snake_case")]
pub enum Reply {
    Pong {
        status: WorkerStatus,
    },
    Ok,
    Exec {
        outcome: ExecOutcome,
    },
    Session {
        outcome: SessionOutcome,
    },
    FsRead {
        #[serde(with = "b64")]
        data: Vec<u8>,
        size: u64,
        eof: bool,
    },
    FsList {
        entries: Vec<FileInfo>,
    },
    FsStat {
        info: FileInfo,
    },
    Snapshot {
        persisted_bytes: Option<u64>,
    },
    Ps {
        processes: Vec<ProcessInfo>,
    },
    Export {
        #[serde(with = "b64")]
        data: Vec<u8>,
        report: TarReport,
    },
    Import {
        report: TarReport,
    },
    Usage {
        usage: SandboxUsage,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum FromWorker {
    Ready {
        pid: u32,
        info: BackendInfo,
    },
    Reply {
        id: u64,
        result: Result<Reply, RpcError>,
    },
    Event {
        id: u64,
        stream: Stream,
        #[serde(with = "b64")]
        data: Vec<u8>,
    },
}

/// Escreve um quadro.
pub fn write_frame<T: Serialize>(w: &mut impl Write, msg: &T) -> io::Result<()> {
    let body = serde_json::to_vec(msg).map_err(io::Error::other)?;
    if body.len() > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("quadro de {} bytes passa do teto", body.len())));
    }
    w.write_all(&(body.len() as u32).to_be_bytes())?;
    w.write_all(&body)?;
    w.flush()
}

/// Lê um quadro; `None` no fim limpo do fluxo.
pub fn read_frame<T: DeserializeOwned>(r: &mut impl Read) -> io::Result<Option<T>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("quadro de {n} bytes passa do teto")));
    }
    let mut body = Vec::new();
    body.try_reserve_exact(n).map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "quadro grande demais"))?;
    body.resize(n, 0);
    r.read_exact(&mut body)?;
    serde_json::from_slice(&body).map(Some).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Versões assíncronas pro supervisor.
pub mod tokio_io {
    use super::*;
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

    pub async fn write_frame<T: Serialize, W: AsyncWrite + Unpin>(w: &mut W, msg: &T) -> io::Result<()> {
        let body = serde_json::to_vec(msg).map_err(io::Error::other)?;
        if body.len() > MAX_FRAME {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "quadro passa do teto"));
        }
        w.write_all(&(body.len() as u32).to_be_bytes()).await?;
        w.write_all(&body).await?;
        w.flush().await
    }

    pub async fn read_frame<T: DeserializeOwned, R: AsyncRead + Unpin>(r: &mut R) -> io::Result<Option<T>> {
        let mut len = [0u8; 4];
        match r.read_exact(&mut len).await {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }
        let n = u32::from_be_bytes(len) as usize;
        if n > MAX_FRAME {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("quadro de {n} bytes passa do teto")));
        }
        let mut body = Vec::new();
        body.try_reserve_exact(n).map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "quadro grande demais"))?;
        body.resize(n, 0);
        r.read_exact(&mut body).await?;
        serde_json::from_slice(&body).map(Some).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let msgs = vec![
            ToWorker::Call { id: 1, call: Call::Ping },
            ToWorker::Call {
                id: 2,
                call: Call::FsWrite {
                    sandbox_id: "sb_x".into(),
                    path: "/a".into(),
                    data: vec![0, 255, 10],
                    mode: 0o644,
                    append: false,
                    exclusive: false,
                    create_parents: true,
                },
            },
            ToWorker::Cancel { id: 2 },
            ToWorker::Shutdown,
        ];
        let mut buf = Vec::new();
        for m in &msgs {
            write_frame(&mut buf, m).unwrap();
        }
        let mut r = buf.as_slice();
        let mut back = Vec::new();
        while let Some(m) = read_frame::<ToWorker>(&mut r).unwrap() {
            back.push(m);
        }
        assert_eq!(back, msgs);
        let reply = FromWorker::Reply { id: 9, result: Err(RpcError::busy("x")) };
        let mut buf = Vec::new();
        write_frame(&mut buf, &reply).unwrap();
        assert_eq!(read_frame::<FromWorker>(&mut buf.as_slice()).unwrap(), Some(reply));
    }

    #[test]
    fn oversized_and_truncated_frames() {
        let mut bad = (u32::MAX).to_be_bytes().to_vec();
        bad.extend_from_slice(b"{}");
        assert!(read_frame::<ToWorker>(&mut bad.as_slice()).is_err());
        let mut buf = Vec::new();
        write_frame(&mut buf, &ToWorker::Shutdown).unwrap();
        buf.pop();
        assert!(read_frame::<ToWorker>(&mut buf.as_slice()).is_err());
        assert_eq!(read_frame::<ToWorker>(&mut &[][..]).unwrap(), None);
    }
}
