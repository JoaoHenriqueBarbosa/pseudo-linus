"""pathlib: caminhos orientados a objeto (somente POSIX)."""

import os
import posixpath
import fnmatch
import re
import stat as _stat

__all__ = ['PurePath', 'PurePosixPath', 'PosixPath', 'Path']


def _parse(parts):
    """Junta os pedaços e devolve (raiz, componentes) já normalizados."""
    joined = ''
    for p in parts:
        p = os.fspath(p)
        if not isinstance(p, str):
            raise TypeError("argument should be a str or an os.PathLike object where __fspath__ returns a str, not %r" % type(p).__name__)
        if p.startswith('/'):
            joined = p
        elif joined and not joined.endswith('/'):
            joined = joined + '/' + p
        else:
            joined = joined + p
    root = ''
    if joined.startswith('/'):
        # duas barras iniciais são preservadas, três ou mais viram uma
        root = '//' if joined.startswith('//') and not joined.startswith('///') else '/'
    comps = [c for c in joined.split('/') if c and c != '.']
    return root, comps


class PurePath:
    __slots__ = ('_root', '_comps', '_str')

    def __new__(cls, *args):
        if cls is PurePath:
            cls = PurePosixPath
        return cls._from_parts(args)

    @classmethod
    def _from_parts(cls, args):
        self = object.__new__(cls)
        root, comps = _parse(args)
        self._root = root
        self._comps = tuple(comps)
        self._str = None
        return self

    @classmethod
    def _from_root_comps(cls, root, comps):
        self = object.__new__(cls)
        self._root = root
        self._comps = tuple(comps)
        self._str = None
        return self

    def __reduce__(self):
        return (type(self), tuple(self.parts))

    def __fspath__(self):
        return str(self)

    def __str__(self):
        s = self._str
        if s is None:
            s = self._root + '/'.join(self._comps)
            if not s:
                s = '.'
            self._str = s
        return s

    def as_posix(self):
        return str(self)

    def __bytes__(self):
        return os.fsencode(self)

    def __repr__(self):
        return '{}({!r})'.format(type(self).__name__, self.as_posix())

    def as_uri(self):
        if not self.is_absolute():
            raise ValueError("relative path can't be expressed as a file URI")
        from urllib.parse import quote_from_bytes
        return 'file://' + quote_from_bytes(os.fsencode(self))

    def _key(self):
        return (self._root, self._comps)

    def __eq__(self, other):
        if not isinstance(other, PurePath):
            return NotImplemented
        return self._key() == other._key()

    def __hash__(self):
        return hash(self._key())

    def __lt__(self, other):
        if not isinstance(other, PurePath):
            return NotImplemented
        return self._key() < other._key()

    def __le__(self, other):
        if not isinstance(other, PurePath):
            return NotImplemented
        return self._key() <= other._key()

    def __gt__(self, other):
        if not isinstance(other, PurePath):
            return NotImplemented
        return self._key() > other._key()

    def __ge__(self, other):
        if not isinstance(other, PurePath):
            return NotImplemented
        return self._key() >= other._key()

    drive = property(lambda self: '')
    root = property(lambda self: self._root)

    @property
    def anchor(self):
        return self._root

    @property
    def parts(self):
        if self._root:
            return (self._root,) + self._comps
        return self._comps

    @property
    def name(self):
        return self._comps[-1] if self._comps else ''

    @property
    def suffix(self):
        name = self.name
        i = name.rfind('.')
        if 0 < i < len(name) - 1:
            return name[i:]
        return ''

    @property
    def suffixes(self):
        name = self.name
        if name.endswith('.'):
            return []
        name = name.lstrip('.')
        return ['.' + s for s in name.split('.')[1:]]

    @property
    def stem(self):
        name = self.name
        i = name.rfind('.')
        if 0 < i < len(name) - 1:
            return name[:i]
        return name

    @property
    def parent(self):
        if not self._comps:
            return self
        return self._from_root_comps(self._root, self._comps[:-1])

    @property
    def parents(self):
        return _PathParents(self)

    def is_absolute(self):
        return bool(self._root)

    def is_reserved(self):
        return False

    def joinpath(self, *args):
        return self._from_parts((self,) + args)

    def __truediv__(self, key):
        try:
            return self._from_parts((self, key))
        except TypeError:
            return NotImplemented

    def __rtruediv__(self, key):
        try:
            return self._from_parts((key, self))
        except TypeError:
            return NotImplemented

    def with_name(self, name):
        if not self.name:
            raise ValueError('%r has an empty name' % (self,))
        if not name or '/' in name or name == '.':
            raise ValueError('Invalid name %r' % (name,))
        return self._from_root_comps(self._root, self._comps[:-1] + (name,))

    def with_stem(self, stem):
        return self.with_name(stem + self.suffix)

    def with_suffix(self, suffix):
        if '/' in suffix or (suffix and not suffix.startswith('.')) or suffix == '.':
            raise ValueError('Invalid suffix %r' % (suffix,))
        name = self.name
        if not name:
            raise ValueError('%r has an empty name' % (self,))
        old = self.suffix
        if not old:
            name = name + suffix
        else:
            name = name[:-len(old)] + suffix
        return self._from_root_comps(self._root, self._comps[:-1] + (name,))

    def relative_to(self, *other, walk_up=False):
        if not other:
            raise TypeError('need at least one argument')
        o = self._from_parts(other)
        if o._root != self._root:
            raise ValueError('%r is not in the subpath of %r' % (str(self), str(o)))
        i = 0
        while i < len(o._comps) and i < len(self._comps) and o._comps[i] == self._comps[i]:
            i += 1
        if i < len(o._comps):
            if not walk_up or '..' in o._comps[i:]:
                raise ValueError('%r is not in the subpath of %r' % (str(self), str(o)))
            comps = ('..',) * (len(o._comps) - i) + self._comps[i:]
        else:
            comps = self._comps[i:]
        return self._from_root_comps('', comps)

    def is_relative_to(self, *other):
        try:
            self.relative_to(*other)
            return True
        except ValueError:
            return False

    def match(self, pattern, *, case_sensitive=None):
        pat = self._from_parts((pattern,))
        if not pat._comps and not pat._root:
            raise ValueError('empty pattern')
        pcs = pat._comps
        if pat._root:
            if self._root != pat._root or len(self._comps) != len(pcs):
                return False
        elif len(pcs) > len(self._comps):
            return False
        for c, p in zip(reversed(self._comps), reversed(pcs)):
            if not fnmatch.fnmatchcase(c, p):
                return False
        return True

    def full_match(self, pattern, *, case_sensitive=None):
        pat = self._from_parts((pattern,))
        regex = _glob_to_regex(pat)
        return re.match(regex, str(self)) is not None


