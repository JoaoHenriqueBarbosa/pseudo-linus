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

use std::collections::VecDeque;
use std::io;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use sysabi::{Errno, PollEvents, Stat};
use vfs::procfs::{TcpSock, tcp_state};

use crate::park::{Parker, WaitList};
use crate::pipe::{Pipe, PipeEnd, Try, WriteError};

const EPHEMERAL_LOW: u16 = 32768;
const EPHEMERAL_HIGH: u16 = 60999;
/// `st_dev` do sockfs (0:9 no Debian 13, como o `fstat` de um socket do oráculo mostra).
const SOCKFS_DEV: u64 = 9;
/// `TCP_TIMEWAIT_LEN` e o `tcp_fin_timeout` padrão: o FIN_WAIT2 órfão e o TIME_WAIT duram 60 s.
const TIMEWAIT_LEN: Duration = Duration::from_secs(60);

/// O `%pK` de um socket: o ponteiro com hash, 32 bits aleatórios por objeto.
pub(crate) fn hashed_ptr() -> u32 {
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

/// Dois endereços se encontram: o curinga (`0.0.0.0`, `::`) casa com qualquer um e dois específicos só
/// se são o mesmo (um IPv4 dentro de IPv6 conta como o IPv4). Serve ao `bind` (o que escuta disputa a
/// porta com quem já escuta nela) e ao `connect` (quem escuta atende o destino).
fn addr_meets(a: IpAddr, b: IpAddr) -> bool {
    a.is_unspecified() || b.is_unspecified() || a.to_canonical() == b.to_canonical()
}

/// As portas em escuta de um sandbox.
#[derive(Debug)]
pub(crate) struct Ports {
    st: Mutex<PortsState>,
}

/// Um socket em escuta registrado na tabela; a fraca não segura o socket vivo.
#[derive(Debug)]
struct Bound {
    ip: IpAddr,
    port: u16,
    listener: Weak<Listener>,
}

impl Bound {
    fn live(&self) -> bool {
        self.listener.strong_count() > 0
    }
}

#[derive(Debug)]
struct PortsState {
    listeners: Vec<Bound>,
    /// Os sockets dos serviços do host: ninguém no sandbox os segura, então a tabela segura.
    services: Vec<Arc<Listener>>,
    /// As conexões, da mais antiga à mais nova, até as duas pontas saírem do TIME_WAIT.
    pairs: Vec<Arc<Pair>>,
}

impl Default for Ports {
    fn default() -> Self {
        Ports { st: Mutex::new(PortsState { listeners: Vec::new(), services: Vec::new(), pairs: Vec::new() }) }
    }
}

impl Ports {
    /// Uma porta efêmera livre, como o Linux 6.12 escolhe: começo sorteado no `ip_local_port_range` e
    /// passo 2, com portas pares para o `connect` (`__inet_hash_connect`) e ímpares para o `bind` na
    /// porta 0 (`inet_csk_find_open_port`). Livre é sem ninguém escutando nem conexão viva usando.
    fn ephemeral(st: &mut PortsState, odd: bool) -> Result<u16, Errno> {
        let range = u32::from(EPHEMERAL_HIGH - EPHEMERAL_LOW) + 1;
        let start = (hashed_ptr() % range) & !1;
        let used = |st: &PortsState, port: u16| st.listeners.iter().any(|b| b.port == port && b.live()) || st.pairs.iter().any(|p| p.uses(port));
        for i in 0..range.div_ceil(2) {
            let off = (start + 2 * i) % (range & !1) + u32::from(odd);
            let port = EPHEMERAL_LOW + off as u16;
            if port <= EPHEMERAL_HIGH && !used(st, port) {
                return Ok(port);
            }
        }
        Err(Errno::EADDRINUSE)
    }

    /// `bind` em `ip` mais `listen`: porta 0 escolhe uma efêmera. EADDRINUSE se já há alguém escutando nela
    /// num endereço que se encontra com `ip`.
    pub(crate) fn listen(self: &Arc<Self>, ip: IpAddr, port: u16, backlog: u32, ident: Arc<Pipe>) -> Result<Arc<Listener>, Errno> {
        let mut st = self.st.lock();
        let port = if port == 0 { Self::ephemeral(&mut st, true)? } else { port };
        if st.listeners.iter().any(|b| b.port == port && b.live() && addr_meets(b.ip, ip)) {
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
        st.listeners.push(Bound { ip, port, listener: Arc::downgrade(&l) });
        Ok(l)
    }

    /// Um serviço do host em `ip:port`: um socket em escuta como qualquer outro (aparece no
    /// `/proc/net/tcp`, recusa a porta a quem tenta escutar nela), mas as conexões aceitas vão para
    /// `handler`, que roda numa thread do host por conexão. O aceitador é uma thread do host criada
    /// aqui, de quem chama, e não de uma thread do sandbox: assim as threads dos serviços não herdam o
    /// isolamento (landlock, seccomp) que as dos processos carregam.
    pub(crate) fn host_service(self: &Arc<Self>, ip: IpAddr, port: u16, ident: Arc<Pipe>, handler: Arc<HostHandler>) -> Result<(), Errno> {
        let listener = self.listen(ip, port, 128, ident)?;
        let weak = Arc::downgrade(&listener);
        self.st.lock().services.push(listener);
        std::thread::Builder::new().name("host-service".into()).spawn(move || accept_loop(&weak, &handler)).map_err(|_| Errno::EAGAIN)?;
        Ok(())
    }

    /// `connect` a `dst:port` (um endereço de loopback). As duas pontas nascem prontas: a do servidor
    /// vai pra fila do socket que escuta (o mais específico que atende `dst`). ECONNREFUSED sem ninguém
    /// escutando ou com a fila cheia.
    pub(crate) fn connect(&self, dst: IpAddr, port: u16, mk_pipe: impl Fn() -> Arc<Pipe>) -> Result<Conn, Errno> {
        let (listener, local) = {
            let mut st = self.st.lock();
            let l = st
                .listeners
                .iter()
                .filter(|b| b.port == port && addr_meets(b.ip, dst))
                .min_by_key(|b| b.ip.is_unspecified())
                .and_then(|b| b.listener.upgrade());
            let local = if l.is_some() { Self::ephemeral(&mut st, false) } else { Err(Errno::ECONNREFUSED) };
            (l, local)
        };
        let listener = listener.ok_or(Errno::ECONNREFUSED)?;
        let local = local?;
        let up = mk_pipe();
        let down = mk_pipe();
        // O servidor vê o par na família do socket que escuta: IPv4 num socket IPv6 vira `::ffff:`.
        let server_ip = if listener.ip.is_ipv6() { mapped(dst) } else { dst };
        let client = Side::new((dst, local), (dst, port), down.ino, down.stat(None).uid, &down, true);
        let server = Side::new((server_ip, port), (server_ip, local), up.ino, listener.uid, &up, false);
        let pair = Arc::new(Pair { sides: Mutex::new([client, server]) });
        let (client_reset, server_reset) = (Arc::new(AtomicU8::new(RESET_NONE)), Arc::new(AtomicU8::new(RESET_NONE)));
        let client = Conn::new(down.attach(true, false), up.attach(false, true), local, port, down.clone(), (client_reset.clone(), server_reset.clone()), Some((pair.clone(), 0)));
        let server = Conn::new(up.attach(true, false), down.attach(false, true), port, local, up, (server_reset, client_reset), Some((pair.clone(), 1)));
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
            let ls: Vec<Arc<Listener>> = st.listeners.iter().filter_map(|b| b.listener.upgrade()).collect();
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
    /// Quando esta ponta mandou o FIN: o `shutdown(SHUT_WR)` ou o `close`, o que vier antes.
    fin: Option<Instant>,
    ptr: u32,
    /// O socket de time-wait que o kernel cria no lugar do órfão tem outro endereço.
    tw_ptr: u32,
}

impl Side {
    fn new(local: (IpAddr, u16), remote: (IpAddr, u16), ino: u64, uid: u32, rx: &Arc<Pipe>, accepted: bool) -> Side {
        Side { local, remote, ino, uid, rx: Arc::downgrade(rx), accepted, closed: None, fin: None, ptr: hashed_ptr(), tw_ptr: hashed_ptr() }
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

    /// A linha desta ponta, ou nada se ela já saiu da tabela. Quem manda o FIN primeiro fica no
    /// FIN_WAIT2 (órfão, no formato de time-wait, se já fechou) enquanto o outro lado está no
    /// CLOSE_WAIT; quando o FIN do outro chega, o primeiro passa ao TIME_WAIT e o segundo sai (LAST_ACK
    /// e CLOSED no loopback são instantâneos). Um socket aberto que entra no TIME_WAIT também sai da
    /// tabela: no lugar dele fica o de time-wait.
    fn row(&self, other: &Side, now: Instant) -> Option<TcpSock> {
        let unread = self.rx.upgrade().map_or(0, |p| p.pending()) as u32;
        match (self.fin, other.fin) {
            (Some(mine), Some(theirs)) => {
                // O segundo FIN fecha a conexão: o primeiro a mandar passa pelo TIME_WAIT, se o
                // FIN_WAIT2 dele não venceu antes.
                if mine <= theirs && theirs.saturating_duration_since(mine) < TIMEWAIT_LEN {
                    self.timewait(tcp_state::TIME_WAIT, theirs, now)
                } else {
                    None
                }
            }
            (Some(mine), None) => match self.closed {
                Some(closed) => self.timewait(tcp_state::FIN_WAIT2, closed, now),
                None => {
                    let _ = mine;
                    Some(self.full(tcp_state::FIN_WAIT2, unread, false))
                }
            },
            (None, peer) => {
                let state = if peer.is_some() { tcp_state::CLOSE_WAIT } else { tcp_state::ESTABLISHED };
                Some(self.full(state, unread, peer.is_some()))
            }
        }
    }

    /// A linha de um socket completo (com inode e as colunas do fim).
    fn full(&self, state: u8, unread: u32, peer_fin: bool) -> TcpSock {
        let mut s = self.base(state);
        // O FIN do outro lado conta como um byte na fila de recepção.
        s.rx_queue = unread + u32::from(peer_fin);
        s.uid = self.uid;
        s.inode = if self.accepted { self.ino } else { 0 };
        let got = s.rx_queue > 0;
        s.tail = Some((20, if got { 4 } else { 0 }, if unread > 0 { 30 } else { 0 }, 10, -1));
        s
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
            if let Some((pair, i)) = &c.pair {
                pair.sides.lock()[*i].accepted = true;
            }
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
            ports.st.lock().listeners.retain(Bound::live);
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
    /// O par desta conexão na tabela do TCP e qual ponta esta é; `None` no par de um socket Unix.
    pair: Option<(Arc<Pair>, usize)>,
}

impl Drop for Conn {
    fn drop(&mut self) {
        let unread = self.rx.lock().as_ref().is_some_and(|e| e.pipe.pending() > 0);
        if unread {
            let _ = self.reset_peer.compare_exchange(RESET_NONE, RESET_PENDING, Ordering::SeqCst, Ordering::SeqCst);
        }
        if let Some((pair, i)) = &self.pair {
            let mut sides = pair.sides.lock();
            let now = Instant::now();
            sides[*i].closed.get_or_insert(now);
            sides[*i].fin.get_or_insert(now);
        }
        // As pontas caem depois daqui: o outro lado acorda com a flag já posta.
    }
}

/// As duas pontas de uma conexão sem lugar na tabela do TCP (um par de sockets Unix). Como no TCP,
/// fechar com dados não lidos faz o outro lado receber ECONNRESET.
pub(crate) fn conn_pair(mk_pipe: impl Fn() -> Arc<Pipe>) -> (Conn, Conn) {
    let (up, down) = (mk_pipe(), mk_pipe());
    let (ra, rb) = (Arc::new(AtomicU8::new(RESET_NONE)), Arc::new(AtomicU8::new(RESET_NONE)));
    let a = Conn::new(down.attach(true, false), up.attach(false, true), 0, 0, down.clone(), (ra.clone(), rb.clone()), None);
    let b = Conn::new(up.attach(true, false), down.attach(false, true), 0, 0, up, (rb, ra), None);
    (a, b)
}

impl Conn {
    fn new(rx: PipeEnd, tx: PipeEnd, local: u16, peer: u16, ident: Arc<Pipe>, reset: (Arc<AtomicU8>, Arc<AtomicU8>), pair: Option<(Arc<Pair>, usize)>) -> Conn {
        Conn { rx: Mutex::new(Some(rx)), tx: Mutex::new(Some(tx)), local, peer, ident, reset_me: reset.0, reset_peer: reset.1, pair }
    }

    /// A outra ponta ainda existe (não foi fechada nem descartada).
    pub(crate) fn peer_open(&self) -> bool {
        Arc::strong_count(&self.reset_me) > 1
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
        if write && let Some((pair, i)) = &self.pair {
            pair.sides.lock()[*i].fin.get_or_insert_with(Instant::now);
        }
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

/// O que um serviço do host faz com cada conexão aceita.
pub(crate) type HostHandler = dyn Fn(HostStream) + Send + Sync;

/// De quanto em quanto o aceitador de um serviço confere se o socket ainda existe.
const ACCEPT_RECHECK: Duration = Duration::from_millis(500);

/// O laço do aceitador de um serviço do host: tira as conexões da fila do socket em escuta e entrega
/// cada uma ao `handler` numa thread própria. Sai quando o socket deixa de existir (o sandbox acabou).
fn accept_loop(listener: &Weak<Listener>, handler: &Arc<HostHandler>) {
    let parker = Parker::new();
    loop {
        let Some(l) = listener.upgrade() else { return };
        let r = l.try_accept(false, &parker);
        drop(l);
        match r {
            Try::Ready(Ok(conn)) => {
                let handler = handler.clone();
                let _ = std::thread::Builder::new().name("host-service-conn".into()).spawn(move || handler(HostStream::new(conn)));
            }
            Try::Ready(Err(_)) => return,
            Try::Pending => {
                parker.park_until(Instant::now() + ACCEPT_RECHECK);
            }
        }
    }
}

/// O lado do servidor de uma conexão aceita por um serviço do host, com leitura e escrita bloqueantes
/// sobre os pipes dela. Soltar o valor fecha a conexão: o convidado vê o FIN (e o RST, se sobrou dado
/// que ele mandou e ninguém leu), como no fechamento de um socket.
#[derive(Debug)]
pub struct HostStream {
    conn: Conn,
    parker: Arc<Parker>,
}

impl HostStream {
    fn new(conn: Conn) -> HostStream {
        HostStream { conn, parker: Parker::new() }
    }
}

impl io::Read for HostStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.conn.take_reset() {
            ResetState::Pending => return Err(io::ErrorKind::ConnectionReset.into()),
            ResetState::Done => return Ok(0),
            ResetState::None => {}
        }
        let Some(pipe) = self.conn.rx() else { return Ok(0) };
        loop {
            match pipe.try_read(buf, false, &self.parker) {
                // EOF vindo de um fechamento com RST: o erro vem antes do EOF.
                Try::Ready(Ok(0)) if matches!(self.conn.take_reset(), ResetState::Pending) => return Err(io::ErrorKind::ConnectionReset.into()),
                Try::Ready(Ok(n)) => return Ok(n),
                Try::Ready(Err(e)) => return Err(e.into()),
                Try::Pending => self.parker.park(),
            }
        }
    }
}

impl io::Write for HostStream {
    /// Como a escrita de um processo: o primeiro `write` para um par que já fechou conta como feito e o
    /// seguinte dá ECONNRESET.
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let reset = self.conn.take_reset();
        if matches!(reset, ResetState::Pending) {
            return Err(io::ErrorKind::ConnectionReset.into());
        }
        let none = matches!(reset, ResetState::None);
        if none && self.conn.write_to_closed_peer() {
            return Ok(data.len());
        }
        let Some(pipe) = self.conn.tx().filter(|_| none) else { return Err(io::ErrorKind::BrokenPipe.into()) };
        if data.is_empty() {
            return Ok(0);
        }
        let mut done = 0usize;
        loop {
            match pipe.try_write(data, &mut done, false, &self.parker) {
                Try::Ready(Ok(n)) => return Ok(n),
                Try::Ready(Err(WriteError::BrokenPipe { written: 0 })) => return Err(io::ErrorKind::BrokenPipe.into()),
                Try::Ready(Err(WriteError::BrokenPipe { written })) => return Ok(written),
                // Sem `nonblock` o `Again` não sai; esperar é o que resta se algum dia sair.
                Try::Ready(Err(WriteError::Again)) | Try::Pending => self.parker.park(),
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// O endereço de `name` no texto de um `/etc/hosts`, como o `files` do `nsswitch` do glibc: a primeira
/// linha cujo nome ou alias é `name` (sem diferenciar maiúsculas), ignorando comentários e linhas cujo
/// endereço não é válido. Vale IPv4 e IPv6.
pub(crate) fn lookup_hosts(hosts: &[u8], name: &[u8]) -> Option<IpAddr> {
    hosts.split(|b| *b == b'\n').find_map(|line| {
        let line = line.split(|b| *b == b'#').next().unwrap_or_default();
        let mut words = line.split(u8::is_ascii_whitespace).filter(|w| !w.is_empty());
        let ip = std::str::from_utf8(words.next()?).ok()?.parse::<IpAddr>().ok()?;
        words.any(|w| w.eq_ignore_ascii_case(name)).then_some(ip)
    })
}
