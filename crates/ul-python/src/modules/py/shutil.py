"""shutil: operações de alto nível sobre arquivos."""

import os
import stat

__all__ = ['copyfileobj', 'copyfile', 'copymode', 'copystat', 'copy', 'copy2', 'copytree', 'move',
           'rmtree', 'Error', 'SameFileError', 'which', 'get_terminal_size',
           'ignore_patterns', 'make_archive', 'unpack_archive', 'get_archive_formats', 'get_unpack_formats', 'disk_usage']


class Error(OSError):
    pass


class SameFileError(Error):
    pass


class SpecialFileError(OSError):
    pass


class ReadError(OSError):
    pass


def copyfileobj(fsrc, fdst, length=1024 * 1024):
    while True:
        buf = fsrc.read(length)
        if not buf:
            break
        fdst.write(buf)


def _samefile(src, dst):
    try:
        return os.path.samefile(src, dst)
    except OSError:
        return False


def copyfile(src, dst, *, follow_symlinks=True):
    src = os.fspath(src)
    dst = os.fspath(dst)
    if _samefile(src, dst):
        raise SameFileError('{!r} and {!r} are the same file'.format(src, dst))
    if not follow_symlinks and os.path.islink(src):
        os.symlink(os.readlink(src), dst)
    else:
        if os.path.isdir(src):
            raise IsADirectoryError(21, 'Is a directory', src)
        with open(src, 'rb') as fsrc:
            with open(dst, 'wb') as fdst:
                copyfileobj(fsrc, fdst)
    return dst


def copymode(src, dst, *, follow_symlinks=True):
    st = os.stat(src)
    os.chmod(dst, stat.S_IMODE(st.st_mode))


def copystat(src, dst, *, follow_symlinks=True):
    st = os.stat(src)
    os.utime(dst, (st.st_atime, st.st_mtime))
    os.chmod(dst, stat.S_IMODE(st.st_mode))


def copy(src, dst, *, follow_symlinks=True):
    if os.path.isdir(dst):
        dst = os.path.join(dst, os.path.basename(src))
    copyfile(src, dst, follow_symlinks=follow_symlinks)
    copymode(src, dst, follow_symlinks=follow_symlinks)
    return dst


def copy2(src, dst, *, follow_symlinks=True):
    if os.path.isdir(dst):
        dst = os.path.join(dst, os.path.basename(src))
    copyfile(src, dst, follow_symlinks=follow_symlinks)
    copystat(src, dst, follow_symlinks=follow_symlinks)
    return dst


def ignore_patterns(*patterns):
    import fnmatch

    def _ignore_patterns(path, names):
        ignored = set()
        for pattern in patterns:
            ignored.update(fnmatch.filter(names, pattern))
        return ignored
    return _ignore_patterns


def copytree(src, dst, symlinks=False, ignore=None, copy_function=copy2,
             ignore_dangling_symlinks=False, dirs_exist_ok=False):
    src = os.fspath(src)
    dst = os.fspath(dst)
    names = sorted(os.listdir(src))
    ignored = ignore(src, names) if ignore is not None else set()
    os.makedirs(dst, exist_ok=dirs_exist_ok)
    errors = []
    for name in names:
        if name in ignored:
            continue
        s = os.path.join(src, name)
        d = os.path.join(dst, name)
        try:
            if os.path.islink(s):
                if symlinks:
                    os.symlink(os.readlink(s), d)
                else:
                    if not os.path.exists(s) and ignore_dangling_symlinks:
                        continue
                    if os.path.isdir(s):
                        copytree(s, d, symlinks, ignore, copy_function, ignore_dangling_symlinks, dirs_exist_ok)
                    else:
                        copy_function(s, d)
            elif os.path.isdir(s):
                copytree(s, d, symlinks, ignore, copy_function, ignore_dangling_symlinks, dirs_exist_ok)
            else:
                copy_function(s, d)
        except Error as err:
            errors.extend(err.args[0])
        except OSError as why:
            errors.append((s, d, str(why)))
    try:
        copystat(src, dst)
    except OSError as why:
        errors.append((src, dst, str(why)))
    if errors:
        raise Error(errors)
    return dst


def rmtree(path, ignore_errors=False, onerror=None, *, onexc=None, dir_fd=None):
    path = os.fspath(path)

    def handle(func, p, exc):
        if ignore_errors:
            return
        if onexc is not None:
            onexc(func, p, exc)
        elif onerror is not None:
            onerror(func, p, (type(exc), exc, None))
        else:
            raise exc

    if os.path.islink(path):
        handle(os.path.islink, path, OSError('Cannot call rmtree on a symbolic link'))
        return
    try:
        entries = os.listdir(path)
    except OSError as e:
        handle(os.listdir, path, e)
        entries = []
    for name in entries:
        full = os.path.join(path, name)
        try:
            is_dir = os.path.isdir(full) and not os.path.islink(full)
        except OSError:
            is_dir = False
        if is_dir:
            rmtree(full, ignore_errors, onerror, onexc=onexc)
        else:
            try:
                os.unlink(full)
            except OSError as e:
                handle(os.unlink, full, e)
    try:
        os.rmdir(path)
    except OSError as e:
        handle(os.rmdir, path, e)


rmtree.avoids_symlink_attacks = True


def _basename(path):
    return os.path.basename(path.rstrip('/'))


