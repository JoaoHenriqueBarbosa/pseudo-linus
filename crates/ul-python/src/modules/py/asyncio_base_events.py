"""Laço de eventos do asyncio sem selector nem sockets: fila de prontos, temporizadores em heap e
`time.sleep` quando não há nada a fazer. A semântica de agendamento é a do `BaseEventLoop` do CPython
(`_run_once`), então tarefas, futuros e temporizadores se intercalam na mesma ordem."""

import collections
import concurrent.futures
import heapq
import itertools
import sys
import threading
import time
import weakref

from . import constants
from . import coroutines
from . import events
from . import exceptions
from . import futures
from . import loopback
from . import tasks
from . import trsock
from .log import logger

__all__ = 'BaseEventLoop', 'Server'

# Menor intervalo de espera por passo do laço; também o "relógio" mínimo dos temporizadores.
MAXIMUM_SELECT_TIMEOUT = 24 * 3600

_MIN_SCHEDULED_TIMER_HANDLES = 100
_MIN_CANCELLED_TIMER_HANDLES_FRACTION = 0.5


class Server(events.AbstractServer):

    def __init__(self, loop, sockets, protocol_factory, ssl_context, backlog,
                 ssl_handshake_timeout, ssl_shutdown_timeout=None):
        self._loop = loop
        self._sockets = sockets
        self._clients = weakref.WeakSet()
        self._waiters = []
        self._protocol_factory = protocol_factory
        self._backlog = backlog
        self._ssl_context = ssl_context
        self._ssl_handshake_timeout = ssl_handshake_timeout
        self._ssl_shutdown_timeout = ssl_shutdown_timeout
        self._serving = False
        self._serving_forever_fut = None

    def __repr__(self):
        return f'<{self.__class__.__name__} sockets={self.sockets!r}>'

    def _attach(self, transport):
        assert self._sockets is not None
        self._clients.add(transport)

    def _detach(self, transport):
        self._clients.discard(transport)
        if len(self._clients) == 0 and self._sockets is None:
            self._wakeup()

    def _wakeup(self):
        waiters = self._waiters
        self._waiters = None
        for waiter in waiters:
            if not waiter.done():
                waiter.set_result(None)

    def _start_serving(self):
        if self._serving:
            return
        self._serving = True
        for sock in self._sockets:
            sock.listen(self._backlog)
            self._loop._start_serving(
                self._protocol_factory, sock, self._ssl_context,
                self, self._backlog, self._ssl_handshake_timeout,
                self._ssl_shutdown_timeout)

    def get_loop(self):
        return self._loop

    def is_serving(self):
        return self._serving

    @property
    def sockets(self):
        if self._sockets is None:
            return ()
        return tuple(trsock.TransportSocket(s) for s in self._sockets)

    def close(self):
        sockets = self._sockets
        if sockets is None:
            return
        self._sockets = None

        for sock in sockets:
            self._loop._stop_serving(sock)

        self._serving = False

        if (self._serving_forever_fut is not None and
                not self._serving_forever_fut.done()):
            self._serving_forever_fut.cancel()
            self._serving_forever_fut = None

        if len(self._clients) == 0:
            self._wakeup()

    def close_clients(self):
        for transport in self._clients.copy():
            transport.close()

    def abort_clients(self):
        for transport in self._clients.copy():
            transport.abort()

    async def start_serving(self):
        self._start_serving()
        await tasks.sleep(0)

    async def serve_forever(self):
        if self._serving_forever_fut is not None:
            raise RuntimeError(
                f'server {self!r} is already being awaited on serve_forever()')
        if self._sockets is None:
            raise RuntimeError(f'server {self!r} is closed')

        self._start_serving()
        self._serving_forever_fut = self._loop.create_future()

        try:
            await self._serving_forever_fut
        except exceptions.CancelledError:
            try:
                self.close()
                await self.wait_closed()
            finally:
                raise
        finally:
            self._serving_forever_fut = None

    async def wait_closed(self):
        """Espera o servidor fechar e todas as conexões caírem (`_wakeup` zera `_waiters` ao cumprir as duas)."""
        if self._waiters is None:
            return
        waiter = self._loop.create_future()
        self._waiters.append(waiter)
        await waiter


