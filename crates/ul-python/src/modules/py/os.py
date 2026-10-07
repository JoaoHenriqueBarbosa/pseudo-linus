"""Módulo os do sandbox: interface POSIX sobre as chamadas `_os` (VFS do pseudo-linus)."""

import _os
from abc import ABCMeta as _ABCMeta
from types import GenericAlias as _GenericAlias
import posixpath as path
from posixpath import curdir, pardir, sep, pathsep, defpath, extsep, altsep, devnull

name = 'posix'
linesep = '\n'

F_OK = 0
X_OK = 1
W_OK = 2
R_OK = 4
O_RDONLY = _os.O_RDONLY
O_WRONLY = _os.O_WRONLY
O_RDWR = _os.O_RDWR
O_CREAT = _os.O_CREAT
O_EXCL = _os.O_EXCL
O_TRUNC = _os.O_TRUNC
O_APPEND = _os.O_APPEND
# Valores do x86-64 Linux (asm-generic/fcntl.h), os mesmos do `os` do Debian.
O_ACCMODE = 3
O_NOCTTY = 0o400
O_NONBLOCK = O_NDELAY = 0o4000
O_DSYNC = 0o10000
O_ASYNC = 0o20000
O_DIRECT = 0o40000
O_LARGEFILE = 0
O_DIRECTORY = 0o200000
O_NOFOLLOW = 0o400000
O_NOATIME = 0o1000000
O_CLOEXEC = 0o2000000
O_SYNC = O_RSYNC = O_FSYNC = 0o4010000
O_PATH = 0o10000000
O_TMPFILE = 0o20200000
SEEK_SET = 0
SEEK_CUR = 1
SEEK_END = 2

error = OSError


class PathLike(metaclass=_ABCMeta):
    """Classe base de objetos que representam caminhos (`__fspath__`)."""

    def __fspath__(self):
        raise NotImplementedError

    @classmethod
    def __subclasshook__(cls, subclass):
        return hasattr(subclass, '__fspath__')

    __class_getitem__ = classmethod(_GenericAlias)


def _get_exports_list(module):
    try:
        return list(module.__all__)
    except AttributeError:
        return [n for n in dir(module) if n[0] != '_']


def fspath(p):
    if isinstance(p, (str, bytes)):
        return p
    meth = getattr(p, '__fspath__', None)
    if meth is None:
        raise TypeError('expected str, bytes or os.PathLike object, not ' + type(p).__name__)
    return meth()


def fsencode(filename):
    filename = fspath(filename)
    if isinstance(filename, str):
        return filename.encode('utf-8', 'surrogateescape')
    return filename


def fsdecode(filename):
    filename = fspath(filename)
    if isinstance(filename, bytes):
        return filename.decode('utf-8', 'surrogateescape')
    return filename