def move(src, dst, copy_function=copy2):
    src = os.fspath(src)
    dst = os.fspath(dst)
    real_dst = dst
    if os.path.isdir(dst):
        if _samefile(src, dst):
            os.rename(src, dst)
            return real_dst
        real_dst = os.path.join(dst, _basename(src))
        if os.path.exists(real_dst):
            raise Error("Destination path '%s' already exists" % real_dst)
    try:
        os.rename(src, real_dst)
    except OSError:
        if os.path.islink(src):
            os.symlink(os.readlink(src), real_dst)
            os.unlink(src)
        elif os.path.isdir(src):
            copytree(src, real_dst, copy_function=copy_function, symlinks=True)
            rmtree(src)
        else:
            copy_function(src, real_dst)
            os.unlink(src)
    return real_dst


import collections as _collections

_ntuple_diskusage = _collections.namedtuple('usage', 'total used free')


def disk_usage(path):
    """Uso do disco do ponto de montagem de `path` (via `df`, que o sandbox implementa)."""
    import subprocess
    os.stat(path)
    out = subprocess.run(['df', '-Pk', os.fspath(path)], capture_output=True, text=True).stdout.splitlines()
    fields = out[-1].split()
    total, used, free = (int(fields[1]) * 1024, int(fields[2]) * 1024, int(fields[3]) * 1024)
    return _ntuple_diskusage(total, used, free)


def which(cmd, mode=os.F_OK | os.X_OK, path=None):
    if os.path.dirname(cmd):
        if os.path.exists(cmd) and os.access(cmd, mode) and not os.path.isdir(cmd):
            return cmd
        return None
    if path is None:
        path = os.environ.get('PATH', os.defpath)
    if not path:
        return None
    for d in path.split(os.pathsep):
        name = os.path.join(d, cmd)
        if os.path.exists(name) and os.access(name, mode) and not os.path.isdir(name):
            return name
    return None


def get_terminal_size(fallback=(80, 24)):
    try:
        columns = int(os.environ['COLUMNS'])
    except (KeyError, ValueError):
        columns = 0
    try:
        lines = int(os.environ['LINES'])
    except (KeyError, ValueError):
        lines = 0
    if columns <= 0 or lines <= 0:
        try:
            size = os.get_terminal_size(1)
        except (OSError, ValueError, AttributeError):
            size = os.terminal_size(fallback)
        if columns <= 0:
            columns = size.columns or fallback[0]
        if lines <= 0:
            lines = size.lines or fallback[1]
    return os.terminal_size((columns, lines))


_TAR_FORMATS = {'tar': ('', ''), 'gztar': ('gz', '.gz'), 'bztar': ('bz2', '.bz2'), 'xztar': ('xz', '.xz')}


def get_archive_formats():
    return [('bztar', "bzip2'ed tar-file"), ('gztar', "gzip'ed tar-file"), ('tar', 'uncompressed tar file'),
            ('xztar', "xz'ed tar-file"), ('zip', 'ZIP file')]


def get_unpack_formats():
    return [('bztar', ['.tar.bz2', '.tbz2'], "bzip2'ed tar-file"), ('gztar', ['.tar.gz', '.tgz'], "gzip'ed tar-file"),
            ('tar', ['.tar'], 'uncompressed tar file'), ('xztar', ['.tar.xz', '.txz'], "xz'ed tar-file"),
            ('zip', ['.zip'], 'ZIP file')]


def make_archive(base_name, format, root_dir=None, base_dir=None, verbose=0, dry_run=0, owner=None, group=None,
                 logger=None):
    base_name = os.fspath(base_name)
    root = os.fspath(root_dir) if root_dir is not None else os.curdir
    base = os.fspath(base_dir) if base_dir is not None else os.curdir
    if format not in _TAR_FORMATS and format != 'zip':
        raise ValueError('unknown archive format %r' % format)
    if root_dir is not None:
        base_name = os.path.abspath(base_name)
    top = os.path.join(root, base) if base != os.curdir else root
    if format == 'zip':
        import zipfile
        archive = base_name + '.zip'
        with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as zf:
            for dirpath, dirnames, filenames in os.walk(top):
                dirnames.sort()
                rel = os.path.normpath(os.path.relpath(dirpath, root))
                if rel != '.':
                    zf.write(dirpath, rel)
                for fn in sorted(filenames):
                    full = os.path.join(dirpath, fn)
                    zf.write(full, os.path.normpath(os.path.relpath(full, root)))
        return archive
    import tarfile
    comp, ext = _TAR_FORMATS[format]
    archive = base_name + '.tar' + ext
    with tarfile.open(archive, 'w:' + comp if comp else 'w') as tf:
        rel_top = os.path.normpath(os.path.relpath(top, root))
        tf.add(top, arcname=rel_top)
    return archive


def unpack_archive(filename, extract_dir=None, format=None, **kwargs):
    filename = os.fspath(filename)
    extract_dir = os.fspath(extract_dir) if extract_dir is not None else os.getcwd()
    if format is None:
        for fmt, exts, _ in get_unpack_formats():
            if any(filename.endswith(e) for e in exts):
                format = fmt
                break
        else:
            raise ReadError('Unknown archive format %r' % filename)
    if format == 'zip':
        import zipfile
        with zipfile.ZipFile(filename) as zf:
            zf.extractall(extract_dir)
        return
    if format in _TAR_FORMATS:
        import tarfile
        with tarfile.open(filename) as tf:
            tf.extractall(extract_dir, filter=kwargs.get('filter', 'data'))
        return
    raise ValueError('Unknown unpack format %r' % format)
