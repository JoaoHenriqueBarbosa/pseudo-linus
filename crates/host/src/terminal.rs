//! Modo terminal: um processo do sandbox num pty do kernel, ligado ao terminal do host.
//!
//! É o que um emulador de terminal faz com o mestre de um pty, com o terminal do host no lugar da
//! janela:
//!
//! - o terminal do host fica em modo cru (sem eco, sem ICANON, sem ISIG, sem OPOST) e só transporta
//!   bytes; a disciplina de linha que vale é a do pty do sandbox, então Ctrl-C, Ctrl-Z, Ctrl-D e o eco
//!   são dela;
//! - o que chega no stdin do host vai pro mestre, e o que sai do mestre vai pro stdout do host na hora;
//! - o tamanho da janela do host vale no pty na partida e a cada SIGWINCH do host (que gera SIGWINCH
//!   no grupo em primeiro plano do sandbox);
//! - EOF ou erro na leitura do terminal do host, SIGHUP ou SIGTERM no host: o líder da sessão recebe
//!   SIGHUP e SIGCONT (o hangup de um mestre que fechou) e, se não sair no prazo, tudo leva SIGKILL.
//!
//! O modo do terminal do host volta ao que era na saída, também em erro.

use std::io::{IsTerminal, Read, Write};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use rustix::termios::{self, OptionalActions, Termios};
use signal_hook::consts::{SIGHUP, SIGTERM, SIGWINCH};
use signal_hook::iterator::Signals;
use sysabi::{KillTarget, Pid, Signal, Winsize};

use crate::backend::{HostTty, ReadOutcome, Sandbox, SpawnRequest, TtySpawned};

/// O `TERM` de quando o terminfo do sandbox não conhece o do host.
const FALLBACK_TERM: &str = "xterm-256color";
/// Onde o ncurses do Debian procura as entradas compiladas, na ordem dele.
const TERMINFO_DIRS: [&str; 3] = ["/etc/terminfo", "/lib/terminfo", "/usr/share/terminfo"];
/// Quanto o líder da sessão tem pra sair depois do hangup, antes do SIGKILL em tudo.
const HANGUP_GRACE: Duration = Duration::from_secs(2);
/// Quanto a saída ainda é lida depois de o processo sair, quando outro processo (um job em segundo
/// plano) ficou com o terminal aberto e o fim da saída não vem.
const DRAIN_GRACE: Duration = Duration::from_millis(200);
/// Prazo de cada espera sem fim (a leitura do mestre e a espera do processo voltam a esperar).
const IDLE: Duration = Duration::from_secs(3600);

/// O stdin e o stdout do host são terminais.
pub fn host_is_tty() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// O `TERM` que os processos do sandbox recebem: o do host, se o terminfo do sandbox tem a entrada
/// (`<dir>/<primeira letra>/<nome>`); senão, `xterm-256color`.
pub fn sandbox_term(sb: &dyn Sandbox, host_term: &str) -> String {
    let known = host_term.chars().next().is_some_and(|first| {
        !host_term.contains('/')
            && TERMINFO_DIRS.iter().any(|dir| sb.stat(format!("{dir}/{first}/{host_term}").as_bytes(), true).is_ok())
    });
    if known { host_term.to_string() } else { FALLBACK_TERM.to_string() }
}

/// O terminal do host em modo cru; soltar devolve o modo de antes.
struct RawMode {
    saved: Termios,
}

impl RawMode {
    fn enter() -> rustix::io::Result<RawMode> {
        let saved = termios::tcgetattr(std::io::stdin())?;
        let mut raw = saved.clone();
        raw.make_raw();
        termios::tcsetattr(std::io::stdin(), OptionalActions::Now, &raw)?;
        Ok(RawMode { saved })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = termios::tcsetattr(std::io::stdin(), OptionalActions::Drain, &self.saved);
    }
}

