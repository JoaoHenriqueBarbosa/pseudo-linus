//! Sockets do domínio Unix (`AF_UNIX`) entre os processos de um sandbox.
//!
//! Todo socket nasce na tabela do sandbox, de onde sai o `/proc/net/unix`. O `bind` num caminho cria o
//! arquivo `S_IFSOCK` no VFS (quem faz isso é o `sys`, com a umask do processo) e liga o nó do arquivo,
//! pelo `(st_dev, st_ino)`, ao socket; no espaço abstrato (nome começando com o byte nulo) o nome é a
//! chave. `connect` resolve o caminho de novo, como o `unix_find_bsd`: o arquivo removido ou trocado
//! deixa de levar ao socket.
//!
//! Uma conexão de fluxo é o mesmo par de pipes do TCP de loopback (`net::conn_pair`): o `connect` cria
//! a ponta do servidor (o embrião, `SS_CONNECTING` e sem inode na tabela) e a põe na fila de quem
//! escuta, até o `accept`. Um socket de datagrama guarda as mensagens recebidas com o nome de quem
//! enviou; a fila tem o tamanho do `net.unix.max_dgram_qlen` (10).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use sysabi::{Errno, PollEvents};
use vfs::procfs::UnixSockRow;

use crate::net::{Conn, hashed_ptr};
use crate::park::{Parker, WaitList, Wake};
use crate::pipe::{Pipe, Try};
use crate::scm::{Scm, Ucred};
use crate::seqpacket::{SeqEnd, seq_pair};

pub(crate) const SOCK_STREAM: u8 = 1;
pub(crate) const SOCK_DGRAM: u8 = 2;
pub(crate) const SOCK_SEQPACKET: u8 = 5;

/// `__SO_ACCEPTCON`: o socket está em escuta.
const SO_ACCEPTCON: u32 = 0x1_0000;
/// `net.unix.max_dgram_qlen` do Debian 13.
const MAX_DGRAM_QLEN: usize = 10;

/// O lugar de um nome: o nó do arquivo do socket ou um nome do espaço abstrato.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Key {
    Node(u64, u64),
    Abstract(Vec<u8>),
}

/// Os sockets Unix de um sandbox e os nomes ligados a eles.
#[derive(Debug, Default)]
pub(crate) struct UnixTable {
    st: Mutex<TableState>,
}

#[derive(Debug, Default)]
struct TableState {
    /// Da criação mais antiga à mais nova; os que já fecharam saem na próxima leitura.
    socks: Vec<Weak<UnixSock>>,
    names: HashMap<Key, Weak<UnixSock>>,
}

impl UnixTable {
    /// Um socket novo, sem nome nem conexão.
    pub(crate) fn socket(self: &Arc<Self>, ty: u8, ident: Arc<Pipe>) -> Arc<UnixSock> {
        let role = if ty == SOCK_DGRAM { Role::Dgram(Dgram::default()) } else { Role::Idle };
        self.add(ty, ident, role, None, None, true)
    }

    fn add(self: &Arc<Self>, ty: u8, ident: Arc<Pipe>, role: Role, name: Option<Vec<u8>>, peer_name: Option<Vec<u8>>, accepted: bool) -> Arc<UnixSock> {
        let s = Arc::new(UnixSock {
            ty,
            ident,
            ptr: hashed_ptr(),
            table: Arc::downgrade(self),
            st: Mutex::new(SockState { name, key: None, role, accepted, peer_name, peer_cred: None }),
            passcred: AtomicBool::new(false),
        });
        let mut st = self.st.lock();
        st.socks.retain(|w| w.strong_count() > 0);
        st.socks.push(Arc::downgrade(&s));
        s
    }

