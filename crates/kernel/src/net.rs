//! TCP de loopback (`127.0.0.1`) entre os processos de um sandbox.
//!
//! O sandbox não tem pilha de rede: uma conexão é um par de pipes, um por sentido, e um socket que
//! escuta é uma fila de conexões prontas pendurada numa tabela de portas do sandbox. `read`, `write` e
//! `poll` de uma conexão usam o caminho dos pipes; o lado que fecha (ou faz `shutdown`) solta a sua
//! ponta, e o outro lado vê EOF na leitura ou EPIPE na escrita, como num socket de verdade.
//!
//! Portas efêmeras saem de `ip_local_port_range` do Debian 13 (32768 a 60999), sorteadas como no Linux.
//!
//! Cada conexão fica registrada como um par de pontas até as duas saírem do TIME_WAIT: é dessa tabela,
//! com as portas em escuta, que saem o `/proc/net/tcp`, o `tcp6` e as contagens do `sockstat`.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use sysabi::{Errno, PollEvents, Stat};
use vfs::procfs::{TcpSock, tcp_state};

use crate::park::{Parker, WaitList};
use crate::pipe::{Pipe, PipeEnd, Try};

const EPHEMERAL_LOW: u16 = 32768;
const EPHEMERAL_HIGH: u16 = 60999;
/// `st_dev` do sockfs (0:9 no Debian 13, como o `fstat` de um socket do oráculo mostra).
const SOCKFS_DEV: u64 = 9;
/// `TCP_TIMEWAIT_LEN` e o `tcp_fin_timeout` padrão: o FIN_WAIT2 órfão e o TIME_WAIT duram 60 s.
const TIMEWAIT_LEN: Duration = Duration::from_secs(60);

/// O `%pK` de um socket: o ponteiro com hash, 32 bits aleatórios por objeto.
fn hashed_ptr() -> u32 {
    let mut b = [0u8; 4];
    let _ = getrandom::fill(&mut b);
    u32::from_ne_bytes(b)
}

/// Endereço como o kernel guarda: IPv4 nos 4 primeiros bytes, IPv6 inteiro.
fn ip_bytes(ip: IpAddr) -> [u8; 16] {
    match ip {
        IpAddr::V4(a) => {
            let mut b = [0u8; 16];
            b[..4].copy_from_slice(&a.octets());
            b
        }
        IpAddr::V6(a) => a.octets(),
    }
}

/// IPv4 dentro de IPv6 (`::ffff:a.b.c.d`), como um socket IPv6 de pilha dupla vê um par IPv4.
fn mapped(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(a) => IpAddr::V6(a.to_ipv6_mapped()),
        v6 => v6,
    }
}

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
    /// As conexões, da mais antiga à mais nova, até as duas pontas saírem do TIME_WAIT.
    pairs: Vec<Arc<Pair>>,
}

impl Default for Ports {
    fn default() -> Self {
        Ports { st: Mutex::new(PortsState { listeners: HashMap::new(), pairs: Vec::new() }) }
    }
}

impl Ports {
    /// Uma porta efêmera livre, como o Linux 6.12 escolhe: começo sorteado no `ip_local_port_range` e
    /// passo 2, com portas pares para o `connect` (`__inet_hash_connect`) e ímpares para o `bind` na
    /// porta 0 (`inet_csk_find_open_port`). Livre é sem ninguém escutando nem conexão viva usando.
    fn ephemeral(st: &mut PortsState, odd: bool) -> Result<u16, Errno> {
        let range = u32::from(EPHEMERAL_HIGH - EPHEMERAL_LOW) + 1;
        let start = (hashed_ptr() % range) & !1;
        let used = |st: &PortsState, port: u16| {
            st.listeners.get(&port).is_some_and(|w| w.strong_count() > 0) || st.pairs.iter().any(|p| p.uses(port))
        };
        for i in 0..range.div_ceil(2) {
            let off = (start + 2 * i) % (range & !1) + u32::from(odd);
            let port = EPHEMERAL_LOW + off as u16;
            if port <= EPHEMERAL_HIGH && !used(st, port) {
                return Ok(port);
            }
        }
        Err(Errno::EADDRINUSE)
    }

