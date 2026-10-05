//! Estado de sinais de um processo e as regras de geração e entrega do Linux (`kernel/signal.c`).
//!
//! - Geração (`send`): sinal ignorado (explicitamente, ou com ação padrão de ignorar) é descartado na
//!   hora; SIGCONT sempre retoma e descarta paradas pendentes; parada descarta SIGCONT pendente.
//! - Disposição `Catch`: o sinal entra na fila de capturados (sinais padrão não se acumulam: um que já
//!   está na fila não entra de novo; tempo real entra sempre) e interrompe a syscall bloqueante com
//!   EINTR. O programa consulta com `take_caught_signals` (é o `trap` do shell).
//! - Ação padrão de terminar ou de parar: fica pendente até o próximo ponto de checagem, onde a
//!   disposição é conferida de novo (o programa pode ter mudado de ideia).

use std::collections::VecDeque;

use sysabi::{DefaultAction, SigDisposition, Signal};

pub(crate) const NSIG: usize = 65;

fn bit(sig: Signal) -> u64 {
    1u64 << (sig.0 as u64 & 63)
}

pub(crate) fn is_stop_signal(sig: Signal) -> bool {
    matches!(sig.0, 19..=22)
}

const STOP_MASK: u64 = (1 << 19) | (1 << 20) | (1 << 21) | (1 << 22);

/// O que fazer depois de gerar um sinal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Generated {
    /// Descartado (ignorado, ou ação padrão de ignorar).
    Discarded,
    /// Precisa acordar o processo (pendente ou capturado).
    Wake,
}

/// Ação a tomar num ponto de entrega.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    None,
    Fatal(Signal),
    Stop(Signal),
}

#[derive(Clone, Debug)]
pub(crate) struct SigState {
    /// Sinais com ação padrão de terminar ou parar, esperando o ponto de entrega (bit = número).
    pending: u64,
    disp: [SigDisposition; NSIG],
    caught: VecDeque<Signal>,
    /// Conta capturas: a syscall bloqueante dá EINTR se mudou desde que ela começou.
    caught_seq: u64,
}

impl SigState {
    pub(crate) fn new() -> SigState {
        SigState { pending: 0, disp: [SigDisposition::Default; NSIG], caught: VecDeque::new(), caught_seq: 0 }
    }

    /// Estado do filho de um `fork`: mesmas disposições, nada pendente.
    pub(crate) fn forked(&self) -> SigState {
        SigState { pending: 0, disp: self.disp, caught: VecDeque::new(), caught_seq: 0 }
    }

    /// `execve`: capturados voltam ao padrão; ignorados continuam ignorados.
    pub(crate) fn exec_reset(&mut self) {
        for d in self.disp.iter_mut() {
            if *d == SigDisposition::Catch {
                *d = SigDisposition::Default;
            }
        }
        self.caught.clear();
    }

    pub(crate) fn disposition(&self, sig: Signal) -> SigDisposition {
        self.disp.get(sig.0 as usize).copied().unwrap_or(SigDisposition::Default)
    }

    /// `sigaction`. Ignorar um sinal pendente descarta ele.
    pub(crate) fn set_disposition(&mut self, sig: Signal, d: SigDisposition) -> SigDisposition {
        let old = self.disp[sig.0 as usize];
        self.disp[sig.0 as usize] = d;
        if d == SigDisposition::Ignore || (d == SigDisposition::Default && sig.default_action() == DefaultAction::Ignore) {
            self.pending &= !bit(sig);
            self.caught.retain(|s| *s != sig);
        }
        old
    }

    pub(crate) fn caught_seq(&self) -> u64 {
        self.caught_seq
    }

    pub(crate) fn has_work(&self) -> bool {
        self.pending != 0
    }

    pub(crate) fn fatal_pending(&self) -> bool {
        self.pending & !STOP_MASK != 0
    }

    pub(crate) fn take_caught(&mut self) -> Vec<Signal> {
        self.caught.drain(..).collect()
    }

    /// Máscaras pro `/proc/<pid>/status` (SigPnd, SigIgn, SigCgt).
    pub(crate) fn masks(&self) -> (u64, u64, u64) {
        let mut ign = 0u64;
        let mut cgt = 0u64;
        for n in 1..NSIG {
            let b = 1u64 << ((n - 1) as u64);
            match self.disp[n] {
                SigDisposition::Ignore => ign |= b,
                SigDisposition::Catch => cgt |= b,
                SigDisposition::Default => {}
            }
        }
        let mut pnd = 0u64;
        for n in 1..64u64 {
            if self.pending & (1 << n) != 0 {
                pnd |= 1 << (n - 1);
            }
        }
        for s in &self.caught {
            pnd |= 1u64 << ((s.0 - 1) as u64);
        }
        (pnd, ign, cgt)
    }