_STRERROR = {
    1: 'Operation not permitted',
    2: 'No such file or directory',
    3: 'No such process',
    4: 'Interrupted system call',
    5: 'Input/output error',
    6: 'No such device or address',
    7: 'Argument list too long',
    8: 'Exec format error',
    9: 'Bad file descriptor',
    10: 'No child processes',
    11: 'Resource temporarily unavailable',
    12: 'Cannot allocate memory',
    13: 'Permission denied',
    14: 'Bad address',
    15: 'Block device required',
    16: 'Device or resource busy',
    17: 'File exists',
    18: 'Invalid cross-device link',
    19: 'No such device',
    20: 'Not a directory',
    21: 'Is a directory',
    22: 'Invalid argument',
    23: 'Too many open files in system',
    24: 'Too many open files',
    25: 'Inappropriate ioctl for device',
    26: 'Text file busy',
    27: 'File too large',
    28: 'No space left on device',
    29: 'Illegal seek',
    30: 'Read-only file system',
    31: 'Too many links',
    32: 'Broken pipe',
    33: 'Numerical argument out of domain',
    34: 'Numerical result out of range',
    35: 'Resource deadlock avoided',
    36: 'File name too long',
    37: 'No locks available',
    38: 'Function not implemented',
    39: 'Directory not empty',
    40: 'Too many levels of symbolic links',
    41: 'Unknown error 41',
    42: 'No message of desired type',
    43: 'Identifier removed',
    44: 'Channel number out of range',
    45: 'Level 2 not synchronized',
    46: 'Level 3 halted',
    47: 'Level 3 reset',
    48: 'Link number out of range',
    49: 'Protocol driver not attached',
    50: 'No CSI structure available',
    51: 'Level 2 halted',
    52: 'Invalid exchange',
    53: 'Invalid request descriptor',
    54: 'Exchange full',
    55: 'No anode',
    56: 'Invalid request code',
    57: 'Invalid slot',
    58: 'Unknown error 58',
    59: 'Bad font file format',
    60: 'Device not a stream',
    61: 'No data available',
    62: 'Timer expired',
    63: 'Out of streams resources',
    64: 'Machine is not on the network',
    65: 'Package not installed',
    66: 'Object is remote',
    67: 'Link has been severed',
    68: 'Advertise error',
    69: 'Srmount error',
    70: 'Communication error on send',
    71: 'Protocol error',
    72: 'Multihop attempted',
    73: 'RFS specific error',
    74: 'Bad message',
    75: 'Value too large for defined data type',
    76: 'Name not unique on network',
    77: 'File descriptor in bad state',
    78: 'Remote address changed',
    79: 'Can not access a needed shared library',
    80: 'Accessing a corrupted shared library',
    81: '.lib section in a.out corrupted',
    82: 'Attempting to link in too many shared libraries',
    83: 'Cannot exec a shared library directly',
    84: 'Invalid or incomplete multibyte or wide character',
    85: 'Interrupted system call should be restarted',
    86: 'Streams pipe error',
    87: 'Too many users',
    88: 'Socket operation on non-socket',
    89: 'Destination address required',
    90: 'Message too long',
    91: 'Protocol wrong type for socket',
    92: 'Protocol not available',
    93: 'Protocol not supported',
    94: 'Socket type not supported',
    95: 'Operation not supported',
    96: 'Protocol family not supported',
    97: 'Address family not supported by protocol',
    98: 'Address already in use',
    99: 'Cannot assign requested address',
    100: 'Network is down',
    101: 'Network is unreachable',
    102: 'Network dropped connection on reset',
    103: 'Software caused connection abort',
    104: 'Connection reset by peer',
    105: 'No buffer space available',
    106: 'Transport endpoint is already connected',
    107: 'Transport endpoint is not connected',
    108: 'Cannot send after transport endpoint shutdown',
    109: 'Too many references: cannot splice',
    110: 'Connection timed out',
    111: 'Connection refused',
    112: 'Host is down',
    113: 'No route to host',
    114: 'Operation already in progress',
    115: 'Operation now in progress',
    116: 'Stale file handle',
    117: 'Structure needs cleaning',
    118: 'Not a XENIX named type file',
    119: 'No XENIX semaphores available',
    120: 'Is a named type file',
    121: 'Remote I/O error',
    122: 'Disk quota exceeded',
    123: 'No medium found',
    124: 'Wrong medium type',
    125: 'Operation canceled',
    126: 'Required key not available',
    127: 'Key has expired',
    128: 'Key has been revoked',
    129: 'Key was rejected by service',
    130: 'Owner died',
    131: 'State not recoverable',
    132: 'Operation not possible due to RF-kill',
    133: 'Memory page has hardware error',
}


def strerror(code):
    return _STRERROR.get(code, "Unknown error %d" % code)


def getcwd():
    return _os.getcwd()


def chdir(p):
    _os.chdir(fspath(p))


def listdir(p='.'):
    return _os.listdir(fspath(p))


class stat_result:
    """Resultado de `os.stat` (acessível por atributo e por índice)."""

    def __init__(self, t):
        self._t = tuple(t)
        self.st_mode = t[0]
        self.st_ino = t[1]
        self.st_dev = t[2]
        self.st_nlink = t[3]
        self.st_uid = t[4]
        self.st_gid = t[5]
        self.st_size = t[6]
        self.st_atime = t[7]
        self.st_mtime = t[8]
        self.st_ctime = t[9]
        self.st_atime_ns = int(t[7] * 1000000000)
        self.st_mtime_ns = int(t[8] * 1000000000)
        self.st_ctime_ns = int(t[9] * 1000000000)
        self.st_blksize = 4096
        self.st_blocks = (t[6] + 511) // 512

    def __getitem__(self, i):
        return self._t[i]

    def __len__(self):
        return len(self._t)

    def __iter__(self):
        return iter(self._t)

    def __repr__(self):
        return ('os.stat_result(st_mode=%d, st_ino=%d, st_dev=%d, st_nlink=%d, st_uid=%d, st_gid=%d, '
                'st_size=%d, st_atime=%d, st_mtime=%d, st_ctime=%d)' % (
                    self.st_mode, self.st_ino, self.st_dev, self.st_nlink, self.st_uid, self.st_gid,
                    self.st_size, int(self.st_atime), int(self.st_mtime), int(self.st_ctime)))


