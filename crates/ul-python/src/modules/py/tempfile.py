"""tempfile: arquivos e diretórios temporários."""

import os
import io
import shutil as _shutil

__all__ = ['NamedTemporaryFile', 'TemporaryFile', 'SpooledTemporaryFile', 'TemporaryDirectory',
           'mkstemp', 'mkdtemp', 'mktemp', 'gettempdir', 'gettempdirb', 'gettempprefix', 'tempdir',
           'template']

template = 'tmp'
tempdir = None
TMP_MAX = 10000
_CHARS = 'abcdefghijklmnopqrstuvwxyz0123456789_'


def gettempprefix():
    return template


def _candidate_dirs():
    for name in ('TMPDIR', 'TEMP', 'TMP'):
        d = os.environ.get(name)
        if d:
            yield d
    yield '/tmp'
    yield '/var/tmp'
    yield '/usr/tmp'
    yield os.getcwd()


def gettempdir():
    global tempdir
    if tempdir is None:
        for d in _candidate_dirs():
            if os.path.isdir(d) and os.access(d, os.W_OK):
                tempdir = os.path.abspath(d)
                break
        else:
            raise FileNotFoundError(2, 'No usable temporary directory found')
    return tempdir


def gettempdirb():
    return os.fsencode(gettempdir())


def _random_name():
    # Oito caracteres, como o CPython, tirados do gerador do sistema.
    raw = os.urandom(8)
    return ''.join(_CHARS[b % len(_CHARS)] for b in raw)


def _sanitize(prefix, suffix, dir):
    if dir is None:
        dir = gettempdir()
    if prefix is None:
        prefix = template
    if suffix is None:
        suffix = ''
    return os.fspath(prefix), os.fspath(suffix), os.fspath(dir)


def mkstemp(suffix=None, prefix=None, dir=None, text=False):
    prefix, suffix, dir = _sanitize(prefix, suffix, dir)
    for _ in range(TMP_MAX):
        name = os.path.join(dir, prefix + _random_name() + suffix)
        try:
            fd = os.open(name, os.O_RDWR | os.O_CREAT | os.O_EXCL, 0o600)
        except FileExistsError:
            continue
        return fd, os.path.abspath(name)
    raise FileExistsError(17, 'No usable temporary file name found')


def mkdtemp(suffix=None, prefix=None, dir=None):
    prefix, suffix, dir = _sanitize(prefix, suffix, dir)
    for _ in range(TMP_MAX):
        name = os.path.join(dir, prefix + _random_name() + suffix)
        try:
            os.mkdir(name, 0o700)
        except FileExistsError:
            continue
        return os.path.abspath(name)
    raise FileExistsError(17, 'No usable temporary directory name found')


def mktemp(suffix='', prefix=template, dir=None):
    if dir is None:
        dir = gettempdir()
    for _ in range(TMP_MAX):
        name = os.path.join(dir, prefix + _random_name() + suffix)
        if not os.path.exists(name):
            return name
    raise FileExistsError(17, 'No usable temporary filename found')


class _TemporaryFileWrapper:
    def __init__(self, file, name, delete=True):
        self.file = file
        self.name = name
        self.delete = delete
        self._closed = False

    def __getattr__(self, name):
        return getattr(self.file, name)

    def __enter__(self):
        self.file.__enter__()
        return self

    def close(self):
        if not self._closed:
            self._closed = True
            try:
                self.file.close()
            finally:
                if self.delete:
                    try:
                        os.unlink(self.name)
                    except OSError:
                        pass

    def __exit__(self, exc, value, tb):
        self.close()
        return False

    def __del__(self):
        self.close()

    def __iter__(self):
        return iter(self.file)


def NamedTemporaryFile(mode='w+b', buffering=-1, encoding=None, newline=None, suffix=None,
                       prefix=None, dir=None, delete=True, *, errors=None, delete_on_close=True):
    prefix, suffix, dir = _sanitize(prefix, suffix, dir)
    fd, name = mkstemp(suffix, prefix, dir)
    os.close(fd)
    f = _open_temp(name, mode, buffering, encoding, newline)
    return _TemporaryFileWrapper(f, name, delete)


def _open_temp(name, mode, buffering, encoding, newline):
    # O arquivo já existe (mkstemp), então `w` apenas trunca e `w+` abre para leitura e escrita.
    return open(name, mode, buffering, encoding, newline)


def TemporaryFile(mode='w+b', buffering=-1, encoding=None, newline=None, suffix=None, prefix=None,
                  dir=None, *, errors=None):
    prefix, suffix, dir = _sanitize(prefix, suffix, dir)
    fd, name = mkstemp(suffix, prefix, dir)
    os.close(fd)
    f = _open_temp(name, mode, buffering, encoding, newline)
    os.unlink(name)
    return f


class SpooledTemporaryFile:
    def __init__(self, max_size=0, mode='w+b', buffering=-1, encoding=None, newline=None,
                 suffix=None, prefix=None, dir=None):
        self._max_size = max_size
        self._mode = mode
        self._file = io.BytesIO() if 'b' in mode else io.StringIO()

    def __getattr__(self, name):
        return getattr(self._file, name)

    def __enter__(self):
        return self

    def __exit__(self, exc, value, tb):
        self._file.close()
        return False

    def __iter__(self):
        return iter(self._file)


class TemporaryDirectory:
    def __init__(self, suffix=None, prefix=None, dir=None, ignore_cleanup_errors=False, *, delete=True):
        self.name = mkdtemp(suffix, prefix, dir)
        self._ignore = ignore_cleanup_errors
        self._delete = delete

    def __repr__(self):
        return '<{} {!r}>'.format(self.__class__.__name__, self.name)

    def __enter__(self):
        return self.name

    def __exit__(self, exc, value, tb):
        if self._delete:
            self.cleanup()
        return False

    def cleanup(self):
        if os.path.exists(self.name):
            _shutil.rmtree(self.name, ignore_errors=self._ignore)
