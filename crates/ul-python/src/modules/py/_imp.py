import sys
import _thread

_FROZEN = (
    '__hello__', '__hello_alias__', '__hello_only__', '__phello__', '__phello__.__init__', '__phello__.ham',
    '__phello__.ham.__init__', '__phello__.ham.eggs', '__phello__.spam', '__phello_alias__',
    '__phello_alias__.spam', '_collections_abc', '_frozen_importlib', '_frozen_importlib_external',
    '_sitebuiltins', 'abc', 'codecs', 'genericpath', 'importlib.machinery', 'importlib.util', 'io', 'ntpath',
    'os', 'os.path', 'posixpath', 'runpy', 'site', 'stat', 'zipimport',
)
_FROZEN_PACKAGES = ('__phello__', '__phello__.ham', '__phello_alias__')

check_hash_based_pycs = 'default'

# O lock de import é reentrante e por thread, como o `_PyImport_AcquireLock`.
_owner = None
_count = 0


def acquire_lock():
    global _owner, _count
    me = _thread.get_ident()
    if _owner != me:
        _owner = me
        _count = 0
    _count += 1


def release_lock():
    global _owner, _count
    if _owner != _thread.get_ident() or _count == 0:
        raise RuntimeError('not holding the import lock')
    _count -= 1
    if _count == 0:
        _owner = None


def lock_held():
    return _owner is not None


def extension_suffixes():
    return ['.cpython-313-x86_64-linux-gnu.so', '.abi3.so', '.abi3-x86_64-linux-gnu.so', '.so']


def is_builtin(name):
    if name not in sys.builtin_module_names:
        return 0
    if name in ('sys', 'builtins'):
        return -1
    return 1


def is_frozen(name):
    return name in _FROZEN


def is_frozen_package(name):
    if name not in _FROZEN:
        raise ImportError(f'No such frozen object named {name!r}', name=name)
    return name in _FROZEN_PACKAGES


def find_frozen(name, *, withdata=False):
    if name not in _FROZEN:
        return None
    origname = name[:-len('.__init__')] if name.endswith('.__init__') else name
    return (None, name in _FROZEN_PACKAGES, origname)


def _frozen_module_names():
    return _FROZEN


def get_frozen_object(name, data=None):
    if name not in _FROZEN:
        raise ImportError(f'No such frozen object named {name!r}', name=name)
    raise ImportError(f'cannot load frozen object {name!r}', name=name)


def init_frozen(name):
    if name not in _FROZEN:
        return None
    return sys.modules.get(name) or __import__(name)


def create_builtin(spec):
    return __import__(spec.name)


def exec_builtin(mod):
    return 0


def create_dynamic(spec, file=None):
    raise ImportError(f'cannot load extension module {spec.name!r}', name=spec.name)


def exec_dynamic(mod):
    return 0


def source_hash(key, source):
    raise NotImplementedError


def _fix_co_filename(code, path):
    pass


def _override_frozen_modules_for_tests(override):
    pass


def _override_multi_interp_extensions_check(override):
    return -1
