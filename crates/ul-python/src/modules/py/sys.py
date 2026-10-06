"""Módulo sys do sandbox. Os valores constantes batem com o python3 do Debian 13."""

import _sys

argv = _sys.argv
orig_argv = ['python3'] + _sys.argv
stdin = _sys.stdin
stdout = _sys.stdout
stderr = _sys.stderr
__stdin__ = _sys.stdin
__stdout__ = _sys.stdout
__stderr__ = _sys.stderr
exit = _sys.exit
exc_info = _sys.exc_info
getrecursionlimit = _sys.getrecursionlimit
setrecursionlimit = _sys.setrecursionlimit

version = '3.13.5 (main, Aug 10 2026, 12:06:59) [GCC 14.2.0]'
hexversion = 51185136
api_version = 1013
platform = 'linux'
executable = '/usr/bin/python3'
prefix = '/usr'
exec_prefix = '/usr'
base_prefix = '/usr'
base_exec_prefix = '/usr'
byteorder = 'little'
maxsize = 9223372036854775807
maxunicode = 1114111
path = ['', '/usr/lib/python313.zip', '/usr/lib/python3.13', '/usr/lib/python3.13/lib-dynload',
        '/usr/local/lib/python3.13/dist-packages', '/usr/lib/python3/dist-packages']
path_hooks = []
meta_path = []
warnoptions = []
dont_write_bytecode = True
platlibdir = 'lib'
pycache_prefix = None


class _VersionInfo:
    """`sys.version_info`: tupla nomeada (major, minor, micro, releaselevel, serial)."""

    _fields = ('major', 'minor', 'micro', 'releaselevel', 'serial')

    def __init__(self, values):
        self._values = tuple(values)
        self.major, self.minor, self.micro, self.releaselevel, self.serial = self._values

    def __getitem__(self, i):
        return self._values[i]

    def __iter__(self):
        return iter(self._values)

    def __len__(self):
        return len(self._values)

    def __eq__(self, other):
        return self._values == tuple(other)

    def __lt__(self, other):
        return self._values < tuple(other)

    def __le__(self, other):
        return self._values <= tuple(other)

    def __gt__(self, other):
        return self._values > tuple(other)

    def __ge__(self, other):
        return self._values >= tuple(other)

    def __hash__(self):
        return hash(self._values)

    def __repr__(self):
        return 'sys.version_info(major=%d, minor=%d, micro=%d, releaselevel=%r, serial=%d)' % self._values


version_info = _VersionInfo((3, 13, 5, 'final', 0))


class _Flags:
    optimize = 0
    debug = 0
    verbose = 0
    quiet = 0
    interactive = 0
    inspect = 0
    dont_write_bytecode = 0
    no_site = 0
    ignore_environment = 0
    utf8_mode = 1


flags = _Flags()


class _Implementation:
    name = 'cpython'
    cache_tag = 'cpython-313'
    version = version_info
    hexversion = hexversion


implementation = _Implementation()


def getdefaultencoding():
    return 'utf-8'


def getfilesystemencoding():
    return 'utf-8'


def getfilesystemencodeerrors():
    return 'surrogateescape'


def intern(s):
    return s


def is_finalizing():
    return False


def getswitchinterval():
    return 0.005


builtin_module_names = ('_abc', '_ast', '_codecs', '_io', '_os', '_sys', 'builtins', 'itertools', 'math', 'sys')