def stat(p, *, follow_symlinks=True):
    return stat_result(_os.stat(fspath(p), follow_symlinks))


def lstat(p):
    return stat_result(_os.stat(fspath(p), False))


def fstat(fd):
    return stat_result(_os.fstat(fd))


class DirEntry:
    """Entrada devolvida por `os.scandir`."""

    def __init__(self, dirpath, name, kind):
        self.name = name
        self.path = path.join(dirpath, name)
        self._kind = kind

    def is_dir(self, *, follow_symlinks=True):
        if self._kind == 'l' and follow_symlinks:
            return path.isdir(self.path)
        return self._kind == 'd'

    def is_file(self, *, follow_symlinks=True):
        if self._kind == 'l' and follow_symlinks:
            return path.isfile(self.path)
        return self._kind == 'f'

    def is_symlink(self):
        return self._kind == 'l'

    def stat(self, *, follow_symlinks=True):
        return stat(self.path, follow_symlinks=follow_symlinks)

    def inode(self):
        return _os.stat(self.path, False)[1]

    def __fspath__(self):
        return self.path

    def __repr__(self):
        return '<DirEntry %r>' % (self.name,)


class _ScandirIterator:
    def __init__(self, entries):
        self._entries = entries
        self._it = iter(entries)

    def __iter__(self):
        return self

    def __next__(self):
        return next(self._it)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def close(self):
        pass


def scandir(p='.'):
    p = fspath(p)
    return _ScandirIterator([DirEntry(p, n, k) for n, k in _os.scandir(p)])


def mkdir(p, mode=0o777):
    _os.mkdir(fspath(p), mode)


def makedirs(name, mode=0o777, exist_ok=False):
    name = fspath(name)
    head, tail = path.split(name)
    if not tail:
        head, tail = path.split(head)
    if head and tail and not path.exists(head):
        try:
            makedirs(head, exist_ok=exist_ok)
        except FileExistsError:
            pass
        cdir = curdir
        if tail == cdir:
            return
    try:
        mkdir(name, mode)
    except OSError:
        if not exist_ok or not path.isdir(name):
            raise


def remove(p):
    _os.unlink(fspath(p))


unlink = remove


def rmdir(p):
    _os.rmdir(fspath(p))


def removedirs(name):
    rmdir(name)
    head, tail = path.split(name)
    if not tail:
        head, tail = path.split(head)
    while head and tail:
        try:
            rmdir(head)
        except OSError:
            break
        head, tail = path.split(head)


def rename(src, dst):
    _os.rename(fspath(src), fspath(dst))


def replace(src, dst):
    _os.rename(fspath(src), fspath(dst))


def readlink(p):
    return _os.readlink(fspath(p))


def symlink(src, dst, target_is_directory=False):
    _os.symlink(fspath(src), fspath(dst))


def chmod(p, mode):
    _os.chmod(fspath(p), mode)


def chown(path, uid, gid, *, dir_fd=None, follow_symlinks=True):
    if follow_symlinks:
        _os.chown(fspath(path), uid, gid)
    else:
        _os.lchown(fspath(path), uid, gid)


def lchown(path, uid, gid):
    _os.lchown(fspath(path), uid, gid)


def access(p, mode):
    return _os.access(fspath(p), mode)


_walk_symlinks_as_files = object()


