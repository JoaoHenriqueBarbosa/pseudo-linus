"""This module provides an interface to the Posix calls for tty I/O control.  For a complete description of these calls,
see the Posix or Unix manual pages.  It is only available for those Unix versions that support Posix termios style tty
I/O control.

All functions in this module take a file descriptor fd as their first argument.  This can be an integer file
descriptor, such as returned by sys.stdin.fileno(), or a file object, such as sys.stdin itself."""

import _os
import fcntl as _fcntl

B0 = 0
B1000000 = 4104
B110 = 3
B115200 = 4098
B1152000 = 4105
B1200 = 9
B134 = 4
B150 = 5
B1500000 = 4106
B1800 = 10
B19200 = 14
B200 = 6
B2000000 = 4107
B230400 = 4099
B2400 = 11
B2500000 = 4108
B300 = 7
B3000000 = 4109
B3500000 = 4110
B38400 = 15
B4000000 = 4111
B460800 = 4100
B4800 = 12
B50 = 1
B500000 = 4101
B57600 = 4097
B576000 = 4102
B600 = 8
B75 = 2
B921600 = 4103
B9600 = 13
BRKINT = 2
BS0 = 0
BS1 = 8192
BSDLY = 8192
CBAUD = 4111
CBAUDEX = 4096
CDSUSP = 25
CEOF = 4
CEOL = 0
CEOT = 4
CERASE = 127
CFLUSH = 15
CIBAUD = 269418496
CINTR = 3
CKILL = 21
CLNEXT = 22
CLOCAL = 2048
CQUIT = 28
CR0 = 0
CR1 = 512
CR2 = 1024
CR3 = 1536
CRDLY = 1536
CREAD = 128
CRPRNT = 18
CRTSCTS = 2147483648
CS5 = 0
CS6 = 16
CS7 = 32
CS8 = 48
CSIZE = 48
CSTART = 17
CSTOP = 19
CSTOPB = 64
CSUSP = 26
CWERASE = 23
ECHO = 8
ECHOCTL = 512
ECHOE = 16
ECHOK = 32
ECHOKE = 2048
ECHONL = 64
ECHOPRT = 1024
EXTA = 14
EXTB = 15
EXTPROC = 65536
FF0 = 0
FF1 = 32768
FFDLY = 32768
FIOASYNC = 21586
FIOCLEX = 21585
FIONBIO = 21537
FIONCLEX = 21584
FIONREAD = 21531
FLUSHO = 4096
HUPCL = 1024
ICANON = 2
ICRNL = 256
IEXTEN = 32768
IGNBRK = 1
IGNCR = 128
IGNPAR = 4
IMAXBEL = 8192
INLCR = 64
INPCK = 16
IOCSIZE_MASK = 1073676288
IOCSIZE_SHIFT = 16
ISIG = 1
ISTRIP = 32
IUCLC = 512
IUTF8 = 16384
IXANY = 2048
IXOFF = 4096
IXON = 1024
NCC = 8
NCCS = 32
NL0 = 0
NL1 = 256
NLDLY = 256
NOFLSH = 128
N_MOUSE = 2
N_PPP = 3
N_SLIP = 1
N_STRIP = 4
N_TTY = 0
OCRNL = 8
OFDEL = 128
OFILL = 64
OLCUC = 2
ONLCR = 4
ONLRET = 32
ONOCR = 16
OPOST = 1
PARENB = 256
PARMRK = 8
PARODD = 512
PENDIN = 16384
TAB0 = 0
TAB1 = 2048
TAB2 = 4096
TAB3 = 6144
TABDLY = 6144
TCFLSH = 21515
TCGETA = 21509
TCGETS = 21505
TCIFLUSH = 0
TCIOFF = 2
TCIOFLUSH = 2
TCION = 3
TCOFLUSH = 1
TCOOFF = 0
TCOON = 1
TCSADRAIN = 1
TCSAFLUSH = 2
TCSANOW = 0
TCSBRK = 21513
TCSBRKP = 21541
TCSETA = 21510
TCSETAF = 21512
TCSETAW = 21511
TCSETS = 21506
TCSETSF = 21508
TCSETSW = 21507
TCXONC = 21514
TIOCCONS = 21533
TIOCEXCL = 21516
TIOCGETD = 21540
TIOCGICOUNT = 21597
TIOCGLCKTRMIOS = 21590
TIOCGPGRP = 21519
TIOCGSERIAL = 21534
TIOCGSOFTCAR = 21529
TIOCGWINSZ = 21523
TIOCINQ = 21531
TIOCLINUX = 21532
TIOCMBIC = 21527
TIOCMBIS = 21526
TIOCMGET = 21525
TIOCMIWAIT = 21596
TIOCMSET = 21528
TIOCM_CAR = 64
TIOCM_CD = 64
TIOCM_CTS = 32
TIOCM_DSR = 256
TIOCM_DTR = 2
TIOCM_LE = 1
TIOCM_RI = 128
TIOCM_RNG = 128
TIOCM_RTS = 4
TIOCM_SR = 16
TIOCM_ST = 8
TIOCNOTTY = 21538
TIOCNXCL = 21517
TIOCOUTQ = 21521
TIOCPKT = 21536
TIOCPKT_DATA = 0
TIOCPKT_DOSTOP = 32
TIOCPKT_FLUSHREAD = 1
TIOCPKT_FLUSHWRITE = 2
TIOCPKT_NOSTOP = 16
TIOCPKT_START = 8
TIOCPKT_STOP = 4
TIOCSCTTY = 21518
TIOCSERCONFIG = 21587
TIOCSERGETLSR = 21593
TIOCSERGETMULTI = 21594
TIOCSERGSTRUCT = 21592
TIOCSERGWILD = 21588
TIOCSERSETMULTI = 21595
TIOCSERSWILD = 21589
TIOCSER_TEMT = 1
TIOCSETD = 21539
TIOCSLCKTRMIOS = 21591
TIOCSPGRP = 21520
TIOCSSERIAL = 21535
TIOCSSOFTCAR = 21530
TIOCSTI = 21522
TIOCSWINSZ = 21524
TOSTOP = 256
VDISCARD = 13
VEOF = 4
VEOL = 11
VEOL2 = 16
VERASE = 2
VINTR = 0
VKILL = 3
VLNEXT = 15
VMIN = 6
VQUIT = 1
VREPRINT = 12
VSTART = 8
VSTOP = 9
VSUSP = 10
VSWTC = 7
VSWTCH = 7
VT0 = 0
VT1 = 16384
VTDLY = 16384
VTIME = 5
VWERASE = 14
XCASE = 4
XTABS = 6144
_POSIX_VDISABLE = 0

