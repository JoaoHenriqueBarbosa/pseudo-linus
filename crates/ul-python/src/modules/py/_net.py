"""_net: a rede em loopback do sandbox, inteira dentro do processo.

Não há placa de rede: só se conversa com quem está neste mesmo interpretador (`127.0.0.1`, `::1`, `localhost`,
o nome do host e sockets Unix). Um `Listener` guarda as conexões ainda não aceitas, um `Endpoint` é uma ponta
de uma conexão (os bytes que o par escreveu esperam em `rx`) e um `Datagram` é um socket UDP. Qualquer outro
destino falha com `ENETUNREACH`, como num host sem rota. `socket`, `select` e `asyncio` são camadas sobre isto."""

import collections
import errno

AF_UNIX = 1
AF_INET = 2
AF_INET6 = 10

_listeners = {}
_datagrams = {}
_ports = set()
_next_port = [32768]
_hostname = [None]


def hostname():
    if _hostname[0] is None:
        try:
            with open('/etc/hostname') as f:
                _hostname[0] = f.read().strip() or 'localhost'
        except OSError:
            _hostname[0] = 'localhost'
    return _hostname[0]


def is_local(host):
    if isinstance(host, bytes):
        host = host.decode()
    return host in ('', '0.0.0.0', '127.0.0.1', 'localhost', '::', '::1', '<broadcast>') or \
        host.startswith('127.') or host == hostname()


def loopback_ip(family):
    return '::1' if family == AF_INET6 else '127.0.0.1'


def address(family, host, port):
    return (host, port, 0, 0) if family == AF_INET6 else (host, port)


def alloc_port():
    while True:
        port = _next_port[0]
        _next_port[0] += 1
        if _next_port[0] > 60999:
            _next_port[0] = 32768
        if port not in _ports:
            _ports.add(port)
            return port


def reserve_port(port, reuse=False):
    if port in _ports and not (reuse and port not in _listeners):
        raise OSError(errno.EADDRINUSE, 'Address already in use')
    _ports.add(port)


def release_port(port):
    _ports.discard(port)


class _Notifier:
    def __init__(self):
        self.hooks = []

    def _notify(self):
        for hook in list(self.hooks):
            hook()


class Listener(_Notifier):
    def __init__(self, family, key, addr, backlog):
        _Notifier.__init__(self)
        self.family = family
        self.key = key
        self.addr = addr
        self.backlog = backlog
        self.pending = collections.deque()
        self.closed = False
        self.on_connection = None
        # O fd do kernel em que a mesma porta escuta para os outros processos (só TCP).
        self.kfd = None

    def push(self, endpoint):
        if self.on_connection is not None:
            self.on_connection(endpoint)
        else:
            self.pending.append(endpoint)
            self._notify()

    def _pump(self):
        """Traz as conexões que chegaram pelo kernel."""
        while self.kfd is not None:
            got = _os.tcp_accept(self.kfd)
            if got is None:
                return
            fd, peer_port = got
            ip = loopback_ip(self.family)
            self.push(KernelEndpoint(self.family, fd, address(self.family, ip, self.addr[1]),
                                     address(self.family, ip, peer_port)))

    def readable(self):
        if not self.pending and self.kfd is not None:
            self._pump()
        return bool(self.pending) or self.closed

    def close(self):
        if self.closed:
            return
        self.closed = True
        if self.kfd is not None:
            kfd, self.kfd = self.kfd, None
            _kunregister(kfd)
            _os.close(kfd)
        if _listeners.get(self.key) is self:
            del _listeners[self.key]
        for endpoint in self.pending:
            endpoint.close()
        self.pending.clear()
        self._notify()


