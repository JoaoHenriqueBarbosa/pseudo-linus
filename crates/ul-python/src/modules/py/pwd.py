"""pwd sobre /etc/passwd."""
from collections import namedtuple as _nt

_base_struct_passwd = _nt('struct_passwd', 'pw_name pw_passwd pw_uid pw_gid pw_gecos pw_dir pw_shell')


class struct_passwd(_base_struct_passwd):
    __slots__ = ()

    def __repr__(self):
        return 'pwd.' + _base_struct_passwd.__repr__(self)

    __module__ = 'pwd'


def getpwall():
    out = []
    try:
        with open('/etc/passwd') as f:
            for line in f:
                line = line.rstrip('\n')
                if not line or line.startswith('#'):
                    continue
                p = line.split(':')
                if len(p) >= 7:
                    out.append(struct_passwd(p[0], p[1], int(p[2]), int(p[3]), p[4], p[5], p[6]))
    except OSError:
        pass
    return out


def getpwnam(name):
    if not isinstance(name, str):
        raise TypeError('getpwnam() argument must be str, not %s' % type(name).__name__)
    for e in getpwall():
        if e.pw_name == name:
            return e
    raise KeyError('getpwnam(): name not found: %r' % name)


def getpwuid(uid):
    for e in getpwall():
        if e.pw_uid == uid:
            return e
    raise KeyError('getpwuid(): uid not found: %s' % uid)
