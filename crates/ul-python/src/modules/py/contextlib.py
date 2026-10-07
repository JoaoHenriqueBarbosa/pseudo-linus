"""contextlib do sandbox (Python embutido)."""

import sys


class AbstractContextManager:
    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc_value, traceback):
        return None


class AbstractAsyncContextManager:
    async def __aenter__(self):
        return self

    async def __aexit__(self, exc_type, exc_value, traceback):
        return None

class ContextDecorator:
    def __call__(self, func):
        def inner(*args, **kwds):
            with self._recreate_cm():
                return func(*args, **kwds)
        return inner

    def _recreate_cm(self):
        return self


class _GeneratorContextManager(ContextDecorator):
    def __init__(self, func, args, kwds):
        self.gen = func(*args, **kwds)
        self.func, self.args, self.kwds = func, args, kwds

    def _recreate_cm(self):
        return self.__class__(self.func, self.args, self.kwds)

    def __enter__(self):
        try:
            return next(self.gen)
        except StopIteration:
            raise RuntimeError("generator didn't yield") from None

    def __exit__(self, typ, value, traceback):
        if typ is None:
            try:
                next(self.gen)
            except StopIteration:
                return False
            else:
                raise RuntimeError("generator didn't stop")
        else:
            if value is None:
                value = typ()
            try:
                self.gen.throw(value)
            except StopIteration as exc:
                return exc is not value
            except BaseException as exc:
                if exc is not value:
                    raise
                exc.__traceback__ = traceback
                return False
            raise RuntimeError("generator didn't stop after throw()")


def contextmanager(func):
    def helper(*args, **kwds):
        return _GeneratorContextManager(func, args, kwds)
    helper.__name__ = func.__name__
    helper.__doc__ = func.__doc__
    return helper


class closing:
    def __init__(self, thing):
        self.thing = thing

    def __enter__(self):
        return self.thing

    def __exit__(self, *exc_info):
        self.thing.close()


class nullcontext:
    def __init__(self, enter_result=None):
        self.enter_result = enter_result

    def __enter__(self):
        return self.enter_result

    def __exit__(self, *excinfo):
        pass


class suppress:
    def __init__(self, *exceptions):
        self._exceptions = exceptions

    def __enter__(self):
        pass

    def __exit__(self, exctype, excinst, exctb):
        return exctype is not None and issubclass(exctype, self._exceptions)


class redirect_stdout:
    _stream = 'stdout'

    def __init__(self, new_target):
        self._new_target = new_target
        self._old_targets = []

    def __enter__(self):
        self._old_targets.append(getattr(sys, self._stream))
        setattr(sys, self._stream, self._new_target)
        return self._new_target

    def __exit__(self, exctype, excinst, exctb):
        setattr(sys, self._stream, self._old_targets.pop())


class redirect_stderr(redirect_stdout):
    _stream = 'stderr'


class ExitStack:
    def __init__(self):
        self._exit_callbacks = []

    def enter_context(self, cm):
        result = type(cm).__enter__(cm)
        self._exit_callbacks.append((True, type(cm).__exit__, cm))
        return result

    def push(self, exit):
        self._exit_callbacks.append((False, exit, None))
        return exit

    def callback(self, callback, *args, **kwds):
        def _exit_wrapper(exc_type, exc, tb):
            callback(*args, **kwds)
        self._exit_callbacks.append((False, _exit_wrapper, None))
        return callback

    def pop_all(self):
        new_stack = ExitStack()
        new_stack._exit_callbacks = self._exit_callbacks
        self._exit_callbacks = []
        return new_stack

    def close(self):
        self.__exit__(None, None, None)

    def __enter__(self):
        return self

    def __exit__(self, *exc_details):
        received_exc = exc_details[0] is not None
        suppressed_exc = False
        pending_raise = None
        while self._exit_callbacks:
            is_sync, cb, cm = self._exit_callbacks.pop()
            try:
                if cm is not None:
                    suppress_now = cb(cm, *exc_details)
                else:
                    suppress_now = cb(*exc_details)
                if suppress_now:
                    suppressed_exc = True
                    pending_raise = None
                    exc_details = (None, None, None)
            except BaseException as new_exc:
                pending_raise = new_exc
                exc_details = (type(new_exc), new_exc, None)
        if pending_raise is not None:
            raise pending_raise
        return received_exc and suppressed_exc
