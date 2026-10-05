//! Sessões: um `bash` persistente por sessão, onde `cd`, `export`, variáveis, funções e `alias`
//! sobrevivem entre comandos.
//!
//! O shell da sessão roda este laço (stdin dele é um pipe do host; stdout e stderr, dois pipes do host
//! que ficam abertos a sessão inteira):
//!
//! ```sh
//! __osh_dir='/run/osh/<sessão>'
//! while IFS= read -r __osh_id; do
//!   . "$__osh_dir/$__osh_id.sh" < "$__osh_dir/$__osh_id.in"
//!   __osh_rc=$?
//!   { printf '%s\0' "$PWD"; env -0; } > "$__osh_dir/$__osh_id.state" 2>/dev/null
//!   printf '\036OSH-END %s %d\036\n' "$__osh_id" "$__osh_rc"
//!   printf '\036OSH-END %s\036\n' "$__osh_id" >&2
//! done
//! ```
//!
//! Por comando, o host grava o script e o stdin em arquivos da sandbox, manda o id numa linha e lê os
//! dois fluxos até achar as sentinelas (o id é aleatório por comando). O `.` roda no próprio shell, então
//! o estado fica; o stdin do comando vem do arquivo, então um `cat` não engole o próximo comando; e como
//! o host só manda um id depois do anterior terminar, o `read` pode ler o pipe em blocos sem problema.
//!
//! Depois de cada comando o host guarda o cwd e o ambiente exportado (`env -0`). Se um comando estoura
//! o timeout, o host mata a sessão inteira (shell e filhos) e sobe um shell novo com esse estado
//! (`session_reset`): variáveis não exportadas, funções e `alias` se perdem, cwd e `export` ficam. Se o
//! shell sai (`exit`), a sessão acaba (`session_closed`).
//!
//! Limitações conhecidas: um comando que redireciona o stdout do próprio shell de vez (`exec >arquivo`)
//! esconde a sentinela e a sessão só volta pelo timeout; saída de processo em segundo plano entre dois
//! comandos é descartada; o laço deixa as variáveis `__osh_dir`, `__osh_id` e `__osh_rc` visíveis.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, bounded};
use parking_lot::Mutex;
use sysabi::{Errno, Pid, Signal, WaitStatus};

use crate::backend::{BResult, BackendError, ExitInfo, HostWriter, Sandbox, SpawnRequest, WriteOpts, WriteOutcome, resolve_program};
use crate::exec::{Captured, Cancel, Event, ExecLimits, ExecOutcome, KILL_GRACE, OutputSink, POLL, Stream, kill_session, reader_thread, waiter_thread};
use crate::fsops;

/// Diretório dos arquivos de controle das sessões dentro da sandbox.
pub const SESSION_ROOT: &str = "/run/osh";

/// Pedaços de saída em fila entre os leitores e o comando (cada um até 64 KiB). Com a fila cheia o
/// leitor para, o pipe enche e o processo que escreve espera: saída de segundo plano entre comandos não
/// cresce sem limite na memória do worker.
const QUEUE: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("a sessão já está executando um comando")]
    Busy,
    #[error("{0}")]
    Closed(String),
    #[error(transparent)]
    Backend(#[from] BackendError),
}

/// Estado que sobrevive à troca de shell: cwd, ambiente exportado e o restante do estado do bash.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ShellState {
    pub cwd: Vec<u8>,
    pub env: Vec<Vec<u8>>,
    /// Script que recria variáveis não exportadas, arrays, funções, aliases, `shopt` e `set -o`
    /// (a saída de `declare -p`, `declare -f`, `alias`, `shopt -p` e `set +o`). Vazio quando ainda
    /// não houve comando. Reaplicado com `source`, com erros ignorados (variáveis somente leitura).
    pub dump: Vec<u8>,
}

/// Teto do dump do shell: acima disso ele é descartado em vez de truncado (truncar corromperia o script).
const DUMP_LIMIT: usize = 4 << 20;

