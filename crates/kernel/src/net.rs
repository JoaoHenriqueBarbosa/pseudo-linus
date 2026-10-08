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
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU8, AtomicUsize, Ordering};
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
/// `net.ipv4.tcp_syn_retries`: quantas vezes o SYN é retransmitido antes do `connect` falhar com ETIMEDOUT.
const SYN_RETRIES: u32 = 6;
/// `net.core.somaxconn` do Linux 6.12: o teto do `backlog` do `listen`.
const SOMAXCONN: u32 = 4096;

/// Quem cria os pipes (um por sentido) de uma conexão: o `connect` retransmitido roda numa thread do host, que
/// não tem a tarefa que o chamou, então leva o que ela usaria.
pub(crate) type MkPipe = Arc<dyn Fn() -> Arc<Pipe> + Send + Sync>;

/// Quando sai a transmissão `k` do SYN, contada do primeiro (`k = 0`): o RTO inicial é de 1 s e dobra a cada
/// retransmissão, então saem em 1, 3, 7, 15, 31 e 63 s, e o `ETIMEDOUT` vem em `k = SYN_RETRIES + 1`, 127 s.
fn syn_at(start: Instant, k: u32) -> Instant {
    start + Duration::from_secs((1u64 << k) - 1)
}

/// O resultado de uma tentativa de `connect`.
pub(crate) enum Connect {
    Established(Conn),
    /// A fila de aceite do ouvinte está cheia (`tcp_conn_request` descarta o SYN sem responder): o cliente
    /// fica em SYN_SENT e retransmite. `local` é a porta que o `connect` escolheu.
    Dropped { local: u16 },
}

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
    /// O `SO_REUSEADDR` do socket no momento do `bind`.
    reuse: bool,
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
    /// Os sockets que escolheram a porta de um `connect` ainda sem resposta (SYN_SENT); a porta fica deles.
    syns: Vec<Weak<Listener>>,
}

impl Default for Ports {
    fn default() -> Self {
        Ports { st: Mutex::new(PortsState { listeners: Vec::new(), services: Vec::new(), pairs: Vec::new(), syns: Vec::new() }) }
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
            st.listeners.iter().any(|b| b.port == port && b.live())
                || st.pairs.iter().any(|p| p.uses(port))
                || st.syns.iter().filter_map(Weak::upgrade).any(|l| l.syn_port.load(Ordering::SeqCst) == port)
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

    /// `bind` em `ip` mais `listen`: porta 0 escolhe uma efêmera. EADDRINUSE se já há alguém ligado ou
    /// escutando nela num endereço que se encontra com `ip`.
    pub(crate) fn listen(self: &Arc<Self>, ip: IpAddr, port: u16, backlog: u32, ident: Arc<Pipe>) -> Result<Arc<Listener>, Errno> {
        let l = self.bind(ip, port, false, ident)?;
        l.start_listening(backlog);
        Ok(l)
    }

    /// `bind` sem `listen`: reserva a porta (a 0 sorteia uma efêmera ímpar) até o socket fechar. O socket
    /// ligado não atende `connect` nem aparece no `/proc/net/tcp`; `start_listening` o põe em escuta.
    ///
    /// Como o `inet_csk_bind_conflict` do Linux: um socket em escuta na porta sempre recusa o `bind`; um
    /// ligado sem escuta só deixa passar se os dois têm `SO_REUSEADDR` (`reuse`). Sem `reuse`, uma conexão
    /// aberta ou em TIME_WAIT que use a porta também recusa; com ele, só sobra quem escuta.
    pub(crate) fn bind(self: &Arc<Self>, ip: IpAddr, port: u16, reuse: bool, ident: Arc<Pipe>) -> Result<Arc<Listener>, Errno> {
        let l = self.socket(ip.is_ipv6(), ident);
        self.bind_socket(&l, ip, port, reuse)?;
        Ok(l)
    }

    /// `socket(AF_INET ou AF_INET6, SOCK_STREAM)`: um socket sem endereço nem porta, que não consta da
    /// tabela até o `bind` (ou o `connect`, que escolhe a porta efêmera).
    pub(crate) fn socket(self: &Arc<Self>, v6: bool, ident: Arc<Pipe>) -> Arc<Listener> {
        let any = if v6 { IpAddr::V6(std::net::Ipv6Addr::UNSPECIFIED) } else { IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED) };
        Arc::new(Listener {
            v6,
            addr: Mutex::new((any, 0)),
            uid: ident.stat(None).uid,
            ptr: hashed_ptr(),
            ident,
            backlog: AtomicUsize::new(0),
            listening: AtomicBool::new(false),
            syn_port: AtomicU16::new(0),
            ports: Arc::downgrade(self),
            st: Mutex::new(ListenState::default()),
            conn: Mutex::new(None),
        })
    }