    /// `socketpair`: duas pontas já ligadas, sem nome. As duas guardam as credenciais de quem criou o par
    /// (`init_peercred` do `unix_socketpair`).
    pub(crate) fn pair(self: &Arc<Self>, ty: u8, idents: (Arc<Pipe>, Arc<Pipe>), mk_pipe: impl Fn() -> Arc<Pipe>, cred: Ucred) -> (Arc<UnixSock>, Arc<UnixSock>) {
        let (a, b) = if ty == SOCK_DGRAM {
            let a = self.add(ty, idents.0, Role::Dgram(Dgram::default()), None, None, true);
            let b = self.add(ty, idents.1, Role::Dgram(Dgram::default()), None, None, true);
            for (x, y) in [(&a, &b), (&b, &a)] {
                let mut st = x.st.lock();
                if let Role::Dgram(d) = &mut st.role {
                    d.peer = Some(Arc::downgrade(y));
                }
            }
            (a, b)
        } else if ty == SOCK_SEQPACKET {
            let (ea, eb) = seq_pair();
            let a = self.add(ty, idents.0, Role::Seq(Arc::new(ea)), None, None, true);
            let b = self.add(ty, idents.1, Role::Seq(Arc::new(eb)), None, None, true);
            (a, b)
        } else {
            let (ca, cb) = crate::net::conn_pair(mk_pipe);
            let a = self.add(ty, idents.0, Role::Stream(Arc::new(ca)), None, None, true);
            let b = self.add(ty, idents.1, Role::Stream(Arc::new(cb)), None, None, true);
            (a, b)
        };
        a.st.lock().peer_cred = Some(cred);
        b.st.lock().peer_cred = Some(cred);
        (a, b)
    }

    /// Liga `name` (já criado no VFS, com a chave `key`) a `sock`. EINVAL se ele já tem nome;
    /// EADDRINUSE se o nome abstrato já tem dono.
    pub(crate) fn bind(&self, sock: &Arc<UnixSock>, name: Vec<u8>, key: Key) -> Result<(), Errno> {
        let mut t = self.st.lock();
        let mut st = sock.st.lock();
        if st.name.is_some() {
            return Err(Errno::EINVAL);
        }
        if t.names.get(&key).is_some_and(|w| w.strong_count() > 0) {
            return Err(Errno::EADDRINUSE);
        }
        t.names.insert(key.clone(), Arc::downgrade(sock));
        st.name = Some(name);
        st.key = Some(key);
        Ok(())
    }

    /// O socket ligado a `key`, se ainda existe.
    pub(crate) fn lookup(&self, key: &Key) -> Option<Arc<UnixSock>> {
        self.st.lock().names.get(key).and_then(Weak::upgrade)
    }

    /// A tabela do `/proc/net/unix`.
    pub(crate) fn rows(&self) -> Vec<UnixSockRow> {
        let socks: Vec<Arc<UnixSock>> = {
            let mut t = self.st.lock();
            t.socks.retain(|w| w.strong_count() > 0);
            t.socks.iter().filter_map(Weak::upgrade).collect()
        };
        // Um datagrama conectado segura uma referência do par (`sock_hold` no `unix_dgram_connect`).
        let held = |target: &Arc<UnixSock>| {
            socks
                .iter()
                .filter(|s| !Arc::ptr_eq(s, target))
                .filter(|s| matches!(&s.st.lock().role, Role::Dgram(d) if d.peer.as_ref().is_some_and(|p| std::ptr::eq(p.as_ptr(), Arc::as_ptr(target)))))
                .count() as u32
        };
        let mut out = Vec::new();
        for s in &socks {
            let st = s.st.lock();
            // Cada ponta segura uma referência da outra enquanto ela existe (`unix_peer`): uma conexão
            // cujo par fechou volta a 2. No datagrama, `unix_dgram_connect` marca as duas pontas como
            // conectadas.
            let (flags, state, refcnt) = match &st.role {
                Role::Idle => (0, 1, 2),
                Role::Listening(_) => (SO_ACCEPTCON, 1, 2),
                Role::Stream(c) => (0, if st.accepted { 3 } else { 2 }, 2 + u32::from(c.peer_open())),
                Role::Seq(e) => (0, if st.accepted { 3 } else { 2 }, 2 + u32::from(e.peer_open())),
                Role::Dgram(d) => (0, if d.peer.is_some() { 3 } else { 1 }, 2),
            };
            let dgram = matches!(st.role, Role::Dgram(_));
            drop(st);
            let held = if dgram { held(s) } else { 0 };
            let (state, refcnt) = if held > 0 { (3, refcnt + held) } else { (state, refcnt) };
            let st = s.st.lock();
            out.push(UnixSockRow {
                ptr: s.ptr,
                refcnt,
                flags,
                ty: u16::from(s.ty),
                state,
                inode: if st.accepted { s.ident.ino } else { 0 },
                path: st.name.clone(),
            });
        }
        out
    }
}

