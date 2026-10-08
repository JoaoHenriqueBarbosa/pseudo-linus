//! Dados auxiliares de socket em trânsito (`struct scm_cookie`): descritores (`SCM_RIGHTS`) e credenciais
//! (`SCM_CREDENTIALS`) que viajam com uma mensagem de um socket `AF_UNIX`.
//!
//! Datagrama e seqpacket guardam um [`Scm`] por mensagem. Um fluxo é uma fila de bytes (o par de pipes de
//! [`crate::net::conn_pair`]), então os dados auxiliares ficam ao lado, em [`Marks`]: cada `sendmsg` que
//! leva descritores, ou credenciais diferentes das anteriores, deixa uma marca com a posição absoluta do
//! primeiro byte. A leitura segue o `unix_stream_read_generic`: um `sk_buff` com descritores é lido por
//! inteiro ou em parte, entrega os descritores na primeira leitura que o toca e encerra a chamada; com
//! `SO_PASSCRED` nunca cola mensagens de remetentes diferentes.

use std::collections::VecDeque;
use std::sync::Arc;

use crate::fd::Ofd;

/// O primeiro `sk_buff` de um `sendmsg` de fluxo: `min(SKB_MAX_HEAD(0) + UNIX_SKB_FRAGS_SZ, (sk_sndbuf >> 1) - 64)`
/// com `SKB_MAX_HEAD(0)` = 4096 - 320 e `UNIX_SKB_FRAGS_SZ` = 32768. Os descritores vão só nele.
pub(crate) const STREAM_SKB_MAX: u64 = 3776 + 32768;

/// `struct ucred`: `pid`, `uid` e `gid` de quem enviou (ou do par, no `SO_PEERCRED`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Ucred {
    pub pid: i32,
    pub uid: u32,
    pub gid: u32,
}

impl Ucred {
    /// O que o `SO_PEERCRED` devolve num socket sem par (`cred_to_ucred` sem pid): pid 0 e uid/gid -1.
    pub(crate) const UNSET: Ucred = Ucred { pid: 0, uid: u32::MAX, gid: u32::MAX };

    pub(crate) fn to_bytes(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(12);
        out.extend_from_slice(&self.pid.to_le_bytes());
        out.extend_from_slice(&self.uid.to_le_bytes());
        out.extend_from_slice(&self.gid.to_le_bytes());
        out
    }

    /// O `struct ucred` de um item `SCM_CREDENTIALS`; exige os 12 bytes.
    pub(crate) fn from_bytes(b: &[u8]) -> Option<Ucred> {
        if b.len() != 12 {
            return None;
        }
        let word = |at: usize| <[u8; 4]>::try_from(&b[at..at + 4]).unwrap();
        Some(Ucred { pid: i32::from_le_bytes(word(0)), uid: u32::from_le_bytes(word(4)), gid: u32::from_le_bytes(word(8)) })
    }
}

/// Os dados auxiliares de uma mensagem: os arquivos em trânsito (a referência de cada um é a da mensagem, como o
/// `scm_fp_list`) e as credenciais de quem enviou.
#[derive(Clone, Debug, Default)]
pub(crate) struct Scm {
    pub fds: Vec<Arc<Ofd>>,
    pub cred: Ucred,
}

#[derive(Debug)]
struct Mark {
    /// Posição absoluta do primeiro byte do `sk_buff`.
    start: u64,
    cred: Ucred,
    /// Os descritores que o `sk_buff` ainda leva; esvazia na primeira leitura que o toca.
    fds: Vec<Arc<Ofd>>,
    /// Fim do `sk_buff` com descritores; igual a `start` quando não há (ou quando já foram entregues).
    fd_end: u64,
}

/// O que uma leitura de fluxo enxerga: até onde ela vai e de quem são os dados.
#[derive(Debug, Default)]
pub(crate) struct Window {
    /// Quantos bytes a leitura leva.
    pub len: usize,
    pub cred: Ucred,
    /// A marca com descritores que a leitura toca.
    fds_at: Option<usize>,
}

/// As marcas de um sentido de um fluxo, com os contadores absolutos de bytes escritos e lidos.
#[derive(Debug, Default)]
pub(crate) struct Marks {
    written: u64,
    read: u64,
    list: VecDeque<Mark>,
    /// As credenciais do trecho em que a leitura está, e as da última escrita (o que decide uma marca nova).
    cur: Ucred,
    last: Ucred,
}