# No CPython a classe é criada por `PyErr_NewException("termios.error", NULL, NULL)`: herda de `Exception`, não de
# `OSError`, e os argumentos são `(errno, strerror)`.
error = type('error', (Exception,), {'__module__': 'termios'})

_INT_MAX = 2 ** 31 - 1
_INT_MIN = -2 ** 31


def _termios_error(failure):
    """O `PyErr_SetFromErrno(TermiosError)`: o `OSError` do kernel vira `termios.error(errno, strerror)`."""
    return error(failure.errno, failure.strerror)


def _index(value):
    """O `int` de um argumento numérico: o próprio `int` ou o que o `__index__` devolve."""
    if not isinstance(value, int):
        index = getattr(type(value), '__index__', None)
        if index is None:
            raise TypeError("'%s' object cannot be interpreted as an integer" % type(value).__name__)
        value = index(value)
    return value


def _long(value):
    """O `PyLong_AsLong` dos campos de `tcsetattr`: `int` ou o que tem `__index__`, na faixa do `long` de C."""
    value = _index(value)
    if not -9223372036854775808 <= value <= 9223372036854775807:
        raise OverflowError('Python int too large to convert to C long')
    return value


def _c_int(value):
    """O `_PyLong_AsInt` dos argumentos `int` do argument clinic: a faixa do `int` de C."""
    value = _index(value)
    if not _INT_MIN <= value <= _INT_MAX:
        raise OverflowError('Python int too large to convert to C int')
    return value


def _positional(name, count, args, kwargs):
    """O `_PyArg_NoKeywords` e o `_PyArg_CheckPositional` das funções do argument clinic só posicionais."""
    if kwargs:
        raise TypeError('termios.%s() takes no keyword arguments' % name)
    if len(args) != count:
        raise TypeError('%s expected %d argument%s, got %d' % (name, count, '' if count == 1 else 's', len(args)))
    return args


def _speed(value):
    """O `speed_t` (`unsigned int`) de `cfsetispeed` e `cfsetospeed`: só os 32 bits de baixo; valor com bit fora
    de `CBAUD` é `EINVAL`."""
    speed = _long(value) & 0xFFFFFFFF
    if speed & ~CBAUD:
        raise error(22, 'Invalid argument')
    return speed


def tcgetattr(*args, **kwargs):
    """Get the tty attributes for file descriptor fd.

Returns a list [iflag, oflag, cflag, lflag, ispeed, ospeed, cc]
where cc is a list of the tty special characters (each a string of
length 1, except the items with indices VMIN and VTIME, which are
integers when these fields are defined).  The interpretation of the
flags and the speeds as well as the indexing in the cc array must be
done using the symbolic constants defined in this module."""
    fd, = _positional('tcgetattr', 1, args, kwargs)
    fd = _fcntl._fileno(fd)
    try:
        iflag, oflag, cflag, lflag, line, kernel_cc = _os.tcgetattr(fd)
    except OSError as failure:
        raise _termios_error(failure) from None
    # A glibc devolve o `struct termios` de 32 posições: o que o kernel não tem vem zerado.
    cc = [bytes([c]) for c in kernel_cc] + [b'\x00'] * (NCCS - len(kernel_cc))
    if not lflag & ICANON:
        cc[VMIN] = kernel_cc[VMIN]
        cc[VTIME] = kernel_cc[VTIME]
    speed = cflag & CBAUD
    return [iflag, oflag, cflag, lflag, speed, speed, cc]


