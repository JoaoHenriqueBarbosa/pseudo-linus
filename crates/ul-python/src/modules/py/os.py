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


def listdir(path=None):
    if path is None:
        path = '.'
    return _os.listdir(_path_or_fd(path))


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
    _os.chmod(fspath(path), mode, dir_fd=dir_fd)


def chown(path, uid, gid, *, dir_fd=None, follow_symlinks=True):
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


def utime(path, times=None, *, ns=None, dir_fd=None, follow_symlinks=True):
    p = fspath(path)
    if ns is not None:
        _os.utime(p, ns[0] / 1e9, ns[1] / 1e9, dir_fd=dir_fd)
    elif times is None:
        _os.utime(p, None, None, dir_fd=dir_fd)
    else:
        _os.utime(p, times[0], times[1], dir_fd=dir_fd)


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


def open(path, flags, mode=0o777, *, dir_fd=None):
    return _os.open(fspath(path), flags, mode, dir_fd=dir_fd)


def close(fd):
    _os.close(fd)


def read(fd, n):
    return _os.read(fd, n)


def pipe():
    """Create a pipe.

Returns a tuple of two file descriptors:
  (read_fd, write_fd)"""
    return tuple(_os.pipe())


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
    if isinstance(name, str):
        try:
            code = pathconf_names[name]
        except KeyError:
            raise ValueError('unrecognized configuration name') from None
    elif isinstance(name, int):
        code = name
    else:
        raise TypeError('configuration names must be strings or integers')
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
