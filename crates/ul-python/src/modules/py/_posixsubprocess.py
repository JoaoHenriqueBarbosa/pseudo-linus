"""_posixsubprocess: o `fork_exec` que o `subprocess.py` do Debian chama. O filho nasce pelo `spawn` do kernel
(o `fork` seguido de `execve` do CPython), e uma falha do `exec` volta ao pai pelo `errpipe` no formato que o
`_execute_child` lê: `OSError:<errno em hexa>:noexec` ou `SubprocessError:0:<mensagem>`."""

import _os
import _sys
import errno as _errno
import os
import stat as _stat

_INT_MAX = (1 << 31) - 1
# O pid que `fork_exec` devolve quando o filho não chegou a existir: nenhum pid do kernel chega perto, então o
# `waitpid` que o `_execute_child` faz depois do erro acha `ECHILD` (que ele ignora).
_NO_CHILD = _INT_MAX


def _report(fd, err, message='noexec', kind='OSError'):
    """Escreve no `errpipe` o que o filho do CPython escreveria quando o `exec` (ou o que vem antes) falha."""
    try:
        os.write(fd, ('%s:%x:%s' % (kind, err, message)).encode())
    except OSError:
        pass


def _open_fds():
    """Os fds abertos do processo, pelo `/proc/self/fd` (o `_close_open_fds` do CPython lê o mesmo)."""
    try:
        return sorted(int(name) for name in _os.listdir('/proc/self/fd'))
    except (OSError, ValueError):
        return []


def _plan(close_fds, keep, p2cread, p2cwrite, c2pread, c2pwrite, errread, errwrite, errpipe_read):
    """Os ajustes de fd do filho, na ordem do `child_exec` do CPython: `(dups, closes)`. `dups` são pares
    `(de, para)` aplicados em sequência (`de == para` torna o fd herdável), `closes` os fds fechados depois deles.
    Os fds de saída das pontas do pai (`p2cwrite`, `c2pread`, `errread`, `errpipe_read`) e, com `close_fds`, todo
    fd acima de 2 que não está em `keep` fecham; as pontas do filho (`p2cread`, `c2pwrite`, `errwrite`) viram o
    stdio. Quem escreve no `errpipe` fecha o `errpipe_write` por conta própria."""
    stdio = [(src, dst) for src, dst in ((p2cread, 0), (c2pwrite, 1), (errwrite, 2)) if src >= 0]
    open_fds = _open_fds() if close_fds else []
    involved = [fd for fd in (p2cread, p2cwrite, c2pread, c2pwrite, errread, errwrite, errpipe_read) if fd >= 0]
    scratch = max(involved + open_fds + list(keep) + [2]) + 1
    # Uma ponta que já está em 0, 1 ou 2 e vai para outro lugar passa primeiro por um fd de rascunho, para
    # nenhuma cópia apagar o fonte de outra (`c2pwrite == 0`, `errwrite == 1`...).
    dups = [(fd, fd) for fd in sorted(keep)]
    copies = []
    moves = []
    scratches = []
    for src, dst in stdio:
        if src == dst:
            dups.append((src, dst))
        elif src <= 2:
            temp = scratch + len(scratches)
            scratches.append(temp)
            copies.append((src, temp))
            moves.append((temp, dst))
        else:
            moves.append((src, dst))
    dups = copies + dups + moves
    closes = set(scratches)
    closes.update(fd for fd in (p2cwrite, c2pread, errread, errpipe_read) if fd > 2)
    if close_fds:
        closes.update(fd for fd in open_fds if fd > 2 and fd not in keep)
    return dups, sorted(closes)


def _check_cwd(cwd):
    """O errno que o `chdir(cwd)` do filho daria (0 se der certo)."""
    try:
        mode = os.stat(cwd).st_mode
    except OSError as e:
        return e.errno
    if not _stat.S_ISDIR(mode):
        return _errno.ENOTDIR
    if not os.access(cwd, os.X_OK):
        return _errno.EACCES
    return 0


