"""This module performs file control and I/O control on file descriptors.  It is an interface to the fcntl()
and ioctl() Unix routines.  File descriptors can be obtained with the fileno() method of a file or socket object."""

import _net
import _os
import sys

FASYNC = 8192
FD_CLOEXEC = 1
F_DUPFD = 0
F_DUPFD_CLOEXEC = 1030
F_GETFD = 1
F_GETFL = 3
F_GETLK = 5
F_GETLK64 = 5
F_GETOWN = 9
F_GETSIG = 11
F_RDLCK = 0
F_SETFD = 2
F_SETFL = 4
F_SETLK = 6
F_SETLK64 = 6
F_SETLKW = 7
F_SETLKW64 = 7
F_SETOWN = 8
F_SETSIG = 10
F_UNLCK = 2
F_WRLCK = 1
F_EXLCK = 4
F_SHLCK = 8
F_SETLEASE = 1024
F_GETLEASE = 1025
F_NOTIFY = 1026
F_SETPIPE_SZ = 1031
F_GETPIPE_SZ = 1032
F_ADD_SEALS = 1033
F_GET_SEALS = 1034
F_SEAL_SEAL = 1
F_SEAL_SHRINK = 2
F_SEAL_GROW = 4
F_SEAL_WRITE = 8
F_OFD_GETLK = 36
F_OFD_SETLK = 37
F_OFD_SETLKW = 38
LOCK_SH = 1
LOCK_EX = 2
LOCK_NB = 4
LOCK_UN = 8
LOCK_MAND = 32
LOCK_READ = 64
LOCK_WRITE = 128
LOCK_RW = 192
DN_ACCESS = 1
DN_MODIFY = 2
DN_CREATE = 4
DN_DELETE = 8
DN_RENAME = 16
DN_ATTRIB = 32
DN_MULTISHOT = 2147483648

_INT_MAX = 2 ** 31 - 1
_INT_MIN = -2 ** 31
_BUFFER_SIZE = 1024


def _fileno(fd):
    """`PyObject_AsFileDescriptor` mais a conferência de `fildes`: um `int` ou algo com `fileno()`, nunca negativo."""
    if not isinstance(fd, int):
        method = getattr(fd, 'fileno', None)
        if method is None:
            raise TypeError('argument must be an int, or have a fileno() method.')
        fd = method()
        if not isinstance(fd, int):
            raise TypeError('fileno() returned a non-integer')
    if not _INT_MIN <= fd <= _INT_MAX:
        raise OverflowError('Python int too large to convert to C int')
    if fd < 0:
        raise ValueError('file descriptor cannot be a negative integer (%d)' % fd)
    return fd


def _c_int(value):
    """Um argumento `int` do argument clinic: `__index__` e a faixa do `int` de C."""
    if not isinstance(value, int):
        index = getattr(type(value), '__index__', None)
        if index is None:
            raise TypeError("'%s' object cannot be interpreted as an integer" % type(value).__name__)
        value = index(value)
    if value > _INT_MAX:
        raise OverflowError('signed integer is greater than maximum')
    if value < _INT_MIN:
        raise OverflowError('signed integer is less than minimum')
    return value


def _read_only_bytes(arg):
    """Os bytes de um argumento que o `"s#"`/`"s*"` do CPython aceita: `str`, `bytes` ou outro objeto de
    buffer somente leitura; `None` quando não é nenhum desses."""
    if isinstance(arg, str):
        return arg.encode()
    if isinstance(arg, bytes):
        return bytes(arg)
    if isinstance(arg, memoryview) and arg.readonly:
        return arg.tobytes()
    return None


def _is_writable_buffer(arg):
    if isinstance(arg, bytearray):
        return True
    if isinstance(arg, memoryview):
        return not arg.readonly
    return hasattr(arg, 'typecode') and hasattr(arg, 'frombytes') and hasattr(arg, 'tobytes')


def _buffer_bytes(arg):
    return arg.tobytes() if hasattr(arg, 'tobytes') else bytes(arg)


