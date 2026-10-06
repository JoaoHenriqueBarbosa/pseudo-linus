//! TCP de loopback (`127.0.0.1`) entre os processos de um sandbox.
//!
//! O sandbox não tem pilha de rede: uma conexão é um par de pipes, um por sentido, e um socket que
//! escuta é uma fila de conexões prontas pendurada numa tabela de portas do sandbox. `read`, `write` e
//! `poll` de uma conexão usam o caminho dos pipes; o lado que fecha (ou faz `shutdown`) solta a sua
//! ponta, e o outro lado vê EOF na leitura ou EPIPE na escrita, como num socket de verdade.
//!
//! Portas efêmeras saem de `ip_local_port_range` do Debian 13 (32768 a 60999).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use sysabi::{Errno, PollEvents, Stat};

use crate::park::{Parker, WaitList};
use crate::pipe::{Pipe, PipeEnd, Try};

const EPHEMERAL_LOW: u16 = 32768;
const EPHEMERAL_HIGH: u16 = 60999;
/// `st_dev` do sockfs (0:9 no Debian 13, como o `fstat` de um socket do oráculo mostra).
const SOCKFS_DEV: u64 = 9;

/// `stat` de um socket: o inode e o dono vêm do pipe de identidade do objeto; as datas do sockfs são zero.
pub(crate) fn sock_stat(ident: &Pipe) -> Stat {
    let mut st = ident.stat(None);
    st.dev = SOCKFS_DEV;
    st.mode = sysabi::mode::S_IFSOCK | 0o777;
    st.atime = sysabi::TimeSpec::default();
    st.mtime = st.atime;
    st.ctime = st.atime;
    st
}

/// As portas em escuta de um sandbox.
#[derive(Debug)]
pub(crate) struct Ports {
    st: Mutex<PortsState>,
}

#[derive(Debug)]
struct PortsState {
    listeners: HashMap<u16, Weak<Listener>>,
    next: u16,
}

impl Default for Ports {
    fn default() -> Self {
        Ports { st: Mutex::new(PortsState { listeners: HashMap::new(), next: EPHEMERAL_LOW }) }
    }
}

impl Ports {
    /// Próxima porta efêmera que não está em escuta.
    fn ephemeral(st: &mut PortsState) -> Result<u16, Errno> {
        for _ in EPHEMERAL_LOW..=EPHEMERAL_HIGH {
            let port = st.next;
            st.next = if port >= EPHEMERAL_HIGH { EPHEMERAL_LOW } else { port + 1 };
            if !st.listeners.get(&port).is_some_and(|w| w.strong_count() > 0) {
                return Ok(port);
            }
        }
        Err(Errno::EADDRINUSE)
    }

    /// `bind` mais `listen`: porta 0 escolhe uma efêmera. EADDRINUSE se já há alguém escutando nela.
    pub(crate) fn listen(self: &Arc<Self>, port: u16, backlog: u32, ident: Arc<Pipe>) -> Result<Arc<Listener>, Errno> {
        let mut st = self.st.lock();
        let port = if port == 0 { Self::ephemeral(&mut st)? } else { port };
        if st.listeners.get(&port).is_some_and(|w| w.strong_count() > 0) {
            return Err(Errno::EADDRINUSE);
        }
        let l = Arc::new(Listener {
            port,
            ident,
            backlog: backlog.clamp(1, 4096) as usize,
            ports: Arc::downgrade(self),
            st: Mutex::new(ListenState::default()),
        });
        st.listeners.insert(port, Arc::downgrade(&l));
        Ok(l)
    }

