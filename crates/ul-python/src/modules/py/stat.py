"""stat: constantes e funções para interpretar st_mode."""

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

S_IFMT_MASK = 0o170000
S_IFSOCK = 0o140000
S_IFLNK = 0o120000
S_IFREG = 0o100000
S_IFBLK = 0o060000
S_IFDIR = 0o040000
S_IFCHR = 0o020000
S_IFIFO = 0o010000
S_ISUID = 0o4000
S_ISGID = 0o2000
S_ISVTX = 0o1000
S_IRWXU = 0o700
S_IRUSR = 0o400
S_IWUSR = 0o200
S_IXUSR = 0o100
S_IRWXG = 0o070
S_IRGRP = 0o040
S_IWGRP = 0o020
S_IXGRP = 0o010
S_IRWXO = 0o007
S_IROTH = 0o004
S_IWOTH = 0o002
S_IXOTH = 0o001
S_ENFMT = S_ISGID
S_IREAD = S_IRUSR
S_IWRITE = S_IWUSR
S_IEXEC = S_IXUSR


def S_IMODE(mode):
    return mode & 0o7777


def S_IFMT(mode):
    return mode & S_IFMT_MASK


def S_ISDIR(mode):
    return S_IFMT(mode) == S_IFDIR


def S_ISCHR(mode):
    return S_IFMT(mode) == S_IFCHR


def S_ISBLK(mode):
    return S_IFMT(mode) == S_IFBLK


def S_ISREG(mode):
    return S_IFMT(mode) == S_IFREG


def S_ISFIFO(mode):
    return S_IFMT(mode) == S_IFIFO


def S_ISLNK(mode):
    return S_IFMT(mode) == S_IFLNK


def S_ISSOCK(mode):
    return S_IFMT(mode) == S_IFSOCK


def S_ISDOOR(mode):
    return False


def S_ISPORT(mode):
    return False


def S_ISWHT(mode):
    return False


_FILETYPE = {S_IFDIR: 'd', S_IFCHR: 'c', S_IFBLK: 'b', S_IFREG: '-', S_IFIFO: 'p', S_IFLNK: 'l', S_IFSOCK: 's'}


def filemode(mode):
    perm = [_FILETYPE.get(S_IFMT(mode), '?')]
    for who in range(3):
        shift = 6 - 3 * who
        bits = (mode >> shift) & 7
        perm.append('r' if bits & 4 else '-')
        perm.append('w' if bits & 2 else '-')
        x = bits & 1
        special = (mode & (S_ISUID >> who)) if who < 2 else (mode & S_ISVTX)
        if who == 0:
            perm.append(('s' if x else 'S') if special else ('x' if x else '-'))
        elif who == 1:
            perm.append(('s' if x else 'S') if special else ('x' if x else '-'))
        else:
            perm.append(('t' if x else 'T') if special else ('x' if x else '-'))
    return ''.join(perm)