    /// `bind` em `ip` mais `listen`: porta 0 escolhe uma efêmera. EADDRINUSE se já há alguém escutando nela.
    pub(crate) fn listen(self: &Arc<Self>, ip: IpAddr, port: u16, backlog: u32, ident: Arc<Pipe>) -> Result<Arc<Listener>, Errno> {
        let mut st = self.st.lock();
        let port = if port == 0 { Self::ephemeral(&mut st, true)? } else { port };
        if st.listeners.get(&port).is_some_and(|w| w.strong_count() > 0) {
            return Err(Errno::EADDRINUSE);
        }
        let l = Arc::new(Listener {
            port,
            ip,
            uid: ident.stat(None).uid,
            ptr: hashed_ptr(),
            ident,
            backlog: backlog.clamp(1, 4096) as usize,
            ports: Arc::downgrade(self),
            st: Mutex::new(ListenState::default()),
        });
        st.listeners.insert(port, Arc::downgrade(&l));
        Ok(l)
    }

    /// `connect` a `dst:port` (um endereço de loopback). As duas pontas nascem prontas: a do servidor
    /// vai pra fila do socket que escuta. ECONNREFUSED sem ninguém escutando ou com a fila cheia.
    pub(crate) fn connect(&self, dst: IpAddr, port: u16, mk_pipe: impl Fn() -> Arc<Pipe>) -> Result<Conn, Errno> {
        let (listener, local) = {
            let mut st = self.st.lock();
            let l = st.listeners.get(&port).and_then(Weak::upgrade).ok_or(Errno::ECONNREFUSED)?;
            (l, Self::ephemeral(&mut st, false)?)
        };
        let up = mk_pipe();
        let down = mk_pipe();
        // O servidor vê o par na família do socket que escuta: IPv4 num socket IPv6 vira `::ffff:`.
        let server_ip = if listener.ip.is_ipv6() { mapped(dst) } else { dst };
        let client = Side::new((dst, local), (dst, port), down.ino, down.stat(None).uid, &down, true);
        let server = Side::new((server_ip, port), (server_ip, local), up.ino, listener.uid, &up, false);
        let pair = Arc::new(Pair { sides: Mutex::new([client, server]) });
        let (client_reset, server_reset) = (Arc::new(AtomicU8::new(RESET_NONE)), Arc::new(AtomicU8::new(RESET_NONE)));
        let client = Conn::new(down.attach(true, false), up.attach(false, true), local, port, down.clone(), (client_reset.clone(), server_reset.clone()), (pair.clone(), 0));
        let server = Conn::new(up.attach(true, false), down.attach(false, true), port, local, up, (server_reset, client_reset), (pair.clone(), 1));
        let wake = {
            let mut s = listener.st.lock();
            if s.closed || s.queue.len() >= listener.backlog {
                return Err(Errno::ECONNREFUSED);
            }
            s.queue.push_back(server);
            s.wait.take()
        };
        wake.run();
        self.st.lock().pairs.push(pair);
        Ok(client)
    }