    /// `connect` a `127.0.0.1:port`. As duas pontas nascem prontas: a do servidor vai pra fila do
    /// socket que escuta. ECONNREFUSED sem ninguém escutando ou com a fila cheia.
    pub(crate) fn connect(&self, port: u16, mk_pipe: impl Fn() -> Arc<Pipe>) -> Result<Conn, Errno> {
        let (listener, local) = {
            let mut st = self.st.lock();
            let l = st.listeners.get(&port).and_then(Weak::upgrade).ok_or(Errno::ECONNREFUSED)?;
            (l, Self::ephemeral(&mut st)?)
        };
        let up = mk_pipe();
        let down = mk_pipe();
        let (client_reset, server_reset) = (Arc::new(AtomicU8::new(RESET_NONE)), Arc::new(AtomicU8::new(RESET_NONE)));
        let client = Conn::new(down.attach(true, false), up.attach(false, true), local, port, down.clone(), (client_reset.clone(), server_reset.clone()));
        let server = Conn::new(up.attach(true, false), down.attach(false, true), port, local, up, (server_reset, client_reset));
        let wake = {
            let mut s = listener.st.lock();
            if s.closed || s.queue.len() >= listener.backlog {
                return Err(Errno::ECONNREFUSED);
            }
            s.queue.push_back(server);
            s.wait.take()
        };
        wake.run();
        Ok(client)
    }
}

#[derive(Debug, Default)]
struct ListenState {
    queue: VecDeque<Conn>,
    wait: WaitList,
    closed: bool,
}

/// Um socket em escuta.
#[derive(Debug)]
pub(crate) struct Listener {
    pub port: u16,
    pub ident: Arc<Pipe>,
    backlog: usize,
    ports: Weak<Ports>,
    st: Mutex<ListenState>,
}

impl Listener {
    /// Tira a próxima conexão da fila; sem nenhuma, EAGAIN (`nonblock`) ou registra o parker.
    pub(crate) fn try_accept(&self, nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<Conn, Errno>> {
        let mut s = self.st.lock();
        if let Some(c) = s.queue.pop_front() {
            s.wait.unregister(waiter);
            return Try::Ready(Ok(c));
        }
        if nonblock {
            s.wait.unregister(waiter);
            return Try::Ready(Err(Errno::EAGAIN));
        }
        s.wait.register(waiter);
        Try::Pending
    }

    pub(crate) fn poll(&self, waiter: Option<&Arc<Parker>>) -> PollEvents {
        let mut s = self.st.lock();
        if let Some(w) = waiter {
            s.wait.register(w);
        }
        if s.queue.is_empty() { PollEvents::empty() } else { PollEvents::IN }
    }

    pub(crate) fn unregister(&self, waiter: &Arc<Parker>) {
        self.st.lock().wait.unregister(waiter);
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let wake = {
            let mut s = self.st.lock();
            s.closed = true;
            // As conexões que ninguém aceitou fecham: o cliente vê EOF.
            s.queue.clear();
            s.wait.take()
        };
        wake.run();
        if let Some(ports) = self.ports.upgrade() {
            let mut st = ports.st.lock();
            if st.listeners.get(&self.port).is_some_and(|w| w.strong_count() == 0) {
                st.listeners.remove(&self.port);
            }
        }
    }
}

/// Estado de RST de uma ponta: nenhum, erro pendente (a próxima operação dá ECONNRESET) ou já entregue.
const RESET_NONE: u8 = 0;
const RESET_PENDING: u8 = 1;
const RESET_DONE: u8 = 2;

/// O que uma operação numa conexão resetada devolve.
pub(crate) enum ResetState {
    None,
    /// Primeira operação depois do RST: ECONNRESET.
    Pending,
    /// Depois do erro entregue: a leitura dá EOF e a escrita EPIPE.
    Done,
}

/// Uma conexão estabelecida: a ponta de leitura do pipe que chega e a de escrita do que sai. `shutdown`
/// solta uma delas. Fechar com dados recebidos e não lidos manda RST, como o TCP do Linux: o outro lado
/// recebe ECONNRESET na próxima operação.
#[derive(Debug)]
pub(crate) struct Conn {
    rx: Mutex<Option<PipeEnd>>,
    tx: Mutex<Option<PipeEnd>>,
    pub local: u16,
    pub peer: u16,
    /// Dá o inode, o dono e a data do `fstat`.
    pub ident: Arc<Pipe>,
    /// O RST que esta ponta recebeu e o da outra ponta (que esta marca ao fechar).
    reset_me: Arc<AtomicU8>,
    reset_peer: Arc<AtomicU8>,
}

impl Drop for Conn {
    fn drop(&mut self) {
        let unread = self.rx.lock().as_ref().is_some_and(|e| e.pipe.pending() > 0);
        if unread {
            let _ = self.reset_peer.compare_exchange(RESET_NONE, RESET_PENDING, Ordering::SeqCst, Ordering::SeqCst);
        }
        // As pontas caem depois daqui: o outro lado acorda com a flag já posta.
    }
}

impl Conn {
    fn new(rx: PipeEnd, tx: PipeEnd, local: u16, peer: u16, ident: Arc<Pipe>, reset: (Arc<AtomicU8>, Arc<AtomicU8>)) -> Conn {
        Conn { rx: Mutex::new(Some(rx)), tx: Mutex::new(Some(tx)), local, peer, ident, reset_me: reset.0, reset_peer: reset.1 }
    }

