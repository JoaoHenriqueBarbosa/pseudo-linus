//! `exec`: roda um processo numa sandbox com timeout de parede, limite de saída e cancelamento.
//!
//! Por execução, quatro threads do host (nenhuma é pseudo-processo e nenhuma é de pool): uma escreve o
//! stdin e fecha, duas leem stdout e stderr, uma espera o término. Todas mandam eventos pra um canal e
//! a thread que chamou coordena:
//!
//! - **limite de saída**: guarda até `output_limit` bytes por fluxo e marca `*_truncated`; continua
//!   lendo e descartando (o processo não trava num pipe cheio) até `max_discard`, e aí fecha a ponta de
//!   leitura, como `| head -c`: o escritor leva SIGPIPE;
//! - **timeout de parede**: SIGKILL no grupo de processos do comando (que nasce em sessão própria) e,
//!   pra quem trocou de grupo, em todo processo da mesma sessão;
//! - **escoamento**: quando o processo principal termina, espera a saída fechar por `drain_grace`;
//!   se um processo em segundo plano ainda segura o pipe, o host para de ler e marca
//!   `background_detached`;
//! - **cancelamento**: o cliente desconectou ou o daemon está desligando; mesmo tratamento do
//!   timeout, com `cancelled`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use sysabi::{KillTarget, Pid, Signal, WaitStatus};

use crate::backend::{BResult, BackendError, ExitInfo, HostReader, HostWriter, ReadOutcome, Sandbox, SpawnRequest, WriteOutcome};

/// Fatia de espera das threads auxiliares: o tempo máximo até elas perceberem que devem parar.
pub(crate) const POLL: Duration = Duration::from_millis(25);
/// Quanto esperar o processo morrer depois do SIGKILL antes de desistir dele.
pub(crate) const KILL_GRACE: Duration = Duration::from_secs(5);

/// Qual fluxo de saída.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stream {
    Stdout,
    Stderr,
}

/// Bandeira de cancelamento compartilhada entre quem pediu e quem executa.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Cancel {
        Cancel::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Limites de uma execução.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecLimits {
    pub timeout: Duration,
    pub output_limit: u64,
    pub max_discard: u64,
    pub drain_grace: Duration,
}

/// Recebe a saída enquanto ela chega (streaming). Só recebe o que cabe no limite de saída.
pub trait OutputSink: Send + Sync {
    fn output(&self, stream: Stream, data: &[u8]);
}

/// Saída de um fluxo.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Captured {
    #[serde(with = "crate::api::b64")]
    pub data: Vec<u8>,
    /// Total produzido (inclui o descartado).
    pub total: u64,
    pub truncated: bool,
    /// O host fechou a ponta de leitura por excesso.
    pub closed: bool,
}

/// Desfecho de uma execução, em bytes (o supervisor converte pro formato da resposta).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExecOutcome {
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub cancelled: bool,
    pub duration_ms: u64,
    pub cpu_ns: u64,
    pub stdout: Captured,
    pub stderr: Captured,
    pub background_detached: bool,
}

impl ExecOutcome {
    /// O `$?` do bash.
    pub fn status(&self) -> i32 {
        match (self.exit_code, self.signal) {
            (Some(c), _) => c & 0xff,
            (None, Some(s)) => 128 + s,
            (None, None) => 128 + Signal::SIGKILL.0,
        }
    }
}

pub(crate) enum Event {
    Data(Stream, Vec<u8>),
    Eof(Stream),
    Exited(ExitInfo),
}

/// Manda um evento; com canal cheio, espera em fatias e desiste se mandarem parar ou se o receptor
/// sumiu. Devolve falso quando a thread deve encerrar.
pub(crate) fn send_or_stop(tx: &Sender<Event>, mut ev: Event, stop: &AtomicBool) -> bool {
    loop {
        match tx.send_timeout(ev, POLL) {
            Ok(()) => return true,
            Err(crossbeam_channel::SendTimeoutError::Disconnected(_)) => return false,
            Err(crossbeam_channel::SendTimeoutError::Timeout(back)) => {
                if stop.load(Ordering::Acquire) {
                    return false;
                }
                ev = back;
            }
        }
    }
}

