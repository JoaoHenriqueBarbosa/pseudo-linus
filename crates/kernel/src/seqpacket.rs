//! `SOCK_SEQPACKET` do domínio Unix: um par de filas de mensagens, uma por sentido.
//!
//! Cada `send` é uma mensagem atômica com fronteira própria; cada `recv` entrega uma mensagem e
//! descarta o que não coube no buffer (`unix_dgram_recvmsg`, que o seqpacket reaproveita). Uma mensagem
//! vazia é uma mensagem, não o fim: o fim só vem quando o par fecha ou faz `shutdown`. O que um lado
//! enviou continua legível no outro depois que o remetente fecha.

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use sysabi::{Errno, PollEvents};

use crate::park::{Parker, WaitList, Wake, key, locked};
use crate::pipe::Try;
use crate::scm::Scm;

/// `sk_sndbuf` padrão (`net.core.wmem_default` do Debian 13).
const SNDBUF: usize = 212_992;
/// `RCV_SHUTDOWN` e `SEND_SHUTDOWN` do `sk_shutdown`.
const RCV_SHUTDOWN: u8 = 1;
const SEND_SHUTDOWN: u8 = 2;

/// O que uma mensagem de `len` bytes pesa no `sk_wmem_alloc` do remetente: o `sk_buff` (256) mais o
/// buffer de dados com o `skb_shared_info` (320), que o `kmalloc` arredonda para a potência de dois
/// (ou para a página, nas grandes).
fn truesize(len: usize) -> usize {
    let data = len + 320;
    let data = if data <= 8192 { data.next_power_of_two().max(512) } else { data.next_multiple_of(4096) };
    data + 256
}

#[derive(Debug, Default)]
struct Dir {
    /// As mensagens enviadas por um lado, na ordem, à espera do outro.
    msgs: VecDeque<(Vec<u8>, Scm)>,
    /// A soma do `truesize` delas: o que o remetente tem alocado.
    charged: usize,
}

#[derive(Debug, Default)]
struct State {
    /// `dirs[i]` guarda o que o lado `i` enviou.
    dirs: [Dir; 2],
    /// `sk_shutdown` de cada lado.
    shutdown: [u8; 2],
    /// `sk_err` (`ECONNRESET`) pendente em cada lado.
    reset: [bool; 2],
    /// Quem espera em cada lado: leitura, escrita e `poll`.
    wait: [WaitList; 2],
}

/// As duas pontas compartilham este estado; cada `SeqEnd` é uma delas.
#[derive(Debug, Default)]
struct Shared {
    st: Mutex<State>,
}

/// Uma ponta de uma conexão `SOCK_SEQPACKET`.
#[derive(Debug)]
pub(crate) struct SeqEnd {
    shared: Arc<Shared>,
    side: usize,
}

/// As duas pontas de uma conexão nova.
pub(crate) fn seq_pair() -> (SeqEnd, SeqEnd) {
    let shared = Arc::new(Shared::default());
    (SeqEnd { shared: shared.clone(), side: 0 }, SeqEnd { shared, side: 1 })
}

impl Drop for SeqEnd {
    /// `unix_release_sock`: o par passa a ter `SHUTDOWN_MASK` e, se este lado fechou com mensagens
    /// sem ler, `ECONNRESET`.
    fn drop(&mut self) {
        let other = 1 - self.side;
        locked(&self.shared.st, |s| {
            s.shutdown[self.side] = RCV_SHUTDOWN | SEND_SHUTDOWN;
            s.shutdown[other] = RCV_SHUTDOWN | SEND_SHUTDOWN;
            if !s.dirs[other].msgs.is_empty() {
                s.reset[other] = true;
            }
            ((), s.wait[other].take())
        });
    }
}

impl SeqEnd {
    /// O par passa a ter o `ECONNRESET` pendente: o embrião que o `unix_release_sock` descarta da fila de
    /// `accept` marca o cliente assim, mesmo sem mensagem para ler.
    pub(crate) fn reset_remote(&self) {
        locked(&self.shared.st, |s| {
            let other = 1 - self.side;
            s.reset[other] = true;
            ((), s.wait[other].take())
        });
    }

    /// O outro lado ainda existe.
    pub(crate) fn peer_open(&self) -> bool {
        Arc::strong_count(&self.shared) > 1
    }