def tcsetattr(*args, **kwargs):
    """Set the tty attributes for file descriptor fd.

The attributes to be set are taken from the attributes argument, which
is a list like the one returned by tcgetattr(). The when argument
determines when the attributes are changed: termios.TCSANOW to
change immediately, termios.TCSADRAIN to change after transmitting all
queued output, or termios.TCSAFLUSH to change after transmitting all
queued output and discarding all queued input."""
    fd, when, attributes = _positional('tcsetattr', 3, args, kwargs)
    fd = _fcntl._fileno(fd)
    when = _c_int(when)
    try:
        _os.tcgetattr(fd)
    except OSError as failure:
        raise _termios_error(failure) from None
    if not isinstance(attributes, list) or len(attributes) != 7:
        raise TypeError('tcsetattr, arg 3: must be 7 element list')
    iflag, oflag, cflag, lflag, ispeed, ospeed, cc = attributes
    iflag, oflag, cflag, lflag = _long(iflag), _long(oflag), _long(cflag), _long(lflag)
    if not isinstance(cc, list) or len(cc) != NCCS:
        raise TypeError('tcsetattr: attributes[6] must be %d element list' % NCCS)
    codes = []
    for item in cc:
        if isinstance(item, bytes) and len(item) == 1:
            codes.append(item[0])
        elif isinstance(item, int):
            codes.append(_long(item) & 0xFF)
        else:
            raise TypeError('tcsetattr: elements of attributes must be characters or integers')
    _speed(ispeed)
    cflag = (cflag & ~CBAUD) | _speed(ospeed)
    try:
        _os.tcsetattr(fd, when, iflag & 0xFFFFFFFF, oflag & 0xFFFFFFFF, cflag & 0xFFFFFFFF, lflag & 0xFFFFFFFF, 0,
                      bytes(codes))
    except OSError as failure:
        raise _termios_error(failure) from None


def tcsendbreak(*args, **kwargs):
    """Send a break on file descriptor fd.

A zero duration sends a break for 0.25-0.5 seconds; a nonzero duration
has a system dependent meaning."""
    fd, duration = _positional('tcsendbreak', 2, args, kwargs)
    fd = _fcntl._fileno(fd)
    _c_int(duration)
    # Um pty não tem `break_ctl`: o `TCSBRKP` só confere que o descritor é um terminal.
    try:
        _os.tcgetattr(fd)
    except OSError as failure:
        raise _termios_error(failure) from None


def tcdrain(*args, **kwargs):
    """Wait until all output written to file descriptor fd has been transmitted."""
    fd, = _positional('tcdrain', 1, args, kwargs)
    fd = _fcntl._fileno(fd)
    # A saída de um pty vai ao buffer do mestre na hora: não há o que esperar, só o terminal a conferir.
    try:
        _os.tcgetattr(fd)
    except OSError as failure:
        raise _termios_error(failure) from None


def tcflush(*args, **kwargs):
    """Discard queued data on file descriptor fd.

The queue selector specifies which queue: termios.TCIFLUSH for the input
queue, termios.TCOFLUSH for the output queue, or termios.TCIOFLUSH for
both queues."""
    fd, queue = _positional('tcflush', 2, args, kwargs)
    fd = _fcntl._fileno(fd)
    queue = _c_int(queue)
    try:
        _os.tcflush(fd, queue)
    except OSError as failure:
        raise _termios_error(failure) from None


def tcflow(*args, **kwargs):
    """Suspend or resume input or output on file descriptor fd.

The action argument can be termios.TCOOFF to suspend output,
termios.TCOON to restart output, termios.TCIOFF to suspend input,
or termios.TCION to restart input."""
    fd, action = _positional('tcflow', 2, args, kwargs)
    fd = _fcntl._fileno(fd)
    action = _c_int(action)
    try:
        _os.tcflow(fd, action)
    except OSError as failure:
        raise _termios_error(failure) from None


def tcgetwinsize(*args, **kwargs):
    """Get the tty winsize for file descriptor fd.

Returns a tuple (ws_row, ws_col)."""
    fd, = _positional('tcgetwinsize', 1, args, kwargs)
    fd = _fcntl._fileno(fd)
    try:
        columns, rows = _os.winsize(fd)
    except OSError as failure:
        raise _termios_error(failure) from None
    return rows, columns


def tcsetwinsize(*args, **kwargs):
    """Set the tty winsize for file descriptor fd.

The winsize to be set is taken from the winsize argument, which
is a two-item tuple (ws_row, ws_col) like the one returned by tcgetwinsize()."""
    fd, winsize = _positional('tcsetwinsize', 2, args, kwargs)
    fd = _fcntl._fileno(fd)
    try:
        items = tuple(winsize)
    except TypeError:
        items = ()
    if len(items) != 2:
        raise TypeError('tcsetwinsize, arg 2: must be a two-item sequence')
    rows, columns = (_long(item) & 0xFFFF for item in items)
    try:
        _os.tcsetwinsize(fd, rows, columns)
    except OSError as failure:
        raise _termios_error(failure) from None
