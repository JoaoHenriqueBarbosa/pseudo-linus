"""Operações de caminho POSIX (subconjunto fiel do posixpath do CPython 3.13)."""

import _os

curdir = '.'
pardir = '..'
extsep = '.'
sep = '/'
pathsep = ':'
defpath = '/bin:/usr/bin'
altsep = None
devnull = '/dev/null'


def _fspath(path):
    if isinstance(path, (str, bytes)):
        return path
    meth = getattr(path, '__fspath__', None)
    if meth is None:
        raise TypeError('expected str, bytes or os.PathLike object, not ' + type(path).__name__)
    return meth()


def normcase(s):
    return _fspath(s)


def isabs(s):
    s = _fspath(s)
    return s.startswith('/')


def join(a, *p):
    a = _fspath(a)
    path = a
    for b in p:
        b = _fspath(b)
        if b.startswith('/'):
            path = b
        elif not path or path.endswith('/'):
            path += b
        else:
            path += '/' + b
    return path


def split(p):
    p = _fspath(p)
    i = p.rfind('/') + 1
    head, tail = p[:i], p[i:]
    if head and head != '/' * len(head):
        head = head.rstrip('/')
    return (head, tail)


def splitext(p):
    p = _fspath(p)
    sep_index = p.rfind('/')
    dot_index = p.rfind('.')
    if dot_index > sep_index:
        filename_index = sep_index + 1
        while filename_index < dot_index:
            if p[filename_index] != '.':
                return (p[:dot_index], p[dot_index:])
            filename_index += 1
    return (p, p[:0])


def splitdrive(p):
    p = _fspath(p)
    return (p[:0], p)


def basename(p):
    p = _fspath(p)
    i = p.rfind('/') + 1
    return p[i:]


def dirname(p):
    p = _fspath(p)
    i = p.rfind('/') + 1
    head = p[:i]
    if head and head != '/' * len(head):
        head = head.rstrip('/')
    return head


def normpath(path):
    path = _fspath(path)
    if not path:
        return '.'
    initial_slashes = path.startswith('/')
    if initial_slashes and path.startswith('//') and not path.startswith('///'):
        initial_slashes = 2
    comps = path.split('/')
    new_comps = []
    for comp in comps:
        if comp in ('', '.'):
            continue
        if comp != '..' or (not initial_slashes and not new_comps) or (new_comps and new_comps[-1] == '..'):
            new_comps.append(comp)
        elif new_comps:
            new_comps.pop()
    path = '/'.join(new_comps)
    if initial_slashes:
        path = '/' * initial_slashes + path
    return path or '.'


def abspath(path):
    path = _fspath(path)
    if not isabs(path):
        path = join(_os.getcwd(), path)
    return normpath(path)


class _AllowMissing:
    """`os.path.ALLOW_MISSING`: `strict` que só tolera caminhos inexistentes."""

    def __repr__(self):
        return 'os.path.ALLOW_MISSING'


ALLOW_MISSING = _AllowMissing()


def realpath(filename, *, strict=False):
    filename = _fspath(filename)
    path = abspath(filename)
    parts = path.split('/')[1:]
    resolved = ''
    seen = 0
    while parts:
        name = parts.pop(0)
        if not name or name == '.':
            continue
        if name == '..':
            resolved = dirname(resolved)
            continue
        candidate = resolved + '/' + name
        try:
            st = _os.stat(candidate, False)
        except OSError:
            if strict and strict is not ALLOW_MISSING:
                raise
            resolved = candidate
            continue
        if (st[0] & 0o170000) == 0o120000:
            seen += 1
            if seen > 40:
                raise OSError('[Errno 40] Too many levels of symbolic links: ' + repr(filename))
            target = _os.readlink(candidate)
            if target.startswith('/'):
                resolved = ''
            parts = target.split('/') + parts
        else:
            resolved = candidate
    return resolved or '/'


def relpath(path, start=None):
    path = _fspath(path)
    if not path:
        raise ValueError('no path specified')
    if start is None:
        start = '.'
    start = _fspath(start)
    start_list = [x for x in abspath(start).split('/') if x]
    path_list = [x for x in abspath(path).split('/') if x]
    i = len(commonprefix([start_list, path_list]))
    rel_list = ['..'] * (len(start_list) - i) + path_list[i:]
    if not rel_list:
        return '.'
    return join(*rel_list)


def commonprefix(m):
    if not m:
        return ''
    if not isinstance(m[0], (list, tuple)):
        m = tuple(map(_fspath, m))
    s1 = min(m)
    s2 = max(m)
    for i, c in enumerate(s1):
        if c != s2[i]:
            return s1[:i]
    return s1


def commonpath(paths):
    paths = tuple(map(_fspath, paths))
    if not paths:
        raise ValueError('commonpath() arg is an empty sequence')
    split_paths = [path.split('/') for path in paths]
    if len(set(p.startswith('/') for p in paths)) != 1:
        raise ValueError("Can't mix absolute and relative paths")
    split_paths = [[c for c in s if c and c != '.'] for s in split_paths]
    s1 = min(split_paths)
    s2 = max(split_paths)
    common = s1
    for i, c in enumerate(s1):
        if c != s2[i]:
            common = s1[:i]
            break
    prefix = '/' if paths[0].startswith('/') else ''
    return prefix + '/'.join(common)


def _stat(path, follow=True):
    return _os.stat(_fspath(path), follow)


def exists(path):
    try:
        _stat(path)
    except (OSError, ValueError):
        return False
    return True


def lexists(path):
    try:
        _stat(path, False)
    except (OSError, ValueError):
        return False
    return True


def isfile(path):
    try:
        st = _stat(path)
    except (OSError, ValueError):
        return False
    return (st[0] & 0o170000) == 0o100000


def isdir(s):
    try:
        st = _stat(s)
    except (OSError, ValueError):
        return False
    return (st[0] & 0o170000) == 0o040000


def islink(path):
    try:
        st = _stat(path, False)
    except (OSError, ValueError):
        return False
    return (st[0] & 0o170000) == 0o120000


def ismount(path):
    return _fspath(path) == '/'


def getsize(filename):
    return _stat(filename)[6]


def getatime(filename):
    return _stat(filename)[7]


def getmtime(filename):
    return _stat(filename)[8]


def getctime(filename):
    return _stat(filename)[9]


def samefile(f1, f2):
    s1 = _stat(f1)
    s2 = _stat(f2)
    return s1[1] == s2[1] and s1[2] == s2[2]


def expanduser(path):
    path = _fspath(path)
    if not path.startswith('~'):
        return path
    i = path.find('/', 1)
    if i < 0:
        i = len(path)
    if i == 1:
        userhome = _os.getenv('HOME')
        if userhome is None:
            userhome = '/root'
    else:
        return path
    userhome = userhome.rstrip('/')
    return (userhome + path[i:]) or '/'


def expandvars(path):
    path = _fspath(path)
    if '$' not in path:
        return path
    out = []
    i = 0
    n = len(path)
    while i < n:
        c = path[i]
        if c != '$':
            out.append(c)
            i += 1
            continue
        if i + 1 < n and path[i + 1] == '{':
            j = path.find('}', i + 2)
            if j < 0:
                out.append(path[i:])
                break
            name = path[i + 2:j]
            end = j + 1
        else:
            j = i + 1
            while j < n and (path[j].isalnum() or path[j] == '_'):
                j += 1
            name = path[i + 1:j]
            end = j
        value = _os.getenv(name) if name else None
        if value is None:
            out.append(path[i:end])
        else:
            out.append(value)
        i = end
    return ''.join(out)
