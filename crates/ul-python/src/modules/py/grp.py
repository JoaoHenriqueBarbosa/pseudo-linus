"""grp sobre /etc/group."""
from collections import namedtuple as _nt

_base_struct_group = _nt('struct_group', 'gr_name gr_passwd gr_gid gr_mem')


class struct_group(_base_struct_group):
    __slots__ = ()

    def __repr__(self):
        return 'grp.' + _base_struct_group.__repr__(self)

    __module__ = 'grp'


def getgrall():
    out = []
    try:
        with open('/etc/group') as f:
            for line in f:
                line = line.rstrip('\n')
                if not line or line.startswith('#'):
                    continue
                p = line.split(':')
                if len(p) >= 4:
                    out.append(struct_group(p[0], p[1], int(p[2]), [m for m in p[3].split(',') if m]))
    except OSError:
        pass
    return out


def getgrnam(name):
    if not isinstance(name, str):
        raise TypeError('getgrnam() argument must be str, not %s' % type(name).__name__)
    for e in getgrall():
        if e.gr_name == name:
            return e
    raise KeyError('getgrnam(): name not found: %r' % name)


def getgrgid(gid):
    for e in getgrall():
        if e.gr_gid == gid:
            return e
    raise KeyError('getgrgid(): gid not found: %s' % gid)
