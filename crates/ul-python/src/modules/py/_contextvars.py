"""O `_contextvars` do CPython 3.13 (`Python/context.c`): `Context`, `ContextVar`, `Token` e `copy_context`.

Cada thread (a thread verde de `_gsched`) tem o seu contexto corrente, criado vazio na primeira consulta, como o
`ts->context` do C; uma thread nova começa sem contexto (o 3.13 não copia o da thread que a criou). O `Context` é um
mapa imutável por fora: o `ContextVar.set` troca o mapa do contexto corrente por uma cópia com a mudança, então um
`copy()` feito antes não enxerga o que veio depois. O greenlet troca o contexto corrente da thread a cada switch
(`gr_context`).

O módulo é C no CPython: as funções são `builtin_function_or_method` (`C_EXTENSION_MODULES`) e os três tipos são
estáticos e não aceitam subclasse.
"""

import _gsched
from types import GenericAlias as _GenericAlias

_getattr = object.__getattribute__
_setattr = object.__setattr__


def _current_context():
    """O contexto corrente da thread que roda; o primeiro nasce vazio."""
    green = _gsched.current()
    ctx = green.ctx
    if ctx is None:
        ctx = _new_context({})
        green.ctx = ctx
    return ctx


def _set_current(ctx):
    _gsched.current().ctx = ctx


def _new_context(vars):
    ctx = object.__new__(Context)
    _setattr(ctx, '_Context__vars', vars)
    _setattr(ctx, '_Context__entered', False)
    _setattr(ctx, '_Context__prev', None)
    return ctx


def _vars(ctx):
    return _getattr(ctx, '_Context__vars')


def _no_subclass(name):
    def __init_subclass__(cls, **kwargs):
        raise TypeError("type '_contextvars.%s' is not an acceptable base type" % name)
    return classmethod(__init_subclass__)


def _check_key(key):
    if type(key) is not ContextVar:
        raise TypeError('a ContextVar key was expected, got %r' % (key,))


def _member(slot):
    """Um campo `READONLY` do `PyMemberDef`: lê o slot; atribuir ou apagar dá `readonly attribute`."""
    def get(self):
        return _getattr(self, slot)

    def readonly(self, *value):
        raise AttributeError('readonly attribute')
    return property(get, readonly, readonly)


class _Missing:
    __slots__ = ()

    def __repr__(self):
        return '<Token.MISSING>'


_Missing.__name__ = _Missing.__qualname__ = 'MISSING'
_Missing.__module__ = 'Token'
_MISSING = object.__new__(_Missing)


class Context:
    __slots__ = ('__vars', '__entered', '__prev')

    def __new__(cls, *args, **kwargs):
        if args or kwargs:
            raise TypeError('Context() does not accept any arguments')
        return _new_context({})

    def run(self, callable, /, *args, **kwargs):
        if _getattr(self, '_Context__entered'):
            raise RuntimeError('cannot enter context: %r is already entered' % (self,))
        _setattr(self, '_Context__prev', _gsched.current().ctx)
        _setattr(self, '_Context__entered', True)
        _set_current(self)
        try:
            return callable(*args, **kwargs)
        finally:
            _set_current(_getattr(self, '_Context__prev'))
            _setattr(self, '_Context__prev', None)
            _setattr(self, '_Context__entered', False)

    def copy(self):
        return _new_context(dict(_vars(self)))

    def get(self, key, default=None, /):
        _check_key(key)
        return _vars(self).get(key, default)

    def __getitem__(self, key):
        _check_key(key)
        try:
            return _vars(self)[key]
        except KeyError:
            raise KeyError(key) from None

    def __contains__(self, key):
        _check_key(key)
        return key in _vars(self)

    def __len__(self):
        return len(_vars(self))

    def __iter__(self):
        return iter(list(_vars(self)))

    def keys(self):
        return list(_vars(self)).__iter__()

    def values(self):
        return list(_vars(self).values()).__iter__()

    def items(self):
        return list(_vars(self).items()).__iter__()

    def __eq__(self, other):
        if type(other) is not Context:
            return NotImplemented
        return _vars(self) == _vars(other)

    __hash__ = None
    __init_subclass__ = _no_subclass('Context')


class ContextVar:
    __slots__ = ('__name', '__default')

    def __new__(cls, *args, default=_MISSING):
        if len(args) != 1:
            raise TypeError('ContextVar() takes exactly 1 positional argument (%d given)' % len(args))
        name = args[0]
        if type(name) is not str:
            raise TypeError('context variable name must be a str')
        var = object.__new__(cls)
        _setattr(var, '_ContextVar__name', name)
        _setattr(var, '_ContextVar__default', default)
        return var

    name = _member('_ContextVar__name')

    def get(self, *args):
        if len(args) > 1:
            raise TypeError('get expected at most 1 argument, got %d' % len(args))
        # Como o `PyContextVar_Get`: sem contexto na thread, ler não cria um (quem cria é o `set`).
        ctx = _gsched.current().ctx
        vars = _vars(ctx) if ctx is not None else {}
        if self in vars:
            return vars[self]
        if args:
            return args[0]
        default = _getattr(self, '_ContextVar__default')
        if default is not _MISSING:
            return default
        raise LookupError(self)

    def set(self, value, /):
        ctx = _current_context()
        vars = _vars(ctx)
        old = vars.get(self, _MISSING)
        fresh = dict(vars)
        fresh[self] = value
        _setattr(ctx, '_Context__vars', fresh)
        return _new_token(ctx, self, old)

    def reset(self, token, /):
        if type(token) is not Token:
            raise TypeError('expected an instance of Token, got %r' % (token,))
        if _getattr(token, '_Token__used'):
            raise RuntimeError('%r has already been used once' % (token,))
        if _getattr(token, '_Token__var') is not self:
            raise ValueError('%r was created by a different ContextVar' % (token,))
        ctx = _current_context()
        if _getattr(token, '_Token__ctx') is not ctx:
            raise ValueError('%r was created in a different Context' % (token,))
        _setattr(token, '_Token__used', True)
        fresh = dict(_vars(ctx))
        old = _getattr(token, '_Token__old')
        if old is _MISSING:
            fresh.pop(self, None)
        else:
            fresh[self] = old
        _setattr(ctx, '_Context__vars', fresh)

    def __repr__(self):
        default = _getattr(self, '_ContextVar__default')
        shown = '' if default is _MISSING else ' default=%r' % (default,)
        return '<ContextVar name=%r%s at 0x%x>' % (_getattr(self, '_ContextVar__name'), shown, id(self))

    __class_getitem__ = classmethod(_GenericAlias)
    __init_subclass__ = _no_subclass('ContextVar')


def _new_token(ctx, var, old):
    token = object.__new__(Token)
    _setattr(token, '_Token__ctx', ctx)
    _setattr(token, '_Token__var', var)
    _setattr(token, '_Token__old', old)
    _setattr(token, '_Token__used', False)
    return token


class Token:
    __slots__ = ('__ctx', '__var', '__old', '__used')

    MISSING = _MISSING

    def __new__(cls, *args, **kwargs):
        raise RuntimeError('Tokens can only be created by ContextVars')

    var = _member('_Token__var')
    old_value = _member('_Token__old')

    def __repr__(self):
        used = ' used' if _getattr(self, '_Token__used') else ''
        return '<Token%s var=%r at 0x%x>' % (used, _getattr(self, '_Token__var'), id(self))

    __class_getitem__ = classmethod(_GenericAlias)
    __init_subclass__ = _no_subclass('Token')


def copy_context():
    return _current_context().copy()


del _GenericAlias
__doc__ = 'Context Variables'