def _apply_ids(gid, extra_groups, uid):
    """`setgroups`, `setregid` e `setreuid` do filho, na ordem do CPython. O `spawn` não os leva ao processo
    novo: o pai assume as credenciais enquanto cria o filho (que as herda) e devolve as suas depois, a
    função devolvida. Sem privilégio, o kernel responde `EPERM` como responderia no filho."""
    undo = []
    try:
        if extra_groups is not None:
            groups = os.getgroups()
            os.setgroups(extra_groups)
            undo.append(lambda: os.setgroups(groups))
        if gid is not None:
            ids = os.getresgid()
            os.setresgid(gid, gid, ids[2])
            undo.append(lambda ids=ids: os.setresgid(*ids))
        if uid is not None:
            ids = os.getresuid()
            os.setresuid(uid, uid, ids[2])
            undo.append(lambda ids=ids: os.setresuid(*ids))
    except OSError:
        _undo_ids(undo)
        raise

    def restore():
        _undo_ids(undo)

    return restore


def _undo_ids(undo):
    # Ao contrário: o uid volta primeiro, porque devolver os grupos pede o privilégio que ele devolve.
    for step in reversed(undo):
        step()


def _restore_signals():
    """`_Py_RestoreSignals`: SIGPIPE, SIGXFSZ voltam ao padrão (o interpretador os ignora desde a partida)."""
    import signal
    for name in ('SIGPIPE', 'SIGXFSZ'):
        number = getattr(signal, name, None)
        if number is not None:
            signal.signal(number, signal.SIG_DFL)


def _exec_failure(errors):
    """O errno que o CPython relata depois de tentar todos os candidatos: o primeiro que não é `ENOENT` nem
    `ENOTDIR`, senão o da última tentativa."""
    saved = 0
    last = 0
    for err in errors:
        last = err
        if err not in (_errno.ENOENT, _errno.ENOTDIR) and not saved:
            saved = err
    return saved or last


def _validate(fds_to_keep, close_fds, errpipe_write):
    if not isinstance(fds_to_keep, tuple):
        raise TypeError('fork_exec() argument 4 must be tuple, not %s' % type(fds_to_keep).__name__)
    previous = -1
    for fd in fds_to_keep:
        if not isinstance(fd, int) or fd < 0 or fd > _INT_MAX or fd < previous:
            raise ValueError('bad value(s) in fds_to_keep')
        previous = fd
    if close_fds and errpipe_write < 3:
        raise ValueError('errpipe_write must be >= 3')


def fork_exec(*arguments):
    if len(arguments) != 23:
        raise TypeError('fork_exec expected 23 arguments, got %d' % len(arguments))
    (process_args, executable_list, close_fds, fds_to_keep, cwd, env_list, p2cread, p2cwrite, c2pread, c2pwrite,
     errread, errwrite, errpipe_read, errpipe_write, restore_signals, call_setsid, pgid_to_set, gid, extra_groups,
     uid, child_umask, preexec_fn, allow_vfork) = arguments
    _validate(fds_to_keep, close_fds, errpipe_write)
    argv = None if process_args is None else [os.fsencode(arg) for arg in process_args]
    cwd = None if cwd is None else os.fsencode(cwd)
    executables = [os.fsencode(path) for path in executable_list]
    keep = set(fds_to_keep)
    keep.discard(errpipe_write)
    dups, closes = _plan(close_fds, keep, p2cread, p2cwrite, c2pread, c2pwrite, errread, errwrite, errpipe_read)
    if preexec_fn is not None:
        return _fork_child(argv, executables, env_list, cwd, dups, closes, close_fds, errpipe_write,
                           restore_signals, call_setsid, pgid_to_set, gid, extra_groups, uid, child_umask,
                           preexec_fn)
    if cwd is not None:
        err = _check_cwd(cwd)
        if err:
            _report(errpipe_write, err, 'noexec:chdir')
            return _NO_CHILD
    closes = sorted(set(closes) | {errpipe_write})
    previous_umask = os.umask(child_umask) if child_umask >= 0 else None
    restore_ids = None
    try:
        try:
            restore_ids = _apply_ids(gid, extra_groups, uid)
        except OSError as e:
            _report(errpipe_write, e.errno)
            return _NO_CHILD
        errors = []
        for path in executables:
            # O `exec` do filho roda depois do `chdir`: um caminho relativo vale a partir do cwd novo.
            if cwd is not None and not path.startswith(b'/'):
                path = os.path.join(cwd, path)
            try:
                return _os.spawn(path, [path] if argv is None else argv, env_list, cwd, dups, closes,
                                 bool(restore_signals), bool(call_setsid), pgid_to_set)
            except OSError as e:
                if e.errno in (_errno.EAGAIN, _errno.ENOMEM):
                    # Não é falha do `exec`: o `fork` do CPython levanta direto, sem o nome do programa.
                    raise OSError(e.errno, os.strerror(e.errno)) from None
                errors.append(e.errno)
        _report(errpipe_write, _exec_failure(errors))
        return _NO_CHILD
    finally:
        if restore_ids is not None:
            restore_ids()
        if previous_umask is not None:
            os.umask(previous_umask)