/// O tamanho da janela do terminal do host; zerado (o de um pty recém-aberto) se ele não diz.
fn host_winsize() -> Winsize {
    termios::tcgetwinsize(std::io::stdin())
        .map(|w| Winsize { rows: w.ws_row, cols: w.ws_col, xpixel: w.ws_xpixel, ypixel: w.ws_ypixel })
        .unwrap_or_default()
}

/// Do stdin do host pro mestre, até o EOF ou um erro do host (devolve `true`: o terminal do host
/// acabou) ou até o pty acabar (`false`).
fn pump_input(tty: &dyn HostTty) -> bool {
    let mut stdin = std::io::stdin().lock();
    let mut buf = [0u8; 4096];
    loop {
        match stdin.read(&mut buf) {
            Ok(0) => return true,
            Ok(n) => {
                if !tty.write(&buf[..n]) {
                    return false;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return true,
        }
    }
}

/// Do mestre pro stdout do host, até ninguém mais ter o terminal aberto.
fn pump_output(tty: &dyn HostTty) {
    let mut buf = vec![0u8; 64 << 10];
    loop {
        match tty.read_timeout(&mut buf, IDLE) {
            ReadOutcome::Data(n) => {
                let mut out = std::io::stdout().lock();
                if out.write_all(&buf[..n]).and_then(|()| out.flush()).is_err() {
                    return;
                }
            }
            ReadOutcome::TimedOut => {}
            ReadOutcome::Eof => return,
        }
    }
}

/// O terminal do host acabou: hangup no líder da sessão e, passado o prazo, SIGKILL em tudo.
fn hang_up(sb: &dyn Sandbox, leader: Pid) {
    let _ = sb.kill(KillTarget::Pid(leader), Signal::SIGHUP);
    let _ = sb.kill(KillTarget::Pid(leader), Signal::SIGCONT);
    std::thread::sleep(HANGUP_GRACE);
    let _ = sb.kill(KillTarget::All, Signal::SIGKILL);
}

fn spawn_thread(name: &str, f: impl FnOnce() + Send + 'static) -> Result<(), String> {
    std::thread::Builder::new().name(name.into()).spawn(f).map(drop).map_err(|e| format!("thread {name}: {e}"))
}

/// Roda `req` num pty do sandbox ligado ao terminal do host (stdin e stdout) e devolve o `$?` do
/// processo quando ele termina.
pub fn run(sb: Arc<dyn Sandbox>, req: SpawnRequest) -> Result<u8, String> {
    let raw = RawMode::enter().map_err(|e| format!("terminal do host: {e}"))?;
    let TtySpawned { pid, tty, mut exit } = sb.spawn_tty(req, host_winsize()).map_err(|e| e.to_string())?;
    let on_hangup = move || hang_up(&*sb, pid);

    let mut signals = Signals::new([SIGWINCH, SIGHUP, SIGTERM]).map_err(|e| format!("sinais do host: {e}"))?;
    let signals_handle = signals.handle();
    spawn_thread("osh-tty-sig", {
        let (tty, on_hangup) = (tty.clone(), on_hangup.clone());
        move || {
            for sig in signals.forever() {
                if sig == SIGWINCH {
                    tty.resize(host_winsize());
                } else {
                    on_hangup();
                }
            }
        }
    })?;
    // Fica presa na leitura do stdin quando o processo sai antes do terminal do host; o fim do
    // processo do host a leva junto.
    spawn_thread("osh-tty-in", {
        let tty = tty.clone();
        move || {
            if pump_input(&*tty) {
                on_hangup();
            }
        }
    })?;
    let (drained, wait_drained) = mpsc::channel::<()>();
    spawn_thread("osh-tty-out", move || {
        pump_output(&*tty);
        drop(drained);
    })?;

    let info = loop {
        if let Some(info) = exit.wait_timeout(IDLE) {
            break info;
        }
    };
    let _ = wait_drained.recv_timeout(DRAIN_GRACE);
    signals_handle.close();
    drop(raw);
    Ok(info.status.shell_status() as u8)
}