def walk(top, topdown=True, onerror=None, followlinks=False):
    top = fspath(top)
    try:
        entries = _os.scandir(top)
    except OSError as err:
        if onerror is not None:
            onerror(err)
        return
    dirs = []
    nondirs = []
    as_files = followlinks is _walk_symlinks_as_files
    for name, kind in entries:
        is_dir = kind == 'd' or (kind == 'l' and not as_files and path.isdir(path.join(top, name)))
        if is_dir:
            dirs.append(name)
        else:
            nondirs.append(name)
    if topdown:
        yield top, dirs, nondirs
    for name in dirs:
        new_path = path.join(top, name)
        if followlinks or not path.islink(new_path):
            yield from walk(new_path, topdown, onerror, followlinks)
    if not topdown:
        yield top, dirs, nondirs


# Change environ to automatically call putenv() and unsetenv()
from _collections_abc import MutableMapping, Mapping

class _Environ(MutableMapping):
    def __init__(self, data, encodekey, decodekey, encodevalue, decodevalue):
        self.encodekey = encodekey
        self.decodekey = decodekey
        self.encodevalue = encodevalue
        self.decodevalue = decodevalue
        self._data = data

    def __getitem__(self, key):
        try:
            value = self._data[self.encodekey(key)]
        except KeyError:
            # raise KeyError with the original key value
            raise KeyError(key) from None
        return self.decodevalue(value)

    def __setitem__(self, key, value):
        key = self.encodekey(key)
        value = self.encodevalue(value)
        putenv(key, value)
        self._data[key] = value

    def __delitem__(self, key):
        encodedkey = self.encodekey(key)
        unsetenv(encodedkey)
        try:
            del self._data[encodedkey]
        except KeyError:
            # raise KeyError with the original key value
            raise KeyError(key) from None

    def __iter__(self):
        # list() from dict object is an atomic operation
        keys = list(self._data)
        for key in keys:
            yield self.decodekey(key)

    def __len__(self):
        return len(self._data)

    def __repr__(self):
        formatted_items = ", ".join(
            f"{self.decodekey(key)!r}: {self.decodevalue(value)!r}"
            for key, value in self._data.items()
        )
        return f"environ({{{formatted_items}}})"

    def copy(self):
        return dict(self)

    def setdefault(self, key, value):
        if key not in self:
            self[key] = value
        return self[key]

    def __ior__(self, other):
        self.update(other)
        return self

    def __or__(self, other):
        if not isinstance(other, Mapping):
            return NotImplemented
        new = dict(self)
        new.update(other)
        return new

    def __ror__(self, other):
        if not isinstance(other, Mapping):
            return NotImplemented
        new = dict(other)
        new.update(self)
        return new

def _createenviron():
    # Where Env Var Names Can Be Mixed Case
    encoding = 'utf-8'
    def encode(value):
        if not isinstance(value, str):
            raise TypeError("str expected, not %s" % type(value).__name__)
        return value.encode(encoding, 'surrogateescape')
    def decode(value):
        return value.decode(encoding, 'surrogateescape')
    encodekey = encode
    # O `posix.environ`: o ambiente da partida, em bytes.
    data = {encode(k): encode(v) for k, v in _os.environ()}
    return _Environ(data,
        encodekey, decode,
        encode, decode)

# unicode environ
environ = _createenviron()
del _createenviron


def getenv(key, default=None):
    """Get an environment variable, return None if it doesn't exist.
    The optional second argument can specify an alternate default.
    key, default and the result are str."""
    return environ.get(key, default)

supports_bytes_environ = (name != 'nt')


def _check_bytes(value):
    if not isinstance(value, bytes):
        raise TypeError("bytes expected, not %s" % type(value).__name__)
    return value

# bytes environ
environb = _Environ(environ._data,
    _check_bytes, bytes,
    _check_bytes, bytes)
del _check_bytes


def getenvb(key, default=None):
    """Get an environment variable, return None if it doesn't exist.
    The optional second argument can specify an alternate default.
    key, default and the result are bytes."""
    return environb.get(key, default)


def _env_arg(value):
    # O `PyUnicode_FSConverter` do `posix`: str, bytes ou PathLike, sem byte nulo.
    value = fspath(value)
    if isinstance(value, str):
        value = value.encode('utf-8', 'surrogateescape')
    if b'\0' in value:
        raise ValueError('embedded null byte')
    return value


def putenv(name, value, /):
    """Change or add an environment variable."""
    name = _env_arg(name)
    value = _env_arg(value)
    if not name:
        raise OSError(22, 'Invalid argument')
    if b'=' in name:
        raise ValueError('illegal environment variable name')
    _os.putenv(name.decode('utf-8', 'surrogateescape'), value.decode('utf-8', 'surrogateescape'))