impl Marks {
    /// `n` bytes foram acrescentados à fila. `send` descreve o `sendmsg` (sem ele, a escrita não é de um socket
    /// Unix e não deixa marca); `first` diz que é o começo do envio, `total` o tamanho dele.
    pub(crate) fn wrote(&mut self, n: usize, send: Option<&Scm>, first: bool, total: usize) {
        if n == 0 {
            return;
        }
        let start = self.written;
        self.written += n as u64;
        let Some(send) = send else { return };
        let with_fds = first && !send.fds.is_empty();
        if with_fds || (first && send.cred != self.last) {
            let fd_end = if with_fds { start + (total as u64).min(STREAM_SKB_MAX) } else { start };
            self.list.push_back(Mark { start, cred: send.cred, fds: if with_fds { send.fds.clone() } else { Vec::new() }, fd_end });
            self.last = send.cred;
        }
    }

    /// Onde uma leitura de até `want` bytes termina, dado o que há na fila (`avail`).
    pub(crate) fn window(&self, want: usize, avail: usize, passcred: bool) -> Window {
        let mut end = self.read + want.min(avail) as u64;
        let mut cred = self.cur;
        let mut fds_at = None;
        for (i, m) in self.list.iter().enumerate() {
            if m.start <= self.read {
                cred = m.cred;
                if !m.fds.is_empty() {
                    end = end.min(m.fd_end);
                    fds_at = Some(i);
                    break;
                }
                continue;
            }
            if m.start >= end {
                break;
            }
            // O próximo `sk_buff` entra na leitura: com `SO_PASSCRED` só se for do mesmo remetente.
            if passcred && m.cred != cred {
                end = m.start;
                break;
            }
            if !m.fds.is_empty() {
                end = end.min(m.fd_end);
                fds_at = Some(i);
                break;
            }
        }
        Window { len: (end - self.read) as usize, cred, fds_at }
    }

    /// A leitura levou `window.len` bytes: devolve os descritores (os `sk_buff` já lidos saem da lista).
    pub(crate) fn consume(&mut self, window: &Window) -> Vec<Arc<Ofd>> {
        self.read += window.len as u64;
        let fds = window.fds_at.map(|i| std::mem::take(&mut self.list[i].fds)).unwrap_or_default();
        if let Some(i) = window.fds_at {
            // Os descritores saíram com a primeira leitura: o resto do `sk_buff` é dado comum.
            self.list[i].fd_end = self.list[i].start;
        }
        while let Some(m) = self.list.front() {
            if m.start <= self.read && m.fds.is_empty() {
                self.cur = m.cred;
                self.list.pop_front();
            } else {
                break;
            }
        }
        fds
    }

    /// `MSG_PEEK`: os descritores que a leitura tocaria, duplicados, sem tirá-los da fila.
    pub(crate) fn peek_fds(&self, window: &Window) -> Vec<Arc<Ofd>> {
        window.fds_at.map(|i| self.list[i].fds.clone()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cred(pid: i32) -> Ucred {
        Ucred { pid, uid: 0, gid: 0 }
    }

    fn plain(pid: i32) -> Scm {
        Scm { fds: Vec::new(), cred: cred(pid) }
    }

    #[test]
    fn ucred_round_trips_through_bytes() {
        let c = Ucred { pid: 7, uid: 1000, gid: u32::MAX };
        assert_eq!(Ucred::from_bytes(&c.to_bytes()), Some(c));
        assert_eq!(Ucred::from_bytes(&[0; 11]), None);
    }

    #[test]
    fn plain_writes_leave_no_marks_when_the_sender_does_not_change() {
        let mut m = Marks::default();
        m.wrote(3, Some(&Scm::default()), true, 3);
        m.wrote(3, Some(&Scm::default()), true, 3);
        assert!(m.list.is_empty());
        let w = m.window(100, 6, false);
        assert_eq!(w.len, 6);
        assert!(m.consume(&w).is_empty());
    }

    #[test]
    fn passcred_stops_at_a_different_sender() {
        let mut m = Marks::default();
        m.wrote(3, Some(&plain(5)), true, 3);
        m.wrote(3, Some(&plain(9)), true, 3);
        let w = m.window(100, 6, true);
        assert_eq!((w.len, w.cred), (3, cred(5)));
        m.consume(&w);
        let w = m.window(100, 3, true);
        assert_eq!((w.len, w.cred), (3, cred(9)));
        // Sem `SO_PASSCRED` as duas mensagens colam.
        let mut m = Marks::default();
        m.wrote(3, Some(&plain(5)), true, 3);
        m.wrote(3, Some(&plain(9)), true, 3);
        assert_eq!(m.window(100, 6, false).len, 6);
    }

    #[test]
    fn a_write_that_continues_does_not_open_a_new_segment() {
        let mut m = Marks::default();
        m.wrote(3, Some(&plain(5)), true, 6);
        m.wrote(3, Some(&plain(5)), false, 6);
        assert_eq!(m.window(100, 6, true).len, 6);
    }
}
