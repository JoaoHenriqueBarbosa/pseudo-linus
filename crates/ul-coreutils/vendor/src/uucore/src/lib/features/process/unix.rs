// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore sigwait KTIME timeval itimerval setitimer itimer timerid
// spell-checker:ignore sigevent sigev sigval itimerspec signo clockid sevp

// Porte pseudo-linus: o original esperava o filho com `timer_create` + `sigwait` (libc, nix, rustix,
// unsafe) sobre sinais bloqueados do processo host. Aqui o filho é um `sysio::process::Child`, os
// sinais que o `timeout` repassa ficam com disposição "capturar" no pseudo-kernel
// (`sysio::unistd::signal`) e a espera é um laço de `try_wait` + sinais capturados + `nanosleep`
// curto (um sinal capturado interrompe o sono na hora). A semântica de `TimeoutRet` é a mesma.
use std::io;
use std::time::Duration;
use sysio::process::Child;
use sysio::sysabi::{KillTarget, SigDisposition, Signal};
use sysio::time::Instant;

use super::{ChildExt, TimeoutRet};

/// Maior espera de cada volta do laço: o atraso máximo pra notar que o filho terminou.
const POLL: Duration = Duration::from_millis(10);

fn to_signal(signal: usize) -> io::Result<Signal> {
    let sig = Signal(i32::try_from(signal).map_err(|_| io::Error::from_raw_os_error(sysio::errno::EINVAL))?);
    if sig.0 != 0 && !sig.is_valid() {
        return Err(io::Error::from_raw_os_error(sysio::errno::EINVAL));
    }
    Ok(sig)
}

impl ChildExt for Child {
    fn send_signal(&mut self, signal: usize) -> io::Result<()> {
        let sig = to_signal(signal)?;
        sysio::unistd::kill(self.id() as i32, sig)
    }

    fn send_signal_group(&mut self, signal: usize) -> io::Result<()> {
        // Send signal to our process group (group 0 = caller's group).
        // This matches GNU coreutils behavior: if the child has remained in our
        // process group, it will receive this signal along with all other processes
        // in the group. If the child has created its own process group (via setpgid),
        // it won't receive this group signal, but will have received the direct signal.
        let sig = to_signal(signal)?;
        if sig.0 == 0 {
            return sysio::unistd::kill_target(KillTarget::Group(0), sig);
        }
        // Ignore the signal temporarily so we don't receive it ourselves.
        let old = sysio::unistd::signal(sig, SigDisposition::Ignore)?;
        let result = sysio::unistd::kill_target(KillTarget::Group(0), sig);
        let _ = sysio::unistd::signal(sig, old);
        result
    }

    fn wait_or_timeout(&mut self, timeout: Duration, ignore_term: bool) -> io::Result<TimeoutRet> {
        if timeout == Duration::from_micros(0) {
            return self.wait().map(TimeoutRet::Exited);
        }
        // .try_wait() doesn't drop stdin, so we do it manually
        drop(self.stdin.take());

        let start = Instant::now();
        loop {
            if let Some(status) = self.try_wait()? {
                break Ok(TimeoutRet::Exited(status));
            }
            for sig in sysio::unistd::take_caught_signals() {
                if sig == Signal::SIGCHLD || (sig == Signal::SIGTERM && ignore_term) {
                    continue;
                }
                return Ok(TimeoutRet::Interrupted(sig.0 as usize));
            }
            let elapsed = start.elapsed();
            if elapsed >= timeout {
                break Ok(TimeoutRet::TimedOut);
            }
            // EINTR (sinal capturado) volta na hora pra ser tratado na próxima volta.
            let _ = sysio::time::sleep_interruptible(timeout.saturating_sub(elapsed).min(POLL), true);
        }
    }
}

/// Os sinais que o `timeout` repassa ao filho. No original eram bloqueados antes de esperar; aqui o
/// chamador põe a disposição deles em "capturar" com [`catch_timeout_signals`].
pub fn timeout_signal_set() -> Vec<Signal> {
    vec![
        Signal::SIGALRM,
        Signal::SIGINT,
        Signal::SIGQUIT,
        Signal::SIGHUP,
        Signal::SIGTERM,
        Signal::SIGPIPE,
        Signal::SIGUSR1,
        Signal::SIGUSR2,
        Signal::SIGCHLD,
    ]
}

/// Captura os sinais de [`timeout_signal_set`] (o equivalente a bloqueá-los pro `sigwait`).
pub fn catch_timeout_signals() -> io::Result<()> {
    for sig in timeout_signal_set() {
        sysio::unistd::signal(sig, SigDisposition::Catch)?;
    }
    Ok(())
}

/// Volta um sinal à disposição padrão (o original desbloqueava na thread; um filho criado pelo
/// pseudo-kernel já nasce com as capturas do pai resetadas, como depois de um `execve`).
pub fn unblock_signal(signal: Signal) -> io::Result<()> {
    sysio::unistd::signal(signal, SigDisposition::Default).map(|_| ())
}

// Porte pseudo-linus: os módulos `timer` (timer_create/setitimer + sigwait, com unsafe) saíram; a
// espera com prazo é o laço de `wait_or_timeout` acima.
