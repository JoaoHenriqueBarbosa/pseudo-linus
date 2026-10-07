'This module provides access to operating system functionality that is\nstandardized by the C Standard and the POSIX standard (a thinly\ndisguised Unix interface).  Refer to the library manual and\ncorresponding Unix manual entries for more information on calls.'


def _init(names=['CLD_CONTINUED', 'CLD_DUMPED', 'CLD_EXITED', 'CLD_KILLED', 'CLD_STOPPED', 'CLD_TRAPPED', 'CLONE_FILES',
    'CLONE_FS', 'CLONE_NEWCGROUP', 'CLONE_NEWIPC', 'CLONE_NEWNET', 'CLONE_NEWNS', 'CLONE_NEWPID',
    'CLONE_NEWTIME', 'CLONE_NEWUSER', 'CLONE_NEWUTS', 'CLONE_SIGHAND', 'CLONE_SYSVSEM', 'CLONE_THREAD',
    'CLONE_VM', 'DirEntry', 'EFD_CLOEXEC', 'EFD_NONBLOCK', 'EFD_SEMAPHORE', 'EX_CANTCREAT', 'EX_CONFIG',
    'EX_DATAERR', 'EX_IOERR', 'EX_NOHOST', 'EX_NOINPUT', 'EX_NOPERM', 'EX_NOUSER', 'EX_OK', 'EX_OSERR',
    'EX_OSFILE', 'EX_PROTOCOL', 'EX_SOFTWARE', 'EX_TEMPFAIL', 'EX_UNAVAILABLE', 'EX_USAGE', 'F_LOCK', 'F_OK',
    'F_TEST', 'F_TLOCK', 'F_ULOCK', 'GRND_NONBLOCK', 'GRND_RANDOM', 'MFD_ALLOW_SEALING', 'MFD_CLOEXEC',
    'MFD_HUGETLB', 'MFD_HUGE_16GB', 'MFD_HUGE_16MB', 'MFD_HUGE_1GB', 'MFD_HUGE_1MB', 'MFD_HUGE_256MB',
    'MFD_HUGE_2GB', 'MFD_HUGE_2MB', 'MFD_HUGE_32MB', 'MFD_HUGE_512KB', 'MFD_HUGE_512MB', 'MFD_HUGE_64KB',
    'MFD_HUGE_8MB', 'MFD_HUGE_MASK', 'MFD_HUGE_SHIFT', 'NGROUPS_MAX', 'O_ACCMODE', 'O_APPEND', 'O_ASYNC',
    'O_CLOEXEC', 'O_CREAT', 'O_DIRECT', 'O_DIRECTORY', 'O_DSYNC', 'O_EXCL', 'O_FSYNC', 'O_LARGEFILE',
    'O_NDELAY', 'O_NOATIME', 'O_NOCTTY', 'O_NOFOLLOW', 'O_NONBLOCK', 'O_PATH', 'O_RDONLY', 'O_RDWR',
    'O_RSYNC', 'O_SYNC', 'O_TMPFILE', 'O_TRUNC', 'O_WRONLY', 'PIDFD_NONBLOCK', 'POSIX_FADV_DONTNEED',
    'POSIX_FADV_NOREUSE', 'POSIX_FADV_NORMAL', 'POSIX_FADV_RANDOM', 'POSIX_FADV_SEQUENTIAL',
    'POSIX_FADV_WILLNEED', 'POSIX_SPAWN_CLOSE', 'POSIX_SPAWN_CLOSEFROM', 'POSIX_SPAWN_DUP2',
    'POSIX_SPAWN_OPEN', 'PRIO_PGRP', 'PRIO_PROCESS', 'PRIO_USER', 'P_ALL', 'P_PGID', 'P_PID', 'P_PIDFD',
    'RTLD_DEEPBIND', 'RTLD_GLOBAL', 'RTLD_LAZY', 'RTLD_LOCAL', 'RTLD_NODELETE', 'RTLD_NOLOAD', 'RTLD_NOW',
    'RWF_APPEND', 'RWF_DSYNC', 'RWF_HIPRI', 'RWF_NOWAIT', 'RWF_SYNC', 'R_OK', 'SCHED_BATCH', 'SCHED_FIFO',
    'SCHED_IDLE', 'SCHED_OTHER', 'SCHED_RESET_ON_FORK', 'SCHED_RR', 'SEEK_DATA', 'SEEK_HOLE', 'SPLICE_F_MORE',
    'SPLICE_F_MOVE', 'SPLICE_F_NONBLOCK', 'ST_APPEND', 'ST_MANDLOCK', 'ST_NOATIME', 'ST_NODEV',
    'ST_NODIRATIME', 'ST_NOEXEC', 'ST_NOSUID', 'ST_RDONLY', 'ST_RELATIME', 'ST_SYNCHRONOUS', 'ST_WRITE',
    'TFD_CLOEXEC', 'TFD_NONBLOCK', 'TFD_TIMER_ABSTIME', 'TFD_TIMER_CANCEL_ON_SET', 'TMP_MAX', 'WCONTINUED',
    'WCOREDUMP', 'WEXITED', 'WEXITSTATUS', 'WIFCONTINUED', 'WIFEXITED', 'WIFSIGNALED', 'WIFSTOPPED',
    'WNOHANG', 'WNOWAIT', 'WSTOPPED', 'WSTOPSIG', 'WTERMSIG', 'WUNTRACED', 'W_OK', 'XATTR_CREATE',
    'XATTR_REPLACE', 'XATTR_SIZE_MAX', 'X_OK', '_exit', 'abort', 'access', 'chdir', 'chmod',
    'chown', 'chroot', 'close', 'closerange', 'confstr', 'confstr_names', 'copy_file_range', 'cpu_count',
    'ctermid', 'device_encoding', 'dup', 'dup2', 'error', 'eventfd', 'eventfd_read',
    'eventfd_write', 'execv', 'execve', 'fchdir', 'fchmod', 'fchown', 'fdatasync', 'fork', 'forkpty',
    'fpathconf', 'fspath', 'fstat', 'fstatvfs', 'fsync', 'ftruncate', 'get_blocking', 'get_inheritable',
    'get_terminal_size', 'getcwd', 'getcwdb', 'getegid', 'geteuid', 'getgid', 'getgrouplist', 'getgroups',
    'getloadavg', 'getlogin', 'getpgid', 'getpgrp', 'getpid', 'getppid', 'getpriority', 'getrandom',
    'getresgid', 'getresuid', 'getsid', 'getuid', 'getxattr', 'grantpt', 'initgroups', 'isatty', 'kill',
    'killpg', 'lchown', 'link', 'listdir', 'listxattr', 'lockf', 'login_tty', 'lseek', 'lstat', 'major',
    'makedev', 'memfd_create', 'minor', 'mkdir', 'mkfifo', 'mknod', 'nice', 'open', 'openpty', 'pathconf',
    'pathconf_names', 'pidfd_open', 'pipe', 'pipe2', 'posix_fadvise', 'posix_fallocate', 'posix_openpt',
    'posix_spawn', 'posix_spawnp', 'pread', 'preadv', 'ptsname', 'putenv', 'pwrite', 'pwritev', 'read',
    'readlink', 'readv', 'register_at_fork', 'remove', 'removexattr', 'rename', 'replace', 'rmdir', 'scandir',
    'sched_get_priority_max', 'sched_get_priority_min', 'sched_getaffinity', 'sched_getparam',
    'sched_getscheduler', 'sched_param', 'sched_rr_get_interval', 'sched_setaffinity', 'sched_setparam',
    'sched_setscheduler', 'sched_yield', 'sendfile', 'set_blocking', 'set_inheritable', 'setegid', 'seteuid',
    'setgid', 'setgroups', 'setns', 'setpgid', 'setpgrp', 'setpriority', 'setregid', 'setresgid', 'setresuid',
    'setreuid', 'setsid', 'setuid', 'setxattr', 'splice', 'stat', 'stat_result', 'statvfs', 'statvfs_result',
    'strerror', 'symlink', 'sync', 'sysconf', 'sysconf_names', 'system', 'tcgetpgrp', 'tcsetpgrp',
    'terminal_size', 'timerfd_create', 'timerfd_gettime', 'timerfd_gettime_ns', 'timerfd_settime',
    'timerfd_settime_ns', 'times', 'times_result', 'truncate', 'ttyname', 'umask', 'uname', 'uname_result',
    'unlink', 'unlockpt', 'unsetenv', 'unshare', 'urandom', 'utime', 'wait', 'wait3', 'wait4', 'waitid',
    'waitid_result', 'waitpid', 'waitstatus_to_exitcode', 'write', 'writev']):
    # As funções e constantes são as mesmas do `os` (que no CPython as importa daqui com
    # `from posix import *`), então `posix.getpid is os.getpid`. O `os` chama isto ao terminar de
    # carregar, porque o `posixpath` importa este módulo antes disso.
    import os, sys
    module = sys.modules[__name__]
    for name in names:
        value = getattr(os, name, module)
        if value is not module:
            setattr(module, name, value)
    # O ambiente do processo na partida, em bytes (o mesmo dict do `os.environ._data`).
    module.environ = os.environ._data


