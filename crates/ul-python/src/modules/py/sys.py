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
_getframe = _sys._getframe
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
if _sys.script_dir:
    path[0] = _sys.script_dir


class _Modules:
    """`sys.modules`: visão dos módulos carregados (leitura sempre atual; escritas ficam à parte)."""

    def __init__(self):
        self._extra = {}

    def _all(self):
        d = _sys._modules()
        d.update(self._extra)
        return d

    def __getitem__(self, key):
        return self._all()[key]

    def __setitem__(self, key, value):
        if isinstance(key, str) and _sys._set_module(key, value):
            self._extra.pop(key, None)
        else:
            self._extra[key] = value

    def __delitem__(self, key):
        if key in self._extra:
            del self._extra[key]
        elif not (isinstance(key, str) and _sys._pop_module(key)):
            raise KeyError(key)

    def __contains__(self, key):
        return key in self._all()

    def __iter__(self):
        return iter(self._all())

    def __len__(self):
        return len(self._all())

    def get(self, key, default=None):
        return self._all().get(key, default)

    def keys(self):
        return self._all().keys()

    def values(self):
        return self._all().values()

    def items(self):
        return self._all().items()

    def pop(self, key, *default):
        if key in self._extra:
            return self._extra.pop(key)
        d = self._all()
        if key in d:
            value = d[key]
            self.__delitem__(key)
            return value
        if default:
            return default[0]
        raise KeyError(key)

    def setdefault(self, key, default=None):
        d = self._all()
        if key in d:
            return d[key]
        self[key] = default
        return default

    def update(self, *args, **kwargs):
        for key, value in dict(*args, **kwargs).items():
            self[key] = value

    def copy(self):
        return dict(self._all())

    def __repr__(self):
        return repr(self._all())


modules = _Modules()


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


class _HashInfo:
    width = 64
    modulus = 2305843009213693951
    inf = 314159
    nan = 0
    imag = 1000003
    algorithm = 'siphash13'
    hash_bits = 64
    seed_bits = 128
    cutoff = 0


hash_info = _HashInfo()


class _FloatInfo:
    max = 1.7976931348623157e+308
    max_exp = 1024
    max_10_exp = 308
    min = 2.2250738585072014e-308
    min_exp = -1021
    min_10_exp = -307
    dig = 15
    mant_dig = 53
    epsilon = 2.220446049250313e-16
    radix = 2
    rounds = 1


float_info = _FloatInfo()


class _IntInfo:
    bits_per_digit = 30
    sizeof_digit = 4
    default_max_str_digits = 4300
    str_digits_check_threshold = 640


int_info = _IntInfo()


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