struct Shell {
    pid: Pid,
    stdin: Box<dyn HostWriter>,
    rx: Receiver<Event>,
    stop: Arc<AtomicBool>,
    handles: Vec<JoinHandle<()>>,
}

impl Shell {
    fn start(sb: &dyn Sandbox, dir: &str, state: &ShellState, fallback_cwd: &[u8]) -> BResult<Shell> {
        let path_var = state.env.iter().find_map(|e| e.strip_prefix(b"PATH=")).map(<[u8]>::to_vec);
        let bash = resolve_program(sb, b"bash", path_var.as_deref(), b"/")?;
        // O dump do shell anterior vai pra um arquivo que o shell novo lê antes do laço.
        let restore = format!("{dir}/restore.sh");
        let w = WriteOpts { append: false, exclusive: false, mode: 0o600 };
        sb.write_file(restore.as_bytes(), &state.dump, w)?;
        let script = format!(
            "__osh_dir='{dir}'\n\
             . \"$__osh_dir/restore.sh\" </dev/null 2>/dev/null\n\
             __osh_dir='{dir}'\n\
             while IFS= read -r __osh_id; do\n\
             . \"$__osh_dir/$__osh_id.sh\" < \"$__osh_dir/$__osh_id.in\"\n\
             __osh_rc=$?\n\
             {{ printf '%s\\0' \"$PWD\"; env -0; }} > \"$__osh_dir/$__osh_id.state\" 2>/dev/null\n\
             {{ declare -p; declare -f; alias; shopt -p; set +o; }} > \"$__osh_dir/$__osh_id.dump\" 2>/dev/null\n\
             printf '\\036OSH-END %s %d\\036\\n' \"$__osh_id\" \"$__osh_rc\"\n\
             printf '\\036OSH-END %s\\036\\n' \"$__osh_id\" >&2\n\
             done\n"
        );
        let cwd = match sb.stat(&state.cwd, true) {
            Ok(st) if st.file_type() == sysabi::FileType::Directory => state.cwd.clone(),
            _ => fallback_cwd.to_vec(),
        };
        let spawned = sb.spawn(SpawnRequest {
            path: bash,
            argv: vec![b"bash".to_vec(), b"-c".to_vec(), script.into_bytes()],
            env: state.env.clone(),
            cwd,
        })?;
        let (tx, rx) = bounded(QUEUE);
        let stop = Arc::new(AtomicBool::new(false));
        let pid = spawned.pid;
        let mut handles = Vec::with_capacity(3);
        let spawn = |name: &str, f: Box<dyn FnOnce() + Send>| {
            thread::Builder::new()
                .name(format!("sess-{name}-{pid}"))
                .spawn(f)
                .map_err(|e| BackendError::Internal(format!("thread da sessão: {e}")))
        };
        {
            let (r, t, s) = (spawned.stdout, tx.clone(), stop.clone());
            handles.push(spawn("out", Box::new(move || reader_thread(Stream::Stdout, r, t, s)))?);
        }
        {
            let (r, t, s) = (spawned.stderr, tx.clone(), stop.clone());
            handles.push(spawn("err", Box::new(move || reader_thread(Stream::Stderr, r, t, s)))?);
        }
        {
            let (w, t, s) = (spawned.exit, tx, stop.clone());
            handles.push(spawn("wait", Box::new(move || waiter_thread(w, t, s)))?);
        }
        Ok(Shell { pid, stdin: spawned.stdin, rx, stop, handles })
    }

