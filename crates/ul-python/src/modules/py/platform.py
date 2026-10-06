"""Identificação da plataforma, sobre `os.uname()` do sandbox."""

import os
import sys
from collections import namedtuple

uname_result = namedtuple('uname_result', 'system node release version machine')


def uname():
    u = os.uname()
    return uname_result(u[0], u[1], u[2], u[3], u[4])


def system():
    return uname().system


def node():
    return uname().node


def release():
    return uname().release


def version():
    return uname().version


def machine():
    return uname().machine


def processor():
    return ''


def architecture(executable=None, bits='', linkage=''):
    return ('64bit', 'ELF')


def python_version():
    return '3.13.5'


def python_version_tuple():
    return ('3', '13', '5')


def python_implementation():
    return 'CPython'


def python_compiler():
    return 'GCC 14.2.0'


def python_build():
    return ('main', 'Aug 10 2026 12:06:59')


def python_branch():
    return ''


def python_revision():
    return ''


def libc_ver(executable=None, lib='', version='', chunksize=16384):
    return ('glibc', '2.41')


def platform(aliased=False, terse=False):
    u = uname()
    if terse:
        return u.system + '-' + u.release
    return '%s-%s-%s-with-glibc2.41' % (u.system, u.release, u.machine)


def freedesktop_os_release():
    info = {}
    with open('/etc/os-release') as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith('#') and '=' in line:
                k, v = line.split('=', 1)
                info[k] = v.strip('"\'')
    return info
