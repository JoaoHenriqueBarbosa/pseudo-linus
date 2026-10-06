"""subprocess: processos filhos sobre `spawn`/`wait`/`pipe` do pseudo-processo."""

import _os
import io
import os
import shutil
import time as _time

__all__ = ['Popen', 'PIPE', 'STDOUT', 'DEVNULL', 'call', 'check_call', 'check_output', 'run',
           'CalledProcessError', 'TimeoutExpired', 'SubprocessError', 'CompletedProcess',
           'getoutput', 'getstatusoutput']

PIPE = -1
STDOUT = -2
DEVNULL = -3


class SubprocessError(Exception):
    pass


class CalledProcessError(SubprocessError):
    def __init__(self, returncode, cmd, output=None, stderr=None):
        self.returncode = returncode
        self.cmd = cmd
        self.output = output
        self.stderr = stderr

    def __str__(self):
        if self.returncode and self.returncode < 0:
            return "Command '%s' died with signal %d." % (self.cmd, -self.returncode)
        return "Command '%s' returned non-zero exit status %d." % (self.cmd, self.returncode)

    @property
    def stdout(self):
        return self.output

    @stdout.setter
    def stdout(self, value):
        self.output = value


class TimeoutExpired(SubprocessError):
    def __init__(self, cmd, timeout, output=None, stderr=None):
        self.cmd = cmd
        self.timeout = timeout
        self.output = output
        self.stderr = stderr

    def __str__(self):
        return "Command '%s' timed out after %s seconds" % (self.cmd, self.timeout)

    @property
    def stdout(self):
        return self.output

    @stdout.setter
    def stdout(self, value):
        self.output = value


def _fsencode(x):
    return x if isinstance(x, bytes) else os.fspath(x).encode('utf-8')


class Popen:
    def __init__(self, args, bufsize=-1, executable=None, stdin=None, stdout=None, stderr=None,
                 preexec_fn=None, close_fds=True, shell=False, cwd=None, env=None,
                 universal_newlines=None, startupinfo=None, creationflags=0, restore_signals=True,
                 start_new_session=False, pass_fds=(), *, text=None, encoding=None, errors=None,
                 user=None, group=None, extra_groups=None, umask=-1, pipesize=-1, process_group=None):
        self.args = args
        self.stdin = None
        self.stdout = None
        self.stderr = None
        self.pid = None
        self.returncode = None
        self.encoding = encoding
        self.errors = errors
        self.text_mode = bool(encoding or errors or text or universal_newlines)
        self._closed = []

        if isinstance(args, (str, bytes)) or hasattr(args, '__fspath__'):
            argv = [args] if not shell else None
        else:
            argv = list(args)
        if shell:
            cmd = args if isinstance(args, (str, bytes)) else ' '.join(args)
            argv = ['/bin/sh', '-c', cmd]
            program = executable or '/bin/sh'
        else:
            if not argv:
                raise IndexError('list index out of range')
            program = executable or argv[0]
        program = os.fspath(program)
        if not shell and isinstance(program, str) and '/' not in program:
            found = shutil.which(program, path=(env or os.environ).get('PATH') if env else None)
            if found is None:
                raise FileNotFoundError(2, 'No such file or directory', program)
            program = found
        elif not _os_exists(program):
            raise FileNotFoundError(2, 'No such file or directory', program)

        dups = []
        opened = []      # fds do pai que o filho herda por dup e que o pai fecha depois
        parent_ends = {}  # nome -> fd do pai

        def plan(target, value, writable_child):
            if value is None:
                return
            if value == PIPE:
                r, w = _os.pipe()
                child_fd, parent_fd = (r, w) if not writable_child else (w, r)
                parent_ends[target] = parent_fd
                opened.append(child_fd)
                dups.append((child_fd, target))
            elif value == DEVNULL:
                fd = _os.open('/dev/null', _os.O_RDONLY if not writable_child else _os.O_WRONLY, 0)
                opened.append(fd)
                dups.append((fd, target))
            elif value == STDOUT:
                dups.append((1, target))
            elif isinstance(value, int):
                dups.append((value, target))
            else:
                dups.append((value.fileno(), target))

        try:
            plan(0, stdin, False)
            plan(1, stdout, True)
            plan(2, stderr, True)
            # `stderr=STDOUT` precisa enxergar o stdout já redirecionado: dup2 em ordem resolve.
            envlist = None
            if env is not None:
                envlist = [_fsencode(k) + b'=' + _fsencode(v) for k, v in env.items()]
            cwd_arg = None if cwd is None else os.fspath(cwd)
            closes = [fd for fd in parent_ends.values()]
            self.pid = _os.spawn(program, [_fsencode(a) for a in argv], envlist, cwd_arg, dups, closes)
        finally:
            for fd in opened:
                try:
                    _os.close(fd)
                except OSError:
                    pass
        self._parent = parent_ends
        if 0 in parent_ends:
            self.stdin = self._wrap(parent_ends[0], 'w')
        if 1 in parent_ends:
            self.stdout = self._wrap(parent_ends[1], 'r')
        if 2 in parent_ends:
            self.stderr = self._wrap(parent_ends[2], 'r')

    def _wrap(self, fd, mode):
        if self.text_mode:
            return io.open(fd, mode, encoding=self.encoding, errors=self.errors)
        return io.open(fd, mode + 'b')

    def __repr__(self):
        return '<Popen: returncode: %s args: %r>' % (self.returncode, self.args)

    def poll(self):
        if self.returncode is None:
            r = _os.wait(self.pid, True)
            if r is not None:
                self.returncode = r[1]
        return self.returncode

    def wait(self, timeout=None):
        if self.returncode is not None:
            return self.returncode
        if timeout is None:
            r = _os.wait(self.pid, False)
            self.returncode = r[1]
            return self.returncode
        end = _time.monotonic() + timeout
        delay = 0.0005
        while self.poll() is None:
            left = end - _time.monotonic()
            if left <= 0:
                raise TimeoutExpired(self.args, timeout)
            _time.sleep(min(delay, left))
            delay = min(delay * 2, 0.05)
        return self.returncode

    def _close_pipes(self):
        for f in (self.stdin, self.stdout, self.stderr):
            if f is not None:
                try:
                    f.close()
                except (OSError, ValueError):
                    pass

    def communicate(self, input=None, timeout=None):
        if timeout is not None and self.returncode is None and self.stdin is None:
            # a saída fica no pipe até ser lida: esperar primeiro respeita o prazo
            self.wait(timeout)
        if self.stdin is not None:
            if input is not None:
                try:
                    self.stdin.write(input)
                    self.stdin.flush()
                except BrokenPipeError:
                    pass
            try:
                self.stdin.close()
            except BrokenPipeError:
                pass
        out = err = None
        if self.stdout is not None:
            out = self.stdout.read()
            self.stdout.close()
        if self.stderr is not None:
            err = self.stderr.read()
            self.stderr.close()
        self.wait(timeout)
        return out, err

    def send_signal(self, sig):
        self.poll()
        if self.returncode is None:
            _os.kill(self.pid, sig)

    def terminate(self):
        self.send_signal(15)

    def kill(self):
        self.send_signal(9)

    def __enter__(self):
        return self

    def __exit__(self, exc_type, value, traceback):
        self._close_pipes()
        if self.returncode is None:
            self.wait()