#[derive(Debug, Default)]
struct Listen {
    backlog: usize,
    queue: VecDeque<Arc<UnixSock>>,
    /// Quem espera uma conexão (`accept`, `poll`).
    wait: WaitList,
    /// Quem espera lugar na fila (`connect` com a fila cheia).
    space: WaitList,
}

/// Uma mensagem de datagrama na fila de quem recebe: os dados, o nome de quem enviou e os dados auxiliares.
#[derive(Clone, Debug)]
pub(crate) struct Datagram {
    pub data: Vec<u8>,
    pub from: Option<Vec<u8>>,
    pub scm: Scm,
}

#[derive(Debug, Default)]
struct Dgram {
    rx: VecDeque<Datagram>,
    /// O par do `connect` (ou do `socketpair`): destino do `send` sem endereço.
    peer: Option<Weak<UnixSock>>,
    wait: WaitList,
    space: WaitList,
}

#[derive(Debug)]
enum Role {
    Idle,
    Listening(Listen),
    /// Conectado (o embrião também, antes do `accept`).
    Stream(Arc<Conn>),
    /// Conectado em `SOCK_SEQPACKET`: filas de mensagens, não bytes.
    Seq(Arc<SeqEnd>),
    Dgram(Dgram),
}

#[derive(Debug)]
struct SockState {
    /// O nome do `bind`; o socket aceito herda o de quem escuta.
    name: Option<Vec<u8>>,
    key: Option<Key>,
    role: Role,
    /// Falso só no embrião, que ainda não tem inode.
    accepted: bool,
    peer_name: Option<Vec<u8>>,
    /// `sk_peer_pid` e `sk_peer_cred`: as credenciais do par (`SO_PEERCRED`). O `listen` grava as de quem escuta,
    /// o `connect` dá ao cliente as do servidor e ao embrião as do cliente, o `socketpair` as de quem o criou.
    peer_cred: Option<Ucred>,
}

/// Um socket do domínio Unix.
#[derive(Debug)]
pub(crate) struct UnixSock {
    /// `SOCK_STREAM`, `SOCK_DGRAM` ou `SOCK_SEQPACKET`.
    pub ty: u8,
    /// Dá o inode, o dono e a data do `fstat`.
    pub ident: Arc<Pipe>,
    ptr: u32,
    table: Weak<UnixTable>,
    /// `SOCK_PASSCRED` (`SO_PASSCRED`): quem recebe passa a ver as credenciais de quem enviou.
    passcred: AtomicBool,
    st: Mutex<SockState>,
}

impl Drop for UnixSock {
    fn drop(&mut self) {
        let mut wake = Wake::none();
        let st = self.st.get_mut();
        match &mut st.role {
            // As conexões que ninguém aceitou fecham: o `unix_release_sock` do embrião marca o cliente com
            // ECONNRESET (`embrion` verdadeiro), que sai uma vez antes do EOF.
            Role::Listening(l) => {
                for embryo in l.queue.drain(..) {
                    match &embryo.st.lock().role {
                        Role::Stream(c) => c.reset_remote(),
                        Role::Seq(e) => e.reset_remote(),
                        _ => {}
                    }
                }
                wake.merge(l.wait.take());
                wake.merge(l.space.take());
            }
            Role::Dgram(d) => {
                wake.merge(d.wait.take());
                wake.merge(d.space.take());
            }
            _ => {}
        }
        if let (Some(key), Some(t)) = (st.key.take(), self.table.upgrade()) {
            let mut t = t.st.lock();
            if t.names.get(&key).is_some_and(|w| w.strong_count() == 0) {
                t.names.remove(&key);
            }
        }
        wake.run();
    }
}

