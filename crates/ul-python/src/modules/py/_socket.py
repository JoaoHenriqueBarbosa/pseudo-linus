"""_socket do sandbox: a API de baixo nível do CPython sobre a rede em loopback de `_net`.

Cada socket tem um descritor inteiro numa tabela própria, para o `socket.py` oficial (`accept`, `dup`,
`fromfd`, `socketpair`, `makefile`) funcionar por cima sem mudança."""

import errno as _errno

import _net

AF_UNSPEC = 0
AF_UNIX = 1
AF_INET = 2
AF_INET6 = 10
AF_NETLINK = 16
AF_PACKET = 17
SOCK_STREAM = 1
SOCK_DGRAM = 2
SOCK_RAW = 3
SOCK_RDM = 4
SOCK_SEQPACKET = 5
SOCK_NONBLOCK = 2048
SOCK_CLOEXEC = 524288
SOL_SOCKET = 1
SOL_IP = 0
SOL_TCP = 6
SOL_UDP = 17
SO_DEBUG = 1
SO_REUSEADDR = 2
SO_TYPE = 3
SO_ERROR = 4
SO_DONTROUTE = 5
SO_BROADCAST = 6
SO_SNDBUF = 7
SO_RCVBUF = 8
SO_KEEPALIVE = 9
SO_OOBINLINE = 10
SO_LINGER = 13
SO_REUSEPORT = 15
SO_RCVLOWAT = 18
SO_SNDLOWAT = 19
SO_RCVTIMEO = 20
SO_SNDTIMEO = 21
SO_ACCEPTCONN = 30
SO_PROTOCOL = 38
SO_DOMAIN = 39
SOMAXCONN = 4096
IPPROTO_IP = 0
IPPROTO_ICMP = 1
IPPROTO_TCP = 6
IPPROTO_UDP = 17
IPPROTO_IPV6 = 41
IPPROTO_RAW = 255
IPV6_V6ONLY = 26
IP_TOS = 1
IP_TTL = 2
IP_MULTICAST_TTL = 33
IP_MULTICAST_LOOP = 34
IP_ADD_MEMBERSHIP = 35
TCP_NODELAY = 1
TCP_MAXSEG = 2
TCP_CORK = 3
TCP_KEEPIDLE = 4
TCP_KEEPINTVL = 5
TCP_KEEPCNT = 6
TCP_QUICKACK = 12
MSG_OOB = 1
MSG_PEEK = 2
MSG_DONTROUTE = 4
MSG_DONTWAIT = 64
MSG_WAITALL = 256
MSG_NOSIGNAL = 16384
SHUT_RD = 0
SHUT_WR = 1
SHUT_RDWR = 2
AI_PASSIVE = 1
AI_CANONNAME = 2
AI_NUMERICHOST = 4
AI_V4MAPPED = 8
AI_ALL = 16
AI_ADDRCONFIG = 32
AI_NUMERICSERV = 1024
NI_NUMERICHOST = 1
NI_NUMERICSERV = 2
NI_NOFQDN = 4
NI_NAMEREQD = 8
NI_DGRAM = 16
NI_MAXHOST = 1025
NI_MAXSERV = 32
EAI_BADFLAGS = -1
EAI_NONAME = -2
EAI_AGAIN = -3
EAI_FAIL = -4
EAI_FAMILY = -6
EAI_SOCKTYPE = -7
EAI_SERVICE = -8
EAI_MEMORY = -10
EAI_SYSTEM = -11
EAI_OVERFLOW = -12
INADDR_ANY = 0
INADDR_BROADCAST = 0xffffffff
INADDR_LOOPBACK = 0x7f000001
INADDR_NONE = 0xffffffff
has_ipv6 = True
SocketType = None


class error(OSError):
    pass


error = OSError


class herror(OSError):
    pass


class gaierror(OSError):
    pass


timeout = TimeoutError

_default_timeout = [None]
_fds = {}
_next_fd = [100]


def getdefaulttimeout():
    return _default_timeout[0]


def setdefaulttimeout(value):
    _default_timeout[0] = _check_timeout(value)