def _os_exists(path):
    try:
        os.stat(path)
        return True
    except OSError:
        return False


class CompletedProcess:
    def __init__(self, args, returncode, stdout=None, stderr=None):
        self.args = args
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = stderr

    def __repr__(self):
        args = ['args={!r}'.format(self.args), 'returncode={!r}'.format(self.returncode)]
        if self.stdout is not None:
            args.append('stdout={!r}'.format(self.stdout))
        if self.stderr is not None:
            args.append('stderr={!r}'.format(self.stderr))
        return 'CompletedProcess(' + ', '.join(args) + ')'

    def check_returncode(self):
        if self.returncode:
            raise CalledProcessError(self.returncode, self.args, self.stdout, self.stderr)


def run(*popenargs, input=None, capture_output=False, timeout=None, check=False, **kwargs):
    if input is not None:
        if kwargs.get('stdin') is not None:
            raise ValueError('stdin and input arguments may not both be used.')
        kwargs['stdin'] = PIPE
    if capture_output:
        if kwargs.get('stdout') is not None or kwargs.get('stderr') is not None:
            raise ValueError('stdout and stderr arguments may not be used with capture_output.')
        kwargs['stdout'] = PIPE
        kwargs['stderr'] = PIPE
    with Popen(*popenargs, **kwargs) as process:
        try:
            stdout, stderr = process.communicate(input, timeout=timeout)
        except TimeoutExpired as exc:
            process.kill()
            process.wait()
            raise TimeoutExpired(process.args, timeout, output=exc.output, stderr=exc.stderr)
        retcode = process.poll()
        if check and retcode:
            raise CalledProcessError(retcode, process.args, output=stdout, stderr=stderr)
    return CompletedProcess(process.args, retcode, stdout, stderr)


def call(*popenargs, timeout=None, **kwargs):
    with Popen(*popenargs, **kwargs) as p:
        try:
            return p.wait(timeout=timeout)
        except TimeoutExpired:
            p.kill()
            p.wait()
            raise


def check_call(*popenargs, **kwargs):
    retcode = call(*popenargs, **kwargs)
    if retcode:
        cmd = kwargs.get('args')
        if cmd is None:
            cmd = popenargs[0]
        raise CalledProcessError(retcode, cmd)
    return 0


def check_output(*popenargs, timeout=None, **kwargs):
    if 'stdout' in kwargs:
        raise ValueError('stdout argument not allowed, it will be overridden.')
    if 'input' in kwargs and kwargs['input'] is None:
        kwargs['input'] = '' if kwargs.get('universal_newlines') or kwargs.get('text') or \
            kwargs.get('encoding') or kwargs.get('errors') else b''
    return run(*popenargs, stdout=PIPE, timeout=timeout, check=True, **kwargs).stdout


def getstatusoutput(cmd, *, encoding=None, errors=None):
    try:
        data = check_output(cmd, shell=True, text=True, stderr=STDOUT, encoding=encoding, errors=errors)
        exitcode = 0
    except CalledProcessError as ex:
        data = ex.output
        exitcode = ex.returncode
    if data[-1:] == '\n':
        data = data[:-1]
    return exitcode, data


def getoutput(cmd, *, encoding=None, errors=None):
    return getstatusoutput(cmd, encoding=encoding, errors=errors)[1]