def _fork_child(argv, executables, env_list, cwd, dups, closes, close_fds, errpipe_write, restore_signals,
                call_setsid, pgid_to_set, gid, extra_groups, uid, child_umask, preexec_fn):
    """O caminho do `preexec_fn`: ele roda no filho, então o filho nasce de `fork` (com os ganchos
    `register_at_fork`, sem a auditoria e o aviso de threads do `os.fork`) e vira o programa com `execve`."""
    pid = os._fork_with_hooks('fork', _os.fork, False)
    if pid != 0:
        return pid
    try:
        _child_exec(argv, executables, env_list, cwd, dups, closes, close_fds, errpipe_write, restore_signals,
                    call_setsid, pgid_to_set, gid, extra_groups, uid, child_umask, preexec_fn)
    finally:
        _os._exit(255)


def _child_exec(argv, executables, env_list, cwd, dups, closes, close_fds, errpipe_write, restore_signals,
                call_setsid, pgid_to_set, gid, extra_groups, uid, child_umask, preexec_fn):
    """O `child_exec` do CPython: só volta se algo falhou, depois de relatar no `errpipe`."""
    try:
        for src, dst in dups:
            _os.child_dup2(src, dst)
    except OSError as e:
        return _report(errpipe_write, e.errno)
    if cwd is not None:
        try:
            os.chdir(cwd)
        except OSError as e:
            return _report(errpipe_write, e.errno, 'noexec:chdir')
    if child_umask >= 0:
        os.umask(child_umask)
    if restore_signals:
        _restore_signals()
    try:
        if call_setsid:
            _os.setsid()
        if pgid_to_set >= 0:
            _os.setpgid(0, pgid_to_set)
        if extra_groups is not None:
            os.setgroups(extra_groups)
        if gid is not None:
            os.setregid(gid, gid)
        if uid is not None:
            os.setreuid(uid, uid)
    except OSError as e:
        return _report(errpipe_write, e.errno)
    try:
        preexec_fn()
    except BaseException:
        # Descrever a exceção pediria alocação no filho: o CPython só diz que houve.
        return _report(errpipe_write, 0, 'Exception occurred in preexec_fn.', 'SubprocessError')
    # Os fds fecham depois do `preexec_fn`, que pode ter aberto outros.
    for fd in closes:
        try:
            _os.close(fd)
        except OSError:
            pass
    errors = []
    for path in executables:
        try:
            _os.execve(path, [path] if argv is None else argv, env_list)
        except OSError as e:
            errors.append(e.errno)
    _report(errpipe_write, _exec_failure(errors))


fork_exec = _sys._builtin(fork_exec)