    /// A tabela do `/proc/net/tcp` e do `tcp6`: os sockets em escuta, depois cada ponta das conexões.
    /// Conexões cujas duas pontas já saíram do TIME_WAIT somem da tabela.
    pub(crate) fn tcp_socks(&self) -> Vec<TcpSock> {
        let now = Instant::now();
        let (mut listeners, pairs) = {
            let mut st = self.st.lock();
            st.pairs.retain(|p| !p.gone(now));
            let ls: Vec<Arc<Listener>> = st.listeners.values().filter_map(Weak::upgrade).collect();
            (ls, st.pairs.clone())
        };
        listeners.sort_by_key(|l| l.port);
        let mut out = Vec::new();
        for l in &listeners {
            let queued = l.st.lock().queue.len() as u32;
            out.push(TcpSock {
                v6: l.ip.is_ipv6(),
                local_ip: ip_bytes(l.ip),
                local_port: l.port,
                remote_ip: [0; 16],
                remote_port: 0,
                state: tcp_state::LISTEN,
                tx_queue: 0,
                rx_queue: queued,
                timer: 0,
                when: 0,
                uid: l.uid,
                inode: l.ident.ino,
                refcnt: 1 + u32::from(queued > 0),
                ptr: l.ptr,
                tail: Some((100, 0, 0, 10, 0)),
            });
        }
        for p in &pairs {
            let sides = p.sides.lock();
            for i in 0..2 {
                if let Some(s) = sides[i].row(&sides[1 - i], now) {
                    out.push(s);
                }
            }
        }
        out
    }
}

/// Uma ponta de uma conexão, do jeito que a tabela do TCP a mostra.
#[derive(Debug)]
struct Side {
    local: (IpAddr, u16),
    remote: (IpAddr, u16),
    ino: u64,
    uid: u32,
    /// O pipe que esta ponta lê: os bytes ainda não lidos são o `rx_queue`.
    rx: Weak<Pipe>,
    /// A ponta do servidor só ganha inode no `accept`; até lá aparece com inode 0.
    accepted: bool,
    /// Quando esta ponta fechou (o `close` do último fd, ou o descarte sem `accept`).
    closed: Option<Instant>,
    /// A leitura desta ponta chegou ao EOF antes do `close`.
    saw_eof: bool,
    ptr: u32,
    /// O socket de time-wait que o kernel cria no lugar do órfão tem outro endereço.
    tw_ptr: u32,
}

impl Side {
    fn new(local: (IpAddr, u16), remote: (IpAddr, u16), ino: u64, uid: u32, rx: &Arc<Pipe>, accepted: bool) -> Side {
        Side { local, remote, ino, uid, rx: Arc::downgrade(rx), accepted, closed: None, saw_eof: false, ptr: hashed_ptr(), tw_ptr: hashed_ptr() }
    }

    fn base(&self, state: u8) -> TcpSock {
        TcpSock {
            v6: self.local.0.is_ipv6(),
            local_ip: ip_bytes(self.local.0),
            local_port: self.local.1,
            remote_ip: ip_bytes(self.remote.0),
            remote_port: self.remote.1,
            state,
            tx_queue: 0,
            rx_queue: 0,
            timer: 0,
            when: 0,
            uid: 0,
            inode: 0,
            refcnt: 1,
            ptr: self.ptr,
            tail: None,
        }
    }

    /// Socket de time-wait (o FIN_WAIT2 órfão ou o TIME_WAIT): timer 3 e o resto do prazo em 1/100 s.
    fn timewait(&self, state: u8, since: Instant, now: Instant) -> Option<TcpSock> {
        let left = TIMEWAIT_LEN.checked_sub(now.saturating_duration_since(since)).filter(|d| !d.is_zero())?;
        let mut s = self.base(state);
        s.timer = 3;
        s.when = (left.as_millis() / 10) as u64;
        s.refcnt = 3;
        s.ptr = self.tw_ptr;
        Some(s)
    }

