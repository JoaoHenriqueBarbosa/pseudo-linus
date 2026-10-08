"""Módulo os do sandbox: interface POSIX sobre as chamadas `_os` (VFS do pseudo-linus)."""

# Combinado com o `posix`: ele deixa aqui o `_init` se for importado enquanto o `os` carrega (ver `_init_posix`).
# Precisa existir antes dos imports, que trazem o `posixpath` e, com ele, o `posix`.
_posix_init = None

import _os
import abc
import sys
import stat as st
from types import GenericAlias
import posixpath as path
sys.modules['os.path'] = path
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
SEEK_DATA = 3
SEEK_HOLE = 4

# Constantes do posix na glibc 2.41 / Linux x86_64 (os mesmos valores do Debian 13).
CLD_EXITED, CLD_KILLED, CLD_DUMPED, CLD_TRAPPED, CLD_STOPPED, CLD_CONTINUED = 1, 2, 3, 4, 5, 6
CLONE_VM, CLONE_FS, CLONE_FILES, CLONE_SIGHAND = 256, 512, 1024, 2048
CLONE_THREAD, CLONE_NEWNS, CLONE_SYSVSEM, CLONE_NEWTIME = 65536, 131072, 262144, 128
CLONE_NEWCGROUP, CLONE_NEWUTS, CLONE_NEWIPC = 33554432, 67108864, 134217728
CLONE_NEWUSER, CLONE_NEWPID, CLONE_NEWNET = 268435456, 536870912, 1073741824
EFD_SEMAPHORE, EFD_NONBLOCK, EFD_CLOEXEC = 1, 2048, 524288
EX_OK, EX_USAGE, EX_DATAERR, EX_NOINPUT, EX_NOUSER, EX_NOHOST = 0, 64, 65, 66, 67, 68
EX_UNAVAILABLE, EX_SOFTWARE, EX_OSERR, EX_OSFILE, EX_CANTCREAT = 69, 70, 71, 72, 73
EX_IOERR, EX_TEMPFAIL, EX_PROTOCOL, EX_NOPERM, EX_CONFIG = 74, 75, 76, 77, 78
F_ULOCK, F_LOCK, F_TLOCK, F_TEST = 0, 1, 2, 3
GRND_NONBLOCK, GRND_RANDOM = 1, 2
MFD_CLOEXEC, MFD_ALLOW_SEALING, MFD_HUGETLB = 1, 2, 4
MFD_HUGE_SHIFT, MFD_HUGE_MASK = 26, 63
MFD_HUGE_64KB, MFD_HUGE_512KB, MFD_HUGE_1MB, MFD_HUGE_2MB = 1073741824, 1275068416, 1342177280, 1409286144
MFD_HUGE_8MB, MFD_HUGE_16MB, MFD_HUGE_32MB, MFD_HUGE_256MB = 1543503872, 1610612736, 1677721600, 1879048192
MFD_HUGE_512MB, MFD_HUGE_1GB, MFD_HUGE_2GB, MFD_HUGE_16GB = 1946157056, 2013265920, 2080374784, 2281701376
NGROUPS_MAX = 65536
PIDFD_NONBLOCK = 2048
POSIX_FADV_NORMAL, POSIX_FADV_RANDOM, POSIX_FADV_SEQUENTIAL = 0, 1, 2
POSIX_FADV_WILLNEED, POSIX_FADV_DONTNEED, POSIX_FADV_NOREUSE = 3, 4, 5
POSIX_SPAWN_OPEN, POSIX_SPAWN_CLOSE, POSIX_SPAWN_DUP2, POSIX_SPAWN_CLOSEFROM = 0, 1, 2, 3
PRIO_PROCESS, PRIO_PGRP, PRIO_USER = 0, 1, 2
P_ALL, P_PID, P_PGID, P_PIDFD = 0, 1, 2, 3
P_WAIT, P_NOWAIT, P_NOWAITO = 0, 1, 1
RTLD_LOCAL, RTLD_LAZY, RTLD_NOW, RTLD_NOLOAD = 0, 1, 2, 4
RTLD_DEEPBIND, RTLD_GLOBAL, RTLD_NODELETE = 8, 256, 4096
RWF_HIPRI, RWF_DSYNC, RWF_SYNC, RWF_NOWAIT, RWF_APPEND = 1, 2, 4, 8, 16
SCHED_OTHER, SCHED_FIFO, SCHED_RR, SCHED_BATCH, SCHED_IDLE = 0, 1, 2, 3, 5
SCHED_RESET_ON_FORK = 1073741824
SPLICE_F_MOVE, SPLICE_F_NONBLOCK, SPLICE_F_MORE = 1, 2, 4
ST_RDONLY, ST_NOSUID, ST_NODEV, ST_NOEXEC, ST_SYNCHRONOUS = 1, 2, 4, 8, 16
ST_MANDLOCK, ST_WRITE, ST_APPEND, ST_NOATIME, ST_NODIRATIME, ST_RELATIME = 64, 128, 256, 1024, 2048, 4096
TFD_TIMER_ABSTIME, TFD_TIMER_CANCEL_ON_SET, TFD_NONBLOCK, TFD_CLOEXEC = 1, 2, 2048, 524288
TMP_MAX = 238328
WNOHANG, WUNTRACED, WSTOPPED, WEXITED, WCONTINUED, WNOWAIT = 1, 2, 2, 4, 8, 16777216
XATTR_CREATE, XATTR_REPLACE, XATTR_SIZE_MAX = 1, 2, 65536


def WCOREDUMP(status, /):
    """Return True if the process returning status was dumped to a core file."""
    return bool(status & 0x80)


def WIFCONTINUED(status, /):
    """Return True if a particular process was continued from a job control stop.

Return True if the process returning status was continued from a
job control stop."""
    return status == 0xffff


def WIFSTOPPED(status, /):
    """Return True if the process returning status was stopped."""
    return (status & 0xff) == 0x7f


def WIFSIGNALED(status, /):
    """Return True if the process returning status was terminated by a signal."""
    sig = (status & 0x7f) + 1
    if sig >= 0x80:
        sig -= 0x100
    return (sig >> 1) > 0


def WIFEXITED(status, /):
    """Return True if the process returning status exited via the exit() system call."""
    return (status & 0x7f) == 0


def WEXITSTATUS(status, /):
    """Return the process return code from status."""
    return (status >> 8) & 0xff


def WTERMSIG(status, /):
    """Return the signal that terminated the process that provided the status value."""
    return status & 0x7f


def WSTOPSIG(status, /):
    """Return the signal that stopped the process that provided the status value."""
    return (status >> 8) & 0xff


def waitstatus_to_exitcode(status):
    """Convert a wait status to an exit code.

On Unix:

* If WIFEXITED(status) is true, return WEXITSTATUS(status).
* If WIFSIGNALED(status) is true, return -WTERMSIG(status).
* Otherwise, raise a ValueError.

On Windows, return status shifted right by 8 bits.

On Unix, if the process is being traced or if waitpid() was called with
WUNTRACED option, the caller must first check if WIFSTOPPED(status) is true.
This function must not be called if WIFSTOPPED(status) is true."""
    if not isinstance(status, int):
        if not hasattr(type(status), '__index__'):
            raise TypeError(f"'{type(status).__name__}' object cannot be interpreted as an integer")
        status = type(status).__index__(status)
    if WIFEXITED(status):
        return WEXITSTATUS(status)
    if WIFSIGNALED(status):
        return -WTERMSIG(status)
    if WIFSTOPPED(status):
        raise ValueError(f'process stopped by delivery of signal {WSTOPSIG(status)}')
    raise ValueError(f'invalid wait status: {status}')

error = OSError


class PathLike(abc.ABC):
    """Abstract base class for implementing the file system path protocol."""

    @abc.abstractmethod
    def __fspath__(self):
        """Return the file system path representation of the object."""
        raise NotImplementedError

    @classmethod
    def __subclasshook__(cls, subclass):
        if cls is PathLike:
            return _check_methods(subclass, '__fspath__')
        return NotImplemented

    __class_getitem__ = classmethod(GenericAlias)


def _get_exports_list(module):
    try:
        return list(module.__all__)
    except AttributeError:
        return [n for n in dir(module) if n[0] != '_']


def _fspath(path):
    """Return the path representation of a path-like object.

    If str or bytes is passed in, it is returned unchanged. Otherwise the
    os.PathLike interface is used to get the path representation. If the
    path representation is not str or bytes, TypeError is raised. If the
    provided path is not str, bytes, or os.PathLike, TypeError is raised.
    """
    if isinstance(path, (str, bytes)):
        return path

    # Work from the object's type to match method resolution of other magic
    # methods.
    path_type = type(path)
    try:
        path_repr = path_type.__fspath__(path)
    except AttributeError:
        if hasattr(path_type, '__fspath__'):
            raise
        else:
            raise TypeError("expected str, bytes or os.PathLike object, "
                            "not " + path_type.__name__)
    except TypeError:
        if path_type.__fspath__ is None:
            raise TypeError("expected str, bytes or os.PathLike object, "
                            "not " + path_type.__name__) from None
        else:
            raise
    if isinstance(path_repr, (str, bytes)):
        return path_repr
    else:
        raise TypeError("expected {}.__fspath__() to return str or bytes, "
                        "not {}".format(path_type.__name__,
                                        type(path_repr).__name__))


def fspath(p):
    if isinstance(p, (str, bytes)):
        return p
    meth = getattr(p, '__fspath__', None)
    if meth is None:
        raise TypeError('expected str, bytes or os.PathLike object, not ' + type(p).__name__)
    return meth()


def _fscodec():
    encoding = sys.getfilesystemencoding()
    errors = sys.getfilesystemencodeerrors()

    def fsencode(filename):
        filename = fspath(filename)
        if isinstance(filename, str):
            return filename.encode(encoding, errors)
        else:
            return filename

    def fsdecode(filename):
        filename = fspath(filename)
        if isinstance(filename, bytes):
            return filename.decode(encoding, errors)
        else:
            return filename

    return fsencode, fsdecode

fsencode, fsdecode = _fscodec()
del _fscodec


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
    if isinstance(p, int):
        return _os.fchdir(p)
    _os.chdir(fspath(p))


def listdir(path=None):
    if path is None:
        path = '.'
    return _os.listdir(_path_or_fd(path))