    /// `sendmsg`: a mensagem inteira ou nada, com os dados auxiliares `scm`. EMSGSIZE acima de `sk_sndbuf - 32`;
    /// EPIPE se este lado ou o par encerrou o envio; com o `sndbuf` cheio, EAGAIN (`nonblock`) ou espera.
    pub(crate) fn try_send(&self, data: &[u8], scm: &Scm, nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<usize, Errno>> {
        if data.len() > SNDBUF - 32 {
            return Try::Ready(Err(Errno::EMSGSIZE));
        }
        let (me, other) = (self.side, 1 - self.side);
        locked(&self.shared.st, |s| {
            if std::mem::take(&mut s.reset[me]) {
                return (Try::Ready(Err(Errno::ECONNRESET)), Wake::none());
            }
            if s.shutdown[me] & SEND_SHUTDOWN != 0 {
                return (Try::Ready(Err(Errno::EPIPE)), Wake::none());
            }
            if s.dirs[me].charged >= SNDBUF {
                if nonblock {
                    s.wait[me].unregister(waiter);
                    return (Try::Ready(Err(Errno::EAGAIN)), Wake::none());
                }
                s.wait[me].register(waiter);
                return (Try::Pending, Wake::none());
            }
            s.wait[me].unregister(waiter);
            s.dirs[me].charged += truesize(data.len());
            s.dirs[me].msgs.push_back((data.to_vec(), scm.clone()));
            (Try::Ready(Ok(data.len())), s.wait[other].take_key(key::READ))
        })
    }

    /// `recvmsg`: a próxima mensagem inteira (o chamador corta no tamanho do buffer). Com `peek` ela
    /// continua na fila. Sem mensagem, `Ok` vazio se o envio do par acabou, senão EAGAIN ou espera.
    /// `unix_inq_len` de um `SOCK_SEQPACKET`: a soma dos bytes das mensagens que o par mandou e ninguém leu.
    pub(crate) fn unread(&self) -> usize {
        self.shared.st.lock().dirs[1 - self.side].msgs.iter().map(|(data, _)| data.len()).sum()
    }

    pub(crate) fn try_recv(&self, peek: bool, nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<(Vec<u8>, Scm), Errno>> {
        let (me, other) = (self.side, 1 - self.side);
        locked(&self.shared.st, |s| {
            if std::mem::take(&mut s.reset[me]) {
                return (Try::Ready(Err(Errno::ECONNRESET)), Wake::none());
            }
            let got = if peek {
                s.dirs[other].msgs.front().cloned()
            } else {
                s.dirs[other].msgs.pop_front().inspect(|m| s.dirs[other].charged -= truesize(m.0.len()))
            };
            if let Some(m) = got {
                s.wait[me].unregister(waiter);
                // Sem `peek`, o remetente ganhou espaço.
                let wake = if peek { Wake::none() } else { s.wait[other].take_key(key::WRITE) };
                return (Try::Ready(Ok(m)), wake);
            }
            if s.shutdown[me] & RCV_SHUTDOWN != 0 {
                s.wait[me].unregister(waiter);
                return (Try::Ready(Ok((Vec::new(), Scm::default()))), Wake::none());
            }
            if nonblock {
                s.wait[me].unregister(waiter);
                return (Try::Ready(Err(Errno::EAGAIN)), Wake::none());
            }
            s.wait[me].register(waiter);
            (Try::Pending, Wake::none())
        })
    }

    /// `shutdown`: `RCV_SHUTDOWN` para quem encerra a leitura e `SEND_SHUTDOWN` no par (e o inverso).
    pub(crate) fn shutdown(&self, read: bool, write: bool) {
        let (me, other) = (self.side, 1 - self.side);
        locked(&self.shared.st, |s| {
            if read {
                s.shutdown[me] |= RCV_SHUTDOWN;
                s.shutdown[other] |= SEND_SHUTDOWN;
            }
            if write {
                s.shutdown[me] |= SEND_SHUTDOWN;
                s.shutdown[other] |= RCV_SHUTDOWN;
            }
            let mut wake = s.wait[me].take();
            wake.merge(s.wait[other].take());
            ((), wake)
        });
    }

    /// `unix_dgram_poll`.
    pub(crate) fn poll(&self, waiter: Option<&Arc<Parker>>) -> PollEvents {
        let (me, other) = (self.side, 1 - self.side);
        let mut s = self.shared.st.lock();
        if let Some(w) = waiter {
            s.wait[me].register(w);
        }
        let mut ev = PollEvents::empty();
        if s.reset[me] {
            ev |= PollEvents::ERR;
        }
        if s.shutdown[me] == RCV_SHUTDOWN | SEND_SHUTDOWN {
            ev |= PollEvents::HUP;
        }
        if s.shutdown[me] & RCV_SHUTDOWN != 0 {
            ev |= PollEvents::RDHUP | PollEvents::IN;
        }
        if !s.dirs[other].msgs.is_empty() {
            ev |= PollEvents::IN;
        }
        // `unix_writable`: o `wmem_alloc` cabe em um quarto do `sndbuf`.
        if s.shutdown[me] & SEND_SHUTDOWN != 0 || s.dirs[me].charged * 4 <= SNDBUF {
            ev |= PollEvents::OUT;
        }
        ev
    }

    pub(crate) fn unregister(&self, waiter: &Arc<Parker>) {
        self.shared.st.lock().wait[self.side].unregister(waiter);
    }
}
