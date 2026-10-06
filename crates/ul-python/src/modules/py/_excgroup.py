"""BaseExceptionGroup, ExceptionGroup e o auxiliar `_eg_split` da instrução `except*`."""


class BaseExceptionGroup(BaseException):

    def __new__(cls, message, exceptions):
        if not isinstance(message, str):
            raise TypeError('argument 1 must be str, not %s' % type(message).__name__)
        if not isinstance(exceptions, (list, tuple)) or isinstance(exceptions, str):
            raise TypeError('second argument (exceptions) must be a sequence')
        exceptions = tuple(exceptions)
        if not exceptions:
            raise ValueError('second argument (exceptions) must be a non-empty sequence')
        for i, e in enumerate(exceptions):
            if not isinstance(e, BaseException):
                raise ValueError('Item %d of second argument (exceptions) is not an exception' % i)
        if cls is BaseExceptionGroup:
            if all(isinstance(e, Exception) for e in exceptions):
                cls = ExceptionGroup
        elif issubclass(cls, Exception):
            for e in exceptions:
                if not isinstance(e, Exception):
                    if cls is ExceptionGroup:
                        raise TypeError('Cannot nest BaseExceptions in an ExceptionGroup')
                    raise TypeError('Cannot nest BaseExceptions in %r' % cls.__name__)
        self = object.__new__(cls)
        self._message = message
        self._exceptions = exceptions
        return self

    def __init__(self, message, exceptions):
        BaseException.__init__(self, message, exceptions)

    @property
    def message(self):
        return self._message

    @property
    def exceptions(self):
        return self._exceptions

    def __str__(self):
        n = len(self._exceptions)
        return '%s (%d sub-exception%s)' % (self._message, n, '' if n == 1 else 's')

    def __repr__(self):
        return '%s(%r, %r)' % (type(self).__name__, self._message, list(self._exceptions))

    def derive(self, excs):
        return BaseExceptionGroup(self._message, excs)

    def _matcher(self, condition):
        if isinstance(condition, type) and issubclass(condition, BaseException):
            return lambda e: isinstance(e, condition)
        if isinstance(condition, tuple):
            return lambda e: isinstance(e, condition)
        if callable(condition):
            return condition
        raise TypeError('expected an exception type, a tuple of exception types, or a callable (other than a class)')

    def split(self, condition):
        match = self._matcher(condition)
        if match(self):
            return self, None
        keep, rest = [], []
        for e in self._exceptions:
            if isinstance(e, BaseExceptionGroup):
                m, r = e.split(condition)
            elif match(e):
                m, r = e, None
            else:
                m, r = None, e
            if m is not None:
                keep.append(m)
            if r is not None:
                rest.append(r)
        return self._copy_with(keep), self._copy_with(rest)

    def subgroup(self, condition):
        return self.split(condition)[0]

    def _copy_with(self, excs):
        if not excs:
            return None
        new = self.derive(excs)
        for attr in ('__traceback__', '__cause__', '__context__'):
            try:
                setattr(new, attr, getattr(self, attr))
            except AttributeError:
                pass
        return new


class ExceptionGroup(BaseExceptionGroup, Exception):
    pass


def _eg_split(exc, types):
    """`except* types`: `(parte que casa, resto)`; uma exceção avulsa que casa vira grupo de um."""
    if isinstance(exc, BaseExceptionGroup):
        return exc.split(types)
    if isinstance(exc, types):
        return BaseExceptionGroup('', [exc]), None
    return None, exc