    /// Escrita para um par que já fechou: o Linux aceita a primeira e o outro lado responde com RST,
    /// então a escrita conta como feita e a próxima operação desta ponta dá ECONNRESET.
    pub(crate) fn write_to_closed_peer(&self) -> bool {
        let closed = self.tx().is_some_and(|p| p.counters().0 == 0);
        if closed {
            let _ = self.reset_me.compare_exchange(RESET_NONE, RESET_PENDING, Ordering::SeqCst, Ordering::SeqCst);
        }
        closed
    }

    /// Consome o RST pendente: `Pending` só uma vez, depois `Done`.
    pub(crate) fn take_reset(&self) -> ResetState {
        match self.reset_me.load(Ordering::SeqCst) {
            RESET_NONE => ResetState::None,
            RESET_PENDING => {
                self.reset_me.store(RESET_DONE, Ordering::SeqCst);
                ResetState::Pending
            }
            _ => ResetState::Done,
        }
    }

    /// O pipe de leitura; `None` depois de `shutdown(SHUT_RD)` (a leitura dá EOF).
    pub(crate) fn rx(&self) -> Option<Arc<Pipe>> {
        self.rx.lock().as_ref().map(|e| e.pipe.clone())
    }

    /// O pipe de escrita; `None` depois de `shutdown(SHUT_WR)` (a escrita dá EPIPE).
    pub(crate) fn tx(&self) -> Option<Arc<Pipe>> {
        self.tx.lock().as_ref().map(|e| e.pipe.clone())
    }

    pub(crate) fn shutdown(&self, read: bool, write: bool) {
        let (r, w) = (read.then(|| self.rx.lock().take()), write.then(|| self.tx.lock().take()));
        drop((r, w));
    }

    pub(crate) fn poll(&self, waiter: Option<&Arc<Parker>>) -> PollEvents {
        let mut ev = PollEvents::empty();
        if self.reset_me.load(Ordering::SeqCst) == RESET_PENDING {
            ev |= PollEvents::IN | PollEvents::OUT | PollEvents::ERR | PollEvents::HUP;
        }
        match self.rx() {
            Some(p) => ev |= p.poll(true, false, waiter),
            None => ev |= PollEvents::IN,
        }
        match self.tx() {
            Some(p) => ev |= p.poll(false, true, waiter),
            None => ev |= PollEvents::OUT,
        }
        // Do lado que escreve, um par fechado é HUP de socket, não ERR de pipe.
        if ev.contains(PollEvents::ERR) {
            ev.remove(PollEvents::ERR);
            ev |= PollEvents::HUP;
        }
        ev
    }

    pub(crate) fn unregister(&self, waiter: &Arc<Parker>) {
        if let Some(p) = self.rx() {
            p.unregister(waiter);
        }
        if let Some(p) = self.tx() {
            p.unregister(waiter);
        }
    }
}