def unsetenv(name, /):
    """Delete an environment variable."""
    name = _env_arg(name)
    if not name or b'=' in name:
        raise OSError(22, 'Invalid argument')
    _os.unsetenv(name.decode('utf-8', 'surrogateescape'))


def getpid():
    return _os.getpid()


def getppid():
    return _os.getppid()


def umask(mask, /):
    """Set the current numeric umask and return the previous umask."""
    return _os.umask(mask)


def _as_fd(fd):
    if isinstance(fd, int):
        return fd
    fileno = getattr(fd, 'fileno', None)
    if fileno is None:
        raise TypeError('argument must be an int, or have a fileno() method.')
    fd = fileno()
    if not isinstance(fd, int):
        raise TypeError('fileno() returned a non-integer')
    if fd < 0:
        raise ValueError('file descriptor cannot be a negative integer (%d)' % fd)
    return fd


def fsync(fd):
    """Force write of fd to disk.

    fd
      Either an integer file descriptor, or an object with a fileno() method."""
    _os.fsync(_as_fd(fd))


def fdatasync(fd):
    """Force write of fd to disk without forcing update of metadata."""
    _os.fsync(_as_fd(fd))


def sync():
    """Force write of everything to disk."""


def _exit(status):
    """Sai na hora: descarrega os fluxos, mas não roda as funções do `atexit`."""
    import sys
    import atexit
    atexit._clear()
    try:
        sys.stdout.flush()
        sys.stderr.flush()
    except Exception:
        pass
    raise SystemExit(status)


def urandom(n):
    return _os.urandom(n)


def utime(p, times=None, *, ns=None, dir_fd=None, follow_symlinks=True):
    p = fspath(p)
    if ns is not None:
        _os.utime(p, ns[0] / 1e9, ns[1] / 1e9)
    elif times is None:
        _os.utime(p, None, None)
    else:
        _os.utime(p, times[0], times[1])


def truncate(p, length):
    fd = _os.open(fspath(p), O_WRONLY, 0)
    try:
        _os.ftruncate(fd, length)
    finally:
        _os.close(fd)


def ftruncate(fd, length):
    _os.ftruncate(fd, length)


def cpu_count():
    return 1


# `confstr(3)` da glibc 2.41 do Debian 13 em x86-64.
confstr_names = {
    'CS_GNU_LIBC_VERSION': 2, 'CS_GNU_LIBPTHREAD_VERSION': 3, 'CS_LFS64_CFLAGS': 1004, 'CS_LFS64_LDFLAGS': 1005,
    'CS_LFS64_LIBS': 1006, 'CS_LFS64_LINTFLAGS': 1007, 'CS_LFS_CFLAGS': 1000, 'CS_LFS_LDFLAGS': 1001,
    'CS_LFS_LIBS': 1002, 'CS_LFS_LINTFLAGS': 1003, 'CS_PATH': 0, 'CS_XBS5_ILP32_OFF32_CFLAGS': 1100,
    'CS_XBS5_ILP32_OFF32_LDFLAGS': 1101, 'CS_XBS5_ILP32_OFF32_LIBS': 1102, 'CS_XBS5_ILP32_OFF32_LINTFLAGS': 1103,
    'CS_XBS5_ILP32_OFFBIG_CFLAGS': 1104, 'CS_XBS5_ILP32_OFFBIG_LDFLAGS': 1105, 'CS_XBS5_ILP32_OFFBIG_LIBS': 1106,
    'CS_XBS5_ILP32_OFFBIG_LINTFLAGS': 1107, 'CS_XBS5_LP64_OFF64_CFLAGS': 1108, 'CS_XBS5_LP64_OFF64_LDFLAGS': 1109,
    'CS_XBS5_LP64_OFF64_LIBS': 1110, 'CS_XBS5_LP64_OFF64_LINTFLAGS': 1111, 'CS_XBS5_LPBIG_OFFBIG_CFLAGS': 1112,
    'CS_XBS5_LPBIG_OFFBIG_LDFLAGS': 1113, 'CS_XBS5_LPBIG_OFFBIG_LIBS': 1114,
    'CS_XBS5_LPBIG_OFFBIG_LINTFLAGS': 1115,
}
_CONFSTR_VALUES = {
    0: '/bin:/usr/bin', 2: 'glibc 2.41', 3: 'NPTL 2.41', 1004: '-D_LARGEFILE64_SOURCE',
    1007: '-D_LARGEFILE64_SOURCE', 1108: '-m64', 1109: '-m64',
}


