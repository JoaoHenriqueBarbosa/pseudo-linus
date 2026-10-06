"""socket do sandbox: sem rede. O módulo importa e expõe constantes e exceções, mas toda tentativa de
abrir conexão falha com o erro que um host sem rota devolveria (`ENETUNREACH`)."""
import errno as _errno
import os as _os

AF_UNSPEC = 0
AF_UNIX = 1
AF_INET = 2
AF_INET6 = 10
SOCK_STREAM = 1
SOCK_DGRAM = 2
SOCK_RAW = 3
SOL_SOCKET = 1
SOL_TCP = 6
SO_REUSEADDR = 2
SO_KEEPALIVE = 9
SO_BROADCAST = 6
SO_RCVBUF = 8
SO_SNDBUF = 7
IPPROTO_IP = 0
IPPROTO_TCP = 6
IPPROTO_UDP = 17
IPPROTO_IPV6 = 41
TCP_NODELAY = 1
SOMAXCONN = 4096
SHUT_RD = 0
SHUT_WR = 1
SHUT_RDWR = 2
AI_PASSIVE = 1
AI_CANONNAME = 2
AI_NUMERICHOST = 4
MSG_PEEK = 2
MSG_WAITALL = 256
EAI_NONAME = -2
has_ipv6 = True
_GLOBAL_DEFAULT_TIMEOUT = object()

error = OSError


class herror(OSError):
    pass


class gaierror(OSError):
    pass


timeout = TimeoutError

_defaulttimeout = None


def getdefaulttimeout():
    return _defaulttimeout


def setdefaulttimeout(value):
    global _defaulttimeout
    _defaulttimeout = value


def _unreachable(*args):
    raise OSError(_errno.ENETUNREACH, 'Network is unreachable')


class socket:
    def __init__(self, family=AF_INET, type=SOCK_STREAM, proto=0, fileno=None):
        self.family = family
        self.type = type
        self.proto = proto
        self._timeout = _defaulttimeout
        self._closed = False

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    def close(self):
        self._closed = True

    def settimeout(self, value):
        self._timeout = value

    def gettimeout(self):
        return self._timeout

    def setblocking(self, flag):
        self._timeout = None if flag else 0.0

    def setsockopt(self, *args):
        pass

    def getsockopt(self, *args):
        return 0

    def fileno(self):
        return -1

    def detach(self):
        self._closed = True
        return -1

    def shutdown(self, how):
        pass

    connect = _unreachable
    connect_ex = lambda self, address: _errno.ENETUNREACH
    bind = _unreachable
    listen = _unreachable
    accept = _unreachable
    send = _unreachable
    sendall = _unreachable
    sendto = _unreachable
    recv = _unreachable
    recv_into = _unreachable
    recvfrom = _unreachable

    def makefile(self, *args, **kwargs):
        raise OSError(_errno.ENOTCONN, 'Transport endpoint is not connected')


SocketType = socket


def create_connection(address, timeout=None, source_address=None, *, all_errors=False):
    raise OSError(_errno.ENETUNREACH, 'Network is unreachable')


def create_server(address, *, family=AF_INET, backlog=None, reuse_port=False, dualstack_ipv6=False):
    raise OSError(_errno.ENETUNREACH, 'Network is unreachable')


def gethostname():
    try:
        with open('/etc/hostname') as f:
            return f.read().strip() or 'localhost'
    except OSError:
        return 'localhost'


def gethostbyname(name):
    if name in ('localhost', gethostname()):
        return '127.0.0.1'
    parts = name.split('.')
    if len(parts) == 4 and all(p.isdigit() and 0 <= int(p) <= 255 for p in parts):
        return name
    raise gaierror(-3, 'Temporary failure in name resolution')


def gethostbyname_ex(name):
    return (name, [], [gethostbyname(name)])


def getaddrinfo(host, port, family=0, type=0, proto=0, flags=0):
    ip = gethostbyname(host) if host else '127.0.0.1'
    port = int(port) if port not in (None, '') else 0
    return [(AF_INET, type or SOCK_STREAM, proto, '', (ip, port))]


def getfqdn(name=''):
    return name or gethostname()


def inet_aton(text):
    parts = text.split('.')
    if len(parts) != 4 or not all(p.isdigit() and 0 <= int(p) <= 255 for p in parts):
        raise OSError('illegal IP address string passed to inet_aton')
    return bytes(int(p) for p in parts)


def inet_ntoa(packed):
    if len(packed) != 4:
        raise OSError('packed IP wrong length for inet_ntoa')
    return '.'.join(str(b) for b in packed)


def htons(x):
    return ((x & 0xff) << 8) | ((x >> 8) & 0xff)


ntohs = htons


def htonl(x):
    return int.from_bytes(x.to_bytes(4, 'big'), 'little')


ntohl = htonl


def socketpair(*args, **kwargs):
    raise OSError(_errno.EAFNOSUPPORT, 'Address family not supported by protocol')