def _store_buffer(arg, data):
    """Devolve ao buffer do programa os bytes que a chamada de sistema escreveu (o `memcpy` de volta)."""
    if isinstance(arg, bytearray):
        arg[:len(data)] = data
    elif isinstance(arg, memoryview):
        arg.cast('B')[:len(data)] = data
    else:
        arg[:] = type(arg)(arg.typecode, data)


def _int_argument(arg, message):
    """O `"I"` do CPython: um `int` (ou `__index__`) e só os 32 bits de baixo."""
    if isinstance(arg, int):
        return arg & 0xFFFFFFFF
    index = getattr(type(arg), '__index__', None)
    if index is None or isinstance(arg, float):
        raise TypeError(message)
    return index(arg) & 0xFFFFFFFF


def fcntl(fd, cmd, arg=0, /):
    """Perform the operation `cmd` on file descriptor fd.

The values used for `cmd` are operating system dependent, and are available
as constants in the fcntl module, using the same names as used in
the relevant C header files.  The argument arg is optional, and
defaults to 0; it may be an int or a string.  If arg is given as a string,
the return value of fcntl is a string of that length, containing the
resulting value put in the arg buffer by the operating system.  The length
of the arg string is not allowed to exceed 1024 bytes.  If the arg given
is an integer or if none is specified, the result value is an integer
corresponding to the return value of the fcntl call in the C code."""
    fd = _fileno(fd)
    cmd = _c_int(cmd)
    sys.audit('fcntl.fcntl', fd, cmd, arg)
    data = _read_only_bytes(arg)
    if data is not None:
        if len(data) > _BUFFER_SIZE:
            raise ValueError('fcntl argument 3 is too long')
        return _os.fcntl_buffer(fd, cmd, data)
    value = _int_argument(arg, 'fcntl requires a file or file descriptor, an integer and optionally a third '
                               'integer or a string')
    return _os.fcntl(fd, cmd, value)


def ioctl(fd, request, arg=0, mutate_flag=True, /):
    """Perform the operation `request` on file descriptor `fd`.

The values used for `request` are operating system dependent, and are available
as constants in the termios module, using the same names as used in the relevant
C header files.

The argument `arg` is optional, and defaults to 0; it may be an int or a
buffer containing character data (most likely a string or an array).

If the argument is a mutable buffer (such as an array) and if the
mutate_flag argument (which is only allowed in this case) is true then the
buffer is (in effect) passed to the operating system and changes made by
the OS will be reflected in the contents of the buffer after the call has
returned.  The return value is the integer returned by the ioctl system
call.

If the argument is a mutable buffer and the mutable_flag argument is false,
the behavior is as if a string had been passed.

If the argument is an immutable buffer (most likely a string) then a copy
of the buffer is passed to the operating system and the return value is a
string of the same length containing whatever the operating system put in
the buffer.  The length of the arg buffer in this case is not allowed to
exceed 1024 bytes.

If the arg given is an integer or if none is specified, the result value is
an integer corresponding to the return value of the ioctl call in the C
code."""
    fd = _fileno(fd)
    request = _c_int_unsigned(request)
    sys.audit('fcntl.ioctl', fd, request, arg)
    if _is_writable_buffer(arg):
        data = _buffer_bytes(arg)
        if mutate_flag:
            result = _os.ioctl(fd, request, data)
            _store_buffer(arg, result)
            return 0
        if len(data) > _BUFFER_SIZE:
            raise ValueError('ioctl argument is too long')
        return _os.ioctl(fd, request, data)
    data = _read_only_bytes(arg)
    if data is not None:
        if len(data) > _BUFFER_SIZE:
            raise ValueError('ioctl argument is too long')
        return _os.ioctl(fd, request, data)
    message = 'ioctl requires a file or file descriptor, an integer and optionally an integer or buffer argument'
    if not isinstance(arg, int):
        index = getattr(type(arg), '__index__', None)
        if index is None or isinstance(arg, float):
            raise TypeError(message)
        arg = index(arg)
    if not _INT_MIN <= arg <= _INT_MAX:
        raise OverflowError('signed integer is greater than maximum' if arg > 0 else 'signed integer is less than minimum')
    _os.ioctl(fd, request, None)
    return 0


