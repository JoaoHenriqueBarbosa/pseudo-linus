"""glob: expansão de padrões de caminho."""

import os
import fnmatch

__all__ = ['glob', 'iglob', 'escape', 'has_magic']

_magic = ('*', '?', '[')


def has_magic(s):
    return any(c in s for c in _magic)


def escape(pathname):
    drive, pathname = os.path.splitdrive(pathname)
    out = []
    for c in pathname:
        out.append('[' + c + ']' if c in _magic else c)
    return drive + ''.join(out)


def _ishidden(path):
    return path[0] == '.'


def _listdir(dirname):
    try:
        return os.listdir(dirname or os.curdir)
    except OSError:
        return []


def _isdir(path):
    return os.path.isdir(path)


def glob(pathname, *, root_dir=None, dir_fd=None, recursive=False, include_hidden=False):
    return list(iglob(pathname, root_dir=root_dir, recursive=recursive, include_hidden=include_hidden))


def iglob(pathname, *, root_dir=None, dir_fd=None, recursive=False, include_hidden=False):
    pathname = os.fspath(pathname)
    if root_dir is not None:
        root_dir = os.fspath(root_dir)
    for p in _iglob(pathname, root_dir, recursive, include_hidden, False):
        yield p


def _join(root_dir, rel):
    return os.path.join(root_dir, rel) if root_dir else rel


def _iglob(pathname, root_dir, recursive, include_hidden, dironly):
    dirname, basename = os.path.split(pathname)
    if not has_magic(pathname):
        if basename:
            if os.path.lexists(_join(root_dir, pathname)):
                yield pathname
        elif os.path.isdir(_join(root_dir, dirname)):
            yield pathname
        return
    if not dirname:
        if recursive and basename == '**':
            for p in _glob2(root_dir, basename, dironly, include_hidden):
                yield p
        else:
            for p in _glob1(root_dir, basename, dironly, include_hidden):
                yield p
        return
    if dirname != pathname and has_magic(dirname):
        dirs = _iglob(dirname, root_dir, recursive, include_hidden, True)
    else:
        dirs = [dirname]
    if has_magic(basename):
        if recursive and basename == '**':
            glob_in_dir = _glob2
        else:
            glob_in_dir = _glob1
    else:
        glob_in_dir = _glob0
    for d in dirs:
        for name in glob_in_dir(_join(root_dir, d), basename, dironly, include_hidden):
            yield os.path.join(d, name)


def _glob1(dirname, pattern, dironly, include_hidden):
    names = _listdir(dirname)
    if dironly:
        names = [x for x in names if _isdir(os.path.join(dirname, x))]
    if include_hidden or not _ishidden(pattern):
        names = [x for x in names if include_hidden or not _ishidden(x)]
    return fnmatch.filter(names, pattern)


def _glob0(dirname, basename, dironly, include_hidden):
    if not basename:
        if os.path.isdir(dirname):
            return [basename]
    elif os.path.lexists(os.path.join(dirname, basename)):
        return [basename]
    return []


def _glob2(dirname, pattern, dironly, include_hidden):
    yield pattern[:0]
    for p in _rlistdir(dirname, dironly, include_hidden):
        yield p


def _rlistdir(dirname, dironly, include_hidden):
    names = _listdir(dirname)
    for x in sorted(names):
        if not include_hidden and _ishidden(x):
            continue
        path = os.path.join(dirname, x) if dirname else x
        if dironly and not _isdir(path):
            continue
        yield x
        if _isdir(path) and not os.path.islink(path):
            for y in _rlistdir(path, dironly, include_hidden):
                yield os.path.join(x, y)
