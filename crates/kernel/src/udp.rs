//! UDP de loopback entre os processos de um sandbox.
//!
//! Um datagrama vai direto para a fila do socket de destino, com o endereço de quem enviou. A escolha do
//! destino segue o `udp4_lib_lookup`: porta igual, endereço local igual ou curinga, e, num socket
//! conectado, só o par dele; ganha o mais específico. Sem destino, um socket conectado recebe o
//! ECONNREFUSED do ICMP de porta inalcançável na próxima operação; um sem conexão não fica sabendo.
//!
//! A fila conta o `truesize` de cada datagrama no `sk_rmem_alloc`, como o Linux 6.12 o calcula no
//! loopback, e descarta (contando em `drops`) quando passa do `rmem_default` (212992). Só os sockets
//! com porta entram na tabela do `/proc/net/udp`, no balde `(porta + mistura do namespace) & 0x3fff`.

use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use sysabi::{Errno, PollEvents};
use vfs::procfs::UdpSockRow;

use crate::net::hashed_ptr;
use crate::park::{Parker, WaitList, key};
use crate::pipe::{Pipe, Try};

const EPHEMERAL_LOW: u16 = 32768;
const EPHEMERAL_HIGH: u16 = 60999;
/// `net.core.rmem_default` do Debian 13.
const RCVBUF: u32 = 212_992;
/// Máscara da tabela de hash do UDP (16384 baldes).
const HASH_MASK: u32 = 0x3fff;

/// `truesize` de um datagrama de `len` bytes no loopback (`__ip_append_data` mais `kmalloc_reserve`):
/// até o limite do `SKB_MAX_ALLOC` o dado vai na cabeça do skb, arredondada para o cache pequeno de 704
/// bytes ou para o balde do kmalloc; acima dele a cabeça é pequena e o dado vai em páginas, contado
/// byte a byte. Mais os 256 bytes do próprio `sk_buff`.
fn truesize(len: usize) -> u32 {
    const SKB: u32 = 256;
    const SHINFO: usize = 320;
    const SMALL_HEAD: usize = 704;
    // Cabeçalhos IP e UDP, mais o `LL_RESERVED_SPACE` do lo e o alinhamento de 15.
    let size = len + 28 + 16 + 15;
    if size >= 16384 - SHINFO {
        return SMALL_HEAD as u32 + SKB + len as u32;
    }
    let obj = size.div_ceil(64) * 64 + SHINFO;
    let head = if obj <= SMALL_HEAD { SMALL_HEAD } else { obj.next_power_of_two() };
    head as u32 + SKB
}

/// O que um endereço local cobre, para o conflito do `bind`.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Scope {
    AllV4,
    All,
    V4(Ipv4Addr),
    V6(Ipv6Addr),
}

fn scope(v6: bool, ip: IpAddr) -> Scope {
    match ip {
        IpAddr::V4(a) if a.is_unspecified() => Scope::AllV4,
        IpAddr::V4(a) => Scope::V4(a),
        IpAddr::V6(a) if a.is_unspecified() => {
            if v6 {
                Scope::All
            } else {
                Scope::AllV4
            }
        }
        IpAddr::V6(a) => match a.to_ipv4_mapped() {
            Some(m) => Scope::V4(m),
            None => Scope::V6(a),
        },
    }
}

fn overlap(a: Scope, b: Scope) -> bool {
    match (a, b) {
        (Scope::All, _) | (_, Scope::All) => true,
        (Scope::AllV4, Scope::V6(_)) | (Scope::V6(_), Scope::AllV4) => false,
        (Scope::AllV4, _) | (_, Scope::AllV4) => true,
        (x, y) => x == y,
    }
}