def _c_int_unsigned(value):
    """O `request` do `ioctl`: `unsigned int` com `bitwise=True`, que aceita qualquer `int` e guarda os 32 bits."""
    if not isinstance(value, int):
        index = getattr(type(value), '__index__', None)
        if index is None:
            raise TypeError("'%s' object cannot be interpreted as an integer" % type(value).__name__)
        value = index(value)
    return value & 0xFFFFFFFF


def flock(fd, operation, /):
    """Perform the lock operation `operation` on file descriptor `fd`.

See the Unix manual page for flock(2) for details (On some systems, this
function is emulated using fcntl())."""
    fd = _fileno(fd)
    operation = _c_int(operation)
    sys.audit('fcntl.flock', fd, operation)
    if operation in (LOCK_SH, LOCK_EX) and _net.cooperative() is not None:
        # A trava pode estar com outra thread (outra descrição de arquivo do mesmo processo conflita): tenta
        # sem bloquear e, enquanto não vem, as outras threads rodam; o erro de qualquer outro tipo sobe já.
        nonblocking = operation | LOCK_NB

        def attempt():
            try:
                _os.flock(fd, nonblocking)
            except OSError as e:
                if e.errno != 11:
                    raise
                return False
            return True

        _net.wait_retry(attempt, 'flock()')
        return
    _os.flock(fd, operation)


def _long_long(value, name):
    """`PyLong_AsLongLong`: o argumento de `lockf` que vira `off_t`."""
    if not isinstance(value, int):
        index = getattr(type(value), '__index__', None)
        if index is None:
            raise TypeError("'%s' object cannot be interpreted as an integer" % type(value).__name__)
        value = index(value)
    return value


def lockf(fd, cmd, len=0, start=0, whence=0, /):
    """A wrapper around the fcntl() locking calls.

`fd` is the file descriptor of the file to lock or unlock, and operation is one
of the following values:

    LOCK_UN - unlock
    LOCK_SH - acquire a shared lock
    LOCK_EX - acquire an exclusive lock

When operation is LOCK_SH or LOCK_EX, it can also be bitwise ORed with
LOCK_NB to avoid blocking on lock acquisition.  If LOCK_NB is used and the
lock cannot be acquired, an OSError will be raised and the exception will
have an errno attribute set to EACCES or EAGAIN (depending on the operating
system -- for portability, check for either value).

`len` is the number of bytes to lock, with the default meaning to lock to
EOF.  `start` is the byte offset, relative to `whence`, to that the lock
starts.  `whence` is as with fileobj.seek(), specifically:

    0 - relative to the start of the file (SEEK_SET)
    1 - relative to the current buffer position (SEEK_CUR)
    2 - relative to the end of the file (SEEK_END)"""
    fd = _fileno(fd)
    cmd = _c_int(cmd)
    whence = _c_int(whence)
    sys.audit('fcntl.lockf', fd, cmd, len, start, whence)
    if cmd == LOCK_UN:
        l_type = F_UNLCK
    elif cmd & LOCK_SH:
        l_type = F_RDLCK
    elif cmd & LOCK_EX:
        l_type = F_WRLCK
    else:
        raise ValueError('unrecognized lockf argument')
    l_start = 0 if start is None else _long_long(start, 'start')
    l_len = 0 if len is None else _long_long(len, 'len')
    # O `struct flock` do x86-64: `l_type` e `l_whence` (`short`), 4 bytes de enchimento, `l_start`, `l_len`,
    # `l_pid` e mais 4 de enchimento.
    flock_struct = (l_type.to_bytes(2, 'little') + (whence & 0xFFFF).to_bytes(2, 'little') + bytes(4)
                    + l_start.to_bytes(8, 'little', signed=True) + l_len.to_bytes(8, 'little', signed=True) + bytes(8))
    _os.fcntl_buffer(fd, F_SETLK if cmd & LOCK_NB else F_SETLKW, flock_struct)
