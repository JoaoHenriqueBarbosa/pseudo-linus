"""concurrent.futures: executores que rodam cada tarefa até o fim no `submit` (um fluxo só; ver
`threading`). O `Future` devolvido já está concluído, com resultado ou exceção."""

import threading as _threading

__all__ = ['FIRST_COMPLETED', 'FIRST_EXCEPTION', 'ALL_COMPLETED', 'CancelledError', 'TimeoutError', 'Future',
           'Executor', 'wait', 'as_completed', 'ThreadPoolExecutor', 'ProcessPoolExecutor', 'BrokenExecutor']

FIRST_COMPLETED = 'FIRST_COMPLETED'
FIRST_EXCEPTION = 'FIRST_EXCEPTION'
ALL_COMPLETED = 'ALL_COMPLETED'

PENDING = 'PENDING'
RUNNING = 'RUNNING'
CANCELLED = 'CANCELLED'
FINISHED = 'FINISHED'


class Error(Exception):
    pass


class CancelledError(Error):
    pass


TimeoutError = TimeoutError


class InvalidStateError(Error):
    pass


class BrokenExecutor(RuntimeError):
    pass


class Future:
    def __init__(self):
        self._state = PENDING
        self._result = None
        self._exception = None
        self._callbacks = []

    def __repr__(self):
        if self._state == FINISHED:
            if self._exception:
                detail = 'finished raised %s' % self._exception.__class__.__name__
            else:
                detail = 'finished returned %s' % self._result.__class__.__name__
        else:
            detail = self._state.lower()
        return '<Future at %#x state=%s>' % (id(self), detail)

    def cancel(self):
        if self._state in (RUNNING, FINISHED):
            return False
        if self._state == CANCELLED:
            return True
        self._state = CANCELLED
        self._invoke_callbacks()
        return True

    def cancelled(self):
        return self._state == CANCELLED

    def running(self):
        return self._state == RUNNING

    def done(self):
        return self._state in (CANCELLED, FINISHED)

    def _report(self):
        if self._state == CANCELLED:
            raise CancelledError()
        if self._exception is not None:
            raise self._exception
        return self._result

    def result(self, timeout=None):
        if self._state == PENDING or self._state == RUNNING:
            raise TimeoutError()
        return self._report()

    def exception(self, timeout=None):
        if self._state == CANCELLED:
            raise CancelledError()
        if self._state != FINISHED:
            raise TimeoutError()
        return self._exception

    def add_done_callback(self, fn):
        if self._state in (CANCELLED, FINISHED):
            self._call(fn)
        else:
            self._callbacks.append(fn)

    def _call(self, fn):
        try:
            fn(self)
        except Exception:
            import logging
            logging.getLogger('concurrent.futures').exception('exception calling callback for %r', self)

    def _invoke_callbacks(self):
        for callback in self._callbacks:
            self._call(callback)
        self._callbacks = []

    def set_running_or_notify_cancel(self):
        if self._state == CANCELLED:
            return False
        if self._state == PENDING:
            self._state = RUNNING
            return True
        raise RuntimeError('Future in unexpected state')

    def set_result(self, result):
        if self._state in (CANCELLED, FINISHED):
            raise InvalidStateError('{}: {!r}'.format(self._state, self))
        self._result = result
        self._state = FINISHED
        self._invoke_callbacks()

    def set_exception(self, exception):
        if self._state in (CANCELLED, FINISHED):
            raise InvalidStateError('{}: {!r}'.format(self._state, self))
        self._exception = exception
        self._state = FINISHED
        self._invoke_callbacks()


class Executor:
    def submit(self, fn, /, *args, **kwargs):
        raise NotImplementedError()

    def map(self, fn, *iterables, timeout=None, chunksize=1):
        futures = [self.submit(fn, *args) for args in zip(*iterables)]

        def results():
            for f in futures:
                yield f.result()
        return results()

    def shutdown(self, wait=True, *, cancel_futures=False):
        pass

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        self.shutdown(wait=True)
        return False


class ThreadPoolExecutor(Executor):
    def __init__(self, max_workers=None, thread_name_prefix='', initializer=None, initargs=()):
        if max_workers is not None and max_workers <= 0:
            raise ValueError('max_workers must be greater than 0')
        self._max_workers = max_workers if max_workers is not None else 8
        self._shutdown = False
        self._prefix = thread_name_prefix
        self._initializer = initializer
        self._initargs = initargs
        self._initialized = False

    def submit(self, fn, /, *args, **kwargs):
        if self._shutdown:
            raise RuntimeError('cannot schedule new futures after shutdown')
        if self._initializer is not None and not self._initialized:
            self._initialized = True
            self._initializer(*self._initargs)
        f = Future()
        if not f.set_running_or_notify_cancel():
            return f
        try:
            result = fn(*args, **kwargs)
        except BaseException as exc:
            f.set_exception(exc)
        else:
            f.set_result(result)
        return f

    def shutdown(self, wait=True, *, cancel_futures=False):
        self._shutdown = True


class ProcessPoolExecutor(ThreadPoolExecutor):
    pass


def wait(fs, timeout=None, return_when=ALL_COMPLETED):
    fs = set(fs)
    done = {f for f in fs if f.done()}
    return (done, fs - done)


def as_completed(fs, timeout=None):
    fs = list(fs)
    for f in fs:
        if f.done():
            yield f
