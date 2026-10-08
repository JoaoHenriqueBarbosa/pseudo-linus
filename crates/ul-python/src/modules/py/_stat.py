"""`_stat`: constantes e funções de `os.stat()` (o módulo em C que o `stat.py` do Debian importa por cima das suas)."""

ST_MODE = 0
ST_INO = 1
ST_DEV = 2
ST_NLINK = 3
ST_UID = 4
ST_GID = 5
ST_SIZE = 6
ST_ATIME = 7
ST_MTIME = 8
ST_CTIME = 9

S_IFDIR = 0o040000
S_IFCHR = 0o020000
S_IFBLK = 0o060000
S_IFREG = 0o100000
S_IFIFO = 0o010000
S_IFLNK = 0o120000
S_IFSOCK = 0o140000
S_IFDOOR = 0
S_IFPORT = 0
S_IFWHT = 0

S_ISUID = 0o4000
S_ISGID = 0o2000
S_ENFMT = S_ISGID
S_ISVTX = 0o1000
S_IREAD = 0o0400
S_IWRITE = 0o0200
S_IEXEC = 0o0100
S_IRWXU = 0o0700
S_IRUSR = 0o0400
S_IWUSR = 0o0200
S_IXUSR = 0o0100
S_IRWXG = 0o0070
S_IRGRP = 0o0040
S_IWGRP = 0o0020
S_IXGRP = 0o0010
S_IRWXO = 0o0007
S_IROTH = 0o0004
S_IWOTH = 0o0002
S_IXOTH = 0o0001

UF_SETTABLE = 0x0000ffff
UF_NODUMP = 0x00000001
UF_IMMUTABLE = 0x00000002
UF_APPEND = 0x00000004
UF_OPAQUE = 0x00000008
UF_NOUNLINK = 0x00000010
UF_COMPRESSED = 0x00000020
UF_TRACKED = 0x00000040
UF_DATAVAULT = 0x00000080
UF_HIDDEN = 0x00008000
SF_SETTABLE = 0xffff0000
SF_ARCHIVED = 0x00010000
SF_IMMUTABLE = 0x00020000
SF_APPEND = 0x00040000
SF_NOUNLINK = 0x00100000
SF_SNAPSHOT = 0x00200000
SF_FIRMLINK = 0x00800000
SF_DATALESS = 0x40000000


def _mode_arg(name, args):
    """O `mode` do único argumento (`METH_O`), convertido como o `_PyLong_AsMode_t` do C."""
    if len(args) != 1:
        raise TypeError('%s() takes exactly one argument (%d given)' % (name, len(args)))
    mode = args[0]
    if not isinstance(mode, int):
        raise TypeError('an integer is required')
    if mode < 0:
        raise OverflowError("can't convert negative value to unsigned int")
    return mode


def S_IMODE(*args):
    return _mode_arg('S_IMODE', args) & 0o7777


def S_IFMT(*args):
    return _mode_arg('S_IFMT', args) & 0o170000


def S_ISDIR(*args):
    return _mode_arg('S_ISDIR', args) & 0o170000 == S_IFDIR


def S_ISCHR(*args):
    return _mode_arg('S_ISCHR', args) & 0o170000 == S_IFCHR


def S_ISBLK(*args):
    return _mode_arg('S_ISBLK', args) & 0o170000 == S_IFBLK


def S_ISREG(*args):
    return _mode_arg('S_ISREG', args) & 0o170000 == S_IFREG


def S_ISFIFO(*args):
    return _mode_arg('S_ISFIFO', args) & 0o170000 == S_IFIFO


def S_ISLNK(*args):
    return _mode_arg('S_ISLNK', args) & 0o170000 == S_IFLNK


def S_ISSOCK(*args):
    return _mode_arg('S_ISSOCK', args) & 0o170000 == S_IFSOCK


def S_ISDOOR(*args):
    _mode_arg('S_ISDOOR', args)
    return False


def S_ISPORT(*args):
    _mode_arg('S_ISPORT', args)
    return False


def S_ISWHT(*args):
    _mode_arg('S_ISWHT', args)
    return False


_FILEMODE_TABLE = (
    ((S_IFLNK, 'l'), (S_IFSOCK, 's'), (S_IFREG, '-'), (S_IFBLK, 'b'), (S_IFDIR, 'd'), (S_IFCHR, 'c'), (S_IFIFO, 'p')),
    ((S_IRUSR, 'r'),),
    ((S_IWUSR, 'w'),),
    ((S_IXUSR | S_ISUID, 's'), (S_ISUID, 'S'), (S_IXUSR, 'x')),
    ((S_IRGRP, 'r'),),
    ((S_IWGRP, 'w'),),
    ((S_IXGRP | S_ISGID, 's'), (S_ISGID, 'S'), (S_IXGRP, 'x')),
    ((S_IROTH, 'r'),),
    ((S_IWOTH, 'w'),),
    ((S_IXOTH | S_ISVTX, 't'), (S_ISVTX, 'T'), (S_IXOTH, 'x')),
)


def filemode(*args):
    mode = _mode_arg('filemode', args)
    perm = []
    for index, table in enumerate(_FILEMODE_TABLE):
        for bit, char in table:
            if mode & bit == bit:
                perm.append(char)
                break
        else:
            perm.append('?' if index == 0 else '-')
    return ''.join(perm)