def _path_splitroot_ex(p):
    """Split a pathname into drive, root and tail.

    The tail contains anything after the root."""
    if not isinstance(p, (str, bytes)):
        fspath = getattr(type(p), '__fspath__', None)
        if fspath is None:
            raise TypeError('expected str, bytes or os.PathLike object, not ' + type(p).__name__)
        result = fspath(p)
        if not isinstance(result, (str, bytes)):
            raise TypeError('expected %s.__fspath__() to return str or bytes, not %s'
                            % (type(p).__name__, type(result).__name__))
        p = result
    if isinstance(p, bytes):
        sep = b'/'
        empty = b''
    else:
        sep = '/'
        empty = ''
    if p[:1] != sep:
        return empty, empty, p
    elif p[1:2] != sep or p[2:3] == sep:
        return empty, sep, p[1:]
    else:
        return empty, p[:2], p[2:]


def _path_normpath(path):
    """Normalize path, eliminating double slashes, etc."""
    _, initial_slashes, path = _path_splitroot_ex(path)
    if isinstance(path, bytes):
        sep = b'/'
        dot = b'.'
        dotdot = b'..'
    else:
        sep = '/'
        dot = '.'
        dotdot = '..'
    if not path and not initial_slashes:
        return dot
    comps = path.split(sep)
    new_comps = []
    for comp in comps:
        if not comp or comp == dot:
            continue
        if (comp != dotdot or (not initial_slashes and not new_comps) or
             (new_comps and new_comps[-1] == dotdot)):
            new_comps.append(comp)
        elif new_comps:
            new_comps.pop()
    path = initial_slashes + sep.join(new_comps)
    return path or dot


