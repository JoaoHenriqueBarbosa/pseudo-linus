"""compileall: percorre diretórios e compila cada `.py` com `py_compile` (checa sintaxe; ver a nota lá)."""

import os
import sys
import py_compile
import importlib.util

__all__ = ['compile_dir', 'compile_file', 'compile_path']


def _walk_dir(dir, maxlevels, quiet=0):
    if quiet < 2 and isinstance(dir, os.PathLike):
        dir = os.fspath(dir)
    if not quiet:
        print('Listing {!r}...'.format(dir))
    try:
        names = os.listdir(dir)
    except OSError:
        if quiet < 2:
            print("Can't list {!r}".format(dir))
        names = []
    names.sort()
    for name in names:
        if name == '__pycache__':
            continue
        fullname = os.path.join(dir, name)
        if not os.path.isdir(fullname):
            yield fullname
        elif maxlevels > 0 and name != os.curdir and name != os.pardir and os.path.isdir(fullname) and not os.path.islink(fullname):
            yield from _walk_dir(fullname, maxlevels=maxlevels - 1, quiet=quiet)


def compile_dir(dir, maxlevels=None, ddir=None, force=False, rx=None, quiet=0, legacy=False, optimize=-1,
                workers=1, invalidation_mode=None, *, stripdir=None, prependdir=None, limit_sl_dest=None,
                hardlink_dupes=False):
    ProcessPoolExecutor = None
    if maxlevels is None:
        maxlevels = sys.getrecursionlimit()
    success = True
    for file in _walk_dir(dir, quiet=quiet, maxlevels=maxlevels):
        if not compile_file(file, ddir, force, rx, quiet, legacy, optimize, invalidation_mode):
            success = False
    return success


def compile_file(fullname, ddir=None, force=False, rx=None, quiet=0, legacy=False, optimize=-1,
                 invalidation_mode=None, *, stripdir=None, prependdir=None, limit_sl_dest=None,
                 hardlink_dupes=False):
    success = True
    fullname = os.fspath(fullname)
    name = os.path.basename(fullname)
    dfile = None
    if ddir is not None:
        dfile = os.path.join(ddir, name)
    if rx is not None and rx.search(fullname):
        return success
    if os.path.isfile(fullname):
        if name.endswith('.py'):
            if not quiet:
                print('Compiling {!r}...'.format(fullname))
            try:
                ok = py_compile.compile(fullname, None, dfile, True, optimize=optimize, quiet=quiet)
            except py_compile.PyCompileError as err:
                success = False
                if quiet >= 2:
                    return success
                elif quiet:
                    print('*** Error compiling {!r}...'.format(fullname))
                else:
                    print('*** ', end='')
                print(err.msg.encode(sys.stdout.encoding, errors='backslashreplace').decode(sys.stdout.encoding))
            except (SyntaxError, UnicodeError, OSError) as e:
                success = False
                if quiet >= 2:
                    return success
                elif quiet:
                    print('*** Error compiling {!r}...'.format(fullname))
                else:
                    print('*** ', end='')
                print(e.__class__.__name__ + ':', e)
            else:
                if ok == 0:
                    success = False
    return success


def compile_path(skip_curdir=1, maxlevels=0, force=False, quiet=0, legacy=False, optimize=-1,
                 invalidation_mode=None):
    success = True
    for dir in sys.path:
        if (not dir or dir == os.curdir) and skip_curdir:
            if quiet < 2:
                print('Skipping current directory')
        else:
            success = success and compile_dir(dir, maxlevels, None, force, quiet=quiet, legacy=legacy,
                                              optimize=optimize, invalidation_mode=invalidation_mode)
    return success


def main():
    import argparse
    parser = argparse.ArgumentParser(description='Utilities to support installing Python libraries.')
    parser.add_argument('-l', action='store_const', const=0, default=None, dest='maxlevels',
                        help="don't recurse into subdirectories")
    parser.add_argument('-r', type=int, dest='recursion', help='control the maximum recursion level')
    parser.add_argument('-f', action='store_true', dest='force', help='force rebuild even if timestamps are up to date')
    parser.add_argument('-q', action='count', dest='quiet', default=0, help='output only error messages; -qq will suppress the error messages as well.')
    parser.add_argument('-b', action='store_true', dest='legacy', help='use legacy (pre-PEP3147) compiled file locations')
    parser.add_argument('-d', metavar='DESTDIR', dest='ddir', default=None, help='directory to prepend to file paths for use in compile-time tracebacks and in runtime tracebacks in cases where the source file is unavailable')
    parser.add_argument('-x', metavar='REGEXP', dest='rx', default=None, help='skip files matching the regular expression')
    parser.add_argument('-i', metavar='FILE', dest='flist', help='add all the files and directories listed in FILE to the list considered for compilation; if "-", names are read from stdin')
    parser.add_argument('compile_dest', metavar='FILE|DIR', nargs='*', help='zero or more file and directory names to compile; if no arguments given, defaults to the equivalent of -l sys.path')
    parser.add_argument('-j', '--workers', default=1, type=int, help='Run compileall concurrently')
    args = parser.parse_args()
    compile_dests = args.compile_dest
    if args.rx:
        import re
        args.rx = re.compile(args.rx)
    maxlevels = args.recursion if args.recursion is not None else args.maxlevels
    success = True
    try:
        if compile_dests:
            for dest in compile_dests:
                if os.path.isfile(dest):
                    if not compile_file(dest, args.ddir, args.force, args.rx, args.quiet, args.legacy):
                        success = False
                else:
                    if not compile_dir(dest, maxlevels, args.ddir, args.force, args.rx, args.quiet, args.legacy):
                        success = False
            return success
        else:
            return compile_path(legacy=args.legacy, force=args.force, quiet=args.quiet)
    except KeyboardInterrupt:
        if args.quiet < 2:
            print('\n[interrupted]')
        return False
    return True


if __name__ == '__main__':
    exit_status = int(not main())
    sys.exit(exit_status)
