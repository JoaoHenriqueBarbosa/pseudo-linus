"""fcntl do sandbox: travas de arquivo (`flock`, `lockf`) e as constantes. Um único processo e threads
cooperativas não disputam trava entre si, então as travas sempre são concedidas."""

FD_CLOEXEC = 1
F_DUPFD = 0
F_GETFD = 1
F_SETFD = 2
F_GETFL = 3
F_SETFL = 4
F_GETLK = 5
F_SETLK = 6
F_SETLKW = 7
F_RDLCK = 0
F_WRLCK = 1
F_UNLCK = 2
LOCK_SH = 1
LOCK_EX = 2
LOCK_NB = 4
LOCK_UN = 8


def _fileno(fd):
    if hasattr(fd, 'fileno'):
        fd = fd.fileno()
    if not isinstance(fd, int):
        raise TypeError('argument must be an int, or have a fileno() method.')
    if fd < 0:
        raise ValueError('file descriptor cannot be a negative integer (%d)' % fd)
    return fd


def flock(fd, operation):
    _fileno(fd)
    if operation not in (LOCK_SH, LOCK_EX, LOCK_UN) and operation & ~LOCK_NB not in (LOCK_SH, LOCK_EX, LOCK_UN):
        raise OSError(22, 'Invalid argument')


def lockf(fd, cmd, len=0, start=0, whence=0):
    _fileno(fd)
    if cmd not in (LOCK_SH, LOCK_EX, LOCK_UN) and cmd & ~LOCK_NB not in (LOCK_SH, LOCK_EX, LOCK_UN):
        raise ValueError('unrecognized lockf argument')