def _inputhook():
    """Calls PyOS_CallInputHook droppping the GIL first"""
    return None


def _is_inputhook_installed():
    """Check if PyOS_InputHook is set"""
    return False

_have_functions = ['HAVE_EVENTFD', 'HAVE_TIMERFD_CREATE', 'HAVE_FACCESSAT', 'HAVE_FCHDIR', 'HAVE_FCHMOD',
    'HAVE_FCHMODAT', 'HAVE_FCHOWN', 'HAVE_FCHOWNAT', 'HAVE_FEXECVE', 'HAVE_FDOPENDIR', 'HAVE_FPATHCONF',
    'HAVE_FSTATAT', 'HAVE_FSTATVFS', 'HAVE_FTRUNCATE', 'HAVE_FUTIMENS', 'HAVE_FUTIMES', 'HAVE_FUTIMESAT',
    'HAVE_LINKAT', 'HAVE_LCHOWN', 'HAVE_LSTAT', 'HAVE_LUTIMES', 'HAVE_MEMFD_CREATE', 'HAVE_MKDIRAT',
    'HAVE_MKFIFOAT', 'HAVE_MKNODAT', 'HAVE_OPENAT', 'HAVE_READLINKAT', 'HAVE_RENAMEAT', 'HAVE_SYMLINKAT',
    'HAVE_UNLINKAT', 'HAVE_UTIMENSAT', 'HAVE_PTSNAME_R']

# O `posixpath` importa este módulo no meio da carga do `os`: aí o `os` recebe o `_init` e o chama
# ao terminar. Importado sozinho, o `os` vem agora e o `_init` roda em seguida.
if 'os' in __import__('sys').modules:
    __import__('sys').modules['os']._posix_init = _init
else:
    __import__('os')
    _init()