def _glob_to_regex(pat):
    out = []
    comps = list(pat._comps)
    if pat._root:
        out.append('/')
    first = True
    for c in comps:
        if not first:
            out.append('/')
        first = False
        if c == '**':
            out.append('(?:[^/]+/)*[^/]*')
        else:
            out.append(_seg(c))
    return '^' + ''.join(out) + '$'


def _seg(c):
    out = []
    i = 0
    while i < len(c):
        ch = c[i]
        if ch == '*':
            out.append('[^/]*')
        elif ch == '?':
            out.append('[^/]')
        elif ch == '[':
            j = c.find(']', i + 2)
            if j == -1:
                out.append(re.escape(ch))
            else:
                body = c[i + 1:j]
                if body.startswith('!'):
                    body = '^' + body[1:]
                out.append('[' + body + ']')
                i = j
        else:
            out.append(re.escape(ch))
        i += 1
    return ''.join(out)


class _PathParents:
    def __init__(self, path):
        self._path = path
        self._n = len(path._comps)

    def __len__(self):
        return self._n

    def __getitem__(self, idx):
        if isinstance(idx, slice):
            return tuple(self[i] for i in range(*idx.indices(len(self))))
        if idx >= self._n or idx < -self._n:
            raise IndexError(idx)
        if idx < 0:
            idx += self._n
        p = self._path
        return p._from_root_comps(p._root, p._comps[:self._n - idx - 1])

    def __iter__(self):
        for i in range(self._n):
            yield self[i]

    def __repr__(self):
        return '<{}.parents>'.format(type(self._path).__name__)


class PurePosixPath(PurePath):
    __slots__ = ()