def _check_timeout(value):
    if value is None:
        return None
    try:
        value = float(value)
    except (TypeError, ValueError):
        raise TypeError('Timeout value must be an int or float') from None
    if value < 0:
        raise ValueError('Timeout value out of range')
    return value


class _State:
    """O que o descritor guarda: o socket de verdade, independente de quantos objetos Python o embrulham."""

    def __init__(self, family, type_, proto):
        self.family = family
        self.type = type_
        self.proto = proto
        self.timeout = _default_timeout[0]
        self.addr = None
        self.port = None
        self.endpoint = None
        self.listener = None
        self.dgram = None
        self.peer = None
        self.options = {}
        self.closed = False
        self.fd = None
        self.refs = 0


def _new_fd(state):
    fd = _next_fd[0]
    _next_fd[0] += 1
    state.fd = fd
    _fds[fd] = state
    return fd


def _wait(cond, timeout, what):
    """Espera `cond()` rodando as threads pendentes (cooperativas); `False` se o prazo vence antes."""
    if cond():
        return True
    if timeout == 0.0:
        return False
    import threading
    return bool(threading._wait_for(cond, timeout, what))


def _would_block():
    return BlockingIOError(_errno.EAGAIN, 'Resource temporarily unavailable')


class socket:
    def __init__(self, family=-1, type=-1, proto=-1, fileno=None):
        if fileno is not None:
            state = _fds.get(fileno)
            if state is None:
                raise OSError(_errno.EBADF, 'Bad file descriptor')
            if family != -1 and family != state.family:
                state.family = family
            self._st = state
            state.refs += 1
            return
        if family == -1:
            family = AF_INET
        if type == -1:
            type = SOCK_STREAM
        if proto == -1:
            proto = 0
        if family not in (AF_UNIX, AF_INET, AF_INET6):
            raise OSError(_errno.EAFNOSUPPORT, 'Address family not supported by protocol')
        flags = type & (SOCK_NONBLOCK | SOCK_CLOEXEC)
        type &= ~(SOCK_NONBLOCK | SOCK_CLOEXEC)
        if type not in (SOCK_STREAM, SOCK_DGRAM):
            raise OSError(_errno.ESOCKTNOSUPPORT if type else _errno.EINVAL, 'Socket type not supported')
        state = _State(family, type, proto)
        if flags & SOCK_NONBLOCK:
            state.timeout = 0.0
        _new_fd(state)
        state.refs = 1
        self._st = state

    # -- propriedades ---------------------------------------------------------------------------
    @property
    def family(self):
        return self._st.family

    @property
    def type(self):
        return self._st.type

    @property
    def proto(self):
        return self._st.proto

    @property
    def timeout(self):
        return self._st.timeout

    def __repr__(self):
        return '<_socket.socket fd=%d, family=%d, type=%d, proto=%d>' % (
            self.fileno(), self._st.family, self._st.type, self._st.proto)

    def fileno(self):
        st = self._st
        return -1 if st is None or st.closed else st.fd

    def _live(self):
        st = self._st
        if st is None or st.closed:
            raise OSError(_errno.EBADF, 'Bad file descriptor')
        return st

    def detach(self):
        st = self._st
        fd = self.fileno()
        self._st = None
        if st is not None:
            st.refs -= 1
        return fd

    def close(self):
        st = self._st
        if st is None or st.closed:
            return
        st.refs -= 1
        if st.refs > 0:
            self._st = None
            return
        st.closed = True
        _fds.pop(st.fd, None)
        if st.endpoint is not None:
            st.endpoint.close()
        if st.listener is not None:
            st.listener.close()
        if st.dgram is not None:
            st.dgram.close()
        if st.port is not None and st.dgram is None and st.endpoint is None:
            _net.release_port(st.port)

    # -- timeouts e opções ----------------------------------------------------------------------
    def settimeout(self, value):
        self._live().timeout = _check_timeout(value)

    def gettimeout(self):
        return self._live().timeout

    def setblocking(self, flag):
        self._live().timeout = None if flag else 0.0

    def getblocking(self):
        return self._live().timeout != 0.0

    def setsockopt(self, level, name, value, optlen=None):
        st = self._live()
        if isinstance(value, (bytes, bytearray)):
            value = int.from_bytes(value, 'little') if value else 0
        st.options[(level, name)] = value

    def getsockopt(self, level, name, buflen=0):
        st = self._live()
        if level == SOL_SOCKET:
            if name == SO_TYPE:
                value = st.type
            elif name == SO_ERROR:
                value = 0
            elif name == SO_ACCEPTCONN:
                value = 1 if st.listener is not None else 0
            elif name == SO_DOMAIN:
                value = st.family
            elif name == SO_PROTOCOL:
                value = st.proto
            elif name in (SO_RCVBUF, SO_SNDBUF):
                value = st.options.get((level, name), 212992)
            else:
                value = st.options.get((level, name), 0)
        else:
            value = st.options.get((level, name), 0)
        if buflen:
            return int(value).to_bytes(buflen, 'little')
        return value

    # -- endereços ------------------------------------------------------------------------------
    def _norm(self, address):
        st = self._st
        if st.family == AF_UNIX:
            if isinstance(address, bytes):
                address = address.decode()
            if not isinstance(address, str):
                raise TypeError('a bytes-like object is required, not %r' % type(address).__name__)
            return address
        if not isinstance(address, tuple):
            raise TypeError('%s address must be tuple, not %s' % (
                'AF_INET6' if st.family == AF_INET6 else 'AF_INET', type(address).__name__))
        if st.family == AF_INET and len(address) != 2:
            raise TypeError('AF_INET address must be a pair (host, port)')
        host, port = address[0], address[1]
        if not isinstance(port, int):
            raise TypeError('an integer is required')
        if not 0 <= port <= 65535:
            raise OverflowError('bind(): port must be 0-65535.')
        if isinstance(host, bytes):
            host = host.decode()
        if not isinstance(host, str):
            raise TypeError('str, bytes or bytearray expected, not %s' % type(host).__name__)
        if host == '':
            host = '0.0.0.0' if st.family == AF_INET else '::'
        elif host == '<broadcast>':
            host = '255.255.255.255'
        elif not _is_numeric(host, st.family):
            host = getaddrinfo(host, port, st.family)[0][4][0]
        return _net.address(st.family, host, port)

    def getsockname(self):
        st = self._live()
        if st.endpoint is not None:
            return st.endpoint.local
        if st.dgram is not None and st.dgram.addr is not None:
            return st.dgram.addr
        if st.addr is not None:
            return st.addr
        if st.family == AF_UNIX:
            return ''
        return _net.address(st.family, '::' if st.family == AF_INET6 else '0.0.0.0', 0)

    def getpeername(self):
        st = self._live()
        if st.endpoint is not None:
            return st.endpoint.peer
        if st.peer is not None:
            return st.peer
        raise OSError(_errno.ENOTCONN, 'Transport endpoint is not connected')

    def bind(self, address):
        st = self._live()
        if st.addr is not None or st.endpoint is not None:
            raise OSError(_errno.EINVAL, 'Invalid argument')
        address = self._norm(address)
        if st.family == AF_UNIX:
            st.addr = address
            return
        host, port = address[0], address[1]
        if not _net.is_local(host):
            raise OSError(_errno.EADDRNOTAVAIL, 'Cannot assign requested address')
        reuse = bool(st.options.get((SOL_SOCKET, SO_REUSEADDR)) or st.options.get((SOL_SOCKET, SO_REUSEPORT)))
        if port == 0:
            port = _net.alloc_port()
        else:
            _net.reserve_port(port, reuse)
        st.port = port
        st.addr = _net.address(st.family, host, port)
        if st.type == SOCK_DGRAM:
            st.dgram = _net.Datagram(st.family)
            st.dgram.bind(st.addr, True)

    def listen(self, backlog=128):
        st = self._live()
        if st.type != SOCK_STREAM or st.endpoint is not None:
            raise OSError(_errno.EOPNOTSUPP if st.type != SOCK_STREAM else _errno.EINVAL, 'Operation not supported')
        if st.listener is not None:
            st.listener.backlog = backlog
            return
        if st.addr is None:
            if st.family == AF_UNIX:
                raise OSError(_errno.EINVAL, 'Invalid argument')
            self.bind(('', 0))
        st.listener = _net.listen(st.family, st.addr if st.family == AF_UNIX else st.addr, backlog)

    def _accept(self):
        st = self._live()
        if st.listener is None:
            raise OSError(_errno.EINVAL, 'Invalid argument')
        listener = st.listener
        if not _wait(listener.readable, st.timeout, 'socket.accept()'):
            if st.timeout == 0.0:
                raise _would_block()
            raise TimeoutError('timed out')
        if not listener.pending:
            raise OSError(_errno.EINVAL, 'Invalid argument')
        endpoint = listener.pending.popleft()
        new = _State(st.family, st.type, st.proto)
        new.endpoint = endpoint
        new.timeout = _default_timeout[0]
        fd = _new_fd(new)
        return fd, endpoint.peer

    def connect(self, address):
        err = self.connect_ex(address)
        if err:
            raise _os_error(err)

    def connect_ex(self, address):
        st = self._live()
        if st.endpoint is not None or st.listener is not None:
            return _errno.EISCONN
        address = self._norm(address)
        if st.type == SOCK_DGRAM:
            if not _net.is_local(address[0]) if st.family != AF_UNIX else False:
                return _errno.ENETUNREACH
            st.peer = address
            return 0
        try:
            st.endpoint = _net.connect(st.family, address)
        except OSError as e:
            return e.errno
        if st.port is not None:
            _net.release_port(st.port)
            st.port = None
        return 0

    # -- transferência --------------------------------------------------------------------------
    def _stream(self):
        st = self._live()
        if st.endpoint is None:
            raise OSError(_errno.ENOTCONN, 'Transport endpoint is not connected')
        return st, st.endpoint

    def recv(self, bufsize, flags=0):
        if bufsize < 0:
            raise ValueError('negative buffersize in recv')
        st = self._live()
        if st.type == SOCK_DGRAM:
            return self.recvfrom(bufsize, flags)[0]
        st, ep = self._stream()
        if not _wait(ep.readable, 0.0 if flags & MSG_DONTWAIT else st.timeout, 'socket.recv()'):
            if st.timeout == 0.0 or flags & MSG_DONTWAIT:
                raise _would_block()
            raise TimeoutError('timed out')
        if flags & MSG_PEEK:
            return bytes(ep.rx[:bufsize])
        return ep.read(bufsize)

    def recv_into(self, buffer, nbytes=0, flags=0):
        view = memoryview(buffer)
        want = nbytes or len(view)
        data = self.recv(want, flags)
        view[:len(data)] = data
        return len(data)

    def recvfrom(self, bufsize, flags=0):
        st = self._live()
        if st.type == SOCK_STREAM:
            data = self.recv(bufsize, flags)
            return data, (st.endpoint.peer if st.endpoint is not None else None)
        if st.dgram is None:
            self.bind(('', 0))
        dg = st.dgram
        if not _wait(dg.readable, 0.0 if flags & MSG_DONTWAIT else st.timeout, 'socket.recvfrom()'):
            if st.timeout == 0.0 or flags & MSG_DONTWAIT:
                raise _would_block()
            raise TimeoutError('timed out')
        data, source = dg.rx.popleft() if not flags & MSG_PEEK else dg.rx[0]
        return data[:bufsize], source

    def recvfrom_into(self, buffer, nbytes=0, flags=0):
        view = memoryview(buffer)
        data, source = self.recvfrom(nbytes or len(view), flags)
        view[:len(data)] = data
        return len(data), source

    def send(self, data, flags=0):
        st = self._live()
        data = bytes(data)
        if st.type == SOCK_DGRAM:
            if st.peer is None:
                raise OSError(_errno.EDESTADDRREQ, 'Destination address required')
            return self.sendto(data, st.peer)
        st, ep = self._stream()
        return ep.write(data)

    def sendall(self, data, flags=0):
        self.send(data, flags)

    def sendto(self, data, *args):
        st = self._live()
        address = args[-1]
        data = bytes(data)
        if st.type == SOCK_STREAM:
            if st.endpoint is None:
                raise OSError(_errno.ENOTCONN, 'Transport endpoint is not connected')
            return st.endpoint.write(data)
        address = self._norm(address)
        if st.dgram is None:
            st.dgram = _net.Datagram(st.family)
            self.bind((_net.loopback_ip(st.family), 0))
        return st.dgram.sendto(data, address)

    def shutdown(self, how):
        st, ep = self._stream()
        if how in (SHUT_WR, SHUT_RDWR):
            ep.shutdown_write()
        if how in (SHUT_RD, SHUT_RDWR):
            ep.shutdown_read()


