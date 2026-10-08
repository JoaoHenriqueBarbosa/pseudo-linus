"""_socket do sandbox: a API de baixo nível do CPython sobre os sockets do kernel do sandbox.

Todo socket é um fd do kernel desde a criação (`_os.tcp_socket`, `_os.udp_socket`, `_os.unix_socket`,
`_os.unix_socketpair`): o número do fd, a conexão, o erro pendente (`SO_ERROR`), o `connect` em andamento e as
opções vivem lá, como no Linux, e este módulo só chama o kernel. O que é do objeto Python, como no CPython, é a
família, o tipo, o protocolo e o timeout (que liga `O_NONBLOCK` no fd e decide se a espera passa pelo `poll`)."""

import errno as _errno
import time as _time

import _net
import _os

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
SO_PASSCRED = 16
SO_PEERCRED = 17
SO_RCVLOWAT = 18
SO_SNDLOWAT = 19
SO_RCVTIMEO = 20
SO_SNDTIMEO = 21
SO_ACCEPTCONN = 30
SO_PEERSEC = 31
SO_PASSSEC = 34
SO_PROTOCOL = 38
SO_DOMAIN = 39
SCM_RIGHTS = 1
SCM_CREDENTIALS = 2
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
MSG_CTRUNC = 8
MSG_TRUNC = 32
MSG_DONTWAIT = 64
MSG_EOR = 128
MSG_WAITALL = 256
MSG_CONFIRM = 2048
MSG_ERRQUEUE = 8192
MSG_NOSIGNAL = 16384
MSG_MORE = 32768
MSG_FASTOPEN = 0x20000000
MSG_CMSG_CLOEXEC = 0x40000000
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
INADDR_ALLHOSTS_GROUP = 0xe0000001
INADDR_MAX_LOCAL_GROUP = 0xe00000ff
INADDR_UNSPEC_GROUP = 0xe0000000
EAI_NODATA = -5
EAI_ADDRFAMILY = -9
NI_IDN = 32
# Famílias que o Linux 6.12 numera e que o `socketmodule.c` do Debian exporta.
AF_ROUTE = 16
AF_AX25 = 3
AF_IPX = 4
AF_APPLETALK = 5
AF_NETROM = 6
AF_BRIDGE = 7
AF_ATMPVC = 8
AF_X25 = 9
AF_ROSE = 11
AF_DECnet = 12
AF_NETBEUI = 13
AF_SECURITY = 14
AF_KEY = 15
AF_ASH = 18
AF_ECONET = 19
AF_ATMSVC = 20
AF_RDS = 21
AF_SNA = 22
AF_IRDA = 23
AF_PPPOX = 24
AF_WANPIPE = 25
AF_LLC = 26
AF_CAN = 29
AF_TIPC = 30
AF_BLUETOOTH = 31
AF_ALG = 38
AF_VSOCK = 40
AF_QIPCRTR = 42
PF_CAN = 29
PF_PACKET = 17
PF_RDS = 21
ALG_SET_KEY = 1
ALG_SET_IV = 2
ALG_SET_OP = 3
ALG_SET_AEAD_ASSOCLEN = 4
ALG_SET_AEAD_AUTHSIZE = 5
ALG_SET_PUBKEY = 6
ALG_OP_DECRYPT = 0
ALG_OP_ENCRYPT = 1
ALG_OP_SIGN = 2
ALG_OP_VERIFY = 3
SOL_ALG = 279
SOL_RDS = 276
SOL_TIPC = 271
SOL_HCI = 0
SOL_CAN_BASE = 100
SOL_CAN_RAW = 101
BDADDR_ANY = '00:00:00:00:00:00'
BDADDR_LOCAL = '00:00:00:FF:FF:FF'
BTPROTO_L2CAP = 0
BTPROTO_HCI = 1
BTPROTO_SCO = 2
BTPROTO_RFCOMM = 3
HCI_DATA_DIR = 1
HCI_FILTER = 2
HCI_TIME_STAMP = 3
CAN_RAW = 1
CAN_BCM = 2
CAN_ISOTP = 6
CAN_J1939 = 7
CAN_EFF_FLAG = 0x80000000
CAN_RTR_FLAG = 0x40000000
CAN_ERR_FLAG = 0x20000000
CAN_SFF_MASK = 0x7ff
CAN_EFF_MASK = 0x1fffffff
CAN_ERR_MASK = 0x1fffffff
CAN_RAW_FILTER = 1
CAN_RAW_ERR_FILTER = 2
CAN_RAW_LOOPBACK = 3
CAN_RAW_RECV_OWN_MSGS = 4
CAN_RAW_FD_FRAMES = 5
CAN_RAW_JOIN_FILTERS = 6
CAN_BCM_TX_SETUP = 1
CAN_BCM_TX_DELETE = 2
CAN_BCM_TX_READ = 3
CAN_BCM_TX_SEND = 4
CAN_BCM_RX_SETUP = 5
CAN_BCM_RX_DELETE = 6
CAN_BCM_RX_READ = 7
CAN_BCM_TX_STATUS = 8
CAN_BCM_TX_EXPIRED = 9
CAN_BCM_RX_STATUS = 10
CAN_BCM_RX_TIMEOUT = 11
CAN_BCM_RX_CHANGED = 12
CAN_BCM_SETTIMER = 0x0001
CAN_BCM_STARTTIMER = 0x0002
CAN_BCM_TX_COUNTEVT = 0x0004
CAN_BCM_TX_ANNOUNCE = 0x0008
CAN_BCM_TX_CP_CAN_ID = 0x0010
CAN_BCM_RX_FILTER_ID = 0x0020
CAN_BCM_RX_CHECK_DLC = 0x0040
CAN_BCM_RX_NO_AUTOTIMER = 0x0080
CAN_BCM_RX_ANNOUNCE_RESUME = 0x0100
CAN_BCM_TX_RESET_MULTI_IDX = 0x0200
CAN_BCM_RX_RTR_FRAME = 0x0400
CAN_BCM_CAN_FD_FRAME = 0x0800
J1939_MAX_UNICAST_ADDR = 0xfd
J1939_IDLE_ADDR = 0xfe
J1939_NO_ADDR = 0xff
J1939_NO_NAME = 0
J1939_PGN_REQUEST = 0x0ea00
J1939_PGN_ADDRESS_CLAIMED = 0x0ee00
J1939_PGN_ADDRESS_COMMANDED = 0x0fed8
J1939_PGN_PDU1_MAX = 0x3ff00
J1939_PGN_MAX = 0x3ffff
J1939_NO_PGN = 0x40000
J1939_FILTER_MAX = 512
SO_J1939_FILTER = 1
SO_J1939_PROMISC = 2
SO_J1939_SEND_PRIO = 3
SO_J1939_ERRQUEUE = 4
SCM_J1939_DEST_ADDR = 1
SCM_J1939_DEST_NAME = 2
SCM_J1939_PRIO = 3
SCM_J1939_ERRQUEUE = 4
J1939_NLA_PAD = 0
J1939_NLA_BYTES_ACKED = 1
J1939_EE_INFO_NONE = 0
J1939_EE_INFO_TX_ABORT = 1
ETHERTYPE_ARP = 0x0806
ETHERTYPE_IP = 0x0800
ETHERTYPE_IPV6 = 0x86dd
ETHERTYPE_VLAN = 0x8100
ETH_P_ALL = 3
IOCTL_VM_SOCKETS_GET_LOCAL_CID = 0x7b9
VMADDR_CID_ANY = 0xffffffff
VMADDR_CID_HOST = 2
VMADDR_PORT_ANY = 0xffffffff
VM_SOCKETS_INVALID_VERSION = 0xffffffff
SO_VM_SOCKETS_BUFFER_SIZE = 0
SO_VM_SOCKETS_BUFFER_MIN_SIZE = 1
SO_VM_SOCKETS_BUFFER_MAX_SIZE = 2
IP_OPTIONS = 4
IP_HDRINCL = 3
IP_RECVOPTS = 6
IP_RETOPTS = 7
IP_RECVRETOPTS = 7
IP_PKTINFO = 8
IP_RECVTOS = 13
IP_TRANSPARENT = 19
IP_BIND_ADDRESS_NO_PORT = 24
IP_MULTICAST_IF = 32
IP_DROP_MEMBERSHIP = 36
IP_UNBLOCK_SOURCE = 37
IP_BLOCK_SOURCE = 38
IP_ADD_SOURCE_MEMBERSHIP = 39
IP_DROP_SOURCE_MEMBERSHIP = 40
IP_DEFAULT_MULTICAST_TTL = 1
IP_DEFAULT_MULTICAST_LOOP = 1
IP_MAX_MEMBERSHIPS = 20
IPPORT_RESERVED = 1024
IPPORT_USERRESERVED = 5000
IPPROTO_HOPOPTS = 0
IPPROTO_IGMP = 2
IPPROTO_IPIP = 4
IPPROTO_EGP = 8
IPPROTO_PUP = 12
IPPROTO_IDP = 22
IPPROTO_TP = 29
IPPROTO_ROUTING = 43
IPPROTO_FRAGMENT = 44
IPPROTO_RSVP = 46
IPPROTO_GRE = 47
IPPROTO_ESP = 50
IPPROTO_AH = 51
IPPROTO_ICMPV6 = 58
IPPROTO_NONE = 59
IPPROTO_DSTOPTS = 60
IPPROTO_PIM = 103
IPPROTO_SCTP = 132
IPPROTO_UDPLITE = 136
IPPROTO_MPTCP = 262
IPV6_CHECKSUM = 7
IPV6_NEXTHOP = 9
IPV6_UNICAST_HOPS = 16
IPV6_MULTICAST_IF = 17
IPV6_MULTICAST_HOPS = 18
IPV6_MULTICAST_LOOP = 19
IPV6_JOIN_GROUP = 20
IPV6_LEAVE_GROUP = 21
IPV6_RECVPKTINFO = 49
IPV6_PKTINFO = 50
IPV6_RECVHOPLIMIT = 51
IPV6_HOPLIMIT = 52
IPV6_RECVHOPOPTS = 53
IPV6_HOPOPTS = 54
IPV6_RTHDRDSTOPTS = 55
IPV6_RECVRTHDR = 56
IPV6_RTHDR = 57
IPV6_RECVDSTOPTS = 58
IPV6_DSTOPTS = 59
IPV6_RECVPATHMTU = 60
IPV6_PATHMTU = 61
IPV6_DONTFRAG = 62
IPV6_RECVTCLASS = 66
IPV6_TCLASS = 67
IPV6_RTHDR_TYPE_0 = 0
NETLINK_ROUTE = 0
NETLINK_USERSOCK = 2
NETLINK_FIREWALL = 3
NETLINK_NFLOG = 5
NETLINK_XFRM = 6
NETLINK_IP6_FW = 13
NETLINK_DNRTMSG = 14
NETLINK_CRYPTO = 21
PACKET_HOST = 0
PACKET_BROADCAST = 1
PACKET_MULTICAST = 2
PACKET_OTHERHOST = 3
PACKET_OUTGOING = 4
PACKET_LOOPBACK = 5
PACKET_FASTROUTE = 6
SO_PRIORITY = 12
SO_BINDTODEVICE = 25
SO_MARK = 36
SO_INCOMING_CPU = 49
SO_BINDTOIFINDEX = 62
TCP_SYNCNT = 7
TCP_LINGER2 = 8
TCP_DEFER_ACCEPT = 9
TCP_WINDOW_CLAMP = 10
TCP_INFO = 11
TCP_CONGESTION = 13
TCP_MD5SIG = 14
TCP_THIN_LINEAR_TIMEOUTS = 16
TCP_THIN_DUPACK = 17
TCP_USER_TIMEOUT = 18
TCP_REPAIR = 19
TCP_REPAIR_QUEUE = 20
TCP_QUEUE_SEQ = 21
TCP_REPAIR_OPTIONS = 22
TCP_FASTOPEN = 23
TCP_TIMESTAMP = 24
TCP_NOTSENT_LOWAT = 25
TCP_CC_INFO = 26
TCP_SAVE_SYN = 27
TCP_SAVED_SYN = 28
TCP_REPAIR_WINDOW = 29
TCP_FASTOPEN_CONNECT = 30
TCP_ULP = 31
TCP_MD5SIG_EXT = 32
TCP_FASTOPEN_KEY = 33
TCP_FASTOPEN_NO_COOKIE = 34
TCP_ZEROCOPY_RECEIVE = 35
TCP_INQ = 36
TCP_TX_DELAY = 37
TIPC_ADDR_NAMESEQ = 1
TIPC_ADDR_NAME = 2
TIPC_ADDR_ID = 3
TIPC_ZONE_SCOPE = 1
TIPC_CLUSTER_SCOPE = 2
TIPC_NODE_SCOPE = 3
TIPC_CFG_SRV = 0
TIPC_TOP_SRV = 1
TIPC_LOW_IMPORTANCE = 0
TIPC_MEDIUM_IMPORTANCE = 1
TIPC_HIGH_IMPORTANCE = 2
TIPC_CRITICAL_IMPORTANCE = 3
TIPC_SUB_PORTS = 1
TIPC_SUB_SERVICE = 2
TIPC_SUB_CANCEL = 4
TIPC_WAIT_FOREVER = -1
TIPC_PUBLISHED = 1
TIPC_WITHDRAWN = 2
TIPC_SUBSCR_TIMEOUT = 125
TIPC_IMPORTANCE = 127
TIPC_SRC_DROPPABLE = 128
TIPC_DEST_DROPPABLE = 129
TIPC_CONN_TIMEOUT = 130
UDPLITE_SEND_CSCOV = 10
UDPLITE_RECV_CSCOV = 11
has_ipv6 = True
SocketType = None

