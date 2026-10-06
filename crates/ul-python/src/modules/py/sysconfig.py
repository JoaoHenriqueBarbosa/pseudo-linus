"""sysconfig enxuto: as variáveis e caminhos do Python 3.13 do Debian 13."""

import os
import sys

__all__ = [
    'get_config_var', 'get_config_vars', 'get_default_scheme', 'get_path', 'get_path_names', 'get_paths',
    'get_platform', 'get_python_version', 'get_scheme_names', 'is_python_build', 'get_preferred_scheme']

_VARS = {
    'TZPATH': '/usr/share/zoneinfo:/usr/lib/zoneinfo:/usr/share/lib/zoneinfo:/etc/zoneinfo',
    'EXT_SUFFIX': '.cpython-313-x86_64-linux-gnu.so',
    'SOABI': 'cpython-313-x86_64-linux-gnu',
    'MULTIARCH': 'x86_64-linux-gnu',
    'CC': 'x86_64-linux-gnu-gcc',
    'prefix': '/usr',
    'exec_prefix': '/usr',
    'base': '/usr',
    'platbase': '/usr',
    'LIBDIR': '/usr/lib/x86_64-linux-gnu',
    'Py_GIL_DISABLED': 0,
    'VERSION': '3.13',
    'py_version_nodot': '313',
    'py_version_short': '3.13',
    'SIZEOF_VOID_P': 8,
    'WITH_DOC_STRINGS': 1,
    'abiflags': '',
    'BINDIR': '/usr/bin',
    'LIBDEST': '/usr/lib/python3.13',
    'INCLUDEPY': '/usr/include/python3.13',
}

_PATHS = {
    'stdlib': '/usr/lib/python3.13',
    'platstdlib': '/usr/lib/python3.13',
    'purelib': '/usr/local/lib/python3.13/dist-packages',
    'platlib': '/usr/local/lib/python3.13/dist-packages',
    'include': '/usr/include/python3.13',
    'platinclude': '/usr/include/python3.13',
    'scripts': '/usr/local/bin',
    'data': '/usr/local',
}

_SCHEMES = ('deb_system', 'nt', 'nt_user', 'nt_venv', 'osx_framework_user', 'posix_home', 'posix_local',
            'posix_prefix', 'posix_user', 'posix_venv', 'venv')


def get_config_vars(*args):
    if args:
        return [_VARS.get(a) for a in args]
    return dict(_VARS)


def get_config_var(name):
    return _VARS.get(name)


def get_python_version():
    return '3.13'


def get_platform():
    return 'linux-x86_64'


def get_default_scheme():
    return 'posix_local'


def get_preferred_scheme(key):
    return 'posix_local'


def get_scheme_names():
    return _SCHEMES


def get_path_names():
    return tuple(_PATHS)


def get_paths(scheme=None, vars=None, expand=True):
    return dict(_PATHS)


def get_path(name, scheme=None, vars=None, expand=True):
    return _PATHS[name]


def is_python_build(check_home=None):
    return False
