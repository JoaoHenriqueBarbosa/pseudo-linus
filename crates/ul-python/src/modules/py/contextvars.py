"""contextvars do sandbox: variáveis de contexto de um fluxo só (sem asyncio nem threads reais)."""

__all__ = ('Context', 'ContextVar', 'Token', 'copy_context')

from types import GenericAlias as _GenericAlias

_MISSING = object()


class Token:
    MISSING = _MISSING

    __class_getitem__ = classmethod(_GenericAlias)

    def __init__(self, var, old_value):
        self.var = var
        self.old_value = old_value
        self._used = False

    def __repr__(self):
        return f'<Token var={self.var!r}>'


class ContextVar:
    def __init__(self, name, *, default=_MISSING):
        if not isinstance(name, str):
            raise TypeError('context variable name must be a str')
        self._name = name
        self._default = default
        self._value = _MISSING

    @property
    def name(self):
        return self._name

    def get(self, *args):
        if len(args) > 1:
            raise TypeError(f'get() takes at most 1 positional argument ({len(args)} given)')
        if self._value is not _MISSING:
            return self._value
        if args:
            return args[0]
        if self._default is not _MISSING:
            return self._default
        raise LookupError(self)

    def set(self, value):
        token = Token(self, self._value)
        self._value = value
        return token

    def reset(self, token):
        if token._used:
            raise RuntimeError('<Token> has already been used once')
        if token.var is not self:
            raise ValueError(f'<Token> was created by a different ContextVar')
        token._used = True
        self._value = token.old_value

    def __repr__(self):
        return f'<ContextVar name={self._name!r}>'

    def __hash__(self):
        return id(self)

    __class_getitem__ = classmethod(_GenericAlias)


class Context:
    def run(self, callable, *args, **kwargs):
        return callable(*args, **kwargs)

    def copy(self):
        return Context()


def copy_context():
    return Context()


del _GenericAlias
