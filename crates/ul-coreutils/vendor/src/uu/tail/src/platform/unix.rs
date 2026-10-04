// This file is part of the uutils coreutils package.
//
// For the full copyright and license information, please view the LICENSE
// file that was distributed with this source code.

// spell-checker:ignore (ToDO) stdlib, ISCHR, GETFD
// spell-checker:ignore (options) EPERM, ENOSYS, NOSYS

// Porte pseudo-linus: `kill(pid, 0)` do pseudo-kernel no lugar do rustix.
use sysabi::Signal;

pub type Pid = i32;

pub struct ProcessChecker {
    pid: Pid,
}

impl ProcessChecker {
    pub fn new(process_id: Pid) -> Self {
        Self { pid: process_id }
    }

    pub fn is_dead(&self) -> bool {
        // Vivo enquanto kill(pid, 0) funciona ou dá EPERM (existe, mas não é nosso).
        self.pid <= 0
            || sysio::unistd::kill(self.pid, Signal(0))
                .is_err_and(|e| e.raw_os_error() != Some(sysio::errno::EPERM))
    }
}

impl Drop for ProcessChecker {
    fn drop(&mut self) {}
}

pub fn supports_pid_checks(pid: Pid) -> bool {
    pid > 0
        && sysio::unistd::kill(pid, Signal(0))
            .err()
            .and_then(|e| e.raw_os_error())
            != Some(sysio::errno::ENOSYS)
}
