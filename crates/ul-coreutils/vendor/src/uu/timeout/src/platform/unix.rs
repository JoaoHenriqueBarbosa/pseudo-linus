// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (ToDO) sigstr setpgid sigchld sigwait getpid TTIN TTOU

// Porte pseudo-linus: o original usava rustix e libc (setpgid, sigprocmask, pre_exec com
// signal/close/prctl no filho entre o fork e o exec). Aqui tudo passa pelo pseudo-kernel:
//
// - os sinais que o timeout repassa ficam em "capturar" (`catch_timeout_signals`) e a espera é o
//   `wait_or_timeout` do uucore portado, que os consulta;
// - o que o `pre_exec` fazia no filho é feito no próprio timeout antes do spawn, porque o filho
//   herda as disposições "ignorar" e "padrão" (como depois de um `execve` no Linux) e as capturas
//   voltam ao padrão nele: TTIN e TTOU ficam no padrão, SIGPIPE ignorado continua ignorado;
// - `PR_SET_PDEATHSIG` não existe no contrato: se o timeout morrer por SIGKILL, o filho segue vivo
//   (no Linux receberia o sinal do timeout).
use sysio::io;
use sysio::os::unix::process::ExitStatusExt;
use sysio::process::Child;

use sysabi::{SigDisposition, Signal};
use uucore::process::{ChildExt, catch_timeout_signals, unblock_signal};
use uucore::signals::signal_by_name_or_value;

fn signal_from_raw(sig: i32) -> Option<Signal> {
    let s = Signal(sig);
    (sig > 0 && s.is_valid()).then_some(s)
}

/// Configure our own process group and the signals the wait consumes, right before the child is
/// spawned.
pub(crate) fn prepare(
    _cmd_builder: &mut sysio::process::Command,
    foreground: bool,
    _signal: usize,
) -> io::Result<()> {
    if !foreground {
        let _ = sysio::unistd::setpgid(0, 0);
    }
    // O que o `pre_exec` fazia no filho (ver o comentário do módulo).
    let _ = sysio::unistd::signal(Signal::SIGTTIN, SigDisposition::Default);
    let _ = sysio::unistd::signal(Signal::SIGTTOU, SigDisposition::Default);
    catch_timeout_signals()
}

/// Unix keeps no per-spawn platform state; the type exists so the facade
/// signatures match the Windows implementation (which carries a job object).
pub(crate) struct SpawnState;

/// Nothing to do after spawning on unix.
pub(crate) fn post_spawn(_child: &Child, _foreground: bool) -> SpawnState {
    SpawnState
}

pub(crate) fn send_signal(
    process: &mut Child,
    signal: usize,
    foreground: bool,
    _external: Option<usize>,
    _state: &SpawnState,
) {
    // NOTE: GNU timeout doesn't check for errors of signal.
    // The subprocess might have exited just after the timeout.
    let _ = process.send_signal(signal);
    if signal == 0 || foreground {
        return;
    }
    let _ = process.send_signal_group(signal);
    let kill_signal = signal_by_name_or_value("KILL").unwrap();
    let continued_signal = signal_by_name_or_value("CONT").unwrap();
    if signal != kill_signal && signal != continued_signal {
        let _ = process.send_signal(continued_signal);
        let _ = process.send_signal_group(continued_signal);
    }
}

/// The signal the child was terminated by, if it was terminated by a signal.
pub(crate) fn status_signal(status: sysio::process::ExitStatus) -> Option<i32> {
    status.signal()
}

pub(crate) fn preserve_signal_info(signal: core::ffi::c_int) -> core::ffi::c_int {
    // This is needed because timeout is expected to preserve the exit
    // status of its child. It is not the case that utilities have a
    // single simple exit code, that's an illusion some shells
    // provide.  Instead exit status is really two numbers:
    //
    //  - An exit code if the program ran to completion
    //
    //  - A signal number if the program was terminated by a signal
    //
    // The easiest way to preserve the latter seems to be to kill
    // ourselves with whatever signal our child exited with, which is
    // what the following is intended to accomplish.
    if let Some(sig) = signal_from_raw(signal) {
        let _ = unblock_signal(sig);
        let _ = sysio::unistd::kill(sysio::unistd::getpid(), sig);
    }
    signal
}