    /// Mata a sessão inteira e recolhe as threads.
    fn kill(mut self, sb: &dyn Sandbox) -> Option<ExitInfo> {
        kill_session(sb, self.pid);
        let deadline = Instant::now() + KILL_GRACE;
        let mut exit = None;
        while exit.is_none() && Instant::now() < deadline {
            match self.rx.recv_deadline(Instant::now() + POLL) {
                Ok(Event::Exited(i)) => exit = Some(i),
                Ok(_) => {}
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }
        self.shutdown();
        exit
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Esvazia a fila pra destravar leitores que esperam espaço.
        while self.rx.try_recv().is_ok() {}
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }

    fn send_line(&mut self, line: &[u8], timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut off = 0;
        while off < line.len() {
            if Instant::now() >= deadline {
                return false;
            }
            match self.stdin.write_timeout(&line[off..], POLL) {
                WriteOutcome::Wrote(n) => off += n,
                WriteOutcome::Closed => return false,
                WriteOutcome::TimedOut => {}
            }
        }
        true
    }
}

/// Acha a sentinela num fluxo, guardando o que pode ser começo dela entre leituras.
struct Scanner {
    marker: Vec<u8>,
    pending: Vec<u8>,
    payload: Option<Vec<u8>>,
}

impl Scanner {
    fn new(id: &str) -> Scanner {
        Scanner { marker: format!("\x1eOSH-END {id}").into_bytes(), pending: Vec::new(), payload: None }
    }

    fn found(&self) -> bool {
        self.payload.is_some()
    }

    /// Acrescenta bytes; devolve o que já é saída do comando com certeza.
    fn push(&mut self, data: &[u8]) -> Vec<u8> {
        if self.found() {
            return Vec::new();
        }
        self.pending.extend_from_slice(data);
        if let Some(i) = find(&self.pending, &self.marker) {
            let after = i + self.marker.len();
            if let Some(j) = find(&self.pending[after..], b"\x1e\n") {
                self.payload = Some(self.pending[after..after + j].to_vec());
                let out = self.pending[..i].to_vec();
                self.pending.clear();
                return out;
            }
            let out = self.pending[..i].to_vec();
            self.pending.drain(..i);
            return out;
        }
        // Segura o maior sufixo que é prefixo da sentinela.
        let max = self.marker.len().saturating_sub(1).min(self.pending.len());
        let hold = (1..=max).rev().find(|&k| self.pending.ends_with(&self.marker[..k])).unwrap_or(0);
        let cut = self.pending.len() - hold;
        self.pending.drain(..cut).collect()
    }

    /// O que sobrou sem sentinela (o shell morreu no meio).
    fn flush(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn keep(cap: &mut Captured, data: &[u8], limit: u64) -> usize {
    let before = cap.total;
    cap.total += data.len() as u64;
    let room = limit.saturating_sub(before);
    let k = (data.len() as u64).min(room) as usize;
    if k < data.len() {
        cap.truncated = true;
    }
    cap.data.extend_from_slice(&data[..k]);
    k
}

/// Desfecho de um comando de sessão.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionOutcome {
    pub exec: ExecOutcome,
    pub cwd: Option<String>,
    pub reset: bool,
    pub closed: bool,
    /// O estado do shell depois do comando, pro supervisor recriar a sessão se o worker cair.
    #[serde(default)]
    pub snapshot: Option<SessionSnapshot>,
}

/// O [`ShellState`] no formato que atravessa o IPC (texto no cwd e no ambiente, base64 no dump).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionSnapshot {
    pub cwd: String,
    pub env: Vec<String>,
    #[serde(with = "crate::api::b64")]
    pub dump: Vec<u8>,
}

impl ShellState {
    pub fn to_snapshot(&self) -> SessionSnapshot {
        SessionSnapshot {
            cwd: String::from_utf8_lossy(&self.cwd).into_owned(),
            env: self.env.iter().map(|e| String::from_utf8_lossy(e).into_owned()).collect(),
            dump: self.dump.clone(),
        }
    }
}

struct Inner {
    shell: Option<Shell>,
    state: ShellState,
    closed: Option<String>,
}

/// Uma sessão.
pub struct Session {
    sb: Arc<dyn Sandbox>,
    pub id: String,
    dir: String,
    fallback_cwd: Vec<u8>,
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("id", &self.id).finish_non_exhaustive()
    }
}

