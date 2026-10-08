//! `pidfd_open(2)`: o objeto atrás de um fd `anon_inode:[pidfd]`.
//!
//! O pidfd guarda o processo (não o número do pid, que pode ser reaproveitado) e fica legível quando ele
//! termina, como o `pidfd_poll` do 6.12: zumbi dá `EPOLLIN`, e depois que o pai o colhe (o `pid` já saiu da
//! tabela) vem `EPOLLHUP` junto. O despertar das filas vem do [`Death`](crate::proc::Death) do processo.

use std::sync::Arc;

use sysabi::PollEvents;

use crate::park::Parker;
use crate::proc::Proc;

/// `PIDFD_THREAD`: o mesmo valor de `O_EXCL`.
pub(crate) const PIDFD_THREAD: u32 = 0o200;

pub(crate) struct Pidfd {
    pub(crate) proc: Arc<Proc>,
}

impl Pidfd {
    pub(crate) fn new(proc: Arc<Proc>) -> Pidfd {
        Pidfd { proc }
    }

    /// O pid que o `fdinfo` mostra (`Pid:` e `NSpid:`).
    pub(crate) fn pid(&self) -> i32 {
        self.proc.pid
    }

    pub(crate) fn poll(&self, waiter: Option<&Arc<Parker>>) -> PollEvents {
        let mut death = self.proc.death.lock();
        if let Some(w) = waiter {
            death.wait.register(w);
        }
        match (death.exited, death.reaped) {
            (_, true) => PollEvents::IN | PollEvents::HUP,
            (true, false) => PollEvents::IN,
            (false, false) => PollEvents::empty(),
        }
    }

    pub(crate) fn unregister(&self, waiter: &Arc<Parker>) {
        self.proc.death.lock().wait.unregister(waiter);
    }

    /// As linhas que o `pidfd_show_fdinfo` acrescenta ao `fdinfo`.
    pub(crate) fn fdinfo_lines(&self) -> String {
        format!("Pid:\t{0}\nNSpid:\t{0}\n", self.pid())
    }
}