    /// A linha desta ponta, ou nada se ela já saiu da tabela. Quem fecha primeiro fica órfão no
    /// FIN_WAIT2 enquanto o outro lado está no CLOSE_WAIT; quando o outro fecha, o primeiro passa ao
    /// TIME_WAIT e o segundo sai (LAST_ACK e CLOSED no loopback são instantâneos).
    fn row(&self, other: &Side, now: Instant) -> Option<TcpSock> {
        let unread = self.rx.upgrade().map_or(0, |p| p.pending()) as u32;
        match (self.closed, other.closed) {
            (None, peer) => {
                let mut s = self.base(if peer.is_some() { tcp_state::CLOSE_WAIT } else { tcp_state::ESTABLISHED });
                // O FIN do outro lado conta como um byte na fila de recepção.
                s.rx_queue = unread + u32::from(peer.is_some());
                s.uid = self.uid;
                s.inode = if self.accepted { self.ino } else { 0 };
                let got = s.rx_queue > 0;
                s.tail = Some((20, if got { 4 } else { 0 }, if unread > 0 { 30 } else { 0 }, 10, -1));
                Some(s)
            }
            (Some(mine), None) => self.timewait(tcp_state::FIN_WAIT2, mine, now),
            (Some(mine), Some(theirs)) => {
                // O primeiro a fechar passa pelo TIME_WAIT, se o FIN_WAIT2 dele não venceu antes. O
                // segundo sai pelo LAST_ACK, a menos que tenha fechado sem ler o EOF: aí os dois FIN
                // se cruzaram (fechamento simultâneo, CLOSING) e ele também fica no TIME_WAIT.
                let first = mine <= theirs && theirs.saturating_duration_since(mine) < TIMEWAIT_LEN;
                if first {
                    self.timewait(tcp_state::TIME_WAIT, theirs, now)
                } else if mine > theirs && !self.saw_eof && mine.saturating_duration_since(theirs) < TIMEWAIT_LEN {
                    self.timewait(tcp_state::TIME_WAIT, mine, now)
                } else {
                    None
                }
            }
        }
    }
}

/// As duas pontas de uma conexão (0 é o cliente, 1 o servidor).
#[derive(Debug)]
pub(crate) struct Pair {
    sides: Mutex<[Side; 2]>,
}

impl Pair {
    /// Alguma ponta ainda aberta tem `port` como porta local.
    fn uses(&self, port: u16) -> bool {
        self.sides.lock().iter().any(|s| s.closed.is_none() && s.local.1 == port)
    }

    /// As duas pontas fecharam e nenhuma aparece mais na tabela.
    fn gone(&self, now: Instant) -> bool {
        let s = self.sides.lock();
        s[0].closed.is_some() && s[1].closed.is_some() && s[0].row(&s[1], now).is_none() && s[1].row(&s[0], now).is_none()
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
    /// O endereço do `bind` (`0.0.0.0`, `127.0.0.1`, `::`...).
    pub ip: IpAddr,
    uid: u32,
    ptr: u32,
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
            c.pair.0.sides.lock()[c.pair.1].accepted = true;
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
    /// O par desta conexão na tabela do TCP e qual ponta esta é.
    pair: (Arc<Pair>, usize),
}

impl Drop for Conn {
    fn drop(&mut self) {
        let unread = self.rx.lock().as_ref().is_some_and(|e| e.pipe.pending() > 0);
        if unread {
            let _ = self.reset_peer.compare_exchange(RESET_NONE, RESET_PENDING, Ordering::SeqCst, Ordering::SeqCst);
        }
        self.pair.0.sides.lock()[self.pair.1].closed.get_or_insert_with(Instant::now);
        // As pontas caem depois daqui: o outro lado acorda com a flag já posta.
    }
}

impl Conn {
    fn new(rx: PipeEnd, tx: PipeEnd, local: u16, peer: u16, ident: Arc<Pipe>, reset: (Arc<AtomicU8>, Arc<AtomicU8>), pair: (Arc<Pair>, usize)) -> Conn {
        Conn { rx: Mutex::new(Some(rx)), tx: Mutex::new(Some(tx)), local, peer, ident, reset_me: reset.0, reset_peer: reset.1, pair }
    }

    /// A leitura desta ponta já devolveu o EOF: o FIN do outro lado foi consumido.
    pub(crate) fn saw_eof(&self) {
        self.pair.0.sides.lock()[self.pair.1].saw_eof = true;
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