def listen(family, addr, backlog, reuse=False, ephemeral=False):
    """Um `Listener` em `addr`. Com `ephemeral` (o `bind` pediu a porta 0), quem escolhe a porta é o kernel,
    que conhece as portas em escuta de todos os processos: o endereço do ouvinte sai com a porta dele."""
    if family == AF_UNIX:
        key = ('u', addr)
        if key in _listeners:
            raise OSError(errno.EADDRINUSE, 'Address already in use')
        kfd = None
    else:
        if not ephemeral and ('t', addr[1]) in _listeners:
            raise OSError(errno.EADDRINUSE, 'Address already in use')
        # A mesma porta escuta também no kernel, para os outros processos do sandbox.
        try:
            kfd, kport = _os.tcp_listen(0 if ephemeral else addr[1], backlog)
        except OSError as e:
            if e.errno != errno.ENOSYS:
                raise
            kfd, kport = None, addr[1]
        if kport != addr[1]:
            release_port(addr[1])
            _ports.add(kport)
            addr = address(family, addr[0], kport)
        key = ('t', kport)
    listener = Listener(family, key, addr, backlog)
    if kfd is not None:
        listener.kfd = kfd
        _kregister(kfd, listener)
    _listeners[key] = listener
    return listener


# ---- conexões com outros processos (kernel) ---------------------------------------------------------
# Um `Listener` TCP escuta também no kernel; quem conecta a uma porta sem ouvinte neste interpretador vai
# ao kernel. Os fds do kernel são não bloqueantes: as esperas passam pelo `threading._wait_for`, que chama
# `_kpoll` quando não há thread cooperativa para rodar.

import _os

_kfds = {}


def _kregister(kfd, obj):
    import threading
    if not _kfds and _kpoll not in threading._pollers:
        threading._pollers.append(_kpoll)
    _kfds[kfd] = obj


def _kunregister(kfd):
    _kfds.pop(kfd, None)
    if not _kfds:
        import threading
        if _kpoll in threading._pollers:
            threading._pollers.remove(_kpoll)


def _kpoll(timeout):
    """Espera até `timeout` por dado, conexão ou EOF em algum fd do kernel e os entrega."""
    if not _kfds:
        return
    for kfd in _os.tcp_poll(list(_kfds), timeout):
        obj = _kfds.get(kfd)
        if obj is not None:
            obj._pump()


class Endpoint(_Notifier):
    """Uma ponta de conexão: o que o par escreve chega em `rx`; `rx_eof` marca que ele não escreverá mais."""

    def __init__(self, family, local, peer):
        _Notifier.__init__(self)
        self.family = family
        self.local = local
        self.peer = peer
        self.peer_ep = None
        self.rx = bytearray()
        self.rx_eof = False
        self.rd_shut = False
        self.wr_shut = False
        self.closed = False
        self.reset = False
        self.port = local[1] if family != AF_UNIX else None

    def readable(self):
        return bool(self.rx) or self.rx_eof or self.rd_shut or self.reset or self.closed

    def write(self, data):
        if self.closed or self.wr_shut:
            raise BrokenPipeError(errno.EPIPE, 'Broken pipe')
        if self.reset:
            raise BrokenPipeError(errno.EPIPE, 'Broken pipe')
        other = self.peer_ep
        if other.closed or other.rd_shut:
            # O primeiro envio a um par que já fechou é aceito; o seguinte encontra o RST.
            self.reset = True
            self._notify()
            return len(data)
        other.rx += data
        other._notify()
        return len(data)

    def read(self, n):
        if self.reset:
            self.reset = False
            raise ConnectionResetError(errno.ECONNRESET, 'Connection reset by peer')
        if self.rd_shut:
            return b''
        data = bytes(self.rx[:n])
        del self.rx[:n]
        return data

    def shutdown_write(self):
        if self.wr_shut:
            return
        self.wr_shut = True
        other = self.peer_ep
        other.rx_eof = True
        other._notify()

    def shutdown_read(self):
        self.rd_shut = True
        self._notify()

    def close(self):
        if self.closed:
            return
        self.closed = True
        self.shutdown_write()
        if self.port is not None:
            release_port(self.port)
        self._notify()