SocketType = socket


def _os_error(code):
    import os
    return OSError(code, os.strerror(code))


def socketpair(family=AF_UNIX, type=SOCK_STREAM, proto=0):
    if family not in (AF_UNIX, AF_INET, AF_INET6):
        raise OSError(_errno.EAFNOSUPPORT, 'Address family not supported by protocol')
    a_ep, b_ep = _net.pair(AF_UNIX)
    out = []
    for ep in (a_ep, b_ep):
        s = socket(AF_UNIX, type, proto)
        s._st.endpoint = ep
        out.append(s)
    return tuple(out)


def close(fd):
    st = _fds.get(fd)
    if st is None:
        raise OSError(_errno.EBADF, 'Bad file descriptor')
    st.refs = 1
    s = socket(fileno=fd)
    s._st.refs = 1
    s.close()


def dup(fd):
    st = _fds.get(fd)
    if st is None:
        raise OSError(_errno.EBADF, 'Bad file descriptor')
    st.refs += 1
    return fd


# -- nomes e endereços --------------------------------------------------------------------------

def _is_numeric(host, family=AF_UNSPEC):
    if family in (AF_INET, AF_UNSPEC):
        parts = host.split('.')
        if len(parts) == 4 and all(p.isdigit() and 0 <= int(p) <= 255 for p in parts):
            return True
    if family in (AF_INET6, AF_UNSPEC) and ':' in host:
        return True
    return False