    /// Geração de um sinal (`send_signal_locked` + `prepare_signal`). SIGCONT e paradas já foram
    /// tratados pelo chamador quanto a retomar o processo; aqui só a fila.
    pub(crate) fn generate(&mut self, sig: Signal) -> Generated {
        if sig.0 == 18 {
            self.pending &= !STOP_MASK;
        }
        if is_stop_signal(sig) {
            self.pending &= !bit(Signal(18));
        }
        if sig.0 == 9 || sig.0 == 19 {
            self.pending |= bit(sig);
            return Generated::Wake;
        }
        match self.disposition(sig) {
            SigDisposition::Ignore => Generated::Discarded,
            SigDisposition::Catch => {
                if sig.0 >= sysabi::linux::SIGRTMIN || !self.caught.contains(&sig) {
                    self.caught.push_back(sig);
                }
                self.caught_seq += 1;
                Generated::Wake
            }
            SigDisposition::Default => match sig.default_action() {
                DefaultAction::Ignore | DefaultAction::Continue => Generated::Discarded,
                DefaultAction::Stop | DefaultAction::Terminate | DefaultAction::CoreDump => {
                    self.pending |= bit(sig);
                    Generated::Wake
                }
            },
        }
    }

    /// Escolhe o que entregar agora (`get_signal`): SIGKILL primeiro, depois do menor número pro maior.
    /// Um pendente cuja disposição mudou pra `Catch` vai pra fila; pra `Ignore`, some.
    pub(crate) fn dequeue(&mut self) -> Action {
        if self.pending & bit(Signal(9)) != 0 {
            self.pending &= !bit(Signal(9));
            return Action::Fatal(Signal(9));
        }
        while self.pending != 0 {
            let n = self.pending.trailing_zeros() as i32;
            let sig = Signal(n);
            self.pending &= !bit(sig);
            if n == 19 {
                return Action::Stop(sig);
            }
            match self.disposition(sig) {
                SigDisposition::Ignore => {}
                SigDisposition::Catch => {
                    if !self.caught.contains(&sig) {
                        self.caught.push_back(sig);
                    }
                    self.caught_seq += 1;
                }
                SigDisposition::Default => match sig.default_action() {
                    DefaultAction::Stop => return Action::Stop(sig),
                    DefaultAction::Terminate | DefaultAction::CoreDump => return Action::Fatal(sig),
                    DefaultAction::Ignore | DefaultAction::Continue => {}
                },
            }
        }
        Action::None
    }

    /// Descarta paradas pendentes (o SIGCONT chegou).
    pub(crate) fn clear_stops(&mut self) {
        self.pending &= !STOP_MASK;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ignored_signals_are_discarded_and_fatal_ones_pend() {
        let mut s = SigState::new();
        assert_eq!(s.generate(Signal::SIGCHLD), Generated::Discarded);
        assert_eq!(s.generate(Signal::SIGWINCH), Generated::Discarded);
        assert_eq!(s.generate(Signal::SIGTERM), Generated::Wake);
        assert_eq!(s.generate(Signal::SIGINT), Generated::Wake);
        assert_eq!(s.dequeue(), Action::Fatal(Signal::SIGINT));
        assert_eq!(s.dequeue(), Action::Fatal(Signal::SIGTERM));
        assert_eq!(s.dequeue(), Action::None);
    }

    #[test]
    fn kill_wins_and_catch_queues_without_duplicates() {
        let mut s = SigState::new();
        s.set_disposition(Signal::SIGUSR1, SigDisposition::Catch);
        s.generate(Signal::SIGUSR1);
        s.generate(Signal::SIGUSR1);
        s.generate(Signal::SIGTERM);
        s.generate(Signal::SIGKILL);
        assert_eq!(s.dequeue(), Action::Fatal(Signal::SIGKILL));
        assert_eq!(s.take_caught(), vec![Signal::SIGUSR1]);
        assert_eq!(s.caught_seq(), 2);
    }

    #[test]
    fn ignoring_a_pending_signal_discards_it() {
        let mut s = SigState::new();
        s.generate(Signal::SIGTERM);
        s.set_disposition(Signal::SIGTERM, SigDisposition::Ignore);
        assert_eq!(s.dequeue(), Action::None);
        assert_eq!(s.generate(Signal::SIGTERM), Generated::Discarded);
        assert_eq!(s.generate(Signal::SIGKILL), Generated::Wake, "SIGKILL não se ignora");
    }

    #[test]
    fn exec_resets_caught_to_default() {
        let mut s = SigState::new();
        s.set_disposition(Signal::SIGINT, SigDisposition::Catch);
        s.set_disposition(Signal::SIGQUIT, SigDisposition::Ignore);
        s.exec_reset();
        assert_eq!(s.disposition(Signal::SIGINT), SigDisposition::Default);
        assert_eq!(s.disposition(Signal::SIGQUIT), SigDisposition::Ignore);
    }
}