impl UnixSock {
    /// `SIOCINQ` (`unix_inq_len`): os bytes de um fluxo ou de um `SOCK_SEQPACKET` que um `recv` leria, ou o
    /// tamanho do próximo datagrama; EINVAL num socket em escuta.
    pub(crate) fn unread(&self) -> Result<usize, Errno> {
        match &self.st.lock().role {
            Role::Listening(_) => Err(Errno::EINVAL),
            Role::Idle => Ok(0),
            Role::Stream(c) => Ok(c.unread()),
            Role::Seq(e) => Ok(e.unread()),
            Role::Dgram(d) => Ok(d.rx.front().map_or(0, |m| m.data.len())),
        }
    }
    /// O nome deste socket e o do par (`getsockname`/`getpeername`).
    pub(crate) fn names(&self) -> (Option<Vec<u8>>, Option<Vec<u8>>, bool) {
        let st = self.st.lock();
        let connected = match &st.role {
            Role::Stream(_) | Role::Seq(_) => true,
            Role::Dgram(d) => d.peer.is_some(),
            _ => false,
        };
        let peer = match &st.role {
            Role::Dgram(d) => d.peer.as_ref().and_then(Weak::upgrade).and_then(|p| p.st.lock().name.clone()),
            _ => st.peer_name.clone(),
        };
        (st.name.clone(), peer, connected)
    }

    /// O socket está em escuta (`SO_ACCEPTCONN`).
    pub(crate) fn listening(&self) -> bool {
        matches!(self.st.lock().role, Role::Listening(_))
    }

    /// A conexão de um socket de fluxo conectado.
    pub(crate) fn conn(&self) -> Option<Arc<Conn>> {
        match &self.st.lock().role {
            Role::Stream(c) => Some(c.clone()),
            _ => None,
        }
    }

    /// A ponta de um socket `SOCK_SEQPACKET` conectado.
    pub(crate) fn seq(&self) -> Option<Arc<SeqEnd>> {
        match &self.st.lock().role {
            Role::Seq(e) => Some(e.clone()),
            _ => None,
        }
    }

    /// `shutdown`: num socket conectado, de fluxo ou de seqpacket, encerra os sentidos pedidos. Sem conexão o
    /// `unix_shutdown` só marca o `sk_shutdown` e devolve 0 (num fluxo, num seqpacket ou num datagrama).
    pub(crate) fn shutdown(&self, read: bool, write: bool) -> Result<(), Errno> {
        if let Some(c) = self.conn() {
            c.shutdown(read, write);
        } else if let Some(e) = self.seq() {
            e.shutdown(read, write);
        }
        Ok(())
    }

    /// `listen`: o socket precisa ter nome (sem autobind, como o `unix_listen` exige). Num socket já em
    /// escuta só troca o tamanho da fila. As credenciais de quem chama ficam como as do "par" do socket
    /// (`update_peercred`), que o `connect` copia para o cliente.
    pub(crate) fn listen(&self, backlog: u32, cred: Ucred) -> Result<(), Errno> {
        if self.ty == SOCK_DGRAM {
            return Err(Errno::EOPNOTSUPP);
        }
        let mut st = self.st.lock();
        if st.name.is_none() {
            return Err(Errno::EINVAL);
        }
        // `somaxconn` do Debian 13 é 4096.
        let backlog = backlog.min(4096) as usize;
        match &mut st.role {
            Role::Idle => st.role = Role::Listening(Listen { backlog, ..Listen::default() }),
            Role::Listening(l) => l.backlog = backlog,
            _ => return Err(Errno::EINVAL),
        }
        st.peer_cred = Some(cred);
        Ok(())
    }

    /// `SO_PEERCRED`: as credenciais do par, ou as de um socket sem par (pid 0, uid e gid -1).
    pub(crate) fn peer_cred(&self) -> Ucred {
        self.st.lock().peer_cred.unwrap_or(Ucred::UNSET)
    }

    pub(crate) fn passcred(&self) -> bool {
        self.passcred.load(Ordering::Relaxed)
    }

    pub(crate) fn set_passcred(&self, on: bool) {
        self.passcred.store(on, Ordering::Relaxed);
    }