def gethostname():
    return _net.hostname()


def sethostname(name):
    raise PermissionError(_errno.EPERM, 'Operation not permitted')


def gethostbyname(name):
    return getaddrinfo(name, None, AF_INET)[0][4][0]


def gethostbyname_ex(name):
    return (name, [], [gethostbyname(name)])


def gethostbyaddr(ip):
    if ip in ('127.0.0.1', '::1') or ip.startswith('127.'):
        return ('localhost', [], [ip])
    raise herror(1, 'Unknown host')


_SERVICES = {'http': 80, 'https': 443, 'ftp': 21, 'ssh': 22, 'telnet': 23, 'smtp': 25, 'domain': 53, 'pop3': 110,
             'imap': 143, 'ntp': 123, 'echo': 7, 'daytime': 13, 'imaps': 993, 'pop3s': 995, 'smtps': 465,
             'submission': 587, 'ldap': 389, 'ldaps': 636, 'mysql': 3306, 'postgresql': 5432, 'www': 80}


def getservbyname(name, protocolname=None):
    try:
        return _SERVICES[name]
    except KeyError:
        raise OSError('service/proto not found') from None


def getservbyport(port, protocolname=None):
    for name, value in _SERVICES.items():
        if value == port and name != 'www':
            return name
    raise OSError('port/proto not found')