/// O endereço como o pacote o leva: IPv4 quando é IPv4 ou mapeado.
fn wire(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(a) => a.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

/// O endereço como um socket da família `v6` o vê.
fn as_family(v6: bool, ip: IpAddr) -> IpAddr {
    match (v6, ip) {
        (true, IpAddr::V4(a)) => IpAddr::V6(a.to_ipv6_mapped()),
        (_, ip) => ip,
    }
}

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

/// O destino de um envio, no loopback: `0.0.0.0` e `::` são este host. Fora do loopback não há rota.
fn route(ip: IpAddr) -> Result<IpAddr, Errno> {
    match wire(ip) {
        IpAddr::V4(a) if a.is_unspecified() => Ok(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        IpAddr::V4(a) if a.is_loopback() => Ok(IpAddr::V4(a)),
        IpAddr::V6(a) if a.is_unspecified() || a.is_loopback() => Ok(IpAddr::V6(Ipv6Addr::LOCALHOST)),
        _ => Err(Errno::ENETUNREACH),
    }
}

/// O endereço de origem que o loopback escolhe para um destino.
fn source_for(dst: IpAddr) -> IpAddr {
    match dst {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
    }
}

/// Os sockets UDP de um sandbox.
#[derive(Debug)]
pub(crate) struct UdpTable {
    socks: Mutex<Vec<Weak<UdpSock>>>,
    /// O `net_hash_mix` do namespace: desloca o balde de cada porta.
    mix: u32,
}

impl Default for UdpTable {
    fn default() -> Self {
        UdpTable { socks: Mutex::new(Vec::new()), mix: hashed_ptr() }
    }
}

#[derive(Debug)]
struct Datagram {
    data: Vec<u8>,
    from: (IpAddr, u16),
    truesize: u32,
}

#[derive(Debug, Default)]
struct UdpState {
    /// O endereço e a porta locais; `None` antes do `bind` (ou do autobind do primeiro envio).
    local: Option<(IpAddr, u16)>,
    peer: Option<(IpAddr, u16)>,
    reuse: bool,
    rx: VecDeque<Datagram>,
    rmem: u32,
    drops: u32,
    /// O erro do ICMP, entregue (e limpo) pela próxima operação.
    err: Option<Errno>,
    wait: WaitList,
}

/// Um socket UDP.
#[derive(Debug)]
pub(crate) struct UdpSock {
    pub v6: bool,
    /// Dá o inode, o dono e a data do `fstat`.
    pub ident: Arc<Pipe>,
    uid: u32,
    ptr: u32,
    st: Mutex<UdpState>,
}

impl UdpTable {
    pub(crate) fn socket(&self, v6: bool, ident: Arc<Pipe>) -> Arc<UdpSock> {
        let uid = ident.stat(None).uid;
        let s = Arc::new(UdpSock { v6, ident, uid, ptr: hashed_ptr(), st: Mutex::new(UdpState::default()) });
        let mut socks = self.socks.lock();
        socks.retain(|w| w.strong_count() > 0);
        socks.push(Arc::downgrade(&s));
        s
    }

    fn live(&self) -> Vec<Arc<UdpSock>> {
        let mut socks = self.socks.lock();
        socks.retain(|w| w.strong_count() > 0);
        socks.iter().filter_map(Weak::upgrade).collect()
    }

    /// Quantos sockets UDP existem, com porta ou não.
    pub(crate) fn count(&self) -> usize {
        self.live().len()
    }

    /// `bind`: porta 0 sorteia uma livre, como o `udp_lib_get_port`. EADDRINUSE se outro socket já
    /// tem a porta num endereço que se sobrepõe, a menos que os dois tenham `SO_REUSEADDR`.
    pub(crate) fn bind(&self, sock: &Arc<UdpSock>, ip: IpAddr, port: u16, reuse: bool) -> Result<u16, Errno> {
        if sock.st.lock().local.is_some() {
            return Err(Errno::EINVAL);
        }
        match wire(ip) {
            // O `inet_bind` também aceita o broadcast (`RTN_BROADCAST`): o socket só recebe o que for a ele.
            IpAddr::V4(a) if a.is_unspecified() || a.is_loopback() || a.is_broadcast() => {}
            IpAddr::V6(a) if a.is_unspecified() || a.is_loopback() => {}
            _ => return Err(Errno::EADDRNOTAVAIL),
        }
        let others = self.live();
        let mine = scope(sock.v6, ip);
        let taken = |port: u16, reuse: bool| {
            others.iter().filter(|o| !Arc::ptr_eq(o, sock)).any(|o| {
                let st = o.st.lock();
                st.local.is_some_and(|(oip, op)| op == port && overlap(scope(o.v6, oip), mine) && !(reuse && st.reuse))
            })
        };
        let port = if port == 0 {
            let range = u32::from(EPHEMERAL_HIGH - EPHEMERAL_LOW) + 1;
            let start = hashed_ptr() % range;
            (0..range)
                .map(|i| EPHEMERAL_LOW + ((start + i) % range) as u16)
                .find(|&p| !taken(p, false))
                .ok_or(Errno::EADDRINUSE)?
        } else if taken(port, reuse) {
            return Err(Errno::EADDRINUSE);
        } else {
            port
        };
        let mut st = sock.st.lock();
        st.local = Some((ip, port));
        st.reuse = reuse;
        Ok(port)
    }

    /// Porta efêmera para quem envia ou conecta sem `bind`.
    fn autobind(&self, sock: &Arc<UdpSock>) -> Result<(), Errno> {
        if sock.st.lock().local.is_some() {
            return Ok(());
        }
        let any = if sock.v6 { IpAddr::V6(Ipv6Addr::UNSPECIFIED) } else { IpAddr::V4(Ipv4Addr::UNSPECIFIED) };
        self.bind(sock, any, 0, false).map(|_| ())
    }

    /// `connect`: fixa o par e, sem `bind`, a porta e o endereço de origem da rota.
    pub(crate) fn connect(&self, sock: &Arc<UdpSock>, ip: IpAddr, port: u16) -> Result<(), Errno> {
        let dst = route(ip)?;
        self.autobind(sock)?;
        let mut st = sock.st.lock();
        if let Some((lip, lport)) = st.local
            && wire(lip).is_unspecified()
        {
            st.local = Some((as_family(sock.v6, source_for(dst)), lport));
        }
        st.peer = Some((as_family(sock.v6, dst), port));
        st.err = None;
        Ok(())
    }

    /// `sendto`: entrega a `dst` (ou ao par do `connect`).
    pub(crate) fn send(&self, sock: &Arc<UdpSock>, data: &[u8], dst: Option<(IpAddr, u16)>) -> Result<usize, Errno> {
        let max = if sock.v6 { 65527 } else { 65507 };
        if let Some(e) = sock.st.lock().err.take() {
            return Err(e);
        }
        let (dip, dport) = match dst {
            Some(d) => d,
            None => sock.st.lock().peer.ok_or(Errno::EDESTADDRREQ)?,
        };
        if dport == 0 {
            return Err(Errno::EINVAL);
        }
        let dip = route(dip)?;
        if data.len() > max {
            return Err(Errno::EMSGSIZE);
        }
        self.autobind(sock)?;
        let (lip, lport, connected) = {
            let st = sock.st.lock();
            let (lip, lport) = st.local.unwrap_or((IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0));
            (lip, lport, st.peer.is_some())
        };
        let src_ip = if wire(lip).is_unspecified() { source_for(dip) } else { wire(lip) };
        // O destino mais específico: endereço local exato e conectado ao remetente valem mais.
        let mut best: Option<(u8, Arc<UdpSock>)> = None;
        for o in self.live() {
            let st = o.st.lock();
            let Some((oip, oport)) = st.local else { continue };
            if oport != dport {
                continue;
            }
            let family_ok = match dip {
                IpAddr::V4(_) => !o.v6 || matches!(scope(true, oip), Scope::All | Scope::V4(_)),
                IpAddr::V6(_) => o.v6,
            };
            if !family_ok {
                continue;
            }
            let exact = match scope(o.v6, oip) {
                Scope::All | Scope::AllV4 => false,
                _ if wire(oip) == dip => true,
                _ => continue,
            };
            if let Some((pip, pport)) = st.peer
                && (wire(pip) != src_ip || pport != lport)
            {
                continue;
            }
            let score = u8::from(exact) * 2 + u8::from(st.peer.is_some()) * 4;
            if best.as_ref().is_none_or(|(b, _)| score >= *b) {
                best = Some((score, o.clone()));
            }
        }
        let Some((_, dest)) = best else {
            // Porta inalcançável: o ICMP volta e só um socket conectado guarda o erro.
            if connected {
                sock.st.lock().err = Some(Errno::ECONNREFUSED);
            }
            return Ok(data.len());
        };
        let size = truesize(data.len());
        let mut st = dest.st.lock();
        if st.rmem > RCVBUF {
            st.drops += 1;
            return Ok(data.len());
        }
        st.rmem += size;
        st.rx.push_back(Datagram { data: data.to_vec(), from: (as_family(dest.v6, src_ip), lport), truesize: size });
        let wake = st.wait.take_key(key::READ);
        drop(st);
        wake.run();
        Ok(data.len())
    }

    /// A tabela do `/proc/net/udp` e do `udp6`, na ordem dos baldes.
    pub(crate) fn rows(&self) -> Vec<UdpSockRow> {
        let mut out: Vec<UdpSockRow> = self
            .live()
            .iter()
            .filter_map(|s| {
                let st = s.st.lock();
                let (lip, lport) = st.local?;
                let (rip, rport) = st.peer.unwrap_or((if s.v6 { IpAddr::V6(Ipv6Addr::UNSPECIFIED) } else { IpAddr::V4(Ipv4Addr::UNSPECIFIED) }, 0));
                Some(UdpSockRow {
                    v6: s.v6,
                    sl: (u32::from(lport).wrapping_add(self.mix)) & HASH_MASK,
                    local_ip: ip_bytes(lip),
                    local_port: lport,
                    remote_ip: ip_bytes(rip),
                    remote_port: rport,
                    state: if st.peer.is_some() { 1 } else { 7 },
                    rx_queue: st.rmem,
                    uid: s.uid,
                    inode: s.ident.ino,
                    refcnt: 2,
                    ptr: s.ptr,
                    drops: st.drops,
                })
            })
            .collect();
        out.sort_by_key(|r| r.sl);
        out
    }
}

impl UdpSock {
    /// `SIOCINQ` do UDP (`first_packet_length`): o tamanho do próximo datagrama, 0 sem nenhum.
    pub(crate) fn next_len(&self) -> usize {
        self.st.lock().rx.front().map_or(0, |d| d.data.len())
    }
    /// O endereço local (`0.0.0.0:0` antes do `bind`) e o par.
    pub(crate) fn names(&self) -> ((IpAddr, u16), Option<(IpAddr, u16)>) {
        let st = self.st.lock();
        let any = if self.v6 { IpAddr::V6(Ipv6Addr::UNSPECIFIED) } else { IpAddr::V4(Ipv4Addr::UNSPECIFIED) };
        (st.local.unwrap_or((any, 0)), st.peer)
    }

    /// `recvfrom`: o próximo datagrama inteiro (o resto do que não cabe se perde) e quem enviou.
    pub(crate) fn try_recv(&self, peek: bool, nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<(Vec<u8>, (IpAddr, u16)), Errno>> {
        let mut st = self.st.lock();
        if let Some(e) = st.err.take() {
            st.wait.unregister(waiter);
            return Try::Ready(Err(e));
        }
        let got = if peek {
            st.rx.front().map(|d| (d.data.clone(), d.from))
        } else {
            st.rx.pop_front().map(|d| {
                st.rmem -= d.truesize;
                (d.data, d.from)
            })
        };
        if let Some(m) = got {
            st.wait.unregister(waiter);
            return Try::Ready(Ok(m));
        }
        if nonblock {
            st.wait.unregister(waiter);
            return Try::Ready(Err(Errno::EAGAIN));
        }
        st.wait.register(waiter);
        Try::Pending
    }

    pub(crate) fn poll(&self, waiter: Option<&Arc<Parker>>) -> PollEvents {
        let mut st = self.st.lock();
        if let Some(w) = waiter {
            st.wait.register(w);
        }
        let mut ev = PollEvents::OUT;
        if !st.rx.is_empty() {
            ev |= PollEvents::IN;
        }
        if st.err.is_some() {
            ev |= PollEvents::ERR;
        }
        ev
    }

    pub(crate) fn unregister(&self, waiter: &Arc<Parker>) {
        self.st.lock().wait.unregister(waiter);
    }
}