    /// `connect` de fluxo a `target`. ECONNREFUSED se ele não escuta; com a fila cheia, EAGAIN
    /// (`nonblock`) ou espera vaga. `cred` é de quem conecta: o embrião do servidor o guarda como o do par, e o
    /// cliente guarda o de quem escuta (`unix_stream_connect`).
    pub(crate) fn try_connect(
        self: &Arc<Self>,
        target: &Arc<UnixSock>,
        nonblock: bool,
        waiter: &Arc<Parker>,
        mk_pipe: &dyn Fn() -> Arc<Pipe>,
        cred: Ucred,
    ) -> Try<Result<(), Errno>> {
        match &self.st.lock().role {
            Role::Idle => {}
            Role::Stream(_) | Role::Seq(_) => return Try::Ready(Err(Errno::EISCONN)),
            _ => return Try::Ready(Err(Errno::EINVAL)),
        }
        if target.ty != self.ty {
            return Try::Ready(Err(Errno::EPROTOTYPE));
        }
        let mut tst = target.st.lock();
        let listener_name = tst.name.clone();
        let listener_cred = tst.peer_cred;
        let Role::Listening(l) = &mut tst.role else { return Try::Ready(Err(Errno::ECONNREFUSED)) };
        // `unix_recvq_full`: cabe uma conexão além do backlog.
        if l.queue.len() > l.backlog {
            if nonblock {
                l.space.unregister(waiter);
                return Try::Ready(Err(Errno::EAGAIN));
            }
            l.space.register(waiter);
            return Try::Pending;
        }
        l.space.unregister(waiter);
        let Some(table) = target.table.upgrade() else { return Try::Ready(Err(Errno::ECONNREFUSED)) };
        let my_name = self.st.lock().name.clone();
        let (client_role, server_role) = if self.ty == SOCK_SEQPACKET {
            let (client, server) = seq_pair();
            (Role::Seq(Arc::new(client)), Role::Seq(Arc::new(server)))
        } else {
            let (client, server) = crate::net::conn_pair(mk_pipe);
            (Role::Stream(Arc::new(client)), Role::Stream(Arc::new(server)))
        };
        let embryo = table.add(self.ty, mk_pipe(), server_role, listener_name.clone(), my_name, false);
        embryo.st.lock().peer_cred = Some(cred);
        l.queue.push_back(embryo);
        let wake = l.wait.take_key(crate::park::key::READ);
        drop(tst);
        {
            let mut st = self.st.lock();
            st.role = client_role;
            st.peer_name = listener_name;
            st.peer_cred = listener_cred;
        }
        wake.run();
        Try::Ready(Ok(()))
    }