def confstr(name, /):
    """Return a string-valued system configuration variable."""
    if isinstance(name, str):
        if name not in confstr_names:
            raise ValueError('unrecognized configuration name')
        name = confstr_names[name]
    elif not isinstance(name, int):
        raise TypeError('configuration names must be strings or integers')
    if name not in confstr_names.values():
        raise OSError(22, 'Invalid argument')
    return _CONFSTR_VALUES.get(name, '')


confstr.__module__ = 'posix'


def get_terminal_size(fd=1):
    return terminal_size((80, 24))


class terminal_size:
    def __init__(self, t):
        self.columns, self.lines = t

    def __getitem__(self, i):
        return (self.columns, self.lines)[i]

    def __iter__(self):
        return iter((self.columns, self.lines))

    def __len__(self):
        return 2

    def __repr__(self):
        return 'os.terminal_size(columns=%d, lines=%d)' % (self.columns, self.lines)


def isatty(fd):
    return _os.isatty(fd)


def open(p, flags, mode=0o777):
    return _os.open(fspath(p), flags, mode)


def close(fd):
    _os.close(fd)


def read(fd, n):
    return _os.read(fd, n)


def set_blocking(fd, blocking):
    _os.set_blocking(fd, blocking)

def write(fd, data):
    return _os.write(fd, data)


def lseek(fd, pos, how):
    return _os.lseek(fd, pos, how)


def getuid():
    return 0


def getgid():
    return 0


def geteuid():
    return 0


def getlogin():
    return getenv('USER', 'root')


class uname_result(tuple):
    """`os.uname()`: tupla de cinco campos com nomes."""

    _fields = ('sysname', 'nodename', 'release', 'version', 'machine')
    n_fields = 5
    n_sequence_fields = 5
    n_unnamed_fields = 0

    def __new__(cls, sequence):
        return tuple.__new__(cls, tuple(sequence))

    sysname = property(lambda self: self[0])
    nodename = property(lambda self: self[1])
    release = property(lambda self: self[2])
    version = property(lambda self: self[3])
    machine = property(lambda self: self[4])

    def __repr__(self):
        return 'posix.uname_result(sysname=%r, nodename=%r, release=%r, version=%r, machine=%r)' % tuple(self)


def uname():
    return uname_result(('Linux', 'localhost', '6.12.0', '#1 SMP', 'x86_64'))


def kill(pid, sig):
    _os.kill(pid, sig)


def system(command):
    """Roda `command` no `/bin/sh -c` e devolve o status de espera (código << 8, ou o sinal)."""
    import subprocess
    code = subprocess.call(command, shell=True)
    return (-code if code < 0 else code << 8)


class _wrap_close:
    def __init__(self, stream, proc):
        self._stream = stream
        self._proc = proc

    def close(self):
        self._stream.close()
        returncode = self._proc.wait()
        if returncode == 0:
            return None
        return returncode << 8 if returncode > 0 else -returncode

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    def __getattr__(self, name):
        return getattr(self._stream, name)

    def __iter__(self):
        return iter(self._stream)


def popen(cmd, mode='r', buffering=-1):
    import subprocess
    if mode == 'r':
        proc = subprocess.Popen(cmd, shell=True, text=True, stdout=subprocess.PIPE, bufsize=buffering)
        return _wrap_close(proc.stdout, proc)
    if mode == 'w':
        proc = subprocess.Popen(cmd, shell=True, text=True, stdin=subprocess.PIPE, bufsize=buffering)
        return _wrap_close(proc.stdin, proc)
    raise ValueError('invalid mode %r' % mode)


# O módulo `posix` (de onde o CPython tira estas funções) recebe-as quando o `os` termina de carregar.
def _init_posix():
    init = globals().pop('_posix_init', None)
    if init is not None:
        init()

_init_posix()
del _init_posix