class BaseEventLoop(loopback.NetworkMixin, events.AbstractEventLoop):

    def __init__(self):
        self._timer_cancelled_count = 0
        self._closed = False
        self._stopping = False
        self._ready = collections.deque()
        self._io_ready = collections.deque()
        self._scheduled = []
        self._default_executor = None
        self._internal_fds = 0
        self._thread_id = None
        self._clock_resolution = time.get_clock_info('monotonic').resolution
        self._exception_handler = None
        self.set_debug(False)
        self._current_handle = None
        self._task_factory = None
        self._asyncgens = None
        self._asyncgens_shutdown_called = False
        self._executor_shutdown_called = False

    def __repr__(self):
        return (f'<{self.__class__.__name__} running={self.is_running()} '
                f'closed={self.is_closed()} debug={self.get_debug()}>')

    def create_future(self):
        return futures.Future(loop=self)

    def create_task(self, coro, *, name=None, context=None):
        self._check_closed()
        if self._task_factory is None:
            task = tasks.Task(coro, loop=self, name=name, context=context)
            if task._source_traceback:
                del task._source_traceback[-1]
        else:
            if context is None:
                task = self._task_factory(self, coro)
            else:
                task = self._task_factory(self, coro, context=context)
            tasks._set_task_name(task, name)
        return task

    def set_task_factory(self, factory):
        if factory is not None and not callable(factory):
            raise TypeError('task factory must be a callable or None')
        self._task_factory = factory

    def get_task_factory(self):
        return self._task_factory

    def _check_closed(self):
        if self._closed:
            raise RuntimeError('Event loop is closed')

    def _check_default_executor(self):
        if self._executor_shutdown_called:
            raise RuntimeError('Executor shutdown has been called')

    def _check_running(self):
        if self.is_running():
            raise RuntimeError('This event loop is already running')
        if events._get_running_loop() is not None:
            raise RuntimeError('Cannot run the event loop while another loop is running')

    def run_forever(self):
        self._check_closed()
        self._check_running()
        self._thread_id = threading.get_ident()
        events._set_running_loop(self)
        try:
            while True:
                self._run_once()
                if self._stopping:
                    break
        finally:
            self._stopping = False
            self._thread_id = None
            events._set_running_loop(None)

    def run_until_complete(self, future):
        self._check_closed()
        self._check_running()
        new_task = not futures.isfuture(future)
        future = tasks.ensure_future(future, loop=self)
        if new_task:
            # Evita o aviso "Task exception was never retrieved" do `run_until_complete`.
            future._log_destroy_pending = False
        future.add_done_callback(_run_until_complete_cb)
        try:
            self.run_forever()
        except:
            if new_task and future.done() and not future.cancelled():
                future.exception()
            raise
        finally:
            future.remove_done_callback(_run_until_complete_cb)
        if not future.done():
            raise RuntimeError('Event loop stopped before Future completed.')
        return future.result()

    def stop(self):
        self._stopping = True

    def close(self):
        if self.is_running():
            raise RuntimeError("Cannot close a running event loop")
        if self._closed:
            return
        self._closed = True
        self._ready.clear()
        self._scheduled.clear()
        self._executor_shutdown_called = True
        executor = self._default_executor
        if executor is not None:
            self._default_executor = None
            executor.shutdown(wait=False)

    def is_closed(self):
        return self._closed

    def is_running(self):
        return self._thread_id is not None

    def time(self):
        return time.monotonic()

    # --- agendamento ---------------------------------------------------------------------------

    def call_later(self, delay, callback, *args, context=None):
        if delay is None:
            raise TypeError('delay must not be None')
        timer = self.call_at(self.time() + delay, callback, *args, context=context)
        if timer._source_traceback:
            del timer._source_traceback[-1]
        return timer

    def call_at(self, when, callback, *args, context=None):
        if when is None:
            raise TypeError("when cannot be None")
        self._check_closed()
        timer = events.TimerHandle(when, callback, args, self, context)
        if timer._source_traceback:
            del timer._source_traceback[-1]
        heapq.heappush(self._scheduled, timer)
        timer._scheduled = True
        return timer

    def call_soon(self, callback, *args, context=None):
        self._check_closed()
        handle = self._call_soon(callback, args, context)
        if handle._source_traceback:
            del handle._source_traceback[-1]
        return handle

    def _call_soon(self, callback, args, context):
        handle = events.Handle(callback, args, self, context)
        if handle._source_traceback:
            del handle._source_traceback[-1]
        self._ready.append(handle)
        return handle

    def call_soon_threadsafe(self, callback, *args, context=None):
        self._check_closed()
        handle = self._call_soon(callback, args, context)
        if handle._source_traceback:
            del handle._source_traceback[-1]
        return handle

    def _timer_handle_cancelled(self, handle):
        if handle._scheduled:
            self._timer_cancelled_count += 1

    # --- executor ------------------------------------------------------------------------------

    def run_in_executor(self, executor, func, *args):
        self._check_closed()
        if executor is None:
            executor = self._default_executor
            self._check_default_executor()
            if executor is None:
                executor = concurrent.futures.ThreadPoolExecutor(thread_name_prefix='asyncio')
                self._default_executor = executor
        return futures.wrap_future(executor.submit(func, *args), loop=self)

    def set_default_executor(self, executor):
        if not hasattr(executor, 'submit'):
            raise TypeError('executor must be a concurrent.futures.Executor')
        self._default_executor = executor

    async def shutdown_asyncgens(self):
        self._asyncgens_shutdown_called = True

    async def shutdown_default_executor(self, timeout=None):
        self._executor_shutdown_called = True
        if self._default_executor is None:
            return
        executor, self._default_executor = self._default_executor, None
        executor.shutdown(wait=False)

    # --- tratamento de erros -------------------------------------------------------------------

    def get_exception_handler(self):
        return self._exception_handler

    def set_exception_handler(self, handler):
        if handler is not None and not callable(handler):
            raise TypeError(f'A callable object or None is expected, got {handler!r}')
        self._exception_handler = handler

    def default_exception_handler(self, context):
        message = context.get('message')
        if not message:
            message = 'Unhandled exception in event loop'
        exception = context.get('exception')
        exc_info = (type(exception), exception, exception.__traceback__) if exception is not None else False
        log_lines = [message]
        for key in sorted(context):
            if key in {'message', 'exception'}:
                continue
            value = context[key]
            if key == 'source_traceback':
                tb = ''.join(__import__('traceback').format_list(value))
                value = 'Object created at (most recent call last):\n' + tb.rstrip()
            elif key == 'handle_traceback':
                tb = ''.join(__import__('traceback').format_list(value))
                value = 'Handle created at (most recent call last):\n' + tb.rstrip()
            else:
                value = repr(value)
            log_lines.append(f'{key}: {value}')
        logger.error('\n'.join(log_lines), exc_info=exc_info)

    def call_exception_handler(self, context):
        if self._exception_handler is None:
            try:
                self.default_exception_handler(context)
            except (SystemExit, KeyboardInterrupt):
                raise
            except BaseException:
                logger.error('Exception in default exception handler', exc_info=True)
        else:
            try:
                ctx = None
                thing = context.get('task')
                if thing is None:
                    thing = context.get('future')
                if thing is None:
                    thing = context.get('handle')
                if thing is not None and hasattr(thing, 'get_context'):
                    ctx = thing.get_context()
                if ctx is not None and hasattr(ctx, 'run'):
                    ctx.run(self._exception_handler, self, context)
                else:
                    self._exception_handler(self, context)
            except (SystemExit, KeyboardInterrupt):
                raise
            except BaseException as exc:
                try:
                    self.default_exception_handler({
                        'message': 'Unhandled error in exception handler',
                        'exception': exc,
                        'context': context,
                    })
                except (SystemExit, KeyboardInterrupt):
                    raise
                except BaseException:
                    logger.error('Exception in default exception handler while handling an unexpected error in custom exception handler', exc_info=True)

    def _add_callback(self, handle):
        if not handle._cancelled:
            self._ready.append(handle)

    def _add_callback_signalsafe(self, handle):
        self._add_callback(handle)

    # --- depuração -----------------------------------------------------------------------------

    def get_debug(self):
        return self._debug

    def set_debug(self, enabled):
        self._debug = enabled

    # --- o passo do laço -----------------------------------------------------------------------

    def _run_once(self):
        sched_count = len(self._scheduled)
        if (sched_count > _MIN_SCHEDULED_TIMER_HANDLES and
                self._timer_cancelled_count / sched_count > _MIN_CANCELLED_TIMER_HANDLES_FRACTION):
            new_scheduled = []
            for handle in self._scheduled:
                if handle._cancelled:
                    handle._scheduled = False
                else:
                    new_scheduled.append(handle)
            heapq.heapify(new_scheduled)
            self._scheduled = new_scheduled
            self._timer_cancelled_count = 0
        else:
            while self._scheduled and self._scheduled[0]._cancelled:
                self._timer_cancelled_count -= 1
                handle = heapq.heappop(self._scheduled)
                handle._scheduled = False

        timeout = None
        if self._ready or self._stopping or self._io_ready:
            timeout = 0
        elif self._scheduled:
            when = self._scheduled[0]._when
            timeout = min(max(0, when - self.time()), MAXIMUM_SELECT_TIMEOUT)

        if timeout is None:
            # Sem prontos nem temporizadores o laço ficaria parado para sempre (não há I/O a esperar).
            raise RuntimeError('event loop has nothing to wait for: no ready callbacks, timers or I/O')
        if timeout > 0:
            time.sleep(timeout)

        # O `select` do CPython devolve os eventos de I/O depois dos prontos que já estavam na fila.
        while self._io_ready:
            self._ready.append(self._io_ready.popleft())

        end_time = self.time() + self._clock_resolution
        while self._scheduled:
            handle = self._scheduled[0]
            if handle._when >= end_time:
                break
            handle = heapq.heappop(self._scheduled)
            handle._scheduled = False
            self._ready.append(handle)

        ntodo = len(self._ready)
        for i in range(ntodo):
            handle = self._ready.popleft()
            if handle._cancelled:
                continue
            handle._run()
        handle = None


def _run_until_complete_cb(fut):
    if not fut.cancelled():
        exc = fut.exception()
        if isinstance(exc, (SystemExit, KeyboardInterrupt)):
            # Deixa a exceção subir pelo `run_forever`.
            return
    futures._get_loop(fut).stop()