    /// `accept`: a próxima conexão da fila, que ganha o inode na tabela.
    pub(crate) fn try_accept(&self, nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<Arc<UnixSock>, Errno>> {
        let mut st = self.st.lock();
        let Role::Listening(l) = &mut st.role else { return Try::Ready(Err(Errno::EINVAL)) };
        if let Some(c) = l.queue.pop_front() {
            l.wait.unregister(waiter);
            let wake = l.space.take();
            drop(st);
            c.st.lock().accepted = true;
            wake.run();
            return Try::Ready(Ok(c));
        }
        if nonblock {
            l.wait.unregister(waiter);
            return Try::Ready(Err(Errno::EAGAIN));
        }
        l.wait.register(waiter);
        Try::Pending
    }

    /// `connect` de datagrama: o destino padrão do `send`. EPROTOTYPE se `target` não é de datagrama.
    pub(crate) fn dgram_connect(self: &Arc<Self>, target: &Arc<UnixSock>) -> Result<(), Errno> {
        if target.ty != SOCK_DGRAM {
            return Err(Errno::EPROTOTYPE);
        }
        let mut st = self.st.lock();
        let Role::Dgram(d) = &mut st.role else { return Err(Errno::EINVAL) };
        d.peer = Some(Arc::downgrade(target));
        Ok(())
    }

    /// `sendto` de datagrama para `target` (ou para o par do `connect`). ENOTCONN sem destino;
    /// ECONNREFUSED se o par fechou; EPERM se o destino está conectado a outro socket. `scm` viaja com a mensagem.
    pub(crate) fn try_send(self: &Arc<Self>, data: &[u8], scm: &Scm, target: Option<&Arc<UnixSock>>, nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<usize, Errno>> {
        let (me_name, peer) = {
            let st = self.st.lock();
            let Role::Dgram(d) = &st.role else { return Try::Ready(Err(Errno::EOPNOTSUPP)) };
            (st.name.clone(), d.peer.clone())
        };
        let dest = match target {
            Some(t) => t.clone(),
            None => match peer {
                None => return Try::Ready(Err(Errno::ENOTCONN)),
                Some(w) => match w.upgrade() {
                    Some(p) => p,
                    None => return Try::Ready(Err(Errno::ECONNREFUSED)),
                },
            },
        };
        if dest.ty != SOCK_DGRAM {
            return Try::Ready(Err(Errno::EPROTOTYPE));
        }
        let mut dst = dest.st.lock();
        let Role::Dgram(d) = &mut dst.role else { return Try::Ready(Err(Errno::ECONNREFUSED)) };
        if let Some(p) = &d.peer
            && !std::ptr::eq(p.as_ptr(), Arc::as_ptr(self))
        {
            return Try::Ready(Err(Errno::EPERM));
        }
        if d.rx.len() >= MAX_DGRAM_QLEN && !Arc::ptr_eq(&dest, self) {
            if nonblock {
                d.space.unregister(waiter);
                return Try::Ready(Err(Errno::EAGAIN));
            }
            d.space.register(waiter);
            return Try::Pending;
        }
        d.space.unregister(waiter);
        d.rx.push_back(Datagram { data: data.to_vec(), from: me_name, scm: scm.clone() });
        let wake = d.wait.take_key(crate::park::key::READ);
        drop(dst);
        wake.run();
        Try::Ready(Ok(data.len()))
    }

    /// `recvfrom` de datagrama: a próxima mensagem inteira, o nome de quem enviou e os dados auxiliares.
    pub(crate) fn try_recv(&self, peek: bool, nonblock: bool, waiter: &Arc<Parker>) -> Try<Result<Datagram, Errno>> {
        let mut st = self.st.lock();
        let Role::Dgram(d) = &mut st.role else { return Try::Ready(Err(Errno::EOPNOTSUPP)) };
        let got = if peek { d.rx.front().cloned() } else { d.rx.pop_front() };
        if let Some(m) = got {
            d.wait.unregister(waiter);
            let wake = d.space.take();
            drop(st);
            wake.run();
            return Try::Ready(Ok(m));
        }
        if nonblock {
            d.wait.unregister(waiter);
            return Try::Ready(Err(Errno::EAGAIN));
        }
        d.wait.register(waiter);
        Try::Pending
    }

    pub(crate) fn poll(&self, waiter: Option<&Arc<Parker>>) -> PollEvents {
        let conn = {
            let mut st = self.st.lock();
            match &mut st.role {
                // Um socket de fluxo sem conexão: só HUP, como o `unix_poll`.
                Role::Idle => return PollEvents::OUT | PollEvents::HUP,
                Role::Listening(l) => {
                    if let Some(w) = waiter {
                        l.wait.register(w);
                    }
                    return if l.queue.is_empty() { PollEvents::empty() } else { PollEvents::IN };
                }
                Role::Dgram(d) => {
                    if let Some(w) = waiter {
                        d.wait.register(w);
                    }
                    let mut ev = PollEvents::OUT;
                    if !d.rx.is_empty() {
                        ev |= PollEvents::IN;
                    }
                    return ev;
                }
                Role::Stream(c) => c.clone(),
                Role::Seq(e) => {
                    let e = e.clone();
                    drop(st);
                    return e.poll(waiter);
                }
            }
        };
        conn.poll(waiter)
    }

    pub(crate) fn unregister(&self, waiter: &Arc<Parker>) {
        let seq = self.seq();
        if let Some(e) = &seq {
            e.unregister(waiter);
        }
        let conn = {
            let mut st = self.st.lock();
            match &mut st.role {
                Role::Listening(l) => {
                    l.wait.unregister(waiter);
                    l.space.unregister(waiter);
                    None
                }
                Role::Dgram(d) => {
                    d.wait.unregister(waiter);
                    d.space.unregister(waiter);
                    None
                }
                Role::Stream(c) => Some(c.clone()),
                Role::Idle | Role::Seq(_) => None,
            }
        };
        if let Some(c) = conn {
            c.unregister(waiter);
        }
    }
}