/// Lê um fluxo até EOF ou até mandarem parar. Soltar o leitor fecha a ponta do host.
pub(crate) fn reader_thread(stream: Stream, mut r: Box<dyn HostReader>, tx: Sender<Event>, stop: Arc<AtomicBool>) {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        match r.read_timeout(&mut buf, POLL) {
            ReadOutcome::Data(n) => {
                if !send_or_stop(&tx, Event::Data(stream, buf[..n].to_vec()), &stop) {
                    return;
                }
            }
            ReadOutcome::Eof => {
                send_or_stop(&tx, Event::Eof(stream), &stop);
                return;
            }
            ReadOutcome::TimedOut => {}
        }
    }
}

/// Espera o término e avisa no canal.
pub(crate) fn waiter_thread(mut waiter: Box<dyn crate::backend::ExitWaiter>, tx: Sender<Event>, stop: Arc<AtomicBool>) {
    loop {
        if let Some(info) = waiter.wait_timeout(POLL) {
            send_or_stop(&tx, Event::Exited(info), &stop);
            return;
        }
        if stop.load(Ordering::Acquire) {
            return;
        }
    }
}

/// Escreve o stdin inteiro e fecha. Para se o processo fechar o stdin ou se mandarem parar.
fn writer_thread(mut w: Box<dyn HostWriter>, data: Vec<u8>, stop: Arc<AtomicBool>) {
    let mut off = 0;
    while off < data.len() {
        if stop.load(Ordering::Acquire) {
            return;
        }
        match w.write_timeout(&data[off..], POLL) {
            WriteOutcome::Wrote(n) => off += n,
            WriteOutcome::Closed => return,
            WriteOutcome::TimedOut => {}
        }
    }
}

/// O stdin de um exec: tudo de uma vez, ou um fluxo lido sob demanda (o stdin do `osh`, que pode
/// ser um pipe que nunca fecha: o comando roda sem esperar EOF, como o `bash -c`).
pub enum StdinFeed {
    Bytes(Vec<u8>),
    Stream(Box<dyn std::io::Read + Send>),
}

/// Bombeia um fluxo para o processo à medida que chega. A leitura do fluxo fica numa thread solta:
/// ela pode estar bloqueada num `read` do host quando o processo termina, e ninguém espera por ela.
fn stream_writer_thread(mut w: Box<dyn HostWriter>, mut src: Box<dyn std::io::Read + Send>, stop: Arc<AtomicBool>) {
    let (tx, rx) = crossbeam_channel::bounded::<Vec<u8>>(4);
    let reader = thread::Builder::new().name("exec-stdin-src".into()).spawn(move || {
        let mut buf = vec![0u8; 64 << 10];
        loop {
            match src.read(&mut buf) {
                Ok(0) => return,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        return;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return,
            }
        }
    });
    if reader.is_err() {
        return;
    }
    loop {
        if stop.load(Ordering::Acquire) {
            return;
        }
        let chunk = match rx.recv_timeout(POLL) {
            Ok(c) => c,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            // EOF do fluxo: soltar a ponta de escrita fecha o stdin do processo.
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
        };
        let mut off = 0;
        while off < chunk.len() {
            if stop.load(Ordering::Acquire) {
                return;
            }
            match w.write_timeout(&chunk[off..], POLL) {
                WriteOutcome::Wrote(n) => off += n,
                WriteOutcome::Closed => return,
                WriteOutcome::TimedOut => {}
            }
        }
    }
}

struct StreamState {
    cap: Captured,
    open: bool,
    stop: Arc<AtomicBool>,
}

impl StreamState {
    fn new() -> StreamState {
        StreamState { cap: Captured::default(), open: true, stop: Arc::new(AtomicBool::new(false)) }
    }

    /// Acumula `data` respeitando os limites; devolve o pedaço que cabe (pra streaming).
    fn push<'a>(&mut self, data: &'a [u8], limits: &ExecLimits) -> &'a [u8] {
        let before = self.cap.total;
        self.cap.total += data.len() as u64;
        let room = limits.output_limit.saturating_sub(before);
        let keep = (data.len() as u64).min(room) as usize;
        if keep < data.len() {
            self.cap.truncated = true;
        }
        self.cap.data.extend_from_slice(&data[..keep]);
        if self.cap.total > limits.output_limit.saturating_add(limits.max_discard) && self.open {
            // Passou do teto de descarte: fecha a ponta de leitura.
            self.cap.closed = true;
            self.open = false;
            self.stop.store(true, Ordering::Release);
        }
        &data[..keep]
    }
}