def getprotobyname(name):
    try:
        return {'ip': 0, 'icmp': 1, 'tcp': 6, 'udp': 17, 'ipv6': 41}[name]
    except KeyError:
        raise OSError('protocol not found') from None


def getaddrinfo(host, port, family=0, type=0, proto=0, flags=0):
    if isinstance(port, str):
        if port.isdigit():
            port = int(port)
        else:
            try:
                port = getservbyname(port)
            except OSError:
                raise gaierror(EAI_SERVICE, 'Servname not supported for ai_socktype') from None
    elif isinstance(port, bytes):
        port = int(port)
    if port is None:
        port = 0
    if isinstance(host, bytes):
        host = host.decode()
    if host is None:
        addresses = ['::1' if family == AF_INET6 else '127.0.0.1'] if not flags & AI_PASSIVE else \
            ['::' if family == AF_INET6 else '0.0.0.0']
    elif _is_numeric(host, family):
        addresses = [host]
    elif host == '':
        addresses = ['127.0.0.1']
    elif flags & AI_NUMERICHOST:
        raise gaierror(EAI_NONAME, 'Name or service not known')
    elif host == 'localhost' or host == _net.hostname():
        addresses = ['::1', '127.0.0.1'] if family in (0, AF_UNSPEC) else \
            (['::1'] if family == AF_INET6 else ['127.0.0.1'])
        if family in (0, AF_UNSPEC):
            addresses = ['127.0.0.1', '::1']
    elif host.rstrip('.').endswith('.invalid') or host == 'invalid':
        raise gaierror(EAI_NONAME, 'Name or service not known')
    else:
        raise gaierror(EAI_AGAIN, 'Temporary failure in name resolution')
    result = []
    types = [type] if type else [SOCK_STREAM, SOCK_DGRAM]
    for ip in addresses:
        fam = AF_INET6 if ':' in ip else AF_INET
        if family not in (0, AF_UNSPEC) and family != fam:
            continue
        for t in types:
            p = proto or (IPPROTO_TCP if t == SOCK_STREAM else IPPROTO_UDP)
            sockaddr = (ip, port, 0, 0) if fam == AF_INET6 else (ip, port)
            result.append((fam, t, p, 'localhost' if flags & AI_CANONNAME else '', sockaddr))
    if not result:
        raise gaierror(EAI_FAMILY, 'ai_family not supported')
    return result