class stat_result:
    """Resultado de `os.stat` (acessível por atributo e por índice)."""

    def __init__(self, t):
        self._t = tuple(t[:10])
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
        if len(t) > 10:
            # O `_os.stat` do kernel traz os nanossegundos exatos, o `st_blksize`, o `st_blocks` e o `st_rdev`;
            # os campos 7 a 9 da tupla são os segundos inteiros, como no CPython.
            self.st_atime_ns, self.st_mtime_ns, self.st_ctime_ns = t[10], t[11], t[12]
            self.st_blksize, self.st_blocks, self.st_rdev = t[13], t[14], t[15]
            self._t = tuple(t[:7]) + (t[10] // 1000000000, t[11] // 1000000000, t[12] // 1000000000)
        else:
            self.st_atime_ns = int(t[7] * 1000000000)
            self.st_mtime_ns = int(t[8] * 1000000000)
            self.st_ctime_ns = int(t[9] * 1000000000)
            self.st_blksize = 4096
            self.st_blocks = (t[6] + 511) // 512
            self.st_rdev = 0

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


def _path_or_fd(p):
    return p if isinstance(p, int) else fspath(p)


def _isdir_at(p, dir_fd):
    try:
        return (stat(p, dir_fd=dir_fd).st_mode & 0o170000) == 0o040000
    except (OSError, ValueError):
        return False


def _isreg_at(p, dir_fd):
    try:
        return (stat(p, dir_fd=dir_fd).st_mode & 0o170000) == 0o100000
    except (OSError, ValueError):
        return False


def stat(path, *, dir_fd=None, follow_symlinks=True):
    return stat_result(_os.stat(_path_or_fd(path), follow_symlinks, dir_fd=dir_fd))


def lstat(path, *, dir_fd=None):
    return stat_result(_os.stat(fspath(path), False, dir_fd=dir_fd))


def fstat(fd):
    return stat_result(_os.fstat(fd))


class DirEntry:
    __module__ = 'posix'

    __class_getitem__ = classmethod(GenericAlias)

    def __init__(self, dirpath, name, kind, dir_fd=None):
        self.name = name
        self.path = name if dir_fd is not None else path.join(dirpath, name)
        self._kind = kind
        self._dir_fd = dir_fd

    def is_dir(self, *, follow_symlinks=True):
        if self._kind == 'l' and follow_symlinks:
            return _isdir_at(self.path, self._dir_fd)
        return self._kind == 'd'

    def is_file(self, *, follow_symlinks=True):
        if self._kind == 'l' and follow_symlinks:
            return _isreg_at(self.path, self._dir_fd)
        return self._kind == 'f'

    def is_symlink(self):
        return self._kind == 'l'

    def stat(self, *, follow_symlinks=True):
        return stat(self.path, dir_fd=self._dir_fd, follow_symlinks=follow_symlinks)

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


def scandir(path=None):
    if path is None:
        path = '.'
    if isinstance(path, int):
        return _ScandirIterator([DirEntry(None, n, k, path) for n, k in _os.scandir(path)])
    p = fspath(path)
    return _ScandirIterator([DirEntry(p, n, k) for n, k in _os.scandir(p)])


def mkdir(path, mode=0o777, *, dir_fd=None):
    _os.mkdir(fspath(path), mode, dir_fd=dir_fd)


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


def remove(path, *, dir_fd=None):
    _os.unlink(fspath(path), dir_fd=dir_fd)


def unlink(path, *, dir_fd=None):
    _os.unlink(fspath(path), dir_fd=dir_fd)


def rmdir(path, *, dir_fd=None):
    _os.rmdir(fspath(path), dir_fd=dir_fd)


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


def rename(src, dst, *, src_dir_fd=None, dst_dir_fd=None):
    _os.rename(fspath(src), fspath(dst), src_dir_fd=src_dir_fd, dst_dir_fd=dst_dir_fd)


def replace(src, dst, *, src_dir_fd=None, dst_dir_fd=None):
    _os.rename(fspath(src), fspath(dst), src_dir_fd=src_dir_fd, dst_dir_fd=dst_dir_fd)


def renames(old, new):
    """renames(old, new)

    Super-rename; create directories as necessary and delete any left
    empty.  Works like rename, except creation of any intermediate
    directories needed to make the new pathname good is attempted
    first.  After the rename, directories corresponding to rightmost
    path segments of the old name will be pruned until either the
    whole path is consumed or a nonempty directory is found.

    Note: this function can fail with the new directory structure made
    if you lack permissions needed to unlink the leaf directory or
    file.

    """
    head, tail = path.split(new)
    if head and tail and not path.exists(head):
        makedirs(head)
    rename(old, new)
    head, tail = path.split(old)
    if head and tail:
        try:
            removedirs(head)
        except OSError:
            pass


def _exec_argv(fname, argv):
    if not isinstance(argv, (tuple, list)):
        raise TypeError(f'{fname}() arg 2 must be a tuple or list')
    out = []
    for a in argv:
        if not isinstance(a, (str, bytes)) and not hasattr(type(a), '__fspath__'):
            raise TypeError(f'expected str, bytes or os.PathLike object, not {type(a).__name__}')
        out.append(fsencode(a))
    return out


def execv(path, argv, /):
    """Execute an executable path with arguments, replacing current process.

  path
    Path of executable file.
  argv
    Tuple or list of strings."""
    args = _exec_argv('execv', argv)
    if not args:
        raise ValueError('execv() arg 2 must not be empty')
    if not args[0]:
        raise ValueError('execv() arg 2 first element cannot be empty')
    _os.execve(_path_or_fd(path), args, None)


def execve(path, argv, env):
    """Execute an executable path with arguments, replacing current process.

  path
    Path of executable file.
  argv
    Tuple or list of strings.
  env
    Dictionary of strings mapping to strings."""
    args = _exec_argv('execve', argv)
    if not args:
        raise ValueError('execve: argv must not be empty')
    if not args[0]:
        raise ValueError('execve: argv first element cannot be empty')
    if not isinstance(env, Mapping) and not hasattr(type(env), 'keys'):
        raise TypeError('execve: environment must be a mapping object')
    envlist = []
    for k, v in env.items():
        k, v = fsencode(k), fsencode(v)
        if not k or b'=' in k:
            raise ValueError('illegal environment variable name')
        if b'\0' in k or b'\0' in v:
            raise ValueError('embedded null byte')
        envlist.append(k + b'=' + v)
    _os.execve(_path_or_fd(path), args, envlist)


def execl(file, *args):
    """execl(file, *args)

    Execute the executable file with argument list args, replacing the
    current process. """
    execv(file, args)

def execle(file, *args):
    """execle(file, *args, env)

    Execute the executable file with argument list args and
    environment env, replacing the current process. """
    env = args[-1]
    execve(file, args[:-1], env)

def execlp(file, *args):
    """execlp(file, *args)

    Execute the executable file (which is searched for along $PATH)
    with argument list args, replacing the current process. """
    execvp(file, args)

def execlpe(file, *args):
    """execlpe(file, *args, env)

    Execute the executable file (which is searched for along $PATH)
    with argument list args and environment env, replacing the current
    process. """
    env = args[-1]
    execvpe(file, args[:-1], env)

def execvp(file, args):
    """execvp(file, args)

    Execute the executable file (which is searched for along $PATH)
    with argument list args, replacing the current process.
    args may be a list or tuple of strings. """
    _execvpe(file, args)

def execvpe(file, args, env):
    """execvpe(file, args, env)

    Execute the executable file (which is searched for along $PATH)
    with argument list args and environment env, replacing the
    current process.
    args may be a list or tuple of strings. """
    _execvpe(file, args, env)

def _execvpe(file, args, env=None):
    if env is not None:
        exec_func = execve
        argrest = (args, env)
    else:
        exec_func = execv
        argrest = (args,)
        env = environ

    if path.dirname(file):
        exec_func(file, *argrest)
        return
    saved_exc = None
    path_list = get_exec_path(env)
    if name != 'nt':
        file = fsencode(file)
        path_list = map(fsencode, path_list)
    for dir in path_list:
        fullname = path.join(dir, file)
        try:
            exec_func(fullname, *argrest)
        except (FileNotFoundError, NotADirectoryError) as e:
            last_exc = e
        except OSError as e:
            last_exc = e
            if saved_exc is None:
                saved_exc = e
    if saved_exc is not None:
        raise saved_exc
    raise last_exc


def get_exec_path(env=None):
    """Returns the sequence of directories that will be searched for the
    named executable (similar to a shell) when launching a process.

    *env* must be an environment variable dict or None.  If *env* is None,
    os.environ will be used.
    """
    import warnings

    if env is None:
        env = environ

    with warnings.catch_warnings():
        warnings.simplefilter("ignore", BytesWarning)

        try:
            path_list = env.get('PATH')
        except TypeError:
            path_list = None

        if supports_bytes_environ:
            try:
                path_listb = env[b'PATH']
            except (KeyError, TypeError):
                pass
            else:
                if path_list is not None:
                    raise ValueError(
                        "env cannot contain 'PATH' and b'PATH' keys")
                path_list = path_listb

            if path_list is not None and isinstance(path_list, bytes):
                path_list = fsdecode(path_list)

    if path_list is None:
        path_list = defpath
    return path_list.split(pathsep)


def fdopen(fd, mode="r", buffering=-1, encoding=None, *args, **kwargs):
    if not isinstance(fd, int):
        raise TypeError("invalid fd type (%s, expected integer)" % type(fd))
    import io
    if "b" not in mode:
        encoding = io.text_encoding(encoding)
    return io.open(fd, mode, buffering, encoding, *args, **kwargs)


def link(src, dst, *, src_dir_fd=None, dst_dir_fd=None, follow_symlinks=True):
    _os.link(fspath(src), fspath(dst), src_dir_fd=src_dir_fd, dst_dir_fd=dst_dir_fd, follow_symlinks=follow_symlinks)


def mkfifo(path, mode=0o666, *, dir_fd=None):
    _os.mknod(fspath(path), 0o010000 | (mode & 0o7777), 0, dir_fd=dir_fd)


def mknod(path, mode=0o600, device=0, *, dir_fd=None):
    _os.mknod(fspath(path), mode, device, dir_fd=dir_fd)


def major(device, /):
    """Extracts a device major number from a raw device number."""
    return ((device >> 8) & 0xfff) | ((device >> 32) & ~0xfff)


def minor(device, /):
    """Extracts a device minor number from a raw device number."""
    return (device & 0xff) | ((device >> 12) & ~0xff)


def makedev(major, minor, /):
    """Composes a raw device number from the major and minor device numbers."""
    return ((major & 0xfff) << 8) | ((major & ~0xfff) << 32) | (minor & 0xff) | ((minor & ~0xff) << 12)


def readlink(path, *, dir_fd=None):
    return _os.readlink(fspath(path), dir_fd=dir_fd)


def symlink(src, dst, target_is_directory=False, *, dir_fd=None):
    _os.symlink(fspath(src), fspath(dst), dir_fd=dir_fd)


def chmod(path, mode, *, dir_fd=None, follow_symlinks=True):
    """Change the access permissions of a file.

  path may be specified as either a string, bytes or a path-like object, or an open file descriptor."""
    if isinstance(path, int):
        return fchmod(path, mode)
    path = fspath(path)
    if not follow_symlinks and _S_ISLNK(lstat(path, dir_fd=dir_fd).st_mode):
        # O `fchmodat(AT_SYMLINK_NOFOLLOW)` da glibc recusa o symlink com EOPNOTSUPP.
        raise NotImplementedError('chmod: follow_symlinks unavailable on this platform')
    _os.chmod(path, mode, dir_fd=dir_fd)


def _S_ISLNK(mode):
    return (mode & 0o170000) == 0o120000


def chown(path, uid, gid, *, dir_fd=None, follow_symlinks=True):
    """Change the owner and group id of path to the numeric uid and gid.

  path may be specified as either a string, bytes or a path-like object, or an open file descriptor."""
    if isinstance(path, int):
        if dir_fd is not None:
            raise ValueError("chown: can't specify both dir_fd and fd")
        if not follow_symlinks:
            raise ValueError('chown: cannot use fd and follow_symlinks together')
        return fchown(path, uid, gid)
    if follow_symlinks:
        _os.chown(fspath(path), uid, gid, dir_fd=dir_fd)
    else:
        _os.lchown(fspath(path), uid, gid, dir_fd=dir_fd)


def lchown(path, uid, gid):
    _os.lchown(fspath(path), uid, gid)


def access(path, mode, *, dir_fd=None, effective_ids=False, follow_symlinks=True):
    return _os.access(fspath(path), mode, dir_fd=dir_fd)


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
from collections.abc import _check_methods

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
    """Exit to the system with specified status, without normal exit processing."""
    # O `_exit(2)`: nada de `atexit`, de finalizadores nem de descarregar os fluxos. O que o stdout ainda
    # tinha no buffer se perde (num pipe, tudo o que não chegou a 8 KiB), como no CPython; num filho de
    # `os.fork` é o que mantém a cópia do buffer do pai de sair duas vezes.
    _os._exit(status)


def waitpid(pid, options, /):
    """Wait for completion of a given child process.

Returns a tuple of information regarding the child process:
    (pid, status)

The options argument is ignored on Windows."""
    pid = _as_index(pid)
    options = _as_index(options)
    found = _wait_child(pid, options)
    if found is None:
        return (0, 0)
    return found


def _wait_child(pid, options, native=_os.wait):
    """O `waitpid` bloqueante dividido em esperas curtas (`WNOHANG`), para as threads cooperativas rodarem.
    `native` é o `_os.wait` (pid, status), o `_os.wait4` (com o rusage) ou o `waitid` (`pid` é ignorado)."""
    import _net
    threading = None if options & WNOHANG else _net.cooperative()
    if threading is None:
        return native(pid, options)
    options |= WNOHANG
    box = []

    def reaped():
        # A condição tem efeito (colhe o filho) e o escalonador a avalia mais de uma vez: depois da colheita ela só
        # confirma, senão a segunda chamada daria `ECHILD`.
        if box:
            return True
        found = native(pid, options)
        if found is not None:
            box.append(found)
        return found is not None

    step = 0.0005
    while not reaped():
        threading._wait_for(reaped, step, 'waitpid()')
        if box:
            break
        step = min(step * 2, 0.05)
    return box[0]


def wait4(pid, options):
    """Wait for completion of a child process.

Returns a tuple of information about the child process:
  (pid, status, rusage)"""
    from resource import _rusage
    found = _wait_child(_as_index(pid), _as_index(options), _os.wait4)
    if found is None:
        return (0, 0, _rusage(0.0, 0.0, 0))
    return found[0], found[1], _rusage(*found[2:])


def wait3(options):
    """Wait for completion of a child process.

Returns a tuple of information about the child process:
  (pid, status, rusage)"""
    return wait4(-1, options)


class waitid_result(tuple):
    """waitid_result: Result from waitid.

This object may be accessed either as a tuple of
  (si_pid, si_uid, si_signo, si_status, si_code),
or via the attributes si_pid, si_uid, and so on.

See os.waitid for more information."""

    __module__ = 'posix'

    _fields = ('si_pid', 'si_uid', 'si_signo', 'si_status', 'si_code')
    n_fields = 5
    n_sequence_fields = 5
    n_unnamed_fields = 0

    def __new__(cls, sequence):
        return tuple.__new__(cls, tuple(sequence))

    si_pid = property(lambda self: self[0])
    si_uid = property(lambda self: self[1])
    si_signo = property(lambda self: self[2])
    si_status = property(lambda self: self[3])
    si_code = property(lambda self: self[4])

    def __repr__(self):
        return 'posix.waitid_result(' + ', '.join('%s=%r' % (n, v) for n, v in zip(self._fields, self)) + ')'


def waitid(idtype, id, options, /):
    """Returns the result of waiting for a process or processes.

  idtype
    Must be one of be P_PID, P_PGID or P_ALL.
  id
    The id to wait on.
  options
    Constructed from the ORing of one or more of WEXITED, WSTOPPED
    or WCONTINUED and additionally may be ORed with WNOHANG or WNOWAIT.

Returns either waitid_result or None if WNOHANG is specified and there are
no children in a waitable state."""
    idtype, id, options = _as_index(idtype), _as_index(id), _as_index(options)
    found = _wait_child(0, options, lambda _pid, flags: _os.waitid(idtype, id, flags))
    if found is None:
        return None
    return waitid_result(found)


def pidfd_open(pid, flags=0):
    """Return a file descriptor referring to the process *pid*.

The descriptor can be used to perform process management without races and
signals."""
    return _os.pidfd_open(_as_index(pid), _as_index(flags))


def wait():
    """Wait for completion of a child process.

Returns a tuple of information about the child process:
    (pid, status)"""
    return waitpid(-1, 0)


def _as_index(value):
    if not isinstance(value, int):
        index = getattr(type(value), '__index__', None)
        if index is None:
            raise TypeError(f"'{type(value).__name__}' object cannot be interpreted as an integer")
        value = index(value)
    return value


# As funções de `register_at_fork`: `before` roda ao contrário da ordem de registro, as outras duas na ordem.
_at_fork_before = []
_at_fork_after_in_parent = []
_at_fork_after_in_child = []
_NOT_GIVEN = object()


def register_at_fork(*args, before=_NOT_GIVEN, after_in_child=_NOT_GIVEN, after_in_parent=_NOT_GIVEN):
    """Register callables to be called when forking a new process.

  before
    A callable to be called in the parent before the fork() syscall.
  after_in_child
    A callable to be called in the child after fork().
  after_in_parent
    A callable to be called in the parent after fork().

'before' callbacks are called in reverse order.
'after_in_child' and 'after_in_parent' callbacks are called in order."""
    if args:
        raise TypeError('register_at_fork() takes no positional arguments')
    given = (('before', before), ('after_in_parent', after_in_parent), ('after_in_child', after_in_child))
    if all(func is _NOT_GIVEN for _, func in given):
        raise TypeError('At least one argument is required.')
    for name, func in given:
        if func is not _NOT_GIVEN and not callable(func):
            raise TypeError(f"'{name}' must be callable, not {type(func).__name__}")
    if before is not _NOT_GIVEN:
        _at_fork_before.append(before)
    if after_in_parent is not _NOT_GIVEN:
        _at_fork_after_in_parent.append(after_in_parent)
    if after_in_child is not _NOT_GIVEN:
        _at_fork_after_in_child.append(after_in_child)


def _run_at_fork(funcs):
    """Roda as funções de uma lista (uma cópia: elas podem registrar outras). Uma exceção não aborta o
    fork: vai ao `sys.unraisablehook`, como o `PyErr_WriteUnraisable(func)` do CPython."""
    for func in list(funcs):
        try:
            func()
        except BaseException as e:
            import sys
            import _unraisable
            sys.unraisablehook(_unraisable.UnraisableHookArgs((type(e), e, e.__traceback__, None, func)))


def _fork_with_hooks(name, create, public=True):
    """O `os_fork_impl` e o `os_forkpty_impl` do CPython: a auditoria, os `before` no pai, a criação do
    processo (`create` devolve o pid, ou `(pid, fd)`), e depois os `after_in_child` no filho ou, no pai, o
    aviso do 3.12+ de processo com threads e os `after_in_parent`. Uma criação que falha também passa
    pelo ramo do pai (os ganchos rodam) antes de o erro subir. Com `public` falso (o `fork_exec` do
    `_posixsubprocess`, que chama `PyOS_BeforeFork` e companhia direto) não há auditoria nem aviso."""
    import sys
    if public:
        sys.audit('os.' + name)
    _run_at_fork(reversed(_at_fork_before))
    failure = None
    try:
        result = create()
    except OSError as e:
        failure = e
        result = -1
    if (result[0] if isinstance(result, tuple) else result) == 0:
        _run_at_fork(_at_fork_after_in_child)
        return result
    _run_at_fork(_at_fork_after_in_parent)
    # O `warn_about_fork_with_threads` vem depois do `PyOS_AfterFork_Parent` (que roda os `after_in_parent`).
    threading = sys.modules.get('threading')
    if public and threading is not None and threading.active_count() > 1:
        # O aviso é trabalho pesado no pai (lê o fonte): o filho do Linux já escreveu a primeira linha dele.
        _os._fork_settle()
        import warnings
        try:
            warnings.warn(f'This process (pid={getpid()}) is multi-threaded, use of {name}() may lead to deadlocks in the child.',
                          DeprecationWarning, stacklevel=1)
        except Exception:
            pass
    if failure is not None:
        raise failure
    return result


def _reject_arguments(name, args, kwargs):
    """O `METH_NOARGS` do CPython: `posix.fork() takes no arguments (1 given)` e `... takes no keyword
    arguments`, com o nome qualificado que o `_PyObject_FunctionStr` monta."""
    if kwargs:
        raise TypeError(f'posix.{name}() takes no keyword arguments')
    if args:
        raise TypeError(f'posix.{name}() takes no arguments ({len(args)} given)')


def fork(*args, **kwargs):
    """Fork a child process.

Return 0 to child process and PID of child to parent process."""
    _reject_arguments('fork', args, kwargs)
    return _fork_with_hooks('fork', _os.fork)


def _create_forkpty():
    master, slave = _os.openpty(False)
    try:
        pid = _os.fork()
    except OSError:
        _os.close(master)
        _os.close(slave)
        raise
    if pid == 0:
        # No filho o mestre fecha e o escravo vira o terminal de controle e o stdio. O `master_fd` do
        # CPython fica em -1 (a glibc só o preenche no pai).
        _os.close(master)
        try:
            _os.login_tty(slave)
        except OSError:
            _exit(1)
        return (0, -1)
    _os.close(slave)
    return (pid, master)


def forkpty(*args, **kwargs):
    """Fork a new process with a new pseudo-terminal as controlling tty.

Returns a tuple of (pid, master_fd).
Like fork(), return pid of 0 to the child process,
and a pid of the child to the parent process.
To both, return fd of newly opened pseudo-terminal."""
    _reject_arguments('forkpty', args, kwargs)
    return _fork_with_hooks('forkpty', _create_forkpty)


def openpty():
    """Open a pseudo-terminal.

Return a tuple of (master_fd, slave_fd) containing open file descriptors
for both the master and slave ends."""
    return _os.openpty()


def login_tty(fd, /):
    """Prepare the tty of which fd is a file descriptor for a new login session.

Make the calling process a session leader; make the tty the
controlling tty, the stdin, the stdout, and the stderr of the
calling process; close fd."""
    _os.login_tty(_as_fd(fd))


def urandom(n):
    return _os.urandom(n)


def _utime_pair(value):
    """O `(segundos, nanossegundos)` de um tempo de `utime(times=...)`: `_PyTime_ObjectToTimespec` com
    arredondamento para baixo."""
    if isinstance(value, float):
        from math import floor
        if value != value:
            raise ValueError('Invalid value NaN (not a number)')
        whole = float(int(value))
        nanos = int(floor((value - whole) * 1e9))
        if nanos >= 1000000000:
            nanos -= 1000000000
            whole += 1.0
        elif nanos < 0:
            nanos += 1000000000
            whole -= 1.0
        return int(whole), nanos
    return _as_index(value), 0


def utime(path, times=None, *, ns=None, dir_fd=None, follow_symlinks=True):
    """Set the access and modified time of path.

path may always be specified as a string.
On some platforms, path may also be specified as an open file descriptor."""
    if times is not None and ns is not None:
        raise ValueError("utime: you may specify either 'times' or 'ns' but not both")
    if times is not None:
        if not isinstance(times, tuple) or len(times) != 2:
            raise TypeError("utime: 'times' must be either a tuple of two ints or None")
        (asec, ansec), (msec, mnsec) = _utime_pair(times[0]), _utime_pair(times[1])
    elif ns is not None:
        if not isinstance(ns, tuple) or len(ns) != 2:
            raise TypeError("utime: 'ns' must be a tuple of two ints")
        asec, ansec = divmod(_as_index(ns[0]), 1000000000)
        msec, mnsec = divmod(_as_index(ns[1]), 1000000000)
    else:
        asec = ansec = msec = mnsec = None
    if isinstance(path, int):
        if dir_fd is not None:
            raise ValueError("utime: can't specify dir_fd without matching path")
        if not follow_symlinks:
            raise ValueError('utime: cannot use fd and follow_symlinks together')
        return _os.utimens(path, asec, ansec, msec, mnsec, None, True)
    _os.utimens(fspath(path), asec, ansec, msec, mnsec, dir_fd, follow_symlinks)


def truncate(path, length):
    """Truncate a file, specified by path, to a specific length.

path may be specified as an open file descriptor."""
    length = _as_index(length)
    if isinstance(path, int):
        return ftruncate(path, length)
    if length < 0:
        raise OSError(22, 'Invalid argument')
    fd = _os.open(fspath(path), O_WRONLY, 0)
    try:
        _os.ftruncate(fd, length)
    finally:
        _os.close(fd)


def ftruncate(fd, length, /):
    """Truncate a file, specified by file descriptor, to a specific length."""
    fd = _as_fd(fd)
    length = _as_index(length)
    if length < 0:
        raise OSError(22, 'Invalid argument')
    _os.ftruncate(fd, length)


def cpu_count():
    """Return the number of logical CPUs in the system.

Return None if indeterminable."""
    import sys
    configured = sys._get_cpu_count_config()
    if configured > 0:
        return configured
    count = sysconf('SC_NPROCESSORS_ONLN')
    return count if count >= 1 else None


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
    name = _conf_code(name, confstr_names)
    if name not in confstr_names.values():
        raise OSError(22, 'Invalid argument')
    return _CONFSTR_VALUES.get(name, '')


confstr.__module__ = 'posix'


def get_terminal_size(fd=1, /):
    """Return the size of the terminal window as (columns, lines).

The optional argument fd (default standard output) specifies
which file descriptor should be queried.

If the file descriptor is not connected to a terminal, an OSError
is thrown.

This function will only be defined if an implementation is
available for this system.

shutil.get_terminal_size is the high-level function which should
normally be used, os.get_terminal_size is the low-level implementation."""
    return terminal_size(_os.winsize(_fd_arg(fd)))


class terminal_size(tuple):
    """A tuple of (columns, lines) for holding terminal window size"""

    __module__ = 'os'
    n_fields = 2
    n_sequence_fields = 2
    n_unnamed_fields = 0
    _fields = ('columns', 'lines')

    def __new__(cls, sequence):
        items = tuple(sequence)
        if len(items) != 2:
            raise TypeError('os.terminal_size() takes a 2-sequence (%d-sequence given)' % len(items))
        return tuple.__new__(cls, items)

    columns = property(lambda self: self[0], doc='width of the terminal window in characters')
    lines = property(lambda self: self[1], doc='height of the terminal window in characters')

    def __repr__(self):
        return 'os.terminal_size(columns=%r, lines=%r)' % tuple(self)


def isatty(fd):
    return _os.isatty(fd)


def open(path, flags, mode=0o777, *, dir_fd=None):
    return _os.open(fspath(path), flags, mode, dir_fd=dir_fd)


def close(fd):
    _os.close(fd)


def read(fd, n):
    # Com outras threads vivas, esperar bloqueado no kernel as prenderia (o `Pool` do `multiprocessing` lê o
    # resultado dos workers numa thread enquanto outra despacha as tarefas): a thread suspende até o fd ter
    # o que ler e só então lê. No CPython o `read` solta a GIL.
    if type(fd) is int and fd >= 0 and type(n) is int and n > 0:
        import _net
        return _net.read(fd, n)
    return _os.read(fd, n)


def pipe():
    """Create a pipe.

Returns a tuple of two file descriptors:
  (read_fd, write_fd)"""
    return tuple(_os.pipe())


def set_blocking(fd, blocking, /):
    """Set the blocking mode of the specified file descriptor.

Set the O_NONBLOCK flag if blocking is False,
clear the O_NONBLOCK flag otherwise."""
    _os.set_blocking(_fd_arg(fd), blocking)


def get_blocking(fd, /):
    """Get the blocking mode of the file descriptor.

Return False if the O_NONBLOCK flag is set, True if the flag is cleared."""
    return _os.get_blocking(_fd_arg(fd))


def _fd_arg(fd):
    """O conversor `int` dos descritores do `posixmodule.c`: inteiro (ou `__index__`) que cabe num `int` de C."""
    fd = _as_index(fd)
    if not -2147483648 <= fd <= 2147483647:
        raise OverflowError('Python int too large to convert to C int')
    return fd


def dup(fd, /):
    """Return a duplicate of a file descriptor."""
    return _os.dup(_fd_arg(fd))


def dup2(fd, fd2, inheritable=True):
    """Duplicate file descriptor."""
    return _os.dup2(_fd_arg(fd), _fd_arg(fd2), inheritable)


def get_inheritable(fd, /):
    """Get the close-on-exe flag of the specified file descriptor."""
    return _os.get_inheritable(_fd_arg(fd))


def set_inheritable(fd, inheritable, /):
    """Set the inheritable flag of the specified file descriptor."""
    _os.set_inheritable(_fd_arg(fd), _as_index(inheritable))

def write(fd, data):
    # Num pipe cheio cujo leitor é outra thread, esperar bloqueado no kernel prenderia o leitor (ver `read`).
    if type(fd) is int and fd >= 0:
        import _net
        return _net.write(fd, data)
    return _os.write(fd, data)


def lseek(fd, pos, how):
    return _os.lseek(fd, pos, how)


def getuid():
    """Return the current process's user id."""
    return _os._creds('uid')


def getgid():
    """Return the current process's group id."""
    return _os._creds('gid')


def geteuid():
    """Return the current process's effective user id."""
    return _os._creds('euid')


def getegid():
    """Return the current process's effective group id."""
    return _os._creds('egid')


def getgroups():
    """Return list of supplemental group IDs for the process."""
    return _os._creds('groups')


def getresuid():
    """Return a tuple of the current process's real, effective, and saved user ids."""
    return _os._creds('resuid')


def getresgid():
    """Return a tuple of the current process's real, effective, and saved group ids."""
    return _os._creds('resgid')


def setuid(uid, /):
    """Set the current process's user id."""
    _os._setids('setuid', uid)


def seteuid(euid, /):
    """Set the current process's effective user id."""
    _os._setids('seteuid', euid)


def setgid(gid, /):
    """Set the current process's group id."""
    _os._setids('setgid', gid)


def setegid(egid, /):
    """Set the current process's effective group id."""
    _os._setids('setegid', egid)


def setreuid(ruid, euid, /):
    """Set the current process's real and effective user ids."""
    _os._setids('setreuid', ruid, euid)


def setregid(rgid, egid, /):
    """Set the current process's real and effective group ids."""
    _os._setids('setregid', rgid, egid)


def setresuid(ruid, euid, suid, /):
    """Set the current process's real, effective, and saved user ids."""
    _os._setids('setresuid', ruid, euid, suid)


def setresgid(rgid, egid, sgid, /):
    """Set the current process's real, effective, and saved group ids."""
    _os._setids('setresgid', rgid, egid, sgid)


def setgroups(groups, /):
    """Set the groups of the current process to list."""
    _os._setids('setgroups', list(groups))


def getgrouplist(user, group, /):
    """Returns a list of groups to which a user belongs.

  user
    username to lookup
  group
    base group id of the user"""
    # O `getgrouplist` da glibc pelo NSS `files`: o grupo base primeiro, depois os grupos do
    # `/etc/group` que listam o usuário, na ordem do arquivo e sem repetir.
    if not isinstance(user, str):
        raise TypeError('getgrouplist() argument 1 must be str, not ' + type(user).__name__)
    result = [group]
    import builtins
    try:
        with builtins.open('/etc/group', encoding='utf-8', errors='surrogateescape') as f:
            for line in f:
                fields = line.rstrip('\n').split(':')
                if len(fields) < 4 or not fields[2].isdigit():
                    continue
                gid = int(fields[2])
                if user in fields[3].split(',') and gid not in result:
                    result.append(gid)
    except OSError:
        pass
    return result


def initgroups(username, gid, /):
    """Initialize the group access list.

Call the system initgroups() to initialize the group access list with all of
the groups of which the specified username is a member, plus the specified
group id."""
    setgroups(getgrouplist(username, gid))


def getlogin():
    return getenv('USER', 'root')


class uname_result(tuple):
    """uname_result: Result from os.uname().

This object may be accessed either as a tuple of
  (sysname, nodename, release, version, machine),
or via the attributes sysname, nodename, release, version, and machine.

See os.uname for more information."""

    __module__ = 'posix'

    _fields = ('sysname', 'nodename', 'release', 'version', 'machine')
    n_fields = 5
    n_sequence_fields = 5
    n_unnamed_fields = 0

    def __new__(cls, sequence):
        return tuple.__new__(cls, tuple(sequence))

    sysname = property(lambda self: self[0], doc='operating system name')
    nodename = property(lambda self: self[1], doc='name of machine on network (implementation-defined)')
    release = property(lambda self: self[2], doc='operating system release')
    version = property(lambda self: self[3], doc='operating system version')
    machine = property(lambda self: self[4], doc='hardware identifier')

    def __repr__(self):
        return 'posix.uname_result(sysname=%r, nodename=%r, release=%r, version=%r, machine=%r)' % tuple(self)


def uname():
    """Return an object identifying the current operating system.

The object behaves like a named tuple with the following fields:
  (sysname, nodename, release, version, machine)"""
    return uname_result(_os.uname())


class statvfs_result(tuple):
    """statvfs_result: Result from statvfs or fstatvfs.

This object may be accessed either as a tuple of
  (bsize, frsize, blocks, bfree, bavail, files, ffree, favail, flag, namemax),
or via the attributes f_bsize, f_frsize, f_blocks, f_bfree, and so on.

See os.statvfs for more information."""

    _fields = ('f_bsize', 'f_frsize', 'f_blocks', 'f_bfree', 'f_bavail', 'f_files',
               'f_ffree', 'f_favail', 'f_flag', 'f_namemax')
    n_fields = 11
    n_sequence_fields = 10
    n_unnamed_fields = 0

    def __new__(cls, sequence):
        seq = tuple(sequence)
        self = tuple.__new__(cls, seq[:10])
        self._fsid = seq[10] if len(seq) > 10 else None
        return self

    f_bsize = property(lambda self: self[0])
    f_frsize = property(lambda self: self[1])
    f_blocks = property(lambda self: self[2])
    f_bfree = property(lambda self: self[3])
    f_bavail = property(lambda self: self[4])
    f_files = property(lambda self: self[5])
    f_ffree = property(lambda self: self[6])
    f_favail = property(lambda self: self[7])
    f_flag = property(lambda self: self[8])
    f_namemax = property(lambda self: self[9])
    f_fsid = property(lambda self: self._fsid)

    def __repr__(self):
        return 'os.statvfs_result(' + ', '.join('%s=%r' % (n, v) for n, v in zip(self._fields, self)) + ')'


pathconf_names = {
    'PC_ALLOC_SIZE_MIN': 18, 'PC_ASYNC_IO': 10, 'PC_CHOWN_RESTRICTED': 6, 'PC_FILESIZEBITS': 13,
    'PC_LINK_MAX': 0, 'PC_MAX_CANON': 1, 'PC_MAX_INPUT': 2, 'PC_NAME_MAX': 3, 'PC_NO_TRUNC': 7,
    'PC_PATH_MAX': 4, 'PC_PIPE_BUF': 5, 'PC_PRIO_IO': 11, 'PC_REC_INCR_XFER_SIZE': 14,
    'PC_REC_MAX_XFER_SIZE': 15, 'PC_REC_MIN_XFER_SIZE': 16, 'PC_REC_XFER_ALIGN': 17,
    'PC_SOCK_MAXBUF': 12, 'PC_SYMLINK_MAX': 19, 'PC_SYNC_IO': 9, 'PC_VDISABLE': 8,
}


def _pathconf_value(target, name):
    code = _conf_code(name, pathconf_names)
    # O `pathconf` da glibc: o que depende do sistema de arquivos sai do `statfs` (os tmpfs, proc e
    # devpts do sandbox ficam fora da tabela de tipos dela, com os padrões do Linux); o resto é fixo.
    st = _os.statvfs(target)
    if code == 0:
        return 127
    if code == 3:
        return st[9]
    if code in (16, 17, 18):
        return st[0]
    fixed = {1: 255, 2: 255, 4: 4096, 5: 4096, 6: 1, 7: 1, 8: 0, 9: -1, 10: -1, 11: -1, 12: -1,
             13: 32, 14: -1, 15: -1, 19: -1}
    if code not in fixed:
        raise OSError(22, 'Invalid argument')
    return fixed[code]


def pathconf(path, name):
    """Return the configuration limit name for the file or directory path.

If there is no limit, return -1.
On some platforms, path may also be specified as an open file descriptor.
  If this functionality is unavailable, using it raises an exception."""
    return _pathconf_value(_path_or_fd(path), name)


def fpathconf(fd, name, /):
    """Return the configuration limit name for the file descriptor fd.

If there is no limit, return -1."""
    if not isinstance(fd, int) and hasattr(fd, 'fileno'):
        fd = fd.fileno()
    return _pathconf_value(_path_or_fd(fd), name)


def statvfs(path):
    """Perform a statvfs system call on the given path.

path may always be specified as a string.
On some platforms, path may also be specified as an open file descriptor.
  If this functionality is unavailable, using it raises an exception."""
    return statvfs_result(_os.statvfs(_path_or_fd(path)))


def fstatvfs(fd, /):
    """Perform an fstatvfs system call on the given fd.

Equivalent to statvfs(fd)."""
    if not isinstance(fd, int):
        raise TypeError(f"'{type(fd).__name__}' object cannot be interpreted as an integer")
    return statvfs_result(_os.statvfs(fd))


def kill(pid, signal, /):
    """Kill a process with a signal."""
    pid = _c_int(pid)
    signal = _as_index(signal)
    if pid > 0:
        _os.kill(pid, signal)
    elif pid == -1:
        _os.kill_many(1, 0, signal)
    else:
        _os.kill_many(0, -pid, signal)


# ---- posix_spawn ----

def _spawn_path(function, path):
    """O `path_converter` do `posix_spawn`: `str`, `bytes` ou `PathLike`, sem byte nulo."""
    if not isinstance(path, (str, bytes)):
        if not hasattr(type(path), '__fspath__'):
            raise TypeError('%s: path should be string, bytes or os.PathLike, not %s' % (function, type(path).__name__))
        path = fspath(path)
    data = path.encode('utf-8', 'surrogateescape') if isinstance(path, str) else path
    if b'\0' in data:
        raise ValueError('%s: embedded null character in path' % function)
    return data


def _spawn_signals(signals):
    """O `_Py_Sigset_Converter` de `setsigmask` e `setsigdef`: cada item é um sinal de 1 a 64."""
    out = []
    for signum in signals:
        signum = _as_index(signum)
        if not 1 <= signum <= 64:
            raise ValueError('signal number %d out of range [1; 64]' % signum)
        out.append(signum)
    return out


def _spawn_actions(file_actions):
    """O `parse_file_actions`: as tuplas `(POSIX_SPAWN_*, ...)` viram as que o `_os.posix_spawn` lê. Um fd negativo
    é `EBADF` já aqui, como no `posix_spawn_file_actions_add*` (sem o nome do programa na mensagem)."""
    try:
        actions = list(file_actions)
    except TypeError:
        raise TypeError('file_actions must be a sequence or None') from None
    out = []
    for action in actions:
        if not isinstance(action, tuple) or len(action) < 2:
            raise TypeError('Each file_actions element must be a non-empty tuple')
        tag = _as_index(action[0])
        if tag == POSIX_SPAWN_OPEN:
            if len(action) != 5:
                raise TypeError('A open file_action tuple must have 5 elements')
            fds = [_c_int(action[1])]
            entry = (POSIX_SPAWN_OPEN, fds[0], _env_arg(action[2]), _c_int(action[3]), _as_index(action[4]) & 0xFFFFFFFF)
        elif tag in (POSIX_SPAWN_CLOSE, POSIX_SPAWN_CLOSEFROM):
            if len(action) != 2:
                name = 'close' if tag == POSIX_SPAWN_CLOSE else 'closefrom'
                raise TypeError('A %s file_action tuple must have 2 elements' % name)
            fds = [_c_int(action[1])]
            entry = (tag, fds[0])
        elif tag == POSIX_SPAWN_DUP2:
            if len(action) != 3:
                raise TypeError('A dup2 file_action tuple must have 3 elements')
            fds = [_c_int(action[1]), _c_int(action[2])]
            entry = (POSIX_SPAWN_DUP2, fds[0], fds[1])
        else:
            raise TypeError('Unknown file_actions identifier')
        if any(fd < 0 for fd in fds):
            raise OSError(9, strerror(9))
        out.append(entry)
    return out


def _spawn_exec_failed(errno_number, path):
    """O erro de um `posix_spawn` que falhou no filho: o nome do programa vai na exceção."""
    return OSError(errno_number, strerror(errno_number), path)


def _spawn_in_path(file, attempt, fail):
    """O `__execvpex` do glibc: sem `/` no nome, tenta `$PATH` (o do próprio processo, `/bin:/usr/bin` se não
    há), segue nas falhas de arquivo ausente ou ilegível e devolve `EACCES` se alguma entrada negou o acesso.
    `fail(errno)` faz a exceção da falha final, com o nome do programa."""
    if not file:
        raise fail(2)
    if b'/' in file:
        return attempt(file)
    if len(file) > 255:
        raise fail(36)
    search = getenvb(b'PATH')
    if search is None:
        search = b'/bin:/usr/bin'
    got_eacces = False
    last = 2
    for directory in search.split(b':'):
        try:
            return attempt(directory + b'/' + file if directory else file)
        except OSError as e:
            last = e.errno
            if e.errno == 13:
                got_eacces = True
            elif e.errno not in (2, 116, 20, 19, 110):
                raise
    raise fail(13 if got_eacces else last)


def _posix_spawn(function, path, argv, env, file_actions, setpgroup, resetids, setsid, setsigmask, setsigdef,
                 scheduler, use_path):
    """O `py_posix_spawn` do CPython: valida os argumentos na ordem dele, converte e chama o `posix_spawn(3)`
    (o `_os.posix_spawn`). A falha de um ajuste ou do `exec` volta como `OSError` com o nome do programa."""
    import sys
    path_bytes = _spawn_path(function, path)
    sys.audit('os.posix_spawn', path, argv, env)
    if not isinstance(argv, (tuple, list)):
        raise TypeError('%s: argv must be a tuple or list' % function)
    if len(argv) < 1:
        raise ValueError('%s: argv must not be empty' % function)
    if env is not None and not (isinstance(env, Mapping) or hasattr(type(env), 'keys')):
        raise TypeError('%s: environment must be a mapping object or None' % function)
    args = [_env_arg(item) for item in argv]
    if not args[0]:
        raise ValueError('%s: argv first element cannot be empty' % function)
    envlist = None
    if env is not None:
        envlist = []
        for key, value in env.items():
            key, value = _env_arg(key), _env_arg(value)
            if not key or b'=' in key:
                raise ValueError('illegal environment variable name')
            envlist.append(key + b'=' + value)
    actions = _spawn_actions(() if file_actions is None else file_actions)
    group = -1
    if setpgroup is not None:
        group = _c_int(setpgroup)
    _spawn_signals(setsigmask)
    signals_default = _spawn_signals(setsigdef)
    schedule = None
    if scheduler is not None:
        if not isinstance(scheduler, tuple) or len(scheduler) != 2:
            raise TypeError('A scheduler tuple must have two elements')
        policy, param = scheduler
        if param is not None and type(param) is not sched_param:
            raise TypeError('must have a sched_param object')
        if policy is not None:
            policy = _c_int(policy)
        schedule = (policy, 0 if param is None else _c_int(param.sched_priority))
    # Os ajustes que o filho faz antes do `exec` e que falham: `setpgid(0, pgrp)` com `pgrp` negativo é
    # EINVAL, e quem acabou de virar líder de sessão não entra em outro grupo (EPERM).
    if setpgroup is not None and group < 0:
        raise _spawn_exec_failed(22, path)
    if setsid and group > 0:
        raise _spawn_exec_failed(1, path)

    def attempt(candidate):
        try:
            return _os.posix_spawn(candidate, args, envlist, actions, group, bool(resetids), bool(setsid),
                                   signals_default, schedule)
        except OSError as e:
            raise _spawn_exec_failed(e.errno, path) from None

    return _spawn_in_path(path_bytes, attempt, lambda code: _spawn_exec_failed(code, path)) if use_path else attempt(path_bytes)


def posix_spawn(path, argv, env, /, *, file_actions=(), setpgroup=None, resetids=False, setsid=False,
                setsigmask=(), setsigdef=(), scheduler=None):
    """Execute the program specified by path in a new process.

  path
    Path of executable file.
  argv
    Tuple or list of strings.
  env
    Dictionary of strings mapping to strings.
  file_actions
    A sequence of file action tuples.
  setpgroup
    The pgroup to use with the POSIX_SPAWN_SETPGROUP flag.
  resetids
    If the value is `true` the POSIX_SPAWN_RESETIDS will be activated.
  setsid
    If the value is `true` the POSIX_SPAWN_SETSID or POSIX_SPAWN_SETSID_NP will be activated.
  setsigmask
    The sigmask to use with the POSIX_SPAWN_SETSIGMASK flag.
  setsigdef
    The sigmask to use with the POSIX_SPAWN_SETSIGDEF flag.
  scheduler
    A tuple with the scheduler policy (optional) and parameters."""
    return _posix_spawn('posix_spawn', path, argv, env, file_actions, setpgroup, resetids, setsid, setsigmask,
                        setsigdef, scheduler, False)


def posix_spawnp(path, argv, env, /, *, file_actions=(), setpgroup=None, resetids=False, setsid=False,
                 setsigmask=(), setsigdef=(), scheduler=None):
    """Execute the program specified by path in a new process.

  path
    Path of executable file.
  argv
    Tuple or list of strings.
  env
    Dictionary of strings mapping to strings.
  file_actions
    A sequence of file action tuples.
  setpgroup
    The pgroup to use with the POSIX_SPAWN_SETPGROUP flag.
  resetids
    If the value is `True` the POSIX_SPAWN_RESETIDS will be activated.
  setsid
    If the value is `True` the POSIX_SPAWN_SETSID or POSIX_SPAWN_SETSID_NP will be activated.
  setsigmask
    The sigmask to use with the POSIX_SPAWN_SETSIGMASK flag.
  setsigdef
    The sigmask to use with the POSIX_SPAWN_SETSIGDEF flag.
  scheduler
    A tuple with the scheduler policy (optional) and parameters."""
    return _posix_spawn('posix_spawnp', path, argv, env, file_actions, setpgroup, resetids, setsid, setsigmask,
                        setsigdef, scheduler, True)


posix_spawn.__module__ = 'posix'
posix_spawnp.__module__ = 'posix'


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


# ---- sysconf, tempos e carga do sistema ----
# As tabelas e os valores são os da glibc 2.41 do Debian 13 em x86-64 (os que dependem da máquina ou do
# processo saem do `/proc` e dos `rlimit` do pseudo-processo, como a glibc os lê).

sysconf_names = {
    'SC_2_CHAR_TERM': 95, 'SC_2_C_BIND': 47, 'SC_2_C_DEV': 48, 'SC_2_C_VERSION': 96, 'SC_2_FORT_DEV': 49,
    'SC_2_FORT_RUN': 50, 'SC_2_LOCALEDEF': 52, 'SC_2_SW_DEV': 51, 'SC_2_UPE': 97, 'SC_2_VERSION': 46,
    'SC_AIO_LISTIO_MAX': 23, 'SC_AIO_MAX': 24, 'SC_AIO_PRIO_DELTA_MAX': 25, 'SC_ARG_MAX': 0,
    'SC_ASYNCHRONOUS_IO': 12, 'SC_ATEXIT_MAX': 87, 'SC_AVPHYS_PAGES': 86, 'SC_BC_BASE_MAX': 36,
    'SC_BC_DIM_MAX': 37, 'SC_BC_SCALE_MAX': 38, 'SC_BC_STRING_MAX': 39, 'SC_CHARCLASS_NAME_MAX': 45,
    'SC_CHAR_BIT': 101, 'SC_CHAR_MAX': 102, 'SC_CHAR_MIN': 103, 'SC_CHILD_MAX': 1, 'SC_CLK_TCK': 2,
    'SC_COLL_WEIGHTS_MAX': 40, 'SC_DELAYTIMER_MAX': 26, 'SC_EQUIV_CLASS_MAX': 41, 'SC_EXPR_NEST_MAX': 42,
    'SC_FSYNC': 15, 'SC_GETGR_R_SIZE_MAX': 69, 'SC_GETPW_R_SIZE_MAX': 70, 'SC_INT_MAX': 104, 'SC_INT_MIN': 105,
    'SC_IOV_MAX': 60, 'SC_JOB_CONTROL': 7, 'SC_LINE_MAX': 43, 'SC_LOGIN_NAME_MAX': 71, 'SC_LONG_BIT': 106,
    'SC_MAPPED_FILES': 16, 'SC_MB_LEN_MAX': 108, 'SC_MEMLOCK': 17, 'SC_MEMLOCK_RANGE': 18,
    'SC_MEMORY_PROTECTION': 19, 'SC_MESSAGE_PASSING': 20, 'SC_MQ_OPEN_MAX': 27, 'SC_MQ_PRIO_MAX': 28,
    'SC_NGROUPS_MAX': 3, 'SC_NL_ARGMAX': 119, 'SC_NL_LANGMAX': 120, 'SC_NL_MSGMAX': 121, 'SC_NL_NMAX': 122,
    'SC_NL_SETMAX': 123, 'SC_NL_TEXTMAX': 124, 'SC_NPROCESSORS_CONF': 83, 'SC_NPROCESSORS_ONLN': 84,
    'SC_NZERO': 109, 'SC_OPEN_MAX': 4, 'SC_PAGESIZE': 30, 'SC_PAGE_SIZE': 30, 'SC_PASS_MAX': 88,
    'SC_PHYS_PAGES': 85, 'SC_PII': 53, 'SC_PII_INTERNET': 56, 'SC_PII_INTERNET_DGRAM': 62,
    'SC_PII_INTERNET_STREAM': 61, 'SC_PII_OSI': 57, 'SC_PII_OSI_CLTS': 64, 'SC_PII_OSI_COTS': 63,
    'SC_PII_OSI_M': 65, 'SC_PII_SOCKET': 55, 'SC_PII_XTI': 54, 'SC_POLL': 58, 'SC_PRIORITIZED_IO': 13,
    'SC_PRIORITY_SCHEDULING': 10, 'SC_REALTIME_SIGNALS': 9, 'SC_RE_DUP_MAX': 44, 'SC_RTSIG_MAX': 31,
    'SC_SAVED_IDS': 8, 'SC_SCHAR_MAX': 111, 'SC_SCHAR_MIN': 112, 'SC_SELECT': 59, 'SC_SEMAPHORES': 21,
    'SC_SEM_NSEMS_MAX': 32, 'SC_SEM_VALUE_MAX': 33, 'SC_SHARED_MEMORY_OBJECTS': 22, 'SC_SHRT_MAX': 113,
    'SC_SHRT_MIN': 114, 'SC_SIGQUEUE_MAX': 34, 'SC_SSIZE_MAX': 110, 'SC_STREAM_MAX': 5,
    'SC_SYNCHRONIZED_IO': 14, 'SC_THREADS': 67, 'SC_THREAD_ATTR_STACKADDR': 77, 'SC_THREAD_ATTR_STACKSIZE': 78,
    'SC_THREAD_DESTRUCTOR_ITERATIONS': 73, 'SC_THREAD_KEYS_MAX': 74, 'SC_THREAD_PRIORITY_SCHEDULING': 79,
    'SC_THREAD_PRIO_INHERIT': 80, 'SC_THREAD_PRIO_PROTECT': 81, 'SC_THREAD_PROCESS_SHARED': 82,
    'SC_THREAD_SAFE_FUNCTIONS': 68, 'SC_THREAD_STACK_MIN': 75, 'SC_THREAD_THREADS_MAX': 76, 'SC_TIMERS': 11,
    'SC_TIMER_MAX': 35, 'SC_TTY_NAME_MAX': 72, 'SC_TZNAME_MAX': 6, 'SC_T_IOV_MAX': 66, 'SC_UCHAR_MAX': 115,
    'SC_UINT_MAX': 116, 'SC_UIO_MAXIOV': 60, 'SC_ULONG_MAX': 117, 'SC_USHRT_MAX': 118, 'SC_VERSION': 29,
    'SC_WORD_BIT': 107, 'SC_XBS5_ILP32_OFF32': 125, 'SC_XBS5_ILP32_OFFBIG': 126, 'SC_XBS5_LP64_OFF64': 127,
    'SC_XBS5_LPBIG_OFFBIG': 128, 'SC_XOPEN_CRYPT': 92, 'SC_XOPEN_ENH_I18N': 93, 'SC_XOPEN_LEGACY': 129,
    'SC_XOPEN_REALTIME': 130, 'SC_XOPEN_REALTIME_THREADS': 131, 'SC_XOPEN_SHM': 94, 'SC_XOPEN_UNIX': 91,
    'SC_XOPEN_VERSION': 89, 'SC_XOPEN_XCU_VERSION': 90, 'SC_XOPEN_XPG2': 98, 'SC_XOPEN_XPG3': 99,
    'SC_XOPEN_XPG4': 100, 'SC_MINSIGSTKSZ': 249,
}

# O que a glibc 2.41 do Debian 13 responde sem consultar o sistema (`-1` é "sem limite" ou "não suportado").
_SC_FIXED = {
    'SC_2_CHAR_TERM': 200809, 'SC_2_C_BIND': 200809, 'SC_2_C_DEV': 200809, 'SC_2_C_VERSION': 200809,
    'SC_2_FORT_DEV': -1, 'SC_2_FORT_RUN': -1, 'SC_2_LOCALEDEF': 200809, 'SC_2_SW_DEV': 200809, 'SC_2_UPE': -1,
    'SC_2_VERSION': 200809, 'SC_AIO_LISTIO_MAX': -1, 'SC_AIO_MAX': -1, 'SC_AIO_PRIO_DELTA_MAX': 20,
    'SC_ASYNCHRONOUS_IO': 200809, 'SC_ATEXIT_MAX': 2147483647, 'SC_BC_BASE_MAX': 99, 'SC_BC_DIM_MAX': 2048,
    'SC_BC_SCALE_MAX': 99, 'SC_BC_STRING_MAX': 1000, 'SC_CHARCLASS_NAME_MAX': 2048, 'SC_CHAR_BIT': 8,
    'SC_CHAR_MAX': 127, 'SC_CHAR_MIN': -128, 'SC_CLK_TCK': 100, 'SC_COLL_WEIGHTS_MAX': 255,
    'SC_DELAYTIMER_MAX': 2147483647, 'SC_EXPR_NEST_MAX': 32, 'SC_FSYNC': 200809, 'SC_GETGR_R_SIZE_MAX': 1024,
    'SC_GETPW_R_SIZE_MAX': 1024, 'SC_INT_MAX': 2147483647, 'SC_INT_MIN': -2147483648, 'SC_IOV_MAX': 1024,
    'SC_JOB_CONTROL': 1, 'SC_LINE_MAX': 2048, 'SC_LOGIN_NAME_MAX': 256, 'SC_LONG_BIT': 64,
    'SC_MAPPED_FILES': 200809, 'SC_MB_LEN_MAX': 16, 'SC_MEMLOCK': 200809, 'SC_MEMLOCK_RANGE': 200809,
    'SC_MEMORY_PROTECTION': 200809, 'SC_MESSAGE_PASSING': 200809, 'SC_MQ_OPEN_MAX': -1, 'SC_MQ_PRIO_MAX': 32768,
    'SC_NGROUPS_MAX': 65536, 'SC_NL_ARGMAX': 4096, 'SC_NL_LANGMAX': 2048, 'SC_NL_MSGMAX': 2147483647,
    'SC_NL_NMAX': 2147483647, 'SC_NL_SETMAX': 2147483647, 'SC_NL_TEXTMAX': 2147483647, 'SC_NZERO': 20,
    'SC_PAGESIZE': 4096, 'SC_PAGE_SIZE': 4096, 'SC_PASS_MAX': 8192, 'SC_PII': -1, 'SC_PII_INTERNET': -1,
    'SC_PII_INTERNET_DGRAM': -1, 'SC_PII_INTERNET_STREAM': -1, 'SC_PII_OSI': -1, 'SC_PII_OSI_CLTS': -1,
    'SC_PII_OSI_COTS': -1, 'SC_PII_OSI_M': -1, 'SC_PII_SOCKET': -1, 'SC_PII_XTI': -1, 'SC_POLL': -1,
    'SC_PRIORITIZED_IO': 200809, 'SC_PRIORITY_SCHEDULING': 200809, 'SC_REALTIME_SIGNALS': 200809,
    'SC_RE_DUP_MAX': 32767, 'SC_RTSIG_MAX': 32, 'SC_SAVED_IDS': 1, 'SC_SCHAR_MAX': 127, 'SC_SCHAR_MIN': -128,
    'SC_SELECT': -1, 'SC_SEMAPHORES': 200809, 'SC_SEM_NSEMS_MAX': -1, 'SC_SEM_VALUE_MAX': 2147483647,
    'SC_SHARED_MEMORY_OBJECTS': 200809, 'SC_SHRT_MAX': 32767, 'SC_SHRT_MIN': -32768, 'SC_SSIZE_MAX': 32767,
    'SC_STREAM_MAX': 16, 'SC_SYNCHRONIZED_IO': 200809, 'SC_THREADS': 200809, 'SC_THREAD_ATTR_STACKADDR': 200809,
    'SC_THREAD_ATTR_STACKSIZE': 200809, 'SC_THREAD_DESTRUCTOR_ITERATIONS': 4, 'SC_THREAD_KEYS_MAX': 1024,
    'SC_THREAD_PRIORITY_SCHEDULING': 200809, 'SC_THREAD_PRIO_INHERIT': 200809, 'SC_THREAD_PRIO_PROTECT': 200809,
    'SC_THREAD_PROCESS_SHARED': 200809, 'SC_THREAD_SAFE_FUNCTIONS': 200809, 'SC_THREAD_STACK_MIN': 16384,
    'SC_THREAD_THREADS_MAX': -1, 'SC_TIMERS': 200809, 'SC_TIMER_MAX': -1, 'SC_TTY_NAME_MAX': 32,
    'SC_TZNAME_MAX': -1, 'SC_T_IOV_MAX': -1, 'SC_UCHAR_MAX': 255, 'SC_UINT_MAX': 4294967295,
    'SC_UIO_MAXIOV': 1024, 'SC_ULONG_MAX': -1, 'SC_USHRT_MAX': 65535, 'SC_VERSION': 200809, 'SC_WORD_BIT': 32,
    'SC_XBS5_ILP32_OFF32': -1, 'SC_XBS5_ILP32_OFFBIG': -1, 'SC_XBS5_LP64_OFF64': 1, 'SC_XBS5_LPBIG_OFFBIG': -1,
    'SC_XOPEN_CRYPT': -1, 'SC_XOPEN_ENH_I18N': 1, 'SC_XOPEN_LEGACY': 1, 'SC_XOPEN_REALTIME': 1,
    'SC_XOPEN_REALTIME_THREADS': 1, 'SC_XOPEN_SHM': 1, 'SC_XOPEN_UNIX': 1, 'SC_XOPEN_VERSION': 700,
    'SC_XOPEN_XCU_VERSION': 4, 'SC_XOPEN_XPG2': 1, 'SC_XOPEN_XPG3': 1, 'SC_XOPEN_XPG4': 1,
    # Depende da CPU (o `AT_MINSIGSTKSZ` do kernel): o valor da máquina de referência.
    'SC_MINSIGSTKSZ': 3376,
}


def _proc_text(path):
    """O texto de um arquivo do `/proc` (`str`)."""
    fd = _os.open(path, 0, 0)
    try:
        return _os.read(fd, -1).decode('utf-8', 'replace')
    finally:
        _os.close(fd)


def _rlimit(resource):
    """O limite atual de um `RLIMIT_*`, com `-1` no ilimitado."""
    return _os.getrlimit(resource)[0]


def _online_cpus():
    """As CPUs do `/proc/stat` (a `get_nprocs` da glibc; sem o arquivo, uma)."""
    try:
        lines = _proc_text('/proc/stat').splitlines()
    except OSError:
        return 1
    return sum(1 for line in lines if line.startswith('cpu') and line[3:4].isdigit()) or 1


def _meminfo_pages(field):
    """Páginas de 4 KiB do campo do `/proc/meminfo` (o `sysinfo(2)` que a glibc usa, em kB)."""
    for line in _proc_text('/proc/meminfo').splitlines():
        if line.startswith(field + ':'):
            return int(line.split()[1]) * 1024 // 4096
    raise OSError(38, 'Function not implemented')


def _sc_arg_max():
    # O kernel reserva um quarto do limite da pilha para os argumentos; a glibc não baixa de 128 KiB.
    limit = _rlimit(3)
    if limit < 0:
        return 4611686018427387903
    return max(131072, limit // 4)


_SC_DYNAMIC = {
    'SC_ARG_MAX': _sc_arg_max,
    'SC_CHILD_MAX': lambda: _rlimit(6),
    'SC_OPEN_MAX': lambda: _rlimit(7),
    'SC_SIGQUEUE_MAX': lambda: _rlimit(11),
    'SC_NPROCESSORS_CONF': _online_cpus,
    'SC_NPROCESSORS_ONLN': _online_cpus,
    'SC_PHYS_PAGES': lambda: _meminfo_pages('MemTotal'),
    'SC_AVPHYS_PAGES': lambda: _meminfo_pages('MemFree'),
}
_SC_NAME_OF = {}
for _name, _code in sysconf_names.items():
    _SC_NAME_OF.setdefault(_code, _name)
del _name, _code
# As variáveis que só o número alcança (`_SC_ADVISORY_INFO`, `_SC_HOST_NAME_MAX`, `_SC_LEVEL1_ICACHE_SIZE`...);
# os tamanhos de cache e `_SC_SIGSTKSZ` (250) são os da máquina de referência.
_SC_EXTRA = {
    132: 200809, 133: 200809, 134: -1, 135: -1, 136: -1, 137: 200809, 138: 200809, 139: 200809, 140: -1,
    141: -1, 142: -1, 143: -1, 144: -1, 145: -1, 146: -1, 147: -1, 148: -1, 149: 200809, 150: -1, 151: -1,
    152: -1, 153: 200809, 154: 200809, 155: 1, 156: -1, 157: 1, 158: -1, 159: 200809, 160: -1, 161: -1,
    162: -1, 163: -1, 164: 200809, 165: -1, 166: -1, 167: -1, 168: -1, 169: -1, 170: -1, 171: -1, 172: -1,
    173: -1, 174: -1, 175: -1, 176: -1, 177: -1, 178: 1, 179: -1, 180: 64, 181: -1, 182: -1, 183: -1, 184: -1,
    185: 32768, 186: -1, 187: 64, 188: 32768, 189: 8, 190: 64, 191: 524288, 192: 8, 193: 64, 194: 16777216,
    195: 16, 196: 64, 197: 0, 198: -1, 199: -1, 235: 200809, 236: 200809, 237: -1, 238: -1, 239: 1, 240: -1,
    242: -1, 243: -1, 244: -1, 245: -1, 246: -1, 250: 13504,
}


def _conf_code(name, names):
    """O número de uma variável de `sysconf`/`confstr`/`pathconf`: o nome da tabela ou um inteiro."""
    if isinstance(name, str):
        try:
            return names[name]
        except KeyError:
            raise ValueError('unrecognized configuration name') from None
    if isinstance(name, int):
        return name
    raise TypeError('configuration names must be strings or integers')


def sysconf(name, /):
    """Return an integer-valued system configuration variable."""
    code = _conf_code(name, sysconf_names)
    # Os números fora da tabela da glibc (e `SC_EQUIV_CLASS_MAX`) são EINVAL.
    if code < 0 or code in (41, 241) or 200 <= code <= 234 or 247 <= code <= 248 or code > 250:
        raise OSError(22, 'Invalid argument')
    known = _SC_NAME_OF.get(code)
    if known is None:
        # Os que a glibc define mas o Python não nomeia (só por número).
        return _SC_EXTRA.get(code, -1)
    dynamic = _SC_DYNAMIC.get(known)
    if dynamic is not None:
        return dynamic()
    return _SC_FIXED.get(known, -1)


sysconf.__module__ = 'posix'


def process_cpu_count():
    """Get the number of CPUs of the current process.

Return the number of logical CPUs usable by the calling thread of the
current process. Return None if indeterminable."""
    import sys
    if sys._get_cpu_count_config() < 0:
        return len(sched_getaffinity(0))
    return cpu_count()


def getloadavg():
    """Return average recent system load information.

Return the number of processes in the system run queue averaged over
the last 1, 5, and 15 minutes as a tuple of three floats.
Raises OSError if the load average was unobtainable."""
    try:
        fields = _proc_text('/proc/loadavg').split()
        return (float(fields[0]), float(fields[1]), float(fields[2]))
    except (OSError, ValueError, IndexError):
        raise OSError('Load averages are unobtainable') from None


class times_result(tuple):
    """times_result: Result from os.times().

This object may be accessed either as a tuple of
  (user, system, children_user, children_system, elapsed),
or via the attributes user, system, children_user, children_system,
and elapsed.

See os.times for more information."""

    __module__ = 'posix'
    n_fields = 5
    n_sequence_fields = 5
    n_unnamed_fields = 0
    _fields = ('user', 'system', 'children_user', 'children_system', 'elapsed')

    def __new__(cls, sequence):
        items = tuple(sequence)
        if len(items) != 5:
            raise TypeError('posix.times_result() takes a 5-sequence (%d-sequence given)' % len(items))
        return tuple.__new__(cls, items)

    user = property(lambda self: self[0], doc='user time')
    system = property(lambda self: self[1], doc='system time')
    children_user = property(lambda self: self[2], doc='user time of children')
    children_system = property(lambda self: self[3], doc='system time of children')
    elapsed = property(lambda self: self[4], doc='elapsed real time since an arbitrary point in the past')

    def __repr__(self):
        return ('posix.times_result(user=%r, system=%r, children_user=%r, children_system=%r, elapsed=%r)'
                % tuple(self))


def _ticks(seconds):
    """Segundos em tiques de 10 ms (o `clock_t` do `times(2)`), como o CPython os divide por `CLK_TCK`."""
    return int(seconds * 1e9 + 0.5) // 10000000 / 100


def times():
    """Return a collection containing process timing information.

The object returned behaves like a named tuple with these fields:
  (utime, stime, cutime, cstime, elapsed_time)
All fields are floating-point numbers."""
    user, system = _os.getrusage(0)
    children_user, children_system = _os.getrusage(-1)
    elapsed = _ticks(float(_proc_text('/proc/uptime').split()[0]))
    return times_result((_ticks(user), _ticks(system), _ticks(children_user), _ticks(children_system), elapsed))


# ---- processos, grupos, sessões e terminais ----

def _c_int(value):
    """O `int` de C das chamadas de sistema (`pid_t`, política, prioridade): inteiro (ou `__index__`) entre
    `INT_MIN` e `INT_MAX`."""
    value = _as_index(value)
    if value > 2147483647:
        raise OverflowError('signed integer is greater than maximum')
    if value < -2147483648:
        raise OverflowError('signed integer is less than minimum')
    return value


def _off_t(value):
    """O `off_t` (`long` de C) das chamadas posicionais."""
    value = _as_index(value)
    if not -9223372036854775808 <= value <= 9223372036854775807:
        raise OverflowError('Python int too large to convert to C long')
    return value


def getpgid(pid):
    """Call the system call getpgid() and return the process group ID."""
    return _os.getpgid(_c_int(pid))


def getpgrp():
    """Return the current process group id."""
    return _os.getpgid(0)


def setpgid(pid, pgrp, /):
    """Call the system call setpgid(pid, pgrp)."""
    _os.setpgid(_c_int(pid), _c_int(pgrp))


def setpgrp():
    """Make the current process the leader of its process group."""
    _os.setpgid(0, 0)


def getsid(pid, /):
    """Call the system call getsid(pid) and return the result."""
    return _os.getsid(_c_int(pid))


def setsid():
    """Call the system call setsid()."""
    _os.setsid()


def killpg(pgid, signal, /):
    """Kill a process group with a signal."""
    pgid = _c_int(pgid)
    signal = _as_index(signal)
    if pgid < 0:
        raise OSError(22, 'Invalid argument')
    _os.kill_many(0, pgid, signal)


def abort():
    """Abort the interpreter immediately.

This function 'dumps core' or otherwise fails in the hardest way possible
on the hosting operating system.  This function never returns."""
    import signal
    # A glibc devolve o SIGABRT à ação padrão antes de se matar, então um tratador nunca o salva.
    try:
        signal.signal(signal.SIGABRT, signal.SIG_DFL)
    except ValueError:
        pass
    _os.kill(_os.getpid(), signal.SIGABRT)


def tcgetpgrp(fd, /):
    """Return the process group associated with the terminal specified by fd."""
    return _os.tcgetpgrp(_fd_arg(fd))


def tcsetpgrp(fd, pgid, /):
    """Set the process group associated with the terminal specified by fd."""
    _os.tcsetpgrp(_fd_arg(fd), _c_int(pgid))


def ctermid():
    """Return the name of the controlling terminal for this process."""
    return '/dev/tty'


def ttyname(fd, /):
    """Return the name of the terminal device connected to 'fd'.

  fd
    Integer file descriptor handle."""
    fd = _fd_arg(fd)
    if not _os.isatty(fd):
        _os.fstat(fd)
        raise OSError(25, 'Inappropriate ioctl for device')
    return _os.readlink('/proc/self/fd/%d' % fd)


def posix_openpt(oflag, /):
    """Open and return a file descriptor for a master pseudo-terminal device.

Performs a posix_openpt() C function call. The oflag argument is used to
set file status flags and file access modes as specified in the manual page
of posix_openpt() of your system."""
    return _os.open('/dev/ptmx', _c_int(oflag), 0)


def _pty_master_call(call, fd):
    """O `grantpt` e o `unlockpt` da glibc trocam o ENOTTY de um fd que não é mestre de pty por EINVAL."""
    try:
        call(_fd_arg(fd))
    except OSError as error:
        if error.errno == 25:
            raise OSError(22, 'Invalid argument') from None
        raise


def grantpt(fd, /):
    """Grant access to the slave pseudo-terminal device.

  fd
    File descriptor of a master pseudo-terminal device.

Performs a grantpt() C function call."""
    _pty_master_call(_os.pty_number, fd)


def unlockpt(fd, /):
    """Unlock a pseudo-terminal master/slave pair.

  fd
    File descriptor of a master pseudo-terminal device.

Performs an unlockpt() C function call."""
    _pty_master_call(_os.pty_unlock, fd)


def ptsname(fd, /):
    """Return the name of the slave pseudo-terminal device.

  fd
    File descriptor of a master pseudo-terminal device.

If the ptsname_r() C function is available, it is called;
otherwise, performs a ptsname() C function call."""
    return '/dev/pts/%d' % _os.pty_number(_fd_arg(fd))


def device_encoding(fd):
    """Return a string describing the encoding of a terminal's file descriptor.

The file descriptor must be attached to a terminal.
If the device is not a terminal, return None."""
    return 'UTF-8' if _os.isatty(_fd_arg(fd)) else None


# ---- escalonador e prioridade ----

class sched_param(tuple):
    """Currently has only one field: sched_priority

  sched_priority
    A scheduling parameter."""

    __module__ = 'posix'
    n_fields = 1
    n_sequence_fields = 1
    n_unnamed_fields = 0
    _fields = ('sched_priority',)

    def __new__(cls, sched_priority):
        return tuple.__new__(cls, (sched_priority,))

    sched_priority = property(lambda self: self[0], doc='the scheduling priority')

    def __repr__(self):
        return 'posix.sched_param(sched_priority=%r)' % (self[0],)


def _sched_priority(function, param):
    if not isinstance(param, sched_param):
        raise TypeError('%s() argument 2 must be posix.sched_param, not %s' % (function, type(param).__name__))
    return _c_int(param.sched_priority)


def sched_yield():
    """Voluntarily relinquish the CPU."""
    _os.sched_yield()


def sched_get_priority_max(policy):
    """Get the maximum scheduling priority for policy."""
    return _os.sched_get_priority_max(_c_int(policy))


def sched_get_priority_min(policy):
    """Get the minimum scheduling priority for policy."""
    return _os.sched_get_priority_min(_c_int(policy))


def sched_getscheduler(pid, /):
    """Get the scheduling policy for the process identified by pid.

Passing 0 for pid returns the scheduling policy for the calling process."""
    return _os.sched_getscheduler(_c_int(pid))


def sched_setscheduler(pid, policy, param, /):
    """Set the scheduling policy for the process identified by pid.

If pid is 0, the calling process is changed.
param is an instance of sched_param."""
    pid = _c_int(pid)
    policy = _c_int(policy)
    _os.sched_setscheduler(pid, policy, _sched_priority('sched_setscheduler', param))


def sched_getparam(pid, /):
    """Returns scheduling parameters for the process identified by pid.

If pid is 0, returns parameters for the calling process.
Return value is an instance of sched_param."""
    return sched_param(_os.sched_getparam(_c_int(pid)))


def sched_setparam(pid, param, /):
    """Set scheduling parameters for the process identified by pid.

If pid is 0, sets parameters for the calling process.
param should be an instance of sched_param."""
    pid = _c_int(pid)
    _os.sched_setparam(pid, _sched_priority('sched_setparam', param))


def sched_rr_get_interval(pid, /):
    """Return the round-robin quantum for the process identified by pid, in seconds.

Value returned is a float."""
    return _os.sched_rr_get_interval(_c_int(pid))


def sched_getaffinity(pid, /):
    """Return the affinity of the process identified by pid (or the current process if zero).

The affinity is returned as a set of CPU identifiers."""
    return set(_os.sched_getaffinity(_c_int(pid)))


def sched_setaffinity(pid, mask, /):
    """Set the CPU affinity of the process identified by pid to mask.

mask should be an iterable of integers identifying CPUs."""
    pid = _c_int(pid)
    cpus = []
    for cpu in mask:
        cpu = _as_index(cpu)
        if cpu < 0:
            raise ValueError('negative CPU number')
        cpus.append(cpu)
    _os.sched_setaffinity(pid, cpus)


PRIO_PROCESS, PRIO_PGRP, PRIO_USER = 0, 1, 2


def _priority_targets(which, who):
    """Os processos que o `getpriority`/`setpriority` alcançam: `who` é um pid, um grupo ou um usuário."""
    which = _c_int(which)
    who = _c_int(who)
    if which == PRIO_PROCESS:
        return [who]
    if which not in (PRIO_PGRP, PRIO_USER):
        raise OSError(22, 'Invalid argument')
    wanted = who or (_os.getpgid(0) if which == PRIO_PGRP else _os._creds('uid'))
    found = []
    for entry in _os.listdir('/proc'):
        if not entry.isdigit():
            continue
        try:
            owner = _os.getpgid(int(entry)) if which == PRIO_PGRP else stat('/proc/' + entry).st_uid
        except OSError:
            continue
        if owner == wanted:
            found.append(int(entry))
    return found


def getpriority(which, who):
    """Return program scheduling priority."""
    pids = _priority_targets(which, who)
    if which == PRIO_PROCESS:
        return _os.getpriority(pids[0])
    values = []
    for pid in pids:
        try:
            values.append(_os.getpriority(pid))
        except OSError:
            pass
    if not values:
        raise OSError(3, 'No such process')
    return min(values)


def setpriority(which, who, priority):
    """Set program scheduling priority."""
    priority = _c_int(priority)
    pids = _priority_targets(which, who)
    if which == PRIO_PROCESS:
        return _os.setpriority(pids[0], priority)
    if not pids:
        raise OSError(3, 'No such process')
    for pid in pids:
        _os.setpriority(pid, priority)


def nice(increment, /):
    """Add increment to the priority of process and return the new priority."""
    increment = _c_int(increment)
    # O `nice(3)` da glibc: lê a prioridade, soma e grava (o kernel limita a -20..19), e lê de novo.
    current = _os.getpriority(0)
    try:
        _os.setpriority(0, current + increment)
    except PermissionError as error:
        if error.errno == 13:
            raise OSError(1, 'Operation not permitted') from None
        raise
    return _os.getpriority(0)


# ---- leitura e escrita posicionais, vetoriais e entre descritores ----

def pread(fd, length, offset, /):
    """Read a number of bytes from a file descriptor starting at a particular offset.

Read length bytes from file descriptor fd, starting at offset bytes from
the beginning of the file.  The file offset remains unchanged."""
    fd = _fd_arg(fd)
    length = _as_index(length)
    offset = _off_t(offset)
    if length < 0:
        raise OSError(22, 'Invalid argument')
    return _os.pread(fd, length, offset)


def pwrite(fd, data, offset, /):
    """Write bytes to a file descriptor starting at a particular offset.

Write buffer to fd, starting at offset bytes from the beginning of
the file.  Returns the number of bytes written.  Does not change the
current file offset."""
    return _os.pwrite(_fd_arg(fd), data, _off_t(offset))


def _scatter(data, buffers):
    """Reparte `data` pelos buffers, na ordem, até acabar; devolve quantos bytes couberam."""
    position = 0
    for buffer in buffers:
        view = memoryview(buffer)
        chunk = data[position:position + view.nbytes]
        if not chunk:
            break
        view[:len(chunk)] = chunk
        position += len(chunk)
    return position


def _gather(buffers):
    return b''.join(bytes(memoryview(buffer)) for buffer in buffers)


def _capacity(buffers):
    return sum(memoryview(buffer).nbytes for buffer in buffers)


def readv(fd, buffers, /):
    """Read from a file descriptor fd into an iterable of buffers.

The buffers should be mutable buffers accepting bytes.
readv will transfer data into each buffer until it is full
and then move on to the next buffer in the sequence to hold
the rest of the data.

readv returns the total number of bytes read,
which may be less than the total capacity of all the buffers."""
    fd = _fd_arg(fd)
    buffers = list(buffers)
    capacity = _capacity(buffers)
    if not capacity:
        return 0
    import _net
    return _scatter(_net.read(fd, capacity), buffers)


def writev(fd, buffers, /):
    """Iterate over buffers, and write the contents of each to a file descriptor.

Returns the total number of bytes written.
buffers must be a sequence of bytes-like objects."""
    fd = _fd_arg(fd)
    import _net
    return _net.write(fd, _gather(buffers))


def _rwf_flags(flags):
    flags = _as_index(flags)
    if flags & ~(RWF_HIPRI | RWF_DSYNC | RWF_SYNC | RWF_NOWAIT | RWF_APPEND):
        raise OSError(95, 'Operation not supported')
    return flags


def preadv(fd, buffers, offset, flags=0, /):
    """Reads from a file descriptor into a number of mutable bytes-like objects.

Combines the functionality of readv() and pread(). As readv(), it will
transfer data into each buffer until it is full and then move on to the next
buffer in the sequence to hold the rest of the data. Its fourth argument,
specifies the file offset at which the input operation is to be performed. It
will return the total number of bytes read (which can be less than the total
capacity of all the objects)."""
    fd = _fd_arg(fd)
    offset = _off_t(offset)
    _rwf_flags(flags)
    buffers = list(buffers)
    capacity = _capacity(buffers)
    if not capacity:
        return 0
    return _scatter(_os.pread(fd, capacity, offset), buffers)


def pwritev(fd, buffers, offset, flags=0, /):
    """Writes the contents of bytes-like objects to a file descriptor at a given offset.

Combines the functionality of writev() and pwrite(). All buffers must be a sequence
of bytes-like objects. Buffers are processed in array order. Entire contents of first
buffer is written before proceeding to second, and so on. The operating system may
set a limit (sysconf() value SC_IOV_MAX) on the number of buffers that can be used.
This function writes the contents of each object to the file descriptor and returns
the total number of bytes written."""
    fd = _fd_arg(fd)
    offset = _off_t(offset)
    if _rwf_flags(flags) & RWF_APPEND:
        offset = fstat(fd).st_size
    return _os.pwrite(fd, _gather(buffers), offset)


_COPY_CHUNK = 65536


def _is_fifo(fd):
    return (fstat(fd).st_mode & 0o170000) == 0o010000


def _transfer(src, dst, count, offset_src, offset_dst, once):
    """Move até `count` bytes de `src` para `dst` (lendo e gravando nos deslocamentos dados, ou nos do
    descritor quando `None`); `once` pára na primeira leitura, como o `splice` de um pipe."""
    import _net
    total = 0
    while total < count:
        size = min(count - total, _COPY_CHUNK)
        if offset_src is None:
            data = _net.read(src, size)
        else:
            data = _os.pread(src, size, offset_src + total)
        if not data:
            break
        if offset_dst is None:
            _net.write(dst, data)
        else:
            _os.pwrite(dst, data, offset_dst + total)
        total += len(data)
        if once:
            break
    return total


def sendfile(out_fd, in_fd, offset, count):
    """Copy count bytes from file descriptor in_fd to file descriptor out_fd."""
    out_fd = _fd_arg(out_fd)
    in_fd = _fd_arg(in_fd)
    count = _as_index(count)
    if offset is not None:
        offset = _off_t(offset)
    if count < 0:
        raise OSError(22, 'Invalid argument')
    return _transfer(in_fd, out_fd, count, offset, None, False)


def copy_file_range(src, dst, count, offset_src=None, offset_dst=None):
    """Copy count bytes from one file descriptor to another.

  src
    Source file descriptor.
  dst
    Destination file descriptor.
  count
    Number of bytes to copy.
  offset_src
    Starting offset in src.
  offset_dst
    Starting offset in dst.

If offset_src is None, then src is read from the current position;
respectively for offset_dst."""
    src = _fd_arg(src)
    dst = _fd_arg(dst)
    count = _as_index(count)
    if offset_src is not None:
        offset_src = _as_index(offset_src)
    if offset_dst is not None:
        offset_dst = _as_index(offset_dst)
    if count < 0:
        raise OSError(22, 'Invalid argument')
    for fd in (src, dst):
        if (fstat(fd).st_mode & 0o170000) != 0o100000:
            raise OSError(22, 'Invalid argument')
    return _transfer(src, dst, count, offset_src, offset_dst, False)


def splice(src, dst, count, offset_src=None, offset_dst=None, flags=0):
    """Transfer count bytes from one pipe to a descriptor or vice versa.

  src
    Source file descriptor.
  dst
    Destination file descriptor.
  count
    Number of bytes to copy.
  offset_src
    Starting offset in src.
  offset_dst
    Starting offset in dst.
  flags
    Flags to modify the semantics of the call.

If offset_src is None, then src is read from the current position;
respectively for offset_dst. The offset associated to the file
descriptor that refers to a pipe must be None."""
    src = _fd_arg(src)
    dst = _fd_arg(dst)
    count = _as_index(count)
    flags = _as_index(flags)
    if offset_src is not None:
        offset_src = _as_index(offset_src)
    if offset_dst is not None:
        offset_dst = _as_index(offset_dst)
    src_fifo, dst_fifo = _is_fifo(src), _is_fifo(dst)
    if not (src_fifo or dst_fifo) or flags & ~15 or count < 0:
        raise OSError(22, 'Invalid argument')
    if (src_fifo and offset_src is not None) or (dst_fifo and offset_dst is not None):
        raise OSError(29, 'Illegal seek')
    if dst_fifo:
        count = min(count, _COPY_CHUNK)
    return _transfer(src, dst, count, offset_src, offset_dst, True)


def posix_fallocate(fd, offset, length, /):
    """Ensure a file has allocated at least a particular number of bytes on disk.

Ensure that the file specified by fd encompasses a range of bytes
starting at offset bytes from the beginning and continuing for length bytes."""
    _os.fallocate(_fd_arg(fd), 0, _as_index(offset), _as_index(length))


def posix_fadvise(fd, offset, length, advice, /):
    """Announce an intention to access data in a specific pattern.

Announce an intention to access data in a specific pattern, thus allowing
the kernel to make optimizations.
The advice applies to the region of the file specified by fd starting at
offset and continuing for length bytes.
advice is one of POSIX_FADV_NORMAL, POSIX_FADV_SEQUENTIAL,
POSIX_FADV_RANDOM, POSIX_FADV_NOREUSE, POSIX_FADV_WILLNEED, or
POSIX_FADV_DONTNEED."""
    fd = _fd_arg(fd)
    offset, length, advice = _as_index(offset), _as_index(length), _as_index(advice)
    if (fstat(fd).st_mode & 0o170000) == 0o010000:
        raise OSError(29, 'Illegal seek')
    if not 0 <= advice <= 5 or offset < 0 or length < 0:
        raise OSError(22, 'Invalid argument')


def lockf(fd, command, length, /):
    """Apply, test or remove a POSIX lock on an open file descriptor.

  fd
    An open file descriptor.
  command
    One of F_LOCK, F_TLOCK, F_ULOCK or F_TEST.
  length
    The number of bytes to lock, starting at the current position."""
    import fcntl
    fd = _fd_arg(fd)
    command = _as_index(command)
    length = _as_index(length)
    if command == F_ULOCK:
        fcntl.lockf(fd, fcntl.LOCK_UN, length, 0, 1)
    elif command == F_LOCK:
        fcntl.lockf(fd, fcntl.LOCK_EX, length, 0, 1)
    elif command == F_TLOCK:
        fcntl.lockf(fd, fcntl.LOCK_EX | fcntl.LOCK_NB, length, 0, 1)
    elif command == F_TEST:
        import struct
        probe = struct.pack('<hh4xqqi4x', fcntl.F_WRLCK, 1, 0, length, 0)
        kind, _, _, _, owner = struct.unpack('<hh4xqqi4x', fcntl.fcntl(fd, fcntl.F_GETLK, probe))
        if kind != fcntl.F_UNLCK and owner != getpid():
            raise OSError(13, 'Permission denied')
    else:
        raise OSError(22, 'Invalid argument')


def getrandom(size, flags=0):
    """Obtain a series of random bytes."""
    size = _as_index(size)
    flags = _as_index(flags)
    if size < 0 or flags & ~(GRND_NONBLOCK | GRND_RANDOM):
        raise OSError(22, 'Invalid argument')
    return _os.urandom(size)


def getcwdb():
    """Return a bytes string representing the current working directory."""
    return fsencode(_os.getcwd())


def closerange(fd_low, fd_high, /):
    """Closes all file descriptors in [fd_low, fd_high), ignoring errors."""
    fd_low, fd_high = _fd_arg(fd_low), _fd_arg(fd_high)
    for entry in _os.listdir('/proc/self/fd'):
        fd = int(entry)
        if fd_low <= fd < fd_high:
            try:
                _os.close(fd)
            except OSError:
                pass


def pipe2(flags, /):
    """Create a pipe with flags set atomically.

Returns a tuple of two file descriptors:
  (read_fd, write_fd)

flags can be constructed by ORing together one or more of these values:
O_NONBLOCK, O_CLOEXEC."""
    flags = _as_index(flags)
    if flags & ~(O_CLOEXEC | O_NONBLOCK | O_DIRECT):
        raise OSError(22, 'Invalid argument')
    read_fd, write_fd = _os.pipe()
    for fd in (read_fd, write_fd):
        _os.set_inheritable(fd, 0 if flags & O_CLOEXEC else 1)
        if flags & O_NONBLOCK:
            _os.set_blocking(fd, False)
    return read_fd, write_fd


def fchdir(fd):
    """Change to the directory of the given file descriptor.

fd must be opened on a directory, not a file.
Equivalent to os.chdir(fd)."""
    _os.fchdir(_as_fd(fd))


def fchmod(fd, mode):
    """Change the access permissions of the file given by file descriptor fd.

  fd
    The file descriptor of the file to be modified.
  mode
    Operating-system mode bitfield.

Equivalent to os.chmod(fd, mode)."""
    _os.fchmod(_as_fd(fd), _as_index(mode))


def fchown(fd, uid, gid):
    """Change the owner and group id of the file specified by file descriptor.

Equivalent to os.chown(fd, uid, gid)."""
    _os.fchown(_as_fd(fd), _as_index(uid), _as_index(gid))


def unshare(flags):
    """Disassociate parts of a process (or thread) execution context.

  flags
    Namespaces to be unshared."""
    _as_index(flags)
    # O contêiner do oráculo não tem `CAP_SYS_ADMIN` e o seccomp do docker recusa o `unshare` com EPERM.
    raise OSError(1, 'Operation not permitted')


def setns(fd, nstype=0):
    """Move the calling thread into different namespaces.

  fd
    A file descriptor to a namespace.
  nstype
    Type of namespace."""
    _as_fd(fd)
    _as_index(nstype)
    # O contêiner do oráculo não tem `CAP_SYS_ADMIN` e o seccomp do docker recusa o `setns` com EPERM, antes
    # de o kernel olhar o descritor.
    raise OSError(1, 'Operation not permitted')


# `fwalk` do `os.py` do CPython (o `scandir` e o `stat` aceitam descritores de diretório).

def fwalk(top=".", topdown=True, onerror=None, *, follow_symlinks=False, dir_fd=None):
    """Directory tree generator.

    This behaves exactly like walk(), except that it yields a 4-tuple

        dirpath, dirnames, filenames, dirfd

    `dirpath`, `dirnames` and `filenames` are identical to walk() output,
    and `dirfd` is a file descriptor referring to the directory `dirpath`.

    The advantage of fwalk() over walk() is that it's safe against symlink
    races (when follow_symlinks is False).

    If dir_fd is not None, it should be a file descriptor open to a directory,
      and top should be relative; top will then be relative to that directory.
      (dir_fd is always supported for fwalk.)

    Caution:
    Since fwalk() yields file descriptors, those are only valid until the
    next iteration step, so you should dup() them if you want to keep them
    for a longer period.

    Example:

    import os
    for root, dirs, files, rootfd in os.fwalk('python/Lib/xml'):
        print(root, "consumes", end="")
        print(sum(os.stat(name, dir_fd=rootfd).st_size for name in files),
              end="")
        print("bytes in", len(files), "non-directory files")
        if '__pycache__' in dirs:
            dirs.remove('__pycache__')  # don't visit __pycache__ directories
    """
    sys.audit("os.fwalk", top, topdown, onerror, follow_symlinks, dir_fd)
    top = fspath(top)
    stack = [(_fwalk_walk, (True, dir_fd, top, top, None))]
    isbytes = isinstance(top, bytes)
    try:
        while stack:
            yield from _fwalk(stack, isbytes, topdown, onerror, follow_symlinks)
    finally:
        # Close any file descriptors still on the stack.
        while stack:
            action, value = stack.pop()
            if action == _fwalk_close:
                close(value)


# Each item in the _fwalk() stack is a pair (action, args).
_fwalk_walk = 0  # args: (isroot, dirfd, toppath, topname, entry)
_fwalk_yield = 1  # args: (toppath, dirnames, filenames, topfd)
_fwalk_close = 2  # args: dirfd


def _fwalk(stack, isbytes, topdown, onerror, follow_symlinks):
    # Note: This uses O(depth of the directory tree) file descriptors: if
    # necessary, it can be adapted to only require O(1) FDs, see issue
    # #13734.

    action, value = stack.pop()
    if action == _fwalk_close:
        close(value)
        return
    elif action == _fwalk_yield:
        yield value
        return
    assert action == _fwalk_walk
    isroot, dirfd, toppath, topname, entry = value
    try:
        if not follow_symlinks:
            # Note: To guard against symlink races, we use the standard
            # lstat()/open()/fstat() trick.
            if entry is None:
                orig_st = stat(topname, follow_symlinks=False, dir_fd=dirfd)
            else:
                orig_st = entry.stat(follow_symlinks=False)
        topfd = open(topname, O_RDONLY | O_NONBLOCK, dir_fd=dirfd)
    except OSError as err:
        if isroot:
            raise
        if onerror is not None:
            onerror(err)
        return
    stack.append((_fwalk_close, topfd))
    if not follow_symlinks:
        if isroot and not st.S_ISDIR(orig_st.st_mode):
            return
        if not path.samestat(orig_st, stat(topfd)):
            return

    scandir_it = scandir(topfd)
    dirs = []
    nondirs = []
    entries = None if topdown or follow_symlinks else []
    for entry in scandir_it:
        name = entry.name
        if isbytes:
            name = fsencode(name)
        try:
            if entry.is_dir():
                dirs.append(name)
                if entries is not None:
                    entries.append(entry)
            else:
                nondirs.append(name)
        except OSError:
            try:
                # Add dangling symlinks, ignore disappeared files
                if entry.is_symlink():
                    nondirs.append(name)
            except OSError:
                pass

    if topdown:
        yield toppath, dirs, nondirs, topfd
    else:
        stack.append((_fwalk_yield, (toppath, dirs, nondirs, topfd)))

    toppath = path.join(toppath, toppath[:0])  # Add trailing slash.
    if entries is None:
        stack.extend(
            (_fwalk_walk, (False, topfd, toppath + name, name, None))
            for name in dirs[::-1])
    else:
        stack.extend(
            (_fwalk_walk, (False, topfd, toppath + name, name, entry))
            for name, entry in zip(dirs[::-1], entries[::-1]))


# ---- atributos estendidos, memfd, eventfd, timerfd e chroot ----

def _xattr_target(function, path, follow_symlinks):
    if isinstance(path, int):
        if not follow_symlinks:
            raise ValueError('%s: cannot use fd and follow_symlinks together' % function)
        return path
    return fspath(path)


def getxattr(path, attribute, *, follow_symlinks=True):
    """Return the value of extended attribute attribute on path.

path may be either a string, a path-like object, or an open file descriptor.
If follow_symlinks is False, and the last element of the path is a symbolic
  link, getxattr will examine the symbolic link itself instead of the file
  the link points to."""
    target = _xattr_target('getxattr', path, follow_symlinks)
    return _os.getxattr(target, fspath(attribute), follow_symlinks)


def setxattr(path, attribute, value, flags=0, *, follow_symlinks=True):
    """Set extended attribute attribute on path to value.

path may be either a string, a path-like object,  or an open file descriptor.
If follow_symlinks is False, and the last element of the path is a symbolic
  link, setxattr will modify the symbolic link itself instead of the file
  the link points to."""
    target = _xattr_target('setxattr', path, follow_symlinks)
    _os.setxattr(target, fspath(attribute), value, _as_index(flags), follow_symlinks)


def removexattr(path, attribute, *, follow_symlinks=True):
    """Remove extended attribute attribute on path.

path may be either a string, a path-like object, or an open file descriptor.
If follow_symlinks is False, and the last element of the path is a symbolic
  link, removexattr will modify the symbolic link itself instead of the file
  the link points to."""
    target = _xattr_target('removexattr', path, follow_symlinks)
    _os.removexattr(target, fspath(attribute), follow_symlinks)


def listxattr(path=None, *, follow_symlinks=True):
    """Return a list of extended attributes on path.

path may be either None, a string, a path-like object, or a file descriptor.
If path is None, listxattr will examine the current directory.
If follow_symlinks is False, and the last element of the path is a symbolic
  link, listxattr will examine the symbolic link itself instead of the file
  the link points to."""
    target = _xattr_target('listxattr', '.' if path is None else path, follow_symlinks)
    names = _os.listxattr(target, follow_symlinks)
    return [fsdecode(name) for name in names]


def memfd_create(name, flags=MFD_CLOEXEC):
    """Create an anonymous file."""
    return _os.memfd_create(fsencode(name), _as_index(flags))


def chroot(path):
    """Change root directory to path."""
    _os.chroot(fspath(path))


def eventfd(initval, flags=EFD_CLOEXEC):
    """Creates and returns an event notification file descriptor."""
    initval = _as_index(initval)
    if not 0 <= initval <= 4294967295:
        raise OverflowError('unsigned int is greater than maximum' if initval > 0 else 'can\'t convert negative value to unsigned int')
    return _os.eventfd(initval, _c_int(flags))


def eventfd_read(fd):
    """Read eventfd value"""
    import _net
    fd = _fd_arg(fd)
    # Contador zero: espera o `POLLIN` rodando as outras threads (só uma delas pode escrever).
    return int.from_bytes(_net.read(fd, 8), 'little')


def eventfd_write(fd, value):
    """Write eventfd value."""
    fd = _fd_arg(fd)
    value = _as_index(value)
    if not 0 <= value <= 18446744073709551615:
        raise OverflowError('Python int too large to convert to C unsigned long' if value > 0 else 'can\'t convert negative int to unsigned')
    import _net
    # Soma que estouraria o contador espera o `POLLOUT` (algum leitor zerá-lo) rodando as outras threads.
    _net.wait_writable(fd)
    _os.write(fd, value.to_bytes(8, 'little'))


def timerfd_create(clockid, /, *, flags=0):
    """Create and return a timer file descriptor.

  clockid
    A valid clock ID constant as timer file descriptor.

    time.CLOCK_REALTIME
    time.CLOCK_MONOTONIC
    time.CLOCK_BOOTTIME
  flags
    0 or a bit mask of os.TFD_NONBLOCK or os.TFD_CLOEXEC.

    os.TFD_NONBLOCK
        If *TFD_NONBLOCK* is set as a flag, read doesn't blocks.
        If *TFD_NONBLOCK* is not set as a flag, read block until the timer fires.

    os.TFD_CLOEXEC
        If *TFD_CLOEXEC* is set as a flag, enable the close-on-exec flag"""
    return _os.timerfd_create(_c_int(clockid), _c_int(flags))


def _timer_ns(value):
    """Os nanossegundos de um prazo em segundos (`float` arredondado para baixo, ou `int`). Prazo negativo
    segue ao kernel, que o recusa com `EINVAL`."""
    if isinstance(value, float):
        from math import floor
        if value != value:
            raise ValueError('Invalid value NaN (not a number)')
        return int(floor(value * 1e9))
    return _as_index(value) * 1000000000


def timerfd_settime(fd, /, *, flags=0, initial=0.0, interval=0.0):
    """Alter a timer file descriptor's internal timer in seconds.

  fd
    A timer file descriptor.
  flags
    0 or a bit mask of TFD_TIMER_ABSTIME or TFD_TIMER_CANCEL_ON_SET.
  initial
    The initial expiration time, in seconds.
  interval
    The timer's interval, in seconds."""
    remaining, every = _os.timerfd_settime(_fd_arg(fd), _c_int(flags), _timer_ns(initial),
                                           _timer_ns(interval))
    return remaining / 1e9, every / 1e9


def timerfd_settime_ns(fd, /, *, flags=0, initial=0, interval=0):
    """Alter a timer file descriptor's internal timer in nanoseconds.

  fd
    A timer file descriptor.
  flags
    0 or a bit mask of TFD_TIMER_ABSTIME or TFD_TIMER_CANCEL_ON_SET.
  initial
    initial expiration timing in seconds.
  interval
    interval for the timer in seconds."""
    return tuple(_os.timerfd_settime(_fd_arg(fd), _c_int(flags), _as_index(initial), _as_index(interval)))


def timerfd_gettime(fd, /):
    """Return a tuple of a timer file descriptor's (interval, next expiration) in float seconds.

  fd
    A timer file descriptor."""
    remaining, every = _os.timerfd_gettime(_fd_arg(fd))
    return remaining / 1e9, every / 1e9


def timerfd_gettime_ns(fd, /):
    """Return a tuple of a timer file descriptor's (interval, next expiration) in nanoseconds.

  fd
    A timer file descriptor."""
    return tuple(_os.timerfd_gettime(_fd_arg(fd)))


# `spawn*()` do `os.py` do CPython, que no Linux são `fork` + `exec*` (o `posix` não tem `spawnv`).

def _spawnvef(mode, file, args, env, func):
    # Internal helper; func is the exec*() function to use
    if not isinstance(args, (tuple, list)):
        raise TypeError('argv must be a tuple or a list')
    if not args or not args[0]:
        raise ValueError('argv first element cannot be empty')
    pid = fork()
    if not pid:
        # Child
        try:
            if env is None:
                func(file, args)
            else:
                func(file, args, env)
        except:
            _exit(127)
    else:
        # Parent
        if mode == P_NOWAIT:
            return pid  # Caller is responsible for waiting!
        while 1:
            wpid, sts = waitpid(pid, 0)
            if WIFSTOPPED(sts):
                continue
            return waitstatus_to_exitcode(sts)


def spawnv(mode, file, args):
    """spawnv(mode, file, args) -> integer

Execute file with arguments from args in a subprocess.
If mode == P_NOWAIT return the pid of the process.
If mode == P_WAIT return the process's exit code if it exits normally;
otherwise return -SIG, where SIG is the signal that killed it. """
    return _spawnvef(mode, file, args, None, execv)


def spawnve(mode, file, args, env):
    """spawnve(mode, file, args, env) -> integer

Execute file with arguments from args in a subprocess with the
specified environment.
If mode == P_NOWAIT return the pid of the process.
If mode == P_WAIT return the process's exit code if it exits normally;
otherwise return -SIG, where SIG is the signal that killed it. """
    return _spawnvef(mode, file, args, env, execve)


def spawnvp(mode, file, args):
    """spawnvp(mode, file, args) -> integer

Execute file (which is looked for along $PATH) with arguments from
args in a subprocess.
If mode == P_NOWAIT return the pid of the process.
If mode == P_WAIT return the process's exit code if it exits normally;
otherwise return -SIG, where SIG is the signal that killed it. """
    return _spawnvef(mode, file, args, None, execvp)


def spawnvpe(mode, file, args, env):
    """spawnvpe(mode, file, args, env) -> integer

Execute file (which is looked for along $PATH) with arguments from
args in a subprocess with the supplied environment.
If mode == P_NOWAIT return the pid of the process.
If mode == P_WAIT return the process's exit code if it exits normally;
otherwise return -SIG, where SIG is the signal that killed it. """
    return _spawnvef(mode, file, args, env, execvpe)


def spawnl(mode, file, *args):
    """spawnl(mode, file, *args) -> integer

Execute file with arguments from args in a subprocess.
If mode == P_NOWAIT return the pid of the process.
If mode == P_WAIT return the process's exit code if it exits normally;
otherwise return -SIG, where SIG is the signal that killed it. """
    return spawnv(mode, file, args)


def spawnle(mode, file, *args):
    """spawnle(mode, file, *args, env) -> integer

Execute file with arguments from args in a subprocess with the
supplied environment.
If mode == P_NOWAIT return the pid of the process.
If mode == P_WAIT return the process's exit code if it exits normally;
otherwise return -SIG, where SIG is the signal that killed it. """
    env = args[-1]
    return spawnve(mode, file, args[:-1], env)


def spawnlp(mode, file, *args):
    """spawnlp(mode, file, *args) -> integer

Execute file (which is looked for along $PATH) with arguments from
args in a subprocess with the supplied environment.
If mode == P_NOWAIT return the pid of the process.
If mode == P_WAIT return the process's exit code if it exits normally;
otherwise return -SIG, where SIG is the signal that killed it. """
    return spawnvp(mode, file, args)


def spawnlpe(mode, file, *args):
    """spawnlpe(mode, file, *args, env) -> integer

Execute file (which is looked for along $PATH) with arguments from
args in a subprocess with the supplied environment.
If mode == P_NOWAIT return the pid of the process.
If mode == P_WAIT return the process's exit code if it exits normally;
otherwise return -SIG, where SIG is the signal that killed it. """
    env = args[-1]
    return spawnvpe(mode, file, args[:-1], env)


# O módulo `posix` (de onde o CPython tira estas funções) recebe-as quando o `os` termina de carregar.
def _init_posix():
    init = globals().pop('_posix_init', None)
    if init is not None:
        init()

_init_posix()
del _init_posix


def _build_supports():
    import posix
    _globals = globals()

    def _add(str, fn):
        if (fn in _globals) and (str in _have_functions):
            _set.add(_globals[fn])

    _have_functions = posix._have_functions
    global supports_dir_fd, supports_effective_ids, supports_fd, supports_follow_symlinks

    _set = set()
    _add("HAVE_FACCESSAT",  "access")
    _add("HAVE_FCHMODAT",   "chmod")
    _add("HAVE_FCHOWNAT",   "chown")
    _add("HAVE_FSTATAT",    "stat")
    _add("HAVE_LSTAT",      "lstat")
    _add("HAVE_FUTIMESAT",  "utime")
    _add("HAVE_LINKAT",     "link")
    _add("HAVE_MKDIRAT",    "mkdir")
    _add("HAVE_MKFIFOAT",   "mkfifo")
    _add("HAVE_MKNODAT",    "mknod")
    _add("HAVE_OPENAT",     "open")
    _add("HAVE_READLINKAT", "readlink")
    _add("HAVE_RENAMEAT",   "rename")
    _add("HAVE_SYMLINKAT",  "symlink")
    _add("HAVE_UNLINKAT",   "unlink")
    _add("HAVE_UNLINKAT",   "rmdir")
    _add("HAVE_UTIMENSAT",  "utime")
    supports_dir_fd = _set

    _set = set()
    _add("HAVE_FACCESSAT",  "access")
    supports_effective_ids = _set

    _set = set()
    _add("HAVE_FCHDIR",     "chdir")
    _add("HAVE_FCHMOD",     "chmod")
    _add("MS_WINDOWS",      "chmod")
    _add("HAVE_FCHOWN",     "chown")
    _add("HAVE_FDOPENDIR",  "listdir")
    _add("HAVE_FDOPENDIR",  "scandir")
    _add("HAVE_FEXECVE",    "execve")
    _set.add(stat)
    _add("HAVE_FTRUNCATE",  "truncate")
    _add("HAVE_FUTIMENS",   "utime")
    _add("HAVE_FUTIMES",    "utime")
    _add("HAVE_FPATHCONF",  "pathconf")
    if _exists("statvfs") and _exists("fstatvfs"):
        _add("HAVE_FSTATVFS", "statvfs")
    supports_fd = _set

    _set = set()
    _add("HAVE_FACCESSAT",  "access")
    _add("HAVE_FCHOWNAT",   "chown")
    _add("HAVE_FSTATAT",    "stat")
    _add("HAVE_LCHFLAGS",   "chflags")
    _add("HAVE_LCHMOD",     "chmod")
    if _exists("lchown"):
        _add("HAVE_LCHOWN", "chown")
    _add("HAVE_LINKAT",     "link")
    _add("HAVE_LUTIMES",    "utime")
    _add("HAVE_LSTAT",      "stat")
    _add("HAVE_FSTATAT",    "stat")
    _add("HAVE_UTIMENSAT",  "utime")
    _add("MS_WINDOWS",      "stat")
    supports_follow_symlinks = _set


def _exists(name):
    return name in globals()


_build_supports()
del _build_supports


def _build_all():
    import posix

    # A lista do `os.py` do Debian, na ordem em que ele a monta (as partes de cada bloco vêm de onde o bloco
    # as define): os nomes do `posix`, o que o `os` acrescenta e as funções condicionais.
    exported = ["altsep", "curdir", "pardir", "sep", "pathsep", "linesep",
                "defpath", "name", "path", "devnull", "SEEK_SET", "SEEK_CUR",
                "SEEK_END", "fsencode", "fsdecode", "get_exec_path", "fdopen",
                "extsep"]
    exported.append('_exit')
    exported.extend(_get_exports_list(posix))
    exported.extend(["makedirs", "removedirs", "renames"])
    exported.append("walk")
    if {open, stat} <= supports_dir_fd and {scandir, stat} <= supports_fd:
        exported.append("fwalk")
    exported.extend(["execl", "execle", "execlp", "execlpe", "execvp", "execvpe"])
    exported.extend(("getenv", "supports_bytes_environ"))
    exported.extend(("environb", "getenvb"))
    exported.extend(["P_WAIT", "P_NOWAIT", "P_NOWAITO"])
    exported.extend(["spawnv", "spawnve", "spawnvp", "spawnvpe"])
    exported.extend(["spawnl", "spawnle"])
    exported.extend(["spawnlp", "spawnlpe"])
    exported.append("popen")
    return exported


__all__ = _build_all()
del _build_all