_POLLIN = 1
_POLLOUT = 4


def _capsule_repr(self):
    return '<capsule object "_socket.CAPI" at 0x%x>' % (id(self) & 0xffffffffffff)


# O `PyCapsule` do `_socket.CAPI`: a API de C que o Python entrega a extensões; aqui só o objeto.
# O nome entra pelo `type()` porque atribuir `__name__` à classe não troca o nome do tipo.
CAPI = type('PyCapsule', (), {'__repr__': _capsule_repr, '__module__': 'builtins'})()

error = OSError


class herror(OSError):
    __module__ = 'socket'


class gaierror(OSError):
    __module__ = 'socket'


timeout = TimeoutError

_default_timeout = [None]


def getdefaulttimeout():
    return _default_timeout[0]


def setdefaulttimeout(value):
    _default_timeout[0] = _check_timeout(value)


_PYTIME_MAX = 2 ** 63


def _check_timeout(value):
    """`socket_parse_timeout`: os segundos como o `pytime` do CPython 3.13 os arredonda (para cima, em
    nanossegundos) e como o `gettimeout` os devolve (`_PyTime_AsSecondsDouble`)."""
    if value is None:
        return None
    if isinstance(value, float):
        if value != value:
            raise ValueError('Invalid value NaN (not a number)')
        scaled = value * 1e9
        if not -9.223372036854776e18 <= scaled < 9.223372036854776e18:
            raise OverflowError('timestamp out of range for platform time_t')
        import math
        nanos = math.ceil(scaled)
    else:
        try:
            seconds = _index(value)
        except TypeError:
            raise TypeError("'%s' object cannot be interpreted as an integer" % type(value).__name__) from None
        nanos = seconds * 1000000000
        if not -_PYTIME_MAX <= nanos < _PYTIME_MAX:
            raise OverflowError('timestamp too large to convert to C PyTime_t')
    if nanos < 0:
        raise ValueError('Timeout value out of range')
    return float(nanos // 1000000000) if nanos % 1000000000 == 0 else float(nanos) / 1e9


def _would_block():
    return BlockingIOError(_errno.EAGAIN, 'Resource temporarily unavailable')


def _bad_fd():
    return OSError(_errno.EBADF, 'Bad file descriptor')


def _bad_type(type_):
    """O erro de um tipo de socket sem suporte: `SOCK_MAX` é 11 e acima dele o kernel dá EINVAL (como em 0)."""
    code = _errno.EINVAL if not type_ or type_ >= 11 or type_ < 0 else _errno.ESOCKTNOSUPPORT
    return _os_error(code)


def _pause(what):
    """Cede ao resto do interpretador por 1 ms: a espera de quem não tem um fd em que o `poll` acorde."""
    import threading
    threading._wait_for(lambda: False, 0.001, what)


# -- opções de socket, como o Linux 6.12 (net/core/sock.c, ipv4/tcp.c, ipv4/ip_sockglue.c) -------------
# O valor de cada opção fica no kernel (`_os.sock_setopt`); aqui só a validação e a normalização que o
# `setsockopt` do Linux faz antes de guardá-lo, e os padrões de quem nunca a definiu. `info` é o que
# `_os.sock_info` devolve: `(domínio, tipo, protocolo, em escuta)`.
_HZ = 250
_MEM_MAX = 212992
_MIN_SNDBUF = 4608
_MIN_RCVBUF = 2304
_INT_MAX = 2147483647
_STRUCT_SIZES = {'linger': 8, 'timeval': 16, 'membership': 8}
# O kernel recusa estas duas em socket de fluxo.
_NOT_ON_STREAM = ((SOL_IP, IP_MULTICAST_TTL), (SOL_IP, IP_MULTICAST_LOOP))


def _is_tcp(info):
    return info[1] == SOCK_STREAM and info[0] != AF_UNIX


def _buffer_default(tcp_value):
    return lambda info: tcp_value if _is_tcp(info) else _MEM_MAX


# nome: (tipo, padrão ou função de `info`, mínimo, máximo)
_OPTIONS = {(SOL_SOCKET, n): ('bool', 0, 0, 1) for n in (
    SO_DEBUG, SO_REUSEADDR, SO_DONTROUTE, SO_BROADCAST, SO_KEEPALIVE, SO_OOBINLINE, SO_REUSEPORT, SO_PASSCRED,
    SO_PASSSEC)}
_OPTIONS.update({
    # `struct ucred` (pid, uid, gid) do par; só leitura, e quem o guarda é o kernel.
    (SOL_SOCKET, SO_PEERCRED): ('peercred', bytes(12), 0, 0),
    (SOL_SOCKET, SO_TYPE): ('ro', lambda info: info[1], 0, 0),
    (SOL_SOCKET, SO_ERROR): ('ro', 0, 0, 0),
    (SOL_SOCKET, SO_ACCEPTCONN): ('ro', lambda info: 1 if info[3] else 0, 0, 0),
    (SOL_SOCKET, SO_DOMAIN): ('ro', lambda info: info[0], 0, 0),
    (SOL_SOCKET, SO_PROTOCOL): ('ro', lambda info: info[2], 0, 0),
    (SOL_SOCKET, SO_SNDLOWAT): ('ro', 1, 0, 0),
    (SOL_SOCKET, SO_RCVLOWAT): ('lowat', 1, 0, 0),
    (SOL_SOCKET, SO_SNDBUF): ('sndbuf', _buffer_default(16384), 0, 0),
    (SOL_SOCKET, SO_RCVBUF): ('rcvbuf', _buffer_default(131072), 0, 0),
    (SOL_SOCKET, SO_LINGER): ('linger', bytes(8), 0, 0),
    (SOL_SOCKET, SO_RCVTIMEO): ('timeval', bytes(16), 0, 0),
    (SOL_SOCKET, SO_SNDTIMEO): ('timeval', bytes(16), 0, 0),
    (IPPROTO_TCP, TCP_NODELAY): ('bool', 0, 0, 1),
    (IPPROTO_TCP, TCP_CORK): ('bool', 0, 0, 1),
    (IPPROTO_TCP, TCP_QUICKACK): ('bool', 1, 0, 1),
    (IPPROTO_TCP, TCP_MAXSEG): ('range', 536, 48, 65535),
    (IPPROTO_TCP, TCP_KEEPIDLE): ('range', 7200, 1, 32767),
    (IPPROTO_TCP, TCP_KEEPINTVL): ('range', 75, 1, 32767),
    (IPPROTO_TCP, TCP_KEEPCNT): ('range', 9, 1, 127),
    (IPPROTO_IP, IP_TOS): ('tos', 0, 0, 0),
    (IPPROTO_IP, IP_TTL): ('ttl', 64, 0, 255),
    (IPPROTO_IP, IP_MULTICAST_TTL): ('ttl', 1, 0, 255),
    (IPPROTO_IP, IP_MULTICAST_LOOP): ('bool', 1, 0, 1),
    (IPPROTO_IP, IP_ADD_MEMBERSHIP): ('membership', 0, 0, 0),
    (IPPROTO_IPV6, IPV6_V6ONLY): ('bool', 0, 0, 1),
})


def _no_proto_opt():
    return OSError(_errno.ENOPROTOOPT, 'Protocol not available')


def _check_level(info, level):
    """Quem atende o nível: o kernel devolve ENOPROTOOPT para nível que a família ou o tipo não tem."""
    if level == SOL_SOCKET:
        return
    if info[0] == AF_UNIX:
        raise OSError(_errno.EOPNOTSUPP, 'Operation not supported')
    if level == IPPROTO_IP or (level == IPPROTO_TCP and _is_tcp(info)) or (
            level == IPPROTO_IPV6 and info[0] == AF_INET6):
        return
    raise _no_proto_opt()


def _opt_int(value, one_byte_ok):
    """O inteiro que o kernel lê do valor do setsockopt (int ou os primeiros bytes do buffer)."""
    if isinstance(value, int):
        if value > _INT_MAX:
            raise OverflowError('signed integer is greater than maximum')
        if value < -_INT_MAX - 1:
            raise OverflowError('signed integer is less than minimum')
        return value
    if len(value) >= 4:
        return int.from_bytes(value[:4], 'little', signed=True)
    if one_byte_ok and value:
        return value[0]
    raise OSError(_errno.EINVAL, 'Invalid argument')


def _opt_struct(value, size):
    """O buffer de uma opção com struct: um int tem 4 bytes, curto para qualquer uma."""
    if isinstance(value, int) or len(value) < size:
        raise OSError(_errno.EINVAL, 'Invalid argument')
    return bytes(value[:size])


def _timeval_normalize(raw):
    """O `struct timeval` como o getsockopt o devolve: o kernel guarda em jiffies (HZ=250)."""
    sec = int.from_bytes(raw[:8], 'little', signed=True)
    usec = int.from_bytes(raw[8:16], 'little', signed=True)
    if not 0 <= usec < 1000000:
        raise OSError(_errno.EDOM, 'Numerical argument out of domain')
    jiffies = 0
    if sec > 0 or (sec == 0 and usec):
        if sec < (2 ** 63 - 1) // _HZ - 1:
            jiffies = sec * _HZ + -(-usec // (1000000 // _HZ))
    sec, rest = divmod(jiffies, _HZ)
    return sec.to_bytes(8, 'little') + (rest * (1000000 // _HZ)).to_bytes(8, 'little')


def _stored(fd, level, name, kind):
    """O valor que o kernel guardou para a opção, ou `None` se ninguém a definiu."""
    raw = _os.sock_getopt(fd, level, name)
    if raw is None:
        return None
    return raw if kind in ('linger', 'timeval', 'peercred') else int.from_bytes(raw, 'little', signed=True)


def _store(fd, level, name, value):
    if isinstance(value, int):
        value = value.to_bytes(8, 'little', signed=True)
    _os.sock_setopt(fd, level, name, value)


def _norm_unix(address):
    """O nome de um socket `AF_UNIX`: `str` (caminho), bytes (caminho ou espaço abstrato) ou path-like."""
    if isinstance(address, bytearray):
        address = bytes(address)
    if not isinstance(address, (str, bytes)):
        import os
        try:
            address = os.fspath(address)
        except TypeError:
            raise TypeError('a bytes-like object is required, not %r' % address.__class__.__name__) from None
    return address


# -- dados auxiliares: o que o `socketmodule.c` converte entre Python e o `msg_control` -----------
_SOCKLEN_T_LIMIT = 0x7fffffff
_CMSG_HEADER = 16


def _index(value):
    """O `__index__` que os conversores `n` e `i` do Argument Clinic aplicam."""
    if isinstance(value, int):
        return value
    method = getattr(type(value), '__index__', None)
    if method is None:
        raise TypeError("'%s' object cannot be interpreted as an integer" % type(value).__name__)
    return method(value)


def _c_int(value):
    """O conversor `i`: um inteiro que cabe num `int` de C."""
    value = _index(value)
    if value > _INT_MAX:
        raise OverflowError('signed integer is greater than maximum')
    if value < -_INT_MAX - 1:
        raise OverflowError('signed integer is less than minimum')
    return value


def _as_bytes(item):
    """Os bytes de um objeto de buffer (`y*`): `bytes`, `bytearray`, `memoryview`, `array.array` e quem tem `__buffer__`."""
    if isinstance(item, (bytes, bytearray)):
        return bytes(item)
    if isinstance(item, memoryview) or hasattr(type(item), '__buffer__'):
        return memoryview(item).tobytes()
    tobytes = getattr(type(item), 'tobytes', None)
    if tobytes is None or isinstance(item, str):
        raise TypeError('a bytes-like object is required, not %r' % type(item).__name__)
    return tobytes(item)


def _writable_view(fname, buffer):
    """O formato `w*` (buffer de leitura e escrita), como vista de bytes; senão o `TypeError` do CPython."""
    try:
        view = memoryview(buffer)
    except TypeError:
        view = None
    if view is None or view.readonly:
        raise TypeError('%s() argument 1 must be read-write bytes-like object, not %s' % (
            fname, type(buffer).__name__))
    return view if view.format == 'B' else view.cast('B')


def _sendmsg_parts(buffers):
    try:
        items = list(buffers)
    except TypeError:
        raise TypeError('sendmsg() argument 1 must be an iterable') from None
    return [_as_bytes(item) for item in items]


def _ancdata_items(ancdata):
    """Os itens `(nível, tipo, bytes)` de um `ancdata`, com as conversões do `PyArg_Parse` de `(iiy*)`."""
    try:
        items = list(ancdata)
    except TypeError:
        raise TypeError('sendmsg() argument 2 must be an iterable') from None
    name = '[sendmsg() ancillary data items]'
    out = []
    for item in items:
        try:
            parts = tuple(item)
        except TypeError:
            raise TypeError('%s() argument must be 3-item sequence, not %s' % (name, type(item).__name__)) from None
        if len(parts) != 3:
            raise TypeError('%s() argument must be sequence of length 3, not %d' % (name, len(parts)))
        try:
            data = _as_bytes(parts[2])
        except TypeError:
            raise TypeError('%s() argument 3 must be bytes-like object, not %s' % (name, type(parts[2]).__name__)) from None
        out.append((_c_int(parts[0]), _c_int(parts[1]), data))
    return out


def _pack_ancdata(items):
    """O `msg_control`: cada item com o cabeçalho (`cmsg_len` do `CMSG_LEN`) e o preenchimento do `CMSG_SPACE`."""
    out = bytearray()
    for level, kind, data in items:
        out += (_CMSG_HEADER + len(data)).to_bytes(8, 'little')
        out += level.to_bytes(4, 'little', signed=True) + kind.to_bytes(4, 'little', signed=True)
        out += data + bytes(-len(data) % 8)
    return bytes(out)


def _parse_ancdata(control):
    """Os itens `(nível, tipo, dados)` do `msg_control` de um `recvmsg`; o último pode vir cortado (`MSG_CTRUNC`)."""
    items = []
    at, end = 0, len(control)
    while at + _CMSG_HEADER <= end:
        length = int.from_bytes(control[at:at + 8], 'little')
        if length < _CMSG_HEADER:
            break
        level = int.from_bytes(control[at + 8:at + 12], 'little', signed=True)
        kind = int.from_bytes(control[at + 12:at + 16], 'little', signed=True)
        items.append((level, kind, bytes(control[at + _CMSG_HEADER:min(at + length, end)])))
        at += (length + 7) & ~7
    return items


def _recvmsg_sizes(fname, bufsize, ancbufsize, flags):
    bufsize, ancbufsize, flags = _index(bufsize), _index(ancbufsize), _c_int(flags)
    if bufsize < 0:
        raise ValueError('negative buffer size in %s()' % fname)
    if ancbufsize < 0:
        raise ValueError('invalid ancillary data buffer length')
    if ancbufsize > _SOCKLEN_T_LIMIT - _CMSG_HEADER:
        raise OverflowError('Python int too large to convert to C int')
    return bufsize, ancbufsize, flags


def CMSG_LEN(length, /):
    length = _index(length)
    if length < 0 or length > _SOCKLEN_T_LIMIT - _CMSG_HEADER:
        raise OverflowError('CMSG_LEN() argument out of range')
    return _CMSG_HEADER + length


def CMSG_SPACE(length, /):
    length = _index(length)
    if length < 0 or length > _SOCKLEN_T_LIMIT - (_CMSG_HEADER + 8):
        raise OverflowError('CMSG_SPACE() argument out of range')
    return _CMSG_HEADER + ((length + 7) & ~7)


class socket:
    # O tipo em C não tem `__dict__`: o estado fica em slots, que o VM esconde de `dir`, `vars` e `__dict__`
    # (a classe do módulo `socket` herda daqui e acrescenta os dela).
    __slots__ = ('_fd', '_guard', '_timeout', '_family', '_type', '_proto')

    def __init__(self, family=-1, type=-1, proto=-1, fileno=None):
        family, type, proto = _c_int(family), _c_int(type), _c_int(proto)
        self._fd = -1
        self._guard = None
        self._timeout = _default_timeout[0]
        if fileno is not None:
            if not isinstance(fileno, int):
                raise TypeError("'%s' object cannot be interpreted as an integer" % fileno.__class__.__name__)
            if fileno < 0:
                raise ValueError('negative file descriptor')
            if family == -1 or type == -1 or proto == -1:
                # Como o CPython: o que não foi dito sai do próprio socket (SO_DOMAIN, SO_TYPE, SO_PROTOCOL).
                info = _os.sock_info(fileno)
                if family == -1:
                    family = info[0]
                if type == -1:
                    type = info[1]
                if proto == -1:
                    proto = info[2]
            self._family = family
            self._type = type & ~(SOCK_NONBLOCK | SOCK_CLOEXEC)
            self._proto = proto
            self._adopt(fileno)
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
        if type not in (SOCK_STREAM, SOCK_DGRAM) and not (type == SOCK_SEQPACKET and family == AF_UNIX):
            raise _bad_type(type)
        if family == AF_UNIX:
            fd = _os.unix_socket(type)
        elif type == SOCK_DGRAM:
            fd = _os.udp_socket(family == AF_INET6)
        else:
            fd = _os.tcp_socket(family == AF_INET6)
        self._family = family
        self._type = type
        self._proto = proto
        self._adopt(fd)
        if flags & SOCK_NONBLOCK:
            self._timeout = 0.0
            _os.set_blocking(fd, False)

    def _adopt(self, fd):
        """Passa a ser o dono do fd do kernel: ele fecha com este objeto, como o `sock_dealloc` do CPython."""
        self._fd = fd
        self._guard = _os.fdguard(fd)
        if self._timeout is not None:
            _os.set_blocking(fd, False)

    # -- propriedades ---------------------------------------------------------------------------
    @property
    def family(self):
        return self._family

    @property
    def type(self):
        return self._type

    @property
    def proto(self):
        return self._proto

    @property
    def timeout(self):
        return self._timeout

    def __repr__(self):
        return '<socket object, fd=%d, family=%d, type=%d, proto=%d>' % (
            self._fd, self._family, self._type, self._proto)

    def fileno(self):
        return self._fd

    def _checked(self):
        if self._fd < 0:
            raise _bad_fd()
        return self._fd

    def detach(self):
        fd = self._fd
        guard, self._guard = self._guard, None
        self._fd = -1
        if guard is not None:
            guard.detach()
        return fd

    def close(self):
        guard, self._guard = self._guard, None
        self._fd = -1
        if guard is not None:
            try:
                guard.close()
            except OSError as e:
                if e.errno != _errno.ECONNRESET:
                    raise

    def __del__(self):
        """`sock_finalize`: o que o dono deixou aberto fecha com o objeto; um objeto que nem chegou a ser
        construído não tem o que fechar."""
        try:
            self.close()
        except (OSError, AttributeError):
            pass

    # -- esperas --------------------------------------------------------------------------------
    def _call(self, op, events, what, flags=0, deadline=None):
        """`sock_call_ex` do CPython: sem timeout (fd bloqueante) espera a prontidão pelo `poll`, rodando as
        threads pendentes, e então chama o kernel; com timeout espera o prazo inteiro (ou `deadline`, o prazo
        de um `sendall`), e com 0 chama direto. `MSG_DONTWAIT` num socket bloqueante liga `O_NONBLOCK` só
        durante a chamada."""
        fd = self._checked()
        limit = self._timeout
        if limit == 0.0:
            return op()
        if flags & MSG_DONTWAIT and limit is None:
            _os.set_blocking(fd, False)
            try:
                return op()
            finally:
                _os.set_blocking(fd, True)
        if deadline is None and limit is not None:
            deadline = _time.monotonic() + limit
        while True:
            left = None if deadline is None else deadline - _time.monotonic()
            if left is not None and left <= 0:
                raise TimeoutError('timed out')
            if not _net.wait_fd(fd, events, left, what):
                raise TimeoutError('timed out')
            try:
                return op()
            except BlockingIOError:
                if limit is None:
                    raise

    def _send_call(self, op, what, flags):
        """A escrita de um socket bloqueante não pode parar o interpretador num kernel cheio: o fd fica não
        bloqueante durante a chamada e a espera por lugar roda as outras threads."""
        fd = self._checked()
        if self._timeout is not None or flags & MSG_DONTWAIT:
            return self._call(op, _POLLOUT, what, flags)
        _os.set_blocking(fd, False)
        try:
            while True:
                try:
                    return op()
                except BlockingIOError:
                    if _net.wait_fd(fd, _POLLOUT, 0.001, what):
                        _pause(what)
        finally:
            _os.set_blocking(fd, True)

    # -- timeouts e opções ----------------------------------------------------------------------
    def settimeout(self, value):
        # Como o `sock_settimeout`: o prazo fica guardado antes de o kernel dar (ou negar) a mudança do fd.
        self._timeout = _check_timeout(value)
        _os.set_blocking(self._checked(), value is None)

    def gettimeout(self):
        return self._timeout

    def setblocking(self, flag):
        self.settimeout(None if flag else 0.0)

    def getblocking(self):
        return self._timeout != 0.0

    def setsockopt(self, level, name, value, optlen=None):
        fd = self._checked()
        for arg in (level, name):
            if not isinstance(arg, int):
                raise TypeError("'%s' object cannot be interpreted as an integer" % type(arg).__name__)
        if value is None:
            if optlen is None:
                raise TypeError("a bytes-like object is required, not 'NoneType'")
            raise OSError(_errno.EFAULT, 'Bad address')
        if not isinstance(value, (int, bytes, bytearray)):
            raise TypeError("a bytes-like object is required, not '%s'" % type(value).__name__)
        info = _os.sock_info(fd)
        _check_level(info, level)
        entry = _OPTIONS.get((level, name))
        if entry is None:
            if level == SOL_SOCKET:
                _opt_int(value, False)
            raise _no_proto_opt()
        kind = entry[0]
        if kind in ('ro', 'peercred'):
            raise _no_proto_opt()
        if kind in ('linger', 'timeval', 'membership'):
            raw = _opt_struct(value, _STRUCT_SIZES[kind])
            if kind == 'linger':
                onoff = 1 if int.from_bytes(raw[:4], 'little') else 0
                raw = onoff.to_bytes(4, 'little') + (raw[4:8] if onoff else bytes(4))
            elif kind == 'timeval':
                raw = _timeval_normalize(raw)
            else:
                return
            _store(fd, level, name, raw)
            return
        number = _opt_int(value, level == SOL_IP)
        if (level, name) in _NOT_ON_STREAM and info[1] == SOCK_STREAM:
            raise OSError(_errno.EINVAL, 'Invalid argument')
        if kind == 'bool':
            if name == SO_REUSEPORT and level == SOL_SOCKET and number and info[0] == AF_UNIX:
                raise OSError(_errno.EOPNOTSUPP, 'Operation not supported')
            number = 1 if number else 0
        elif kind == 'range':
            if not entry[2] <= number <= entry[3]:
                raise OSError(_errno.EINVAL, 'Invalid argument')
        elif kind == 'ttl':
            if number == -1:
                number = entry[1]
            elif not 0 <= number <= 255:
                raise OSError(_errno.EINVAL, 'Invalid argument')
        elif kind == 'tos':
            number &= 0xff
            if info[1] == SOCK_STREAM:
                number &= 0xfc
        elif kind == 'sndbuf':
            number = max(min(number, _MEM_MAX) * 2, _MIN_SNDBUF)
        elif kind == 'rcvbuf':
            number = max(min(number, _MEM_MAX) * 2, _MIN_RCVBUF)
        elif kind == 'lowat':
            number = number if number > 0 else (1 if number == 0 else _INT_MAX)
        _store(fd, level, name, number)

    def getsockopt(self, level, name, buflen=None):
        fd = self._checked()
        for arg in (level, name):
            if not isinstance(arg, int):
                raise TypeError("'%s' object cannot be interpreted as an integer" % type(arg).__name__)
        info = _os.sock_info(fd)
        _check_level(info, level)
        entry = _OPTIONS.get((level, name))
        if entry is None or entry[0] == 'membership':
            raise _no_proto_opt()
        if (level, name) == (SOL_SOCKET, SO_ERROR):
            # Ler SO_ERROR consome o erro pendente (sock_error_report do kernel).
            value = _os.sock_error(fd)
        else:
            value = _stored(fd, level, name, entry[0])
            if value is None:
                value = entry[1](info) if callable(entry[1]) else entry[1]
        if buflen is not None and not isinstance(buflen, int):
            raise TypeError("'%s' object cannot be interpreted as an integer" % type(buflen).__name__)
        if not buflen:
            # `buflen` ausente ou 0: o CPython devolve o int da opção.
            if isinstance(value, bytes):
                value = int.from_bytes(value[:4], 'little', signed=True)
            return value
        if not 0 < buflen <= 1024:
            raise OSError('getsockopt buflen out of range')
        if not isinstance(value, bytes):
            # O kernel devolve um byte só quando as opções de IP cabem em um e o pedido é menor que um int.
            if level == SOL_IP and buflen < 4 and 0 <= value <= 255:
                return bytes([value])
            value = (value & 0xffffffff).to_bytes(4, 'little')
        return value[:buflen]

    # -- endereços ------------------------------------------------------------------------------
    def _norm(self, address, caller):
        """O `getsockaddrarg`: o endereço de um `bind`/`connect`/`sendto` já com o IP numérico resolvido."""
        family = self._family
        if family == AF_UNIX:
            return _norm_unix(address)
        label = 'AF_INET6' if family == AF_INET6 else 'AF_INET'
        if not isinstance(address, tuple):
            raise TypeError('%s(): %s address must be tuple, not %s' % (caller, label, type(address).__name__))
        if family == AF_INET and len(address) != 2:
            raise TypeError('AF_INET address must be a pair (host, port)')
        if family == AF_INET6 and not 2 <= len(address) <= 4:
            raise TypeError('AF_INET6 address must be a tuple (host, port[, flowinfo[, scopeid]])')
        host = _host_arg(address[0])
        try:
            port = _c_int(address[1])
        except OverflowError:
            raise OverflowError('%s(): port must be 0-65535.' % caller) from None
        if len(address) > 2:
            flowinfo = _index(address[2])
            if flowinfo < 0:
                raise OverflowError("can't convert negative value to unsigned int")
        ip = _set_ip_address(host, family)
        if not 0 <= port <= 65535:
            raise OverflowError('%s(): port must be 0-65535.' % caller)
        if len(address) > 2 and flowinfo > 0xfffff:
            raise OverflowError('%s(): flowinfo must be 0-1048575.' % caller)
        return _net.address(family, ip, port)

    def _dest(self, host):
        """O IP numérico de um destino local: o endereço de bind genérico vira o loopback, e um `127.x.y.z` (o
        kernel aceita qualquer um deles) segue como está."""
        if host in ('0.0.0.0', '::'):
            return _net.loopback_ip(self._family)
        return host

    def getsockname(self):
        fd = self._checked()
        if self._family == AF_UNIX:
            return _net.unix_name(_os.unix_names(fd)[0])
        if self._type == SOCK_DGRAM:
            ip, port = _os.udp_names(fd)[0]
        else:
            ip, port = _os.tcp_names(fd)[0]
        return _net.address(self._family, ip, port)

    def getpeername(self):
        fd = self._checked()
        if self._family == AF_UNIX:
            _, peer, connected = _os.unix_names(fd)
            if not connected:
                raise OSError(_errno.ENOTCONN, 'Transport endpoint is not connected')
            return _net.unix_name(peer)
        peer = (_os.udp_names(fd) if self._type == SOCK_DGRAM else _os.tcp_names(fd))[1]
        if peer is None:
            raise OSError(_errno.ENOTCONN, 'Transport endpoint is not connected')
        return _net.address(self._family, peer[0], peer[1])

    def _flag(self, *names):
        fd = self._fd
        return any(_stored(fd, SOL_SOCKET, n, 'bool') for n in names)

    def bind(self, address):
        address = self._norm(address, 'bind')
        fd = self._checked()
        if self._family == AF_UNIX:
            _os.unix_bind(fd, _net.unix_encode(address))
            return
        host, port = address[0], address[1]
        # O `inet_bind` aceita o broadcast (`RTN_BROADCAST`) além dos endereços locais.
        if not _net.is_local(host) and host != '255.255.255.255':
            raise OSError(_errno.EADDRNOTAVAIL, 'Cannot assign requested address')
        if self._type == SOCK_DGRAM:
            _os.udp_bind(fd, host, port, self._flag(SO_REUSEADDR, SO_REUSEPORT))
        else:
            _os.tcp_bind_fd(fd, host, port, self._flag(SO_REUSEADDR))

    def listen(self, backlog=128):
        backlog = _c_int(backlog)
        fd = self._checked()
        if self._family == AF_UNIX:
            _os.unix_listen(fd, max(backlog, 0))
            return
        if self._type == SOCK_DGRAM:
            raise OSError(_errno.EOPNOTSUPP, 'Operation not supported')
        me, peer = _os.tcp_names(fd)
        if me[1] == 0 and peer is None:
            # `inet_listen` num socket sem porta: o kernel escolhe uma efêmera (autobind).
            socket.bind(self, ('', 0))
        _os.tcp_listen_bound(fd, max(backlog, 0))

    def _accept(self):
        fd = self._checked()
        if self._type not in (SOCK_STREAM, SOCK_SEQPACKET):
            raise OSError(_errno.EOPNOTSUPP, 'Operation not supported')
        unix = self._family == AF_UNIX

        def op():
            got = _os.unix_accept(fd) if unix else _os.tcp_accept(fd)
            if got is None:
                raise _would_block()
            return got if unix else got[0]

        new = self._call(op, _POLLIN, 'socket.accept()')
        # Só o número sai daqui: o `socket.accept` do socket.py o reabre com `socket(fileno=fd)`.
        if unix:
            return new, _net.unix_name(_os.unix_names(new)[1])
        peer = _os.tcp_names(new)[1]
        return new, _net.address(self._family, peer[0], peer[1])

    def connect(self, address):
        err = self._connect(address, 'connect')
        if err == _TIMED_OUT:
            raise TimeoutError('timed out')
        if err:
            raise _os_error(err)

    def connect_ex(self, address):
        err = self._connect(address, 'connect_ex')
        return _errno.EAGAIN if err == _TIMED_OUT else err

    def _connect(self, address, caller):
        """O errno do `connect` (0 se conectou) ou `_TIMED_OUT`. Num socket com timeout o `connect` não
        bloqueante devolve EINPROGRESS, a espera é pela escrita e o resultado vem de `SO_ERROR`."""
        address = self._norm(address, caller)
        fd = self._checked()
        if self._family == AF_UNIX:
            return self._connect_unix(fd, _net.unix_encode(address))
        host, port = address[0], address[1]
        if not _net.is_local(host):
            return _errno.ENETUNREACH
        host = self._dest(host)
        try:
            if self._type == SOCK_DGRAM:
                _os.udp_connect(fd, host, port)
            else:
                _os.tcp_connect_fd(fd, host, port)
        except OSError as e:
            err = e.errno
        else:
            return 0
        limit = self._timeout
        if err == _errno.EINPROGRESS and limit:
            if not _net.wait_fd(fd, _POLLOUT, limit, 'socket.connect()'):
                return _TIMED_OUT
            return _os.sock_error(fd)
        return err

    def _connect_unix(self, fd, name):
        if self._timeout is None:
            # A fila de quem escuta está cheia: espera vaga, como o connect bloqueante, sem parar as threads.
            _os.set_blocking(fd, False)
            try:
                while True:
                    try:
                        if _os.unix_connect(fd, name):
                            return 0
                    except OSError as e:
                        return e.errno
                    _pause('socket.connect()')
            finally:
                _os.set_blocking(fd, True)
        try:
            return 0 if _os.unix_connect(fd, name) else _errno.EAGAIN
        except OSError as e:
            return e.errno

    # -- transferência --------------------------------------------------------------------------
    def recv(self, bufsize, flags=0):
        bufsize, flags = _index(bufsize), _c_int(flags)
        if bufsize < 0:
            raise ValueError('negative buffersize in recv')
        fd = self._checked()
        if self._type == SOCK_STREAM:
            return self._call(lambda: _os.sock_recv(fd, bufsize, flags), _POLLIN, 'socket.recv()', flags)
        return socket.recvfrom(self, bufsize, flags)[0]

    def recv_into(self, buffer, nbytes=0, flags=0):
        view = _writable_view('recv_into', buffer)
        nbytes, flags = _index(nbytes), _c_int(flags)
        if nbytes < 0:
            raise ValueError('negative buffersize in recv_into')
        if nbytes > len(view):
            raise ValueError('buffer too small for requested bytes')
        data = socket.recv(self, nbytes or len(view), flags)
        view[:len(data)] = data
        return len(data)

    def recvfrom(self, bufsize, flags=0):
        bufsize, flags = _index(bufsize), _c_int(flags)
        if bufsize < 0:
            raise ValueError('negative buffersize in recvfrom')
        fd = self._checked()
        peek = bool(flags & MSG_PEEK)
        kind = self._type
        if kind == SOCK_STREAM:
            # Num socket de fluxo o kernel não entrega endereço: o CPython devolve `None`.
            return socket.recv(self, bufsize, flags), None
        if self._family == AF_INET or self._family == AF_INET6:
            def op():
                got = _os.udp_recvfrom(fd, bufsize, peek)
                if got is None:
                    raise _would_block()
                return got[0], _net.address(self._family, got[1][0], got[1][1])
        else:
            def op():
                got = _os.unix_recvfrom(fd, bufsize, peek)
                if got is None:
                    raise _would_block()
                return got[0], self._unix_source(fd, got[1])
        return self._call(op, _POLLIN, 'socket.recvfrom()', flags)

    def _unix_source(self, fd, source):
        """O endereço de quem enviou, como o `unix_copy_addr`: nome do remetente do datagrama, ou, no seqpacket,
        o nome que o par tem; `None` sem nome."""
        if self._type == SOCK_SEQPACKET:
            source = _os.unix_names(fd)[1]
        return _net.unix_name(source) if source else None

    def recvfrom_into(self, buffer, nbytes=0, flags=0):
        view = _writable_view('recvfrom_into', buffer)
        nbytes, flags = _index(nbytes), _c_int(flags)
        if nbytes < 0:
            raise ValueError('negative buffersize in recvfrom_into')
        if nbytes > len(view):
            raise ValueError('nbytes is greater than the length of the buffer')
        data, source = socket.recvfrom(self, nbytes or len(view), flags)
        view[:len(data)] = data
        return len(data), source

    def _write_stream(self, data, flags, whole):
        """Escrita num socket de fluxo: no bloqueante e no `sendall` vai até o fim (o `sendall` com timeout
        divide o prazo entre as escritas); nos outros casos é uma escrita só, que pode ser parcial."""
        fd = self._checked()
        blocking = self._timeout is None and not flags & MSG_DONTWAIT
        whole = whole or blocking
        deadline = _time.monotonic() + self._timeout if whole and self._timeout else None
        total = 0
        while True:
            chunk = data[total:]
            if blocking:
                n = self._send_call(lambda: _os.sock_send(fd, chunk, flags), 'socket.send()', flags)
            else:
                n = self._call(lambda: _os.sock_send(fd, chunk, flags), _POLLOUT, 'socket.send()', flags, deadline)
            total += n
            if not whole or total >= len(data):
                return total

    def send(self, data, flags=0):
        data, flags = _as_bytes(data), _c_int(flags)
        fd = self._checked()
        if self._type == SOCK_STREAM:
            return self._write_stream(data, flags, False)
        if self._type == SOCK_SEQPACKET:
            # `unix_dgram_sendmsg`: a mensagem não pode passar do SO_SNDBUF menos 32 bytes.
            sndbuf = _stored(fd, SOL_SOCKET, SO_SNDBUF, 'sndbuf')
            if len(data) > (_MEM_MAX if sndbuf is None else sndbuf) - 32:
                raise OSError(_errno.EMSGSIZE, 'Message too long')
        return self._send_datagram(fd, data, None, flags)

    def _send_datagram(self, fd, data, address, flags):
        """`sendto` de mensagem (UDP, Unix de datagrama ou seqpacket): `address` já normalizado, ou `None`
        para o par do `connect`."""
        if self._family == AF_UNIX:
            name = None if address is None else _net.unix_encode(address)

            def op():
                n = _os.unix_sendto(fd, data, name)
                if n is None:
                    raise _would_block()
                return n
        else:
            if address is None:
                def op():
                    return _os.udp_sendto(fd, data, None)
            else:
                if not _net.is_local(address[0]):
                    raise OSError(_errno.ENETUNREACH, 'Network is unreachable')
                host = self._dest(address[0])

                def op():
                    return _os.udp_sendto(fd, data, host, address[1])
        return self._send_call(op, 'socket.send()', flags)

    def sendall(self, data, flags=0):
        data, flags = _as_bytes(data), _c_int(flags)
        if self._type == SOCK_STREAM:
            self._write_stream(data, flags, True)
        else:
            socket.send(self, data, flags)

    def sendto(self, data, *args):
        if len(args) not in (1, 2):
            raise TypeError('sendto() takes 2 or 3 arguments (%d given)' % (len(args) + 1))
        data = _as_bytes(data)
        flags = _c_int(args[0]) if len(args) == 2 else 0
        address = args[-1]
        if self._family == AF_UNIX:
            address = _norm_unix(address)
        else:
            address = self._norm(address, 'sendto')
        fd = self._checked()
        if self._family == AF_UNIX:
            return self._send_datagram(fd, data, address, flags)
        if self._type == SOCK_STREAM:
            # O TCP ignora o endereço de um socket já conectado.
            return self._write_stream(data, flags, False)
        return self._send_datagram(fd, data, address, flags)

    # -- dados auxiliares (sendmsg, recvmsg) ----------------------------------------------------
    def sendmsg(self, buffers, ancdata=(), flags=0, address=None, /):
        parts = _sendmsg_parts(buffers)
        items = _ancdata_items(ancdata)
        flags = _c_int(flags)
        data = b''.join(parts)
        if self._family != AF_UNIX:
            # O `__scm_send` só aceita `SCM_RIGHTS` em socket Unix; o resto dos itens o TCP e o UDP ignoram.
            self._checked()
            if any(level == SOL_SOCKET and kind == SCM_RIGHTS for level, kind, _ in items):
                raise OSError(_errno.EINVAL, 'Invalid argument')
            if address is None:
                return socket.send(self, data, flags)
            return socket.sendto(self, data, flags, address)
        name = None if address is None else _net.unix_encode(_norm_unix(address))
        control = _pack_ancdata(items)
        fd = self._checked()

        def op():
            sent = _os.unix_sendmsg(fd, data, name, control, flags)
            if sent is None:
                raise _would_block()
            return sent
        sent = self._send_call(op, 'socket.sendmsg()', flags)
        if self._type == SOCK_STREAM and sent < len(data) and self._timeout is None and not flags & MSG_DONTWAIT:
            # Um fluxo bloqueante só volta com tudo enfileirado; os descritores já foram com o primeiro trecho.
            self._write_stream(data[sent:], flags, True)
            sent = len(data)
        return sent

    def sendmsg_afalg(self, msg=None, *, op, iv=None, assoclen=-1, flags=0):
        # Os itens de nível `SOL_ALG` só valem para `AF_ALG`; em outro socket o kernel os ignora e envia os dados.
        return self.sendmsg([] if msg is None else msg, [], flags)

    def recvmsg(self, bufsize, ancbufsize=0, flags=0, /):
        bufsize, ancbufsize, flags = _recvmsg_sizes('recvmsg', bufsize, ancbufsize, flags)
        data, ancdata, msg_flags, address = self._recvmsg(bufsize, ancbufsize, flags)
        return data, ancdata, msg_flags, address

    def recvmsg_into(self, buffers, ancbufsize=0, flags=0, /):
        try:
            items = list(buffers)
        except TypeError:
            raise TypeError('recvmsg_into() argument 1 must be an iterable') from None
        views = []
        for item in items:
            try:
                view = memoryview(item)
            except TypeError:
                view = None
            if view is None or view.readonly:
                raise TypeError('recvmsg_into() argument 1 must be an iterable of single-segment read-write buffers')
            views.append(view)
        _, ancbufsize, flags = _recvmsg_sizes('recvmsg_into', 0, ancbufsize, flags)
        data, ancdata, msg_flags, address = self._recvmsg(sum(len(view) for view in views), ancbufsize, flags)
        pos = 0
        for view in views:
            chunk = data[pos:pos + len(view)]
            if not chunk:
                break
            view[:len(chunk)] = chunk
            pos += len(chunk)
        return len(data), ancdata, msg_flags, address

    def _recvmsg(self, bufsize, ancbufsize, flags):
        """`recvmsg(2)`: `(dados, dados auxiliares, flags de saída, endereço)`. Só o socket Unix tem dados auxiliares;
        no TCP e no UDP o `msg_control` volta vazio."""
        fd = self._checked()
        if self._family != AF_UNIX:
            if self._type == SOCK_STREAM:
                return socket.recv(self, bufsize, flags), [], 0, None
            data, address = socket.recvfrom(self, bufsize, flags)
            return data, [], 0, address

        def op():
            got = _os.unix_recvmsg(fd, bufsize, ancbufsize, flags)
            if got is None:
                raise _would_block()
            return got
        data, name, control, out = self._call(op, _POLLIN, 'socket.recvmsg()', flags)
        return data, _parse_ancdata(control), out, self._unix_source(fd, name)

    def shutdown(self, how):
        fd = self._checked()
        # `inet_shutdown` e `unix_shutdown` validam `how` antes de olhar o estado da conexão.
        if how not in (SHUT_RD, SHUT_WR, SHUT_RDWR):
            raise OSError(_errno.EINVAL, 'Invalid argument')
        if self._type == SOCK_DGRAM and self._family != AF_UNIX:
            # `inet_shutdown` recusa o UDP sem par; o `unix_shutdown` aceita qualquer tipo.
            raise OSError(_errno.ENOTCONN, 'Transport endpoint is not connected')
        _os.tcp_shutdown(fd, how in (SHUT_RD, SHUT_RDWR), how in (SHUT_WR, SHUT_RDWR))


SocketType = socket
_TIMED_OUT = -1


def _os_error(code):
    import os
    return OSError(code, os.strerror(code))


def socketpair(family=AF_UNIX, type=SOCK_STREAM, proto=0):
    family, type, proto = _c_int(family), _c_int(type), _c_int(proto)
    if family not in (AF_UNIX, AF_INET, AF_INET6):
        raise OSError(_errno.EAFNOSUPPORT, 'Address family not supported by protocol')
    base_type = type & ~(SOCK_NONBLOCK | SOCK_CLOEXEC)
    # O kernel cria os dois sockets antes de parear: o tipo que a família não tem falha primeiro.
    if base_type not in (SOCK_STREAM, SOCK_DGRAM, SOCK_SEQPACKET, SOCK_RAW):
        raise _bad_type(base_type)
    if family != AF_UNIX:
        # `inet_family_ops` não tem `socketpair` (sock_no_socketpair).
        raise OSError(_errno.EOPNOTSUPP, 'Operation not supported')
    fds = _os.unix_socketpair(base_type)
    out = []
    for fd in fds:
        s = socket(AF_UNIX, base_type, proto, fileno=fd)
        if type & SOCK_NONBLOCK:
            s.setblocking(False)
        out.append(s)
    return tuple(out)


def close(fd):
    _os.close(fd)


def dup(fd):
    return _os.dup(fd)


# -- nomes e endereços --------------------------------------------------------------------------

# -- nomes e endereços --------------------------------------------------------------------------
# A resolução é a do glibc 2.41 com o `nsswitch.conf` da imagem (`hosts: files dns`, `multi on`) numa máquina
# sem rede: o /etc/hosts responde, e o DNS que sobra nunca alcança servidor algum (`EAI_AGAIN`).

_ASCII_DIGITS = '0123456789'
_ASCII_SPACE = ' \t\n\v\f\r'
_AI_VALID = (AI_PASSIVE | AI_CANONNAME | AI_NUMERICHOST | AI_ADDRCONFIG | AI_V4MAPPED | AI_NUMERICSERV | AI_ALL
             | 0x40 | 0x80 | 0x100 | 0x200)
_GAI_MESSAGES = {
    EAI_BADFLAGS: 'Bad value for ai_flags',
    EAI_NONAME: 'Name or service not known',
    EAI_AGAIN: 'Temporary failure in name resolution',
    EAI_FAIL: 'Non-recoverable failure in name resolution',
    EAI_NODATA: 'No address associated with hostname',
    EAI_FAMILY: 'ai_family not supported',
    EAI_SOCKTYPE: 'ai_socktype not supported',
    EAI_SERVICE: 'Servname not supported for ai_socktype',
    EAI_ADDRFAMILY: 'Address family for hostname not supported',
    EAI_MEMORY: 'Memory allocation failure',
    EAI_SYSTEM: 'System error',
    EAI_OVERFLOW: 'Result too large for supplied buffer',
}


def _gai(code):
    return gaierror(code, _GAI_MESSAGES[code])


def _str_arg(fname, position, value):
    """O formato `s` do `PyArg_ParseTuple` (`position` é o número do argumento) ou o `str` do Argument Clinic de
    um `METH_O` (`position` é `None`: sem número); só `str`, sem NUL embutido. Como o `converterr` do
    `getargs.c`, o `None` aparece pelo nome, não como `NoneType`."""
    if not isinstance(value, str):
        tname = 'None' if value is None else type(value).__name__
        if position is None:
            raise TypeError('%s() argument must be str, not %s' % (fname, tname))
        raise TypeError('%s() argument %d must be str, not %s' % (fname, position, tname))
    if '\0' in value:
        raise ValueError('embedded null character')
    return value


def _host_arg(host, caller=None):
    """O `idna_converter` do socketmodule: `str` (em idna se não for ASCII), `bytes` ou `bytearray`. Com `caller`
    (o formato `et` de `gethostbyname` e companhia) a mensagem de tipo leva o nome da função."""
    if isinstance(host, str):
        return host if host.isascii() else host.encode('idna').decode('ascii')
    if isinstance(host, (bytes, bytearray)):
        return bytes(host).decode('latin-1')
    if caller is not None:
        raise TypeError('%s() argument 1 must be str, bytes or bytearray, not %s' % (caller, type(host).__name__))
    raise TypeError('str, bytes or bytearray expected, not %s' % type(host).__name__)


def _pton4(text):
    """`inet_pton(AF_INET)` do glibc: quatro decimais sem zero à esquerda."""
    parts = text.split('.')
    if len(parts) != 4:
        return None
    out = bytearray()
    for part in parts:
        if not part or not all(c in _ASCII_DIGITS for c in part) or (len(part) > 1 and part[0] == '0'):
            return None
        value = int(part)
        if value > 255:
            return None
        out.append(value)
    return bytes(out)


def _aton(text, exact):
    """`inet_aton` do glibc (formas `a`, `a.b`, `a.b.c`, `a.b.c.d`; decimal, octal e hexadecimal). `exact` (o
    `getaddrinfo`) não aceita nada depois do número; sem ele um espaço encerra o endereço."""
    size = len(text)
    at = 0
    parts = []
    while True:
        if at >= size or text[at] not in _ASCII_DIGITS:
            return None
        base = 10
        caught = False
        value = 0
        if text[at] == '0':
            at += 1
            if at < size and text[at] in 'xX':
                base = 16
                at += 1
            else:
                base = 8
                caught = True
        while at < size:
            c = text[at]
            if c in _ASCII_DIGITS:
                if base == 8 and c in '89':
                    return None
                value = value * base + ord(c) - 48
            elif base == 16 and c in 'abcdefABCDEF':
                value = (value << 4) | int(c, 16)
            else:
                break
            if value > 0xffffffff:
                return None
            caught = True
            at += 1
        if at < size and text[at] == '.':
            if len(parts) >= 3 or value > 0xff:
                return None
            parts.append(value)
            at += 1
            continue
        break
    if at < size and (exact or text[at] not in _ASCII_SPACE):
        return None
    if not caught:
        return None
    count = len(parts) + 1
    limit = (0xffffffff, 0xffffff, 0xffff, 0xff)[count - 1]
    if value > limit:
        return None
    for index, part in enumerate(parts):
        value |= part << (24 - 8 * index)
    return value.to_bytes(4, 'big')


def _pton6(text):
    """`inet_pton(AF_INET6)` do glibc: grupos de até quatro dígitos, um `::` e um IPv4 no fim."""
    size = len(text)
    at = 0
    if text[:1] == ':':
        if text[1:2] != ':':
            return None
        at = 1
    words = []
    compress = None
    token = at
    value = 0
    digits = 0
    seen = False
    while at < size:
        c = text[at]
        at += 1
        if c in '0123456789abcdefABCDEF':
            digits += 1
            if digits > 4:
                return None
            value = (value << 4) | int(c, 16)
            seen = True
            continue
        if c == ':':
            token = at
            if not seen:
                if compress is not None:
                    return None
                compress = len(words)
                continue
            if at >= size or len(words) >= 8:
                return None
            words.append(value)
            seen = False
            digits = 0
            value = 0
            continue
        if c == '.' and len(words) <= 6:
            tail = _pton4(text[token:])
            if tail is None:
                return None
            words.append(tail[0] << 8 | tail[1])
            words.append(tail[2] << 8 | tail[3])
            seen = False
            break
        return None
    if seen:
        if len(words) >= 8:
            return None
        words.append(value)
    if compress is not None:
        if len(words) == 8:
            return None
        words = words[:compress] + [0] * (8 - len(words)) + words[compress:]
    elif len(words) != 8:
        return None
    return b''.join(w.to_bytes(2, 'big') for w in words)


def _ntop4(packed):
    return '.'.join(str(b) for b in packed)


def _ntop6(packed):
    """`inet_ntop(AF_INET6)` do glibc: a maior sequência de zeros (de dois grupos em diante) vira `::` e o IPv4
    encapsulado (`::a.b.c.d`, `::ffff:a.b.c.d`) sai em decimal."""
    words = [int.from_bytes(packed[i:i + 2], 'big') for i in range(0, 16, 2)]
    best_at, best_len = -1, 0
    cur_at, cur_len = -1, 0
    for i, word in enumerate(words + [1]):
        if word == 0:
            if cur_at < 0:
                cur_at, cur_len = i, 1
            else:
                cur_len += 1
        elif cur_at >= 0:
            if best_at < 0 or cur_len > best_len:
                best_at, best_len = cur_at, cur_len
            cur_at = -1
    if best_at >= 0 and best_len < 2:
        best_at = -1
    out = ''
    for i in range(8):
        if best_at >= 0 and best_at <= i < best_at + best_len:
            if i == best_at:
                out += ':'
            continue
        if i != 0:
            out += ':'
        if i == 6 and best_at == 0 and (best_len == 6 or (best_len == 7 and words[7] != 1)
                                        or (best_len == 5 and words[5] == 0xffff)):
            return out + _ntop4(packed[12:])
        out += '%x' % words[i]
    if best_at >= 0 and best_at + best_len == 8:
        out += ':'
    return out


def _numeric_ip(host):
    """`(família, texto canônico)` de um endereço numérico (o que o `getaddrinfo` aceita sem consultar nome), ou
    `None`."""
    packed = _aton(host, True)
    if packed is not None:
        return AF_INET, _ntop4(packed)
    if ':' in host:
        packed = _pton6(host)
        if packed is not None:
            return AF_INET6, _ntop6(packed)
    return None


def gethostname():
    return _net.hostname()


def sethostname(name):
    import os
    os.fspath(name)
    raise PermissionError(_errno.EPERM, 'Operation not permitted')


def _db_lines(path):
    """Os campos de cada linha útil de um banco de /etc (`#` começa comentário), relido a cada consulta."""
    try:
        with open(path, 'rb') as f:
            data = f.read().decode('utf-8', 'replace')
    except OSError:
        return []
    return [fields for fields in (line.split('#', 1)[0].split() for line in data.splitlines()) if fields]


def _hosts_entries():
    """O /etc/hosts como o glibc (`hosts: files`) o lê: cada linha vale `(endereço canônico, [nome canônico,
    aliases...])`, na ordem do arquivo."""
    entries = []
    for fields in _db_lines('/etc/hosts'):
        if len(fields) < 2:
            continue
        packed = _pton4(fields[0])
        if packed is not None:
            entries.append((_ntop4(packed), fields[1:]))
            continue
        packed = _pton6(fields[0]) if ':' in fields[0] else None
        if packed is not None:
            entries.append((_ntop6(packed), fields[1:]))
    return entries


def _hosts_family(ip):
    return AF_INET6 if ':' in ip else AF_INET


def _hosts_lookup(name, family=AF_UNSPEC):
    """As linhas do /etc/hosts que citam `name` (sem diferenciar maiúsculas) para a família pedida, na ordem do
    arquivo."""
    wanted = name.lower()
    if not wanted.rstrip('.'):
        return []
    # O glibc lê o `::1` do /etc/hosts como `127.0.0.1` numa busca IPv4 (por isso `localhost` sai duas vezes).
    return [('127.0.0.1' if family == AF_INET and ip == '::1' else ip, names) for ip, names in _hosts_entries()
            if family in (0, AF_UNSPEC, _hosts_family(ip), AF_INET if ip == '::1' else None)
            and wanted in [n.lower() for n in names]]


def _set_ip_address(host, family):
    """O `setipaddr` do socketmodule: o endereço numérico que um nome de máquina resolve (o primeiro do
    `getaddrinfo`), com os atalhos do vazio (qualquer endereço) e de `<broadcast>`."""
    if host == '':
        return getaddrinfo(None, '0', family, SOCK_DGRAM, 0, AI_PASSIVE)[0][4][0]
    if host in ('255.255.255.255', '<broadcast>'):
        if family not in (AF_INET, AF_UNSPEC):
            raise OSError('address family mismatched')
        return '255.255.255.255'
    return getaddrinfo(host, None, family, SOCK_STREAM)[0][4][0]


def gethostbyname(name):
    return _set_ip_address(_host_arg(name, 'gethostbyname'), AF_INET)


def gethostbyname_ex(name):
    host = _host_arg(name, 'gethostbyname_ex')
    address = _set_ip_address(host, AF_INET)
    # O `gethostbyname_r` do glibc devolve um endereço numérico como o próprio nome; o resto vem do /etc/hosts.
    if _aton(host, True) is not None:
        return (host, [], [address])
    found = _hosts_lookup(host, AF_INET)
    if not found:
        # O nome vazio e `<broadcast>` passam pelo `setipaddr` e o resolvedor os recusa: `NO_RECOVERY`.
        raise herror(3, 'Unknown server error')
    # `multi on`: as linhas do mesmo nome somam endereços e apelidos ao nome canônico da primeira.
    canonical = found[0][1][0]
    aliases = [n for _, names in found for n in names if n != canonical]
    return (canonical, aliases, [ip for ip, _ in found])


def gethostbyaddr(ip):
    address = _set_ip_address(_host_arg(ip, 'gethostbyaddr'), AF_UNSPEC)
    for known, names in _hosts_entries():
        if known == address:
            return (names[0], names[1:], [address])
    # O DNS que o `nsswitch.conf` manda tentar depois dos arquivos não alcança servidor: `TRY_AGAIN`.
    raise herror(2, 'Host name lookup failure')


def _services():
    """As linhas de /etc/services: `(nome, porta, protocolo, aliases)`."""
    for fields in _db_lines('/etc/services'):
        port, _, proto = fields[1].partition('/') if len(fields) > 1 else ('', '', '')
        if proto and port and all(c in _ASCII_DIGITS for c in port):
            yield fields[0], int(port) & 0xffff, proto, fields[2:]


def _service_port(name, proto):
    for service, port, kind, aliases in _services():
        if (name == service or name in aliases) and (proto is None or proto == kind):
            return port
    return None


def _service_name(port, proto):
    for service, number, kind, _ in _services():
        if number == port and (proto is None or proto == kind):
            return service
    return None


def _argc(fname, args, least, most):
    """A contagem de argumentos do `PyArg_ParseTuple` (`getservbyname() takes at least 1 argument (0 given)`)."""
    count = len(args)
    if least <= count <= most:
        return
    limit = least if count < least else most
    kind = 'exactly' if least == most else 'at least' if count < least else 'at most'
    raise TypeError('%s() takes %s %d argument%s (%d given)' % (fname, kind, limit, '' if limit == 1 else 's', count))


def getservbyname(*args):
    _argc('getservbyname', args, 1, 2)
    name, protocolname = args[0], args[1] if len(args) == 2 else None
    _str_arg('getservbyname', 1, name)
    if len(args) == 2:
        _str_arg('getservbyname', 2, protocolname)
    port = _service_port(name, protocolname)
    if port is None:
        raise OSError('service/proto not found')
    return port


def getservbyport(*args):
    _argc('getservbyport', args, 1, 2)
    port = _c_int(args[0])
    protocolname = args[1] if len(args) == 2 else None
    if len(args) == 2:
        _str_arg('getservbyport', 2, protocolname)
    if not 0 <= port <= 0xffff:
        raise OverflowError('getservbyport: port must be 0-65535.')
    name = _service_name(port, protocolname)
    if name is None:
        raise OSError('port/proto not found')
    return name


def getprotobyname(*args):
    _argc('getprotobyname', args, 1, 1)
    name = _str_arg('getprotobyname', 1, args[0])
    for fields in _db_lines('/etc/protocols'):
        if len(fields) > 1 and (name == fields[0] or name in fields[2:]) and all(c in _ASCII_DIGITS for c in fields[1]):
            return int(fields[1])
    raise OSError('protocol not found')


# Os pares (tipo, protocolo, nome no /etc/services) que o `getaddrinfo` do glibc oferece quando o tipo não vem.
_TYPE_PROTOS = ((SOCK_STREAM, IPPROTO_TCP, 'tcp'), (SOCK_DGRAM, IPPROTO_UDP, 'udp'), (SOCK_RAW, 0, ''))
# A tabela inteira do glibc, na ordem: o que só atende a um tipo ou protocolo pedido (udplite, sctp) fica entre o
# udp e o raw.
_EXPLICIT_TYPE_PROTOS = (_TYPE_PROTOS[0], _TYPE_PROTOS[1], (SOCK_DGRAM, IPPROTO_UDPLITE, 'udplite'),
                         (SOCK_STREAM, IPPROTO_SCTP, 'sctp'), (SOCK_SEQPACKET, IPPROTO_SCTP, 'sctp'), _TYPE_PROTOS[2])


def _numeric_service(text):
    """A porta de um serviço numérico (o `strtoul` do glibc truncado a 16 bits), ou `None` se é um nome (o glibc
    não aceita o sinal de menos: `-1` vai para o `/etc/services` e dá `EAI_SERVICE`)."""
    body = text.lstrip(_ASCII_SPACE)
    if text == '':
        return 0
    if body[:1] == '+':
        body = body[1:]
    if body and all(c in _ASCII_DIGITS for c in body):
        return int(body) & 0xffff
    return None


def _socket_types(type_, proto, service, flags):
    """Os `(tipo, protocolo, porta)` de um `getaddrinfo`: um por tipo de socket que serve ao pedido."""
    if type_ or proto:
        # Com tipo ou protocolo pedido o glibc fica só com a primeira linha da tabela que serve.
        chosen = [t for t in _EXPLICIT_TYPE_PROTOS
                  if (type_ == 0 or type_ == t[0]) and (proto == 0 or proto == t[1] or t[0] == SOCK_RAW)][:1]
        if not chosen:
            raise _gai(EAI_SOCKTYPE if type_ else EAI_SERVICE)
        # O `raw` não tem serviço: qualquer porta, até numérica, é `EAI_SERVICE`.
        if service is not None and chosen[0][0] == SOCK_RAW:
            raise _gai(EAI_SERVICE)
    else:
        chosen = list(_TYPE_PROTOS)
    if service is None:
        return [(kind, proto if kind == SOCK_RAW else number, 0) for kind, number, _ in chosen]
    port = _numeric_service(service)
    if port is not None:
        return [(kind, proto if kind == SOCK_RAW else number, port) for kind, number, _ in chosen]
    if flags & AI_NUMERICSERV:
        raise _gai(EAI_NONAME)
    found = []
    for kind, number, name in chosen:
        port = _service_port(service, name) if name else None
        if port is not None:
            found.append((kind, number, port))
    if not found:
        raise _gai(EAI_SERVICE)
    return found


def _is_dns_name(name):
    """O `res_hnok` do glibc que antecede a consulta ao DNS: só letras, dígitos, `-`, `_` e `.`; o resto (espaço,
    `:`...) é nome inexistente, não falha de rede."""
    return all(c.isascii() and (c.isalnum() or c in '-_.') for c in name)


def _scope_id(label):
    """O scope id de `fe80::1%lo`: número ou nome de interface (só existe a `lo`), `None` se não existe."""
    if label.isascii() and label.isdigit():
        return int(label)
    return 1 if label == 'lo' else None


def _resolve(name, family, flags):
    """Os `[(família, endereço)]` de um nome (na ordem do glibc) e o nome canônico (ou `None`)."""
    if name is None:
        if flags & AI_PASSIVE:
            both = [(AF_INET, '0.0.0.0'), (AF_INET6, '::')]
        else:
            both = [(AF_INET6, '::1'), (AF_INET, '127.0.0.1')]
        return [a for a in both if family in (AF_UNSPEC, a[0])], None
    numeric = _numeric_ip(name)
    if numeric is not None:
        kind, ip = numeric
        if family not in (AF_UNSPEC, kind):
            if kind == AF_INET and family == AF_INET6 and flags & AI_V4MAPPED:
                return [(AF_INET6, _ntop6(bytes(10) + b'\xff\xff' + _aton(ip, True)))], name
            raise _gai(EAI_ADDRFAMILY)
        return [(kind, ip)], name
    if flags & AI_NUMERICHOST or name == '' or not _is_dns_name(name):
        raise _gai(EAI_NONAME)
    found = _hosts_lookup(name, family)
    if family == AF_INET6 and flags & AI_V4MAPPED:
        if not found or flags & AI_ALL:
            found += [(_ntop6(bytes(10) + b'\xff\xff' + _pton4(ip)), names) for ip, names in _hosts_lookup(name, AF_INET)]
    if not found:
        # O que o /etc/hosts não sabe vai para o DNS, e sem rede nenhum servidor responde.
        raise _gai(EAI_AGAIN)
    addresses = [(_hosts_family(ip), ip) for ip, _ in found]
    # RFC 3484: com os dois lados na lista, o IPv6 (precedência 50) sai antes do IPv4 (35).
    addresses.sort(key=lambda a: a[0] != AF_INET6)
    return addresses, found[0][1][0]


def getaddrinfo(host, port, family=0, type=0, proto=0, flags=0):
    family, type, proto, flags = _c_int(family), _c_int(type), _c_int(proto), _c_int(flags)
    if host is None:
        name = None
    elif isinstance(host, str):
        name = _host_arg(host)
    elif isinstance(host, bytes):
        name = host.decode('latin-1')
    else:
        raise TypeError('getaddrinfo() argument 1 must be string or None')
    if port is None:
        service = None
    elif isinstance(port, int):
        service = str(int(port))
    elif isinstance(port, str):
        service = port
    elif isinstance(port, bytes):
        service = port.decode('latin-1')
    else:
        raise OSError('Int or String expected')
    if flags & ~_AI_VALID or (flags & AI_CANONNAME and name is None):
        raise _gai(EAI_BADFLAGS)
    if name is None and service is None:
        raise _gai(EAI_NONAME)
    if family not in (AF_UNSPEC, AF_INET, AF_INET6):
        raise _gai(EAI_FAMILY)
    kinds = _socket_types(type, proto, service, flags)
    scope = 0
    base, _, label = name.partition('%') if name is not None and ':' in name else (name, '', '')
    if label and _pton6(base) is not None:
        scope = _scope_id(label)
        if scope is None:
            raise _gai(EAI_NONAME)
        addresses, canonical = _resolve(base, family, flags)
        canonical = name
    else:
        addresses, canonical = _resolve(name, family, flags)
    result = []
    for fam, ip in addresses:
        for kind, number, number_port in kinds:
            sockaddr = (ip, number_port, 0, scope) if fam == AF_INET6 else (ip, number_port)
            # O nome canônico só vem no primeiro item.
            canon = canonical if flags & AI_CANONNAME and not result and canonical is not None else ''
            result.append((fam, kind, number, canon, sockaddr))
    return result


def getnameinfo(sockaddr, flags):
    flags = _c_int(flags)
    if not isinstance(sockaddr, tuple):
        raise TypeError('getnameinfo() argument 1 must be a tuple')
    if not 2 <= len(sockaddr) <= 4 or not isinstance(sockaddr[0], str):
        raise TypeError('getnameinfo(): illegal sockaddr argument')
    host, port = sockaddr[0], _c_int(sockaddr[1])
    if len(sockaddr) > 2 and _index(sockaddr[2]) > 0xfffff:
        raise OverflowError('getnameinfo(): flowinfo must be 0-1048575.')
    # O endereço passa antes pelo `getaddrinfo` numérico: nome de máquina aqui é erro de nome.
    resolved = _resolve(host, AF_UNSPEC, AI_NUMERICHOST)[0]
    if len(resolved) > 1:
        raise OSError('sockaddr resolved to multiple addresses')
    kind, ip = resolved[0]
    if kind == AF_INET and len(sockaddr) != 2:
        raise OSError('IPv4 sockaddr must be 2 tuple')
    name = ip
    if not flags & NI_NUMERICHOST:
        for known, names in _hosts_entries():
            if known == ip:
                name = names[0]
                break
        else:
            # O que o /etc/hosts não sabe vai para o DNS reverso, e sem rede o glibc devolve a falha temporária,
            # mesmo sem NI_NAMEREQD.
            raise _gai(EAI_AGAIN)
    port &= 0xffff
    service = None if flags & NI_NUMERICSERV else _service_name(port, 'udp' if flags & NI_DGRAM else 'tcp')
    return name, str(port) if service is None else service


def inet_aton(text, /):
    packed = _aton(_str_arg('inet_aton', None, text), False)
    if packed is None:
        raise OSError('illegal IP address string passed to inet_aton')
    return packed


def inet_ntoa(packed, /):
    packed = _as_bytes(packed)
    if len(packed) != 4:
        raise OSError('packed IP wrong length for inet_ntoa')
    return _ntop4(packed)


def inet_pton(family, text, /):
    family = _c_int(family)
    text = _str_arg('inet_pton', 2, text)
    if family not in (AF_INET, AF_INET6):
        raise OSError(_errno.EAFNOSUPPORT, 'Address family not supported by protocol')
    packed = _pton4(text) if family == AF_INET else _pton6(text)
    if packed is None:
        raise OSError('illegal IP address string passed to inet_pton')
    return packed


def inet_ntop(family, packed, /):
    family = _c_int(family)
    packed = _as_bytes(packed)
    if family not in (AF_INET, AF_INET6):
        raise ValueError('unknown address family %d' % family)
    if len(packed) != (4 if family == AF_INET else 16):
        raise ValueError('invalid length of packed IP address string')
    return _ntop4(packed) if family == AF_INET else _ntop6(packed)


def _byte_swap(fname, bits, value):
    """`htons`/`ntohs` (16 bits) e `htonl`/`ntohl` (32 bits): o mesmo troca-bytes, com os erros de cada um."""
    if bits == 32:
        if not isinstance(value, int):
            raise TypeError('expected int, %s found' % type(value).__name__)
        if value < 0:
            raise OverflowError("can't convert negative value to unsigned int")
        if value >= 1 << 64:
            raise OverflowError('Python int too large to convert to C unsigned long')
        if value >> 32:
            raise OverflowError('int larger than 32 bits')
    else:
        # O `htons`/`ntohs` usam o `int` do Argument Clinic, com a mensagem própria de estouro.
        value = _index(value)
        if value > _INT_MAX:
            raise OverflowError('Python int too large to convert to C int')
        if value < -_INT_MAX - 1:
            raise OverflowError('Python int too small to convert to C int')
        if value < 0:
            raise OverflowError("%s: can't convert negative Python int to C 16-bit unsigned integer" % fname)
        if value > 0xffff:
            raise OverflowError('%s: Python int too large to convert to C 16-bit unsigned integer' % fname)
    return int.from_bytes(value.to_bytes(bits // 8, 'big'), 'little')


def htons(x, /):
    return _byte_swap('htons', 16, x)


def ntohs(x, /):
    return _byte_swap('ntohs', 16, x)


def htonl(x, /):
    return _byte_swap('htonl', 32, x)


def ntohl(x, /):
    return _byte_swap('ntohl', 32, x)


def if_nameindex():
    return [(1, 'lo')]


def if_nametoindex(oname, /):
    import os
    name = os.fsencode(oname)
    if b'\0' in name:
        raise ValueError('embedded null byte')
    if name != b'lo':
        raise OSError('no interface with this name')
    return 1


def if_indextoname(index, /):
    if not isinstance(index, int):
        raise TypeError('an integer is required')
    if index < 0:
        raise OverflowError("can't convert negative value to unsigned int")
    if index >= 1 << 64:
        raise OverflowError('Python int too large to convert to C unsigned long')
    if index > 0xffffffff:
        raise OverflowError('index is too large')
    if index == 1:
        return 'lo'
    raise OSError(_errno.ENXIO, 'No such device or address')
