"""py_compile: compila o fonte de verdade (para achar erros de sintaxe), mas o interpretador não gera bytecode do
CPython. O arquivo `.pyc` escrito é só um marcador com o cabeçalho de 16 bytes, sem objeto de código."""

import enum
import os
import sys
import traceback

__all__ = ['compile', 'main', 'PyCompileError', 'PycInvalidationMode']


class PyCompileError(Exception):
    def __init__(self, exc_type, exc_value, file, msg=''):
        exc_type_name = exc_type.__name__
        if exc_type is SyntaxError:
            tbtext = ''.join(traceback.format_exception_only(exc_type, exc_value))
            errmsg = tbtext.replace('File "<string>"', 'File "%s"' % file)
        else:
            errmsg = 'Sorry: %s: %s' % (exc_type_name, exc_value)
        Exception.__init__(self, msg or errmsg, exc_type_name, exc_value, file)
        self.exc_type_name = exc_type_name
        self.exc_value = exc_value
        self.file = file
        self.msg = msg or errmsg

    def __str__(self):
        return self.msg


class PycInvalidationMode(enum.Enum):
    TIMESTAMP = 1
    CHECKED_HASH = 2
    UNCHECKED_HASH = 3


def _cache_path(path):
    head, tail = os.path.split(path)
    base = tail[:-3] if tail.endswith('.py') else tail
    return os.path.join(head, '__pycache__', base + '.cpython-313.pyc')


def compile(file, cfile=None, dfile=None, doraise=False, optimize=-1, invalidation_mode=None, quiet=0):
    if cfile is None:
        cfile = _cache_path(os.fspath(file))
    if os.path.islink(cfile):
        raise FileExistsError('{} is a symlink and will be changed into a regular file if import writes a byte-compiled file to it'.format(cfile))
    with open(file, 'rb') as f:
        source = f.read()
    try:
        __builtins__['compile'](source, dfile or file, 'exec', dont_inherit=True)
    except Exception as err:
        py_exc = PyCompileError(err.__class__, err, dfile or file)
        if quiet < 2:
            if doraise:
                raise py_exc
            sys.stderr.write(py_exc.msg + '\n')
        return None
    dirname = os.path.dirname(cfile)
    if dirname:
        os.makedirs(dirname, exist_ok=True)
    mtime = int(os.stat(file).st_mtime) & 0xFFFFFFFF
    header = b'\xf3\r\r\n' + (0).to_bytes(4, 'little') + mtime.to_bytes(4, 'little') + (len(source) & 0xFFFFFFFF).to_bytes(4, 'little')
    with open(cfile, 'wb') as f:
        f.write(header + b'N')
    return cfile


def main():
    args = sys.argv[1:]
    rv = 0
    if args == ['-']:
        while True:
            filename = sys.stdin.readline()
            if not filename:
                break
            filename = filename.rstrip('\n')
            try:
                compile(filename, doraise=True)
            except PyCompileError as error:
                rv = 1
                sys.stderr.write('%s\n' % error.msg)
            except OSError as error:
                rv = 1
                sys.stderr.write('%s\n' % error)
    else:
        for filename in args:
            try:
                compile(filename, doraise=True)
            except PyCompileError as error:
                rv = 1
                sys.stderr.write(error.msg)
            except OSError as error:
                rv = 1
                sys.stderr.write('%s' % error)
    return rv


if __name__ == '__main__':
    sys.exit(main())