impl Session {
    pub fn open(sb: Arc<dyn Sandbox>, id: &str, state: ShellState, fallback_cwd: &[u8]) -> BResult<Session> {
        let dir = format!("{SESSION_ROOT}/{id}");
        fsops::mkdir_p(&*sb, SESSION_ROOT.as_bytes(), 0o755)?;
        match sb.mkdir(dir.as_bytes(), 0o700) {
            Ok(()) => {}
            Err(BackendError::Os { errno, .. }) if errno == Errno::EEXIST => {}
            Err(e) => return Err(e),
        }
        let shell = Shell::start(&*sb, &dir, &state, fallback_cwd)?;
        Ok(Session {
            sb,
            id: id.to_string(),
            dir,
            fallback_cwd: fallback_cwd.to_vec(),
            inner: Mutex::new(Inner { shell: Some(shell), state, closed: None }),
        })
    }

    pub fn is_closed(&self) -> Option<String> {
        self.inner.try_lock().and_then(|i| i.closed.clone())
    }

    /// Roda um comando. Um por vez: o segundo concorrente leva [`SessionError::Busy`].
    pub fn exec(
        &self,
        command: &str,
        stdin: &[u8],
        limits: ExecLimits,
        sink: Option<&dyn OutputSink>,
        cancel: &Cancel,
    ) -> Result<SessionOutcome, SessionError> {
        let mut inner = self.inner.try_lock().ok_or(SessionError::Busy)?;
        if let Some(reason) = &inner.closed {
            return Err(SessionError::Closed(reason.clone()));
        }
        let start = Instant::now();
        let mut reset = false;
        // Shell que morreu entre dois comandos (um `kill -9 $$` em segundo plano, por exemplo): sobe outro.
        let dead = match &inner.shell {
            None => true,
            Some(sh) => {
                let mut dead = false;
                while let Ok(ev) = sh.rx.try_recv() {
                    if matches!(ev, Event::Exited(_)) {
                        dead = true;
                    }
                }
                dead
            }
        };
        if dead {
            if let Some(mut sh) = inner.shell.take() {
                sh.shutdown();
            }
            inner.shell = Some(Shell::start(&*self.sb, &self.dir, &inner.state, &self.fallback_cwd)?);
            reset = true;
        }

        let cmd_id = crate::ids::random_id("c");
        let base = format!("{}/{cmd_id}", self.dir);
        let w = WriteOpts { append: false, exclusive: false, mode: 0o600 };
        let mut script = command.as_bytes().to_vec();
        script.push(b'\n');
        self.sb.write_file(format!("{base}.sh").as_bytes(), &script, w)?;
        self.sb.write_file(format!("{base}.in").as_bytes(), stdin, w)?;
        let cleanup = |sb: &dyn Sandbox| {
            for ext in ["sh", "in", "state", "dump"] {
                let _ = sb.unlink(format!("{base}.{ext}").as_bytes());
            }
        };

        let mut line = cmd_id.clone().into_bytes();
        line.push(b'\n');
        let sent = inner.shell.as_mut().expect("shell presente").send_line(&line, limits.timeout);

        let mut out_scan = Scanner::new(&cmd_id);
        let mut err_scan = Scanner::new(&cmd_id);
        let mut out = Captured::default();
        let mut err = Captured::default();
        let deadline = start + limits.timeout;
        let mut exited: Option<ExitInfo> = None;
        let mut drain_until: Option<Instant> = None;
        let mut timed_out = false;
        let mut cancelled = false;
        let mut overflow = false;
        let mut eof = (false, false);

        let emit = |stream: Stream, data: &[u8], out: &mut Captured, err: &mut Captured| {
            if data.is_empty() {
                return;
            }
            let cap = if stream == Stream::Stdout { out } else { err };
            let k = keep(cap, data, limits.output_limit);
            if let (Some(s), true) = (sink, k > 0) {
                s.output(stream, &data[..k]);
            }
        };

        if sent {
            let rx = inner.shell.as_ref().expect("shell presente").rx.clone();
            loop {
                if out_scan.found() && err_scan.found() {
                    break;
                }
                let now = Instant::now();
                if exited.is_some() && (eof == (true, true) || drain_until.is_some_and(|u| now >= u)) {
                    break;
                }
                if exited.is_none() {
                    if cancel.is_cancelled() {
                        cancelled = true;
                        break;
                    }
                    if now >= deadline {
                        timed_out = true;
                        break;
                    }
                }
                let wake = (now + POLL).min(if exited.is_none() { deadline } else { drain_until.unwrap_or(now + POLL) });
                match rx.recv_deadline(wake) {
                    Ok(Event::Data(s, d)) => {
                        let shown = if s == Stream::Stdout { out_scan.push(&d) } else { err_scan.push(&d) };
                        emit(s, &shown, &mut out, &mut err);
                        if out.total.max(err.total) > limits.output_limit.saturating_add(limits.max_discard) {
                            overflow = true;
                            break;
                        }
                    }
                    Ok(Event::Eof(s)) => {
                        if s == Stream::Stdout {
                            eof.0 = true;
                        } else {
                            eof.1 = true;
                        }
                    }
                    Ok(Event::Exited(info)) => {
                        exited = Some(info);
                        drain_until = Some(Instant::now() + limits.drain_grace);
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
                }
            }
        }

        // O shell saiu durante o comando (`exit N`, ou morreu): a sessão acaba.
        if !sent || (exited.is_some() && !(out_scan.found() && err_scan.found())) {
            let tail_out = out_scan.flush();
            let tail_err = err_scan.flush();
            emit(Stream::Stdout, &tail_out, &mut out, &mut err);
            emit(Stream::Stderr, &tail_err, &mut out, &mut err);
            let info = match inner.shell.take() {
                Some(sh) => exited.or_else(|| sh.kill(&*self.sb)),
                None => exited,
            };
            let (exit_code, signal) = split_status(info.map(|i| i.status));
            inner.closed = Some(match (exit_code, signal) {
                (Some(c), _) => format!("a sessão terminou: o shell saiu com {c}"),
                (None, Some(s)) => format!("a sessão terminou: o shell morreu com o sinal {}", sig_name(s)),
                _ => "a sessão terminou".to_string(),
            });
            cleanup(&*self.sb);
            return Ok(SessionOutcome {
                exec: ExecOutcome {
                    exit_code,
                    signal,
                    timed_out: false,
                    cancelled: false,
                    duration_ms: start.elapsed().as_millis() as u64,
                    cpu_ns: info.map_or(0, |i| i.cpu_ns),
                    stdout: out,
                    stderr: err,
                    background_detached: false,
                },
                cwd: None,
                reset,
                closed: true,
                snapshot: None,
            });
        }

        if timed_out || cancelled || overflow {
            // Mata shell e filhos e sobe um shell novo com o último estado conhecido.
            if let Some(sh) = inner.shell.take() {
                sh.kill(&*self.sb);
            }
            out.closed = overflow;
            cleanup(&*self.sb);
            match Shell::start(&*self.sb, &self.dir, &inner.state, &self.fallback_cwd) {
                Ok(sh) => inner.shell = Some(sh),
                Err(e) => inner.closed = Some(format!("a sessão terminou: o shell não subiu de novo: {e}")),
            }
            return Ok(SessionOutcome {
                exec: ExecOutcome {
                    exit_code: None,
                    signal: Some(Signal::SIGKILL.0),
                    timed_out,
                    cancelled,
                    duration_ms: start.elapsed().as_millis() as u64,
                    cpu_ns: 0,
                    stdout: out,
                    stderr: err,
                    background_detached: false,
                },
                cwd: Some(String::from_utf8_lossy(&inner.state.cwd).into_owned()),
                reset: true,
                closed: inner.closed.is_some(),
                snapshot: Some(inner.state.to_snapshot()),
            });
        }

        let rc: i32 = out_scan
            .payload
            .as_deref()
            .and_then(|p| std::str::from_utf8(p).ok())
            .and_then(|p| p.trim().parse().ok())
            .unwrap_or(0);
        if let Ok(state) = self.sb.read_file(format!("{base}.state").as_bytes(), 0, 4 << 20)
            && let Some(s) = parse_state(&state)
        {
            inner.state = s;
            // Sem dump novo (ou grande demais) o estado fica com o dump do comando anterior.
            if let Ok(dump) = self.sb.read_file(format!("{base}.dump").as_bytes(), 0, DUMP_LIMIT + 1)
                && dump.len() <= DUMP_LIMIT
            {
                inner.state.dump = dump;
            }
        }
        cleanup(&*self.sb);
        Ok(SessionOutcome {
            exec: ExecOutcome {
                exit_code: Some(rc),
                signal: None,
                timed_out: false,
                cancelled: false,
                duration_ms: start.elapsed().as_millis() as u64,
                cpu_ns: 0,
                stdout: out,
                stderr: err,
                background_detached: false,
            },
            cwd: Some(String::from_utf8_lossy(&inner.state.cwd).into_owned()),
            reset,
            closed: false,
            snapshot: Some(inner.state.to_snapshot()),
        })
    }

    /// Fecha: mata o shell e o que ele deixou rodando, e apaga os arquivos de controle.
    pub fn close(&self) {
        let mut inner = self.inner.lock();
        if let Some(sh) = inner.shell.take() {
            sh.kill(&*self.sb);
        }
        if inner.closed.is_none() {
            inner.closed = Some("a sessão foi fechada".into());
        }
        let _ = fsops::remove(&*self.sb, self.dir.as_bytes(), true, true);
    }
}

fn split_status(s: Option<WaitStatus>) -> (Option<i32>, Option<i32>) {
    match s {
        Some(WaitStatus::Exited(c)) => (Some(c), None),
        Some(WaitStatus::Signaled { signal, .. }) => (None, Some(signal.0)),
        Some(WaitStatus::Stopped(s)) => (None, Some(s.0)),
        Some(WaitStatus::Continued) => (Some(0), None),
        None => (None, None),
    }
}

fn sig_name(s: i32) -> String {
    Signal(s).name().unwrap_or_else(|| s.to_string())
}

/// `cwd\0VAR=valor\0...` (sem a `_` que o bash põe no ambiente do `env`).
fn parse_state(raw: &[u8]) -> Option<ShellState> {
    let mut parts = raw.split(|b| *b == 0);
    let cwd = parts.next()?.to_vec();
    if !cwd.starts_with(b"/") {
        return None;
    }
    let env = parts.filter(|e| !e.is_empty() && e.contains(&b'=') && !e.starts_with(b"_=")).map(<[u8]>::to_vec).collect();
    Some(ShellState { cwd, env, dump: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::test_sandbox;

    fn limits() -> ExecLimits {
        ExecLimits {
            timeout: Duration::from_secs(10),
            output_limit: 1 << 20,
            max_discard: 1 << 22,
            drain_grace: Duration::from_millis(100),
        }
    }

    fn state() -> ShellState {
        ShellState { cwd: b"/root".to_vec(), env: vec![b"PATH=/bin".to_vec(), b"HOME=/root".to_vec()], dump: Vec::new() }
    }

    #[test]
    fn scanner_splits_sentinel_across_reads() {
        let mut s = Scanner::new("c_1");
        let full = b"hello\x1eOSH-END c_1 7\x1e\nbg";
        let mut out = Vec::new();
        for b in full.chunks(1) {
            out.extend(s.push(b));
        }
        assert_eq!(out, b"hello");
        assert_eq!(s.payload.as_deref(), Some(&b" 7"[..]));
        let mut s = Scanner::new("c_1");
        assert_eq!(s.push(b"a\x1eb"), b"a\x1eb");
        assert_eq!(s.push(b"\x1eOSH-E"), b"");
        assert_eq!(s.push(b"X"), b"\x1eOSH-EX");
    }

    #[test]
    fn cd_and_export_survive() {
        let sb = test_sandbox();
        let s = Session::open(sb.clone(), "ss_t1", state(), b"/root").unwrap();
        let o = s.exec("cd /work\nexport FOO=bar", b"", limits(), None, &Cancel::new()).unwrap();
        assert_eq!(o.exec.exit_code, Some(0));
        assert_eq!(o.cwd.as_deref(), Some("/work"));
        let o = s.exec("pwd; printenv FOO", b"", limits(), None, &Cancel::new()).unwrap();
        assert_eq!(o.exec.stdout.data, b"/work\nbar\n");
        let o = s.exec("cat", b"do stdin\n", limits(), None, &Cancel::new()).unwrap();
        assert_eq!(o.exec.stdout.data, b"do stdin\n");
        let o = s.exec("errout ops; exit 0", b"", limits(), None, &Cancel::new()).unwrap();
        assert!(o.closed);
        assert_eq!(o.exec.stderr.data, b"ops\n");
        assert!(matches!(s.exec("pwd", b"", limits(), None, &Cancel::new()), Err(SessionError::Closed(_))));
    }

    #[test]
    fn status_and_output_limit() {
        let sb = test_sandbox();
        let s = Session::open(sb, "ss_t2", state(), b"/root").unwrap();
        let o = s.exec("false", b"", limits(), None, &Cancel::new()).unwrap();
        assert_eq!(o.exec.exit_code, Some(1));
        let mut l = limits();
        l.output_limit = 10;
        let o = s.exec("bigout 100000", b"", l, None, &Cancel::new()).unwrap();
        assert_eq!(o.exec.stdout.data.len(), 10);
        assert!(o.exec.stdout.truncated);
        assert_eq!(o.exec.stdout.total, 100_000);
        let o = s.exec("echo depois", b"", limits(), None, &Cancel::new()).unwrap();
        assert_eq!(o.exec.stdout.data, b"depois\n");
    }

    #[test]
    fn timeout_resets_with_exported_state() {
        let sb = test_sandbox();
        let s = Session::open(sb.clone(), "ss_t3", state(), b"/root").unwrap();
        s.exec("cd /tmp; export KEEP=1", b"", limits(), None, &Cancel::new()).unwrap();
        let mut l = limits();
        l.timeout = Duration::from_millis(300);
        let o = s.exec("spin", b"", l, None, &Cancel::new()).unwrap();
        assert!(o.exec.timed_out && o.reset && !o.closed);
        let o = s.exec("pwd; printenv KEEP", b"", limits(), None, &Cancel::new()).unwrap();
        assert_eq!(o.exec.stdout.data, b"/tmp\n1\n");
        s.close();
        let alive = sb.processes().into_iter().filter(|p| p.state != 'Z').count();
        assert_eq!(alive, 0);
        assert!(sb.stat(b"/run/osh/ss_t3", false).is_err());
    }

    #[test]
    fn concurrent_exec_is_busy() {
        let sb = test_sandbox();
        let s = Arc::new(Session::open(sb, "ss_t4", state(), b"/root").unwrap());
        let s2 = s.clone();
        let h = thread::spawn(move || s2.exec("sleep 0.5", b"", limits(), None, &Cancel::new()).unwrap());
        thread::sleep(Duration::from_millis(100));
        assert!(matches!(s.exec("pwd", b"", limits(), None, &Cancel::new()), Err(SessionError::Busy)));
        assert_eq!(h.join().unwrap().exec.exit_code, Some(0));
    }
}
