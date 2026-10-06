"""asyncio.subprocess: processos filhos sobre `subprocess.Popen`. Os pipes do pai ficam não bloqueantes e as
leituras/escritas esperam com `asyncio.sleep` curto (o laço de eventos não tem seletor de descritores)."""

__all__ = ('create_subprocess_exec', 'create_subprocess_shell')

import os
import subprocess as _sp

from . import events
from . import tasks
from .exceptions import IncompleteReadError, LimitOverrunError

PIPE = _sp.PIPE
STDOUT = _sp.STDOUT
DEVNULL = _sp.DEVNULL

_POLL = 0.002
_LIMIT = 2 ** 16


class StreamReader:
    """Lado de leitura de um pipe de filho (API de `asyncio.StreamReader`)."""

    def __init__(self, fileobj, limit=_LIMIT):
        self._file = fileobj
        self._fd = fileobj.fileno()
        self._limit = limit
        self._buf = bytearray()
        self._eof = False
        self._exception = None
        os.set_blocking(self._fd, False)

    def exception(self):
        return self._exception

    def at_eof(self):
        return self._eof and not self._buf

    def feed_data(self, data):
        self._buf += data

    async def _fill(self):
        """Lê o que houver; cede ao laço enquanto o pipe está vazio. `False` no fim do arquivo."""
        spins = 0
        while True:
            if self._eof:
                return False
            try:
                data = os.read(self._fd, _LIMIT)
            except BlockingIOError:
                spins += 1
                await tasks.sleep(0 if spins < 4 else _POLL)
                continue
            if not data:
                self._eof = True
                return False
            self._buf += data
            return True

    async def read(self, n=-1):
        if n == 0:
            return b''
        if n < 0:
            while await self._fill():
                pass
            data = bytes(self._buf)
            self._buf.clear()
            return data
        if not self._buf and not self._eof:
            await self._fill()
        data = bytes(self._buf[:n])
        del self._buf[:n]
        return data

    async def readexactly(self, n):
        if n < 0:
            raise ValueError('readexactly size can not be less than zero')
        while len(self._buf) < n:
            if not await self._fill():
                partial = bytes(self._buf)
                self._buf.clear()
                raise IncompleteReadError(partial, n)
        data = bytes(self._buf[:n])
        del self._buf[:n]
        return data

    async def readuntil(self, separator=b'\n'):
        if not separator:
            raise ValueError('Separator should be at least one-byte string')
        start = 0
        while True:
            i = self._buf.find(separator, start)
            if i >= 0:
                end = i + len(separator)
                if end > self._limit:
                    raise LimitOverrunError('Separator is found, but chunk is longer than limit', i)
                data = bytes(self._buf[:end])
                del self._buf[:end]
                return data
            if len(self._buf) > self._limit:
                raise LimitOverrunError('Separator is not found, and chunk exceed the limit', len(self._buf))
            start = max(0, len(self._buf) - len(separator) + 1)
            if not await self._fill():
                partial = bytes(self._buf)
                self._buf.clear()
                raise IncompleteReadError(partial, None)

    async def readline(self):
        try:
            return await self.readuntil(b'\n')
        except IncompleteReadError as e:
            return e.partial
        except LimitOverrunError as e:
            consumed = e.consumed
            data = bytes(self._buf[:consumed])
            del self._buf[:consumed]
            raise ValueError(e.args[0])

    def __aiter__(self):
        return self

    async def __anext__(self):
        line = await self.readline()
        if line == b'':
            raise StopAsyncIteration
        return line

    def _close(self):
        try:
            self._file.close()
        except OSError:
            pass


class StreamWriter:
    """Lado de escrita do stdin do filho: `write` tenta na hora, `drain` espera o resto caber no pipe."""

    def __init__(self, fileobj):
        self._file = fileobj
        self._fd = fileobj.fileno()
        self._pending = bytearray()
        self._closing = False
        self._closed = False
        os.set_blocking(self._fd, False)

    def _flush(self):
        while self._pending and not self._closed:
            try:
                n = os.write(self._fd, bytes(self._pending))
            except BlockingIOError:
                return
            except BrokenPipeError:
                self._pending.clear()
                return
            del self._pending[:n]

    def write(self, data):
        if self._closing:
            raise RuntimeError('Cannot write to closing transport')
        self._pending += data
        self._flush()

    def writelines(self, data):
        for chunk in data:
            self.write(chunk)

    def can_write_eof(self):
        return True

    def write_eof(self):
        self.close()

    def is_closing(self):
        return self._closing

    def get_extra_info(self, name, default=None):
        return default

    async def drain(self):
        spins = 0
        while self._pending and not self._closed:
            self._flush()
            if self._pending:
                spins += 1
                await tasks.sleep(0 if spins < 4 else _POLL)
        await tasks.sleep(0)

    def close(self):
        self._closing = True
        self._flush()
        if not self._pending:
            self._really_close()

    def _really_close(self):
        if not self._closed:
            self._closed = True
            try:
                self._file.close()
            except OSError:
                pass

    async def wait_closed(self):
        await self.drain()
        self._really_close()


class Process:
    def __init__(self, popen, stdin, stdout, stderr):
        self._popen = popen
        self.pid = popen.pid
        self.stdin = stdin
        self.stdout = stdout
        self.stderr = stderr

    def __repr__(self):
        return '<%s %s>' % (self.__class__.__name__, self.pid)

    @property
    def returncode(self):
        self._popen.poll()
        return self._popen.returncode

    async def wait(self):
        spins = 0
        while self._popen.poll() is None:
            spins += 1
            await tasks.sleep(0 if spins < 4 else _POLL)
        return self._popen.returncode

    def send_signal(self, signal):
        self._popen.send_signal(signal)

    def terminate(self):
        self._popen.terminate()

    def kill(self):
        self._popen.kill()

    async def _feed_stdin(self, input):
        if input:
            self.stdin.write(input)
        try:
            await self.stdin.drain()
        except (BrokenPipeError, ConnectionResetError):
            pass
        self.stdin.close()

    async def _read_stream(self, stream):
        data = await stream.read()
        stream._close()
        return data

    async def communicate(self, input=None):
        coros = []
        if self.stdin is not None:
            coros.append(self._feed_stdin(input))
        out_idx = err_idx = None
        if self.stdout is not None:
            out_idx = len(coros)
            coros.append(self._read_stream(self.stdout))
        if self.stderr is not None:
            err_idx = len(coros)
            coros.append(self._read_stream(self.stderr))
        results = await tasks.gather(*coros) if coros else []
        await self.wait()
        stdout = results[out_idx] if out_idx is not None else None
        stderr = results[err_idx] if err_idx is not None else None
        return stdout, stderr


def _spawn(argv, shell, stdin, stdout, stderr, limit, kwds):
    popen = _sp.Popen(argv, shell=shell, stdin=stdin, stdout=stdout, stderr=stderr, **kwds)
    writer = StreamWriter(popen.stdin) if popen.stdin is not None else None
    out = StreamReader(popen.stdout, limit) if popen.stdout is not None else None
    err = StreamReader(popen.stderr, limit) if popen.stderr is not None else None
    return Process(popen, writer, out, err)


async def create_subprocess_shell(cmd, stdin=None, stdout=None, stderr=None, limit=_LIMIT, **kwds):
    return _spawn(cmd, True, stdin, stdout, stderr, limit, kwds)


async def create_subprocess_exec(program, *args, stdin=None, stdout=None, stderr=None, limit=_LIMIT, **kwds):
    return _spawn([program, *args], False, stdin, stdout, stderr, limit, kwds)