class KernelEndpoint(Endpoint):
    """Uma ponta de conexão com outro processo: os bytes chegam pelo fd do kernel e são trazidos para `rx`
    quando alguém pergunta se há o que ler (`readable`) ou quando o `poll` do escalonador os encontra."""

    def __init__(self, family, kfd, local, peer):
        Endpoint.__init__(self, family, local, peer)
        self.kfd = kfd
        self.port = None
        _kregister(kfd, self)

    def _pump(self):
        while self.kfd is not None and not self.rx_eof:
            try:
                data = _os.tcp_recv(self.kfd, 65536)
            except ConnectionResetError:
                self.reset = True
                break
            if data is None:
                break
            if not data:
                self.rx_eof = True
                _kunregister(self.kfd)
                break
            self.rx += data
        self._notify()

    def readable(self):
        if not self.rx and not self.rx_eof:
            self._pump()
        return Endpoint.readable(self)

    def write(self, data):
        if self.closed or self.wr_shut or self.kfd is None:
            raise BrokenPipeError(errno.EPIPE, 'Broken pipe')
        return _os.tcp_send(self.kfd, data)

    def shutdown_write(self):
        if self.wr_shut:
            return
        self.wr_shut = True
        if self.kfd is not None:
            _os.tcp_shutdown(self.kfd, False, True)

    def close(self):
        if self.closed:
            return
        self.closed = True
        self.wr_shut = True
        if self.kfd is not None:
            kfd, self.kfd = self.kfd, None
            _kunregister(kfd)
            _os.close(kfd)
        self._notify()


def connect(family, addr):
    """Abre uma conexão com um `Listener` local e devolve a ponta do cliente."""
    if family == AF_UNIX:
        key = ('u', addr)
        listener = _listeners.get(key)
        if listener is None:
            raise FileNotFoundError(errno.ENOENT, 'No such file or directory')
        client = Endpoint(family, '', addr)
        server = Endpoint(family, addr, '')
    else:
        host, port = addr[0], addr[1]
        if not is_local(host):
            raise OSError(errno.ENETUNREACH, 'Network is unreachable')
        listener = _listeners.get(('t', port))
        if listener is None or listener.closed:
            # Ninguém escuta neste interpretador: talvez outro processo do sandbox.
            try:
                kfd, cport = _os.tcp_connect(port)
            except OSError as e:
                if e.errno == errno.ENOSYS:
                    raise ConnectionRefusedError(errno.ECONNREFUSED, 'Connection refused') from None
                raise
            ip = loopback_ip(family)
            return KernelEndpoint(family, kfd, address(family, ip, cport), address(family, ip, port))
        ip = loopback_ip(family)
        cport = alloc_port()
        client = Endpoint(family, address(family, ip, cport), address(family, ip, port))
        server = Endpoint(family, address(family, ip, port), address(family, ip, cport))
        server.port = None
    client.peer_ep = server
    server.peer_ep = client
    listener.push(server)
    return client


def pair(family=AF_UNIX):
    """Duas pontas já ligadas entre si (`socketpair`)."""
    a = Endpoint(family, '' if family == AF_UNIX else address(family, loopback_ip(family), 0), '')
    b = Endpoint(family, '' if family == AF_UNIX else address(family, loopback_ip(family), 0), '')
    a.peer_ep = b
    b.peer_ep = a
    a.port = b.port = None
    return a, b


class Datagram(_Notifier):
    """Socket UDP: as mensagens recebidas ficam em `rx` com o endereço de quem enviou."""

    def __init__(self, family):
        _Notifier.__init__(self)
        self.family = family
        self.addr = None
        self.rx = collections.deque()
        self.closed = False

    def readable(self):
        return bool(self.rx) or self.closed

    def bind(self, addr, reuse=False):
        port = addr[1]
        if port == 0:
            port = alloc_port()
        else:
            reserve_port(port, reuse)
        ip = addr[0] if addr[0] not in ('', None) else ('::' if self.family == AF_INET6 else '0.0.0.0')
        self.addr = address(self.family, ip, port)
        _datagrams[port] = self
        return self.addr

    def sendto(self, data, addr):
        if self.addr is None:
            self.bind((loopback_ip(self.family), 0))
        host, port = addr[0], addr[1]
        if not is_local(host):
            raise OSError(errno.ENETUNREACH, 'Network is unreachable')
        dest = _datagrams.get(port)
        if dest is not None and not dest.closed:
            source_ip = loopback_ip(self.family)
            dest.rx.append((bytes(data), address(self.family, source_ip, self.addr[1])))
            dest._notify()
        return len(data)

    def close(self):
        if self.closed:
            return
        self.closed = True
        if self.addr is not None:
            if _datagrams.get(self.addr[1]) is self:
                del _datagrams[self.addr[1]]
            release_port(self.addr[1])
        self._notify()