/// Mata o grupo do comando e quem ficou na sessão dele.
pub fn kill_session(sb: &dyn Sandbox, pid: Pid) {
    let _ = sb.kill(KillTarget::Group(pid), Signal::SIGKILL);
    let _ = sb.kill(KillTarget::Pid(pid), Signal::SIGKILL);
    for p in sb.processes() {
        if (p.sid == pid || p.pgid == pid) && p.state != 'Z' {
            let _ = sb.kill(KillTarget::Pid(p.pid), Signal::SIGKILL);
        }
    }
}

/// Executa e espera.
pub fn run(
    sb: &dyn Sandbox,
    req: SpawnRequest,
    stdin: Vec<u8>,
    limits: ExecLimits,
    sink: Option<&dyn OutputSink>,
    cancel: &Cancel,
) -> BResult<ExecOutcome> {
    run_feed(sb, req, StdinFeed::Bytes(stdin), limits, sink, cancel)
}

/// Como `run`, com o stdin dado como `StdinFeed`.
pub fn run_feed(
    sb: &dyn Sandbox,
    req: SpawnRequest,
    stdin: StdinFeed,
    limits: ExecLimits,
    sink: Option<&dyn OutputSink>,
    cancel: &Cancel,
) -> BResult<ExecOutcome> {
    let start = Instant::now();
    let spawned = sb.spawn(req)?;
    let pid = spawned.pid;
    let (tx, rx): (Sender<Event>, Receiver<Event>) = unbounded();
    let mut out = StreamState::new();
    let mut err = StreamState::new();
    let stdin_stop = Arc::new(AtomicBool::new(false));
    let wait_stop = Arc::new(AtomicBool::new(false));

    let mut handles = Vec::with_capacity(4);
    let spawn_aux = |name: &str, f: Box<dyn FnOnce() + Send>| {
        thread::Builder::new()
            .name(format!("exec-{name}-{pid}"))
            .spawn(f)
            .map_err(|e| BackendError::Internal(format!("thread auxiliar do exec: {e}")))
    };
    {
        let (r, t, s) = (spawned.stdout, tx.clone(), out.stop.clone());
        handles.push(spawn_aux("out", Box::new(move || reader_thread(Stream::Stdout, r, t, s)))?);
    }
    {
        let (r, t, s) = (spawned.stderr, tx.clone(), err.stop.clone());
        handles.push(spawn_aux("err", Box::new(move || reader_thread(Stream::Stderr, r, t, s)))?);
    }
    {
        let (w, s) = (spawned.stdin, stdin_stop.clone());
        let f: Box<dyn FnOnce() + Send> = match stdin {
            StdinFeed::Bytes(data) => Box::new(move || writer_thread(w, data, s)),
            StdinFeed::Stream(src) => Box::new(move || stream_writer_thread(w, src, s)),
        };
        handles.push(spawn_aux("in", f)?);
    }
    {
        let (waiter, t, s) = (spawned.exit, tx.clone(), wait_stop.clone());
        handles.push(spawn_aux("wait", Box::new(move || waiter_thread(waiter, t, s)))?);
    }
    drop(tx);

    let deadline = start + limits.timeout;
    let mut exit: Option<ExitInfo> = None;
    let mut timed_out = false;
    let mut cancelled = false;
    let mut killed_at: Option<Instant> = None;
    let mut drain_until: Option<Instant> = None;
    let mut detached = false;

    loop {
        let all_closed = !out.open && !err.open;
        if exit.is_some() && all_closed {
            break;
        }
        let now = Instant::now();
        if let (Some(_), Some(until)) = (exit, drain_until)
            && now >= until
        {
            detached = out.open || err.open;
            break;
        }
        if exit.is_none() && killed_at.is_none() {
            if cancel.is_cancelled() {
                cancelled = true;
            } else if now >= deadline {
                timed_out = true;
            }
            if cancelled || timed_out {
                kill_session(sb, pid);
                killed_at = Some(now);
            }
        }
        if let Some(k) = killed_at
            && exit.is_none()
            && now >= k + KILL_GRACE
        {
            // O kernel não entregou o término (não deveria acontecer): desiste do processo.
            tracing::error!(pid, "processo não terminou depois do SIGKILL; abandonando a espera");
            detached = true;
            break;
        }
        let mut wake = now + POLL;
        if exit.is_none() && killed_at.is_none() {
            wake = wake.min(deadline);
        }
        if let Some(u) = drain_until {
            wake = wake.min(u);
        }
        let ev = match rx.recv_deadline(wake) {
            Ok(ev) => ev,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                // Todas as threads saíram: os dois fluxos fecharam e o término já veio (ou nunca vem).
                break;
            }
        };
        match ev {
            Event::Data(stream, data) => {
                let st = if stream == Stream::Stdout { &mut out } else { &mut err };
                let kept = st.push(&data, &limits);
                if let (Some(s), false) = (sink, kept.is_empty()) {
                    s.output(stream, kept);
                }
            }
            Event::Eof(stream) => {
                let st = if stream == Stream::Stdout { &mut out } else { &mut err };
                st.open = false;
            }
            Event::Exited(info) => {
                exit = Some(info);
                drain_until = Some(Instant::now() + limits.drain_grace);
                // Ninguém mais lê o stdin depois que o processo principal saiu (um filho em segundo
                // plano pode até ler, mas a entrada era do comando).
                stdin_stop.store(true, Ordering::Release);
            }
        }
    }
    out.stop.store(true, Ordering::Release);
    err.stop.store(true, Ordering::Release);
    stdin_stop.store(true, Ordering::Release);
    wait_stop.store(true, Ordering::Release);
    for h in handles {
        let _ = h.join();
    }

    let (exit_code, signal, cpu_ns) = match exit {
        Some(ExitInfo { status: WaitStatus::Exited(c), cpu_ns }) => (Some(c), None, cpu_ns),
        Some(ExitInfo { status: WaitStatus::Signaled { signal, .. }, cpu_ns }) => (None, Some(signal.0), cpu_ns),
        Some(ExitInfo { status: WaitStatus::Stopped(s), cpu_ns }) => (None, Some(s.0), cpu_ns),
        Some(ExitInfo { status: WaitStatus::Continued, cpu_ns }) => (Some(0), None, cpu_ns),
        None => (None, Some(Signal::SIGKILL.0), 0),
    };
    Ok(ExecOutcome {
        exit_code,
        signal,
        timed_out,
        cancelled,
        duration_ms: start.elapsed().as_millis() as u64,
        cpu_ns,
        stdout: out.cap,
        stderr: err.cap,
        background_detached: detached,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::test_sandbox;

    fn req(argv: &[&str]) -> SpawnRequest {
        SpawnRequest {
            path: format!("/bin/{}", argv[0]).into_bytes(),
            argv: argv.iter().map(|a| a.as_bytes().to_vec()).collect(),
            env: vec![b"PATH=/bin".to_vec(), b"HOME=/root".to_vec()],
            cwd: b"/root".to_vec(),
        }
    }

    fn limits() -> ExecLimits {
        ExecLimits {
            timeout: Duration::from_secs(10),
            output_limit: 1 << 20,
            max_discard: 1 << 22,
            drain_grace: Duration::from_millis(100),
        }
    }

    #[test]
    fn stdout_stderr_exit_and_stdin() {
        let sb = test_sandbox();
        let o = run(&*sb, req(&["echo", "oi", "mundo"]), vec![], limits(), None, &Cancel::new()).unwrap();
        assert_eq!(o.stdout.data, b"oi mundo\n");
        assert_eq!(o.exit_code, Some(0));
        assert_eq!(o.status(), 0);
        let o = run(&*sb, req(&["cat"]), b"entrada\n".to_vec(), limits(), None, &Cancel::new()).unwrap();
        assert_eq!(o.stdout.data, b"entrada\n");
        let o = run(&*sb, req(&["exit", "3"]), vec![], limits(), None, &Cancel::new()).unwrap();
        assert_eq!((o.exit_code, o.status()), (Some(3), 3));
        let o = run(&*sb, req(&["errout", "falhou"]), vec![], limits(), None, &Cancel::new()).unwrap();
        assert_eq!(o.stderr.data, b"falhou\n");
        assert!(o.stdout.data.is_empty());
        // stdin grande que o programa não lê: o escritor para quando o processo sai.
        let o = run(&*sb, req(&["true"]), vec![b'z'; 1 << 20], limits(), None, &Cancel::new()).unwrap();
        assert_eq!(o.exit_code, Some(0));
    }

    #[test]
    fn wall_timeout_kills_the_whole_group() {
        let sb = test_sandbox();
        let mut l = limits();
        l.timeout = Duration::from_millis(300);
        let t = Instant::now();
        let o = run(&*sb, req(&["spin"]), vec![], l, None, &Cancel::new()).unwrap();
        assert!(o.timed_out);
        assert_eq!(o.signal, Some(9));
        assert_eq!(o.status(), 137);
        assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
        // Um shell com filho em segundo plano: o timeout mata os dois.
        let mut r = req(&["sh", "-c", "sleep 30 &\nspin"]);
        r.path = b"/bin/sh".to_vec();
        let o = run(&*sb, r, vec![], l, None, &Cancel::new()).unwrap();
        assert!(o.timed_out);
        std::thread::sleep(Duration::from_millis(100));
        let alive: Vec<_> = sb.processes().into_iter().filter(|p| p.state != 'Z').collect();
        assert!(alive.is_empty(), "sobrou processo: {alive:?}");
    }

    #[test]
    fn output_limit_truncates_and_discard_limit_closes() {
        let sb = test_sandbox();
        let mut l = limits();
        l.output_limit = 1000;
        l.max_discard = 1 << 20;
        let o = run(&*sb, req(&["bigout", "500000"]), vec![], l, None, &Cancel::new()).unwrap();
        assert_eq!(o.stdout.data.len(), 1000);
        assert!(o.stdout.truncated);
        assert_eq!(o.stdout.total, 500_000);
        assert!(!o.stdout.closed);
        assert_eq!(o.exit_code, Some(0));
        // `yes` nunca acaba: passou do teto de descarte, o host fecha e ele leva SIGPIPE.
        let o = run(&*sb, req(&["yes"]), vec![], l, None, &Cancel::new()).unwrap();
        assert!(o.stdout.closed);
        assert_eq!(o.signal, Some(Signal::SIGPIPE.0));
        assert!(!o.timed_out);
    }

    #[test]
    fn background_holder_is_detached_after_grace() {
        let sb = test_sandbox();
        let t = Instant::now();
        let o = run(&*sb, req(&["bgsleep", "30"]), vec![], limits(), None, &Cancel::new()).unwrap();
        assert_eq!(o.exit_code, Some(0));
        assert!(o.background_detached);
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    }

    #[test]
    fn cancel_and_streaming() {
        struct Sink(parking_lot::Mutex<Vec<(Stream, Vec<u8>)>>);
        impl OutputSink for Sink {
            fn output(&self, s: Stream, d: &[u8]) {
                self.0.lock().push((s, d.to_vec()));
            }
        }
        let sb = test_sandbox();
        let sink = Sink(parking_lot::Mutex::new(Vec::new()));
        let o = run(&*sb, req(&["echo", "x"]), vec![], limits(), Some(&sink), &Cancel::new()).unwrap();
        assert_eq!(o.stdout.data, b"x\n");
        assert_eq!(sink.0.lock().concat_stream(Stream::Stdout), b"x\n");
        let cancel = Cancel::new();
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            c2.cancel();
        });
        let o = run(&*sb, req(&["sleep", "30"]), vec![], limits(), None, &cancel).unwrap();
        assert!(o.cancelled && !o.timed_out);
        assert_eq!(o.signal, Some(9));
    }

    trait Concat {
        fn concat_stream(&self, s: Stream) -> Vec<u8>;
    }
    impl Concat for Vec<(Stream, Vec<u8>)> {
        fn concat_stream(&self, s: Stream) -> Vec<u8> {
            self.iter().filter(|(t, _)| *t == s).flat_map(|(_, d)| d.clone()).collect()
        }
    }

    #[test]
    fn spawn_errors_surface() {
        let sb = test_sandbox();
        let mut r = req(&["nope"]);
        r.path = b"/bin/nope".to_vec();
        let e = run(&*sb, r, vec![], limits(), None, &Cancel::new()).unwrap_err();
        assert!(matches!(e, BackendError::Os { errno: sysabi::Errno::ENOENT, .. }), "{e:?}");
        let mut r = req(&["echo"]);
        r.cwd = b"/nao/existe".to_vec();
        let e = run(&*sb, r, vec![], limits(), None, &Cancel::new()).unwrap_err();
        assert_eq!(e.to_string(), "/nao/existe: No such file or directory");
    }
}
