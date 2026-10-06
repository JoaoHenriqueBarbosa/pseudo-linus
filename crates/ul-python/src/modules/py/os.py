"""Módulo os do sandbox: interface POSIX sobre as chamadas `_os` (VFS do pseudo-linus)."""

import _os
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
SEEK_SET = 0
SEEK_CUR = 1
SEEK_END = 2

error = OSError


class PathLike:
    """Classe base de objetos que representam caminhos (`__fspath__`)."""

    def __fspath__(self):
        raise NotImplementedError

    @classmethod
    def __subclasshook__(cls, subclass):
        return hasattr(subclass, '__fspath__')


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
        return filename.encode('utf-8')
    return filename


def fsdecode(filename):
    filename = fspath(filename)
    if isinstance(filename, bytes):
        return filename.decode('utf-8')
    return filename


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


def access(p, mode):
    return _os.access(fspath(p), mode)


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
    for name, kind in entries:
        is_dir = kind == 'd' or (kind == 'l' and path.isdir(path.join(top, name)))
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


class _Environ:
    """`os.environ`: leitura e escrita refletidas no ambiente do pseudo-processo."""

    def __init__(self):
        self._data = dict(_os.environ())

    def __getitem__(self, key):
        try:
            return self._data[key]
        except KeyError:
            raise KeyError(key) from None

    def __setitem__(self, key, value):
        if not isinstance(key, str) or not isinstance(value, str):
            raise TypeError('str expected, not ' + type(value if isinstance(key, str) else key).__name__)
        _os.putenv(key, value)
        self._data[key] = value

    def __delitem__(self, key):
        if key not in self._data:
            raise KeyError(key)
        _os.unsetenv(key)
        del self._data[key]

    def __contains__(self, key):
        return key in self._data

    def __iter__(self):
        return iter(self._data)

    def __len__(self):
        return len(self._data)

    def get(self, key, default=None):
        return self._data.get(key, default)

    def keys(self):
        return self._data.keys()

    def values(self):
        return self._data.values()

    def items(self):
        return self._data.items()

    def setdefault(self, key, default=''):
        if key not in self._data:
            self[key] = default
        return self._data[key]

    def pop(self, key, *default):
        if key in self._data:
            value = self._data[key]
            del self[key]
            return value
        if default:
            return default[0]
        raise KeyError(key)

    def update(self, other=(), **kwargs):
        for k, v in dict(other, **kwargs).items():
            self[k] = v

    def copy(self):
        return dict(self._data)

    def __repr__(self):
        return 'environ(' + repr(self._data) + ')'


environ = _Environ()


def getenv(key, default=None):
    return environ.get(key, default)


def putenv(key, value):
    environ[key] = value


def unsetenv(key):
    if key in environ:
        del environ[key]


def getpid():
    return _os.getpid()


def cpu_count():
    return 1


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


def uname():
    return ('Linux', 'localhost', '6.12.0', '#1 SMP', 'x86_64')