def getnameinfo(sockaddr, flags):
    host, port = sockaddr[0], sockaddr[1]
    if not flags & NI_NUMERICHOST and (host in ('127.0.0.1', '::1')):
        host = 'localhost'
    service = str(port) if flags & NI_NUMERICSERV else _service_name(port)
    return host, service


def _service_name(port):
    try:
        return getservbyport(port)
    except OSError:
        return str(port)


def inet_aton(text):
    parts = text.split('.')
    if not 1 <= len(parts) <= 4 or not all(p.isdigit() for p in parts):
        raise OSError('illegal IP address string passed to inet_aton')
    nums = [int(p) for p in parts]
    if len(nums) == 4 and all(n <= 255 for n in nums):
        return bytes(nums)
    raise OSError('illegal IP address string passed to inet_aton')


def inet_ntoa(packed):
    if len(packed) != 4:
        raise OSError('packed IP wrong length for inet_ntoa')
    return '.'.join(str(b) for b in packed)


def inet_pton(family, text):
    if family == AF_INET:
        parts = text.split('.')
        if len(parts) != 4 or not all(p.isdigit() and 0 <= int(p) <= 255 for p in parts):
            raise OSError('illegal IP address string passed to inet_pton')
        return bytes(int(p) for p in parts)
    if family == AF_INET6:
        import ipaddress
        try:
            return ipaddress.IPv6Address(text).packed
        except ValueError:
            raise OSError('illegal IP address string passed to inet_pton') from None
    raise OSError(_errno.EAFNOSUPPORT, 'Address family not supported by protocol')


def inet_ntop(family, packed):
    if family == AF_INET:
        if len(packed) != 4:
            raise ValueError('invalid length of packed IP address string')
        return inet_ntoa(packed)
    if family == AF_INET6:
        if len(packed) != 16:
            raise ValueError('invalid length of packed IP address string')
        import ipaddress
        return str(ipaddress.IPv6Address(bytes(packed)))
    raise ValueError('unknown address family %d' % family)


def htons(x):
    if not 0 <= x <= 0xffff:
        raise OverflowError('htons: Python int too large to convert to C 16-bit unsigned integer')
    return ((x & 0xff) << 8) | ((x >> 8) & 0xff)


ntohs = htons


def htonl(x):
    if not 0 <= x <= 0xffffffff:
        raise OverflowError('htonl: Python int too large to convert to C 32-bit unsigned integer')
    return int.from_bytes(x.to_bytes(4, 'big'), 'little')


ntohl = htonl


def if_nameindex():
    return [(1, 'lo')]


def if_nametoindex(name):
    if name == 'lo':
        return 1
    raise OSError(_errno.ENODEV, 'No such device')


def if_indextoname(index):
    if index == 1:
        return 'lo'
    raise OSError(_errno.ENXIO, 'No such device or address')