class Path(PurePath):
    __slots__ = ()

    def __new__(cls, *args, **kwargs):
        if cls is Path:
            cls = PosixPath
        return cls._from_parts(args)

    @classmethod
    def cwd(cls):
        return cls(os.getcwd())

    @classmethod
    def home(cls):
        return cls(os.path.expanduser('~'))

    def stat(self, *, follow_symlinks=True):
        return os.stat(self) if follow_symlinks else os.lstat(self)

    def lstat(self):
        return os.lstat(self)

    def exists(self, *, follow_symlinks=True):
        try:
            self.stat(follow_symlinks=follow_symlinks)
        except (OSError, ValueError):
            return False
        return True

    def _check(self, test, follow_symlinks=True):
        try:
            return test(self.stat(follow_symlinks=follow_symlinks).st_mode)
        except (OSError, ValueError):
            return False

    def is_dir(self, *, follow_symlinks=True):
        return self._check(_stat.S_ISDIR, follow_symlinks)

    def is_file(self, *, follow_symlinks=True):
        return self._check(_stat.S_ISREG, follow_symlinks)

    def is_symlink(self):
        return self._check(_stat.S_ISLNK, False)

    def is_mount(self):
        return os.path.ismount(self)

    def is_socket(self):
        return self._check(_stat.S_ISSOCK)

    def is_fifo(self):
        return self._check(_stat.S_ISFIFO)

    def is_block_device(self):
        return self._check(_stat.S_ISBLK)

    def is_char_device(self):
        return self._check(_stat.S_ISCHR)

    def samefile(self, other):
        return os.path.samefile(self, other)

    def open(self, mode='r', buffering=-1, encoding=None, errors=None, newline=None):
        return open(self, mode, buffering, encoding, errors, newline)

    def read_text(self, encoding=None, errors=None):
        with self.open('r', encoding=encoding, errors=errors) as f:
            return f.read()

    def read_bytes(self):
        with self.open('rb') as f:
            return f.read()

    def write_text(self, data, encoding=None, errors=None, newline=None):
        if not isinstance(data, str):
            raise TypeError('data must be str, not %s' % type(data).__name__)
        with self.open('w', encoding=encoding, errors=errors, newline=newline) as f:
            return f.write(data)

    def write_bytes(self, data):
        view = memoryview(data) if not isinstance(data, (bytes, bytearray)) else data
        with self.open('wb') as f:
            return f.write(view)

    def iterdir(self):
        for name in os.listdir(self):
            yield self._make_child(name)

    def _make_child(self, name):
        return self._from_root_comps(self._root, self._comps + (name,))

    def glob(self, pattern, *, case_sensitive=None, recurse_symlinks=False):
        pat = self._from_parts((pattern,))
        if pat._root:
            raise NotImplementedError('Non-relative patterns are unsupported')
        if not pat._comps:
            raise ValueError('Unacceptable pattern: {!r}'.format(pattern))
        trailing_dir = pattern.endswith('/')
        for p in self._glob(self, list(pat._comps)):
            if trailing_dir and not p.is_dir():
                continue
            yield p

    def _glob(self, base, comps):
        if not comps:
            yield base
            return
        head, rest = comps[0], comps[1:]
        if head == '**':
            # zero ou mais diretórios
            yield from self._glob(base, rest)
            for child in self._dirs(base):
                yield from self._glob(child, comps)
            return
        if not any(c in head for c in '*?['):
            child = base._make_child(head)
            if rest:
                if child.is_dir():
                    yield from self._glob(child, rest)
            elif child.exists() or child.is_symlink():
                yield child
            return
        try:
            names = sorted(os.listdir(base))
        except OSError:
            return
        for name in names:
            if fnmatch.fnmatchcase(name, head):
                child = base._make_child(name)
                if rest:
                    if child.is_dir():
                        yield from self._glob(child, rest)
                else:
                    yield child

    def _dirs(self, base):
        try:
            names = sorted(os.listdir(base))
        except OSError:
            return
        for name in names:
            child = base._make_child(name)
            if child.is_dir() and not child.is_symlink():
                yield child

    def rglob(self, pattern, *, case_sensitive=None, recurse_symlinks=False):
        return self.glob('**/' + pattern, case_sensitive=case_sensitive)

    def walk(self, top_down=True, on_error=None, follow_symlinks=False):
        for dirpath, dirnames, filenames in os.walk(self, topdown=top_down, onerror=on_error,
                                                    followlinks=follow_symlinks):
            yield self._from_parts((dirpath,)), dirnames, filenames

    def absolute(self):
        if self.is_absolute():
            return self
        return self._from_parts((os.getcwd(), self))

    def resolve(self, strict=False):
        p = os.path.realpath(self, strict=strict)
        return self._from_parts((p,))

    def expanduser(self):
        if self._comps and self._comps[0].startswith('~') and not self._root:
            expanded = os.path.expanduser(self._comps[0])
            if expanded.startswith('~'):
                raise RuntimeError("Could not determine home directory.")
            return self._from_parts((expanded,) + self._comps[1:])
        return self

    def readlink(self):
        return self._from_parts((os.readlink(self),))

    def touch(self, mode=0o666, exist_ok=True):
        if self.exists():
            if not exist_ok:
                raise FileExistsError(17, 'File exists', str(self))
            os.utime(self, None)
            return
        fd = os.open(self, os.O_CREAT | os.O_WRONLY | os.O_EXCL if not exist_ok else os.O_CREAT | os.O_WRONLY, mode)
        os.close(fd)

    def mkdir(self, mode=0o777, parents=False, exist_ok=False):
        try:
            os.mkdir(self, mode)
        except FileNotFoundError:
            if not parents or self.parent == self:
                raise
            self.parent.mkdir(parents=True, exist_ok=True)
            self.mkdir(mode, parents=False, exist_ok=exist_ok)
        except OSError:
            if not exist_ok or not self.is_dir():
                raise

    def chmod(self, mode, *, follow_symlinks=True):
        os.chmod(self, mode)

    def unlink(self, missing_ok=False):
        try:
            os.unlink(self)
        except FileNotFoundError:
            if not missing_ok:
                raise

    def rmdir(self):
        os.rmdir(self)

    def rename(self, target):
        os.rename(self, target)
        return self.__class__(target)

    def replace(self, target):
        os.replace(self, target)
        return self.__class__(target)

    def symlink_to(self, target, target_is_directory=False):
        os.symlink(target, self)

    def owner(self):
        raise NotImplementedError('Path.owner() is unsupported on this system')

    def group(self):
        raise NotImplementedError('Path.group() is unsupported on this system')

    def __enter__(self):
        return self

    def __exit__(self, t, v, tb):
        pass


class PosixPath(Path, PurePosixPath):
    __slots__ = ()