    /// `bind` de um socket já criado: devolve a porta. EINVAL se ele já tem endereço (ou já conectou), como o
    /// `inet_bind`. Os conflitos são os de [`Ports::bind`].
    pub(crate) fn bind_socket(self: &Arc<Self>, l: &Arc<Listener>, ip: IpAddr, port: u16, reuse: bool) -> Result<u16, Errno> {
        if l.conn().is_some() || l.syn_sent() {
            return Err(Errno::EINVAL);
        }
        let mut st = self.st.lock();
        let mut addr = l.addr.lock();
        if addr.1 != 0 {
            return Err(Errno::EINVAL);
        }
        let port = if port == 0 { Self::ephemeral(&mut st, true)? } else { port };
        let conflict = |b: &Bound| {
            b.port == port && addr_meets(b.ip, ip) && b.listener.upgrade().is_some_and(|l| l.is_listening() || !(reuse && b.reuse))
        };
        if st.listeners.iter().any(conflict) {
            return Err(Errno::EADDRINUSE);
        }
        if !reuse {
            let now = Instant::now();
            if st.pairs.iter().any(|p| p.occupies(port, now)) {
                return Err(Errno::EADDRINUSE);
            }
        }
        st.listeners.push(Bound { ip, port, reuse, listener: Arc::downgrade(l) });
        *addr = (ip, port);
        Ok(port)
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

    /// `connect` de `from` a `dst:port` (um endereço de loopback). As duas pontas nascem prontas: a do
    /// servidor vai pra fila do socket que escuta (o mais específico que atende `dst`). ECONNREFUSED sem
    /// ninguém escutando; com a fila de aceite cheia o SYN é descartado e o resultado é [`Connect::Dropped`].
    ///
    /// A ponta local é a reserva de `from`: a porta é a do `bind` (ou `local`, a que um SYN anterior já
    /// escolheu, ou uma efêmera par), o endereço é o do `bind` (ou o destino, se o `bind` foi no curinga) e o
    /// inode é o do próprio socket.
    pub(crate) fn connect(&self, dst: IpAddr, port: u16, from: &Listener, local: Option<u16>, mk_pipe: impl Fn() -> Arc<Pipe>) -> Result<Connect, Errno> {
        let (listener, local) = {
            let mut st = self.st.lock();
            let l = st
                .listeners
                .iter()
                .filter(|b| b.port == port && addr_meets(b.ip, dst))
                .filter_map(|b| b.listener.upgrade())
                .filter(|l| l.listening.load(Ordering::SeqCst))
                .min_by_key(|l| l.ip().is_unspecified());
            let l = l.ok_or(Errno::ECONNREFUSED)?;
            let local = match local {
                Some(p) => p,
                // Um socket ainda sem porta ganha uma efêmera no `connect` (autobind), como o `connect` do Linux.
                None if from.port() != 0 => from.port(),
                None => Self::ephemeral(&mut st, false)?,
            };
            from.syn_port.store(local, Ordering::SeqCst);
            (l, local)
        };
        // Sem vaga na fila de aceite o SYN morre antes de gastar inode com a conexão.
        if !listener.has_room()? {
            return Ok(Connect::Dropped { local });
        }
        let up = mk_pipe();
        let down = from.ident.clone();
        let src = if from.ip().is_unspecified() { dst } else { from.ip() };
        // O servidor vê o par na família do socket que escuta: IPv4 num socket IPv6 vira `::ffff:`.
        let seen = |ip: IpAddr| if listener.ip().is_ipv6() { mapped(ip) } else { ip };
        let client = Side::new((src, local), (dst, port), down.ino, down.stat(None).uid, &down, true);
        let server = Side::new((seen(dst), port), (seen(src), local), up.ino, listener.uid, &up, false);
        let pair = Arc::new(Pair { sides: Mutex::new([client, server]) });
        let (client_reset, server_reset) = (Arc::new(AtomicU8::new(RESET_NONE)), Arc::new(AtomicU8::new(RESET_NONE)));
        let client = Conn::new(down.attach(true, false), up.attach(false, true), local, port, down.clone(), (client_reset.clone(), server_reset.clone()), Some((pair.clone(), 0)));
        let server = Conn::new(up.attach(true, false), down.attach(false, true), port, local, up, (server_reset, client_reset), Some((pair.clone(), 1)));
        let wake = {
            let mut s = listener.st.lock();
            if !listener.room_in(&s)? {
                return Ok(Connect::Dropped { local });
            }
            s.queue.push_back(server);
            s.wait.take_key(crate::park::key::READ)
        };
        wake.run();
        self.st.lock().pairs.push(pair);
        Ok(Connect::Established(client))
    }

    /// A tabela do `/proc/net/tcp` e do `tcp6`: os sockets em escuta, depois cada ponta das conexões.
    /// Conexões cujas duas pontas já saíram do TIME_WAIT somem da tabela.
    pub(crate) fn tcp_socks(&self) -> Vec<TcpSock> {
        let now = Instant::now();
        let (mut listeners, pairs, syns) = {
            let mut st = self.st.lock();
            st.pairs.retain(|p| !p.gone(now));
            st.syns.retain(|w| w.strong_count() > 0);
            let ls: Vec<Arc<Listener>> = st.listeners.iter().filter_map(|b| b.listener.upgrade()).filter(|l| l.listening.load(Ordering::SeqCst)).collect();
            (ls, st.pairs.clone(), st.syns.iter().filter_map(Weak::upgrade).collect::<Vec<_>>())
        };
        listeners.sort_by_key(|l| l.port());
        let mut out = Vec::new();
        for l in &listeners {
            let queued = l.st.lock().queue.len() as u32;
            out.push(TcpSock {
                v6: l.ip().is_ipv6(),
                local_ip: ip_bytes(l.ip()),
                local_port: l.port(),
                remote_ip: [0; 16],
                remote_port: 0,
                state: tcp_state::LISTEN,
                tx_queue: 0,
                rx_queue: queued,
                timer: 0,
                when: 0,
                retrans: 0,
                uid: l.uid,
                inode: l.ident.ino,
                refcnt: 1 + u32::from(queued > 0),
                ptr: l.ptr,
                tail: Some((100, 0, 0, 10, 0)),
            });
        }
        out.extend(syns.iter().filter_map(|l| l.syn_row(now)));
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
            retrans: 0,
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

    /// Alguma ponta com `port` como porta local ainda aparece na tabela do TCP (aberta ou em time-wait):
    /// é ela que o `bind` sem `SO_REUSEADDR` não deixa tomar.
    fn occupies(&self, port: u16, now: Instant) -> bool {
        let s = self.sides.lock();
        (0..2).any(|i| s[i].local.1 == port && s[i].row(&s[1 - i], now).is_some())
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
    /// O `connect` deste socket, quando o SYN foi descartado pela fila cheia do ouvinte.
    syn: SynState,
}

/// O `connect` pendente de um socket (SYN_SENT).
#[derive(Debug, Default)]
enum SynState {
    #[default]
    Idle,
    Sent(Syn),
    /// O SYN retransmitido foi aceito: a conexão (a mesma que o socket guarda).
    Done(Arc<Conn>),
    /// A tentativa acabou sem conexão (ETIMEDOUT, ou ECONNREFUSED se o ouvinte sumiu): o erro espera quem o lê.
    Failed(Errno),
}

#[derive(Debug)]
struct Syn {
    src: IpAddr,
    dst: IpAddr,
    port: u16,
    local: u16,
    start: Instant,
    /// As retransmissões já feitas (`icsk_retransmits`).
    retries: u32,
    /// Acorda a thread das retransmissões para ela sair (o socket fechou).
    ticker: Arc<Parker>,
}

/// A thread das retransmissões de um SYN: dorme até o instante de cada uma e a tenta; acaba quando o `connect`
/// termina, o socket some ou o `ticker` a acorda.
fn syn_loop(sock: &Weak<Listener>, ticker: &Parker, start: Instant, mk_pipe: &MkPipe) {
    for k in 1..=SYN_RETRIES + 1 {
        if ticker.park_until(syn_at(start, k)) {
            return;
        }
        let Some(l) = sock.upgrade() else { return };
        if l.retransmit(k, mk_pipe) {
            return;
        }
    }
}

/// Um socket TCP: criado sem endereço, ligado (`bind`), em escuta ou conectado.
#[derive(Debug)]
pub(crate) struct Listener {
    /// A família do socket (`AF_INET6` se verdadeiro).
    v6: bool,
    /// O endereço do `bind` (`0.0.0.0`, `127.0.0.1`, `::`...) e a porta; porta 0 é sem `bind`.
    addr: Mutex<(IpAddr, u16)>,
    uid: u32,
    ptr: u32,
    pub ident: Arc<Pipe>,
    backlog: AtomicUsize,
    /// Depois do `listen`; antes disso o socket só segura a porta.
    listening: AtomicBool,
    /// A porta que um `connect` pendente escolheu (0 sem `connect` pendente): fica reservada até ele acabar.
    syn_port: AtomicU16,
    ports: Weak<Ports>,
    st: Mutex<ListenState>,
    /// A conexão que o `connect` num socket ligado criou. O socket é um objeto só, compartilhado por
    /// todos os fds vindos de `dup`, `dup2` ou herdados num spawn: depois do `connect`, todos eles leem,
    /// escrevem e dão poll nesta conexão, como no `struct socket` do Linux.
    conn: Mutex<Option<Arc<Conn>>>,
}

impl Listener {
    /// A porta do `bind` (0 se o socket ainda não tem).
    pub(crate) fn port(&self) -> u16 {
        self.addr.lock().1
    }

    /// O endereço do `bind` (o curinga da família se o socket ainda não tem).
    pub(crate) fn ip(&self) -> IpAddr {
        self.addr.lock().0
    }

    /// O socket é `AF_INET6`.
    pub(crate) fn is_v6(&self) -> bool {
        self.v6
    }

    /// A conexão do socket, se o `connect` já aconteceu.
    pub(crate) fn conn(&self) -> Option<Arc<Conn>> {
        self.conn.lock().clone()
    }

    /// `connect` do socket a `dst:port`, com a porta e o inode do próprio socket. EISCONN se ele já escuta ou
    /// já conectou, EALREADY se um SYN anterior ainda espera resposta (`inet_stream_connect`). A trava da
    /// conexão fica presa durante o `connect`, então dois `connect` simultâneos no mesmo socket não criam duas
    /// conexões. `Some` é a conexão já estabelecida; `None` é o SYN descartado pela fila cheia do ouvinte: o
    /// socket fica em SYN_SENT e uma thread retransmite (1, 3, 7... s), quem espera acorda com [`Listener::try_syn`].
    pub(crate) fn connect(self: &Arc<Self>, ports: &Arc<Ports>, dst: IpAddr, port: u16, mk_pipe: MkPipe) -> Result<Option<Arc<Conn>>, Errno> {
        let mut slot = self.conn.lock();
        if slot.is_some() || self.is_listening() {
            return Err(Errno::EISCONN);
        }
        if self.syn_sent() {
            return Err(Errno::EALREADY);
        }
        match ports.connect(dst, port, self, None, &*mk_pipe) {
            Ok(Connect::Established(conn)) => {
                let conn = Arc::new(conn);
                *slot = Some(conn.clone());
                self.syn_port.store(0, Ordering::SeqCst);
                Ok(Some(conn))
            }
            Ok(Connect::Dropped { local }) => {
                self.begin_syn(ports, dst, port, local, mk_pipe)?;
                Ok(None)
            }
            Err(e) => {
                self.syn_port.store(0, Ordering::SeqCst);
                Err(e)
            }
        }
    }

    /// Põe o socket em SYN_SENT e dispara a thread das retransmissões.
    fn begin_syn(self: &Arc<Self>, ports: &Arc<Ports>, dst: IpAddr, port: u16, local: u16, mk_pipe: MkPipe) -> Result<(), Errno> {
        let (start, ticker, sock) = (Instant::now(), Parker::new(), Arc::downgrade(self));
        let src = if self.ip().is_unspecified() { dst } else { self.ip() };
        let thread = (sock.clone(), ticker.clone());
        std::thread::Builder::new()
            .name("syn-retransmit".into())
            .spawn(move || syn_loop(&thread.0, &thread.1, start, &mk_pipe))
            .map_err(|_| Errno::EAGAIN)?;
        self.st.lock().syn = SynState::Sent(Syn { src, dst, port, local, start, retries: 0, ticker });
        ports.st.lock().syns.push(sock);
        Ok(())
    }

    /// A retransmissão `k` do SYN (ou, passado o limite, a desistência). `true` quando o `connect` acabou.
    fn retransmit(&self, k: u32, mk_pipe: &MkPipe) -> bool {
        let Some(ports) = self.ports.upgrade() else { return true };
        let (dst, port, local) = match &self.st.lock().syn {
            SynState::Sent(s) => (s.dst, s.port, s.local),
            _ => return true,
        };
        let outcome = if k > SYN_RETRIES { Err(Errno::ETIMEDOUT) } else { ports.connect(dst, port, self, Some(local), &**mk_pipe) };
        match outcome {
            Ok(Connect::Dropped { .. }) => {
                if let SynState::Sent(s) = &mut self.st.lock().syn {
                    s.retries = k;
                }
                false
            }
            Ok(Connect::Established(conn)) => {
                let conn = Arc::new(conn);
                *self.conn.lock() = Some(conn.clone());
                self.end_syn(SynState::Done(conn));
                true
            }
            Err(e) => {
                self.end_syn(SynState::Failed(e));
                true
            }
        }
    }

    /// O `connect` pendente acabou: guarda o desfecho, solta a porta reservada e acorda quem espera.
    fn end_syn(&self, outcome: SynState) {
        let wake = {
            let mut s = self.st.lock();
            s.syn = outcome;
            s.wait.take()
        };
        self.syn_port.store(0, Ordering::SeqCst);
        wake.run();
    }

    /// O `connect` do socket está sem resposta (SYN_SENT).
    pub(crate) fn syn_sent(&self) -> bool {
        matches!(self.st.lock().syn, SynState::Sent(_))
    }

    /// Espera do `connect` bloqueante: a conexão quando estabeleceu, o erro se o SYN falhou, senão registra o parker.
    pub(crate) fn try_syn(&self, waiter: &Arc<Parker>) -> Try<Result<Arc<Conn>, Errno>> {
        let mut s = self.st.lock();
        let state = match &s.syn {
            SynState::Done(c) => Some(Ok(c.clone())),
            SynState::Failed(e) => Some(Err(*e)),
            SynState::Idle => Some(Err(Errno::ECONNABORTED)),
            SynState::Sent(_) => None,
        };
        let Some(done) = state else {
            s.wait.register(waiter);
            return Try::Pending;
        };
        if done.is_err() {
            s.syn = SynState::Idle;
        }
        s.wait.unregister(waiter);
        Try::Ready(done)
    }

    /// O erro de um `connect` que falhou sem ninguém esperando (o `sk_err` do `tcp_write_err`), uma vez só.
    pub(crate) fn take_syn_failure(&self) -> Option<Errno> {
        let mut s = self.st.lock();
        let SynState::Failed(e) = s.syn else { return None };
        s.syn = SynState::Idle;
        Some(e)
    }

    /// O endereço do socket para o `getsockname`: o do `connect` pendente (SYN_SENT) ou o do `bind`.
    pub(crate) fn name(&self) -> (IpAddr, u16) {
        match &self.st.lock().syn {
            SynState::Sent(s) => (s.src, s.local),
            _ => (self.ip(), self.port()),
        }
    }

    /// A linha do `/proc/net/tcp` de um socket em SYN_SENT: timer de retransmissão com o tempo que falta
    /// para a próxima, `retrnsmt` com as já feitas e o RTO dobrado a cada uma (até 120 s).
    fn syn_row(&self, now: Instant) -> Option<TcpSock> {
        let s = self.st.lock();
        let SynState::Sent(syn) = &s.syn else { return None };
        let left = syn_at(syn.start, syn.retries + 1).saturating_duration_since(now);
        Some(TcpSock {
            v6: syn.src.is_ipv6(),
            local_ip: ip_bytes(syn.src),
            local_port: syn.local,
            remote_ip: ip_bytes(syn.dst),
            remote_port: syn.port,
            state: tcp_state::SYN_SENT,
            tx_queue: 1,
            rx_queue: 0,
            timer: 1,
            when: (left.as_millis() / 10) as u64,
            retrans: syn.retries,
            uid: self.uid,
            inode: self.ident.ino,
            refcnt: 2,
            ptr: self.ptr,
            tail: Some(((100u32 << syn.retries).min(12000), 0, 0, 10, -1)),
        })
    }

    /// A fila de aceite tem vaga para mais uma conexão (`!sk_acceptq_is_full`). ECONNREFUSED se o socket não escuta.
    fn has_room(&self) -> Result<bool, Errno> {
        self.room_in(&self.st.lock())
    }

    /// [`Listener::has_room`] sob a trava da fila. O Linux compara com `>`: o `backlog` aceita `backlog + 1`
    /// conexões completas, então `listen(fd, 0)` ainda aceita uma.
    fn room_in(&self, s: &ListenState) -> Result<bool, Errno> {
        if s.closed || !self.listening.load(Ordering::SeqCst) {
            return Err(Errno::ECONNREFUSED);
        }
        Ok(s.queue.len() <= self.backlog.load(Ordering::SeqCst))
    }

    /// O socket já passou pelo `listen`.
    pub(crate) fn is_listening(&self) -> bool {
        self.listening.load(Ordering::SeqCst)
    }

    /// `listen` num socket ligado (ou de novo num que já escuta, que só ajusta o `backlog`): passa a atender
    /// `connect`. O `backlog` fica limitado por `net.core.somaxconn`.
    pub(crate) fn start_listening(&self, backlog: u32) {
        self.backlog.store(backlog.min(SOMAXCONN) as usize, Ordering::SeqCst);
        self.listening.store(true, Ordering::SeqCst);
    }

    /// Tira a próxima conexão da fila; sem nenhuma, EAGAIN (`nonblock`) ou registra o parker. EINVAL
    /// num socket que não escuta (ainda, ou depois de `stop_listening`), como o `accept` do Linux; o teste é
    /// feito sob a trava da fila, a mesma em que `stop_listening` age, então quem espera não perde o aviso.
    pub(crate) fn try_accept(&self, nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<Conn, Errno>> {
        let mut s = self.st.lock();
        if !self.listening.load(Ordering::SeqCst) {
            s.wait.unregister(waiter);
            return Try::Ready(Err(Errno::EINVAL));
        }
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

    /// `shutdown(SHUT_RD)` num socket em escuta (`tcp_disconnect` do `inet_shutdown`): deixa de escutar, as
    /// conexões que ninguém aceitou fecham com RST (o cliente vê ECONNRESET) e quem espera no
    /// `accept` ou no poll acorda (o `accept` seguinte dá EINVAL). A porta do `bind` continua reservada.
    pub(crate) fn stop_listening(&self) {
        let wake = {
            let mut s = self.st.lock();
            self.listening.store(false, Ordering::SeqCst);
            s.queue.iter().for_each(Conn::reset_remote);
            s.queue.clear();
            s.wait.take()
        };
        wake.run();
    }

    pub(crate) fn poll(&self, waiter: Option<&Arc<Parker>>) -> PollEvents {
        if let Some(c) = self.conn() {
            return c.poll(waiter);
        }
        let mut s = self.st.lock();
        if matches!(s.syn, SynState::Sent(_)) {
            // SYN_SENT: o `tcp_poll` não dá nada até o handshake terminar.
            if let Some(w) = waiter {
                s.wait.register(w);
            }
            return PollEvents::empty();
        }
        if !self.listening.load(Ordering::SeqCst) {
            // Socket TCP ligado e sem conexão: o Linux responde OUT e HUP.
            return PollEvents::OUT | PollEvents::HUP;
        }
        if let Some(w) = waiter {
            s.wait.register(w);
        }
        if s.queue.is_empty() { PollEvents::empty() } else { PollEvents::IN }
    }

    pub(crate) fn unregister(&self, waiter: &Arc<Parker>) {
        if let Some(c) = self.conn() {
            c.unregister(waiter);
        }
        self.st.lock().wait.unregister(waiter);
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let wake = {
            let mut s = self.st.lock();
            s.closed = true;
            // As conexões que ninguém aceitou fecham com RST (`inet_csk_listen_stop`): o cliente vê ECONNRESET.
            s.queue.iter().for_each(Conn::reset_remote);
            s.queue.clear();
            // Um `connect` pendente morre com o socket: a thread das retransmissões sai.
            if let SynState::Sent(syn) = &s.syn {
                syn.ticker.unpark();
            }
            s.wait.take()
        };
        wake.run();
        if let Some(ports) = self.ports.upgrade() {
            let mut st = ports.st.lock();
            st.listeners.retain(Bound::live);
            st.syns.retain(|w| w.strong_count() > 0);
        }
    }
}

/// Estado de RST de uma ponta, o `sk_err` e o `SHUTDOWN_MASK` que o `tcp_reset` deixa: nenhum, `ECONNRESET`
/// pendente (RST recebido em ESTABLISHED, o que um `close` com dados não lidos do par provoca), `EPIPE`
/// pendente (RST recebido em CLOSE_WAIT, a resposta do par que já fechou a uma escrita) ou o erro já
/// entregue (a leitura dá EOF e a escrita EPIPE).
const RESET_NONE: u8 = 0;
const RESET_PENDING: u8 = 1;
const RESET_DONE: u8 = 2;
const RESET_EPIPE: u8 = 3;

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
            self.reset_remote();
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

    /// Os bytes que chegaram e ainda não foram lidos (`SIOCINQ`).
    pub(crate) fn unread(&self) -> usize {
        self.rx.lock().as_ref().map_or(0, |e| e.pipe.pending())
    }

    /// A outra ponta passa a ter o `ECONNRESET` pendente (o RST que o fechamento daqui manda).
    pub(crate) fn reset_remote(&self) {
        let _ = self.reset_peer.compare_exchange(RESET_NONE, RESET_PENDING, Ordering::SeqCst, Ordering::SeqCst);
    }

    /// Os endereços local e remoto desta ponta, como a tabela do TCP os mostra; `None` no par de um
    /// socket Unix (que não tem endereço IP).
    pub(crate) fn addrs(&self) -> Option<((IpAddr, u16), (IpAddr, u16))> {
        let (pair, i) = self.pair.as_ref()?;
        let sides = pair.sides.lock();
        Some((sides[*i].local, sides[*i].remote))
    }

    /// A outra ponta ainda existe (não foi fechada nem descartada).
    pub(crate) fn peer_open(&self) -> bool {
        Arc::strong_count(&self.reset_me) > 1
    }

    /// Escrita para um par TCP que já fechou: o Linux aceita a primeira, o par responde com RST e, como esta
    /// ponta está em CLOSE_WAIT, o `sk_err` vira `EPIPE` (não `ECONNRESET`): o `SO_ERROR` e a próxima escrita o
    /// entregam, e a leitura segue dando EOF (o FIN do par já a encerrou).
    fn swallow_write(&self) {
        let _ = self.reset_me.compare_exchange(RESET_NONE, RESET_EPIPE, Ordering::SeqCst, Ordering::SeqCst);
    }

    /// `sock_error`: o `sk_err` pendente de um RST sai uma vez; depois a leitura dá EOF e a escrita EPIPE.
    pub(crate) fn take_error(&self) -> Option<Errno> {
        let taken = self.reset_me.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |s| matches!(s, RESET_PENDING | RESET_EPIPE).then_some(RESET_DONE));
        taken.ok().map(|s| if s == RESET_EPIPE { Errno::EPIPE } else { Errno::ECONNRESET })
    }

    /// A leitura chegou ao fim da fila: um `ECONNRESET` pendente sai uma vez antes do EOF (o `sk_err` vem
    /// antes do `RCV_SHUTDOWN` no `tcp_recvmsg`); o `EPIPE` do CLOSE_WAIT fica, porque o `SOCK_DONE` do FIN
    /// faz a leitura devolver 0 sem olhar o `sk_err`.
    pub(crate) fn eof_error(&self) -> Option<Errno> {
        self.reset_me.compare_exchange(RESET_PENDING, RESET_DONE, Ordering::SeqCst, Ordering::SeqCst).ok().map(|_| Errno::ECONNRESET)
    }

    /// O pipe de uma escrita, antes de copiar dados (`tcp_sendmsg`, `unix_stream_sendmsg`). No TCP o erro
    /// pendente sai primeiro (`ECONNRESET`, ou o `EPIPE` do RST em CLOSE_WAIT); depois o `SHUT_WR` e o RST já
    /// entregue dão `EPIPE`; para um par que já fechou, `Ok(None)`: a escrita conta como feita. No Unix, o
    /// `SHUT_WR` e o par fechado são `EPIPE` na hora, sem tocar no erro pendente da leitura.
    pub(crate) fn tx_for_write(&self) -> Result<Option<Arc<Pipe>>, Errno> {
        if self.pair.is_none() {
            return match self.tx() {
                Some(p) if p.accepts_writes() => Ok(Some(p)),
                _ => Err(Errno::EPIPE),
            };
        }
        if let Some(e) = self.take_error() {
            return Err(e);
        }
        if self.reset_me.load(Ordering::SeqCst) == RESET_DONE {
            return Err(Errno::EPIPE);
        }
        let pipe = self.tx().ok_or(Errno::EPIPE)?;
        if pipe.counters().0 == 0 {
            self.swallow_write();
            return Ok(None);
        }
        Ok(Some(pipe))
    }

    /// `shutdown(SHUT_RD)` já foi feito (`RCV_SHUTDOWN`).
    pub(crate) fn read_shut(&self) -> bool {
        self.rx().is_some_and(|p| p.rcv_shut())
    }

    /// O pipe de leitura.
    pub(crate) fn rx(&self) -> Option<Arc<Pipe>> {
        self.rx.lock().as_ref().map(|e| e.pipe.clone())
    }

    /// O pipe de escrita; `None` depois de `shutdown(SHUT_WR)` (a escrita dá EPIPE).
    pub(crate) fn tx(&self) -> Option<Arc<Pipe>> {
        self.tx.lock().as_ref().map(|e| e.pipe.clone())
    }

    /// `shutdown`. `SHUT_RD` só marca `RCV_SHUTDOWN` no pipe que esta ponta lê (acordando quem espera nele): o que
    /// já chegou segue legível e depois a leitura dá 0. No TCP o par continua escrevendo; no Unix ele passa a ver
    /// EPIPE (`unix_shutdown` põe `SEND_SHUTDOWN` nele). `SHUT_WR` solta a ponta de escrita: o par vê o FIN.
    pub(crate) fn shutdown(&self, read: bool, write: bool) {
        if read && let Some(p) = self.rx() {
            p.shutdown_read(self.pair.is_none());
        }
        if write && let Some(p) = self.tx() {
            p.shutdown_write();
        }
        drop(write.then(|| self.tx.lock().take()));
        if write && let Some((pair, i)) = &self.pair {
            pair.sides.lock()[*i].fin.get_or_insert_with(Instant::now);
        }
    }

    pub(crate) fn poll(&self, waiter: Option<&Arc<Parker>>) -> PollEvents {
        if self.pair.is_some() {
            return self.tcp_poll(waiter);
        }
        let mut ev = PollEvents::empty();
        if self.reset_me.load(Ordering::SeqCst) == RESET_PENDING {
            ev |= PollEvents::IN | PollEvents::OUT | PollEvents::ERR | PollEvents::HUP;
        }
        if let Some(p) = self.rx() {
            ev |= p.poll(true, false, waiter);
        }
        let shut_rd = ev.contains(PollEvents::RDHUP);
        match self.tx() {
            Some(p) => ev |= p.poll(false, true, waiter),
            // Os dois sentidos encerrados: `unix_poll` dá HUP com `sk_shutdown == SHUTDOWN_MASK`.
            None if shut_rd => ev |= PollEvents::OUT | PollEvents::HUP,
            None => ev |= PollEvents::OUT,
        }
        // Do lado que escreve, um par fechado é HUP de socket, não ERR de pipe.
        if ev.contains(PollEvents::ERR) {
            ev.remove(PollEvents::ERR);
            ev |= PollEvents::HUP;
        }
        ev
    }

    /// O `tcp_poll` de uma conexão estabelecida. `HUP` só com os dois sentidos encerrados (`SHUTDOWN_MASK`: o
    /// RST, ou o FIN do par junto com o `shutdown(SHUT_WR)` daqui); o FIN do par sozinho é `IN|RDHUP`, e o par
    /// fechado não impede a escrita (o RST só volta depois dela). `ERR` enquanto o `sk_err` do RST não foi lido.
    fn tcp_poll(&self, waiter: Option<&Arc<Parker>>) -> PollEvents {
        let state = self.reset_me.load(Ordering::SeqCst);
        let rx = self.rx().map_or(PollEvents::empty(), |p| p.poll(true, false, waiter));
        let tx = self.tx().map(|p| p.poll(false, true, waiter));
        let aborted = state != RESET_NONE;
        let rcv_shut = aborted || self.read_shut() || rx.contains(PollEvents::HUP);
        let snd_shut = aborted || tx.is_none();
        let mut ev = rx & PollEvents::IN;
        if rcv_shut {
            ev |= PollEvents::IN | PollEvents::RDHUP;
        }
        if snd_shut || tx.is_some_and(|t| t.intersects(PollEvents::OUT | PollEvents::ERR)) {
            ev |= PollEvents::OUT;
        }
        if rcv_shut && snd_shut {
            ev |= PollEvents::HUP;
        }
        if matches!(state, RESET_PENDING | RESET_EPIPE) {
            ev |= PollEvents::ERR;
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
        let Some(pipe) = self.conn.rx() else { return Ok(0) };
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            match pipe.try_read(buf, false, &self.parker) {
                // EOF vindo de um fechamento com RST: o erro vem antes do EOF.
                Try::Ready(Ok(0)) => return self.conn.eof_error().map_or(Ok(0), |e| Err(e.into())),
                Try::Ready(Ok(n)) => return Ok(n),
                Try::Ready(Err(e)) => return Err(e.into()),
                Try::Pending => self.parker.park(),
            }
        }
    }
}

impl io::Write for HostStream {
    /// Como a escrita de um processo (`Conn::tx_for_write`): o primeiro `write` para um par que já fechou conta
    /// como feito e o seguinte dá EPIPE.
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let pipe = match self.conn.tx_for_write() {
            Ok(Some(pipe)) => pipe,
            Ok(None) => return Ok(data.len()),
            Err(e) => return Err(e.into()),
        };
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
